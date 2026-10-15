use super::*;
use crate::{
    adv::{
        higurashi::music_commands::MusicCommands,
        higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
        input_area::{PointerEvent, PointerKind},
    },
    app::higurashi::{
        gameplay::tests::{audio, fresh, policy},
        runtime_game_data::RuntimeGameData,
        runtime_saves::Slots,
    },
};
use shin_core::{
    format::{save::higurashi::SaveDate, scenario::higurashi::HigurashiVariant},
    primitives::color::UnormColor,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosColVertex,
    },
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

mod native;

fn marker(pass: &mut RenderPass) {
    let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| PosColVertex {
        position: (glam::vec2(-0.5, 0.5) + uv * glam::vec2(0.25, -0.25)).extend(0.0),
        color: UnormColor::from_rgba(32, 96, 160, 255),
    });
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::Fill {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            transform: glam::Mat4::IDENTITY,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
struct Frames {
    number: u32,
    size: (u32, u32),
    composed: Option<RenderTexture>,
}
impl Frames {
    fn pointer<'a>(&mut self, events: &'a [PointerEvent]) -> PointerFrame<'a> {
        self.number += 1;
        PointerFrame {
            number: self.number,
            time_seconds: f64::from(self.number) / 20.0,
            events,
        }
    }
    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
    ) -> Vec<u8> {
        let mut composed = None;
        let image = reference_render_to_texture(device, queue, self.size, |context, target| {
            slot.prepare(context).unwrap();
            let mut frame = context.new_render_texture("quiz platform composed frame".into());
            for output in [
                frame.as_texture_target(),
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
            ] {
                let mut pass =
                    context.begin_pass(output, Some(context.depth_stencil), "quiz scene frame");
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                slot.render(&mut pass).unwrap();
                marker(&mut pass);
            }
            composed = Some(frame);
        });
        self.composed = composed;
        image
    }
    fn platform(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut TitleSlot,
        elapsed: f32,
    ) -> Vec<u8> {
        let pointer = self.pointer(&[]);
        reference_render_to_texture(device, queue, self.size, |context, _| {
            shin_tasks::block_on(slot.update_frame(
                context,
                self.composed.as_ref().unwrap(),
                (self.size.0 as u16, self.size.1 as u16),
                Ticks::from_f32(elapsed),
                true,
                Buttons::default(),
                &pointer,
                policy(),
                audio(),
            ))
            .unwrap();
        });
        self.draw(device, queue, slot)
    }
    fn quiz(&mut self, slot: &mut TitleSlot, delta: f32, pressed: u32, events: &[PointerEvent]) {
        let pointer = self.pointer(events);
        slot.update_quiz(
            Ticks::from_f32(delta),
            true,
            Buttons {
                pressed,
                ..Buttons::default()
            },
            &pointer,
        )
        .unwrap();
    }
    fn question(&mut self, slot: &mut TitleSlot, index: i32) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let m = &slot.quiz.as_ref().unwrap().motion;
            if m.model.current_question == index && m.model.clocks[2].is_idle() {
                break;
            }
            assert!(m.model.current_question <= index);
            self.quiz(slot, 3.0, 0, &[]);
            assert!(Instant::now() < end, "quiz question gate timeout");
            if !slot.quiz.as_ref().unwrap().motion.ready {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn resume(&mut self, slot: &mut TitleSlot) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let pointer = self.pointer(&[]);
            shin_tasks::block_on(slot.update_game(
                Ticks::from_f32(3.0),
                true,
                Buttons::default(),
                &pointer,
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
                Some(Cmd::WAIT { amount: 1000, .. })
            ) {
                break;
            }
            assert!(
                Instant::now() < end,
                "quiz replay did not resume following script"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
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
            "quiz composed capture",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        overlay.render(&mut pass);
    })
}
fn assert_manual(actual: &Slots, expected: &Slots) {
    for (a, e) in actual.manual.iter().zip(&expected.manual) {
        assert_eq!(a.valid, e.valid);
        assert_eq!(a.data, e.data);
    }
}

#[test]
#[ignore = "actual next-frame Gameplay/Quiz/ADV transfers/full records/manual-autosave-histories/seed-clock owners/source retirement/exact captures/score replay at720p/1080p on offscreen Metal/exact Loopback Audio;no window/saves"]
fn higurashi_quiz_scene_roundtrips_preserve_runtime_owners_captures_and_script_score() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let code = &[
        0x82, 7, 1, 0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0xd7, 31, 0, 2, 3, 4, 0x82,
        9, 2, 0x83, 0, 0x83, 0xe8, 0, 0, 0,
    ];
    let (authored, common) =
        crate::app::higurashi::gameplay::tests::character::common(&server, code);
    let mut captures = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for active in 0..2 {
            let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
            let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
            let mut saved = SaveData::default();
            saved.settings.common.v3_sysvol = 53;
            saved.save_vectors.seen_messages = vec![0xa5; 4];
            saved.save_vectors.select_seen_choices = vec![0x1234; 3];
            saved.save_vectors.select_last_choices = vec![1, 2, 3, 4];
            let legacy = fresh();
            saved.auto_save_slot = Some(legacy.clone());
            saved.manual_save_slots[2] = Some(legacy.clone());
            let mut slots = Slots::loaded(&saved);
            slots.manual[2].data = crate::app::higurashi::tips_menu::tests::source(1000, 1);
            slots.auto.data.trailing_values = vec![0xffff, 0x1234];
            slots.auto.data.byte_variables = vec![9, 8];
            let manual = slots.clone();
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
            let old_date = SaveDate {
                year: 2025,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            };
            let now = SaveDate {
                year: 2026,
                month: 10,
                day: 13,
                hour: 20,
                minute: 21,
                second: 22,
            };
            let mut source = RuntimeGameData::from(fresh());
            source.date = old_date;
            source.progress.extra_word = 0xdeadbe12;
            source.progress.entries[0].random_seed = 42;
            source.progress.entries[0].response_data = vec![0x23, 0xee];
            source.progress.entries[1] = crate::adv::higurashi::autosave::ProgressEntry {
                scenario_id: 2084,
                random_seed: 0x87654321,
                save_position: 19,
                replay_target: 23,
                response_data: vec![0xf1, 0x55, 0x23, 0xee],
            };
            if active == 1 {
                source.progress.entries.swap(0, 1);
                source.progress.set_active_index(1).unwrap();
            }
            source.trailing_values = vec![7, 0xffff];
            source.byte_variables = vec![2, 255];
            let dormant = source.progress.entries[1 - active].clone();
            let seeds = Arc::new(AtomicUsize::new(0));
            let seed_calls = seeds.clone();
            let mut game = None;
            reference_render_to_texture(&device, &queue, size, |context, _| {
                let mut initial = context.new_render_texture("initial quiz source".into());
                {
                    let mut pass = context.begin_pass(
                        initial.as_texture_target(),
                        Some(context.depth_stencil),
                        "initial quiz",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                }
                game = Some(
                    Gameplay::new(globals, source.clone(), 0, initial, move || {
                        seed_calls.fetch_add(1, Ordering::SeqCst);
                        [1, 2, 3, 4]
                    })
                    .unwrap(),
                );
            });
            let mut game = game.unwrap();
            let clock_calls = Arc::new(AtomicUsize::new(0));
            let calls = clock_calls.clone();
            game.set_clock_source(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(now)
            });
            game.bindings.history.record_completion(true, 73);
            let mut slot = TitleSlot::from_game(game);
            let mut frames = Frames {
                number: 0,
                size,
                composed: None,
            };
            let end = Instant::now() + Duration::from_secs(10);
            while slot.pending_quiz().is_none() {
                let pointer = frames.pointer(&[]);
                shin_tasks::block_on(slot.update_game(
                    Ticks::from_f32(3.0),
                    true,
                    Buttons::default(),
                    &pointer,
                    policy(),
                    audio(),
                ))
                .unwrap();
                assert!(Instant::now() < end, "source QUIZ request timeout");
                std::thread::sleep(Duration::from_millis(1));
            }
            let payload = slot.pending_quiz().unwrap().data.clone();
            assert_eq!(payload.date, old_date);
            assert_eq!(payload.progress.active_index(), active);
            assert_eq!(payload.progress.entries[1 - active], dormant);
            assert_eq!(payload.progress.active_entry().save_position, 2);
            assert_eq!(payload.progress.active_entry().response_data, [0x23, 0xee]);
            let initial_calls = clock_calls.load(Ordering::SeqCst);
            assert_eq!(initial_calls, 2);
            let old_seed_owner = slot.game.as_ref().unwrap().seeds.clone();
            let history = slot
                .game
                .as_ref()
                .unwrap()
                .bindings
                .history
                .bits()
                .words()
                .as_ptr();
            let choices = slot.game.as_ref().unwrap().bindings.choices.last().as_ptr();
            let ids = slot.game.as_ref().unwrap().host().unwrap().next_mode_ids();
            let before = frames.draw(&device, &queue, &mut slot);
            // Invalid arguments/extent/metadata reject before entropy, capture or retirement.
            for bad in 0..4 {
                let old_common = slot.game.as_ref().unwrap().globals.common.clone();
                if bad == 0 {
                    if let Some(Pending::Quiz(r)) = &mut slot.pending {
                        r.args[0] = 8;
                    }
                }
                if bad == 1 {
                    if let Some(Pending::Quiz(r)) = &mut slot.pending {
                        r.args[1] = 9;
                    }
                }
                // Variant fault uses the same loaded metadata with its tag rejected.
                if bad == 3 {
                    let mut scenario =
                        shin_core::format::scenario::higurashi::HigurashiScenario::new(
                            old_common.scenario.raw().to_vec().into(),
                        )
                        .unwrap();
                    scenario.variant = HigurashiVariant::Original;
                    slot.game.as_mut().unwrap().globals.common = Arc::new(CommonAssets {
                        scenario: Arc::new(scenario),
                        font: old_common.font.clone(),
                        system: old_common.system.clone(),
                        message: old_common.message.clone(),
                        effects: old_common.effects.clone(),
                        sysse: old_common.sysse.clone(),
                    });
                }
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    assert!(
                        slot.dispatch_quiz_with_seed(
                            context,
                            frames.composed.as_ref().unwrap(),
                            if bad == 2 {
                                (0, 720)
                            } else {
                                (size.0 as u16, size.1 as u16)
                            },
                            || panic!("preflight consumed entropy")
                        )
                        .is_err()
                    );
                });
                assert!(slot.game.is_some() && slot.quiz.is_none() && slot.continuation.is_none());
                assert_eq!(slot.pending_quiz().unwrap().data, payload);
                if let Some(Pending::Quiz(r)) = &mut slot.pending {
                    r.args = [2, 3, 4];
                }
                slot.game.as_mut().unwrap().globals.common = old_common;
            }
            let game = slot.game.as_mut().unwrap();
            game.host_mut().unwrap().audio.browser_music_multiplier(0.5);
            let mut job = crate::app::higurashi::gameplay::selection::character::Job::new(
                server.clone(),
                &common,
                5,
            );
            let retirement = job.retirement.clone();
            let task = job.task.take().unwrap();
            let (sender, receiver) = futures::channel::oneshot::channel::<()>();
            job.task = Some(shin_tasks::async_io::spawn(async move {
                receiver.await.unwrap();
                task.await
            }));
            game.selection.character.job = Some(job);
            slot.reference_scene_events.clear();
            let old_clock = slot.game.as_ref().unwrap().globals.clock.ticks();
            if active == 1 {
                let mut used = 0;
                reference_render_to_texture(&device, &queue, size, |context, _| {
                    assert!(
                        slot.dispatch_quiz_with_seed(
                            context,
                            frames.composed.as_ref().unwrap(),
                            (size.0 as u16, size.1 as u16),
                            || {
                                assert!(retirement.is_finished());
                                used += 1;
                                17
                            }
                        )
                        .unwrap()
                    );
                });
                assert_eq!(used, 1);
            }
            let after = frames.platform(&device, &queue, &mut slot, 0.0);
            assert_eq!(std::mem::take(&mut slot.reference_scene_events), [1, 2, 3]);
            assert!(retirement.is_finished());
            drop(sender);
            assert_eq!(before, after);
            captures += 1;
            assert_eq!(
                copied(
                    &device,
                    &queue,
                    size,
                    slot.quiz.as_ref().unwrap().reference_capture().unwrap()
                ),
                before
            );
            captures += 1;
            let owner = slot.quiz.as_ref().unwrap();
            assert_eq!(owner.globals.clock.ticks(), old_clock);
            assert_eq!(owner.motion.model.data, payload);
            assert_manual(owner.globals.runtime_saves.as_ref().unwrap(), &manual);
            let first_retirement = owner.reference_retirement().unwrap();
            frames.question(&mut slot, 0);
            assert!(first_retirement.is_finished());
            assert_eq!(
                slot.quiz
                    .as_ref()
                    .unwrap()
                    .globals
                    .audio
                    .reference_music_focus(),
                0.5
            );
            for index in 0..10 {
                frames.question(&mut slot, index);
                let correct = slot.quiz.as_ref().unwrap().motion.model.correct_answer as usize;
                let answer = if active == 0 {
                    correct
                } else {
                    (correct + 1) % 4
                };
                if size.0 == 1920 {
                    let r = crate::app::higurashi::quiz_scene::ANSWER_RECTS[answer];
                    let position =
                        glam::ivec2(((r[0] + r[2]) * 0.5) as i32, ((r[1] + r[3]) * 0.5) as i32);
                    for kind in [PointerKind::Down, PointerKind::Up] {
                        frames.quiz(
                            &mut slot,
                            0.0,
                            0,
                            &[PointerEvent {
                                kind,
                                id: 7,
                                position,
                            }],
                        );
                    }
                } else {
                    frames.quiz(&mut slot, 0.0, 1 << [12, 13, 14, 15][answer], &[]);
                }
                assert_eq!(
                    slot.quiz.as_ref().unwrap().motion.model.score,
                    if active == 0 { index + 1 } else { 0 }
                );
            }
            while !slot.quiz.as_ref().unwrap().motion.model.closing {
                frames.quiz(&mut slot, 3.0, 0, &[]);
            }
            // This explicit stop controls only the hardware BGM Finish gate.
            slot.quiz.as_mut().unwrap().globals.audio.stop_bgm(0);
            let end = Instant::now() + Duration::from_secs(15);
            while slot.pending.is_none() {
                frames.quiz(&mut slot, 3.0, 0, &[]);
                assert!(Instant::now() < end, "quiz EOF/fade return timeout");
                std::thread::sleep(Duration::from_millis(2));
            }
            let expected = if active == 0 { 10 } else { 0 };
            let Some(Pending::Gameplay(returning)) = &slot.pending else {
                panic!("missing score factory");
            };
            assert_eq!(returning.flags, expected);
            assert_eq!(returning.data, payload);
            let held_auto = slot.continuation.as_ref().unwrap().autosave.clone();
            let auto_ptr = slot
                .continuation
                .as_ref()
                .unwrap()
                .autosave
                .progress
                .entries[active]
                .response_data
                .as_ptr();
            let last = frames.draw(&device, &queue, &mut slot);
            let old_common = slot.quiz.as_ref().unwrap().globals.common.clone();
            let mut invalid = shin_core::format::scenario::higurashi::HigurashiScenario::new(
                old_common.scenario.raw().to_vec().into(),
            )
            .unwrap();
            invalid.variant = HigurashiVariant::Original;
            slot.quiz.as_mut().unwrap().globals.common = Arc::new(CommonAssets {
                scenario: Arc::new(invalid),
                font: old_common.font.clone(),
                system: old_common.system.clone(),
                message: old_common.message.clone(),
                effects: old_common.effects.clone(),
                sysse: old_common.sysse.clone(),
            });
            let before_seeds = seeds.load(Ordering::SeqCst);
            reference_render_to_texture(&device, &queue, size, |context, _| {
                assert!(
                    slot.dispatch_kakera_return(context, frames.composed.as_ref().unwrap())
                        .is_err()
                );
            });
            assert!(slot.quiz.is_some() && slot.game.is_none() && slot.continuation.is_some());
            let Some(Pending::Gameplay(r)) = &slot.pending else {
                panic!("lost return during preflight");
            };
            assert_eq!(r.data, payload);
            assert_eq!(r.flags, expected);
            assert_eq!(seeds.load(Ordering::SeqCst), before_seeds);
            slot.quiz.as_mut().unwrap().globals.common = old_common;
            slot.reference_scene_events.clear();
            let returned = frames.platform(&device, &queue, &mut slot, 0.0);
            assert_eq!(std::mem::take(&mut slot.reference_scene_events), [1, 2, 3]);
            assert_eq!(last, returned);
            captures += 1;
            let game = slot.game.as_ref().unwrap();
            assert!(slot.quiz.is_none() && slot.continuation.is_none());
            assert_eq!(game.runtime_game_data().unwrap(), payload);
            assert_eq!(game.flags, expected);
            assert_eq!(game.bindings.autosave, held_auto);
            assert_eq!(
                game.bindings.autosave.progress.entries[active]
                    .response_data
                    .as_ptr(),
                auto_ptr
            );
            assert_eq!(game.bindings.history.bits().words().as_ptr(), history);
            assert_eq!(game.bindings.choices.last().as_ptr(), choices);
            assert!(Arc::ptr_eq(&game.seeds, &old_seed_owner));
            assert_eq!(game.host().unwrap().next_mode_ids(), ids);
            assert!(
                Arc::ptr_eq(&game.globals.common, &common)
                    && Arc::ptr_eq(&game.globals.sounds, &sounds)
            );
            assert_manual(game.globals.runtime_saves.as_ref().unwrap(), &manual);
            assert_eq!(game.host().unwrap().audio.reference_music_focus(), 0.5);
            frames.resume(&mut slot);
            let game = slot.game.as_ref().unwrap();
            assert_eq!(game.host().unwrap().runner.reference_register(
                shin_core::format::scenario::instruction_elements::Register::from_regular_register(31)),Some(expected as i32));
            assert_eq!(
                game.bindings.progress.responses().0,
                [(expected as u8) << 4, 0xee]
            );
            assert_eq!(game.bindings.progress.responses().1, 4);
            assert_eq!(
                game.bindings.progress.progress().entries[1 - active],
                dormant
            );
            assert_eq!(clock_calls.load(Ordering::SeqCst), initial_calls);
            assert_eq!(game.globals.data.manual_save_slots[2], Some(legacy));
            assert_eq!(game.globals.data.settings.common.v3_sysvol, 53);
            assert_manual(game.globals.runtime_saves.as_ref().unwrap(), &manual);
        }
    }
    println!(
        "Quiz live scenes:4 real ten-question roundtrips/both active records/score0 and10 at720p/1080p;{captures} byte-identical whole composed capture/transfer checks,source producer retirement/preflight rejection,full records/manual-autosave/history-choice/seed-clock/mode owners/global music focus and four-bit/full-register script replay pass;offscreen Metal/exact Loopback Audio,no window/saves"
    );
}
