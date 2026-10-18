use super::*;
use crate::adv::gerokasu2::save_commands::SaveSession;
use shin_core::{
    format::{save::gerokasu2::AutosaveMemory, scenario::gerokasu2::Gerokasu2Scenario},
    vm::gerokasu2::Gerokasu2Scripter,
};
use std::sync::Arc;
fn vm() -> Gerokasu2Scripter {
    let mut d = b"SNR ".to_vec();
    for w in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        d.extend(w.to_le_bytes());
    }
    let mut t = Vec::new();
    for i in 0..10 {
        d.extend((0x4c + t.len() as u32).to_le_bytes());
        if i < 7 {
            t.extend(4u32.to_le_bytes());
        }
        t.extend(0u32.to_le_bytes());
    }
    d.extend(t);
    let start = d.len() as u32;
    d[32..36].copy_from_slice(&start.to_le_bytes());
    d.extend([0, 0, 0]);
    let len = d.len() as u32;
    d[4..8].copy_from_slice(&len.to_le_bytes());
    let mut vm = Gerokasu2Scripter::new(Arc::new(Gerokasu2Scenario::new(d.into()).unwrap()), 0, 42);
    vm.run(shin_core::vm::command::CommandResult::None, 32).unwrap();
    vm
}
struct Services {
    save: SaveSession,
    kind: u32,
    value: i32,
    ready: bool,
    installs: u32,
    captures: u32,
    fail_publish: bool,
}
impl QuizServices for Services {
    fn bind_quiz(&mut self, _: Option<(PlaneId, LayerbankId)>) -> Result<()> {
        self.installs += (self.kind == 1) as u32;
        Ok(())
    }
    fn quiz_result(&mut self) -> Result<Option<i32>> {
        Ok(if self.kind != 1 {
            Some(0)
        } else {
            self.ready.then_some(self.value)
        })
    }
    fn finish_quiz(&mut self) {}
}
impl SaveServices for Services {
    fn save_session(&mut self) -> &mut SaveSession {
        &mut self.save
    }
    fn packed_time(&mut self) -> Result<u64> {
        Ok(0)
    }
    fn autosave_captured(&mut self, _: &AutosaveMemory) -> Result<()> {
        if self.fail_publish {
            self.fail_publish = false;
            anyhow::bail!("publish failure");
        }
        self.captures += 1;
        Ok(())
    }
    fn show_scene_title(&mut self, _: &str) -> Result<()> {
        Ok(())
    }
}
fn snapshot(s: &Services, layers: &Gerokasu2Layers, done: bool, value: i32) -> Vec<u8> {
    let t = s.save.timeline;
    let bytes = &s.save.progress.active_entry().responses;
    let record = layers.record(LayerbankId::new(5));
    let mut out = b"GQZ1".to_vec();
    for w in [
        done as u32,
        s.save.checkpoints.len() as u32,
        s.captures,
        s.installs,
        t.boundary,
        t.current_interaction,
        t.last_skip,
        s.save.response_bit_position as u32,
        done as u32,
        value as u32,
        if s.kind == 0 {
            0
        } else {
            record.interaction_completed() as u32
        },
        if s.kind == 0 {
            0
        } else {
            record.interaction_result() as u32
        },
        bytes.len() as u32,
    ] {
        out.extend(w.to_le_bytes());
    }
    out.extend(bytes);
    out
}
#[test]
#[ignore = "original QUIZ ctor/live,route writer;CPU only"]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn ger_native_quiz_ctor_live_boundaries_result_and_six_bit_route() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/card-select");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-quiz");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-quiz.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let vm = vm();
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    for has_text in [false, true] {
        for boundary in [0, 1, 99, u32::MAX] {
            for kind in [0, 1, 2] {
                for value in [0, 1, 2, 3, 63, 64, -1, i32::MIN, i32::MAX] {
                    let mut s = Services {
                        save: SaveSession::fresh(0, 42),
                        kind,
                        value,
                        ready: false,
                        installs: 0,
                        captures: 0,
                        fail_publish: false,
                    };
                    let mut layers = Gerokasu2Layers::default();
                    s.save.timeline.boundary = boundary;
                    s.save.interactive(&vm, &layers, has_text).unwrap();
                    let target = (kind != 0).then_some((PlaneId::new(2), LayerbankId::new(5)));
                    let mut cmd = QuizCommand::new(target);
                    requests.extend(
                        [0, has_text as u32, boundary, kind, value as u32]
                            .into_iter()
                            .flat_map(u32::to_le_bytes),
                    );
                    expected.push(snapshot(&s, &layers, false, 0));
                    for ready in if kind == 1 {
                        vec![false, false, true]
                    } else {
                        vec![false]
                    } {
                        s.ready = ready;
                        cmd.bind(&mut s).unwrap();
                        cmd.capture_before(&mut s).unwrap();
                        let result = cmd.poll(&mut s).unwrap();
                        if let Some(v) = result {
                            cmd.capture_after(v, &mut layers, &mut s).unwrap();
                        }
                        requests.extend(
                            [1, ready as u32, 0, 0, 0]
                                .into_iter()
                                .flat_map(u32::to_le_bytes),
                        );
                        expected.push(snapshot(&s, &layers, result.is_some(), result.unwrap_or(0)));
                    }
                }
            }
        }
    }
    let input = folder.join("quiz-request.bin");
    fs::write(&input, requests).unwrap();
    let run = Command::new(binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("quiz-native.trace"), &run.stdout).unwrap();
    fs::write(folder.join("quiz-native.stderr"), &run.stderr).unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let mut at = 0;
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(&run.stdout[at..at + e.len()], e, "quiz snapshot{i}");
        at += e.len();
    }
    assert_eq!(at, run.stdout.len());
    println!(
        "GEROKASU2_QUIZ_MODEL_NATIVE snapshots={} compared_bytes={} ctor_live_ready_missing_noninteractive_6bit_route=verified",
        expected.len(),
        at
    );
}

#[test]
fn ger_quiz_capture_retry_records_once_and_marks_bank_after_publication() {
    let mut s = Services {
        save: SaveSession::fresh(0, 42),
        kind: 1,
        value: 3,
        ready: true,
        installs: 0,
        captures: 0,
        fail_publish: false,
    };
    let bank = LayerbankId::new(5);
    let mut layers = Gerokasu2Layers::default();
    let mut cmd = QuizCommand::new(Some((PlaneId::new(2), bank)));
    cmd.bind(&mut s).unwrap();
    cmd.capture_before(&mut s).unwrap();
    assert_eq!(cmd.poll(&mut s).unwrap(), Some(3));
    s.fail_publish = true;
    assert!(cmd.capture_after(3, &mut layers, &mut s).is_err());
    assert!(!layers.record(bank).interaction_completed());
    assert_eq!(s.save.timeline.boundary, 1);
    assert_eq!(s.save.response_bit_position, 6);
    cmd.capture_after(3, &mut layers, &mut s).unwrap();
    assert!(layers.record(bank).interaction_completed());
    assert_eq!(layers.record(bank).interaction_result(), 3);
    cmd.capture_after(3, &mut layers, &mut s).unwrap();
    assert_eq!(s.save.timeline.boundary, 1);
    assert_eq!(s.save.response_bit_position, 6);
    assert_eq!(s.captures, 2);
}
