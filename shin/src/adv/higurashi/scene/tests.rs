use super::*;
use shin_core::format::scenario::instruction_elements::UntypedNumberArray;

#[test]
fn mask_wipe_without_asset_services_does_not_publish_back_records() {
    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    layers
        .state
        .load(1, 0, 0, UntypedNumberArray::default())
        .unwrap();
    layers.state.start_back(1).unwrap();
    layers.state.control(1, 0, 123, 0).unwrap();
    let mut scene = HigurashiScene::new(layers);
    let wipe = |kind, duration| HigurashiRuntimeCommand::WIPE {
        args: [kind, 0, duration],
        params: UntypedNumberArray::default(),
    };
    assert!(scene.dispatch(&wipe(1, 60), false).is_err());
    assert!(!scene.command_pending());
    assert_eq!(scene.layers().state.target(1).unwrap().bank, 1);
    // The native signed-duration guard precedes the effect-type switch.
    assert_eq!(
        scene.dispatch(&wipe(4, 0), false).unwrap(),
        Some(CommandStatus::Complete)
    );
    assert_eq!(scene.layers().state.target(1).unwrap().bank, 0);
    assert!(scene.transition().is_none());
}
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "executes updated AArch64 WIPE callbacks and root lifecycle"]
fn higurashi_native_scene_wipe_wait_and_retirement() {
    let fixture = std::env::var("SHIN_HIGURASHI_NATIVE_SCENE").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut case = |first_duration: i32,
                    first_flags: u32,
                    second_duration: i32,
                    second_flags: u32,
                    skip: bool| {
        let mut scene = HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4]));
        let mut groups = [0, 1, 2];
        let mut phase = 0;
        let mut allocations = 0;
        let mut frees = 0;
        for line in [
            "N".to_owned(),
            format!("C {first_duration} {first_flags} 0"),
            "W".into(),
            "U 40a00000 0 0".into(),
            "U 40a00000 1 1".into(),
            "U c0000000 1 0".into(),
            "P".into(),
            format!("C {second_duration} {second_flags} {}", u8::from(skip)),
            "P".into(),
            "U 40a00000 1 0".into(),
            "P".into(),
            "F".into(),
            "W".into(),
            "U 00000000 0 0".into(),
            "W".into(),
            "U 00000000 1 0".into(),
            "P".into(),
            "W".into(),
            "F".into(),
            "W".into(),
            "U 00000000 1 0".into(),
            "P".into(),
            "W".into(),
        ] {
            let words: Vec<_> = line.split_whitespace().collect();
            let was_busy = scene.transition().is_some();
            let mut events = Vec::new();
            let result = match words[0] {
                "N" => 0,
                "C" => {
                    // Model the native script contract: a new command arrives
                    // only after bit0 releases the previous command.
                    assert!(!scene.command_pending());
                    phase = 0;
                    let duration = words[1].parse().unwrap();
                    let flags = words[2].parse::<u32>().unwrap() as i32;
                    let status = scene
                        .dispatch(
                            &HigurashiRuntimeCommand::WIPE {
                                args: [0, flags, duration],
                                params: UntypedNumberArray::default(),
                            },
                            words[3] == "1",
                        )
                        .unwrap()
                        .unwrap();
                    if !was_busy {
                        phase = 2;
                        groups.rotate_left(1);
                    }
                    u32::from(status == CommandStatus::Complete)
                }
                "P" => {
                    let status = scene.poll_command(skip);
                    if phase == 0 && !was_busy {
                        phase = 2;
                        groups.rotate_left(1);
                    }
                    u32::from(status == CommandStatus::Complete)
                }
                "W" => u32::from(scene.transition().is_none()),
                "U" => {
                    let delta = f32::from_bits(u32::from_str_radix(words[1], 16).unwrap());
                    let animate = words[2] == "1";
                    let paused = words[3] == "1";
                    scene.advance(Ticks::from_f32(delta), animate, paused, None);
                    if animate && !paused {
                        events.extend(groups);
                    }
                    0
                }
                "F" => {
                    scene.fast_forward_wiper();
                    events.extend(groups);
                    0
                }
                _ => unreachable!(),
            };
            let busy = scene.transition().is_some();
            if !was_busy && busy {
                allocations += 1;
            }
            if was_busy && !busy {
                frees += 1;
            }
            input.push_str(&line);
            input.push('\n');
            expected.push((
                words[0].to_owned(),
                result,
                phase,
                busy,
                scene.transition().map(|t| t.progress().to_bits()),
                events,
                allocations,
                frees,
            ));
        }
        assert!(!scene.command_pending() && scene.transition().is_none());
    };
    // First bit0 permits a second command while its root wiper is still live.
    // Later bit1 alone must not release the second command.
    for duration in [1, 2, 5, 60, 1000, i32::MAX] {
        for first_flags in [1, 3, 0x8000_0001] {
            for second_flags in [0, 1, 2, 3, 0x8000_0000] {
                for second_duration in [-60, 0, 1, 60, i32::MAX] {
                    for skip in [false, true] {
                        case(duration, first_flags, second_duration, second_flags, skip);
                    }
                }
            }
        }
    }
    let mut child = Command::new(fixture)
        .arg(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        stdin.write_all(input.as_bytes()).unwrap();
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), expected.len());
    for (row, (line, expected)) in stdout.lines().zip(expected).enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let (op, result, phase, busy, progress, events, allocations, frees) = expected;
        assert_eq!(fields[0], op, "row{row}");
        let number = |i: usize| fields[i].parse::<u32>().unwrap();
        assert_eq!(
            (number(1), number(2), number(3), number(4)),
            (result, phase, 1, u32::from(busy)),
            "row{row}: {line}"
        );
        let float = |i| f32::from_bits(u32::from_str_radix(fields[i], 16).unwrap());
        assert_eq!(
            busy.then(|| (float(5) / float(6)).to_bits()),
            progress,
            "row{row}"
        );
        let count = number(7) as usize;
        assert_eq!(
            (8..8 + count).map(number).collect::<Vec<_>>(),
            events,
            "row{row}"
        );
        assert_eq!(
            (number(8 + count), number(9 + count)),
            (allocations, frees),
            "row{row}"
        );
        assert_eq!(
            (number(10 + count), number(11 + count)),
            (u32::from(busy), frees),
            "row{row}: outgoing resource release"
        );
    }
    println!(
        "900 native WIPE sequences and 20700 callback/update observations match; prior waits, bit0, gates, skip ownership and same-update retirement pass"
    );
}
