use super::*;
use crate::adv::higurashi::runner::reconstruction_mode::ReconstructionUi;
use std::{
    io::Write,
    process::{Command, Stdio},
};

mod host;

fn snapshot(ui: &HigurashiUiState, op: char) -> String {
    let words = [
        u32::from(ui.indicator.hidden),
        ui.indicator.opacity.to_bits(),
        ui.indicator.transition.value().to_bits(),
        u32::from(ui.indicator.transition.is_waiting()),
        u32::from(ui.menu.indicator.hidden),
        ui.menu.indicator.opacity.to_bits(),
        ui.menu.indicator.transition.value().to_bits(),
        u32::from(ui.menu.indicator.transition.is_waiting()),
        ui.menu.elapsed.to_bits(),
        ui.hint.value().to_bits(),
        u32::from(ui.hint.is_waiting()),
        1,
    ];
    format!("Q {op} {}", words.map(|x| x.to_string()).join(" "))
}

#[test]
#[ignore = "actual55f90 notification stop with original older tween queue,hidden/opacity/flags and busy/idle clocks"]
fn higurashi_exit_notification_stop_matches_native_busy_flags_hidden_and_clock() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for hidden in [0, 1] {
        for opacity in [0.0f32, 0.5, 1.0] {
            for flags in [0, 0x200, 0x140, 0x340] {
                for active in [false, true] {
                    let mut ui=HigurashiUiState::default();
                    ui.indicator.set_hidden(hidden);
                    ui.indicator.opacity=opacity;
                    input+=&format!("N {hidden} 0 {:x} 3f800000 0\n",opacity.to_bits());
                    expected.push(snapshot(&ui,'N'));
                    if active {
                        for (value,time,flags) in [(1.0f32,9.0f32,flags),(0.5,12.0,0)] {
                            input+=&format!("E 1 {flags:x} {:x} {:x}\n",value.to_bits(),time.to_bits());
                            ui.indicator.transition.control(value,Ticks::from_f32(time),flags).unwrap();
                            expected.push(snapshot(&ui,'E'));
                        }
                    }
                    for op in ['C','C','U','U','U','U','U','C'] {
                        if op=='C' {
                            input+="C\n";
                            ui.indicator.stop();
                        } else {
                            input+="U 40400000\n";
                            ui.advance(Ticks::from_f32(3.0));
                        }
                        expected.push(snapshot(&ui,op));
                    }
                    profiles+=1;
                }
            }
        }
    }
    let mut child=Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_UI_STATE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin=child.stdin.take().unwrap();
    let copy=input.clone();
    let writer=std::thread::spawn(move||stdin.write_all(copy.as_bytes()).unwrap());
    let output=child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let actual=String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(),expected.len());
    for (i,(a,e)) in actual.lines().zip(&expected).enumerate(){assert_eq!(a,e,"Native notification stop{i}");}
    let path="/tmp/higurashi-exit-live-ui-capture";
    std::fs::create_dir_all(path).unwrap();
    for (name,data) in [("input.txt",input.as_bytes()),("output.txt",actual.as_bytes()),("stderr.txt",&output.stderr)]{
        std::fs::write(format!("{path}/{name}"),data).unwrap();
    }
    println!("EXIT notification:{profiles} native55f90 timelines/{} exact stop/hidden/opacity/flags/idle/clock observations pass",expected.len());
}

#[test]
#[ignore = "executes full updated indicator/menu resets and opacity/tween clocks"]
fn higurashi_ui_resets_clocks_and_hint_pulse_match_native() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for ih in [0, 1, 2, u32::MAX] {
        for mh in [0, 1, 2, u32::MAX] {
            for opacity in [-0.25f32, 0.0, 0.5, 1.0, 1.25, f32::from_bits(0x7fc00001)] {
                for menu_opacity in [0.0f32, 0.5, 1.0] {
                    for flags in [0, 0x200, 0x140, 0x340] {
                        let mut ui = HigurashiUiState::default();
                        ui.indicator.set_hidden(ih);
                        ui.menu.indicator.set_hidden(mh);
                        ui.indicator.opacity = opacity;
                        ui.menu.indicator.opacity = menu_opacity;
                        ui.menu.elapsed = 17.5;
                        input.push_str(&format!(
                            "N {ih} {mh} {:x} {:x} {:x}\n",
                            opacity.to_bits(),
                            menu_opacity.to_bits(),
                            ui.menu.elapsed.to_bits()
                        ));
                        expected.push(snapshot(&ui, 'N'));
                        for (id, v, t, f) in [
                            (0, 0.8f32, 7.0f32, 0),
                            (0, 0.2, 11.0, 0),
                            (1, 1.0, 9.0, flags),
                            (1, 0.5, 12.0, 0),
                            (2, 0.9, 6.0, flags),
                            (2, 0.1, 4.0, 0),
                        ] {
                            input.push_str(&format!(
                                "E {id} {f:x} {:x} {:x}\n",
                                v.to_bits(),
                                t.to_bits()
                            ));
                            let tween = match id {
                                0 => &mut ui.hint,
                                1 => &mut ui.indicator.transition,
                                _ => &mut ui.menu.indicator.transition,
                            };
                            tween.control(v, Ticks::from_f32(t), f).unwrap();
                            expected.push(snapshot(&ui, 'E'));
                        }
                        for delta in [0.0f32, 0.25, 1.0, 3.0] {
                            input.push_str(&format!("U {:x}\n", delta.to_bits()));
                            ui.advance(Ticks::from_f32(delta));
                            expected.push(snapshot(&ui, 'U'));
                        }
                        input.push_str("S\nF\nH 0 0\n");
                        ui.menu.stop();
                        expected.push(snapshot(&ui, 'S'));
                        ui.indicator.fast_forward();
                        ui.menu.fast_forward();
                        expected.push(snapshot(&ui, 'F'));
                        ui.indicator.set_hidden(0);
                        ui.menu.indicator.set_hidden(0);
                        expected.push(snapshot(&ui, 'H'));
                        for delta in [0.5f32, 3.0, 10.0] {
                            input.push_str(&format!("U {:x}\n", delta.to_bits()));
                            ui.advance(Ticks::from_f32(delta));
                            expected.push(snapshot(&ui, 'U'));
                        }
                        // Clear active/protected/queued motions and immediately
                        // exercise the three-stage hint on the same owners.
                        input.push_str("R\nP\n");
                        ui.reset_blackout_and_indicators();
                        expected.push(snapshot(&ui, 'R'));
                        ui.pulse_hint();
                        expected.push(snapshot(&ui, 'P'));
                        for delta in [0.0f32, 31.0, 1.0, 119.0, 1.0, 31.0, 1.0, 3.0] {
                            input.push_str(&format!("U {:x}\n", delta.to_bits()));
                            ui.advance(Ticks::from_f32(delta));
                            expected.push(snapshot(&ui, 'U'));
                        }
                        input.push_str("R\n");
                        ui.reset_blackout_and_indicators();
                        expected.push(snapshot(&ui, 'R'));
                        profiles += 1;
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_UI_STATE").unwrap())
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
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (i, (a, e)) in actual.lines().zip(&expected).enumerate() {
        assert_eq!(a, e, "Native UI row{i}");
    }
    println!(
        "Higurashi UI:{profiles} native timelines,{} exact reset/clock/queue observations pass",
        expected.len()
    );
}
