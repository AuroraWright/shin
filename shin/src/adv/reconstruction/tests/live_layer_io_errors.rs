use super::*;
use crate::{
    adv::{Adv, AdvAssets, UpdatableCommand},
    app::AppAction,
    asset::system::{
        AssetLoadContext, AssetServer, IoFault, LayeredAssetIo, cache::AssetCache,
        supplied_payload,
    },
    layer::{DrawableLayer, user::UserLayer},
    render::PreRenderContext,
    update::{Updatable, UpdateContext},
};
use shin_core::{
    primitives::update::FrameId,
    time::Ticks,
    vm::command::types::{LayerLoadFlags, LayerType},
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROMs, explicit pread interposer, Metal and exact Loopback Audio"]
fn reference_live_layer_load_io_error_recovery() {
    assert!(std::env::var_os("SHIN_REFERENCE_LIVE_LAYER_IO_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-live-layer-io".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

fn authored_load(
    scenario: &Scenario,
    kind: &str,
) -> (RuntimeCommand, VmState, Scripter, String) {
    if kind == "Mask" {
        // Controlled API composition: retain an actual opening PIC command
        // and source scene, pairing them with parameters from an actual
        // authored WIPE. AUTO_WIPE is native-validated, but this pairing is
        // not claimed as an authored route or an observed normal-play bug.
        let (RuntimeCommand::LAYERLOAD(mut load), mut state, script, _) =
            authored_load(scenario, "PIC")
        else {
            unreachable!()
        };
        let mut scan = Scripter::new(scenario, 0, 12345);
        let mut source = VmState::for_scenario(0, 12345);
        let mut result = CommandResult::None;
        for _ in 0..100_000 {
            let current = scan.run(result).unwrap();
            if let RuntimeCommand::WIPE(wipe) = &current
                && wipe.ty == 1
                && wipe.duration.as_f32() >= 1.0
            {
                state.layers.transition =
                    crate::adv::vm_state::layers::LayerTransitionSettings {
                        ty: wipe.ty,
                        flags: wipe.flags,
                        duration: wipe.duration,
                        params: wipe.params,
                    };
                // AUTO_WIPE deliberately suppresses transitions during
                // PAGEBACK. Exercise its active-page branch in this
                // controlled composition.
                state.layers.is_page_back_started = false;
                load.flags.insert(LayerLoadFlags::AUTO_WIPE);
                let path = scenario.info_tables().mask_info[wipe.params.0 as usize].path();
                return (RuntimeCommand::LAYERLOAD(load), state, script, path);
            }
            if matches!(
                current,
                RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) | RuntimeCommand::RESUME(_)
            ) {
                break;
            }
            result = match current {
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut source);
                    get.token.finish(value)
                }
                current => {
                    command::apply_command_state_ref(&current, &mut source).unwrap();
                    let Some(result) = current.execute_dummy() else {
                        break;
                    };
                    result
                }
            };
        }
        panic!("No authored mask WIPE parameters before an interactive decision/exit");
    }
    let entries = if kind == "Cake" { vec![7] } else { vec![0] };
    for entry in entries {
        let mut script = Scripter::new(scenario, entry, 12345);
        let mut state = VmState::for_scenario(entry, 12345);
        let mut result = CommandResult::None;
        for _ in 0..100_000 {
            let current = script.run(result).unwrap();
            if let RuntimeCommand::LAYERLOAD(load) = &current {
                let path = match kind {
                    "PIC" if load.layer_type == LayerType::Picture => {
                        Some(scenario.info_tables().picture_info[load.params.0 as usize].path())
                    }
                    "BUP" if load.layer_type == LayerType::Bustup => {
                        Some(scenario.info_tables().bustup_info[load.params.0 as usize].path())
                    }
                    "Movie" if load.layer_type == LayerType::Movie => {
                        Some(scenario.info_tables().movie_info[load.params.0 as usize].path())
                    }
                    "Cake" if load.layer_type == LayerType::Quiz && load.params.0 == 1 => {
                        Some("/cake.txa".into())
                    }
                    _ => None,
                };
                if let Some(path) = path {
                    return (current, state, script, path);
                }
            }
            result = match current {
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut state);
                    get.token.finish(value)
                }
                current => {
                    if matches!(
                        current,
                        RuntimeCommand::SELECT(_)
                            | RuntimeCommand::QUIZ(_)
                            | RuntimeCommand::RESUME(_)
                    ) {
                        break; // Do not invent an interactive decision/history.
                    }
                    command::apply_command_state_ref(&current, &mut state).unwrap();
                    let Some(result) = current.execute_dummy() else {
                        break;
                    };
                    result
                }
            };
        }
    }
    panic!("No authored {kind} load found before an interactive decision/exit");
}
pub(super) fn contexts(live: &Adv) -> serde_json::Value {
    let mut value = serde_json::to_value(live.vm_state.snapshot_game_data()).unwrap();
    value.as_object_mut().unwrap().remove("date_time");
    value
}
pub(super) fn fresh_server(context: &PreRenderContext) -> Arc<AssetServer> {
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    Arc::new(AssetServer::new(
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
    ))
}
pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
    target: &wgpu::TextureView,
) {
    let out = PathBuf::from(std::env::var_os("SHIN_REFERENCE_LIVE_LAYER_IO_OUTPUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let fault = IoFault::load();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let baseline = std::env::var_os("SHIN_REFERENCE_LIVE_LAYER_IO_BASELINE").is_some();
    let mut summaries = String::new();
    for kind in if baseline {
        vec!["BUP"]
    } else {
        vec!["PIC", "BUP", "Movie", "Cake", "Mask"]
    } {
        for mode in if baseline { vec![1] } else { vec![1, 2] } {
            let (runtime, state, script, path) = authored_load(scenario, kind);
            let RuntimeCommand::LAYERLOAD(load) = &runtime else {
                unreachable!()
            };
            let loaded_kind = load.layer_type;
            let params = load.params;
            let pc = script.position();
            let server = fresh_server(context);
            let mut local = PreRenderContext {
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
            let context = &mut local;
            let mut live_assets = assets.clone();
            live_assets.scenario = scenario.clone();
            let mut live = Adv::new(
                audio.clone(),
                server.clone(),
                live_assets,
                Scripter::new(scenario, 0, 0),
            );
            // A prepared composition of the genuine decoded VM prefix and
            // its next command; this is not a continuous playback route.
            live.vm_state = state;
            live.scripter = script;
            let loaded = shin_tasks::block_on(crate::adv::restoration::LoadedScene::load(
                &server,
                &audio,
                scenario,
                &live.vm_state,
            ))
            .unwrap();
            let mut fresh =
                crate::adv::AdvState::new(audio.clone(), server.clone(), live.assets.clone());
            let mut loaded_audio =
                loaded.install(&mut fresh, &live.vm_state, scenario, context);
            loaded_audio.install_container_properties(
                &mut live.adv_state,
                &mut fresh,
                &live.vm_state,
                false,
            );
            loaded_audio.install(&mut fresh, &live.vm_state, scenario);
            live.adv_state = fresh;
            let old_picture_cache = server.context().picture_cache.reference_ownership_counts();
            let old_bustup_cache = server.context().bustup_cache.reference_ownership_counts();
            let (archive, mut offset, mut payload) = supplied_payload(&path);
            if kind == "PIC" {
                offset += 36;
                payload.drain(..36);
            }
            let mut frame = FrameId::default();
            fault.arm(&archive, offset, payload.len() as u64, mode);
            let start = command::apply_command_state_and_start(
                runtime,
                &mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: &server,
                    pre_render: context,
                },
                scenario,
                &mut live.vm_state,
                &mut live.adv_state,
            );
            let crate::adv::CommandStartResult::Yield(command) = start else {
                panic!("Expected a live LAYERLOAD")
            };
            live.current_command = Some(command);
            let before_game = contexts(&live);
            let before_counter = live.vm_state.layers.layer_load_counter;
            let before_root = live.adv_state.root_layer_group.properties().identity();
            let before_command = format!("{:?}", live.current_command);
            let deadline = Instant::now() + Duration::from_secs(20);
            while live.restore_error_dialog.is_none() {
                assert!(
                    Instant::now() < deadline,
                    "Live layer error was not reported: {kind}"
                );
                saved_io_errors::tick(
                    &mut live,
                    context,
                    &server,
                    &mut frame,
                    Default::default(),
                    target,
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let selected = fault.report();
            fault.arm(&archive, offset, payload.len() as u64, 0);
            assert!(selected[0] > 0 && selected[1] > 0);
            let error = live.saved_scene_load_error().unwrap();
            assert!(
                error.contains(&path),
                "Missing mandatory asset path: {kind}: {error}"
            );
            assert!(
                error.contains(if mode == 1 {
                    "os error 5"
                } else {
                    "failed to read whole buffer"
                }),
                "{kind}: {error}"
            );
            assert_eq!(live.adv_state.audio_pause_count, 1);
            assert_eq!(
                live.adv_state.root_layer_group.properties().identity(),
                before_root
            );
            assert_eq!(live.vm_state.layers.layer_load_counter, before_counter);
            assert_eq!(live.scripter.position(), pc);
            assert_eq!(contexts(&live), before_game);
            assert_eq!(format!("{:?}", live.current_command), before_command);
            assert_eq!(
                server
                    .context()
                    .picture_cache
                    .reference_ownership_counts()
                    .1,
                0
            );
            assert_eq!(
                server.context().bustup_cache.reference_ownership_counts().1,
                0
            );
            if kind != "Mask" {
                assert_eq!(
                    server.context().picture_cache.reference_ownership_counts(),
                    old_picture_cache
                );
                assert_eq!(
                    server.context().bustup_cache.reference_ownership_counts(),
                    old_bustup_cache
                );
            }
            assert_eq!(
                shin_tasks::async_io::spawn(async { 42 })
                    .poll_with_timeout(Duration::from_secs(2)),
                Some(42)
            );
            for _ in 0..20 {
                saved_io_errors::tick(
                    &mut live,
                    context,
                    &server,
                    &mut frame,
                    Default::default(),
                    target,
                );
            }
            let mut enter = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
            enter[AppAction::Enter].is_clicked = true;
            saved_io_errors::tick(&mut live, context, &server, &mut frame, enter, target);
            for _ in 0..40 {
                if live.restore_error_dialog.is_none() {
                    break;
                }
                saved_io_errors::tick(
                    &mut live,
                    context,
                    &server,
                    &mut frame,
                    Default::default(),
                    target,
                );
            }
            assert!(live.restore_error_dialog.is_none());
            assert_eq!(live.adv_state.audio_pause_count, 0);
            assert!(live.saved_scene_load_error().is_none());
            // Poll the retained production command so its immediate next
            // script commands do not obscure publication/ownership checks.
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                assert!(Instant::now() < deadline, "Retry timed out: {kind}");
                frame.advance();
                let mut update = UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(1.0),
                    asset_server: &server,
                    pre_render: context,
                };
                let done = live
                    .current_command
                    .as_mut()
                    .unwrap()
                    .update(
                        &mut update,
                        scenario,
                        &mut live.vm_state,
                        &mut live.adv_state,
                        false,
                    )
                    .is_some();
                live.adv_state.update(&mut update);
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
                if done {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                live.vm_state.layers.layer_load_counter, before_counter,
                "Retry cannot reapply VM bookkeeping"
            );
            assert_eq!(live.scripter.position(), pc);
            assert_eq!(contexts(&live), before_game);
            let mut published = 0;
            for (bank, descriptor) in &live.vm_state.layers.layerbanks {
                if descriptor.layer_type == Some(loaded_kind) && descriptor.params == params {
                    let layer = live
                        .adv_state
                        .plane_layer_group(descriptor.plane)
                        .get_layer(bank)
                        .unwrap();
                    assert!(
                        match (&*layer, loaded_kind) {
                            (UserLayer::Picture(_), LayerType::Picture)
                            | (UserLayer::Bustup(_), LayerType::Bustup)
                            | (UserLayer::Movie(_), LayerType::Movie)
                            | (UserLayer::Cake(_), LayerType::Quiz) => true,
                            _ => false,
                        },
                        "No silent Null fallback: {kind}"
                    );
                    assert_eq!(
                        layer.properties().layer_load_counter(),
                        descriptor.layer_load_counter
                    );
                    published += 1;
                }
            }
            assert!(published > 0);
            assert!(live.persistence.is_none());
            drop(live);
            assert_eq!(
                server.context().picture_cache.reference_ownership_counts(),
                (0, 0)
            );
            assert_eq!(
                server.context().bustup_cache.reference_ownership_counts(),
                (0, 0)
            );
            let row = format!(
                "{kind} mode{mode} pc={:x} {path}: error notice, retained VM/command/root, responsive executor, acknowledgement, same-command full retry and final cache retirement pass\n",
                pc.0
            );
            print!("{row}");
            summaries.push_str(&row);
        }
    }
    std::fs::write(out.join("live-layer-io.txt"), &summaries).unwrap();
    print!("{summaries}");
}
