use super::*;
#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output"]
fn reference_cake_outside_clicks_play_supplied_error_cue() {
    use crate::{
        adv::input_area::PointerSampler,
        asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
        audio::SystemSoundPlayer,
    };
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device,
            wgpu_queue: queue,
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let bank = server
        .load_sync::<shin_core::format::sysse::SysSe>("/sysse.bin")
        .unwrap();
    assert!(bank.sounds.contains_key("error"));
    assert!(!bank.sounds.contains_key("reject"));
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut player = SystemSoundPlayer::new(manager);
    let mut clicks = 0;
    for extent in [(1280, 720), (3840, 2160)] {
        let mut model = CakeModel::default();
        let mut pointer = PointerSampler::default();
        let scale = Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        for point in [
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
            Vec2::new(480.0, 0.0),
            Vec2::new(1440.0, 500.0),
            Vec2::new(960.0, 960.0),
            Vec2::new(1900.0, 1000.0),
        ] {
            assert_eq!(CakeModel::hit_test(point), None);
            for _ in 0..3 {
                pointer.sample(point * scale, true, 0.0);
                model.pointer_frame(&pointer.frame(), extent);
                pointer.sample(point * scale, false, 0.01);
                model.pointer_frame(&pointer.frame(), extent);
                let cues = model.take_sounds();
                assert_eq!(cues, ["error"]);
                for cue in cues {
                    player.play(&server, cue, shin_core::vm::command::types::Volume(1.0));
                }
                assert_eq!(model.result, None);
                assert!(!model.confirming);
                clicks += 1;
            }
        }
        let point = Vec2::new(850.0, 700.0) * scale;
        pointer.sample(point, true, 0.0);
        model.pointer_frame(&pointer.frame(), extent);
        pointer.sample(point, false, 0.01);
        model.pointer_frame(&pointer.frame(), extent);
        assert_eq!(model.take_sounds(), ["decide"]);
        player.play(
            &server,
            "decide",
            shin_core::vm::command::types::Volume(1.0),
        );
        model.update(Ticks::from_f32(7.0));
        model.update(Ticks::ZERO);
        assert_eq!(model.result, Some(0));
    }
    assert_eq!(player.sounds.len(), clicks + 2);
    println!(
        "{clicks} outside/plate mouse clicks play the supplied error cue without completing Cake; valid selection after rejection succeeds at both viewports."
    );
}

#[test]
fn raw_stick_moves_cake_without_synthesized_dpad_step_but_real_dpad_adds_eight_pixels() {
    use crate::app::AppAction;
    use shin_input::{
        Action, RawInputState,
        inputs::{GamepadButton, VirtualGamepadButton},
    };
    let mut raw = RawInputState::new();
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLLeft] = true;
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLDown] = true;
    let state = |raw: &RawInputState| {
        let lowered = AppAction::lower(raw);
        enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
            is_held: lowered[action],
            ..Default::default()
        })
    };
    let mut model = CakeModel::default();
    model.action_input(&state(&raw), Vec2::new(-0.75, 0.75));
    assert_eq!(model.finger, Vec2::new(1431.0, 909.0));
    raw.gamepads.unified.buttons[GamepadButton::Left] = true;
    raw.gamepads.unified.buttons[GamepadButton::Up] = true;
    model.action_input(&state(&raw), Vec2::new(-0.75, 0.75));
    assert_eq!(model.finger, Vec2::new(1414.0, 910.0));
    raw.gamepads.unified.buttons[GamepadButton::Left] = false;
    raw.gamepads.unified.buttons[GamepadButton::Up] = false;
    raw.keyboard.insert(winit::keyboard::KeyCode::ArrowRight);
    model.action_input(&state(&raw), Vec2::ZERO);
    assert_eq!(model.finger, Vec2::new(1422.0, 910.0));
}
#[test]
fn native_hit_map_has_fifteen_slices_and_rejects_plate_and_outside() {
    for (point, index) in [
        (Vec2::new(850.0, 700.0), 0),
        (Vec2::new(960.0, 700.0), 14),
        (Vec2::new(1100.0, 400.0), 12),
        (Vec2::new(1000.0, 100.0), 7),
    ] {
        assert_eq!(CakeModel::hit_test(point), Some(index));
    }
    let mut seen = [false; 15];
    for y in 0..240 {
        for x in 0..240 {
            if let Some(index) =
                CakeModel::hit_test(Vec2::new(482.0 + x as f32 * 4.0, 2.0 + y as f32 * 4.0))
            {
                seen[index] = true;
            }
        }
    }
    assert!(seen.into_iter().all(|value| value));
    for point in [
        Vec2::new(479.0, 500.0),
        Vec2::new(1440.0, 500.0),
        Vec2::new(900.0, 960.0),
        Vec2::new(480.0, 0.0),
        Vec2::new(f32::NAN, 0.0),
    ] {
        assert_eq!(CakeModel::hit_test(point), None);
    }
}
#[test]
fn selection_waits_for_confirmation_and_reload_clears_previous_answer() {
    let mut model = CakeModel::default();
    model.confirm();
    assert_eq!(model.result, None);
    assert_eq!(model.take_sounds(), ["error"]);
    model.navigate(-1);
    assert_eq!(model.finger, Vec2::new(1440.0, 892.0));
    assert_eq!(model.selected, None);
    model.pointer(Vec2::new(850.0, 700.0), false);
    model.update(Ticks::from_f32(4.0));
    assert_eq!(model.focus[0].value(), 1.0);
    model.confirm();
    assert_eq!(model.result, None);
    model.update(Ticks::from_f32(7.0));
    assert_eq!(model.result, None);
    assert_eq!(model.flash[0].value(), 0.0);
    assert!((model.instruction_opacity.value() - 0.3).abs() < 1e-6);
    model.update(Ticks::ZERO);
    assert_eq!(model.result, Some(0));
    model.navigate(5);
    assert_eq!(model.result, Some(0));
    model.reset();
    assert_eq!(model.selected, None);
    assert_eq!(model.result, None);
    model.pointer(Vec2::new(480.0, 0.0), true);
    assert_eq!(model.result, None);
}

#[test]
fn captured_cake_contacts_consume_controls_delay_feedback_and_cancel_on_drag_or_resize() {
    use crate::adv::input_area::PointerSampler;
    for extent in [(1280, 720), (3840, 2160)] {
        let mut model = CakeModel::default();
        let mut pointer = PointerSampler::default();
        let point = Vec2::new(850.0, 700.0);
        let scale = Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        let sample = |model: &mut CakeModel,
                      sampler: &mut PointerSampler,
                      point: Vec2,
                      held,
                      seconds| {
            sampler.sample(point * scale, held, seconds);
            model.pointer_frame(&sampler.frame(), extent);
        };
        sample(&mut model, &mut pointer, point, false, 0.0);
        assert_eq!(
            model.selected, None,
            "Cake has no passive touch-hover callback"
        );
        sample(&mut model, &mut pointer, point, true, 0.0);
        assert_eq!(model.pointer_row, Some(0));
        assert_eq!(model.selected, None);
        assert!(!model.controller_input(15, true, Vec2::ONE));
        sample(&mut model, &mut pointer, point, true, 0.151);
        model.update(Ticks::from_f32(1.0));
        assert_eq!(model.selected, Some(0));
        assert!((model.flash[0].value() - 0.2).abs() < 1e-6);
        sample(&mut model, &mut pointer, point, true, 0.02);
        model.update(Ticks::from_f32(3.0));
        assert_eq!(model.flash[0].value(), 0.7);
        sample(
            &mut model,
            &mut pointer,
            point + Vec2::new(20.0, 0.0),
            true,
            0.01,
        );
        assert_eq!(model.pointer_row, None);
        model.update(Ticks::from_f32(4.0));
        assert_eq!(model.flash[0].value(), 0.0);
        sample(&mut model, &mut pointer, point, false, 0.01);
        assert_eq!(model.result, None);
        assert!(model.take_sounds().is_empty());
        sample(&mut model, &mut pointer, point, true, 0.0);
        sample(
            &mut model,
            &mut pointer,
            Vec2::new(1900.0, 1000.0),
            false,
            0.01,
        );
        assert!(model.confirming);
        assert_eq!(
            model.selected,
            Some(0),
            "Release coordinates do not replace captured row"
        );
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(Ticks::from_f32(7.0));
        assert_eq!(model.result, None);
        model.update(Ticks::ZERO);
        assert_eq!(model.result, Some(0));

        model.reset();
        pointer = PointerSampler::default();
        sample(&mut model, &mut pointer, point, true, 0.0);
        sample(&mut model, &mut pointer, point, true, 0.151);
        model.update(Ticks::from_f32(1.0));
        model.pointer_frame(&pointer.frame(), (1920, 1080));
        assert_eq!(model.pointer_row, None);
        model.update(Ticks::from_f32(1.0));
        assert_eq!(model.flash[0].value(), 0.0);
        assert!(!model.confirming);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "compares production CakeModel with guarded original/1.03 native input and clocks"]
fn reference_native_cake_input_focus_flash_clocks_match_model() {
    use crate::adv::input_area::InputAreaTarget;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary =
        std::env::temp_dir().join(format!("shin-cake-model-native-{}", std::process::id()));
    let output = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-cake-input.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut commands = vec!["N".to_string()];
    for y in 0..240 {
        for x in 0..240 {
            commands.push(format!("P {} {}", 482 + x * 4, 2 + y * 4));
        }
    }
    for point in [
        Vec2::new(850.0, 700.0),
        Vec2::new(960.0, 700.0),
        Vec2::new(1000.0, 100.0),
        Vec2::new(28.0, 38.0),
        Vec2::new(1863.0, 1015.0),
        Vec2::ZERO,
        Vec2::new(2000.0, 1100.0),
    ] {
        for mask in 0..16 {
            for axes in [Vec2::ZERO, Vec2::ONE, Vec2::NEG_ONE, Vec2::new(0.5, -0.5)] {
                commands.extend([
                    "N".to_string(),
                    format!("M {} {}", point.x, point.y),
                    format!("C {mask} 0 {} {}", axes.x, axes.y),
                    "T 1".to_string(),
                ]);
            }
        }
    }
    for point in [
        Vec2::new(850.0, 700.0),
        Vec2::new(960.0, 700.0),
        Vec2::new(1000.0, 100.0),
        Vec2::ZERO,
    ] {
        for held in [0.0, 0.25, 1.0, 2.0, 3.5, 4.0, 10.0] {
            commands.extend([
                "N".to_string(),
                format!("P {} {}", point.x, point.y),
                format!("H {} {}", point.x, point.y),
                format!("T {held}"),
                "R 2000 -50".to_string(),
            ]);
            for ticks in [0.0, 0.25, 0.5, 1.0, 1.75, 3.5, 0.0, 3.0] {
                commands.push(format!("T {ticks}"));
            }
            commands.push("C 15 1 1 1".to_string());
        }
        for cancel in ["X", "B", "D"] {
            commands.extend([
                "N".to_string(),
                format!("P {} {}", point.x, point.y),
                format!("H {} {}", point.x, point.y),
                "T 2".to_string(),
                cancel.to_string(),
                "T 1".to_string(),
                "T 1".to_string(),
                "G".to_string(),
                "L".to_string(),
            ]);
        }
    }
    commands.extend(
        [
            "N",
            "C 15 1 1 1",
            "M 850 700",
            "C 0 0 0 0",
            "T 4",
            "M 960 700",
            "C 0 0 0 0",
            "T 1",
            "T 3",
            "T 16",
            "C 15 1 1 1",
            "T 7",
            "C 15 1 1 1",
            "T 0",
            "C 15 1 1 1",
            "T 3",
        ]
        .map(str::to_string),
    );
    let input = commands.join("\n") + "\n";
    for name in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut child = Command::new(&binary)
            .arg(root.join("game-data/umineko").join(name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Stream input concurrently: the native runner also writes multi-MB output.
        let mut stdin = child.stdin.take().unwrap();
        let payload = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(payload.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.lines().count(), commands.len());
        let mut model = CakeModel::default();
        let (mut decide, mut reject) = (0, 0);
        for (step, (command, line)) in commands.iter().zip(output.lines()).enumerate() {
            let mut parts = command.split_whitespace();
            let op = parts.next().unwrap();
            let args: Vec<f32> = parts.map(|s| s.parse().unwrap()).collect();
            let mut consumed = -1;
            match op {
                "N" => {
                    model.reset();
                    decide = 0;
                    reject = 0;
                }
                "M" => model.finger = Vec2::new(args[0], args[1]),
                "C" => {
                    consumed = model.controller_input(
                        args[0] as u32,
                        args[1] != 0.0,
                        Vec2::new(args[2], args[3]),
                    ) as i32
                }
                "P" => consumed = model.probe(Vec2::new(args[0], args[1])) as i32,
                "H" => model.press(Vec2::new(args[0], args[1])),
                "R" => model.release(Vec2::new(args[0], args[1])),
                "X" => model.cancel(),
                "B" => model.long_press(Vec2::ZERO),
                "D" => model.drag(Vec2::ZERO, Vec2::ZERO),
                "G" => model.gesture(Vec2::ZERO),
                "L" => InputAreaTarget::reset(&mut model),
                "T" => model.update(Ticks::from_f32(args[0])),
                _ => panic!("{command}"),
            }
            for cue in model.take_sounds() {
                match cue {
                    "decide" => decide += 1,
                    "error" => reject += 1,
                    _ => panic!("{cue}"),
                }
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            let integer = |i: usize| fields[i].parse::<i32>().unwrap();
            let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
            let selection = model.selected.map_or(-1, |row| row as i32);
            assert_eq!(
                [
                    integer(1),
                    integer(2),
                    integer(3),
                    integer(4),
                    integer(5),
                    integer(8),
                    integer(9),
                    integer(10)
                ],
                [
                    selection,
                    if model.result.is_some() {
                        -1
                    } else if model.confirming {
                        1
                    } else {
                        0
                    },
                    model.pointer_row.map_or(-1, |row| row as i32),
                    model.result.is_some() as i32,
                    selection,
                    consumed,
                    decide,
                    reject
                ],
                "{name} step{step} {command}"
            );
            let mut expected = vec![model.finger.x, model.finger.y];
            let mut actual = vec![scalar(6), scalar(7)];
            for row in 0..15 {
                expected.extend([model.focus[row].value(), model.flash[row].value()]);
                actual.extend([scalar(11 + row * 2), scalar(12 + row * 2)]);
            }
            expected.push(model.instruction_opacity.value());
            actual.push(scalar(41));
            for (field, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
                assert!(
                    (actual - expected).abs() < 2e-6,
                    "{name} step{step} {command} field{field}: {actual} vs {expected}"
                );
            }
        }
        eprintln!(
            "{name}:{} production/native Cake command rows, phase/getter/captured row/cues/finger and31native tween values match",
            commands.len()
        );
    }
    std::fs::remove_file(binary).unwrap();
}
