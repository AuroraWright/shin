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
        title_demo::TitleRequest,
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

fn load_globals(
    server: &Arc<AssetServer>,
    common: Arc<crate::app::higurashi::first_boot::CommonAssets>,
) -> Globals {
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut data = SaveData::default();
    data.persist_data.0 = vec![-1; 192];
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
            let mut frame = context.new_render_texture("Picker composed source".into());
            for output in [
                frame.as_texture_target(),
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    context.begin_pass(output, Some(context.depth_stencil), "Picker scene frame");
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
    fn ready_picker(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut TitleSlot) {
        let end = Instant::now() + Duration::from_secs(10);
        while !slot
            .scenario_select
            .as_ref()
            .unwrap()
            .reference_owner()
            .reference_state()
            .2
        {
            self.step(device, queue, slot, 3.0, 0);
            assert!(Instant::now() < end, "Picker did not open");
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
            "Picker source copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}

fn seed_source() -> (Arc<dyn Fn() -> u32 + Send + Sync>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    (
        Arc::new(move || 0x1234_0000 + counter.fetch_add(1, Ordering::SeqCst) as u32),
        calls,
    )
}

fn dispatch_picker(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frames: &Frames,
    slot: &mut TitleSlot,
    seed: Arc<dyn Fn() -> u32 + Send + Sync>,
) -> Result<bool> {
    let mut result = None;
    reference_render_to_texture(device, queue, frames.size, |context, _| {
        result = Some(slot.dispatch_scenario_select_with_seed(
            context,
            frames.composed.as_ref().unwrap(),
            (frames.size.0 as u16, frames.size.1 as u16),
            seed,
        ));
    });
    result.unwrap()
}

fn wait_pending(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frames: &mut Frames,
    slot: &mut TitleSlot,
    predicate: impl Fn(&TitleSlot) -> bool,
) -> Vec<u8> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let image = frames.step(device, queue, slot, 3., 0);
        if predicate(slot) {
            return image;
        }
        assert!(Instant::now() < end, "Native scene request did not arrive");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "actual SNRSEL/next-frame picker/factory retries/full runtime ADV or title returns at720p/1080p;offscreen Metal/exact Loopback,no saves/window"]
fn higurashi_scenario_select_slot_owns_six_factories_and_native_returns() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let mut factories = 0;
    let mut returns = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for kind in 0..6u8 {
            let (_, common) = crate::app::higurashi::gameplay::tests::character::common(
                &server,
                &[
                    0x82, 7, 1, 0xd4, kind, 0x82, 9, 2, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
                ],
            );
            let globals = load_globals(&server, common);
            let manager = globals.manager.clone();
            let mut data = RuntimeGameData::from(fresh());
            data.date = SaveDate {
                year: 2025,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            };
            data.progress.entries[1] = data.progress.entries[0].clone();
            data.progress.entries[0].scenario_id = 1000;
            data.progress.entries[0].random_seed = 17;
            data.progress.entries[0].save_position = 77;
            data.progress.entries[0].replay_target = 88;
            data.progress.entries[0].response_data = vec![0xaa, 0xf1];
            data.progress.entries[1].random_seed = 42;
            data.progress.entries[1].response_data = vec![0x23, 0xee];
            data.progress.set_active_index(1).unwrap();
            data.progress.extra_word = 0xdead_beef;
            data.trailing_values = vec![0, 0xffff, 0x1234];
            data.byte_variables = vec![2, 255, 37];
            let mut game = None;
            reference_render_to_texture(&device, &queue, size, |context, _| {
                let mut snapshot = context.new_render_texture("SNRSEL source".into());
                let mut pass = context.begin_pass(
                    snapshot.as_texture_target(),
                    Some(context.depth_stencil),
                    "SNRSEL source",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                drop(pass);
                game = Some(
                    Gameplay::new(globals, data.clone(), 0, snapshot, || [1, 2, 3, 4]).unwrap(),
                );
            });
            let mut game = game.unwrap();
            game.set_clock_source(|| {
                Ok(SaveDate {
                    year: 2026,
                    month: 10,
                    day: 14,
                    hour: 12,
                    minute: 13,
                    second: 14,
                })
            });
            let mut slot = TitleSlot::from_game(game);
            let mut frames = Frames {
                number: 0,
                size,
                composed: None,
            };
            frames.step(&device, &queue, &mut slot, 0., 0);
            let before = wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
                s.pending_scenario_select()
            });
            let expected = match &slot.pending {
                Some(Pending::ScenarioSelect(r)) => {
                    assert_eq!(r.kind, i32::from(kind));
                    r.data.clone()
                }
                _ => panic!("Missing real SNRSEL request"),
            };
            assert_eq!(expected.date, data.date);
            assert_eq!(expected.progress.active_index(), 1);
            assert_eq!(expected.progress.extra_word, 0xdead_beef);
            assert_eq!(expected.progress.entries[0], data.progress.entries[0]);
            assert_eq!(expected.progress.entries[1].response_data, [0x23, 0xee]);
            assert_eq!(expected.trailing_values, data.trailing_values);
            assert_eq!(expected.byte_variables, data.byte_variables);
            assert!(slot.game.is_some() && slot.scenario_select.is_none());
            let audio = &mut slot.game.as_mut().unwrap().host_mut().unwrap().audio;
            audio.browser_music_multiplier(0.5);
            audio.update_global_music(Ticks::from_f32(30.));
            assert_eq!(audio.reference_music_focus(), 0.5);
            let (seed, calls) = seed_source();
            if kind == 0 {
                let Some(Pending::ScenarioSelect(request)) = &mut slot.pending else {
                    unreachable!()
                };
                request.kind = -1;
                assert!(
                    dispatch_picker(&device, &queue, &frames, &mut slot, seed.clone()).is_err()
                );
                assert!(slot.game.is_some() && slot.scenario_select.is_none());
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                let Some(Pending::ScenarioSelect(request)) = &mut slot.pending else {
                    unreachable!()
                };
                request.kind = 0;
            }
            slot.reference_scene_events.clear();
            slot.reference_fail_next_constructor();
            assert!(dispatch_picker(&device, &queue, &frames, &mut slot, seed.clone()).is_err());
            assert!(slot.game.is_none() && slot.scenario_select.is_none());
            assert_eq!(slot.active_scene().unwrap(), ActiveScene::RetiredFactory);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let Some(Pending::PreparedScenarioSelect(transfer)) = &slot.pending else {
                unreachable!()
            };
            assert_eq!(transfer.prepared.data, expected);
            assert!(transfer.snapshot.is_some());
            assert!(Arc::ptr_eq(
                &transfer.globals.as_ref().unwrap().manager,
                &manager
            ));
            // A retry must keep the original seed owner and entire factory data.
            assert!(
                dispatch_picker(
                    &device,
                    &queue,
                    &frames,
                    &mut slot,
                    Arc::new(|| panic!("Retry replaced seed owner"))
                )
                .unwrap()
            );
            assert_eq!(slot.reference_scene_events, [1, 2]);
            assert_eq!(slot.active_scene().unwrap(), ActiveScene::ScenarioSelect);
            let picker = slot.scenario_select.as_ref().unwrap();
            assert_eq!(picker.reference_owner().reference_model().data, expected);
            assert_eq!(picker.reference_owner().reference_model().selected, 0);
            assert_eq!(
                picker.reference_owner().reference_model().kind,
                u32::from(kind)
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let image = frames.step(&device, &queue, &mut slot, 0., 0);
            assert_eq!(
                slot.scenario_select
                    .as_ref()
                    .unwrap()
                    .globals
                    .audio
                    .reference_music_focus(),
                0.5
            );
            assert_eq!(
                image, before,
                "Whole composed source capture kind{kind}/{size:?}"
            );
            factories += 1;
            if ![0, 4, 5].contains(&kind) {
                continue;
            }
            frames.ready_picker(&device, &queue, &mut slot);
            assert_eq!(
                slot.scenario_select
                    .as_ref()
                    .unwrap()
                    .globals
                    .audio
                    .reference_music_focus(),
                0.5
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            if kind == 0 {
                frames.step(&device, &queue, &mut slot, 0., 1 << 25);
                wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
                    s.scenario_select
                        .as_ref()
                        .unwrap()
                        .reference_owner()
                        .reference_question()
                        == Some((2, 1))
                });
                frames.buttons(
                    &device,
                    &queue,
                    &mut slot,
                    0.,
                    Buttons {
                        repeat: 1 << 4,
                        ..Default::default()
                    },
                );
                frames.step(&device, &queue, &mut slot, 0., 1 << 24);
                wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
                    matches!(s.pending, Some(Pending::Gameplay(_)))
                });
            } else if kind == 4 {
                frames.step(&device, &queue, &mut slot, 0., 1 << 24);
                wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
                    matches!(s.pending, Some(Pending::Gameplay(_)))
                });
            } else {
                frames.step(&device, &queue, &mut slot, 0., 1 << 25);
                wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
                    matches!(s.pending, Some(Pending::PickerTitle(_)))
                });
            }
            assert!(slot.scenario_select.is_some());
            if kind == 5 {
                let Some(Pending::PickerTitle(request)) = &slot.pending else {
                    unreachable!()
                };
                assert_eq!((request.selected, request.mode), (8, u32::MAX));
                reference_render_to_texture(&device, &queue, size, |ctx, _| {
                    assert!(
                        slot.dispatch_picker_title(ctx, frames.composed.as_ref().unwrap())
                            .unwrap()
                    );
                });
                assert_eq!(slot.active_scene().unwrap(), ActiveScene::Title);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            } else {
                let Some(Pending::Gameplay(transfer)) = &slot.pending else {
                    unreachable!()
                };
                assert_eq!(transfer.flags, 0);
                let target = transfer.data.clone();
                if kind == 0 {
                    assert_eq!(target, expected);
                    assert_eq!(calls.load(Ordering::SeqCst), 1);
                } else {
                    let mut chosen = expected.clone();
                    chosen.progress.set_active_index(0).unwrap();
                    let entry = &mut chosen.progress.entries[0];
                    entry.scenario_id = 500;
                    entry.random_seed = 0x1234_0002;
                    entry.save_position = 0;
                    entry.replay_target = 0;
                    assert_eq!(target, chosen);
                    assert_eq!(calls.load(Ordering::SeqCst), 3);
                }
                reference_render_to_texture(&device, &queue, size, |ctx, _| {
                    assert!(
                        slot.dispatch_kakera_return(ctx, frames.composed.as_ref().unwrap())
                            .unwrap()
                    );
                });
                let game = slot.game.as_ref().unwrap();
                assert_eq!(game.flags, 0);
                assert_eq!(game.bindings.progress.progress(), &target.progress);
                assert_eq!(
                    game.bindings.progress.trailing_values().unwrap(),
                    target.trailing_values
                );
                assert_eq!(game.bindings.progress.bytes(), target.byte_variables);
                assert_eq!(game.bindings.date, target.date);
                assert!(Arc::ptr_eq(&game.globals.manager, &manager));
                if kind == 0 {
                    frames.wait_game(&device, &queue, &mut slot);
                    assert!(slot.scenario_select.is_none());
                    assert_eq!(
                        slot.game
                            .as_ref()
                            .unwrap()
                            .bindings
                            .progress
                            .pending_persistent(),
                        [(9, 2)]
                    );
                    assert!(!slot.pending_scenario_select());
                }
            }
            returns += 1;
        }
    }
    assert_eq!((factories, returns), (12, 6));
    eprintln!(
        "ScenarioSelect TitleSlot:12 actual six-kind SNRSEL factories/6 native ADV-or-title returns;preflight source retention,retired-factory retry/full payload,selected0/whole composed capture,real Question/flags0/replay-stop resume and720p/1080p pass"
    );
}

#[test]
#[ignore = "actual Title Origin/Tips/Origin/title flows with real menus,one fresh constructor seed and retained retry at720p/1080p;offscreen Metal/exact Loopback,no window/saves"]
fn higurashi_origin_slot_roundtrips_title_and_alternate_tips_with_one_fresh_seed() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (_, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[0x83, 0, 0x83, 0xe8, 0, 0, 0],
    );
    for size in [(1280, 720), (1920, 1080)] {
        let globals = load_globals(&server, common.clone());
        let manager = globals.manager.clone();
        let auto = globals.runtime_saves.as_ref().unwrap().auto.data.clone();
        let mut slot = TitleSlot::new(
            TitleTransition {
                request: TitleRequest {
                    selected: 8,
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
        frames.step(&device, &queue, &mut slot, 0., 0);
        wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            s.title.as_ref().unwrap().reference_top_selection() == Some(8)
        });
        for _ in 0..25 {
            frames.step(&device, &queue, &mut slot, 3., 0);
        }
        frames.step(&device, &queue, &mut slot, 0., 1 << 24);
        let before = wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            s.pending_origin() == Some(0)
        });
        assert!(slot.title.is_some() && slot.scenario_select.is_none());
        let (seed, calls) = seed_source();
        slot.reference_scene_events.clear();
        slot.reference_fail_next_constructor();
        assert!(dispatch_picker(&device, &queue, &frames, &mut slot, seed.clone()).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let Some(Pending::PreparedScenarioSelect(transfer)) = &slot.pending else {
            unreachable!()
        };
        assert_eq!(transfer.prepared.data, origin_data(0x1234_0000));
        assert_eq!(transfer.origin_selected, Some(0));
        assert!(
            dispatch_picker(
                &device,
                &queue,
                &frames,
                &mut slot,
                Arc::new(|| panic!("Origin retry resampled"))
            )
            .unwrap()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(frames.step(&device, &queue, &mut slot, 0., 0), before);
        frames.ready_picker(&device, &queue, &mut slot);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        for _ in 0..5 {
            frames.buttons(
                &device,
                &queue,
                &mut slot,
                0.,
                Buttons {
                    repeat: 1 << 6,
                    ..Default::default()
                },
            );
        }
        assert_eq!(
            slot.scenario_select
                .as_ref()
                .unwrap()
                .reference_owner()
                .reference_model()
                .selected,
            9
        );
        frames.step(&device, &queue, &mut slot, 0., 1 << 24);
        wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            matches!(s.pending, Some(Pending::Tips(_)))
        });
        let Some(Pending::Tips(request)) = &slot.pending else {
            unreachable!()
        };
        assert!(request.data.is_none());
        assert_eq!((request.selected, request.alternate), (0, 1));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(slot.scenario_select.is_some() && slot.tips.is_none());
        frames.step(&device, &queue, &mut slot, 0., 0);
        let end = Instant::now() + Duration::from_secs(10);
        while slot.tips.as_ref().unwrap().reference_first_pending() {
            frames.step(&device, &queue, &mut slot, 3., 0);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..35 {
            frames.step(&device, &queue, &mut slot, 3., 0);
        }
        frames.step(&device, &queue, &mut slot, 0., 1 << 25);
        wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            s.pending_origin() == Some(9)
        });
        assert!(slot.tips.is_some());
        assert!(dispatch_picker(&device, &queue, &frames, &mut slot, seed.clone()).unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        let picker = slot.scenario_select.as_ref().unwrap();
        assert_eq!(
            picker.reference_owner().reference_model().data,
            origin_data(0x1234_0003)
        );
        assert_eq!(picker.reference_owner().reference_model().selected, 9);
        assert_eq!(
            picker.globals.runtime_saves.as_ref().unwrap().auto.data,
            auto
        );
        assert!(Arc::ptr_eq(&picker.globals.manager, &manager));
        frames.ready_picker(&device, &queue, &mut slot);
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        frames.step(&device, &queue, &mut slot, 0., 1 << 25);
        wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            matches!(s.pending, Some(Pending::PickerTitle(_)))
        });
        frames.step(&device, &queue, &mut slot, 0., 0);
        wait_pending(&device, &queue, &mut frames, &mut slot, |s| {
            s.title.as_ref().unwrap().reference_top_selection() == Some(8)
        });
        assert_eq!(calls.load(Ordering::SeqCst), 5);
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
            auto
        );
        assert!(!slot.has_save_task());
    }
    eprintln!(
        "Origin TitleSlot:actual Top8→Origin0→alternate Tips0→Origin9→Top8 at720p/1080p;one fresh seed after source retirement and retained retry,explicit-zero unwritten fields,full autosave/global owners and exact return payloads pass"
    );
}
