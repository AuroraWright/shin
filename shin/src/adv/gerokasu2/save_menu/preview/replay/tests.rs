use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn ger_preview_replay_pageback_suppresses_messages_without_reforking() {
    let mut c = Controller::new(2, vec![]);
    c.apply(Callback::PageBack).unwrap();
    c.apply(Callback::MessageSet { auto_wait: true }).unwrap();
    assert_eq!((c.boundary, c.ready), (0, false));
    c.apply(Callback::Wipe).unwrap();
    c.apply(Callback::MessageSet { auto_wait: true }).unwrap();
    assert_eq!((c.boundary, c.ready), (1, false));
    c.apply(Callback::EventBegin { event: 7 }).unwrap();
    assert_eq!((c.boundary, c.ready, c.event), (2, true, 7));
}

fn num(code: &mut Vec<u8>, value: i32) {
    if (-64..64).contains(&value) {
        code.push(value as u8 & 0x7f)
    } else if (-2048..2048).contains(&value) {
        code.extend([0x80 | ((value >> 8) & 15) as u8, value as u8])
    } else {
        panic!("fixture number")
    }
}
fn string(code: &mut Vec<u8>, value: &str) {
    code.extend(((value.len() + 1) as u16).to_le_bytes());
    code.extend(value.as_bytes());
    code.push(0)
}

struct Program {
    code: Vec<u8>,
    operations: Vec<(u8, Callback<'static>)>,
}
impl Program {
    fn new() -> Self {
        Self {
            code: vec![0; 16],
            operations: vec![],
        }
    }
    fn add(&mut self, opcode: u8, callback: Callback<'static>) {
        self.code.push(opcode);
        self.operations.push((opcode, callback));
        match callback {
            Callback::SaveInfo { level, text } => {
                num(&mut self.code, level as i32);
                string(&mut self.code, text)
            }
            Callback::MessageSet { auto_wait } => {
                self.code
                    .extend((1u32 | ((auto_wait as u32) << 24)).to_le_bytes());
                num(&mut self.code, 0);
                string(&mut self.code, "本文")
            }
            Callback::MessageWait { signal } => num(&mut self.code, signal),
            Callback::MessageClose => self.code.push(0),
            Callback::EventBegin { event } => num(&mut self.code, event),
            Callback::Exit { continue_running } => {
                self.code.extend([continue_running as u8, 0]);
            }
            Callback::PersistentGet => {
                self.code.extend(7u16.to_le_bytes());
                num(&mut self.code, 99)
            }
            Callback::Selection => {
                self.code.extend([0, 0, 0, 0, 7, 0]);
                num(&mut self.code, 3);
                string(&mut self.code, "選択");
                self.code.extend(5u16.to_le_bytes());
                self.code.extend(b"A\0B\0\0");
            }
            Callback::Quiz => {
                self.code.extend(8u16.to_le_bytes());
                num(&mut self.code, 9);
            }
            Callback::Wipe => {
                for n in [-1, 0, 30] {
                    num(&mut self.code, n)
                }
                self.code.push(0);
            }
            _ => match opcode {
                0x85 => num(&mut self.code, -1),
                0x82 => {
                    num(&mut self.code, 3);
                    num(&mut self.code, 99)
                }
                0xc9 => {
                    for n in [-1, 0, 30] {
                        num(&mut self.code, n)
                    }
                    self.code.push(0)
                }
                0x83 => {
                    self.code.push(1);
                    num(&mut self.code, 12)
                }
                _ => {}
            },
        }
    }
}

#[test]
#[ignore = "whole original D5A10/D5B80/C11B0 specialized callbacks/export; CPU only"]
fn ger_native_save_preview_replay_callbacks_and_label_export_match() {
    let root = PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROOT").unwrap());
    let out = root.join("gerokasu2/.local/save-preview-replay");
    let oracle = std::env::var_os("SHIN_GEROKASU2_SAVE_PREVIEW_REPLAY_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| out.join("native-save-preview-replay"));
    let mut programs = vec![];
    for asynchronous in [false, true] {
        let mut p = Program::new();
        p.add(
            0xa0,
            Callback::SaveInfo {
                level: 0,
                text: "章",
            },
        );
        p.add(
            0xa0,
            Callback::SaveInfo {
                level: 1,
                text: "第一場面",
            },
        );
        p.add(0xa1, Callback::Autosave);
        p.add(0x81, Callback::PersistentGet);
        p.add(0x82, Callback::None);
        p.add(
            0x86,
            Callback::MessageSet {
                auto_wait: !asynchronous,
            },
        );
        p.add(0x85, Callback::None); // MSGINIT does not alter the preview message marker.
        p.add(0x8a, Callback::MessageClose);
        p.add(0x8d, Callback::Selection);
        p.add(0x87, Callback::MessageWait { signal: -1 });
        p.add(0x8a, Callback::MessageClose);
        p.add(0xe0, Callback::Quiz);
        p.add(0xa0, Callback::SaveInfo { level: 0, text: "" });
        p.add(
            0xa0,
            Callback::SaveInfo {
                level: 1,
                text: "第二場面",
            },
        );
        p.add(0xa2, Callback::EventBegin { event: 7 });
        p.add(0x86, Callback::MessageSet { auto_wait: true });
        p.add(0xa3, Callback::EventEnd);
        p.add(0xa4, Callback::ResumeSet);
        p.add(0xa1, Callback::Autosave);
        p.add(0xa5, Callback::Resume);
        programs.push(p);
    }
    let mut p = Program::new();
    p.add(0xcb, Callback::PageBack);
    p.add(0x86, Callback::MessageSet { auto_wait: true });
    p.add(0xc9, Callback::None);
    p.add(0x86, Callback::MessageSet { auto_wait: false });
    p.add(0x8e, Callback::Wipe);
    p.add(0x86, Callback::MessageSet { auto_wait: true });
    p.add(0x83, Callback::None);
    p.add(
        0xa0,
        Callback::SaveInfo {
            level: 1,
            text: "最後",
        },
    );
    p.add(0xa1, Callback::Autosave);
    p.add(
        0,
        Callback::Exit {
            continue_running: true,
        },
    );
    p.add(0xa2, Callback::EventBegin { event: 1 });
    p.add(0x86, Callback::MessageSet { auto_wait: true });
    p.add(
        0,
        Callback::Exit {
            continue_running: false,
        },
    );
    programs.push(p);
    let mut input = vec![];
    let mut expected = vec![];
    let mut profiles = 0;
    for p in &programs {
        for active in 0..2 {
            for seed in [0, 42, u32::MAX] {
                for target in 0..13 {
                    for route in [vec![], vec![0xa5, 0x5a, 0xff, 0x80]] {
                        for n in [
                            0x31525053,
                            p.code.len() as u32,
                            16,
                            0,
                            seed,
                            target,
                            99,
                            active,
                            route.len() as u32,
                            0,
                        ] {
                            input.extend(n.to_le_bytes())
                        }
                        input.extend(&p.code);
                        input.extend(&route);
                        let mut model = Controller::new(target, route);
                        let mut r7 = 0;
                        let mut r8 = 0;
                        let mut rows = vec![];
                        for &(opcode, callback) in &p.operations {
                            if model.ready {
                                break;
                            }
                            let effect = model.apply(callback).unwrap();
                            if let Some(result) = effect.response {
                                if callback == Callback::Quiz {
                                    r8 = result as u32
                                } else {
                                    r7 = result as u32
                                }
                            }
                            rows.push((opcode, model.clone(), r7, r8));
                        }
                        assert!(model.ready, "fixture must terminate");
                        expected.push((rows, model.saved_labels));
                        profiles += 1;
                    }
                }
            }
        }
    }
    std::fs::write(out.join("corpus.input"), &input).unwrap();
    let mut child = Command::new(oracle)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/system.fnt"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join("native.output"), &result.stdout).unwrap();
    std::fs::write(out.join("native.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let mut lines = output.lines();
    let mut observations = 0;
    let label = |s: &str| {
        let (len, hex) = s.split_once(':').unwrap();
        assert_eq!(len.parse::<usize>().unwrap() * 2, hex.len());
        String::from_utf8(
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect(),
        )
        .unwrap()
    };
    for (case, (rows, saved)) in expected.into_iter().enumerate() {
        assert_eq!(lines.next().unwrap(), format!("N {case}"));
        for (opcode, m, r7, r8) in &rows {
            let line = lines.next().unwrap();
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "C", "native preview case{case}: {line}");
            let n: Vec<u32> = f[1..14].iter().map(|x| x.parse().unwrap()).collect();
            assert_eq!(
                (
                    n[1], n[4], n[5], n[6], n[7], n[8], n[9], n[10], n[11], n[12]
                ),
                (
                    *opcode as u32,
                    m.boundary,
                    m.event as u32,
                    m.ready as u32,
                    m.asynchronous as u32,
                    m.message_present as u32,
                    m.response_bit_position as u32,
                    m.back_started as u32,
                    *r7,
                    *r8
                ),
                "native preview callback case{case}: {line}"
            );
            assert_eq!(
                [label(f[14]), label(f[15]), label(f[16]), label(f[17])],
                [
                    m.current_labels[0].clone(),
                    m.saved_labels[0].clone(),
                    m.current_labels[1].clone(),
                    m.saved_labels[1].clone()
                ],
                "native preview labels case{case}"
            );
            observations += 1;
        }
        assert_eq!(lines.next(), Some("S"));
        for owner in 0..7 {
            assert!(lines.next().unwrap().starts_with(&format!("P {owner} ")))
        }
        assert!(lines.next().unwrap().starts_with("M "));
        let final_line = lines.next().unwrap();
        let f: Vec<_> = final_line.split_whitespace().collect();
        assert_eq!(f[0], "L");
        assert_eq!(
            [label(f[1]), label(f[2])],
            saved,
            "native preview exported labels case{case}"
        );
        assert_eq!(lines.next().unwrap(), format!("X {}", rows.len()));
    }
    assert!(lines.next().is_none());
    println!(
        "GER_SAVE_PREVIEW_REPLAY_MATCH profiles={profiles} callbacks={observations} whole_native_constructor_parser_specialized_callbacks_export_destructor"
    );
}
