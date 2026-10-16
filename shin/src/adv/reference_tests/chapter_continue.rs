//! User-save regression through actual pause-menu Load and title-return callbacks.
use crate::adv::*;
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires copied progress, supplied ROM, Metal and exact Loopback Audio"]
fn reference_chapter_prompt_and_reloaded_continue() {
    assert!(std::env::var_os("SHIN_REFERENCE_CHAPTER_CONTINUE_ONLY").is_some());
    std::thread::Builder::new()
        .name("chapter-continue-reference".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(crate::adv::save_menu::render_reference::reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}
fn submit(ctx: &mut PreRenderContext) {
    let encoder = std::mem::replace(
        ctx.encoder,
        ctx.device.create_command_encoder(&Default::default()),
    );
    let mut upload = ctx.device.create_command_encoder(&Default::default());
    ctx.dynamic_buffer.finish(&mut upload);
    ctx.queue.submit([upload.finish(), encoder.finish()]);
    ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}
fn input(action: Option<AppAction>) -> enum_map::EnumMap<AppAction, shin_input::ActionState> {
    let mut input = enum_map::EnumMap::default();
    if let Some(action) = action {
        input[action] = shin_input::ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    input
}
fn step<'a>(
    live: &mut Adv,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    frame: &mut FrameId,
    action: Option<AppAction>,
    ticks: f32,
) {
    frame.advance();
    let input = input(action);
    live.handle_input(input.clone(), true);
    live.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(ticks),
            asset_server: server,
            pre_render: ctx,
        },
        input,
    );
    submit(ctx);
}
fn load_ready<'a>(
    live: &mut Adv,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while live.is_loading_saved_scene() {
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(30.0),
            asset_server: server,
            pre_render: ctx,
        });
        submit(ctx);
        assert!(Instant::now() < deadline, "scene load timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        live.saved_scene_load_error().is_none(),
        "{:?}",
        live.saved_scene_load_error()
    );
}
pub(crate) fn verify<'a>(
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    let source = std::env::var("SHIN_REFERENCE_CHAPTER_CONTINUE_SAVE").unwrap();
    assert!(Path::new(&source).starts_with(std::env::temp_dir()));
    let original = std::fs::read(&source).unwrap();
    let mut globals = SaveData::decode(&original).unwrap();
    globals.save_menu_position = 0; // Select the user's first manual slot in the copied fixture.
    let second = globals.manual_save_slots[0].as_ref().unwrap().clone();
    assert_eq!(
        (second.entry.scenario_id, second.entry.save_position),
        (7, 3420)
    );
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    let origin = std::env::var("SHIN_REFERENCE_CHAPTER_CONTINUE_ORIGIN").unwrap();
    assert!(matches!(origin.as_str(), "Continue" | "Load" | "Both"));
    for (name, first) in [
        ("Continue", globals.auto_save_slot.as_ref().unwrap()),
        ("Load", globals.manual_save_slots[4].as_ref().unwrap()),
    ] {
        if origin != "Both" && name != origin {
            continue;
        }
        let path = Path::new(&source).with_file_name(format!("{name}-regression.dat"));
        std::fs::write(&path, globals.encode()).unwrap();
        let mut live = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        live.configure_save_file(path.clone(), None).unwrap();
        live.load_game_data(&globals, first, server.clone())
            .unwrap();
        load_ready(&mut live, ctx, server, &mut frame);
        assert_eq!(live.vm_state.save_position, first.entry.save_position);
        assert_eq!(
            serde_json::to_value(&live.vm_state.auto_save_slot.as_ref().unwrap().entry).unwrap(),
            serde_json::to_value(&first.entry).unwrap(),
            "First loaded boundary becomes Continue without advancing"
        );
        if name == "Continue" {
            // Measure the actual late Episode8 source; the discarded full
            // replay is retained as a counterfactual timing comparison only.
            let started = Instant::now();
            let canonical = reconstruction::ReconstructedScene::next_chapter_game_data(
                &assets.scenario,
                first,
                &globals,
                1_000_000,
            )
            .unwrap();
            let replay = started.elapsed();
            assert_eq!(live.has_next_chapter().unwrap(), canonical.is_some());
            for direction in [
                chapter_navigation::Direction::Next,
                chapter_navigation::Direction::Previous,
            ] {
                live.pending_chapter = Some(direction);
                let started = Instant::now();
                live.update_chapter_navigation(
                    &mut UpdateContext {
                        frame_started_at: Instant::now(),
                        frame_id: frame,
                        delta_ticks: Ticks::ZERO,
                        asset_server: server,
                        pre_render: ctx,
                    },
                    &Default::default(),
                );
                let elapsed = started.elapsed();
                assert!(
                    live.chapter_dialog.is_some(),
                    "Both prompts exist on their first update"
                );
                assert!(
                    live.pending_restore.is_none(),
                    "Opening a prompt must not install a target"
                );
                submit(ctx);
                live.chapter_dialog = None;
                eprintln!("{direction:?} prompt={elapsed:?}; old discarded next replay={replay:?}");
            }
            if std::env::var_os("SHIN_REFERENCE_CHAPTER_AVAILABILITY_ONLY").is_some() {
                assert_eq!(std::fs::read(&source).unwrap(), original);
                println!(
                    "User-save chapter availability and first-update prompt timing pass; save/menu paths not repeated."
                );
                return;
            }
        }
        // Open pause, choose Load, confirm the first manual slot through the
        // same menu callbacks as manual play. Do not advance the saved message.
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::SystemMenu),
            0.0,
        );
        step(&mut live, ctx, server, &mut frame, None, 30.0);
        assert!(live.system_menu.is_some());
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::AnyDown),
            0.0,
        );
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Enter),
            0.0,
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            step(&mut live, ctx, server, &mut frame, None, 30.0);
            if live
                .system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .is_some_and(|s| s.model().phase() == save_menu::SaveMenuPhase::Active)
            {
                break;
            }
            assert!(Instant::now() < deadline, "pause Load did not open");
            std::thread::sleep(Duration::from_millis(1));
        }
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Enter),
            0.0,
        );
        loop {
            step(&mut live, ctx, server, &mut frame, None, 30.0);
            if live
                .system_menu
                .as_mut()
                .unwrap()
                .reference_save_menu()
                .unwrap()
                .reference_dialog_is_open()
            {
                break;
            }
            assert!(Instant::now() < deadline, "load confirmation did not open");
        }
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Left),
            0.0,
        );
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Enter),
            0.0,
        );
        while live.pending_restore.is_none() {
            step(&mut live, ctx, server, &mut frame, None, 30.0);
            assert!(Instant::now() < deadline, "load not queued");
        }
        load_ready(&mut live, ctx, server, &mut frame);
        assert_eq!(live.vm_state.save_position, second.entry.save_position);
        assert_eq!(
            serde_json::to_value(&live.vm_state.auto_save_slot.as_ref().unwrap().entry).unwrap(),
            serde_json::to_value(&second.entry).unwrap(),
            "Second load replaces Continue immediately"
        );
        // Open pause again, wrap Up to Return to Title, accept Yes and wait
        // for the real immutable save worker and title factory handoff.
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::SystemMenu),
            0.0,
        );
        step(&mut live, ctx, server, &mut frame, None, 30.0);
        step(&mut live, ctx, server, &mut frame, Some(AppAction::Up), 0.0);
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Enter),
            0.0,
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            step(&mut live, ctx, server, &mut frame, None, 30.0);
            if live
                .system_menu
                .as_ref()
                .unwrap()
                .reference_title_return()
                .is_some_and(|d| d.reference_is_open())
            {
                break;
            }
            assert!(Instant::now() < deadline, "title confirmation did not open");
        }
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Left),
            0.0,
        );
        step(
            &mut live,
            ctx,
            server,
            &mut frame,
            Some(AppAction::Enter),
            0.0,
        );
        while live.pending_title_setup().is_none() {
            step(&mut live, ctx, server, &mut frame, None, 30.0);
            assert!(Instant::now() < deadline, "title save did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        let transferred = live.take_title_global_data().unwrap();
        let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        for data in [&transferred.data, &disk] {
            assert_eq!(
                serde_json::to_value(&data.auto_save_slot.as_ref().unwrap().entry).unwrap(),
                serde_json::to_value(&second.entry).unwrap(),
                "{name}: title globals and disk Continue must use the second save"
            );
            assert_eq!(
                serde_json::to_value(data.manual_save_slots.as_slice()).unwrap(),
                serde_json::to_value(globals.manual_save_slots.as_slice()).unwrap(),
                "Manual slots remain unchanged"
            );
        }
        eprintln!(
            "{name} -> pause Load3420 -> title: current boundary, transferred Continue and decoded disk Continue match; manual slots unchanged"
        );
    }
    assert_eq!(
        std::fs::read(&source).unwrap(),
        original,
        "Source copy untouched"
    );
}
