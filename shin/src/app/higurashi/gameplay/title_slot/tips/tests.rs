use super::*;
use crate::{
    adv::{
        higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
        play_clock::PlayClock,
    },
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
    },
    audio::SystemSoundPlayer,
};
use shin_core::{
    format::save::higurashi::SaveDate, primitives::color::UnormColor,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

mod menu;
mod save;

fn load_globals(
    server: &Arc<AssetServer>,
    common: Arc<crate::app::higurashi::first_boot::CommonAssets>,
) -> Globals {
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut data = SaveData::default();
    data.save_vectors.seen_messages = vec![0xa5; 4];
    data.save_vectors.select_seen_choices = vec![0x1234; 3];
    data.save_vectors.select_last_choices = vec![1, 2, 3, 4];
    data.save_vectors.extra_words_0 = vec![u32::MAX; 20];
    let mut slots = Slots::loaded(&data);
    slots.auto.valid = true;
    slots.auto.data = RuntimeGameData::from(fresh());
    slots.auto.data.progress.extra_word = 0xdeadbeef;
    slots.auto.data.progress.entries[1].response_data = vec![0x55, 0xf1];
    slots.manual[2].valid = true;
    slots.manual[2].data = slots.auto.data.clone();
    slots.manual[2].data.progress.set_active_index(1).unwrap();
    slots.manual[2].data.trailing_values = vec![0xffff, 0x1234];
    Globals {
        common: common.clone(),
        data,
        storage: None,
        runtime_saves: Some(slots),
        clock: PlayClock::from_saved_seconds(17),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    }
}
struct Frames {
    number: u32,
    size: (u32, u32),
    composed: Option<RenderTexture>,
}
impl Frames {
    fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        pressed: u32,
    ) -> Vec<u8> {
        self.buttons(
            device,
            queue,
            slot,
            elapsed,
            Buttons {
                pressed,
                ..Buttons::default()
            },
        )
    }
    fn buttons(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
        buttons: Buttons,
    ) -> Vec<u8> {
        self.number += 1;
        let mut composed = None;
        let image = reference_render_to_texture(device, queue, self.size, |context, target| {
            if let Some(previous) = &self.composed {
                shin_tasks::block_on(slot.update_frame(
                    context,
                    previous,
                    (self.size.0 as u16, self.size.1 as u16),
                    Ticks::from_f32(elapsed),
                    true,
                    buttons,
                    &PointerFrame {
                        number: self.number,
                        time_seconds: self.number as f64 / 60.0,
                        events: &[],
                    },
                    policy(),
                    audio(),
                ))
                .unwrap();
            }
            slot.prepare(context).unwrap();
            let mut frame = context.new_render_texture("TIPS composed source".into());
            for output in [
                frame.as_texture_target(),
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    context.begin_pass(output, Some(context.depth_stencil), "TIPS scene frame");
                pass.clear(
                    Some(UnormColor::from_rgba(31, 63, 127, 255)),
                    Some(0),
                    Some(1.0),
                );
                slot.render(&mut pass).unwrap();
            }
            composed = Some(frame);
        });
        self.composed = composed;
        image
    }
    fn ready_tips(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut TitleSlot) {
        let end = Instant::now() + Duration::from_secs(10);
        while slot.tips.as_ref().unwrap().reference_first_pending() {
            self.step(device, queue, slot, 3.0, 0);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn wait_game(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut TitleSlot) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            self.step(device, queue, slot, 3.0, 0);
            if matches!(
                slot.game
                    .as_ref()
                    .unwrap()
                    .host()
                    .unwrap()
                    .runner
                    .pending_command(),
                Some(Cmd::WAIT { amount: 1000, .. })
            ) {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn captured(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    source: &RenderTexture,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
            snapshot: source.copy_snapshot(&mut context.render_clone_ctx()),
            mask: None,
            clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
        };
        let mut pass = context.begin_pass(
            shin_render::shaders::types::texture::TextureTarget {
                view: target,
                kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "TIPS source copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}

#[test]
#[ignore = "actual title/TIPS/ADV/EXIT/TIPS/title next-frame ownership,full histories/autosave/records/clock and real resources/input/audio at720p/1080p;temporary-free,no window/saves"]
fn higurashi_tips_scene_roundtrips_preserve_full_runtime_and_global_owners() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (_authored, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[0x83, 1, 0x83, 0xe8, 0, 0, 0, 0x83, 0, 1],
    );
    let mut transfers = 0;
    for size in [(1280, 720), (1920, 1080)] {
        // Real title selection6 closes its existing TopMenu and queues dcf00.
        let globals = load_globals(&server, common.clone());
        let manager = globals.manager.clone();
        let full_auto = globals.runtime_saves.as_ref().unwrap().auto.data.clone();
        let mut slot = TitleSlot::new(
            TitleTransition {
                request: TitleRequest {
                    selected: 6,
                    mode: u32::MAX,
                },
                globals,
            },
            None,
        );
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        let end = Instant::now() + Duration::from_secs(10);
        while slot.title.as_mut().unwrap().take_menu_request().is_none() {
            frames.step(&device, &queue, &mut slot, 3.0, 0);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..25 {
            frames.step(&device, &queue, &mut slot, 3.0, 0);
        }
        frames.step(&device, &queue, &mut slot, 3.0, 1 << 24);
        let before = loop {
            let image = frames.step(&device, &queue, &mut slot, 3.0, 0);
            if slot.pending_tips() {
                break image;
            }
            assert!(Instant::now() < end);
        };
        assert!(slot.title.is_some() && slot.tips.is_none());
        let original = slot.title.as_ref().unwrap().globals.common.clone();
        let mut wrong = shin_core::format::scenario::higurashi::HigurashiScenario::new(
            original.scenario.raw().to_vec().into(),
        )
        .unwrap();
        wrong.variant = shin_core::format::scenario::higurashi::HigurashiVariant::Original;
        slot.title.as_mut().unwrap().globals.common =
            Arc::new(crate::app::higurashi::first_boot::CommonAssets {
                scenario: Arc::new(wrong),
                font: original.font.clone(),
                system: original.system.clone(),
                message: original.message.clone(),
                effects: original.effects.clone(),
                sysse: original.sysse.clone(),
            });
        reference_render_to_texture(&device, &queue, size, |context, _| {
            assert!(
                slot.dispatch_tips(
                    context,
                    frames.composed.as_ref().unwrap(),
                    (size.0 as u16, size.1 as u16)
                )
                .is_err()
            );
        });
        assert!(slot.pending_tips() && slot.title.is_some() && slot.tips.is_none());
        assert!(Arc::ptr_eq(
            &slot.title.as_ref().unwrap().globals.manager,
            &manager
        ));
        slot.title.as_mut().unwrap().globals.common = original;
        slot.reference_scene_events.clear();
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert_eq!(slot.reference_scene_events, [1, 2, 3]);
        let menu = slot.tips.as_ref().unwrap();
        assert!(Arc::ptr_eq(&menu.globals.manager, &manager));
        assert!(menu.motion.model.data.is_none());
        assert_eq!(
            captured(&device, &queue, size, menu.reference_capture().unwrap()),
            before
        );
        transfers += 1;
        frames.ready_tips(&device, &queue, &mut slot);
        frames.step(&device, &queue, &mut slot, 0.0, 1 << 25);
        let before = loop {
            let image = frames.step(&device, &queue, &mut slot, 1.0, 0);
            if slot.pending.is_some() {
                break image;
            }
        };
        slot.reference_scene_events.clear();
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert_eq!(slot.reference_scene_events, [1, 2]);
        assert!(slot.tips.is_none());
        assert_eq!(
            slot.title
                .as_ref()
                .unwrap()
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .auto
                .data,
            full_auto
        );
        let snapshot = reference_render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "TIPS-to-title capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            slot.title.as_ref().unwrap().render_transition(&mut pass);
        });
        assert_eq!(snapshot, before);
        transfers += 1;
        // A source-free browser starts ADV without a pre-existing continuation.
        slot.queue_tips(Request {
            data: None,
            selected: 0,
            alternate: 0,
        });
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        frames.ready_tips(&device, &queue, &mut slot);
        assert!(slot.continuation.is_none());
        frames.step(&device, &queue, &mut slot, 0.0, 1 << 24);
        while slot.pending.is_none() {
            frames.step(&device, &queue, &mut slot, 1.0, 0);
        }
        let fresh_request = match &slot.pending {
            Some(Pending::Gameplay(returned)) => returned.data.clone(),
            _ => panic!("missing source-free TIPS ADV request"),
        };
        assert_eq!(fresh_request.progress.active_index(), 0);
        assert!(fresh_request.progress.entries[0].response_data.is_empty());
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert!(slot.tips.is_none() && slot.game.is_some());
        assert!(slot.continuation.is_none());
        assert_eq!(
            slot.game.as_ref().unwrap().bindings.progress.progress(),
            &fresh_request.progress
        );
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .globals
                .runtime_saves
                .as_ref()
                .unwrap()
                .auto
                .data,
            full_auto
        );
        transfers += 2;
        drop(slot);
        // A live ADV carries its original owners through full-data TIPS,tip
        // playback,EXIT's active1-to0 factory and primary story restoration.
        let globals = load_globals(&server, common.clone());
        let full_manual = globals.runtime_saves.as_ref().unwrap().manual[2]
            .data
            .clone();
        let mut source = crate::app::higurashi::tips_menu::tests::source(100, 0);
        source.progress.entries[0].save_position = 0;
        source.progress.entries[0].replay_target = 0;
        let seed_calls = Arc::new(AtomicUsize::new(0));
        let counter = seed_calls.clone();
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let frame = context.new_render_texture("TIPS initial ADV".into());
            game = Some(
                Gameplay::new(globals, source.clone(), 0, frame, move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    [1, 2, 3, 4]
                })
                .unwrap(),
            );
        });
        let mut game = game.unwrap();
        game.bindings.history.record_completion(true, 73);
        let history = game.bindings.history.bits().words().as_ptr();
        let choices = game.bindings.choices.seen().words().as_ptr();
        let provider = game.seeds.clone();
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let calls = clock_calls.clone();
        game.set_clock_source(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(SaveDate {
                year: 2026,
                month: 10,
                day: 13,
                hour: 4,
                minute: 5,
                second: 6,
            })
        });
        let mut slot = TitleSlot::from_game(game);
        let mut frames = Frames {
            number: 0,
            size,
            composed: None,
        };
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        frames.wait_game(&device, &queue, &mut slot);
        let current = slot.game.as_ref().unwrap().runtime_game_data().unwrap();
        let before = frames.step(&device, &queue, &mut slot, 0.0, 0);
        slot.queue_tips(Request {
            data: Some(current.clone()),
            selected: 0,
            alternate: 0,
        });
        slot.reference_scene_events.clear();
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert_eq!(slot.reference_scene_events, [1, 2, 3]);
        assert_eq!(
            slot.tips.as_ref().unwrap().motion.model.data,
            Some(current.clone())
        );
        assert_eq!(
            captured(
                &device,
                &queue,
                size,
                slot.tips.as_ref().unwrap().reference_capture().unwrap()
            ),
            before
        );
        transfers += 1;
        assert_eq!(
            slot.continuation
                .as_ref()
                .unwrap()
                .history
                .bits()
                .words()
                .as_ptr(),
            history
        );
        assert_eq!(
            slot.continuation
                .as_ref()
                .unwrap()
                .choices
                .seen()
                .words()
                .as_ptr(),
            choices
        );
        frames.ready_tips(&device, &queue, &mut slot);
        frames.step(&device, &queue, &mut slot, 0.0, 1 << 24);
        while slot.pending.is_none() {
            frames.step(&device, &queue, &mut slot, 1.0, 0);
        }
        let requested = match &slot.pending {
            Some(Pending::Gameplay(returned)) => returned.data.clone(),
            _ => panic!("missing TIPS ADV request"),
        };
        assert_eq!(requested.progress.active_index(), 1);
        assert_eq!(requested.progress.entries[0], current.progress.entries[0]);
        assert_eq!(
            requested.progress.entries[1].response_data,
            current.progress.entries[1].response_data
        );
        slot.reference_scene_events.clear();
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        assert_eq!(slot.reference_scene_events, [1, 2, 3]);
        assert!(Arc::ptr_eq(&slot.game.as_ref().unwrap().seeds, &provider));
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .bindings
                .history
                .bits()
                .words()
                .as_ptr(),
            history
        );
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .bindings
                .choices
                .seen()
                .words()
                .as_ptr(),
            choices
        );
        transfers += 1;
        frames.wait_game(&device, &queue, &mut slot);
        assert!(
            slot.game
                .as_mut()
                .unwrap()
                .host_mut()
                .unwrap()
                .runner
                .interrupt_wait()
        );
        while !slot.pending_tips() {
            frames.step(&device, &queue, &mut slot, 3.0, 0);
        }
        assert_eq!(
            slot.game
                .as_ref()
                .unwrap()
                .bindings
                .progress
                .progress()
                .active_index(),
            0
        );
        assert!(
            slot.game
                .as_ref()
                .unwrap()
                .globals
                .data
                .save_vectors
                .extra_words_0[0]
                & 2
                != 0
        );
        let returned = match &slot.pending {
            Some(Pending::Tips(request)) => request.data.clone().unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(returned.progress.entries[0], current.progress.entries[0]);
        assert_eq!(
            returned.progress.entries[1].response_data,
            current.progress.entries[1].response_data
        );
        assert_eq!(returned.trailing_values, current.trailing_values);
        assert_eq!(returned.byte_variables, current.byte_variables);
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        frames.ready_tips(&device, &queue, &mut slot);
        assert_eq!(
            slot.tips.as_ref().unwrap().motion.model.data,
            Some(returned.clone())
        );
        transfers += 1;
        frames.step(&device, &queue, &mut slot, 0.0, 1 << 25);
        while slot.pending.is_none() {
            frames.step(&device, &queue, &mut slot, 1.0, 0);
        }
        frames.step(&device, &queue, &mut slot, 0.0, 0);
        let game = slot.game.as_ref().unwrap();
        assert_eq!(game.bindings.progress.progress(), &returned.progress);
        assert!(Arc::ptr_eq(&game.seeds, &provider));
        assert_eq!(game.bindings.history.bits().words().as_ptr(), history);
        assert_eq!(game.bindings.choices.seen().words().as_ptr(), choices);
        assert_eq!(
            game.globals.runtime_saves.as_ref().unwrap().manual[2].data,
            full_manual
        );
        assert_eq!(clock_calls.load(Ordering::SeqCst), 0);
        assert!(seed_calls.load(Ordering::SeqCst) >= 3);
        transfers += 1;
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "TIPS scene:{transfers} real720p/1080p next-frame transfers;actual title selection/source-free ADV/full-data browser/tip ADV/EXIT active1-to0/primary resume,complete records/auto-manual rows/original histories/providers,rejected preflight and source-retired-before-constructor captures pass;Origin/other menus remain"
    );
}
