use super::*;
use crate::{
    adv::higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        runtime_saves::Slots,
    },
};
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
mod menus;

fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    slot: &mut TitleSlot,
) -> (RenderTexture, Vec<u8>) {
    let mut composed = None;
    let pixels = reference_render_to_texture(device, queue, size, |context, target| {
        slot.prepare(context).unwrap();
        let mut frame = context.new_render_texture("manual Load source composed frame".into());
        for output in [
            frame.as_texture_target(),
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
        ] {
            let mut pass =
                context.begin_pass(output, Some(context.depth_stencil), "manual Load source");
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            slot.render(&mut pass).unwrap();
        }
        composed = Some(frame);
    });
    (composed.unwrap(), pixels)
}
fn copied(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    snapshot: &RenderTexture,
) -> Vec<u8> {
    reference_render_to_texture(device, queue, size, |context, target| {
        let overlay = crate::adv::higurashi::runner::reconstruction_mode::Overlay {
            clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
            snapshot: snapshot.copy_snapshot(&mut context.render_clone_ctx()),
            mask: None,
        };
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "manual Load captured frame",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}

#[test]
#[ignore = "real Title/ADV manual Load flags0,next-frame transfer,full two-record payload,global/manual/autosave/read/choice preservation,preflight retry and exact720p/1080p capture on offscreen Metal/exact Loopback Audio;no window/user saves"]
fn higurashi_manual_load_scene_transfer_preserves_full_data_and_globals() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (authored, common) = crate::app::higurashi::gameplay::tests::character::common(
        &server,
        &[0x83, 0, 0x83, 0xe8, 0, 0, 0],
    );
    let mut profiles = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for from_game in [false, true] {
            for active in 0..2 {
                let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
                let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
                let mut saved = SaveData::default();
                saved.persist_data.0 = vec![71; 32];
                saved.save_menu_position = 2;
                saved.save_vectors.seen_messages = vec![0xa5, 0x87654321];
                saved.save_vectors.select_seen_choices = vec![0xdeadbeef];
                saved.save_vectors.select_last_choices = vec![7, 255, 3];
                saved.auto_save_slot = Some(fresh());
                saved.manual_save_slots[2] = Some(fresh());
                let mut slots = Slots::loaded(&saved);
                slots.auto.data.progress.extra_word = 0xfedcba98;
                slots.auto.data.trailing_values = vec![0xffff, 0x1234];
                slots.auto.data.byte_variables = vec![9, 255, 7];
                let mut loaded = RuntimeGameData::from(fresh());
                loaded.progress.extra_word = 0x87654321;
                loaded.progress.entries[active].random_seed = 0x12345678;
                loaded.progress.entries[active].response_data = vec![0x23, 0xee, 0x88];
                loaded.progress.entries[1 - active] =
                    crate::adv::higurashi::autosave::ProgressEntry {
                        scenario_id: 2084,
                        random_seed: 0xabcdef12,
                        save_position: 12345,
                        replay_target: 34567,
                        response_data: vec![1, 2, 3, 255],
                    };
                loaded.progress.set_active_index(active).unwrap();
                loaded.date = shin_core::format::save::higurashi::SaveDate {
                    year: 2025,
                    month: 12,
                    day: 31,
                    hour: 23,
                    minute: 58,
                    second: 57,
                };
                loaded.trailing_values = vec![0xffff, 0x1234, 71];
                loaded.byte_variables = vec![254, 17, 255];
                slots.manual[2].data = loaded.clone();
                let expected_slots = slots.clone();
                let globals = Globals {
                    common: common.clone(),
                    data: saved,
                    storage: None,
                    runtime_saves: Some(slots),
                    clock: PlayClock::from_saved_seconds(17),
                    platform_flag: true,
                    last_title_mode: 7,
                    audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
                    server: server.clone(),
                    manager,
                    sounds: sounds.clone(),
                };
                let seed_calls = Arc::new(AtomicUsize::new(0));
                let clock_calls = Arc::new(AtomicUsize::new(0));
                let mut slot = if from_game {
                    let calls = seed_calls.clone();
                    let mut game = None;
                    reference_render_to_texture(&device, &queue, size, |context, _| {
                        let mut initial = context.new_render_texture("manual Load initial".into());
                        {
                            let mut pass = context.begin_pass(
                                initial.as_texture_target(),
                                Some(context.depth_stencil),
                                "manual Load initial",
                            );
                            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                        }
                        game = Some(
                            Gameplay::new(
                                globals,
                                RuntimeGameData::from(fresh()),
                                0,
                                initial,
                                move || {
                                    calls.fetch_add(1, Ordering::SeqCst);
                                    [1, 2, 3, 4]
                                },
                            )
                            .unwrap(),
                        );
                    });
                    let mut game = game.unwrap();
                    let calls = clock_calls.clone();
                    let date = loaded.date;
                    game.set_clock_source(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(date)
                    });
                    let mut slot = TitleSlot::from_game(game);
                    let end = Instant::now() + Duration::from_secs(10);
                    let mut number = 0;
                    loop {
                        number += 1;
                        shin_tasks::block_on(slot.update_game(
                            Ticks::from_f32(3.0),
                            true,
                            Buttons::default(),
                            &PointerFrame {
                                number,
                                time_seconds: f64::from(number) / 60.0,
                                events: &[],
                            },
                            policy(),
                            audio(),
                        ))
                        .unwrap();
                        if matches!(
                            slot.game
                                .as_ref()
                                .unwrap()
                                .host()
                                .unwrap()
                                .runner
                                .pending_command(),
                            Some(
                                shin_core::vm::higurashi::command::HigurashiRuntimeCommand::WAIT {
                                    amount: 1000,
                                    ..
                                }
                            )
                        ) {
                            break;
                        }
                        assert!(Instant::now() < end);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    slot.game
                        .as_mut()
                        .unwrap()
                        .bindings
                        .history
                        .record_completion(true, 73);
                    slot
                } else {
                    TitleSlot::new(
                        TitleTransition {
                            request: crate::app::higurashi::title_demo::TitleRequest {
                                selected: 1,
                                mode: 0,
                            },
                            globals,
                        },
                        None,
                    )
                };
                let (composed, pixels) = capture(&device, &queue, size, &mut slot);
                let expected_global = if let Some(game) = &mut slot.game {
                    game.bindings.publish_globals(&mut game.globals.data);
                    game.globals.data.clone()
                } else {
                    slot.title.as_ref().unwrap().globals.data.clone()
                };
                let before_seed = seed_calls.load(Ordering::SeqCst);
                let before_clock = clock_calls.load(Ordering::SeqCst);
                let prior_seeds = slot.game.as_ref().map(|game| game.seeds.clone());
                // Scenario IDs are signed VM constructor inputs; even
                // i32::MAX is valid here. A manual transfer with nonzero
                // flags violates the proven b2cc8 flags0 boundary instead.
                slot.pending = Some(Pending::ManualLoad(Transfer::new(loaded.clone(), 1)));
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    let error = slot.dispatch_manual_load(context, &composed).unwrap_err();
                    assert!(format!("{error:#}").contains("Manual Load requires native flags0"));
                });
                assert_eq!(slot.game.is_some(), from_game);
                assert_eq!(slot.title.is_some(), !from_game);
                assert!(
                    slot.reference_scene_events
                        .iter()
                        .all(|&event| event != 1 && event != 2)
                );
                assert_eq!(seed_calls.load(Ordering::SeqCst), before_seed);
                assert_eq!(clock_calls.load(Ordering::SeqCst), before_clock);
                if let Some(Pending::ManualLoad(transfer)) = &mut slot.pending {
                    transfer.flags = 0;
                } else {
                    unreachable!()
                };
                assert!(slot.pending_manual_load());
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    assert!(slot.dispatch_manual_load(context, &composed).unwrap());
                });
                assert!(slot.pending.is_none());
                assert!(slot.title.is_none());
                let game = slot.game.as_ref().unwrap();
                assert_eq!(game.bindings.runtime_game_data().unwrap(), loaded);
                assert!(game.bindings.memory.entries().is_empty());
                assert_eq!(game.bindings.progress.responses().1, 0);
                assert_eq!(
                    game.globals.data.persist_data.0,
                    expected_global.persist_data.0
                );
                assert_eq!(game.globals.data.save_vectors, expected_global.save_vectors);
                assert_eq!(game.globals.data.save_menu_position, 2);
                assert!(Arc::ptr_eq(&game.globals.sounds, &sounds));
                let actual = game.globals.runtime_saves.as_ref().unwrap();
                assert_eq!(actual.auto.valid, expected_slots.auto.valid);
                assert_eq!(actual.auto.data, expected_slots.auto.data);
                for (a, e) in actual.manual.iter().zip(&expected_slots.manual) {
                    assert_eq!(a.valid, e.valid);
                    assert_eq!(a.data, e.data);
                }
                assert_eq!(
                    &slot.reference_scene_events[slot.reference_scene_events.len() - 2..],
                    &[1, 2]
                );
                assert_eq!(
                    copied(
                        &device,
                        &queue,
                        size,
                        &game.snapshot.as_ref().unwrap().snapshot
                    ),
                    pixels
                );
                assert_eq!(clock_calls.load(Ordering::SeqCst), before_clock);
                if let Some(prior) = prior_seeds {
                    assert!(Arc::ptr_eq(&game.seeds, &prior));
                    assert!(seed_calls.load(Ordering::SeqCst) > before_seed);
                }
                profiles += 1;
            }
        }
    }
    eprintln!(
        "Manual Load live scene transfer:{profiles} Title/ADV profiles,both active records,exact720p/1080p composed captures,full flags0 payload/global101 slots/read-choice owners,preflight source retention and retirement-before-constructor pass;offscreen Metal/exact Loopback Audio,no window/user saves"
    );
}
