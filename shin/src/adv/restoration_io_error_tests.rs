#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires supplied ROMs, explicit syscall interposer, Metal and exact Loopback Audio"]
fn reference_authored_saved_scene_io_error_recovery() {
    assert!(std::env::var_os("SHIN_REFERENCE_SAVED_IO_ERRORS_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-saved-io-errors".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(target_os = "macos")]
mod saved_io_errors {
    use super::*;
    use crate::{
        adv::{Adv, AdvAssets},
        app::AppAction,
        asset::system::{
            AssetLoadContext, AssetServer, IoFault, cache::AssetCache, supplied_payload,
            supplied_portrait,
        },
        layer::{DrawableLayer, Layer, user::UserLayer},
        render::PreRenderContext,
        update::UpdateContext,
    };
    use shin_core::{
        format::save::GameData,
        primitives::update::FrameId,
        time::Ticks,
        vm::command::types::{LayerbankId, PlaneId},
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{Duration, Instant},
    };

    fn game(entry: i32, seed: u32, boundary: u32) -> GameData {
        let mut route = RouteRecorder::default();
        for _ in 0..512 {
            route.record_persistent(0);
        }
        GameData::from_entry(GameDataEntry {
            scenario_id: entry,
            random_seed: seed,
            save_position: boundary,
            route_data: route.snapshot(),
        })
    }
    fn quiz_game(scenario: &Arc<Scenario>) -> (GameData, SaveData, u32) {
        let mut script = Scripter::new(scenario, 7, 12345);
        let mut state = VmState::for_scenario(7, 12345);
        let mut result = CommandResult::None;
        for _ in 0..250_000 {
            let current = script.run(result).unwrap();
            result = match current {
                RuntimeCommand::QUIZ(quiz) => {
                    let target = quiz.apply_state(&mut state).unwrap();
                    let params = state.layers.layerbanks[target.1].params;
                    if params.0 == 0 {
                        assert_eq!(params.1, 3);
                        assert_eq!(
                            script.position().0,
                            if reference_updated_scenario(scenario) {
                                0x78d718
                            } else {
                                0x78d6da
                            }
                        );
                        let globals = SaveData {
                            persist_data: state.persist.clone(),
                            save_vectors: state.save_vectors.clone(),
                            auto_save_slot: state.auto_save_slot.clone(),
                            ..SaveData::default()
                        };
                        return (
                            GameData::from_entry(state.snapshot_entry()),
                            globals,
                            state.save_position,
                        );
                    }
                    assert_eq!(params.0, 1, "Only the cake precedes the first Quiz");
                    let mut cake = crate::layer::user::cake_layer::CakeModel::default();
                    cake.pointer(glam::Vec2::new(850.0, 700.0), true);
                    cake.update(Ticks::from_f32(7.0));
                    cake.update(Ticks::ZERO);
                    let value = cake.result.unwrap();
                    command::replay_quiz_result(&mut state, Some(target), value);
                    quiz.token.finish(value)
                }
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut state);
                    get.token.finish(value)
                }
                current => {
                    assert!(!matches!(
                        current,
                        RuntimeCommand::SELECT(_) | RuntimeCommand::RESUME(_)
                    ));
                    command::apply_command_state_ref(&current, &mut state).unwrap();
                    current.execute_dummy().unwrap()
                }
            };
        }
        panic!("Authored Quiz checkpoint was not reached");
    }
    fn poll_load(
        live: &mut Adv,
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
        frame: &mut FrameId,
    ) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut pre_render = PreRenderContext {
            device: context.device,
            breakup_texture: context.breakup_texture,
            queue: context.queue,
            resize_source: context.resize_source,
            sampler_store: context.sampler_store,
            depth_stencil: context.depth_stencil,
            pipeline_storage: &mut *context.pipeline_storage,
            dynamic_buffer: &mut *context.dynamic_buffer,
            encoder: &mut *context.encoder,
        };
        while live.pending_restore.is_some() {
            assert!(Instant::now() < deadline, "Saved-scene IO load timed out");
            frame.advance();
            live.update_saved_scene_load(&mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: &mut pre_render,
            });
            if live.pending_restore.is_some() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    pub(super) fn tick(
        live: &mut Adv,
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
        frame: &mut FrameId,
        input: enum_map::EnumMap<AppAction, shin_input::ActionState>,
        target: &wgpu::TextureView,
    ) {
        frame.advance();
        let mut pre_render = PreRenderContext {
            device: context.device,
            breakup_texture: context.breakup_texture,
            queue: context.queue,
            resize_source: context.resize_source,
            sampler_store: context.sampler_store,
            depth_stencil: context.depth_stencil,
            pipeline_storage: &mut *context.pipeline_storage,
            dynamic_buffer: &mut *context.dynamic_buffer,
            encoder: &mut *context.encoder,
        };
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: &mut pre_render,
            },
            input,
        );
        live.adv_state
            .root_layer_group
            .pre_render(context, &Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Saved IO error recovery",
            );
            live.adv_state.render_scene(&mut pass);
            if let Some(dialog) = &live.restore_error_dialog {
                dialog.render(&mut pass);
            }
        }
        let encoder = std::mem::replace(
            context.encoder,
            context.device.create_command_encoder(&Default::default()),
        );
        let mut upload = context.device.create_command_encoder(&Default::default());
        context.dynamic_buffer.finish(&mut upload);
        context.queue.submit([upload.finish(), encoder.finish()]);
        context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU work must complete before readback or retirement");
    }
    pub(super) fn verify<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        assets: &AdvAssets,
        scenario: &Arc<Scenario>,
        target: &wgpu::TextureView,
    ) {
        let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_SAVED_IO_OUTPUT").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let fault = IoFault::load();
        let mut authored_assets = assets.clone();
        authored_assets.scenario = scenario.clone();
        let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
        let baseline = std::env::var_os("SHIN_REFERENCE_SAVED_IO_BASELINE").is_some();
        let mut summaries = String::new();
        for kind in if baseline {
            vec!["BUP"]
        } else {
            vec!["BUP", "PIC", "QUIZ"]
        } {
            for mode in if baseline || kind == "QUIZ" {
                vec![1]
            } else {
                vec![1, 2]
            } {
                for title in if baseline || kind == "QUIZ" {
                    vec![false]
                } else {
                    vec![false, true]
                } {
                    let (io, mut archive, mut path, mut offset, mut bytes) = supplied_portrait();
                    if kind != "BUP" {
                        path = if kind == "PIC" {
                            scenario.info_tables().picture_info[96].path()
                        } else {
                            "/quiz.txa".into()
                        };
                        (archive, offset, bytes) = supplied_payload(&path);
                        if kind == "PIC" {
                            offset += 36;
                            bytes.drain(..36);
                        }
                    }
                    let (target_game, target_globals, target_boundary) = if kind == "QUIZ" {
                        quiz_game(scenario)
                    } else {
                        (game(0, 0, 100), SaveData::default(), 100)
                    };
                    let server = Arc::new(AssetServer::new(
                        io.into(),
                        AssetLoadContext {
                            game: crate::cli::Game::Umineko,
                            higurashi_bustup_cache: Default::default(),
                            wgpu_device: context.device.clone(),
                            wgpu_queue: context.queue.clone(),
                            bustup_cache: AssetCache::new(),
                            picture_cache: Default::default(),
                            font_cache: Default::default(),
                        },
                    ));
                    let mut live = Adv::new(
                        audio.clone(),
                        server.clone(),
                        authored_assets.clone(),
                        Scripter::new(scenario, 0, 0),
                    );
                    let mut frame = FrameId::default();
                    if !title {
                        live.load_game_data(&SaveData::default(), &game(0, 0, 56), server.clone())
                            .unwrap();
                        poll_load(&mut live, context, &server, &mut frame);
                        assert!(live.saved_scene_load_error().is_none());
                        // Existing owners must not be released with the error notice.
                        live.adv_state.acquire_audio_pause();
                        live.adv_state.acquire_audio_pause();
                    }
                    let old_pc = live.scripter.position();
                    let old_boundary = live.vm_state.save_position;
                    let old_route = live.vm_state.route.bit_position();
                    let old_command = format!("{:?}", live.current_command);
                    let old_root_owner = live.adv_state.root_layer_group.properties().identity();
                    let old_screen_owner = live.adv_state.screen_layer().properties().identity();
                    let old_pause = live.adv_state.audio_pause_count;
                    live.sample_modal_pointer(glam::Vec2::ZERO, (1920, 1080), false, 1.0 / 60.0);
                    fault.arm(&archive, offset, bytes.len() as u64, mode);
                    live.load_game_data(&target_globals, &target_game, server.clone())
                        .unwrap();
                    let retirement = live
                        .pending_restore
                        .as_ref()
                        .unwrap()
                        .task
                        .as_ref()
                        .unwrap()
                        .retirement_handle()
                        .unwrap();
                    poll_load(&mut live, context, &server, &mut frame);
                    let report = fault.report();
                    fault.arm(&archive, offset, bytes.len() as u64, 0);
                    assert!(report[0] > 0 && report[1] > 0);
                    assert!(
                        retirement.is_finished(),
                        "Failed loader/children must retire before reporting"
                    );
                    let error = live
                        .saved_scene_load_error()
                        .expect("IO failure must reach existing scene error notice")
                        .to_owned();
                    assert!(error.contains(&path), "Missing failing asset path: {error}");
                    assert!(error.contains(if mode == 1 {
                        "os error 5"
                    } else {
                        "failed to read whole buffer"
                    }));
                    assert!(live.restore_error_dialog.is_some());
                    assert_eq!(live.adv_state.audio_pause_count, old_pause + 1);
                    assert_eq!(
                        server.context().picture_cache.reference_ownership_counts(),
                        (0, 0)
                    );
                    assert_eq!(
                        server.context().bustup_cache.reference_ownership_counts(),
                        (0, 0)
                    );
                    assert_eq!(
                        shin_tasks::async_io::spawn(async { 42 })
                            .poll_with_timeout(Duration::from_secs(2)),
                        Some(42)
                    );
                    for _ in 0..35 {
                        tick(
                            &mut live,
                            context,
                            &server,
                            &mut frame,
                            Default::default(),
                            target,
                        );
                        assert_eq!(live.scripter.position(), old_pc);
                        assert_eq!(live.vm_state.save_position, old_boundary);
                        assert_eq!(live.vm_state.route.bit_position(), old_route);
                        assert_eq!(format!("{:?}", live.current_command), old_command);
                        assert_eq!(
                            live.adv_state.root_layer_group.properties().identity(),
                            old_root_owner
                        );
                        assert_eq!(
                            live.adv_state.screen_layer().properties().identity(),
                            old_screen_owner
                        );
                    }
                    let mut input =
                        enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                    input[AppAction::Cancel].is_clicked = true;
                    tick(&mut live, context, &server, &mut frame, input, target);
                    assert!(
                        live.restore_error_dialog.is_some(),
                        "Cancel cannot dismiss acknowledgement"
                    );
                    let mut input =
                        enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                    input[AppAction::Enter].is_clicked = true;
                    tick(&mut live, context, &server, &mut frame, input, target);
                    for _ in 0..40 {
                        if live.restore_error_dialog.is_none() {
                            break;
                        }
                        tick(
                            &mut live,
                            context,
                            &server,
                            &mut frame,
                            Default::default(),
                            target,
                        );
                    }
                    assert!(live.restore_error_dialog.is_none());
                    assert_eq!(
                        live.vm_state.save_position, old_boundary,
                        "Acknowledgement input cannot advance story"
                    );
                    assert_eq!(live.adv_state.audio_pause_count, old_pause);
                    if title {
                        assert!(live.failed_scene_title_setup().is_some());
                    }
                    // Retry from the same public API/context; no progress files exist.
                    live.load_game_data(&target_globals, &target_game, server.clone())
                        .unwrap();
                    poll_load(&mut live, context, &server, &mut frame);
                    assert!(live.saved_scene_load_error().is_none());
                    assert_eq!(live.vm_state.save_position, target_boundary);
                    let group = live.adv_state.plane_layer_group(PlaneId::new(0));
                    if kind == "QUIZ" {
                        assert!(matches!(
                            live.active_interactive_layer(),
                            Some(crate::layer::user::InteractiveLayerRef::Quiz(_))
                        ));
                    } else {
                        assert!(matches!(
                            group.get_layer(LayerbankId::new(0)).as_deref(),
                            Some(UserLayer::Picture(_))
                        ));
                        assert!(matches!(
                            group.get_layer(LayerbankId::new(3)).as_deref(),
                            Some(UserLayer::Bustup(_))
                        ));
                    }
                    drop(live);
                    assert_eq!(
                        server.context().picture_cache.reference_ownership_counts(),
                        (0, 0)
                    );
                    assert_eq!(
                        server.context().bustup_cache.reference_ownership_counts(),
                        (0, 0)
                    );
                    let summary = format!(
                        "{kind} mode{mode} title={title}: authored public load{}->{target_boundary} actual resource fault={report:?}; notice rendered/submitted, VM/command/property owners retained, partial PIC/BUP/cache jobs retired, IO executor responsive, Cancel blocked, Enter acknowledged without advancing and pause ownership balanced; public full retry{target_boundary} publishes authored layers and final owners retire.\n",
                        if title { 0 } else { 56 }
                    );
                    eprint!("{summary}");
                    summaries.push_str(&summary);
                }
            }
        }
        std::fs::write(output.join("desktop.txt"), summaries).unwrap();
    }
}
