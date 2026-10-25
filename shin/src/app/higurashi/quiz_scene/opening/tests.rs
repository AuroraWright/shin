use super::*;
use crate::{
    adv::{
        higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
        input_area::{PointerEvent, PointerKind},
        play_clock::PlayClock,
    },
    audio::SystemSoundPlayer,
};
use parking_lot::Mutex;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn globals(server: &Arc<crate::asset::system::AssetServer>) -> Globals {
    let common = Arc::new(crate::app::higurashi::first_boot::CommonAssets {
        scenario: server.load_sync("/main.snr").unwrap(),
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut data = shin_core::format::save::higurashi::SaveData::default();
    data.settings.common.v2_voicevol = 71;
    Globals {
        storage: None,
        runtime_saves: None,
        common: common.clone(),
        data,
        clock: PlayClock::from_saved_seconds(12),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
        server: server.clone(),
        manager,
        sounds,
    }
}
fn tick(
    opening: &mut Opening,
    frame: &mut u32,
    delta: f32,
    permission: bool,
    pressed: u32,
    events: &[PointerEvent],
) {
    *frame += 1;
    opening
        .update(
            Ticks::from_f32(delta),
            permission,
            Buttons {
                pressed,
                ..Default::default()
            },
            &PointerFrame {
                number: *frame,
                time_seconds: f64::from(*frame) / 20.0,
                events,
            },
        )
        .unwrap();
}
fn question(opening: &mut Opening, frame: &mut u32, index: i32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while opening.motion.model.current_question != index
        || !opening.motion.model.clocks[2].is_idle()
    {
        assert!(opening.motion.model.current_question <= index);
        tick(opening, frame, 3.0, true, 0, &[]);
        assert!(
            Instant::now() < deadline,
            "quiz question did not become ready"
        );
        if !opening.motion.ready {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn event(kind: PointerKind, answer: usize, extent: (u16, u16)) -> PointerEvent {
    let r = super::super::ANSWER_RECTS[answer];
    PointerEvent {
        kind,
        id: 7,
        position: glam::IVec2::new(
            ((r[0] + r[2]) * 0.5 * f32::from(extent.0) / 1920.0) as i32,
            ((r[1] + r[3]) * 0.5 * f32::from(extent.1) / 1080.0) as i32,
        ),
    }
}
fn direction(answer: usize) -> u32 {
    1 << [12, 13, 14, 15][answer]
}

#[test]
#[ignore = "real quiz resources/voice/BGM/cues/input/captured First/full ten-question score factory at720p/1080p on offscreen Metal/the default audio output;no window/saves"]
fn higurashi_quiz_opening_owns_real_resources_input_audio_and_score_factory() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    for (size, active, context) in [((1280, 720), 0, 0), ((1920, 1080), 1, 8)] {
        let extent = (size.0 as u16, size.1 as u16);
        let data = crate::app::higurashi::tips_menu::tests::source(100, active);
        let mut capture = None;
        reference_render_to_texture(&device, &queue, size, |ctx, _| {
            capture = Some(ctx.new_render_texture("quiz source".into()));
        });
        let mut opening = Opening::new(
            globals(&server),
            data.clone(),
            [2, context, 9],
            17,
            capture,
            extent,
        )
        .unwrap();
        let retirement = opening.resources.retirement().unwrap();
        let mut frame = 0;
        question(&mut opening, &mut frame, 0);
        assert!(retirement.is_finished());
        assert!(opening.resources.atlas.is_some());
        assert!(opening.resources.prepared.as_ref().unwrap().mask.is_none());
        assert!(opening.overlay.is_none());
        assert!(opening.resources.published);
        assert_eq!(opening.motion.model.data, data);
        let correct = opening.motion.model.correct_answer as usize;
        tick(
            &mut opening,
            &mut frame,
            0.0,
            true,
            0,
            &[event(PointerKind::Down, correct, extent)],
        );
        tick(
            &mut opening,
            &mut frame,
            0.0,
            false,
            direction(correct),
            &[],
        );
        assert_eq!(opening.motion.model.contact, [-1; 3]);
        tick(
            &mut opening,
            &mut frame,
            0.0,
            true,
            0,
            &[event(PointerKind::Up, correct, extent)],
        );
        assert_eq!(opening.motion.model.chosen_answer, -1);
        for index in 0..10 {
            question(&mut opening, &mut frame, index);
            let correct = opening.motion.model.correct_answer as usize;
            if index % 2 == 0 {
                // Real contact capture suppresses the simultaneous wrong direction.
                tick(
                    &mut opening,
                    &mut frame,
                    0.0,
                    true,
                    direction((correct + 1) % 4),
                    &[event(PointerKind::Down, correct, extent)],
                );
                assert_eq!(opening.motion.model.chosen_answer, -1);
                tick(
                    &mut opening,
                    &mut frame,
                    0.0,
                    true,
                    0,
                    &[event(PointerKind::Up, correct, extent)],
                );
            } else {
                tick(&mut opening, &mut frame, 0.0, true, direction(correct), &[]);
            }
            assert_eq!(opening.motion.model.chosen_answer, correct as i32);
            assert_eq!(opening.motion.model.score, index + 1);
            assert!(opening.motion.model.reaction_variant > 0);
            assert!(opening.voice.status() & 2 != 0);
            // Compose the retained real text/BUP/question layers after both kinds of input.
            if index == 0 || index == 9 {
                let image = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                    opening.prepare(ctx).unwrap();
                    let mut pass = ctx.begin_pass(shin_render::shaders::types::texture::TextureTarget {
                        view: target, kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },Some(ctx.depth_stencil),"retained quiz");
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    opening.render(&mut pass).unwrap();
                });
                assert!(image.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            }
        }
        while !opening.motion.model.closing {
            tick(&mut opening, &mut frame, 3.0, true, 0, &[]);
        }
        assert_eq!(opening.motion.model.marks[..10], [1; 10]);
        assert_eq!(opening.motion.model.marks[10], 0);
        assert_eq!(opening.motion.model.clamped_argument, 0);
        assert!(opening.motion.score_request.is_none());
        // Real BGM deliberately holds Finish; stopping it is an explicit test
        // control. The final authored voice must actually reach EOF.
        tick(&mut opening, &mut frame, 0.0, true, 0, &[]);
        assert!(opening.globals.audio.bgm_status() & 2 != 0);
        assert!(opening.motion.model.clocks[0].is_idle());
        opening.globals.audio.stop_bgm(0);
        let deadline = Instant::now() + Duration::from_secs(15);
        while opening.motion.score_request.is_none() {
            tick(&mut opening, &mut frame, 3.0, true, 0, &[]);
            assert!(
                Instant::now() < deadline,
                "quiz final voice/fade did not release return"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(opening.motion.score_request, Some(10));
        assert!(
            opening
                .update(
                    Ticks::ZERO,
                    true,
                    Buttons::default(),
                    &PointerFrame {
                        number: frame + 1,
                        time_seconds: 0.0,
                        events: &[]
                    }
                )
                .is_err()
        );
        let mut request = opening.take_scene_request().unwrap();
        assert_eq!(request.flags, 10);
        assert_eq!(request.data, data);
        request.data.progress.entries[0].response_data.push(99);
        assert_eq!(opening.motion.model.data, data);
        assert!(opening.take_scene_request().is_none());
        assert!(opening.globals.data.save_vectors.unlocked_bgms[85 / 32] & (1 << (85 % 32)) != 0);
        drop(opening.into_globals());
    }
    // Dispose the whole concrete owner while its worker is still pending.
    let opening = Opening::new(
        globals(&server),
        crate::app::higurashi::tips_menu::tests::source(100, 0),
        [1, 4, 9],
        1,
        None,
        (1280, 720),
    )
    .unwrap();
    let retirement = opening.resources.retirement().unwrap();
    drop(opening);
    assert!(retirement.is_finished());
    println!(
        "Quiz retained Opening:two complete ten-question owners,720p/1080p actual resources/text/BUP/atlas frames,controller/contact capture/denied reset,scene S99 voice/cues/global BGM Finish gate/real final EOF/score10/full independent data return and early worker retirement pass;offscreen Metal/the default audio output,no window/saves;live scene-slot attachment remains pending"
    );
}
