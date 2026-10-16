use super::*;
#[test]
fn ger_exit_waits_gates_and_save_before_focus_reset_and_title() {
    assert!(ExitMode::for_command(1).is_none());
    let mut mode = ExitMode::for_command(0).unwrap();
    let ready = ExitGates {
        tasks_idle: true,
        ..Default::default()
    };
    assert_eq!(mode.poll(ready), [ExitAction::PrepareExit]);
    mode.advance_clock(100.);
    assert_eq!(mode.remaining(), -70.);
    for gates in [
        ExitGates {
            hide_motion_busy: true,
            ..ready
        },
        ExitGates {
            message_busy: true,
            ..ready
        },
        ExitGates::default(),
    ] {
        assert!(mode.poll(gates).is_empty());
        assert!(mode.save_completed().is_empty());
    }
    assert_eq!(mode.poll(ready), [ExitAction::RequestSave]);
    assert!(mode.poll(ready).is_empty());
    mode.retry_save();
    assert_eq!(mode.poll(ready), [ExitAction::RequestSave]);
    assert_eq!(
        mode.save_completed(),
        [
            ExitAction::RestoreBgmFocus,
            ExitAction::ReturnTitle { requested_id: -1 }
        ]
    );
    assert!(mode.save_completed().is_empty());
}

#[test]
#[ignore = "executes original Ger EXIT controller/save completion/Title factory;CPU only"]
fn ger_native_exit_wait_gates_save_completion_focus_and_title() {
    use std::{
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };
    let root = std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let binary = std::env::var_os("SHIN_GEROKASU2_ADV_TITLE_EXIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("gerokasu2/.local/adv-title-transition/native-exit"));
    let (mut script, mut expected, mut profiles) = (String::new(), vec![], 0);
    for hidden in [false, true] {
        for delta in [0., 0.25, 1., 2.5, 30., 1000.] {
            profiles += 1;
            let mut mode = ExitMode::for_command(0).unwrap();
            let mut frame = 0;
            let mut commits = 0;
            let mut pending = false;
            let mut focus = 0.5f32;
            let mut prepared = false;
            let snapshot = |m: &ExitMode,
                            frame: i64,
                            commits: i64,
                            pending: bool,
                            focus: f32,
                            prepared: bool| {
                vec![
                    frame,
                    match m.phase() {
                        ExitPhase::Pending => 0,
                        ExitPhase::Waiting => 1,
                        _ => -1,
                    },
                    i64::from(m.remaining().to_bits()),
                    i64::from(hidden && !prepared),
                    i64::from(!prepared),
                    i64::from(prepared),
                    if prepared { 0 } else { 3 },
                    0,
                    commits,
                    commits,
                    i64::from(pending),
                    i64::from(focus.to_bits()),
                    if m.phase() == ExitPhase::Complete {
                        -1
                    } else {
                        99
                    },
                ]
            };
            script.push_str(&format!("N {}\n", u8::from(hidden)));
            expected.push(snapshot(&mode, frame, commits, pending, focus, prepared));
            let mut frames = vec![];
            if delta == 0. {
                frames.extend([(0., false, false, true); 3]);
                frames.push((30., false, false, true));
            } else {
                for _ in 0..(30f32 / delta).ceil() as usize {
                    frames.push((delta, false, false, true));
                }
            }
            frames.extend([
                (0., true, false, true),
                (0., false, true, true),
                (0., false, false, false),
                (0., false, false, true),
                (0., false, false, true),
            ]);
            for (dt, hide_busy, message_busy, tasks_idle) in frames {
                script.push_str(&format!(
                    "F {dt} {} {} {}\n",
                    u8::from(hide_busy),
                    u8::from(message_busy),
                    u8::from(tasks_idle)
                ));
                for action in mode.poll(ExitGates {
                    hide_motion_busy: hide_busy,
                    message_busy,
                    tasks_idle,
                }) {
                    match action {
                        ExitAction::PrepareExit => prepared = true,
                        ExitAction::RequestSave => {
                            commits += 1;
                            pending = true;
                        }
                        _ => panic!("unexpected pre-save action"),
                    }
                }
                mode.advance_clock(dt);
                frame += 1;
                expected.push(snapshot(&mode, frame, commits, pending, focus, prepared));
            }
            assert_eq!(
                mode.save_completed(),
                [
                    ExitAction::RestoreBgmFocus,
                    ExitAction::ReturnTitle { requested_id: -1 }
                ]
            );
            pending = false;
            focus = 1.;
            script.push_str("S\n");
            expected.push(snapshot(&mode, frame, commits, pending, focus, prepared));
            script.push_str("X\n");
        }
    }
    let mut child = Command::new(binary)
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
        "nativeEXIT{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    let actual = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = actual.lines().filter(|l| l.starts_with("X ")).collect();
    assert_eq!(rows.len(), expected.len());
    for (i, (line, wanted)) in rows.iter().zip(&expected).enumerate() {
        let got: Vec<i64> = line
            .split_whitespace()
            .skip(2)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(&got, wanted, "EXIT sample{i}:{line}");
    }
    println!(
        "Ger native EXIT:{profiles} profiles,{} exact wait/gate/save/focus/Title snapshots",
        expected.len()
    );
}
