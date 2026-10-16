use super::*;
fn active(mode: Mode, slot: u8, valid: bool) -> Controller {
    let mut availability = [false; SLOT_COUNT];
    availability[usize::from(slot)] = valid;
    let mut m = Controller::new_with_mode(mode, slot, availability).unwrap();
    m.assets_ready();
    m.update(10.);
    m
}
#[test]
fn save_delete_wait_for_both_question_and_write_and_restore_failed_availability() {
    for valid in [false, true] {
        let mut m = active(Mode::Save, 37, valid);
        m.confirm();
        assert_eq!(m.take_sounds(), ["decide"]);
        m.update(7.);
        m.update(0.);
        assert_eq!(
            m.take_question(),
            Some(QuestionRequest {
                slot: 37,
                kind: QuestionKind::Save { overwrite: valid }
            })
        );
        m.dialog_response(false, 0);
        assert_eq!(
            m.take_storage_requests(),
            [StorageRequest::Save { slot: 37 }]
        );
        assert!(m.available(37));
        assert_eq!(m.phase(), Phase::Writing);
        m.write_finished(false);
        assert_eq!(m.available(37), valid);
        assert!(!m.accepts_input());
        m.dialog_response(true, 0);
        assert!(m.accepts_input());
    }
    let mut m = active(Mode::Save, 99, true);
    m.delete();
    m.update(7.);
    m.update(0.);
    assert_eq!(m.take_question().unwrap().kind, QuestionKind::Delete);
    m.dialog_response(false, 0);
    assert_eq!(
        m.take_storage_requests(),
        [StorageRequest::Delete { slot: 99 }]
    );
    assert!(!m.available(99));
    m.dialog_response(true, 0);
    assert!(!m.accepts_input());
    m.write_finished(true);
    assert!(m.accepts_input());
    m.write_finished(false);
    assert!(!m.available(99));
    assert_eq!(m.update(100.), None);
}

#[test]
#[ignore = "original Ger Save/Delete stage0 mutation/rollback CPU oracle"]
fn ger_native_save_delete_questions_write_completion_and_rollback() {
    use std::{
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };
    let root = std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let oracle = std::env::var_os("SHIN_GEROKASU2_SAVE_WRITE_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("gerokasu2/.local/save-menu-write/native-write"));
    struct Host {
        remembered: u8,
        before: u8,
        payload: u32,
        requests: u32,
        pending: bool,
        question: u32,
        clocks: u32,
    }
    impl Host {
        fn drain(&mut self, m: &mut Controller) {
            for effect in m.take_storage_requests() {
                match effect {
                    StorageRequest::Remember(slot) => self.remembered = slot,
                    StorageRequest::Save { slot } => {
                        self.remembered = slot;
                        self.payload = 777;
                        self.requests += 1;
                        self.pending = true;
                        self.clocks += 1;
                    }
                    StorageRequest::Delete { .. } => {
                        self.requests += 1;
                        self.pending = true;
                    }
                }
            }
            if let Some(q) = m.take_question() {
                self.question = match q.kind {
                    QuestionKind::Save { overwrite: false } => 1,
                    QuestionKind::Save { overwrite: true } => 2,
                    QuestionKind::Delete => 3,
                    QuestionKind::Load => 4,
                };
            }
        }
        fn row(&mut self, m: &mut Controller) -> Vec<u64> {
            self.drain(m);
            let cues = m.take_sounds();
            let mut row = vec![
                m.mode() as u64,
                m.selected() as u64,
                u64::from(self.remembered),
                u64::from(m.available(m.selected())),
                u64::from(self.payload),
                u64::from(self.requests),
                u64::from(self.pending),
                u64::from(self.question),
                u64::from(self.clocks),
                u64::from(m.flash.value().to_bits()),
            ];
            row.extend(
                ["move", "decide", "cancel", "error"]
                    .map(|s| cues.iter().filter(|&&v| v == s).count() as u64),
            );
            row
        }
    }
    let (mut script, mut expected, mut profiles) = (String::new(), vec![], 0);
    for mode in [Mode::Save, Mode::Load] {
        for slot in [0, 1, 37, 99] {
            for valid in [false, true] {
                for delete in [false, true] {
                    for answer in [-1, 0, 1] {
                        for success in [false, true] {
                            for final_first in [false, true] {
                                for delta in [1., 2.5, 1000.] {
                                    profiles += 1;
                                    let mut m = active(mode, slot, valid);
                                    let mut host = Host {
                                        remembered: 99 - slot,
                                        before: 99 - slot,
                                        payload: 77,
                                        requests: 0,
                                        pending: false,
                                        question: 0,
                                        clocks: 0,
                                    };
                                    script += &format!(
                                        "N {} {slot} {} {}\n",
                                        mode as u8,
                                        u8::from(valid),
                                        host.remembered
                                    );
                                    expected.push(host.row(&mut m));
                                    script += &format!("A {}\n", u8::from(delete));
                                    if delete {
                                        m.delete()
                                    } else {
                                        m.confirm()
                                    };
                                    expected.push(host.row(&mut m));
                                    for _ in 0..(7f32 / delta).ceil() as usize {
                                        script += &format!("F {delta}\n");
                                        m.update(delta);
                                        expected.push(host.row(&mut m));
                                    }
                                    script += "F 0\n";
                                    m.update(0.);
                                    expected.push(host.row(&mut m));
                                    if host.question != 0 {
                                        script += &format!("Q 0 {answer}\n");
                                        m.dialog_response(false, answer);
                                        expected.push(host.row(&mut m));
                                        if final_first || !host.pending {
                                            script += &format!("Q 1 {answer}\n");
                                            m.dialog_response(true, answer);
                                            expected.push(host.row(&mut m));
                                        }
                                        if host.pending {
                                            script += &format!("W {}\n", u8::from(success));
                                            host.pending = false;
                                            if !success && !delete {
                                                host.payload = 77;
                                                host.remembered = host.before;
                                            }
                                            m.write_finished(success);
                                            expected.push(host.row(&mut m));
                                            if !final_first {
                                                script += &format!("Q 1 {answer}\n");
                                                m.dialog_response(true, answer);
                                                expected.push(host.row(&mut m));
                                            }
                                        }
                                    }
                                    script += "X\n";
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut child = Command::new(oracle)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(script.as_bytes());
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "native Save/Delete {}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    let trace = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = trace.lines().filter(|s| s.starts_with("W ")).collect();
    assert_eq!(rows.len(), expected.len());
    for (i, (actual, expected)) in rows.iter().zip(expected).enumerate() {
        let got: Vec<u64> = actual
            .split_whitespace()
            .skip(2)
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(got, expected, "Save/Delete sample{i}:{actual}");
    }
    println!(
        "GER_SAVE_DELETE_MATCH profiles={profiles} exact_snapshots={} originalSourceCopy777vs999/stage0/YesNoCancel/writeSuccessFailure/rollback/remembered/flash/questions",
        rows.len()
    );
}
