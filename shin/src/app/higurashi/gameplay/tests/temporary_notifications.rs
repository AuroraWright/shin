//! Live asset/widget/Choice/job transport checks. Bounded native SimpleText
//! metrics do not provide original glyph pixels; native component comparers
//! independently cover wrapper/command state and ordering.
use super::{character::common, selection::Frames, *};
use crate::{
    adv::higurashi::{
        audio::HigurashiAudio,
        autosave::{ProgressEntry, SaveProgress},
        input::OverlayInput,
        layers::Target,
        reference_gpu_assets, reference_render_to_texture,
        temporary_wrapper::{Completion, Shared, TIPS_LABELS},
        touch::Overlay as InputOverlay,
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, runtime_saves::Slots},
    layer::user::higurashi::{HigurashiLayerKind, HigurashiUserLayer},
};
use shin_core::{
    primitives::color::UnormColor, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const SOURCE_DATE: SaveDate = SaveDate {
    year: 2025,
    month: 1,
    day: 2,
    hour: 3,
    minute: 4,
    second: 5,
};
const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 12,
    hour: 18,
    minute: 21,
    second: 34,
};
const PREFIX: &[u8] = &[
    0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 0, 63,
];
const TAIL: &[u8] = &[0x82, 9, 23, 0x83, 0, 0x83, 0xe8, 0, 0, 0];
fn script(command: &[u8]) -> Vec<u8> {
    [PREFIX, command, TAIL].concat()
}
struct Fixture {
    game: Gameplay,
    frames: Frames,
    os: Arc<AtomicU32>,
    dates: Arc<AtomicUsize>,
    manual: RuntimeGameData,
}
fn fixture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    server: &Arc<AssetServer>,
    code: &[u8],
    size: (u32, u32),
    active: usize,
    saved_position: u32,
    flags: u32,
) -> Fixture {
    let (authored, common) = common(server, code);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut saved = SaveData::default();
    saved.settings.common.v14_showroutenavi = false;
    saved.settings.common.v3_sysvol = 37;
    saved.save_vectors.extra_words_0 = vec![0; 256];
    saved.save_vectors.extra_words_0[0] = 1 << 5; // Tip2's completion stays distinct from its acquired bit.
    let current = RuntimeGameData::from_progress(
        SaveProgress::new(
            active,
            0xfedc_ab98,
            std::array::from_fn(|index| {
                if index == active {
                    ProgressEntry {
                        scenario_id: 0,
                        random_seed: 42,
                        save_position: saved_position,
                        replay_target: saved_position,
                        response_data: vec![0x93, 0x4f],
                    }
                } else {
                    ProgressEntry {
                        scenario_id: 12345,
                        random_seed: 0xdead_beef,
                        save_position: 53,
                        replay_target: 54,
                        response_data: vec![0xfe, 0x12, 0x87],
                    }
                }
            }),
        )
        .unwrap(),
        SOURCE_DATE,
        vec![0xffff, 0x102, 0x7f00],
        vec![1, 0x9a, 0xff],
    );
    let mut auto = current.clone();
    auto.date = SOURCE_DATE;
    auto.trailing_values = vec![7, 0xffff];
    auto.byte_variables = vec![0x53, 0xfe];
    saved.auto_save_slot = Some(auto.save_data());
    saved.manual_save_slots[2] = Some(current.save_data());
    let mut slots = Slots::loaded(&saved);
    slots.auto.data = auto;
    slots.manual[2].data = current.clone();
    let globals = Globals {
        storage: None,
        runtime_saves: Some(slots),
        common,
        data: saved,
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    };
    let os = Arc::new(AtomicU32::new(0));
    let count = os.clone();
    let mut game = None;
    reference_render_to_texture(device, queue, size, |context, _| {
        let mut snapshot = context.new_render_texture("Notification initial".into());
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Notification initial",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        }
        game = Some(
            Gameplay::new(globals, current.clone(), flags, snapshot, move || {
                std::array::from_fn(|_| 0xabc0_0000 | (count.fetch_add(1, Ordering::SeqCst) + 1))
            })
            .unwrap(),
        );
    });
    let mut game = game.unwrap();
    let dates = Arc::new(AtomicUsize::new(0));
    let calls = dates.clone();
    game.set_clock_source(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(NOW)
    });
    Fixture {
        game,
        frames: Frames { number: 0 },
        os,
        dates,
        manual: current,
    }
}
fn ui(game: &Gameplay) -> Option<(Target, Shared)> {
    let job = game
        .tips_get
        .job
        .as_ref()
        .or(game.notification_get.job.as_ref())?;
    job.ui.as_ref().map(|ui| (ui.target, ui.wrapper.clone()))
}
fn widget_words(game: &Gameplay, target: Target) -> Vec<u32> {
    game.host()
        .unwrap()
        .runner
        .frame()
        .scene
        .layers()
        .layer(target)
        .unwrap()
        .temporary_owner()
        .unwrap()
        .reference_native_words()
}
fn phase(game: &Gameplay, target: Target) -> u32 {
    widget_words(game, target)[0]
}
fn until(f: &mut Fixture, mut ready: impl FnMut(&Gameplay) -> bool, description: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(&f.game) {
        f.frames.idle(&mut f.game);
        assert!(Instant::now() < deadline, "{description}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn barrier(f: &mut Fixture) -> (u32, usize) {
    until(
        f,
        |game| {
            game.host().is_some_and(|host| {
                !host.has_modes() && matches!(host.runner.pending_command(), Some(Cmd::WAIT { .. }))
            })
        },
        "Notification script did not reach live WAIT barrier",
    );
    assert!(ui(&f.game).is_none());
    (f.os.load(Ordering::SeqCst), f.dates.load(Ordering::SeqCst))
}
fn render(
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
            "Live notification frame",
        );
        game.render(&mut pass).unwrap();
    })
}
fn visible(rgba: &[u8]) {
    let first = &rgba[..4];
    assert!(
        rgba.chunks_exact(4).filter(|pixel| *pixel != first).count() > 100,
        "Real notification/Choice resources produced no visible composition"
    );
}
fn confirm(f: &mut Fixture) {
    f.frames.step(
        &mut f.game,
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
        &[],
    );
}

#[test]
#[ignore = "real TIPS widget/Choice/font/callback/fullscene data at720p/1080p, offscreen Metal and exact Loopback Audio; no windows/saves"]
fn higurashi_gameplay_tips_notification_preserves_source_and_delivers_original_choice() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let code = script(&[0xd0, 3, 2, 2, 3]);
    for (active, size, result) in [
        (0, (1280, 720), 0),
        (1, (1280, 720), 0),
        (0, (1920, 1080), 0),
        (1, (1920, 1080), 0),
        (0, (1280, 720), 1),
        (1, (1280, 720), 1),
        (0, (1920, 1080), 1),
        (1, (1920, 1080), 1),
    ] {
        let mt = crate::engine_random::seed_call_count();
        let mut f = fixture(&device, &queue, &server, &code, size, active, 0, 0);
        let (base_os, base_dates) = barrier(&mut f);
        until(
            &mut f,
            |game| ui(game).is_some(),
            "TIPS notification was not published",
        );
        let (target, wrapper) = ui(&f.game).unwrap();
        let owned = f.game.tips_get.job.as_ref().unwrap().reference_retirement();
        assert_eq!(f.os.load(Ordering::SeqCst), base_os + 8);
        assert_eq!(f.dates.load(Ordering::SeqCst), base_dates + 1);
        let layer = f
            .game
            .host()
            .unwrap()
            .runner
            .frame()
            .scene
            .layers()
            .layer(target)
            .unwrap();
        assert_eq!(layer.layer_type(), HigurashiLayerKind::Tips);
        assert_eq!(layer.properties.values()[5], 1.);
        assert_eq!(layer.properties.values()[22], 1.);
        assert_eq!(
            layer.properties.oscillators.map(|o| o.seed),
            std::array::from_fn(|i| 0xabc0_0000 | (base_os + i as u32 + 1))
        );
        let widget = layer.temporary_owner().unwrap();
        assert_eq!(wrapper.widget_identity(), widget.widget_identity());
        assert_ne!(wrapper.identity(), wrapper.widget_identity());
        assert_eq!(widget.result(), None);
        assert_eq!(
            f.game.host().unwrap().reference_touch_overlay_identity(),
            Some(wrapper.identity())
        );
        assert_eq!(phase(&f.game, target), 1);
        assert_eq!(f.game.reference_overlay_cues, ["suzu"]);
        assert!(wrapper.reference_child().is_none());
        let bits = f.game.globals.data.save_vectors.extra_words_0[0];
        assert_eq!(
            bits & ((1 << 4) | (1 << 5) | (1 << 6)),
            (1 << 4) | (1 << 5) | (1 << 6)
        );
        let words = widget_words(&f.game, target);
        assert_eq!(
            (words[9], words[14], words[19]),
            (0, 1, 0),
            "Duplicate tip IDs sample acquired state in native order"
        );
        until(
            &mut f,
            |game| phase(game, target) == 2,
            "TIPS notification opening did not finish",
        );
        visible(&render(&device, &queue, size, &mut f.game));
        confirm(&mut f);
        assert_eq!(phase(&f.game, target), 4);
        let child = wrapper
            .reference_child()
            .expect("TIPS direct child was not created in the current post-input phase4 poll");
        assert_eq!((child.0, child.1, child.2, child.3), (1, 0, 1., false));
        assert_eq!(child.4[5], 1000.);
        assert_eq!(
            child.5,
            std::array::from_fn(|i| 0xabc0_0000 | (base_os + 9 + i as u32))
        );
        assert_eq!(f.os.load(Ordering::SeqCst), base_os + 12);
        assert_eq!(
            child.6.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
            TIPS_LABELS
        );
        assert!(
            child.6[0].4 > -300. && child.6[0].4 < 0. && child.6[0].5 > 0.,
            "New real child did not advance in its creation frame"
        );
        assert_eq!(
            f.game.reference_overlay_cues,
            ["suzu", "decide"],
            "Silent direct Choice emits no select cue"
        );
        until(
            &mut f,
            |_| wrapper.reference_child().is_some_and(|child| child.0 == 2),
            "TIPS direct child opening did not finish",
        );
        visible(&render(&device, &queue, size, &mut f.game));
        f.game.bindings.progress.restore_response_cursor(3).unwrap();
        let source = f.game.runtime_game_data().unwrap();
        let cursor = f.game.bindings.progress.responses().1;
        let choices = f.game.globals.data.save_vectors.select_last_choices.clone();
        let seen = f.game.globals.data.save_vectors.select_seen_choices.clone();
        if result == 1 {
            f.frames.step(
                &mut f.game,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
                &[],
            );
            assert_eq!(wrapper.reference_child().unwrap().1, 1);
        }
        confirm(&mut f);
        until(
            &mut f,
            |_| wrapper.completion().unwrap().is_some(),
            "TIPS outer callback did not arrive",
        );
        assert_eq!(
            wrapper.completion().unwrap(),
            Some(Completion::Tips(result))
        );
        assert_eq!(
            ui(&f.game).unwrap().1.identity(),
            wrapper.identity(),
            "Job must read callback next frame"
        );
        f.frames.idle(&mut f.game);
        if result == 0 {
            assert!(f.game.tips_get.job.is_none());
            assert!(owned.is_finished());
            assert!(f.game.take_tips_scene_request().is_none());
            assert_eq!(
                f.game.host().unwrap().reference_touch_overlay_identity(),
                None
            );
            assert!(
                f.game
                    .host()
                    .unwrap()
                    .runner
                    .frame()
                    .scene
                    .layers()
                    .state
                    .target(target.logical)
                    .is_none()
            );
            assert_eq!(f.game.bindings.progress.pending_persistent(), [(9, 23)]);
            assert_eq!(f.dates.load(Ordering::SeqCst), base_dates + 2);
            assert_eq!(
                f.game
                    .globals
                    .data
                    .auto_save_slot
                    .as_ref()
                    .unwrap()
                    .entry
                    .save_position,
                3
            );
            assert!(wrapper.reference_child().is_none());
        } else {
            let request = f
                .game
                .take_tips_scene_request()
                .expect("TIPS outer1 did not queue its real scene request");
            let mut copy = source.clone();
            copy.progress.entries[active].save_position = 2;
            copy.progress.entries[active].replay_target = 2;
            copy.trailing_values = vec![2, 2, 3];
            assert_eq!(request.data, Some(copy));
            assert_eq!((request.selected, request.alternate), (2, 0));
            assert_eq!(f.game.runtime_game_data().unwrap(), source);
            assert_eq!(f.game.bindings.progress.responses().1, cursor);
            assert!(f.game.bindings.progress.pending_persistent().is_empty());
            assert_eq!(f.dates.load(Ordering::SeqCst), base_dates + 1);
            assert_eq!(phase(&f.game, target), 4);
            assert!(wrapper.reference_child().is_some());
            assert!(f.game.tips_get.job.is_some());
            visible(&render(&device, &queue, size, &mut f.game));
        }
        assert_eq!(
            f.game.globals.data.save_vectors.select_last_choices,
            choices
        );
        assert_eq!(f.game.globals.data.save_vectors.select_seen_choices, seen);
        assert_eq!(f.game.bindings.progress.responses().1, cursor);
        assert_eq!(f.game.bindings.date, SOURCE_DATE);
        assert_eq!(
            f.game.globals.runtime_saves.as_ref().unwrap().manual[2].data,
            f.manual
        );
        assert_eq!(
            f.game
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .auto
                .data
                .trailing_values,
            [7, 0xffff]
        );
        assert_eq!(
            f.game
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .auto
                .data
                .byte_variables,
            [0x53, 0xfe]
        );
        assert_eq!(f.os.load(Ordering::SeqCst), base_os + 12);
        assert_eq!(crate::engine_random::seed_call_count(), mt);
        assert!(wrapper.native_copy().is_none());
        drop(f);
        assert!(owned.is_finished());
    }
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
#[ignore = "real Otsu/Kakera/skip VOID notification jobs and identity retirement at720p/1080p, offscreen Metal/exact Loopback Audio;no windows/saves"]
fn higurashi_gameplay_void_notifications_preserve_data_and_retire_by_identity() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    for (kind, command, size, replace) in [
        (13, vec![0xd2, 27], (1280, 720), false),
        (13, vec![0xd2, 27], (1920, 1080), true),
        (14, vec![0xd6, 5, 3, 0, 2, 6], (1280, 720), false),
        (14, vec![0xd6, 5, 3, 0, 2, 6], (1920, 1080), false),
        (14, vec![0xd6, 0, 3, 0, 2, 6], (1280, 720), false),
        (14, vec![0xd6, 0, 3, 0, 2, 6], (1920, 1080), false),
    ] {
        let mt = crate::engine_random::seed_call_count();
        let mut f = fixture(&device, &queue, &server, &script(&command), size, 1, 0, 0);
        let (base_os, base_dates) = barrier(&mut f);
        let source = f.game.runtime_game_data().unwrap();
        let auto = f
            .game
            .globals
            .runtime_saves
            .as_ref()
            .unwrap()
            .auto
            .data
            .clone();
        let skipped = command[1] == 0;
        if skipped {
            until(
                &mut f,
                |game| !game.bindings.progress.pending_persistent().is_empty(),
                "Skipped Kakera did not resume script",
            );
            assert!(f.game.notification_get.job.is_none());
            assert!(ui(&f.game).is_none());
            assert_eq!(f.os.load(Ordering::SeqCst), base_os);
            assert!(f.game.reference_overlay_cues.is_empty());
        } else {
            until(
                &mut f,
                |game| ui(game).is_some(),
                "VOID notification was not published",
            );
            let (target, wrapper) = ui(&f.game).unwrap();
            let owned = f
                .game
                .notification_get
                .job
                .as_ref()
                .unwrap()
                .reference_retirement();
            assert_eq!(f.os.load(Ordering::SeqCst), base_os + 8);
            assert_eq!(phase(&f.game, target), 1);
            let layer = f
                .game
                .host()
                .unwrap()
                .runner
                .frame()
                .scene
                .layers()
                .layer(target)
                .unwrap();
            assert_eq!(layer.temporary_owner().unwrap().native_id(), kind);
            assert_eq!(
                layer.temporary_owner().unwrap().widget_identity(),
                wrapper.widget_identity()
            );
            assert_ne!(wrapper.identity(), wrapper.widget_identity());
            assert_eq!(
                f.game.host().unwrap().reference_touch_overlay_identity(),
                Some(wrapper.identity())
            );
            assert!(wrapper.reference_child().is_none());
            until(
                &mut f,
                |game| phase(game, target) == 2,
                "VOID notification opening did not finish",
            );
            visible(&render(&device, &queue, size, &mut f.game));
            f.frames.step(
                &mut f.game,
                Buttons {
                    pressed: 1 << 25,
                    ..Default::default()
                },
                &[],
            );
            assert_eq!(phase(&f.game, target), 2);
            assert_eq!(f.game.reference_overlay_cues, ["suzu"]);
            confirm(&mut f);
            until(
                &mut f,
                |_| wrapper.completion().unwrap().is_some(),
                "VOID notification callback did not arrive",
            );
            assert_eq!(
                wrapper.completion().unwrap(),
                Some(Completion::Notification)
            );
            if replace {
                f.game
                    .host_mut()
                    .unwrap()
                    .runner
                    .scene_layers_mut()
                    .unwrap()
                    .publish_temporary_front(target, HigurashiUserLayer::null([99; 4]))
                    .unwrap();
                f.game
                    .host_mut()
                    .unwrap()
                    .set_touch_overlay(Some(Box::new(Replacement)));
            }
            f.frames.idle(&mut f.game);
            assert!(f.game.notification_get.job.is_none());
            assert!(owned.is_finished());
            assert!(
                f.game
                    .host()
                    .unwrap()
                    .runner
                    .frame()
                    .scene
                    .layers()
                    .state
                    .target(target.logical)
                    .is_none()
            );
            assert_eq!(
                f.game.host().unwrap().reference_touch_overlay_identity(),
                if replace { Some(0x123456) } else { None }
            );
            if replace {
                assert_eq!(
                    f.game
                        .host()
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
            }
            assert_eq!(f.game.reference_overlay_cues, ["suzu", "decide"]);
            assert_eq!(f.os.load(Ordering::SeqCst), base_os + 8);
        }
        assert_eq!(f.game.runtime_game_data().unwrap(), source);
        assert_eq!(
            f.game.globals.runtime_saves.as_ref().unwrap().auto.data,
            auto
        );
        assert_eq!(
            f.game.globals.runtime_saves.as_ref().unwrap().manual[2].data,
            f.manual
        );
        assert_eq!(f.dates.load(Ordering::SeqCst), base_dates);
        assert_eq!(f.game.bindings.progress.pending_persistent(), [(9, 23)]);
        assert_eq!(crate::engine_random::seed_call_count(), mt);
    }
}

#[test]
#[ignore = "actual First/replay retained TIPS stop0/full constructorflags and real nested Choice at720p/1080p;exact Loopback Audio/offscreen Metal"]
fn higurashi_gameplay_replayed_tips_stop_zero_retains_constructor_flags() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let code = script(&[0xd0, 3, 2, 2, 3]);
    for (flags, size) in [(0, (1280, 720)), (0x100, (1920, 1080))] {
        let mt = crate::engine_random::seed_call_count();
        let mut f = fixture(&device, &queue, &server, &code, size, 0, 2, flags);
        until(
            &mut f,
            |game| ui(game).is_some(),
            "Retained stop0 TIPS command did not publish its live UI",
        );
        let (target, wrapper) = ui(&f.game).unwrap();
        let owner = f.game.host().unwrap().runner.tips_get_command().unwrap();
        assert_eq!(owner.replay_stop(), 0);
        assert_eq!(owner.alternate(), flags != 0);
        assert_eq!(widget_words(&f.game, target)[1], u32::from(flags != 0));
        let os = f.os.load(Ordering::SeqCst);
        if flags == 0 {
            until(
                &mut f,
                |game| phase(game, target) == 2,
                "Replayed normal TIPS opening did not finish",
            );
            confirm(&mut f);
        }
        until(
            &mut f,
            |_| wrapper.reference_child().is_some(),
            "Replayed TIPS direct Choice was not created",
        );
        let child = wrapper.reference_child().unwrap();
        assert_eq!((child.0, child.1), (1, 0));
        assert_eq!(child.4[5], 1000.);
        assert!(child.6[0].4 > -300. && child.6[0].5 > 0.);
        assert_eq!(f.os.load(Ordering::SeqCst), os + 4);
        assert_eq!(crate::engine_random::seed_call_count(), mt);
        assert_eq!(
            f.game.reference_overlay_cues,
            if flags == 0 {
                vec!["suzu", "decide"]
            } else {
                vec!["suzu"]
            }
        );
        visible(&render(&device, &queue, size, &mut f.game));
        assert_eq!(f.game.bindings.date, SOURCE_DATE);
    }
}
