use super::live_layer_io_errors::{contexts, fresh_server};
use super::*;
use crate::{
    adv::{Adv, AdvAssets, UpdatableCommand},
    app::AppAction,
    asset::system::{IoFault, supplied_payload},
    layer::{DrawableLayer, Layer},
    render::PreRenderContext,
    update::{Updatable, UpdateContext},
};
use shin_core::{primitives::update::FrameId, time::Ticks, vm::command::types::PlaneId};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROMs, explicit pread interposer, Metal and the default audio output"]
fn reference_live_mask_commands_io_error_recovery() {
    assert!(std::env::var_os("SHIN_REFERENCE_LIVE_MASK_IO_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-live-mask-io".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

fn authored(scenario: &Scenario, kind: &str) -> (RuntimeCommand, VmState, Scripter, String) {
    for entry in 0..48 {
        let mut script = Scripter::new(scenario, entry, 12345);
        let mut state = VmState::for_scenario(entry, 12345);
        let mut result = CommandResult::None;
        for _ in 0..100_000 {
            let current = script.run(result).unwrap();
            let path = match &current {
                RuntimeCommand::WIPE(wipe)
                    if kind == "WIPE"
                        && wipe.ty == 1
                        && wipe.duration > Ticks::ZERO
                        && state.layers.is_page_back_started =>
                {
                    scenario
                        .info_tables()
                        .mask_info
                        .get(wipe.params.0 as usize)
                        .map(|m| m.path())
                }
                RuntimeCommand::MASKLOAD(load) if kind == "MASKLOAD" => load
                    .mask_id
                    .repr()
                    .and_then(|id| scenario.info_tables().mask_info_opt(id))
                    .map(|m| m.path()),
                _ => None,
            };
            if let Some(path) = path {
                return (current, state, script, path);
            }
            if matches!(
                current,
                RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) | RuntimeCommand::RESUME(_)
            ) {
                break;
            }
            result = match current {
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut state);
                    get.token.finish(value)
                }
                current => {
                    command::apply_command_state_ref(&current, &mut state).unwrap();
                    let Some(result) = current.execute_dummy() else {
                        break;
                    };
                    result
                }
            };
        }
    }
    panic!("No authored {kind} before an unrecorded choice/exit");
}
fn owners(live: &Adv) -> serde_json::Value {
    let pages: Vec<_> = live
        .adv_state
        .screen_layer()
        .reference_pages()
        .map(|p| p.properties().identity())
        .collect();
    let masks: Vec<_> = (0..4)
        .map(|i| {
            let (mask, flags) = live
                .adv_state
                .plane_layer_group(PlaneId::new(i))
                .reference_mask_state();
            (mask.map(|m| Arc::as_ptr(m) as usize), flags.bits())
        })
        .collect();
    serde_json::json!({"root": live.adv_state.root_layer_group.properties().identity(),
        "screen":live.adv_state.screen_layer().properties().identity(), "pages": pages,"masks":masks})
}
fn acknowledge(
    live: &mut Adv,
    context: &mut PreRenderContext,
    server: &Arc<crate::asset::system::AssetServer>,
    frame: &mut FrameId,
    target: &wgpu::TextureView,
) {
    for _ in 0..20 {
        saved_io_errors::tick(live, context, server, frame, Default::default(), target);
    }
    let mut cancel = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
    cancel[AppAction::Cancel].is_clicked = true;
    saved_io_errors::tick(live, context, server, frame, cancel, target);
    assert!(
        live.restore_error_dialog.is_some(),
        "Cancel cannot drop the command"
    );
    let mut enter = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
    enter[AppAction::Enter].is_clicked = true;
    saved_io_errors::tick(live, context, server, frame, enter, target);
    for _ in 0..40 {
        if live.restore_error_dialog.is_none() {
            break;
        }
        saved_io_errors::tick(live, context, server, frame, Default::default(), target);
    }
    assert!(live.restore_error_dialog.is_none());
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.adv_state.audio_pause_count, 0);
}
pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    assets: &AdvAssets,
    scenario: &Arc<Scenario>,
    target: &wgpu::TextureView,
) {
    let out = PathBuf::from(std::env::var_os("SHIN_REFERENCE_LIVE_MASK_IO_OUTPUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let fault = IoFault::load();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let baseline = std::env::var("SHIN_REFERENCE_LIVE_MASK_IO_BASELINE").ok();
    let mut summaries = String::new();
    for kind in ["WIPE", "MASKLOAD"] {
        if baseline.as_deref().is_some_and(|k| k != kind) {
            continue;
        }
        for mode in if baseline.is_some() {
            vec![1]
        } else {
            vec![1, 2]
        } {
            let (runtime, state, script, path) = authored(scenario, kind);
            let entry = state.snapshot_entry().scenario_id;
            let plane = state.layers.current_plane;
            let mask_flags = match &runtime {
                RuntimeCommand::MASKLOAD(m) => Some(m.mask_flags),
                _ => None,
            };
            let pc = script.position();
            let server = fresh_server(context);
            let mut local = PreRenderContext {
                device: context.device,
                queue: context.queue,
                breakup_texture: context.breakup_texture,
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
            if kind == "WIPE" {
                // The VM snapshot does not retain PAGEBACK's previous page.
                // Use production pageback to compose source/target copies
                // of the real target scene, not a continuous-route claim.
                live.adv_state
                    .screen_layer_mut()
                    .pageback(&mut context.render_clone_ctx(), false);
            }
            let (archive, offset, payload) = supplied_payload(&path);
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
                panic!("Expected pending {kind}")
            };
            live.current_command = Some(command);
            let before_game = contexts(&live);
            let before_owners = owners(&live);
            let counters = live.vm_state.layers.layer_load_counter;
            for attempt in 0..if mode == 1 && baseline.is_none() {
                2
            } else {
                1
            } {
                let deadline = Instant::now() + Duration::from_secs(20);
                while live.restore_error_dialog.is_none() {
                    assert!(Instant::now() < deadline, "{kind} error notice timed out");
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
                assert!(selected[0] > 0 && selected[1] > 0);
                fault.arm(&archive, offset, payload.len() as u64, 0);
                let error = live.saved_scene_load_error().unwrap();
                assert!(error.contains(&path), "{kind}: {error}");
                assert!(
                    error.contains(if mode == 1 {
                        "os error 5"
                    } else {
                        "failed to read whole buffer"
                    }),
                    "{error}"
                );
                assert_eq!(live.adv_state.audio_pause_count, 1);
                assert_eq!(contexts(&live), before_game);
                assert_eq!(owners(&live), before_owners);
                assert_eq!(live.scripter.position(), pc);
                assert!(live.current_command.is_some());
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
                assert_eq!(
                    shin_tasks::async_io::spawn(async { 42 })
                        .poll_with_timeout(Duration::from_secs(2)),
                    Some(42)
                );
                acknowledge(&mut live, context, &server, &mut frame, target);
                assert_eq!(contexts(&live), before_game);
                assert_eq!(owners(&live), before_owners);
                assert_eq!(live.scripter.position(), pc);
                if attempt == 0 && mode == 1 && baseline.is_none() {
                    fault.arm(&archive, offset, payload.len() as u64, mode);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut saw_mask_wipe = false;
            loop {
                assert!(Instant::now() < deadline, "{kind} retry timed out");
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
                if let Some(error) = live.current_command.as_mut().unwrap().take_error() {
                    panic!("Retry failed: {error:#}");
                }
                saw_mask_wipe |= matches!(
                    live.adv_state.screen_layer().reference_active_wiper(),
                    Some(crate::wiper::AnyWiper::Mask(_))
                );
                live.adv_state.update(&mut update);
                live.adv_state
                    .root_layer_group
                    .pre_render(context, &Default::default());
                {
                    let mut pass = context.begin_pass(
                        shin_render::shaders::types::texture::TextureTarget {
                            view: target,
                            kind:
                                shin_render::shaders::types::texture::TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Mask retry",
                    );
                    live.adv_state.render_scene(&mut pass);
                }
                let encoder = std::mem::replace(
                    context.encoder,
                    context.device.create_command_encoder(&Default::default()),
                );
                let mut upload = context.device.create_command_encoder(&Default::default());
                context.dynamic_buffer.finish(&mut upload);
                context.queue.submit([upload.finish(), encoder.finish()]);
                context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
                if done {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(contexts(&live), before_game);
            assert_eq!(live.scripter.position(), pc);
            assert_eq!(live.vm_state.layers.layer_load_counter, counters);
            if kind == "WIPE" {
                assert!(
                    saw_mask_wipe,
                    "Mandatory mask cannot become a Default/no-op transition"
                );
            } else {
                let (mask, flags) = live
                    .adv_state
                    .plane_layer_group(plane)
                    .reference_mask_state();
                assert_eq!(flags, mask_flags.unwrap());
                assert!(mask.unwrap().label.contains(&path));
            }
            assert!(live.adv_state.allow_running_animations);
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
                "{kind} mode{mode} entry{entry} pc={:x} {path}: notice, preserved command/VM/pages/masks, balanced acknowledgement, {} failure(s), same-command real mask publication and final cache retirement pass\n",
                pc.0,
                if mode == 1 { 2 } else { 1 }
            );
            print!("{row}");
            summaries.push_str(&row);
        }
    }
    if baseline.is_none() {
        // Reuse the existing native-policy regression for both animation
        // flags, missing metadata and captured-plane publication.
        let server = fresh_server(context);
        let mut local = PreRenderContext {
            device: context.device,
            queue: context.queue,
            breakup_texture: context.breakup_texture,
            resize_source: context.resize_source,
            sampler_store: context.sampler_store,
            depth_stencil: context.depth_stencil,
            pipeline_storage: &mut *context.pipeline_storage,
            dynamic_buffer: &mut *context.dynamic_buffer,
            encoder: &mut *context.encoder,
        };
        let mut adv = crate::adv::AdvState::new(audio.clone(), server.clone(), assets.clone());
        crate::adv::command::reference_live_maskload_policy(
            &mut local, &server, scenario, &mut adv,
        );
        println!(
            "Existing MASKLOAD animation/metadata/captured-plane policy regression passed"
        );
    }
    std::fs::write(out.join("live-mask-io.txt"), summaries).unwrap();
}
