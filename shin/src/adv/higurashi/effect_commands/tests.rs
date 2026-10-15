use super::*;
use std::{
    cell::Cell,
    io::Write,
    process::{Command, Stdio},
};
#[derive(Default)]
struct Effects {
    calls: Vec<Vec<u32>>,
    descriptors: EffectDescriptors,
    statuses: BTreeMap<i32, i32>,
    slots: Vec<i32>,
    queries: Cell<u32>,
    after: i32,
}
impl EffectCommands for Effects {
    fn prepare_effect_command(&mut self, c: &Cmd) {
        self.descriptors.observe(c);
    }
    fn effect_descriptor_id(&self, s: i32) -> i32 {
        self.descriptors.id(s)
    }
    fn play_effect(&mut self, p: EffectPlay) -> Result<()> {
        self.calls.push(vec![
            1,
            1,
            1,
            p.slot as u32,
            p.id as u32,
            p.fade as u32,
            p.count,
            p.volume.to_bits(),
            p.pan.to_bits(),
            p.speed.to_bits(),
        ]);
        self.statuses.insert(p.slot, self.after);
        Ok(())
    }
    fn stop_effect(&mut self, s: i32, f: i32) -> Result<()> {
        self.calls
            .push(vec![1, 1, 2, s as u32, 0, f as u32, 0, 0, 0, 0]);
        Ok(())
    }
    fn stop_all_effects(&mut self, f: i32) -> Result<()> {
        self.calls.push(vec![1, 1, 3, 0, f as u32, 0, 0]);
        Ok(())
    }
    fn volume_effect(&mut self, s: i32, v: f32, f: i32) -> Result<()> {
        self.calls
            .push(vec![1, 1, 4, s as u32, f as u32, v.to_bits(), 0]);
        Ok(())
    }
    fn pan_effect(&mut self, s: i32, v: f32, f: i32) -> Result<()> {
        self.calls
            .push(vec![1, 1, 5, s as u32, f as u32, 0, v.to_bits()]);
        Ok(())
    }
    fn effect_status(&self, s: i32) -> i32 {
        self.queries.set(self.queries.get() + 1);
        self.statuses.get(&s).copied().unwrap_or(0)
    }
    fn active_effect_slots(&self) -> Vec<i32> {
        self.slots.clone()
    }
}
#[test]
#[ignore = "runs original updated SE callbacks/descriptor constructors with bounded media contracts"]
fn higurashi_effects_match_native_callbacks_descriptors_and_waits() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let values = [i32::MIN, -1000, -1, 0, 1, 500, 1000, i32::MAX];
    let fades = [i32::MIN, -1, 0, 1, 4, 60, i32::MAX];
    let counts = [i32::MIN, -1, 0, 1, 2, 3, i32::MAX];
    let slots = [i32::MIN, -1, 0, 31, 32, i32::MAX];
    for slot in slots {
        for id in [-1, 0, 495] {
            for fade in fades {
                for count in counts {
                    for (index, volume) in values.into_iter().enumerate() {
                        for skip in [false, true] {
                            let pan = values[(index + 3) % values.len()];
                            let speed = values[(index + 5) % values.len()];
                            input.push_str(&format!(
                                "P {slot} {id} {fade} {count} {volume} {pan} {speed} {}\n",
                                u8::from(skip)
                            ));
                            let mut e = Effects::default();
                            assert_eq!(
                                dispatch(
                                    &Cmd::SEPLAY {
                                        args: [slot, id, fade, count, volume, pan, speed]
                                    },
                                    skip,
                                    &mut e
                                )
                                .unwrap(),
                                Some(EffectCommand::Complete)
                            );
                            assert_eq!(
                                e.effect_descriptor_id(slot),
                                if count == 0 { id } else { -1 }
                            );
                            expected.push(e.calls.remove(0));
                        }
                    }
                }
            }
        }
    }
    let plays = expected.len();
    for slot in slots {
        for fade in fades {
            for value in values {
                for skip in [false, true] {
                    for (op, c) in [
                        ('S', Cmd::SESTOP { args: [slot, fade] }),
                        ('A', Cmd::SESTOPALL { fade }),
                        (
                            'V',
                            Cmd::SEVOL {
                                args: [slot, value, fade],
                            },
                        ),
                        (
                            'N',
                            Cmd::SEPAN {
                                args: [slot, value, fade],
                            },
                        ),
                    ] {
                        input.push_str(&format!("{op} {slot} {value} {fade} {}\n", u8::from(skip)));
                        let mut e = Effects::default();
                        assert_eq!(
                            dispatch(&c, skip, &mut e).unwrap(),
                            Some(EffectCommand::Complete)
                        );
                        let mut row = e.calls.remove(0);
                        if op == 'S' {
                            row = vec![row[0], row[1], row[2], row[3], row[5], 0, 0];
                        }
                        expected.push(row);
                    }
                }
            }
        }
    }
    let setters = expected.len() - plays;
    let masks = [i32::MIN, -1, 0, 1, 2, 3, 4, 8, 16, 31, i32::MAX];
    let statuses = [0, 1, 2, 3, 4, 7, 31, i32::MIN];
    for mask in masks {
        for status in statuses {
            for descriptor in [-1, 0, i32::MIN, 99] {
                for slot in [-7, -1, 0, 31] {
                    let entries: &[(i32, i32, i32)] = if slot == 31 {
                        &[(31, status, descriptor)]
                    } else {
                        &[(0, status, descriptor), (1, 3, 0), (-1, status, 0)]
                    };
                    input.push_str(&format!("W {slot} {mask} {}", entries.len()));
                    let mut e = Effects::default();
                    for &(key, state, desc) in entries {
                        input.push_str(&format!(" {key} {} {desc}", state as u32));
                        e.slots.push(key);
                        e.statuses.insert(key, state);
                        e.descriptors.0.insert(key, desc);
                    }
                    input.push('\n');
                    let Some(EffectCommand::Wait(wait)) =
                        dispatch(&Cmd::SEWAIT { args: [slot, mask] }, true, &mut e).unwrap()
                    else {
                        panic!()
                    };
                    let done = wait.complete(&e, true);
                    let allocated = u32::from(slot < 0 && !entries.is_empty());
                    expected.push(vec![u32::from(done), e.queries.get(), allocated, allocated]);
                }
            }
        }
    }
    let waits = expected.len() - plays - setters;
    for id in [-1, 0, 495] {
        for mask in [0, 1, 2, 4, 31, -1] {
            for (index, volume) in values.into_iter().enumerate() {
                for speed in [i32::MIN, 1000, i32::MAX] {
                    for skip in [false, true] {
                        for occupied in [0, 1, 4] {
                            for started in [false, true] {
                                let pan = values[(index + 3) % values.len()];
                                let status = statuses[(index + 2) % statuses.len()];
                                let retained_slot = -20;
                                input.push_str(&format!("O {id} {mask} {volume} {pan} {speed} {} {} {retained_slot} {occupied} {}\n",u8::from(skip),u8::from(started),status as u32));
                                let mut e = Effects {
                                    after: status,
                                    ..Effects::default()
                                };
                                for i in 0..occupied {
                                    e.statuses.insert(-i - 1, 3);
                                }
                                let (done, stored_slot, stored_started) = if started {
                                    e.statuses.insert(retained_slot, status);
                                    (
                                        EffectWait::Once {
                                            slot: retained_slot,
                                            mask,
                                        }
                                        .complete(&e, skip),
                                        retained_slot,
                                        1,
                                    )
                                } else {
                                    match dispatch(
                                        &Cmd::SEONCE {
                                            args: [id, mask, volume, pan, speed],
                                        },
                                        skip,
                                        &mut e,
                                    )
                                    .unwrap()
                                    .unwrap()
                                    {
                                        EffectCommand::Complete => (true, retained_slot, 0),
                                        EffectCommand::Wait(w @ EffectWait::Once { slot, .. }) => {
                                            (w.complete(&e, skip), slot, 1)
                                        }
                                        _ => panic!(),
                                    }
                                };
                                let mut row = if let Some(call) = e.calls.first() {
                                    vec![
                                        u32::from(done),
                                        1,
                                        call[3],
                                        call[4],
                                        call[5],
                                        call[6],
                                        call[7],
                                        call[8],
                                        call[9],
                                    ]
                                } else {
                                    vec![u32::from(done), 0, 0, 0, 0, 0, 0, 0, 0]
                                };
                                row.extend([stored_started, stored_slot as u32, e.queries.get()]);
                                expected.push(row);
                            }
                        }
                    }
                }
            }
        }
    }
    let once = expected.len() - plays - setters - waits;
    for kind in 0..3 {
        for slot in [0, 1, 31] {
            for id in [i32::MIN, -1, 0, 1, 495, i32::MAX] {
                for count in counts {
                    input.push_str(&format!("D {kind} {slot} {id} 9 {count} 1000 0 1000\n"));
                    let mut d = EffectDescriptors::default();
                    for s in 0..32 {
                        d.0.insert(s, 0x12345678);
                    }
                    d.observe(&match kind {
                        0 => Cmd::SEPLAY {
                            args: [slot, id, 9, count, 1000, 0, 1000],
                        },
                        1 => Cmd::SESTOP { args: [slot, 9] },
                        _ => Cmd::SESTOPALL { fade: 9 },
                    });
                    expected.push((0..32).map(|s| d.id(s) as u32).collect());
                }
            }
        }
    }
    let descriptors = expected.len() - plays - setters - waits - once;
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_EFFECT_COMMANDS").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (index, (row, expected)) in text.lines().zip(expected).enumerate() {
        let actual: Vec<u32> = row
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(actual, expected, "SE row{index}");
    }
    println!(
        "Higurashi SE:{plays} native PLAY/skip cases,{setters} setters,{waits} exact/aggregate WAIT cases,{once} ONCE start/retention/skip cases,{descriptors} native VM descriptor constructors match; checked slots/status/media contracts and balanced wait allocations"
    );
}
