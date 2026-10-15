// Bounded live scene replacement with real ROM resources. The gates hold
// publication of real worker results; they do not simulate native IO timing.
mod active_restore {
    use super::*;
    use crate::{
        adv::{Adv, AdvAssets, CommandStartResult, PendingSceneRestore},
        asset::system::AssetServer,
        layer::{DrawableLayer, user::UserLayer},
        render::PreRenderContext,
        update::UpdateContext,
    };
    use shin_core::{
        format::{
            save::GameData,
            scenario::{
                instruction_elements::{FromNumber, MessageId, U8Bool},
                types::U16FixupString,
            },
        },
        primitives::update::FrameId,
        time::{Ticks, Tween},
        vm::{
            IntoRuntimeForm, VmCtx,
            command::types::{AudioWaitStatus, LayerId, Pan, Volume},
        },
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    fn n<T>(v: i32) -> NumberSpec<T> {
        NumberSpec::new(NumberSpec::constant(v).into_untyped())
    }
    fn params(
        values: [i32; 8],
    ) -> shin_core::format::scenario::instruction_elements::BitmaskNumberArray {
        let mut cursor = std::io::Cursor::new(vec![255u8]);
        cursor.set_position(1);
        for value in values {
            NumberSpec::constant(value).write_le(&mut cursor).unwrap();
        }
        cursor.set_position(0);
        shin_core::format::scenario::instruction_elements::BitmaskNumberArray::read_le(&mut cursor)
            .unwrap()
    }
    fn source(original: &Scenario, name: &str) -> Arc<Scenario> {
        // Preserve all actual metadata/string tables; isolate only the script.
        let mut bytes = original.raw().to_vec();
        let offset = bytes.len();
        bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(offset as u64);
        let mut cmds = vec![
            C::LAYERLOAD(c::LAYERLOAD {
                layer_id: n(1),
                layer_type: n(1),
                flags: n(0),
                params: params([0xf090, -960, -540, 1920, 1080, 0, 0, 0]),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rRestored scene boundary"),
            }),
        ];
        if name.starts_with("movie") {
            let movie_name = if name == "movie-opening" {
                "op1"
            } else {
                "door"
            };
            let id = original
                .info_tables()
                .movie_info
                .iter()
                .position(|m| m.name.as_str().eq_ignore_ascii_case(movie_name))
                .unwrap();
            cmds.push(C::LAYERLOAD(c::LAYERLOAD {
                layer_id: n(1),
                layer_type: n(6),
                flags: n(0),
                params: params([
                    id as i32,
                    1000,
                    i32::from(name == "movie-loop"),
                    0,
                    0,
                    0,
                    0,
                    0,
                ]),
            }));
            cmds.push(C::MOVIEWAIT(c::MOVIEWAIT {
                layer_id: n(1),
                unwanted_statuses: n(2),
            }));
        } else if name == "wipe-running" {
            cmds.push(C::PAGEBACK(c::PAGEBACK {}));
            cmds.push(C::LAYERLOAD(c::LAYERLOAD {
                layer_id: n(1),
                layer_type: n(1),
                flags: n(0),
                params: params([0xf009, -960, -540, 1920, 1080, 0, 0, 0]),
            }));
            cmds.push(C::WIPE(c::WIPE {
                ty: n(0),
                flags: n(0),
                duration: n(120),
                params: params([0; 8]),
            }));
        } else {
            cmds.push(C::WAIT(c::WAIT {
                wait_amount: n(10000),
                allow_interrupt: U8Bool(false),
            }));
        }
        // An old command completing must never reach/publish this after load.
        cmds.push(C::SSET(c::SSET {
            slot_number: n(511),
            value: n(1777),
        }));
        cmds.push(C::EXIT(c::EXIT {
            arg1: 0,
            arg2: n(0),
        }));
        for cmd in cmds {
            Instruction::Command(cmd).write_le(&mut cursor).unwrap();
        }
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        Arc::new(Scenario::new(bytes.into()).unwrap())
    }
    fn step<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        ticks: f32,
        click: bool,
    ) {
        let mut input = enum_map::EnumMap::default();
        if click {
            input[crate::app::AppAction::Enter] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                is_held: true,
                ..Default::default()
            };
        }
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(ticks),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        frame.advance();
    }
    fn start<'a>(
        live: &mut Adv,
        command: C,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: FrameId,
    ) -> CommandStartResult {
        command::apply_command_state_and_start(
            command.into_runtime_form(&VmCtx::new(0, 42)),
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            &live.scenario,
            &mut live.vm_state,
            &mut live.adv_state,
        )
    }
    fn frozen_pixels(
        context: &mut PreRenderContext,
        source: &shin_render::render_texture::RenderTexture,
    ) -> Vec<u8> {
        use shin_render::{
            ColorBlendType, RenderRequestBuilder,
            quad_vertices::QuadVertices,
            shaders::types::texture::{TextureTarget, TextureTargetKind},
        };
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Active restore captured source"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Active restore captured source",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                None,
                None,
            );
            QuadVertices::new()
                .with_box(0., 0., 1920., 1080.)
                .with_tex_box(0., 0., 1., 1.)
                .render_sprite(
                    &mut pass,
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::LayerPremultiplied1),
                    source.as_texture_source(),
                    crate::render::top_left_projection_matrix(),
                );
        }
        crate::layer::user::readback_test_texture(context, &target)
    }
    pub(super) fn verify<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        assets: &AdvAssets,
        original: &Arc<Scenario>,
    ) {
        let version = if std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some() {
            "1.03"
        } else {
            "original"
        };
        for name in [
            "movie-once",
            "movie-loop",
            "movie-opening",
            "wipe-running",
            "layer-loader",
            "wipe-loader",
        ] {
            let scenario = source(original, name);
            let mut assets = assets.clone();
            assets.scenario = scenario.clone();
            let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut live = Adv::new(
                audio.clone(),
                server.clone(),
                assets,
                Scripter::new(&scenario, 0, 42),
            );
            let mut frame = FrameId::default();
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let ready = if name.starts_with("movie") {
                    matches!(
                        live.current_command,
                        Some(command::ExecutingCommand::MOVIEWAIT(_))
                    )
                } else if name == "wipe-running" {
                    matches!(
                        live.current_command,
                        Some(command::ExecutingCommand::WIPE(_))
                    ) && live.adv_state.screen_layer().is_transition_active()
                } else {
                    matches!(
                        live.current_command,
                        Some(command::ExecutingCommand::WAIT(_))
                    )
                };
                if ready {
                    break;
                }
                assert!(
                    !live.is_finished(),
                    "{name} exited before source work started"
                );
                assert!(
                    Instant::now() < deadline,
                    "{name} failed to start: {:?}",
                    live.current_command
                );
                let click = matches!(
                    live.current_command,
                    Some(command::ExecutingCommand::MSGSET(_))
                );
                step(&mut live, context, server, &mut frame, 1., click);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(live.vm_state.save_position, 1);
            let plane = live.vm_state.layers.current_plane;
            let bank = live
                .vm_state
                .layers
                .layerbank_allocator
                .get_layerbank_id(plane, LayerId::new(1))
                .unwrap();
            let movie_owner = if name.starts_with("movie") {
                let UserLayer::Movie(movie) = &*live
                    .adv_state
                    .plane_layer_group(plane)
                    .get_layer(bank)
                    .unwrap()
                else {
                    panic!();
                };
                assert!(!movie.is_finished(), "Source must have unfinished playback");
                Some(movie.reference_publication_owner())
            } else {
                None
            };
            let asset_gate = if name == "layer-loader" || name == "wipe-loader" {
                // Start the actual command before its first publication poll.
                // The source script is parked at WAIT; this prepared command
                // isolates cancellation, not original native worker scheduling.
                let cmd = if name == "layer-loader" {
                    C::LAYERLOAD(c::LAYERLOAD {
                        layer_id: n(1),
                        layer_type: n(2),
                        flags: n(0),
                        params: params([0; 8]),
                    })
                } else {
                    assert!(matches!(
                        start(
                            &mut live,
                            C::PAGEBACK(c::PAGEBACK {}),
                            context,
                            server,
                            frame
                        ),
                        CommandStartResult::Continue(_)
                    ));
                    C::WIPE(c::WIPE {
                        ty: n(1),
                        flags: n(0),
                        duration: n(120),
                        params: params([0, 1000, 0, 0, 0, 0, 0, 0]),
                    })
                };
                let CommandStartResult::Yield(mut executing) =
                    start(&mut live, cmd, context, server, frame)
                else {
                    panic!("{name} must yield");
                };
                let gate = command::reference_hold_asset_request(&mut executing);
                live.current_command = Some(executing);
                assert!(!live.adv_state.allow_running_animations);
                Some(gate)
            } else {
                None
            };
            let bgm = &original.info_tables().bgm_info[0];
            live.adv_state.audio.bgm_player.request(
                server.clone(),
                bgm.path().to_owned(),
                FromNumber::from_number(0),
                true,
                Volume(0.75),
                Tween::IMMEDIATE,
            );
            let bgm_gate = live.adv_state.audio.bgm_player.reference_hold_loading();
            live.adv_state.audio.se_player.request(
                server.clone(),
                original
                    .info_tables()
                    .se_info(FromNumber::from_number(0))
                    .path()
                    .to_owned(),
                0,
                FromNumber::from_number(0),
                true,
                Volume(0.7),
                Pan(0.),
                1.,
                Tween::IMMEDIATE,
            );
            let se_gate = live.adv_state.audio.se_player.reference_hold_loading();
            assert!(
                live.adv_state
                    .audio
                    .bgm_player
                    .get_wait_status()
                    .contains(AudioWaitStatus::LOADING)
            );
            assert!(
                live.adv_state
                    .audio
                    .se_player
                    .get_wait_status(0)
                    .contains(AudioWaitStatus::LOADING)
            );
            let position = live.scripter.position();
            let route = live.vm_state.route.bit_position();
            let movie_time = movie_owner.as_ref().map(|owner| owner.time());
            // Serialize and decode the actual entry before using the public load
            // path, retaining both entries during real worker activity.
            let entry = GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: 1,
                route_data: Default::default(),
            };
            let mut globals = SaveData::default();
            globals.manual_save_slots[0] = Some(GameData::from_entry(entry));
            let mut inactive_route = RouteRecorder::default();
            inactive_route.record_persistent(-123);
            inactive_route.record_choice(6);
            globals.manual_save_slots[0].as_mut().unwrap().primary_entry = Some(GameDataEntry {
                scenario_id: 1,
                random_seed: 99,
                save_position: 0,
                route_data: inactive_route.snapshot(),
            });
            let globals = SaveData::decode(&globals.encode()).unwrap();
            let game = globals.manual_save_slots[0].as_ref().unwrap();
            let retained = format!("{:?}", game.primary_entry);
            // Pending restoration cannot publish either entry into the old VM.
            live.vm_state.primary_entry = None;
            live.load_game_data(&globals, game, server.clone()).unwrap();
            let mut pending = live.pending_restore.take().unwrap();
            let task = pending.task.take().unwrap();
            let (release, receiver) = futures::channel::oneshot::channel();
            live.pending_restore = Some(PendingSceneRestore {
                task: Some(shin_tasks::async_io::spawn(async move {
                    receiver.await.unwrap();
                    task.await
                })),
                ready: pending.ready.take(),
                retirement: None,
                kind: pending.kind,
                source: pending.source.take(),
                present_source: pending.present_source,
            });
            for _ in 0..3 {
                step(&mut live, context, server, &mut frame, 5., true);
                assert!(live.vm_state.primary_entry.is_none());
                assert_eq!(
                    live.scripter.position(),
                    position,
                    "Old VM gated while loading"
                );
                assert_eq!(live.vm_state.route.bit_position(), route);
                assert_eq!(live.vm_state.persist.get(511), 0);
                assert!(live.pending_restore.is_some());
                assert!(
                    live.adv_state
                        .audio
                        .bgm_player
                        .get_wait_status()
                        .contains(AudioWaitStatus::LOADING)
                );
                assert!(
                    live.adv_state
                        .audio
                        .se_player
                        .get_wait_status(0)
                        .contains(AudioWaitStatus::LOADING)
                );
            }
            if let (Some(owner), Some(time)) = (&movie_owner, movie_time) {
                // Silent door uses deterministic ADV ticks. Audible opening's
                // clock comes from its real audio callback, not the fixture.
                if name != "movie-opening" {
                    assert!(
                        owner.time() > time,
                        "Old movie must keep advancing during ordinary loading"
                    );
                }
            }
            if name == "wipe-running" {
                assert!(live.adv_state.screen_layer().is_transition_active());
            }
            // Read the load operation's actual frozen texture. Movie workers
            // can publish a newer frame between an earlier screenshot and the
            // snapshot's pre_render, even when no ADV ticks have elapsed.
            let source_pixels = frozen_pixels(
                context,
                live.pending_restore
                    .as_ref()
                    .unwrap()
                    .source
                    .as_ref()
                    .unwrap(),
            );
            if name == "wipe-running" {
                let moving = snapshot_pixels(context, &live, true);
                assert!(
                    [0usize, 1919, 1079 * 1920, 1080 * 1920 - 1]
                        .into_iter()
                        .any(|pixel| source_pixels[4 * pixel..4 * pixel + 3]
                            != moving[4 * pixel..4 * pixel + 3]),
                    "Old wipe must keep advancing while the replacement loads"
                );
            }
            release.send(()).unwrap();
            while live.pending_restore.is_some() {
                assert!(Instant::now() < deadline, "{name} replacement timed out");
                step(&mut live, context, server, &mut frame, 0., true);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                live.saved_scene_load_error().is_none(),
                "{:?}",
                live.saved_scene_load_error()
            );
            assert!(!live.adv_state.screen_layer().is_transition_active());
            assert!(!live.vm_state.layers.is_page_back_started);
            assert!(live.adv_state.allow_running_animations);
            assert!(matches!(
                live.current_command,
                Some(command::ExecutingCommand::MSGSET(_))
            ));
            assert_eq!(live.vm_state.save_position, 1);
            assert_eq!(live.vm_state.persist.get(511), 0);
            assert_eq!(live.vm_state.snapshot_entry().scenario_id, 0);
            assert_eq!(live.vm_state.snapshot_entry().random_seed, 42);
            assert_eq!(format!("{:?}", live.vm_state.primary_entry), retained);
            live.vm_state.capture_autosave();
            assert_eq!(
                format!(
                    "{:?}",
                    live.vm_state.auto_save_slot.as_ref().unwrap().primary_entry
                ),
                retained
            );
            let installed = live.scripter.position();
            if let Some(owner) = movie_owner {
                owner.assert_released();
            }
            let cancel_deadline = Instant::now() + Duration::from_secs(5);
            while !bgm_gate.is_canceled()
                || !se_gate.is_canceled()
                || asset_gate.as_ref().is_some_and(|gate| !gate.is_canceled())
            {
                assert!(
                    Instant::now() < cancel_deadline,
                    "{name} old request was not canceled"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(bgm_gate.send(()).is_err());
            assert!(se_gate.send(()).is_err());
            if let Some(gate) = asset_gate {
                assert!(gate.send(()).is_err());
            }
            assert!(live.adv_state.audio.bgm_player.get_wait_status().is_empty());
            assert!(live.adv_state.audio.se_player.get_wait_status(0).is_empty());
            // The source texture is captured before the old movie/wipe moves.
            // Read safe corners, outside text/UI, at all load-fade quarters.
            let corners = [0usize, 1919, 1079 * 1920, 1080 * 1920 - 1];
            for phase in 0..=4 {
                if phase != 0 {
                    step(&mut live, context, server, &mut frame, 7.5, true);
                }
                let pixels = snapshot_pixels(context, &live, true);
                let fade = live.adv_state.scene_load_fade.take();
                let fresh = snapshot_pixels(context, &live, true);
                live.adv_state.scene_load_fade = fade;
                let a = 1. - phase as f32 / 4.;
                for pixel in corners {
                    for channel in 0..3 {
                        let i = 4 * pixel + channel;
                        let expected = (source_pixels[i] as f32 * a + fresh[i] as f32 * (1. - a))
                            .round() as u8;
                        assert!(
                            pixels[i].abs_diff(expected) <= 1,
                            "{name} frozen fade phase{phase} pixel{pixel} channel{channel}: {} versus{expected}",
                            pixels[i]
                        );
                    }
                }
                assert_eq!(live.scripter.position(), installed);
                assert_eq!(live.vm_state.persist.get(511), 0);
                assert_eq!(live.is_loading_saved_scene(), phase < 4);
            }
            // No held source input may confirm the restored message during the
            // fade. One ordinary update still waits for a new user action.
            step(&mut live, context, server, &mut frame, 1., false);
            assert!(matches!(
                live.current_command,
                Some(command::ExecutingCommand::MSGSET(_))
            ));
            assert_eq!(live.scripter.position(), installed);
            assert_eq!(live.vm_state.persist.get(511), 0);
            assert!(live.adv_state.audio.bgm_player.get_wait_status().is_empty());
            assert!(live.adv_state.audio.se_player.get_wait_status(0).is_empty());
            if name == "layer-loader" {
                let mut invalid = game.clone();
                invalid.entry.save_position = u32::MAX;
                assert!(
                    live.load_game_data(&globals, &invalid, server.clone())
                        .is_err()
                );
                assert_eq!(format!("{:?}", live.vm_state.primary_entry), retained);
                assert!(live.pending_restore.is_none());
                // A later single-entry load replaces the context instead of
                // inheriting the inactive entry from the previous live scene.
                let single = GameData::from_entry(game.entry.clone());
                live.load_game_data(&globals, &single, server.clone())
                    .unwrap();
                assert_eq!(format!("{:?}", live.vm_state.primary_entry), retained);
                let deadline = Instant::now() + Duration::from_secs(5);
                while live.is_loading_saved_scene() {
                    assert!(
                        Instant::now() < deadline,
                        "single-entry context replacement timed out"
                    );
                    step(&mut live, context, server, &mut frame, 1., false);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(live.vm_state.primary_entry.is_none());
                live.vm_state.capture_autosave();
                assert!(
                    live.vm_state
                        .auto_save_slot
                        .as_ref()
                        .unwrap()
                        .primary_entry
                        .is_none()
                );
                // Hold real reconstructed resources inside a suspended loader.
                // Both replacement and shutdown must finish cancellation before
                // returning, rather than leaving cleanup on the IO executor.
                struct RetainedScene {
                    scene: Option<super::super::super::RestoredScene>,
                    retired: Arc<std::sync::atomic::AtomicBool>,
                }
                impl Drop for RetainedScene {
                    fn drop(&mut self) {
                        drop(self.scene.take());
                        self.retired
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                }
                for shutdown in [false, true] {
                    let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let worker_retired = retired.clone();
                    let (entered, arrival) = std::sync::mpsc::channel();
                    let scene = Box::new(
                        ReconstructedScene::from_game_data(&scenario, &single, &globals, 1_000_000)
                            .unwrap(),
                    );
                    let worker_server = server.clone();
                    let worker_audio = audio.clone();
                    let worker_scenario = scenario.clone();
                    live.pending_restore = Some(PendingSceneRestore {
                        task: Some(shin_tasks::async_io::spawn(async move {
                            let loaded = super::super::super::restoration::LoadedScene::load(
                                &worker_server,
                                &worker_audio,
                                &worker_scenario,
                                &scene.state,
                            )
                            .await?;
                            let _retained = RetainedScene {
                                scene: Some((loaded, scene)),
                                retired: worker_retired,
                            };
                            entered.send(()).unwrap();
                            std::future::pending::<anyhow::Result<super::super::super::RestoredScene>>().await
                        })),
                        ready: None,
                        retirement: None,
                        kind: super::super::super::SceneLoadKind::Saved,
                        source: None,
                        present_source: false,
                    });
                    arrival.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(!retired.load(std::sync::atomic::Ordering::Acquire));
                    if shutdown {
                        // Transfer the prepared loader to a disposable live ADV;
                        // its shutdown must drain it before retiring scene fields.
                        let mut dying = Adv::new(
                            audio.clone(),
                            server.clone(),
                            live.assets.clone(),
                            Scripter::new(&scenario, 0, 42),
                        );
                        dying.pending_restore = live.pending_restore.take();
                        drop(dying);
                    } else {
                        live.load_game_data(&globals, &single, server.clone())
                            .unwrap();
                    }
                    assert!(retired.load(std::sync::atomic::Ordering::Acquire));
                    if !shutdown {
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while live.is_loading_saved_scene() {
                            assert!(Instant::now() < deadline);
                            step(&mut live, context, server, &mut frame, 1., false);
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        assert!(live.saved_scene_load_error().is_none());
                    }
                }
                eprintln!(
                    "{version}: replacement and shutdown synchronously retire suspended real-resource scene loaders"
                );
            }
            eprintln!(
                "{version} {name}: two-entry codec roundtrip and inactive retention, real scene load, pending VM gate, old movie/loader/audio retirement, frozen30tick fade and fresh message wait pass"
            );
            drop(live);
            drop(audio);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
