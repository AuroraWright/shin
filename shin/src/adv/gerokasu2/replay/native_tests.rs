use super::tests::{message, num, reg, saved, scenario, text};
use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original Ger SELECT/QUIZ/AUTOSAVE/message/event replay polls; CPU only"]
fn ger_native_replay_poll_constructor_route_publication_and_unsigned_retention() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = root.join("reverse-engineering/gerokasu2/replay/native-replay-polls");
    let c = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-replay-polls.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut snapshots = 0;
    let mut bytes = 0;
    fn compare(
        output: &mut impl Read,
        r: &ReplaySession,
        done: bool,
        checks: u32,
        writes: u32,
        kind: u32,
        snapshots: usize,
    ) -> usize {
        let t = r.save.timeline;
        let p = r.pending.as_ref().unwrap();
        let bank = r
            .layers
            .record(shin_core::vm::command::types::LayerbankId::new_unchecked(0));
        let route = &r.save.progress.active_entry().responses;
        let flags = p.autosave.as_ref().map_or([0, 0], |a| a.flags());
        let mut expected = b"GRP1".to_vec();
        for word in [
            done as u32,
            checks,
            writes,
            r.vm.read_register(reg(7)) as u32,
            t.boundary,
            t.previous_message,
            t.current_message,
            t.current_interaction,
            t.last_skip,
            t.resume_position,
            t.event as u32,
            r.save.response_bit_position as u32,
            p.replay_stop as u32,
            if kind == 1 {
                bank.interaction_completed() as u32
            } else {
                0
            },
            if kind == 1 {
                bank.interaction_result() as u32
            } else {
                0
            },
            flags[0] as u32,
            flags[1] as u32,
            route.len() as u32,
        ] {
            expected.extend(word.to_le_bytes());
        }
        expected.extend(route);
        let mut actual = vec![0; expected.len()];
        output.read_exact(&mut actual).unwrap();
        let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
        assert_eq!(
            mismatch,
            None,
            "replaypollsnapshot{snapshots} kind{kind},timeline{t:?},phase{},head{:?}vs{:?}",
            p.replay_stop,
            &actual[..76],
            &expected[..76]
        );
        actual.len()
    }
    for kind in 0..7 {
        for boundary in [0u32, 1, 199, u32::MAX] {
            for target in [
                boundary.wrapping_sub(1),
                boundary,
                boundary.wrapping_add(1),
                boundary.wrapping_add(2),
                u32::MAX,
            ] {
                for has_text in [false, true] {
                    for cursor in [0, 3, 7] {
                        for event in [-1, 5] {
                            let s = scenario(|d| {
                                match kind {
                                    0 => {
                                        d.push(0x8d);
                                        d.extend(8u16.to_le_bytes());
                                        d.extend(2u16.to_le_bytes());
                                        d.extend(7u16.to_le_bytes());
                                        num(d, 5);
                                        text(d, "");
                                        let labels = b"A\0B\0C\0\0";
                                        d.extend((labels.len() as u16).to_le_bytes());
                                        d.extend(labels);
                                    }
                                    1 => {
                                        d.push(0xe0);
                                        d.extend(7u16.to_le_bytes());
                                        num(d, 9);
                                    }
                                    2 => d.push(0xa1),
                                    3 => message(d),
                                    4 => {
                                        d.push(0xa2);
                                        num(d, 7);
                                    }
                                    5 => d.push(0xa3),
                                    6 => d.push(0xa4),
                                    _ => unreachable!(),
                                };
                                d.extend([0, 0, 0]);
                            });
                            let route = vec![0x20, 0xef, 0x7b, 0x44];
                            let mut replay = saved(s, target, target, route.clone());
                            replay.save.timeline.boundary = boundary;
                            replay.save.timeline.event = event;
                            replay.save.response_bit_position = cursor;
                            if has_text {
                                replay.save.descriptors.message.text = Some(Arc::from(""));
                            }
                            if kind == 1 {
                                replay.layers.load(9, 9, 0, [0; 8]).unwrap();
                            }
                            let row = [
                                0,
                                kind,
                                boundary,
                                target,
                                cursor as u32,
                                has_text as u32,
                                event as u32,
                                route.len() as u32,
                            ];
                            input
                                .write_all(
                                    &row.into_iter()
                                        .flat_map(u32::to_le_bytes)
                                        .collect::<Vec<_>>(),
                                )
                                .unwrap();
                            input.write_all(&route).unwrap();
                            input.flush().unwrap();
                            replay.prepare(&IndexSet::new(), 1000).unwrap();
                            let checks = if kind == 3 {
                                u32::from(event < 0)
                            } else if kind <= 1 {
                                u32::from(!has_text)
                            } else {
                                1
                            };
                            bytes +=
                                compare(&mut output, &replay, false, checks, 0, kind, snapshots);
                            snapshots += 1;
                            input
                                .write_all(
                                    &[1u32, 0, 0, 0, 0, 0, 0, 0]
                                        .into_iter()
                                        .flat_map(u32::to_le_bytes)
                                        .collect::<Vec<_>>(),
                                )
                                .unwrap();
                            input.flush().unwrap();
                            let done = replay.poll().unwrap();
                            let writes = u32::from(
                                replay
                                    .pending
                                    .as_ref()
                                    .unwrap()
                                    .interactive_result
                                    .is_some(),
                            );
                            bytes += compare(
                                &mut output,
                                &replay,
                                done,
                                checks,
                                writes,
                                kind,
                                snapshots,
                            );
                            snapshots += 1;
                        }
                    }
                }
            }
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_REPLAY_POLLS_NATIVE_MATCH snapshots={snapshots} bytes={bytes} original_ctor_route_register_boundary_retained_flags=verified prepared_checkpoint_lookup_read_register=bounded"
    );
}
