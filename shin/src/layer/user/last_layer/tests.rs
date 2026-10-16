use super::*;
#[test]
fn controller_priorities_match_native_door_state_and_ignore_cancel() {
    use crate::app::AppAction;
    for choice in [None, Some(0), Some(1)] {
        for bits in 0..8 {
            let mut model = LastModel::default();
            if let Some(choice) = choice {
                model.navigate(if choice == 0 { -1 } else { 1 });
                model.update(7.0);
                model.update(20.0);
                model.take_sounds();
            }
            let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
            input[AppAction::Left].is_clicked_or_repeated = bits & 1 != 0;
            input[AppAction::Right].is_clicked_or_repeated = bits & 2 != 0;
            input[AppAction::Enter].is_clicked = bits & 4 != 0;
            input[AppAction::Cancel].is_clicked = true;
            model.action_input(&input);
            let expected = match choice {
                None if bits & 1 != 0 => Some(0),
                None if bits & 2 != 0 => Some(1),
                Some(_) if bits & 4 != 0 => choice,
                Some(0) if bits & 2 != 0 => None,
                Some(1) if bits & 1 != 0 => None,
                _ => choice,
            };
            assert_eq!(model.choice, expected);
            assert_eq!(model.confirming, choice.is_some() && bits & 4 != 0);
            let cues = model.take_sounds();
            assert_eq!(
                cues,
                if model.confirming {
                    vec!["decide"]
                } else if expected != choice {
                    vec!["move"]
                } else {
                    vec![]
                }
            );
            if model.confirming {
                model.update(7.0);
                assert_eq!(model.result, choice.map(|c| c as i32));
            }
        }
    }
    // Hover is a desktop pointer convenience, not a native controller choice.
    let mut model = LastModel::default();
    model.pointer(Vec2::new(400.0, 900.0), false);
    model.controller_input(false, false, true);
    assert_eq!(model.choice, None);
}
fn captured_sample(
    model: &mut LastModel,
    sampler: &mut crate::adv::input_area::PointerSampler,
    extent: (u16, u16),
    point: Vec2,
    held: bool,
    seconds: f64,
) {
    sampler.sample(
        point * Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
        held,
        seconds,
    );
    model.pointer_frame(&sampler.frame(), extent);
}
#[test]
fn captured_last_taps_consume_controls_hold_drag_resize_and_retained_up_position() {
    for extent in [(960, 540), (1920, 1080), (3840, 2160)] {
        for choice in 0..2 {
            let mut model = LastModel::default();
            let mut sampler = crate::adv::input_area::PointerSampler::default();
            let rect = CHOICES[choice];
            let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.0);
            model.controller_input(true, true, true);
            assert_eq!(model.choice, None);
            assert_eq!(model.pointer_row, Some(choice));
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.149);
            assert_eq!(model.choice_flashes[choice].value, 0.0);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.002);
            model.update(1.0);
            assert!((model.choice_flashes[choice].value - 0.2).abs() < 1e-6);
            captured_sample(
                &mut model,
                &mut sampler,
                extent,
                point + Vec2::new(20.0, 0.0),
                true,
                0.01,
            );
            model.update(1.0);
            assert_eq!(model.pointer_row, None);
            assert_eq!(model.choice_flashes[choice].value, 0.0);
            assert!(model.take_sounds().is_empty());
            captured_sample(&mut model, &mut sampler, extent, point, false, 0.01);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
            captured_sample(
                &mut model,
                &mut sampler,
                extent,
                Vec2::new(1900.0, 1000.0),
                false,
                0.1,
            );
            assert_eq!(model.choice, Some(choice));
            assert_eq!(model.pointer_row, None);
            assert_eq!(model.take_sounds(), ["move"]);
            model.update(7.0);
            model.update(20.0);
            let rect = confirmation_rects(choice)[1];
            let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
            model.controller_input(choice == 1, choice == 0, true);
            assert!(!model.confirming);
            assert_eq!(model.choice, Some(choice));
            captured_sample(&mut model, &mut sampler, extent, point, false, 0.1);
            assert!(model.confirming);
            assert_eq!(model.take_sounds(), ["decide"]);
            model.update(7.0);
            assert_eq!(model.result, Some(choice as i32));
        }
        let mut model = LastModel::default();
        let mut sampler = crate::adv::input_area::PointerSampler::default();
        let point = Vec2::new(400.0, 900.0);
        captured_sample(&mut model, &mut sampler, extent, point, true, 0.0);
        model.disable_pointer();
        assert_eq!(model.pointer_row, None);
        captured_sample(&mut model, &mut sampler, extent, point, true, 0.2);
        assert_eq!(
            model.pointer_row, None,
            "Already held pointer cannot synthesize a fresh Down"
        );
        captured_sample(&mut model, &mut sampler, extent, point, false, 0.01);
        assert_eq!(model.choice, None);
        captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
        captured_sample(&mut model, &mut sampler, (1280, 720), point, true, 0.2);
        if extent != (1280, 720) {
            assert_eq!(model.pointer_row, None, "Resize cancels old capture");
        }
        assert_eq!(model.choice, None);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original/1.03 Last controller/capture and seven native tween clocks"]
fn reference_native_last_input_and_pending_clocks_match_production_model() {
    use crate::adv::input_area::InputAreaTarget;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary =
        std::env::temp_dir().join(format!("shin-last-input-oracle-{}", std::process::id()));
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-last-input.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut commands = Vec::<String>::new();
    let start = |commands: &mut Vec<String>, choice: Option<usize>| {
        commands.push("N".to_owned());
        if let Some(choice) = choice {
            commands.extend([
                format!(
                    "C {} {} 0",
                    usize::from(choice == 0),
                    usize::from(choice == 1)
                ),
                "T 7".to_owned(),
                "T 20".to_owned(),
            ]);
        }
    };
    for choice in [None, Some(0), Some(1)] {
        for bits in 0..8 {
            for tick in [
                0.0, 0.125, 0.25, 0.75, 1.0, 2.5, 3.5, 6.99, 7.0, 20.0, 1000.0,
            ] {
                start(&mut commands, choice);
                commands.push(format!(
                    "C {} {} {}",
                    bits & 1,
                    (bits >> 1) & 1,
                    (bits >> 2) & 1
                ));
                commands.push(format!("T {tick}"));
                commands.extend(
                    [
                        "C 1 1 1", "T 0", "T 0.25", "T 3.25", "T 3.5", "T 20", "C 1 1 1",
                        "T 7", "T 20", "C 1 1 1", "T 7", "T 0",
                    ]
                    .map(str::to_owned),
                );
            }
        }
        let rects = match choice {
            None => CHOICES.to_vec(),
            Some(c) => confirmation_rects(c).to_vec(),
        };
        for rect in &rects {
            start(&mut commands, choice);
            for p in [
                Vec2::new(rect.x, rect.y),
                Vec2::new(rect.x + rect.z, rect.y),
                Vec2::new(rect.x, rect.y + rect.w),
                Vec2::new(rect.x - 0.001, rect.y),
                Vec2::new(rect.x, rect.y - 0.001),
                Vec2::new(rect.x + rect.z - 0.001, rect.y + rect.w - 0.001),
                Vec2::new(1900.0, 1079.0),
            ] {
                commands.push(format!("P {} {}", p.x, p.y));
            }
        }
        for rect in rects {
            let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
            for held in [0.0, 0.25, 1.0, 3.49, 3.5, 7.0, 20.0] {
                for callback in ["R 1900 1079", "X", "B", "D", "L"] {
                    start(&mut commands, choice);
                    commands.extend([
                        format!("P {} {}", point.x, point.y),
                        "H 1900 1079".to_owned(),
                        format!("T {held}"),
                        callback.to_owned(),
                        "L".to_owned(),
                        "G".to_owned(),
                    ]);
                    commands.extend(
                        [
                            "T 0", "T 0.25", "T 1", "T 2.5", "T 3.5", "T 7", "T 20", "C 1 1 1",
                            "T 7", "T 20",
                        ]
                        .map(str::to_owned),
                    );
                }
            }
        }
    }
    // Native delayed base clocks freeze whenever a choice/back flash is busy,
    // even if a single oversized update completes that flash.
    for choice in 0..2 {
        commands.push("N".to_owned());
        commands.push(format!(
            "C {} {} 0",
            usize::from(choice == 0),
            usize::from(choice == 1)
        ));
        commands.extend((0..116).map(|_| "T 0.25".to_owned()));
        commands.push("C 1 1 0".to_owned());
        commands.extend((0..116).map(|_| "T 0.25".to_owned()));
    }
    let payload = commands.join("\n") + "\n";
    let folder = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .parent()
        .unwrap()
        .to_owned();
    for name in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut child = Command::new(&binary)
            .arg(folder.join(name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = payload.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{name}:{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.lines().count(), commands.len());
        let mut model = LastModel::default();
        let (mut decide, mut moves) = (0, 0);
        for (step, (command, line)) in commands.iter().zip(output.lines()).enumerate() {
            let mut parts = command.split_whitespace();
            let op = parts.next().unwrap();
            let args: Vec<f32> = parts.map(|s| s.parse().unwrap()).collect();
            let mut consumed = -1;
            match op {
                "N" => {
                    model = LastModel::default();
                    decide = 0;
                    moves = 0;
                }
                "C" => {
                    model.controller_input(args[0] != 0.0, args[1] != 0.0, args[2] != 0.0);
                    consumed = 1;
                }
                "P" => consumed = model.probe(Vec2::new(args[0], args[1])) as i32,
                "H" => model.press(Vec2::new(args[0], args[1])),
                "R" => model.release(Vec2::new(args[0], args[1])),
                "X" => model.cancel(),
                "B" => model.long_press(Vec2::ZERO),
                "D" => model.drag(Vec2::ZERO, Vec2::ZERO),
                "L" => InputAreaTarget::reset(&mut model),
                "G" => model.gesture(Vec2::ZERO),
                "T" => model.update(args[0]),
                _ => panic!("{command}"),
            }
            for cue in model.take_sounds() {
                match cue {
                    "decide" => decide += 1,
                    "move" => moves += 1,
                    _ => panic!("{cue}"),
                }
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 24);
            let integer = |i: usize| fields[i].parse::<i32>().unwrap();
            let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
            let choice = model.choice.map_or(-1, |c| c as i32);
            assert_eq!(
                [
                    integer(1),
                    integer(2),
                    integer(3),
                    integer(4),
                    integer(5),
                    integer(6),
                    integer(7),
                    integer(8)
                ],
                [
                    choice,
                    if model.result.is_some() {
                        -1
                    } else if model.confirming {
                        1
                    } else {
                        0
                    },
                    model.pointer_row.map_or(-1, |r| r as i32),
                    model.result.is_some() as i32,
                    choice,
                    consumed,
                    decide,
                    moves
                ],
                "{name} step{step} {command}"
            );
            assert!(
                (scalar(9) - model.phase).abs() < 2e-6,
                "{name} step{step} phase {} vs {}",
                scalar(9),
                model.phase
            );
            let motions = [
                &model.slide,
                &model.front,
                &model.door,
                &model.choice_flashes[0],
                &model.choice_flashes[1],
                &model.back_flash,
                &model.confirm_flash,
            ];
            for (field, motion) in motions.into_iter().enumerate() {
                let actual = scalar(10 + field);
                let tolerance = if field == 0 { 0.0002 } else { 2e-6 };
                assert!(
                    (actual - motion.value).abs() < tolerance,
                    "{name} step{step} {command} field{field}:native{actual} port{}",
                    motion.value
                );
                assert_eq!(
                    integer(17 + field),
                    motion.busy() as i32,
                    "{name} step{step} {command} busy{field}"
                );
            }
        }
        eprintln!(
            "{name}:{} Last native/production command steps match choice/phase/capture/getter/cues/seven values andbusy flags",
            commands.len()
        );
    }
    std::fs::remove_file(binary).unwrap();
}

#[test]
fn completed_choice_restore_preserves_visuals_without_replaying_input() {
    use super::super::cake_layer::CakeModel;
    for answer in 0..15 {
        let mut cake = CakeModel::default();
        cake.restore_completed_answer(answer);
        cake.navigate(1);
        cake.confirm();
        cake.pointer(Vec2::new(960.0, 700.0), true);
        assert_eq!(cake.selected, None);
        assert_eq!(cake.visual_slice(), Some(answer as usize));
        assert_eq!(cake.result, Some(-1));
        cake.reset();
        assert_eq!(cake.visual_slice(), None);
        assert_eq!(cake.result, None);
    }
    for answer in [0, 1] {
        let mut model = LastModel::default();
        model.restore_completed_answer(answer);
        model.navigate(1);
        model.confirm();
        model.back();
        model.pointer(Vec2::new(960.0, 300.0), true);
        model.update(120.0);
        assert_eq!(model.choice, None);
        assert_eq!(model.result, Some(-1));
        assert_eq!(model.slide.value, if answer == 0 { 960.0 } else { -960.0 });
        assert_eq!(model.door.value, 0.0);
        assert!(!model.accepts_input());
        assert!(model.take_sounds().is_empty());
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 Cake and Last completed-save callbacks"]
fn reference_native_completed_cake_and_last_restoration() {
    use super::super::cake_layer::CakeModel;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary =
        std::env::temp_dir().join(format!("shin-choice-restore-oracle-{}", std::process::id()));
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-completed-choice-restoration.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let folder = std::path::Path::new(&rom).parent().unwrap();
    let mut requests = String::new();
    for prior in -1..15 {
        for answer in (-1..=15).chain([99]) {
            requests += &format!("1 {answer} {prior}\n");
        }
    }
    for prior in -1..2 {
        for answer in [-1, 0, 1, 2, 99] {
            requests += &format!("3 {answer} {prior}\n");
        }
    }
    for name in ["uncompressed_executable", "uncompressed_executable_update"] {
        let exe = folder.join(name);
        let mut child = Command::new(&binary)
            .arg(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(requests.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.lines().count(), 303);
        for line in output.lines() {
            let values: Vec<f32> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            let answer = values[1] as i32;
            let selected = (values[2] >= 0.0).then_some(values[2] as usize);
            if values[0] == 1.0 {
                let mut model = CakeModel::default();
                model.selected = selected;
                model.restore_completed_answer(answer);
                assert_eq!(model.selected, selected);
                assert_eq!(model.result, Some(values[3] as i32));
                assert_eq!(
                    model.reference_tween_values().as_slice(),
                    &values[4..],
                    "{name}: {line}"
                );
            } else {
                let mut model = LastModel::default();
                model.choice = selected;
                // Exercise native reset of pre-existing idle values as well.
                model.slide.value = 0.25;
                model.front.value = 0.25;
                model.door.value = 0.25;
                model.restore_completed_answer(answer);
                assert_eq!(model.choice, selected);
                assert_eq!(model.result, Some(values[3] as i32));
                let actual = [
                    model.slide.value,
                    model.front.value,
                    model.door.value,
                    model.choice_flashes[0].value,
                    model.choice_flashes[1].value,
                    model.back_flash.value,
                    model.confirm_flash.value,
                ];
                assert_eq!(&actual, &values[4..], "{name}: {line}");
                assert!(
                    model.slide.queue.is_empty()
                        && model.front.queue.is_empty()
                        && model.door.queue.is_empty()
                );
                assert!(
                    model
                        .choice_flashes
                        .iter()
                        .all(|flash| flash.queue.is_empty())
                );
                assert!(
                    model.back_flash.queue.is_empty() && model.confirm_flash.queue.is_empty()
                );
            }
        }
        let bytes = std::fs::read(&exe).unwrap();
        let delta = if name.ends_with("update") { 0xa0 } else { 0 };
        for (address, words) in [
            (
                0x44e98,
                &[
                    0x91012260u32,
                    0x9401d981,
                    0x4ea01c08,
                    0x1e202008,
                    0x5400088d,
                ][..],
            ),
            (0x44448, &[0x12800008u32, 0xf9005268, 0xb900aa68][..]),
            (0x6daa4, &[0xb2607fe8u32, 0xf9014a68, 0xf9014e68][..]),
        ] {
            for (offset, &word) in words.iter().enumerate() {
                let address = address - delta + offset * 4 + 256;
                assert_eq!(
                    u32::from_le_bytes(bytes[address..address + 4].try_into().unwrap()),
                    word,
                    "{name}: native message/pointer draw gate or constructor retained-selection sentinel changed"
                );
            }
        }
        for address in [0x45330, 0x6f930, 0xbb2e0] {
            let address = if name.ends_with("update") {
                address - 0xa0
            } else {
                address
            };
            let mut bytes = std::fs::read(&exe).unwrap();
            bytes[address + 256] ^= 1;
            let altered = std::env::temp_dir()
                .join(format!("shin-choice-altered-{}", std::process::id()));
            std::fs::write(&altered, bytes).unwrap();
            let rejected = Command::new(&binary).arg(&altered).output().unwrap();
            std::fs::remove_file(altered).unwrap();
            assert_eq!(rejected.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&rejected.stderr).contains("Unreviewed"));
        }
        eprintln!(
            "{name}:303 native Cake/Last completed restore cases, retained selection/result, 31 Cake and seven Last tween values; altered callbacks/reset rejected before execution"
        );
    }
    std::fs::remove_file(binary).unwrap();
}

#[test]
fn two_stage_native_hits_confirm_original_response_and_back_without_answer() {
    for choice in 0..2 {
        let mut model = LastModel::default();
        model.confirm();
        assert_eq!(model.choice, None);
        let rect = CHOICES[choice];
        model.pointer(Vec2::new(rect.x + 100.0, 900.0), true);
        assert_eq!(model.choice, Some(choice));
        assert_eq!(model.result, None);
        model.update(7.0);
        model.update(20.0);
        let back = confirmation_rects(choice)[2];
        model.pointer(Vec2::new(back.x + 100.0, 900.0), true);
        assert_eq!(model.choice, None);
        assert_eq!(model.result, None);
        model.update(7.0);
        model.update(20.0);
        model.pointer(Vec2::new(rect.x + 100.0, 900.0), true);
        model.update(7.0);
        model.update(20.0);
        model.pointer(Vec2::new(960.0, 300.0), true);
        assert_eq!(model.result, None);
        model.update(7.0);
        assert_eq!(model.result, Some(choice as i32));
        model.back();
        model.navigate(1);
        model.confirm();
        assert_eq!(model.result, Some(choice as i32));
    }
}
#[test]
fn invalid_hits_and_keyboard_require_selection_before_confirming() {
    let mut model = LastModel::default();
    for point in [
        Vec2::ZERO,
        Vec2::new(f32::NAN, 900.0),
        Vec2::new(896.0, 900.0),
    ] {
        model.pointer(point, true);
        assert_eq!(model.choice, None);
    }
    model.navigate(-1);
    assert_eq!(model.choice, Some(0));
    assert_eq!(model.result, None);
    model.update(7.0);
    model.update(20.0);
    model.confirm();
    assert_eq!(model.result, None);
    model.update(7.0);
    assert_eq!(model.result, Some(0));
}
#[test]
fn slide_waits_for_flash_locks_input_and_confirmation_finishes_after_flash() {
    let mut model = LastModel::default();
    model.navigate(-1);
    model.confirm();
    model.back();
    model.navigate(1);
    assert_eq!(model.choice, Some(0));
    assert_eq!(model.result, None);
    model.update(3.5);
    assert_eq!(model.slide.value, 0.0);
    assert!((model.choice_flashes[0].value - 0.7).abs() < 1e-5);
    model.update(3.5);
    assert_eq!(model.slide.value, 0.0);
    model.update(10.0);
    assert!((model.slide.value - 960.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    assert_eq!(model.front.value, 0.5);
    assert_eq!(model.door.value, 0.5);
    model.pointer(Vec2::new(960.0, 300.0), true);
    assert!(!model.confirming);
    model.update(10.0);
    model.confirm();
    assert!(model.confirming);
    model.update(5.0);
    assert_eq!(model.door.value, 0.0);
    assert_eq!(model.result, None);
    model.pointer(Vec2::new(1700.0, 900.0), true);
    model.back();
    assert_eq!(model.choice, Some(0));
    model.update(1.99);
    assert_eq!(model.result, None);
    model.update(0.01);
    assert_eq!(model.result, Some(0));
}
#[test]
fn animation_rejects_nonfinite_time_and_back_reverses_slide() {
    let mut model = LastModel::default();
    model.navigate(1);
    model.update(f32::NAN);
    model.update(-1.0);
    model.update(f32::INFINITY);
    assert_eq!(model.slide.value, 0.0);
    model.update(7.0);
    model.update(20.0);
    assert_eq!(model.slide.value, -960.0);
    model.back();
    model.update(7.0);
    assert_eq!(model.slide.value, -960.0);
    model.update(10.0);
    assert_eq!(model.front.value, 0.5);
    model.update(10.0);
    assert_eq!(model.slide.value, 0.0);
    assert_eq!(model.front.value, 1.0);
    assert_eq!(model.door.value, 0.0);
    assert_eq!(model.result, None);
    model.navigate(-1);
    assert_eq!(model.choice, Some(0));
}
#[test]
fn hover_uses_native_speed_and_click_finishes_from_current_brightness() {
    let mut model = LastModel::default();
    model.pointer(Vec2::new(400.0, 900.0), false);
    model.update(1.0);
    assert!((model.choice_flashes[0].value - 0.2).abs() < 1e-5);
    model.update(2.5);
    assert_eq!(model.choice_flashes[0].value, 0.7);
    model.pointer(Vec2::new(400.0, 900.0), true);
    model.update(3.5);
    assert_eq!(model.slide.value, 0.0);
    model.update(20.0);
    model.pointer(Vec2::new(960.0, 300.0), false);
    model.update(3.5);
    assert_eq!(model.confirm_flash.value, 0.7);
    model.pointer(Vec2::new(960.0, 300.0), true);
    model.update(3.49);
    assert_eq!(model.result, None);
    model.update(0.01);
    assert_eq!(model.result, Some(0));
}
#[test]
fn native_sound_events_follow_accepted_actions_and_ignore_hover_and_locked_input() {
    let mut model = LastModel::default();
    model.pointer(Vec2::new(400.0, 900.0), false);
    assert!(model.take_sounds().is_empty());
    model.pointer(Vec2::new(400.0, 900.0), true);
    assert_eq!(model.take_sounds(), ["move"]);
    model.confirm();
    model.back();
    model.navigate(1);
    assert!(model.take_sounds().is_empty());
    model.update(7.0);
    model.update(20.0);
    model.back();
    assert_eq!(model.take_sounds(), ["move"]);
    model.update(7.0);
    model.update(20.0);
    model.navigate(1);
    model.update(7.0);
    model.update(20.0);
    model.confirm();
    assert_eq!(model.take_sounds(), ["move", "decide"]);
    model.update(7.0);
    model.confirm();
    model.back();
    assert!(model.take_sounds().is_empty());
}
