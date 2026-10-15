//! Actual Episode4 Tea Party Plus -> ending -> Continue regression.
use super::{
    StartableCommand,
    reconstruction::{ReconstructedScene, ReconstructionStop},
};
use shin_core::{
    format::{
        rom::RomReader,
        save::{GameData, GameDataEntry, SaveData},
        scenario::Scenario,
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::{CommandResult, RuntimeCommand},
};
use std::{fs::File, io::Read, path::PathBuf};

fn ending_fixture(scenario: &Scenario) -> (SaveData, GameData, GameData) {
    let mut globals = SaveData::default();
    globals.settings.v11_disallowskipunread = false;
    for slot in 1..=48 {
        globals.persist_data.set(slot, 3);
    }
    let entry = GameDataEntry {
        scenario_id: 11,
        random_seed: 12345,
        save_position: 0,
        route_data: serde_json::from_value(serde_json::json!(vec![0u8; 1024])).unwrap(),
    };
    let mut source = ReconstructedScene::from_save(scenario, &entry, &globals, 1_000_000).unwrap();
    let mut result = CommandResult::None;
    loop {
        let c = source.scripter.run(result).unwrap();
        let first = matches!(c, RuntimeCommand::MSGSET(_)) && source.state.current_event.is_none();
        if let RuntimeCommand::SGET(get) = c {
            let v = get.apply_state(&mut source.state);
            result = get.token.finish(v);
        } else {
            super::command::apply_command_state_ref(&c, &mut source.state).unwrap();
            if first {
                break;
            }
            result = c.execute_dummy().unwrap();
        }
    }
    let mut game = source.state.snapshot_game_data();
    assert_eq!(game.entry.save_position, 2);
    // Retained inactive context must survive the rewind independently.
    game.primary_entry = Some(GameDataEntry {
        scenario_id: 0,
        random_seed: 99,
        save_position: 17,
        route_data: serde_json::from_value(serde_json::json!([0x41u8, 0x22])).unwrap(),
    });
    let mut target =
        ReconstructedScene::next_chapter_game_data(scenario, &game, &globals, 1_000_000)
            .unwrap()
            .unwrap();
    assert_eq!(target.state.save_position, 820);
    assert_eq!(target.state.messagebox_state.message_id.0, 32558);
    assert!(target.state.current_event.is_none());
    target.state.capture_autosave(); // Installed Plus target, as the live dispatcher does.
    let mut command = match target.stop {
        ReconstructionStop::Command(c) => Some(c),
        ReconstructionStop::Response(r) => {
            result = r;
            None
        }
    };
    let mut trace = Vec::new();
    for _ in 0..10000 {
        let already_applied = command.is_some();
        let c = command
            .take()
            .unwrap_or_else(|| target.scripter.run(result).unwrap());
        trace.push(format!(
            "{:?} pos={} auto={:?} event={:?} {c:?}",
            target.scripter.position(),
            target.state.save_position,
            target
                .state
                .auto_save_slot
                .as_ref()
                .map(|g| g.entry.save_position),
            target.state.current_event
        ));
        if let RuntimeCommand::SGET(get) = c {
            let v = get.apply_state(&mut target.state);
            result = get.token.finish(v);
        } else {
            if !already_applied {
                super::command::apply_command_state_ref(&c, &mut target.state).unwrap();
            }
            if matches!(&c, RuntimeCommand::EXIT(exit) if exit.arg1 == 0) {
                assert_eq!(target.state.save_position, 822);
                let ending = target.state.auto_save_slot.clone().unwrap();
                assert_eq!(ending.entry.save_position, 821);
                globals.persist_data = target.state.persist.clone();
                globals.save_vectors = target.state.save_vectors.clone();
                globals.auto_save_slot = Some(ending.clone());
                globals.manual_save_slots[0] = Some(ending.clone());
                if let Some(output) = std::env::var_os("SHIN_EP4_ENDING_OUTPUT") {
                    let output = PathBuf::from(output);
                    assert!(output.starts_with("/private/tmp"));
                    std::fs::create_dir_all(&output).unwrap();
                    std::fs::write(output.join("trace.txt"), trace.join("\n")).unwrap();
                    std::fs::write(output.join("main.snr"), scenario.raw()).unwrap();
                    std::fs::write(
                        output.join("source.json"),
                        serde_json::to_vec_pretty(&game).unwrap(),
                    )
                    .unwrap();
                }
                return (globals, game, ending);
            }
            result = c.execute_dummy().unwrap();
        }
    }
    panic!("Tea Party ending did not exit");
}

#[test]
#[ignore = "requires supplied ROM; CPU only, no audio/GPU/user-save writes"]
fn reference_episode4_teaparty_ending_continue_checkpoint() {
    let mut rom = RomReader::new(
        StatelessFile::new(File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut snr = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut snr)
        .unwrap();
    let scenario = Scenario::new(snr.into()).unwrap();
    let (globals, _, ending) = ending_fixture(&scenario);
    let globals = SaveData::decode(&globals.encode()).unwrap();
    let bytes = globals.encode();
    for game in [
        globals.auto_save_slot.as_ref().unwrap(),
        globals.manual_save_slots[0].as_ref().unwrap(),
    ] {
        let raw = ReconstructedScene::from_game_data(&scenario, game, &globals, 1_000_000).unwrap();
        assert_eq!(raw.state.current_event, Some(10));
        let loaded =
            ReconstructedScene::from_game_data_for_load(&scenario, game, &globals, 1_000_000)
                .unwrap();
        assert_eq!(loaded.state.save_position, 820);
        assert!(loaded.state.current_event.is_none());
        assert_eq!(loaded.state.messagebox_state.message_id.0, 32558);
        assert!(
            loaded
                .state
                .messagebox_state
                .text
                .as_ref()
                .unwrap()
                .contains("私は、だぁれ")
        );
        assert_eq!(
            format!("{:?}", loaded.state.primary_entry),
            format!("{:?}", ending.primary_entry)
        );
        let ReconstructionStop::Command(RuntimeCommand::MSGSET(message)) = loaded.stop else {
            panic!("Continue must wait at the pre-ending dialogue");
        };
        assert!(message.auto_wait);
        assert_eq!(
            serde_json::to_value(loaded.state.snapshot_entry().route_data).unwrap(),
            serde_json::to_value(&game.entry.route_data).unwrap()
        );
    }
    assert_eq!(
        globals.encode(),
        bytes,
        "Load preparation must not mutate global saves"
    );
    println!(
        "TeaParty4 Plus target820 -> ending Continue821 -> Load/Continue dialogue820; inactive context and route retained"
    );
}

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio"]
fn reference_episode4_teaparty_ending_continue_live() {
    assert!(std::env::var_os("SHIN_REFERENCE_EP4_ENDING_CONTINUE_ONLY").is_some());
    std::thread::Builder::new()
        .name("ep4-ending-continue".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(super::save_menu_render_reference::reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

pub(super) fn verify_live<'a>(
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    target: &wgpu::Texture,
) {
    use super::{Adv, assets::AdvAssets};
    use shin_core::{primitives::update::FrameId, time::Ticks, vm::Scripter};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let (globals, _, ending) = ending_fixture(&assets.scenario);
    let mut frame = FrameId::default();
    let mut expected_pixels: Option<Vec<u8>> = None;
    for (name, game) in [
        ("dialogue-control", {
            let mut g = ending.clone();
            g.entry.save_position = 820;
            g
        }),
        ("Continue", globals.auto_save_slot.clone().unwrap()),
        ("Load", globals.manual_save_slots[0].clone().unwrap()),
    ] {
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        let path = std::env::temp_dir().join(format!(
            "shin-ep4-ending-{}-{name}/progress.dat",
            std::process::id()
        ));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, globals.encode()).unwrap();
        live.configure_save_file(path.clone(), None).unwrap();
        live.load_game_data(&globals, &game, server.clone())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while live.pending_restore.is_some() {
            frame.advance();
            live.update_saved_scene_load(&mut crate::update::UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(30.0),
                asset_server: server,
                pre_render: ctx,
            });
            submit(ctx);
            assert!(Instant::now() < deadline, "ending Continue load timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.vm_state.save_position, 820);
        assert_eq!(live.vm_state.current_event, None);
        assert_eq!(live.vm_state.messagebox_state.message_id.0, 32558);
        assert!(matches!(
            live.current_command,
            Some(super::ExecutingCommand::MSGSET(_))
        ));
        for _ in 0..3 {
            frame.advance();
            let input = enum_map::EnumMap::default();
            live.update(
                &mut crate::update::UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(60.0),
                    asset_server: server,
                    pre_render: ctx,
                },
                input,
            );
            submit(ctx);
        }
        assert!(
            !live.finished && live.pending_save.is_none(),
            "Continue must not run EXIT/SaveGame again"
        );
        assert_eq!(live.vm_state.save_position, 820);
        let view = target.create_view(&Default::default());
        let mut pass = ctx.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                view: &view,
            },
            Some(ctx.depth_stencil),
            "TeaParty4 Continue",
        );
        live.render(&mut pass);
        drop(pass);
        let pixels = crate::layer::user::readback_test_texture(ctx, target);
        let snapshot = path.with_file_name("scene.rgba");
        std::fs::write(&snapshot, &pixels).unwrap();
        println!("{name} scene: {}", snapshot.display());
        assert!(
            pixels
                .chunks_exact(4)
                .filter(|p| p[..3] != [0, 0, 0])
                .count()
                > 100_000,
            "Restored pre-ending scene must be visible"
        );
        if let Some(ref expected) = expected_pixels {
            // Authored rain and wobble clocks differ between asynchronous loads;
            // retain this measurement as diagnostic, not a framebuffer oracle.
            let different = pixels
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(a, b)| a.abs_diff(*b) > 2))
                .count();
            println!("{name}: {different} changed pixels against asynchronously loaded control");
        } else {
            expected_pixels = Some(pixels);
        }
        live.persistence
            .as_mut()
            .unwrap()
            .flush(&live.vm_state)
            .unwrap();
        let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk.auto_save_slot.unwrap().entry.save_position, 820);
        assert_eq!(
            disk.manual_save_slots[0]
                .as_ref()
                .unwrap()
                .entry
                .save_position,
            821
        );
        println!(
            "TeaParty4 {name}: actual public load, dialogue wait, Metal render and Continue publication pass"
        );
    }
}

fn submit(ctx: &mut crate::render::PreRenderContext) {
    let encoder = std::mem::replace(
        ctx.encoder,
        ctx.device.create_command_encoder(&Default::default()),
    );
    let mut upload = ctx.device.create_command_encoder(&Default::default());
    ctx.dynamic_buffer.finish(&mut upload);
    ctx.queue.submit([upload.finish(), encoder.finish()]);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

fn actual_progress() -> (Vec<u8>, SaveData) {
    let path = PathBuf::from(std::env::var_os("SHIN_EP4_USER_PROGRESS").unwrap());
    assert!(path.starts_with("/private/tmp"));
    let bytes = std::fs::read(path).unwrap();
    let save = SaveData::decode(&bytes).unwrap();
    let entry = &save.auto_save_slot.as_ref().unwrap().entry;
    assert_eq!(
        (entry.scenario_id, entry.random_seed, entry.save_position),
        (11, 4112460519, 822)
    );
    (bytes, save)
}

#[test]
#[ignore = "requires copied actual user progress and supplied ROM;CPU only,no hardware/user writes"]
fn reference_actual_ep4_continue_event_end_capture() {
    let (bytes, mut globals) = actual_progress();
    let mut rom = RomReader::new(
        StatelessFile::new(File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut snr = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut snr)
        .unwrap();
    let scenario = Scenario::new(snr.into()).unwrap();
    let stale = globals.auto_save_slot.as_ref().unwrap();
    let loaded =
        ReconstructedScene::from_game_data_for_load(&scenario, stale, &globals, 1_000_000).unwrap();
    assert_eq!(
        loaded.state.save_position, 822,
        "No old-build compatibility recovery"
    );
    assert!(matches!(
        loaded.stop,
        ReconstructionStop::Command(RuntimeCommand::EVEND(_))
    ));
    assert!(!loaded.publishes_continue_checkpoint());
    assert_eq!(
        globals.encode(),
        bytes,
        "Diagnosing source must not modify it"
    );
    // Recreate the valid preceding event capture using the exact user history.
    // This is an isolated fixture; working progress is never rewritten.
    let mut event = stale.clone();
    event.entry.save_position = 821;
    globals.auto_save_slot = Some(event.clone());
    let skipped =
        ReconstructedScene::skip_event_game_data(&scenario, &event, &globals, 1_000_000).unwrap();
    assert_eq!(skipped.state.save_position, 822);
    assert!(!skipped.publishes_continue_checkpoint());
    assert_eq!(
        skipped
            .state
            .auto_save_slot
            .as_ref()
            .unwrap()
            .entry
            .save_position,
        821
    );
    let mut former = skipped.state.clone();
    former.capture_autosave();
    assert_eq!(
        former.auto_save_slot.unwrap().entry.save_position,
        822,
        "Former unconditional publication reproduces the user's exact checkpoint"
    );
    let target = ReconstructedScene::from_game_data_for_load(
        &scenario,
        skipped.state.auto_save_slot.as_ref().unwrap(),
        &globals,
        1_000_000,
    )
    .unwrap();
    assert_eq!(target.state.save_position, 820);
    assert_eq!(target.state.messagebox_state.message_id.0, 32558);
    println!(
        "Actual Continue822 is post-event; fixed skip publication keeps821 and subsequent Continue waits at dialogue820"
    );
}

#[test]
#[ignore = "requires copied actual progress, supplied ROM, Metal and exact Loopback Audio"]
fn reference_actual_ep4_event_skip_title_continue() {
    assert!(std::env::var_os("SHIN_REFERENCE_EP4_ACTUAL_SKIP_ONLY").is_some());
    std::thread::Builder::new()
        .name("actual-ep4-skip-continue".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(super::save_menu_render_reference::reference_save_menu_gpu_window_and_artwork)
        .unwrap()
        .join()
        .unwrap();
}

pub(super) fn verify_actual_skip<'a>(
    ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    target: &wgpu::Texture,
) {
    use super::{Adv, assets::AdvAssets};
    use crate::{
        app::{AppAction, SceneHost},
        update::UpdateContext,
    };
    use shin_core::{primitives::update::FrameId, time::Ticks, vm::Scripter};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let (source_bytes, mut globals) = actual_progress();
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let mut event = globals.auto_save_slot.clone().unwrap();
    event.entry.save_position = 821;
    globals.auto_save_slot = Some(event.clone());
    let scene =
        ReconstructedScene::from_game_data(&assets.scenario, &event, &globals, 1_000_000).unwrap();
    assert_eq!(scene.state.current_event, Some(10));
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut live = Adv::new(
        audio,
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let path = std::env::temp_dir().join(format!(
        "shin-actual-ep4-skip-{}/progress.dat",
        std::process::id()
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, globals.encode()).unwrap();
    live.configure_save_file(path.clone(), None).unwrap();
    live.queue_reconstructed_scene(Box::new(scene), server.clone(), None);
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(45);
    while live.pending_restore.is_some() {
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(30.0),
            asset_server: server,
            pre_render: ctx,
        });
        submit(ctx);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.vm_state.save_position, 821);
    // Actual controller B/Cancel path queues native event-end reconstruction.
    let mut input = enum_map::EnumMap::default();
    for action in [AppAction::Cancel, AppAction::HideMessage] {
        input[action] = shin_input::ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    frame.advance();
    live.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: ctx,
        },
        input,
    );
    submit(ctx);
    assert!(
        live.pending_restore.is_some() || live.vm_state.save_position == 822,
        "B must queue event skip"
    );
    while live.pending_restore.is_some() {
        frame.advance();
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(30.0),
            asset_server: server,
            pre_render: ctx,
        });
        submit(ctx);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(live.vm_state.save_position, 822);
    assert_eq!(live.vm_state.current_event, None);
    assert_eq!(
        live.vm_state
            .auto_save_slot
            .as_ref()
            .unwrap()
            .entry
            .save_position,
        821,
        "EVEND publication must preserve the last native capture"
    );
    let mut host = SceneHost::new(live, assets, server.clone());
    fn host_step<'a>(
        host: &mut SceneHost,
        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<crate::asset::system::AssetServer>,
        frame: &mut FrameId,
        ticks: f32,
        action: Option<AppAction>,
    ) {
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(ticks),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            glam::vec2(20.0, 20.0),
            glam::vec2(20.0, 20.0),
            (1920, 1080),
            0.0,
            ticks as f64 / 60.0,
        )
        .unwrap();
        submit(ctx);
    }
    while host.reference_title().is_none() {
        host_step(&mut host, ctx, server, &mut frame, 7200.0, None);
        assert!(
            Instant::now() < deadline,
            "Ending save/title return timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        host.reference_save()
            .auto_save_slot
            .unwrap()
            .entry
            .save_position,
        821
    );
    let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disk.auto_save_slot.unwrap().entry.save_position, 821);
    while !host.reference_title().unwrap().is_loaded() {
        host_step(&mut host, ctx, server, &mut frame, 0.0, None);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..360 {
        if host.reference_title().unwrap().motion.phase() == crate::title::TitlePhase::Active {
            break;
        }
        host_step(&mut host, ctx, server, &mut frame, 1.0, None);
    }
    assert_eq!(
        host.reference_title().unwrap().motion.frame().caption,
        Some(crate::title::TitleItem::Continue)
    );
    for (ticks, action) in [
        (7.0, Some(AppAction::Enter)),
        (0.0, None),
        (45.0, None),
        (45.0, None),
        (0.0, None),
        (0.0, None),
    ] {
        host_step(&mut host, ctx, server, &mut frame, ticks, action);
    }
    while host
        .reference_adv()
        .is_none_or(|a| a.is_loading_saved_scene())
    {
        host_step(&mut host, ctx, server, &mut frame, 30.0, None);
        assert!(Instant::now() < deadline, "Actual title Continue timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..3 {
        host_step(&mut host, ctx, server, &mut frame, 60.0, None);
    }
    let live = host.reference_adv().unwrap();
    assert_eq!(live.vm_state.save_position, 820);
    assert_eq!(live.vm_state.messagebox_state.message_id.0, 32558);
    assert!(!live.finished && live.pending_save.is_none());
    let view = target.create_view(&Default::default());
    let mut pass = ctx.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
            view: &view,
        },
        Some(ctx.depth_stencil),
        "Actual TeaParty Continue",
    );
    host.render(&mut pass);
    drop(pass);
    let pixels = crate::layer::user::readback_test_texture(ctx, target);
    assert!(
        pixels
            .chunks_exact(4)
            .filter(|p| p[..3] != [0, 0, 0])
            .count()
            > 100_000
    );
    std::fs::write(path.with_file_name("continued-scene.rgba"), pixels).unwrap();
    assert_eq!(
        std::fs::read(PathBuf::from(
            std::env::var_os("SHIN_EP4_USER_PROGRESS").unwrap()
        ))
        .unwrap(),
        source_bytes
    );
    println!(
        "Actual user history: B event skip -> EVEND822 retaining Continue821 -> real saved title -> Continue waits at dialogue820; no working-save writes"
    );
}
