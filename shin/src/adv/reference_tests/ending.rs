//! Reproduce the user's Episode8 Trick ending from a copied native save.
use crate::adv::{
    Adv, ExecutingCommand, StartableCommand,
    assets::AdvAssets,
    command,
    reconstruction::{ReconstructedScene, ReconstructionStop},
};
use crate::{
    app::{AppAction, SceneHost},
    asset::system::AssetServer,
    render::PreRenderContext,
    update::UpdateContext,
};
use shin_core::{
    format::{save::SaveData, scenario::Scenario},
    primitives::update::FrameId,
    time::Ticks,
    vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand},
    },
};
use std::sync::Arc;

fn credits(scenario: &Scenario, globals: &SaveData) -> ReconstructedScene {
    let entry = &globals
        .auto_save_slot
        .as_ref()
        .expect("Copied ending autosave")
        .entry;
    assert_eq!(entry.scenario_id, 7);
    let mut scene = ReconstructedScene::from_save(scenario, entry, globals, 1_000_000).unwrap();
    eprintln!(
        "Copied Episode8 save{} stops at{:?}",
        entry.save_position,
        scene.scripter.position()
    );
    scene.state.persistent_commits_enabled = false;
    let mut result = match scene.stop {
        ReconstructionStop::Command(command) => command.execute_dummy().unwrap(),
        ReconstructionStop::Response(result) => result,
    };
    for _ in 0..10_000 {
        let next = scene.scripter.run(result).unwrap();
        let is_credits = matches!(&next, RuntimeCommand::LAYERLOAD(load)
            if load.layer_type == shin_core::vm::command::types::LayerType::Picture
                && load.params.0 == 1928);
        result = match next {
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut scene.state);
                get.token.finish(value)
            }
            RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) => {
                panic!("Ending copy has an unresolved choice")
            }
            command => {
                command::apply_command_state_ref(&command, &mut scene.state).unwrap();
                if is_credits {
                    assert!(scene.state.current_event.is_some());
                    scene.state.persistent_commits_enabled = true;
                    // Prepare the authored picture through LoadedScene, then
                    // resume after its load token; LAYERLOAD isn't a save stop.
                    scene.stop = ReconstructionStop::Response(command.execute_dummy().unwrap());
                    eprintln!(
                        "Trick credits start at{:?}, boundary{}",
                        scene.scripter.position(),
                        scene.state.save_position
                    );
                    return scene;
                }
                command
                    .execute_dummy()
                    .expect("Trick credits not found before EXIT")
            }
        };
    }
    panic!("Trick credits not reached")
}

pub(crate) fn assert_ending_returns_to_title<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    target: &wgpu::Texture,
) {
    let source =
        std::env::var("SHIN_REFERENCE_ENDING_SAVE").expect("Use a copy, never a working user save");
    assert!(
        std::path::Path::new(&source).starts_with(std::env::temp_dir())
            || source.starts_with("/tmp/")
    );
    let source_bytes = std::fs::read(&source).unwrap();
    let globals = SaveData::decode(&source_bytes).unwrap();
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frame = FrameId::default();
    for mode in ["natural", "B", "long_tap"] {
        let scene = credits(&assets.scenario, &globals);
        let end_position = scene.state.save_position + 1;
        let mut live = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 7, 42),
        );
        let folder =
            std::env::temp_dir().join(format!("shin-ending-{}-{mode}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("progress.dat");
        std::fs::write(&path, &source_bytes).unwrap();
        live.configure_save_file(path.clone(), None).unwrap();
        live.queue_reconstructed_scene(Box::new(scene), server.clone(), None);
        let mut host = SceneHost::new(live, assets.clone(), server.clone());
        let mut step = |host: &mut SceneHost,
                        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
                        delta: f32,
                        action: Option<AppAction>,
                        held: bool,
                        seconds: f64| {
            let mut input = enum_map::EnumMap::default();
            if let Some(action) = action {
                input[action] = shin_input::ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            input[AppAction::PointerClick].is_held = held;
            frame.advance();
            host.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    pre_render: ctx,
                },
                input,
                glam::vec2(70.0, 1010.0),
                glam::vec2(70.0, 1010.0),
                (1920, 1080),
                0.0,
                seconds,
            )
            .unwrap();
            // Submit each frame as App does, so Metal's command pool cannot fill
            // while the asynchronous restore worker is preparing assets.
            let encoder = std::mem::replace(
                ctx.encoder,
                ctx.device.create_command_encoder(&Default::default()),
            );
            let mut upload = ctx.device.create_command_encoder(&Default::default());
            ctx.dynamic_buffer.finish(&mut upload);
            ctx.queue.submit([upload.finish(), encoder.finish()]);
            ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        while !host.reference_adv().is_some_and(|adv| {
            !adv.is_loading_saved_scene()
                && matches!(adv.current_command, Some(ExecutingCommand::LAYERWAIT(_)))
        }) {
            assert!(
                std::time::Instant::now() < deadline,
                "Credits preparation timed out: {:?}",
                host.reference_adv().map(|adv| (
                    adv.scripter.position(),
                    &adv.current_command,
                    adv.saved_scene_load_error()
                ))
            );
            step(&mut host, context, 1.0, None, false, 0.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // The credits begin below the canvas at Y=20000. Advance to their
        // visible middle using the ordinary animation clock, with no skip.
        step(&mut host, context, 3000.0, None, false, 0.0);
        // Real authored scroll is running, rather than a fabricated EXIT-only fixture.
        let visible = {
            let view = target.create_view(&Default::default());
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Episode8 ending",
            );
            host.render(&mut pass);
            drop(pass);
            crate::layer::user::readback_test_texture(context, target)
        };
        assert!(
            visible
                .chunks_exact(4)
                .any(|p| p[..3].iter().any(|&v| v > 20)),
            "Credits must actually render"
        );
        match mode {
            "B" => step(&mut host, context, 0.0, Some(AppAction::Cancel), false, 0.0),
            "long_tap" => {
                step(&mut host, context, 0.0, None, true, 0.0);
                step(&mut host, context, 0.0, None, true, 1.151);
            }
            _ => {}
        }
        let mut story_ticks = 3000.0;
        loop {
            let adv = host
                .reference_adv()
                .expect("Must queue replacement after authored EXIT");
            if adv.is_finished() && adv.pending_title_setup().is_some() {
                eprintln!(
                    "Ending {mode}: EXIT{:?}, save{}, ticks{story_ticks}",
                    adv.scripter.position(),
                    adv.vm_state.save_position
                );
                assert!(matches!(adv.scripter.position().0, 0x86b4dc | 0x86b538));
                assert_eq!(adv.vm_state.save_position, end_position);
                assert!(
                    adv.pending_title_setup().is_some(),
                    "Finished ADV must queue a title destination"
                );
                break;
            }
            if adv.is_finished() {
                assert!(
                    adv.pending_save.is_some(),
                    "EXIT must retain SaveGame before title handoff"
                );
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Ending {mode} stalled: command{:?}, pc{:?}, restore{:?}",
                adv.current_command,
                adv.scripter.position(),
                adv.saved_scene_load_error()
            );
            step(&mut host, context, 60.0, None, false, 0.0);
            story_ticks += 60.0;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if mode == "natural" {
            assert!(
                story_ticks >= 5200.0,
                "Authored credit duration must elapse"
            );
        }
        let completed = host.reference_save();
        let mut slots_preserved = completed.clone();
        slots_preserved.manual_save_slots = globals.manual_save_slots.clone();
        assert!(
            completed.encode() == slots_preserved.encode(),
            "Ending must preserve manual saves"
        );
        assert_eq!(completed.persist_data.get(49), 1);
        for id in 2608..=2614 {
            assert!(completed.save_vectors.unlocked_cgs.contains(id));
        }
        // Keep the same globals, audio tracks, manual slots and play clock through replacement.
        let ticks = host.reference_play_ticks();
        let bgm_track = host.reference_audio().bgm_player.reference_source().2;
        let system_track = host.reference_audio().system_player.track.id();
        step(&mut host, context, 0.0, None, false, 0.0);
        assert!(
            host.reference_title().is_some(),
            "Finished ADV must leave the black scene"
        );
        assert_eq!(host.reference_play_ticks(), ticks);
        assert_eq!(host.reference_save().encode(), completed.encode());
        assert!(Arc::ptr_eq(&manager, &host.reference_audio().manager));
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            bgm_track
        );
        assert_eq!(
            host.reference_audio().system_player.track.id(),
            system_track
        );
        let written_bytes = std::fs::read(&path).unwrap();
        let written = SaveData::decode(&written_bytes).unwrap();
        assert!(
            written.play_seconds <= completed.play_seconds,
            "Updating dialog must preserve the captured clock, not overwrite newer global elapsed time"
        );
        let mut captured = completed.clone();
        captured.play_seconds = written.play_seconds;
        assert_eq!(
            written_bytes,
            captured.encode(),
            "EXIT worker must write the complete captured globals"
        );
        while !host.reference_title().unwrap().is_loaded() {
            assert!(
                std::time::Instant::now() < deadline,
                "Returned title assets timed out"
            );
            step(&mut host, context, 0.0, None, false, 0.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..300 {
            step(&mut host, context, 1.0, None, false, 0.0);
            if host.reference_title().unwrap().motion.phase() == crate::title::TitlePhase::Active {
                break;
            }
        }
        assert_eq!(
            host.reference_title().unwrap().motion.phase(),
            crate::title::TitlePhase::Active
        );
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Returned title",
            );
            host.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(context, target);
        assert!(
            pixels
                .chunks_exact(4)
                .filter(|p| p[..3].iter().any(|&v| v > 40))
                .count()
                > 10_000,
            "Returned title must render visibly"
        );
        eprintln!(
            "Ending {mode}: title active and visible, globals/audio/save retained, Loopback Audio"
        );
        drop(host);
    }
    assert_eq!(
        std::fs::read(source).unwrap(),
        source_bytes,
        "Copied source remains unchanged"
    );
}
