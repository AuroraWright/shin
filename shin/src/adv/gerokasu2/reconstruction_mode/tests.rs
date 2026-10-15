use super::*;
#[derive(Default)]
struct Events {
    events: Vec<u8>,
    fail: bool,
}
impl Services for Events {
    fn publish_resources(&mut self) -> Result<()> {
        self.events.push(1);
        anyhow::ensure!(!self.fail, "fixture failure");
        Ok(())
    }
    fn reset_blackout_and_indicators(&mut self) {
        self.events.push(2);
    }
    fn clear_message(&mut self) {
        self.events.push(3);
    }
    fn install_and_retire_source_vm(&mut self) {
        self.events.push(4);
    }
}
#[test]
fn ger_reconstruction_modes_publish_vm_before_fade_and_resume_skips_indicator_reset() {
    for mode in 0..=4 {
        let mut owner = ReconstructionMode::new(mode);
        let mut clock = OverlayClock::captured(true);
        clock.mask_present = true;
        let mut events = Events::default();
        for ready in [0, 2, 0] {
            assert_eq!(
                owner.poll(ready, &mut clock, &mut events).unwrap(),
                Phase::Preparing
            );
            assert!(events.events.is_empty());
        }
        let phase = owner.poll(1, &mut clock, &mut events).unwrap();
        assert_eq!(
            events.events,
            match mode {
                0 => vec![1, 2, 3, 4],
                3 => vec![1, 4],
                _ => vec![1, 2, 4],
            }
        );
        assert_eq!(
            phase,
            if matches!(mode, 2 | 3) {
                Phase::Complete
            } else {
                Phase::Fading
            }
        );
        if mode < 2 {
            let duration = if mode == 0 { 30.0 } else { 4.0 };
            clock.advance(Ticks::from_f32(duration));
            assert_eq!(owner.phase(), Phase::Fading);
            assert_eq!(
                owner.poll(0, &mut clock, &mut events).unwrap(),
                Phase::Complete
            );
            assert_eq!(events.events.iter().filter(|&&x| x == 4).count(), 1);
        }
    }
}
#[test]
fn ger_reconstruction_failure_retains_phase_and_resume_requests_once_only_after_success() {
    let mut mode = ReconstructionMode::new(1);
    let mut clock = OverlayClock::captured(true);
    let mut events = Events {
        fail: true,
        ..Default::default()
    };
    assert!(mode.poll(1, &mut clock, &mut events).is_err());
    assert_eq!(mode.phase(), Phase::Preparing);
    assert!(!clock.active);
    let mut resume = ResumeCommand::default();
    assert!(resume.poll(|| anyhow::bail!("failed request")).is_err());
    let mut calls = 0;
    for _ in 0..4 {
        assert!(
            !resume
                .poll(|| {
                    calls += 1;
                    Ok(())
                })
                .unwrap()
        );
    }
    assert_eq!(calls, 1);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original native Ger overlay draw/clock, RESUME and whole phase2; CPU only"]
fn ger_native_reconstruction_mode_clock_packets_resume_and_completion() {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/reconstruction-mode");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-reconstruction-mode");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-reconstruction-mode.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut count = 0;
    let mut query = |row: [u32; 8]| {
        for w in row {
            input.write_all(&w.to_le_bytes()).unwrap();
        }
        input.flush().unwrap();
        let mut bytes = [0; 160];
        output.read_exact(&mut bytes).unwrap();
        count += 1;
        bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    for source in [false, true] {
        for start in 0..=2 {
            for duration in [0u32, 1, 4, 30, u32::MAX] {
                for present in [false, true] {
                    for softness in [0.0f32, -0.3, 0.3, 1.0 / 256.0] {
                        for flags in [0, 1, 2, 4, 8, 15, 0xabcdef00] {
                            for delta in [0.0f32, 0.5, 4.0, 30.0, -0.5] {
                                let row = [
                                    0,
                                    source as u32,
                                    start,
                                    duration,
                                    present as u32,
                                    softness.to_bits(),
                                    flags,
                                    delta.to_bits(),
                                ];
                                let native = query(row);
                                let mut clock = OverlayClock::captured(source);
                                clock.softness = delta;
                                clock.flags = 0xabcdef00;
                                if start == 1 {
                                    clock.clear(duration);
                                } else if start == 2 {
                                    clock.masked(duration, softness, flags, present);
                                }
                                clock.advance(Ticks::from_f32(delta));
                                assert_eq!(
                                    &native[..8],
                                    &[
                                        clock.active as u32,
                                        clock.elapsed.to_bits(),
                                        clock.duration.to_bits(),
                                        source as u32,
                                        clock.mask_present as u32,
                                        clock.softness.to_bits(),
                                        clock.flags,
                                        clock.busy() as u32
                                    ],
                                    "{row:?}"
                                );
                                let drawn = clock.visible();
                                let masked = start == 2 && present;
                                assert_eq!(
                                    native[8],
                                    if !drawn {
                                        0
                                    } else if masked {
                                        33
                                    } else {
                                        9
                                    },
                                    "{row:?}"
                                );
                                if drawn && masked {
                                    assert_eq!(
                                        &native[9..11],
                                        &clock.mask_weights().map(f32::to_bits),
                                        "{row:?}"
                                    );
                                }
                                if drawn && !masked {
                                    assert_eq!(
                                        &native[9..13],
                                        &[(1.0 - clock.elapsed / clock.duration).to_bits(); 4],
                                        "{row:?}"
                                    );
                                }
                                match clock.draw_packet(masked.then_some((640, 360))) {
                                    Some(overlay::DrawPacket::Masked { vertices, .. }) => {
                                        let words: Vec<_> = vertices
                                            .into_iter()
                                            .flat_map(|v| {
                                                [
                                                    v.position.x,
                                                    v.position.y,
                                                    v.texture_position.x,
                                                    v.texture_position.y,
                                                    v.mask_position.x,
                                                    v.mask_position.y,
                                                ]
                                                .map(f32::to_bits)
                                            })
                                            .collect();
                                        assert_eq!(
                                            &native[13..37],
                                            words.as_slice(),
                                            "native masked quad {row:?}"
                                        );
                                    }
                                    Some(overlay::DrawPacket::Plain { vertices, .. }) => {
                                        let words: Vec<_> = vertices
                                            .into_iter()
                                            .flat_map(|v| {
                                                [
                                                    v.position.x,
                                                    v.position.y,
                                                    v.texture_position.x,
                                                    v.texture_position.y,
                                                ]
                                                .map(f32::to_bits)
                                            })
                                            .collect();
                                        assert_eq!(
                                            &native[13..29],
                                            words.as_slice(),
                                            "native plain quad {row:?}"
                                        );
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for phase in [2u32, u32::MAX, 3] {
        for source in [0, 1] {
            for busy in [0, 1, 2, 3] {
                for caller in [0, 1, 2, u32::MAX] {
                    let native = query([1, phase, source, busy, caller, 0, 0, 0]);
                    assert_eq!(
                        &native[..3],
                        &[
                            caller & 1,
                            if phase == 2 && busy & 1 == 0 {
                                u32::MAX
                            } else {
                                phase
                            },
                            if phase == 2 && busy & 1 == 0 {
                                0
                            } else {
                                source
                            }
                        ]
                    );
                }
            }
        }
    }
    for entered in [0u32, 1, 2, 255] {
        for position in [0, 1, 200, u32::MAX] {
            let native = query([2, entered, position, 0, 0, 0, 0, 0]);
            let expected = if entered == 0 {
                [0, 1, 1, 1, 3, 1, 0]
            } else {
                [0, entered, 0, 0, 99, 0, 0]
            };
            assert_eq!(&native[..7], &expected);
        }
    }
    drop(query);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_RECONSTRUCTION_MODE_NATIVE cases={count} clock_draw_FMA_RESUME_phase2=verified SDK_fork_payload=guarded"
    );
}
