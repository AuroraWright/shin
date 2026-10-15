use super::*;
use crate::app::higurashi::kakera_menu::tests::{persistent, source};
use crate::{
    adv::play_clock::PlayClock,
    adv::{
        higurashi::{
            audio::HigurashiAudio, input::Buttons, reference_gpu_assets,
            reference_render_to_texture,
        },
        input_area::{PointerEvent, PointerKind},
    },
    audio::SystemSoundPlayer,
};
use parking_lot::Mutex;
use std::time::{Duration, Instant};

fn globals(server: &Arc<AssetServer>) -> Globals {
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
    data.persist_data = persistent(1, 0, 2);
    Globals {
        storage: None,
        runtime_saves: None,
        common: common.clone(),
        data,
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
        server: server.clone(),
        manager,
        sounds,
    }
}
fn tick(opening: &mut Opening, number: &mut u32, pressed: u32, events: &[PointerEvent]) {
    *number += 1;
    opening
        .update(
            Ticks::from_f32(3.0),
            true,
            Buttons {
                pressed,
                ..Default::default()
            },
            &PointerFrame {
                number: *number,
                time_seconds: f64::from(*number) / 60.0,
                events,
            },
        )
        .unwrap();
}
fn ready(opening: &mut Opening, number: &mut u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while opening.motion.first_phase().is_some() {
        tick(opening, number, 0, &[]);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "real Kakera ROM atlas/default mask loader/First/full parent/controller/contact/standard exit question/ADV payload and pending producer retirement on offscreen Metal/exact Loopback Audio;no window/saves"]
fn higurashi_kakera_opening_loads_real_resources_and_owns_input_question_and_adv_request() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    for (size, contact, exit) in [
        ((1280, 720), false, false),
        ((1920, 1080), true, false),
        ((1920, 1080), true, true),
    ] {
        let mut capture = None;
        reference_render_to_texture(&device, &queue, size, |context, _| {
            capture = Some(context.new_render_texture("Kakera source frame".into()));
        });
        let input = source(0, 1, 1, 64, 0);
        let old_primary = input.primary_entry.clone();
        let globals = globals(&server);
        let sounds = globals.sounds.clone();
        let mut opening = Opening::new(
            globals,
            0,
            input.clone(),
            capture,
            (size.0 as u16, size.1 as u16),
        );
        let mut frame = 0;
        let handles = opening.retirements();
        ready(&mut opening, &mut frame);
        assert!(opening.atlas.is_some());
        assert!(opening.job.is_none());
        assert!(opening.mask.is_none());
        assert!(opening.overlay.is_none());
        for handle in handles {
            assert!(handle.is_finished());
        }
        let root = reference_render_to_texture(&device, &queue, size, |context, target| {
            opening.prepare(context).unwrap();
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Kakera retained parent",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            opening.render(&mut pass);
        });
        assert!(root.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        if exit {
            tick(&mut opening, &mut frame, 1 << 25, &[]);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !opening
                .question
                .as_ref()
                .and_then(|q| q.ready.as_ref())
                .is_some_and(|(q, _)| q.phase() == 2)
            {
                tick(&mut opening, &mut frame, 0, &[]);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                opening
                    .question
                    .as_ref()
                    .unwrap()
                    .ready
                    .as_ref()
                    .unwrap()
                    .0
                    .selected(),
                1
            );
            reference_render_to_texture(&device, &queue, size, |context, target| {
                opening.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Kakera exit question",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                opening.render(&mut pass);
            });
            tick(&mut opening, &mut frame, 1 << 25, &[]);
            for _ in 0..40 {
                tick(&mut opening, &mut frame, 0, &[]);
            }
            assert!(opening.question.is_none());
            assert!(opening.motion.request.is_none());
            assert!(!opening.motion.closing);
            tick(&mut opening, &mut frame, 1 << 25, &[]);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !opening
                .question
                .as_ref()
                .and_then(|q| q.ready.as_ref())
                .is_some_and(|(q, _)| q.phase() == 2)
            {
                tick(&mut opening, &mut frame, 0, &[]);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            let model = &opening.question.as_ref().unwrap().ready.as_ref().unwrap().0;
            let rect = model.buttons()[0].rect;
            let point = ((model.origin()
                + glam::vec2(30.0, 46.0)
                + glam::vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5))
                * glam::vec2(size.0 as f32 / 1920.0, size.1 as f32 / 1080.0))
            .as_ivec2();
            tick(
                &mut opening,
                &mut frame,
                0,
                &[PointerEvent {
                    kind: PointerKind::Down,
                    id: 0,
                    position: point,
                }],
            );
            tick(
                &mut opening,
                &mut frame,
                0,
                &[PointerEvent {
                    kind: PointerKind::Up,
                    id: 0,
                    position: point,
                }],
            );
            assert_eq!(
                opening
                    .question
                    .as_ref()
                    .unwrap()
                    .ready
                    .as_ref()
                    .unwrap()
                    .0
                    .selected(),
                0
            );
        } else if contact {
            tick(
                &mut opening,
                &mut frame,
                0,
                &[PointerEvent {
                    kind: PointerKind::Down,
                    id: 0,
                    position: glam::ivec2(300, 330),
                }],
            );
            tick(
                &mut opening,
                &mut frame,
                0,
                &[PointerEvent {
                    kind: PointerKind::Up,
                    id: 0,
                    position: glam::ivec2(300, 330),
                }],
            );
        } else {
            tick(&mut opening, &mut frame, 1 << 24, &[]);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while opening.motion.request.is_none() {
            tick(&mut opening, &mut frame, 0, &[]);
            assert!(Instant::now() < deadline);
        }
        let request = opening.take_gameplay_request().unwrap();
        let returned = request.data.save_data();
        assert_eq!(request.flags, 0);
        assert_eq!(request.data.date, input.date);
        assert_eq!(request.data.trailing_values, input.trailing_values);
        assert_eq!(request.data.byte_variables, input.byte_variables);
        if exit {
            assert!(returned.primary_entry.is_none());
            assert_eq!(returned.entry, old_primary.unwrap());
            assert_eq!(
                request.data.progress.entries[1].response_data,
                input.entry.response_data
            );
            assert_eq!(
                request.data.progress.entries[1].random_seed,
                input.entry.random_seed
            );
        } else {
            assert_eq!(returned.primary_entry, old_primary);
            assert_eq!(returned.entry.scenario_id, 2000);
            assert_eq!(returned.entry.save_position, 0);
            assert_eq!(returned.entry.response_data, input.entry.response_data);
        }
        assert!(
            opening
                .update(
                    Ticks::from_f32(3.0),
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
        drop(sounds);
        let mut next = None;
        let globals = opening.into_globals();
        reference_render_to_texture(&device, &queue, size, |context, _| {
            let snapshot = context.new_render_texture("Kakera ADV return".into());
            next = Some(
                crate::app::higurashi::gameplay::Gameplay::new(
                    globals,
                    request.data.clone(),
                    0,
                    snapshot,
                    || [1, 2, 3, 4],
                )
                .unwrap(),
            );
        });
        assert_eq!(next.unwrap().runtime_game_data().unwrap(), request.data);
    }
    let opening = Opening::new(
        globals(&server),
        0,
        source(0, 0, 1, 64, 0),
        None,
        (1280, 720),
    );
    let handles = opening.retirements();
    drop(opening);
    for handle in handles {
        assert!(handle.is_finished());
    }
    eprintln!(
        "Kakera opening:real kakera.txa/default.msk publication and First capture/BGM retirement,full retained parent at720p/1080p,720p controller/1080p contact,actual composed exit question cancel/1080p touch confirm with direct viewport mapping,source date/tail/bytes/responses/primary and mode0 request quiescence,pending loader cancellation/drain pass;ordinary scene dispatch remains pending"
    );
}
