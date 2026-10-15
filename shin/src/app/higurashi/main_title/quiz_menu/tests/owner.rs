use super::*;
use crate::{
    adv::{
        higurashi::{
            reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
        },
        input_area::{PointerEvent, PointerKind},
    },
    app::higurashi::{
        load_game::Paths,
        main_title::{CategorySceneRequest, Opening, Phase},
        scene_host::{SceneHost, TitleTransition},
        title_demo::TitleRequest,
    },
    audio::SystemSoundPlayer,
};
use shin_core::{
    format::save::higurashi::{GameData, GameDataEntry, SaveDate},
    primitives::update::FrameId,
};
use std::time::{Duration, Instant};
fn poll(o: &mut Opening, number: &mut u32, a: u32, r: u32, events: &[PointerEvent]) {
    *number += 1;
    o.update_with_input(
        Ticks::ZERO,
        true,
        Buttons {
            pressed: a,
            repeat: r,
            ..Default::default()
        },
        &PointerFrame {
            events,
            number: *number,
            time_seconds: *number as f64 / 600.0,
        },
    )
    .unwrap();
}
fn enter_quiz(o: &mut Opening, number: &mut u32) {
    poll(o, number, 1 << 24, 0, &[]);
    o.update(Ticks::from_f32(100.0)).unwrap();
    o.update(Ticks::ZERO).unwrap();
    assert!(
        o.quiz.is_some() && o.top_visible && !o.quiz_visible,
        "Native constructor frame must retain old draw snapshot"
    );
    o.update(Ticks::from_f32(48.0)).unwrap();
    o.update(Ticks::ZERO).unwrap();
    assert!(o.top.is_none() && o.quiz_visible);
    assert_eq!(o.quiz.as_ref().unwrap().state.phase, 2);
}
#[test]
#[ignore = "real retained startup/Top-Quiz-Top/720p-1080p held physical input/fresh extra-word0 scenarios/90-frame ADV gate;offscreen Metal and exact Loopback Audio,no window/saves"]
fn higurashi_title_quiz_real_owner_preserves_contact_and_starts_authored_adv() {
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(parking_lot::Mutex::new(SystemSoundPlayer::new(
        manager.clone(),
    )));
    let mut host = SceneHost::new(
        server,
        manager,
        sounds,
        Arc::new(Paths {
            current: None,
            trial: None,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut id = FrameId::default();
    let mut number = 0;
    let transition = loop {
        id.advance();
        number += 1;
        if let Some(t) = host
            .update(
                id,
                Ticks::from_f32(3.0),
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &PointerFrame {
                    events: &[],
                    number,
                    time_seconds: number as f64 / 60.0,
                },
                &queue,
            )
            .unwrap()
        {
            break t;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut globals = Some(transition.globals);
    for (size, selected, logical) in [
        ((1280, 720), 0, glam::vec2(300.0, 475.0)),
        ((1920, 1080), 7, glam::vec2(800.0, 875.0)),
    ] {
        let mut o = Opening::new(
            TitleTransition {
                request: TitleRequest {
                    selected: 7,
                    mode: 0,
                },
                globals: globals.take().unwrap(),
            },
            None,
        );
        o.resize_menu_input((size.0 as u16, size.1 as u16));
        while o.motion.phase != Phase::Menu {
            o.update(Ticks::from_f32(3.0)).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        o.update(Ticks::from_f32(120.0)).unwrap();
        o.update(Ticks::ZERO).unwrap();
        enter_quiz(&mut o, &mut number);
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Actual retained title QuizMenu",
            );
            o.render(&mut pass);
        });
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        poll(&mut o, &mut number, 1 << 25, 0, &[]);
        o.update(Ticks::from_f32(48.0)).unwrap();
        o.update(Ticks::ZERO).unwrap();
        assert!(o.top.is_some() && o.quiz_visible && !o.top_visible);
        o.update(Ticks::from_f32(60.0)).unwrap();
        o.update(Ticks::ZERO).unwrap();
        assert!(o.quiz.is_none());
        // A immediate confirmation proving cancel restored native Top item7.
        enter_quiz(&mut o, &mut number);
        let point = (logical * (size.0 as f32 / 1920.0)).as_ivec2();
        poll(
            &mut o,
            &mut number,
            (1 << 24) | (1 << 25),
            1 << 6,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: point,
            }],
        );
        assert_eq!(o.quiz.as_ref().unwrap().state.pointer, selected);
        for _ in 0..3 {
            o.resize_menu_input((size.0 as u16, size.1 as u16));
            poll(&mut o, &mut number, (1 << 24) | (1 << 25), 0xf0, &[]);
            assert_eq!(
                o.quiz.as_ref().unwrap().state.phase,
                2,
                "Unchanged extent lost a held pointer and admitted controller confirmation"
            );
            assert_eq!(o.quiz.as_ref().unwrap().state.pointer, selected);
        }
        poll(
            &mut o,
            &mut number,
            (1 << 24) | (1 << 25),
            0xf0,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 0,
                position: glam::ivec2(-100, -100),
            }],
        );
        assert_eq!(o.quiz.as_ref().unwrap().state.phase, 3);
        assert_eq!(o.quiz.as_ref().unwrap().state.selected(), selected as usize);
        o.update(Ticks::from_f32(100.0)).unwrap();
        let calls = crate::engine_random::seed_call_count();
        crate::engine_random::with_reference_seed(5489, || o.update(Ticks::ZERO).unwrap());
        assert_eq!(crate::engine_random::seed_call_count(), calls + 1);
        assert!(o.take_category_scene_request().is_none());
        assert!(o.frame().black_alpha > 0.0 && o.frame().black_alpha < 1.0);
        for _ in 0..89 {
            o.update(Ticks::ZERO).unwrap();
            assert!(o.take_category_scene_request().is_none());
        }
        assert_eq!(o.frame().black_alpha, 1.0);
        o.update(Ticks::ZERO).unwrap();
        assert_eq!(
            o.take_category_scene_request(),
            Some(CategorySceneRequest::Story {
                flags: 0,
                data: GameData {
                    date: SaveDate {
                        year: 0,
                        month: 0,
                        day: 0,
                        hour: 0,
                        minute: 0,
                        second: 0
                    },
                    extra_word: 0,
                    entry: GameDataEntry {
                        scenario_id: 5 + selected,
                        random_seed: 3499211612,
                        save_position: 0,
                        replay_target: 0,
                        response_data: Vec::new()
                    },
                    primary_entry: None,
                    trailing_values: Vec::new(),
                    byte_variables: Vec::new(),
                }
            })
        );
        assert!(o.take_category_scene_request().is_none());
        assert!(o.quiz.is_none());
        let bank = o.globals.sounds.lock().bank.clone().unwrap();
        for name in ["decide", "cancel", "quiz_call"] {
            assert!(bank.sounds.contains_key(name));
        }
        globals = Some(o.into_globals());
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    println!(
        "Title QuizMenu:two real720p/1080p owners;Top7 cancel/borrowed snapshot/physical held contact across unchanged resize/controller capture/fresh extra-word0 scenario5 and12/one fresh seed/90-frame pre-clock flags0 ADV request/actual title artwork and system cue bank pass;exact Loopback Audio,no window/saves"
    );
}
