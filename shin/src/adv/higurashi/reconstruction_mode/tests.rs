use super::*;
use shin_core::time::Ticks;
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[derive(Default)]
struct Owners {
    events: Vec<&'static str>,
    old: bool,
    vm: u32,
    retained: bool,
    animations: u32,
    mask_refs: u32,
    fail: bool,
}
impl Services for Owners {
    fn publish_resources(&mut self) -> Result<()> {
        self.events.push("P");
        anyhow::ensure!(!self.fail, "publication failed");
        Ok(())
    }
    fn reset_blackout_and_indicators(&mut self) {
        self.events.extend(["B", "I", "J"]);
    }
    fn clear_message(&mut self) {
        self.events.push("C");
        self.mask_refs += 1;
    }
    fn retire_source_vm(&mut self) {
        self.events.push("D");
        self.old = false;
        self.vm = 0;
    }
    fn enable_animations(&mut self) {
        self.animations = 1;
    }
    fn install_replayed_vm(&mut self) {
        if self.old {
            self.events.push("D");
            self.old = false;
        }
        self.vm = 2;
        self.retained = false;
    }
}
#[test]
#[ignore = "original updated19578 phase/VM ownership with checked publication services"]
fn higurashi_reconstruction_mode_matches_native_phase_and_overlay_clock() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for mode in [0, 1, 2, u32::MAX] {
        for source in [false, true] {
            for mask in [false, true] {
                for phase in [Phase::Preparing, Phase::Fading, Phase::Complete] {
                    for delta in [0.0, 0.25, 0.5, 3.0, 4.0, 29.5, 100.0] {
                        profiles += 1;
                        let raw = match phase {
                            Phase::Preparing => 0,
                            Phase::Fading => 1,
                            Phase::Complete => u32::MAX,
                        };
                        input +=
                            &format!("N {mode} {} {} {raw}\n", u8::from(source), u8::from(mask));
                        let mut state = ReconstructionMode { mode, phase };
                        let mut clock = OverlayClock::captured(source);
                        let mut owners = Owners {
                            old: true,
                            vm: 1,
                            retained: true,
                            mask_refs: u32::from(mask),
                            ..Default::default()
                        };
                        for job in [0, 2, u32::MAX, 1, 0, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1] {
                            input += &format!("P {job}\n");
                            owners.events.clear();
                            let phase = state.poll(job, mask, &mut clock, &mut owners).unwrap();
                            let raw = match phase {
                                Phase::Preparing => 0,
                                Phase::Fading => 1,
                                Phase::Complete => -1,
                            };
                            let events = if owners.events.is_empty() {
                                String::new()
                            } else {
                                format!(" {}", owners.events.join(" "))
                            };
                            expected.push(format!(
                                "P{events} | 0 {raw} {} {} {} {} {} {} {} {} {} 1 {} {}",
                                u8::from(phase == Phase::Complete),
                                owners.animations,
                                owners.vm,
                                u8::from(owners.retained),
                                u8::from(clock.active),
                                clock.elapsed.to_bits(),
                                clock.duration.to_bits(),
                                u8::from(clock.mask_present),
                                u8::from(owners.old),
                                owners.mask_refs,
                                u8::from(source)
                            ));
                            input += &format!("U {:08x}\n", f32::to_bits(delta));
                            clock.advance(Ticks::from_f32(delta));
                        }
                    }
                }
            }
        }
    }
    let mut weight_profiles = 0;
    for duration in [1.0, 4.0, 30.0, 100.0] {
        for softness in [1.0 / 256.0, 0.3, 1.0, 4.0] {
            for flags in [0, 1, 6, 9] {
                for progress in [0.0, 0.1, 0.5, 0.9, 1.0] {
                    weight_profiles += 1;
                    let clock = OverlayClock {
                        active: true,
                        elapsed: duration * progress,
                        duration,
                        source_present: true,
                        mask_present: true,
                        softness,
                        flags,
                    };
                    input += &format!(
                        "W {:x} {:x} {:x} {flags}\n",
                        clock.elapsed.to_bits(),
                        duration.to_bits(),
                        softness.to_bits()
                    );
                    let weights = clock.mask_weights();
                    expected.push(format!(
                        "W {} {}",
                        weights[0].to_bits(),
                        weights[1].to_bits()
                    ));
                }
            }
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_RECONSTRUCTION_MODE").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (index, (a, e)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "native phase comparison{index}");
    }
    println!(
        "Native reconstruction modes:{profiles} profiles,{} phase/ownership/clock comparisons,{weight_profiles} mask weight profiles",
        expected.len() - weight_profiles
    );
}
#[test]
fn publication_errors_preserve_preparing_source_and_fade_starts_after_retry() {
    let mut state = ReconstructionMode::new(0);
    let mut clock = OverlayClock::captured(true);
    let mut services = Owners {
        old: true,
        vm: 1,
        retained: true,
        fail: true,
        ..Default::default()
    };
    assert!(state.poll(1, true, &mut clock, &mut services).is_err());
    assert_eq!(state.phase(), Phase::Preparing);
    assert!(services.old);
    assert!(!clock.active);
    services.fail = false;
    assert_eq!(
        state.poll(1, true, &mut clock, &mut services).unwrap(),
        Phase::Fading
    );
    clock.advance(Ticks::from_f32(29.5));
    assert_eq!(
        state.poll(0, false, &mut clock, &mut services).unwrap(),
        Phase::Fading
    );
    clock.advance(Ticks::from_f32(0.5));
    assert_eq!(
        state.poll(0, false, &mut clock, &mut services).unwrap(),
        Phase::Complete
    );
    assert!(!services.old);
    assert_eq!(services.vm, 2);
}
