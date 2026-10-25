use super::selection::Frames;
use super::*;
use crate::app::higurashi::gameplay::selection::character as character_service;
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio, character_selection::view::View, input::OverlayInput,
            layers::Target, reference_gpu_assets, reference_render_to_texture,
            touch::Overlay as InputOverlay,
        },
        input_area::{PointerEvent, PointerKind},
    },
    layer::user::higurashi::{HigurashiLayerKind, HigurashiUserLayer},
};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::atomic::{AtomicUsize, Ordering};

const CODE: &[u8] = &[
    // Leave logical-ID1 metadata in a freed physical slot. Native type11's
    // new base owner ID remains0 when the temporary command reuses that slot.
    0xc1, 1, 0, 0, 0, 0xc2, 1, 0, 0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0xd1, 18, 0,
    31, 0, 5, 0x82, 9, 0xc0, 31, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
];
pub(in crate::app::higurashi::gameplay) fn common(
    server: &Arc<AssetServer>,
    code: &[u8],
) -> (Arc<HigurashiScenario>, Arc<CommonAssets>) {
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    (
        authored,
        Arc::new(CommonAssets {
            scenario,
            font: server.load_sync("/newrodin.fnt").unwrap(),
            system: server.load_sync("/systex.txa").unwrap(),
            message: server.load_sync("/msgtex.txa").unwrap(),
            effects: server.load_sync("/effects.txa").unwrap(),
            sysse: server.load_sync("/sysse.bin").unwrap(),
        }),
    )
}
fn owner(game: &Gameplay) -> Option<(Target, &View)> {
    game.selection
        .character
        .job
        .as_ref()?
        .ui
        .as_ref()
        .map(|(target, view)| (*target, view))
}
fn frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    game: &mut Gameplay,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        game.prepare(context).unwrap();
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Character gameplay",
        );
        game.render(&mut pass).unwrap();
    })
}
struct Replacement;
impl InputOverlay for Replacement {
    fn identity(&self) -> usize {
        0x123456
    }
    fn input_permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: false,
            hide_window: false,
        }
    }
    fn probe(&mut self, _: glam::Vec2) -> bool {
        false
    }
    fn press(&mut self, _: glam::Vec2) {}
    fn release(&mut self, _: glam::Vec2) {}
    fn cancel(&mut self) {}
}
#[test]
#[ignore = "actual Gameplay CHARSELECT atlas worker/scene/input wrapper/controller/contact/cues/11780/result/conditional retirement and unanswered reconstruction at720p/1080p on the default audio output"]
fn higurashi_gameplay_character_selection_returns_original_index_and_retires_scene_and_wrapper() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (authored, common) = common(&server, CODE);
    for (kind, size) in [
        (0, (1280, 720)),
        (1, (1920, 1080)),
        (2, (1280, 720)),
        (3, (1920, 1080)),
    ] {
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut saved = SaveData::default();
        saved.settings.common.v14_showroutenavi = false;
        saved.settings.common.v3_sysvol = 37;
        let mut slot = fresh();
        slot.trailing_values = vec![7, 0xffff];
        slot.byte_variables = vec![1, 255];
        saved.auto_save_slot = Some(slot.clone());
        saved.manual_save_slots[2] = Some(slot.clone());
        let globals = Globals {
            storage: None,
            runtime_saves: None,
            common: common.clone(),
            data: saved,
            clock: PlayClock::from_saved_seconds(0),
            platform_flag: true,
            last_title_mode: 0,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
        };
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let mut snapshot = context.new_render_texture("Character initial".into());
            {
                let mut pass = context.begin_pass(
                    snapshot.as_texture_target(),
                    Some(context.depth_stencil),
                    "Character initial",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            let mut data = fresh();
            data.entry.random_seed = 42;
            game = Some(Gameplay::new(globals, data, 0, snapshot, || [1, 2, 3, 4]).unwrap());
        });
        let mut game = game.unwrap();
        let mut frames = Frames { number: 0 };
        let date = SaveDate {
            year: 2026,
            month: 10,
            day: 12,
            hour: 18,
            minute: 21,
            second: 34,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        game.set_clock_source(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(date)
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            frames.idle(&mut game);
            if owner(&game).is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "CHARSELECT did not load");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (target, view) = owner(&game).unwrap();
        let model = view.model.clone();
        let id = Arc::as_ptr(&model) as usize;
        let retirement = game
            .selection
            .character
            .job
            .as_ref()
            .unwrap()
            .retirement
            .clone();
        // The wrapper polls phase before the scene advances each clock once.
        assert_eq!(model.lock().phase, 1);
        assert_eq!(model.lock().clocks[1].value(), 0.05);
        assert_eq!(game.reference_overlay_cues, ["select"]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(game.globals.sounds.lock().volume_percent, 37);
        assert!(Arc::ptr_eq(
            game.globals.sounds.lock().bank.as_ref().unwrap(),
            &common.sysse
        ));
        let layer = game
            .host()
            .unwrap()
            .runner
            .frame()
            .scene
            .layers()
            .layer(target)
            .unwrap();
        assert_eq!(layer.layer_type(), HigurashiLayerKind::CharacterSelection);
        assert_eq!(
            game.host()
                .unwrap()
                .runner
                .frame()
                .scene
                .layers()
                .group_children()[0]
                .owner_id,
            0
        );
        assert_eq!(layer.properties.values()[5], 1.0);
        assert_eq!(layer.properties.values()[22], 1.0);
        assert!(Arc::ptr_eq(&layer.character_view().unwrap().model, &model));
        assert!(!layer.character_view().unwrap().route_navigation);
        assert_eq!(
            game.host().unwrap().reference_touch_overlay_identity(),
            Some(id)
        );
        // Scene copies are independent; wrapper/intrusive retention is shared.
        let copied = layer.native_copy().unwrap();
        assert!(!Arc::ptr_eq(
            &copied.character_view().unwrap().model,
            &model
        ));
        drop(copied);
        let deadline = Instant::now() + Duration::from_secs(10);
        while model.lock().phase != 2 {
            frames.idle(&mut game);
            assert!(Instant::now() < deadline, "Character screen did not open");
        }
        assert_eq!(
            model
                .lock()
                .rows
                .iter()
                .map(|row| row.original)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        let initial = game.globals.data.auto_save_slot.clone().unwrap();
        assert_eq!(initial.entry.save_position, 2);
        assert_eq!(initial.entry.replay_target, 2);
        assert_eq!(initial.date, date);
        assert!(
            game.host()
                .unwrap()
                .runner
                .native_playback_descriptors()
                .unwrap()
                .message
                .text
                .is_none()
        );
        frames.step(
            &mut game,
            Buttons {
                pressed: 1 << 25,
                repeat: (1 << 5) | (1 << 7),
                ..Default::default()
            },
            &[],
        );
        assert!(!game.host().unwrap().has_modes());
        assert_eq!(model.lock().phase, 2);
        assert_eq!(model.lock().selected, 0);
        assert!(game.pending_input_request().is_none());
        let pixels = frame(&device, &queue, size, &mut game);
        assert!(
            pixels
                .chunks_exact(4)
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        );
        game.globals.data.settings.common.v14_showroutenavi = true;
        frames.idle(&mut game);
        assert!(
            game.host()
                .unwrap()
                .runner
                .frame()
                .scene
                .layers()
                .layer(target)
                .unwrap()
                .character_view()
                .unwrap()
                .route_navigation
        );
        let routed = frame(&device, &queue, size, &mut game);
        assert_ne!(
            pixels, routed,
            "Route setting did not reach the scene renderer"
        );
        if kind == 3 {
            let fork = game
                .bindings
                .memory
                .fork_for_restore(0, 0, &game.bindings.progress)
                .unwrap();
            let mut replay = Some(ReplaySession::new(common.scenario.clone(), fork).unwrap());
            let history = game.bindings.history.clone();
            let mut screens = game.screens.take().unwrap();
            reference_render_to_texture(&device, &queue, size, |context, _| {
                game.host_mut()
                    .unwrap()
                    .begin_scene_reconstruction(
                        &mut replay,
                        0,
                        context,
                        &mut screens,
                        history,
                        || [1, 2, 3, 4],
                    )
                    .unwrap();
            });
            game.screens = Some(screens);
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                frames.idle(&mut game);
                if !game.host().unwrap().runner.native_vm_attached() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Character source was not retired"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(game.selection.character.job.is_none());
            assert!(retirement.is_finished());
            assert_eq!(
                game.host().unwrap().reference_touch_overlay_identity(),
                None
            );
            assert_eq!(model.lock().result, None);
            assert_eq!(Arc::strong_count(&model), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert!(game.bindings.progress.pending_persistent().is_empty());
            assert_eq!(
                game.globals
                    .data
                    .auto_save_slot
                    .as_ref()
                    .unwrap()
                    .entry
                    .save_position,
                2
            );
            continue;
        }
        if kind == 1 {
            let y = (model.lock().top + 150 + 3 + 72) as f32 / 1.5;
            let down = PointerEvent {
                kind: PointerKind::Down,
                id: 17,
                position: glam::ivec2(600, y as i32),
            };
            frames.step(&mut game, Buttons::default(), &[down]);
            assert_eq!(model.lock().selected, 0);
            for _ in 0..10 {
                frames.idle(&mut game);
            }
            assert_eq!(model.lock().selected, 1);
            let up = PointerEvent {
                kind: PointerKind::Up,
                id: 17,
                position: glam::ivec2(0, 0),
            };
            frames.step(&mut game, Buttons::default(), &[up]);
        } else {
            frames.step(
                &mut game,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
                &[],
            );
            assert_eq!(model.lock().selected, 1);
            frames.step(
                &mut game,
                Buttons {
                    pressed: 1 << 24,
                    repeat: 1 << 4,
                    ..Default::default()
                },
                &[],
            );
            assert_eq!(model.lock().selected, 1);
        }
        assert_eq!(model.lock().phase, 3);
        let deadline = Instant::now() + Duration::from_secs(10);
        while model.lock().result.is_none() {
            frames.idle(&mut game);
            assert!(
                Instant::now() < deadline,
                "Character closing did not complete"
            );
        }
        assert_eq!(model.lock().result, Some(2));
        assert!(game.selection.character.job.is_some());
        if kind == 2 {
            game.host_mut()
                .unwrap()
                .runner
                .scene_layers_mut()
                .unwrap()
                .publish_temporary_front(target, HigurashiUserLayer::null([1; 4]))
                .unwrap();
            game.host_mut()
                .unwrap()
                .set_touch_overlay(Some(Box::new(Replacement)));
        }
        frames.idle(&mut game);
        assert!(game.selection.character.job.is_none());
        assert!(retirement.is_finished());
        assert_eq!(Arc::strong_count(&model), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(game.bindings.progress.pending_persistent(), [(9, 2)]);
        assert_eq!(game.bindings.progress.responses().1, 4);
        assert_eq!(game.bindings.progress.responses().0[0] >> 4, 2);
        assert_eq!(game.bindings.choices.last()[17], 3);
        assert!(game.bindings.choices.seen().iter().next().is_none());
        assert_eq!(
            game.host()
                .unwrap()
                .runner
                .native_message_timeline()
                .unwrap()
                .boundary,
            3
        );
        let final_slot = game.globals.data.auto_save_slot.as_ref().unwrap();
        assert_eq!(final_slot.entry.save_position, 3);
        assert_eq!(final_slot.entry.replay_target, 3);
        assert_eq!(final_slot.entry.response_data[0] >> 4, 2);
        assert_eq!(final_slot.trailing_values, slot.trailing_values);
        assert_eq!(final_slot.byte_variables, slot.byte_variables);
        assert_eq!(game.globals.data.manual_save_slots[2], Some(slot));
        assert!(
            game.host()
                .unwrap()
                .runner
                .frame()
                .scene
                .layers()
                .state
                .target(target.logical)
                .is_none()
        );
        if kind == 2 {
            assert_eq!(
                game.host().unwrap().reference_touch_overlay_identity(),
                Some(0x123456)
            );
            assert_eq!(
                game.host()
                    .unwrap()
                    .runner
                    .frame()
                    .scene
                    .layers()
                    .layer(target)
                    .unwrap()
                    .layer_type(),
                HigurashiLayerKind::Null
            );
        } else {
            assert_eq!(
                game.host().unwrap().reference_touch_overlay_identity(),
                None
            );
            assert!(
                game.host()
                    .unwrap()
                    .runner
                    .frame()
                    .scene
                    .layers()
                    .layer(target)
                    .is_none()
            );
        }
        assert_eq!(
            game.reference_overlay_cues,
            if kind == 1 {
                vec!["select", "decide"]
            } else {
                vec!["select", "move", "decide"]
            }
        );
    }
    eprintln!(
        "Gameplay characters:720p/controller and1080p/contact,real charseltxa scene plus non-drawing wrapper,once-per-frame clocks,original index2,A priority/B-horizontal inert,native select/move/decide,route settings,both11780 copies/four-bit response/last without seen/script feedback,conditional replacement and unanswered reconstruction/owner retirement pass"
    );
}

#[test]
#[ignore = "real retained character atlas producer held at delivery,runner reconstruction and cancellation on offscreen Metal/the default audio output;no window/saves"]
fn higurashi_character_loader_retirement_drains_pending_output_without_copy_or_result() {
    let (_, _, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (authored, common) = common(
        &server,
        &[0xd1, 18, 0, 31, 0, 5, 0x83, 0, 0x83, 0xe8, 0, 0, 0],
    );
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut audio = HigurashiAudio::new(manager, server.clone(), authored);
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        server.clone(),
        common.scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut saved = SaveData::default();
    let mut data = fresh();
    data.entry.random_seed = 42;
    let mut bindings = Bindings::new(&saved, &data).unwrap();
    let mut initial_clock: Box<dyn FnMut() -> Result<SaveDate> + Send> =
        Box::new(|| Ok(SaveDate::now()));
    let mut reads = bindings::LiveReads {
        bindings: &mut bindings,
        saved: &mut saved,
        clock: &mut initial_clock,
    };
    shin_tasks::block_on(runner.pump_native(
        policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut reads,
        32,
        10000,
        || [1, 2, 3, 4],
        &mut audio,
    ))
    .unwrap();
    let mut job = character_service::Job::new(server.clone(), &common, 5);
    let retirement = job.retirement.clone();
    let task = job.task.take().unwrap();
    let (sender, receiver) = futures::channel::oneshot::channel();
    job.task = Some(shin_tasks::async_io::spawn(async move {
        receiver.await.unwrap();
        task.await
    }));
    let mut selection = character_service::Selection { job: Some(job) };
    let seeds: Seeds = Arc::new(Mutex::new(Box::new(|| [1, 2, 3, 4])));
    let mut overlay = None;
    let mut copies = 0;
    let mut clock = || {
        copies += 1;
        Ok(SaveDate::now())
    };
    for _ in 0..3 {
        assert!(
            !selection
                .poll(
                    &mut runner,
                    &mut bindings,
                    &mut saved,
                    &mut overlay,
                    &common,
                    &server,
                    &seeds,
                    &mut clock
                )
                .unwrap()
        );
        assert!(overlay.is_none());
        assert!(!bindings.autosave.valid);
        assert_eq!(bindings.progress.responses().1, 0);
        assert!(selection.job.as_ref().unwrap().ui.is_none());
    }
    assert_eq!(copies, 0);
    let fork = bindings
        .memory
        .fork_for_restore(0, 0, &bindings.progress)
        .unwrap();
    let mut replay = ReplaySession::new(common.scenario.clone(), fork).unwrap();
    shin_tasks::block_on(runner.reconstruct_replay(&mut replay, || [1; 4], &mut audio)).unwrap();
    drop(selection);
    assert!(retirement.is_finished());
    assert!(sender.send(()).is_err());
    assert_eq!(copies, 0);
    assert!(overlay.is_none());
    assert_eq!(bindings.progress.responses().1, 0);
    assert!(!bindings.autosave.valid);
    assert!(runner.character_command().is_none());
    eprintln!(
        "Character loader:one real atlas producer held across three polls;no copy/result/UI while loading;reconstruction cancels/drains pending output and retires the source reservation without publication pass"
    );
}
