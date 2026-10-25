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
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

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
    data.save_vectors = super::super::tests::vectors(3);
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
fn tick(
    opening: &mut Opening,
    number: &mut u32,
    delta: f32,
    buttons: Buttons,
    events: &[PointerEvent],
) {
    *number += 1;
    opening
        .update(
            Ticks::from_f32(delta),
            true,
            buttons,
            &PointerFrame {
                number: *number,
                time_seconds: f64::from(*number) / 60.0,
                events,
            },
        )
        .unwrap();
}
fn ready(opening: &mut Opening, number: &mut u32) {
    let end = Instant::now() + Duration::from_secs(10);
    while let Some(first) = opening.motion.first() {
        let pressed = if first.phase >= 0 { 1 << 24 } else { 0 };
        tick(
            opening,
            number,
            3.0,
            Buttons {
                pressed,
                ..Buttons::default()
            },
            &[],
        );
        assert!(opening.motion.request.is_none());
        assert!(!opening.motion.model.closing);
        assert!(Instant::now() < end, "TIPS First did not retire");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "real TIPS atlas/default mask/First/controller/contact/full payload/audio and producer retirement on offscreen Metal/the default audio output;no window/saves"]
fn higurashi_tips_opening_loads_resources_and_owns_live_input_audio_and_requests() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let mut owners = 0;
    let mut renders = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for (has, alternate, retreat, touch) in [
            (true, false, false, false),
            (true, true, true, false),
            (false, false, true, true),
            (false, true, false, true),
        ] {
            let mut capture = None;
            reference_render_to_texture(&device, &queue, size, |context, _| {
                let mut snapshot = context.new_render_texture("TIPS previous scene".into());
                let mut pass = context.begin_pass(
                    snapshot.as_texture_target(),
                    Some(context.depth_stencil),
                    "TIPS previous scene clear",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::from_rgba(
                        31, 63, 127, 255,
                    )),
                    Some(0),
                    Some(1.0),
                );
                drop(pass);
                capture = Some(snapshot);
            });
            let input = super::super::tests::source(if alternate { 11000 } else { 100 }, 0);
            let globals = globals(&server);
            let sounds = globals.sounds.clone();
            let mut opening = Opening::new(
                globals,
                has.then(|| input.clone()),
                if alternate { 188 } else { 0 },
                u32::from(alternate),
                capture,
                (size.0 as u16, size.1 as u16),
            )
            .unwrap();
            let dates = Arc::new(AtomicUsize::new(0));
            let seeds = Arc::new(AtomicUsize::new(0));
            let d = dates.clone();
            let s = seeds.clone();
            let chosen = SaveDate {
                year: 2026,
                month: 10,
                day: 13,
                hour: 1,
                minute: 2,
                second: 3,
            };
            opening.set_sources(
                move || {
                    d.fetch_add(1, Ordering::SeqCst);
                    Ok(chosen)
                },
                move || {
                    s.fetch_add(1, Ordering::SeqCst);
                    Ok(0x8badf00d)
                },
            );
            let retirement = opening.retirements();
            let mut frame = 0;
            ready(&mut opening, &mut frame);
            assert!(opening.atlas.is_some());
            assert!(opening.job.is_none() && opening.mask.is_none() && opening.overlay.is_none());
            assert!(retirement.iter().all(|h| h.is_finished()));
            assert_eq!(opening.motion.model.data, has.then(|| input.clone()));
            let bgm = if alternate { 168 } else { 77 };
            assert!(
                opening.globals.data.save_vectors.unlocked_bgms[bgm / 32] & (1 << (bgm % 32)) != 0
            );
            let pixels = reference_render_to_texture(&device, &queue, size, |context, target| {
                opening.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "TIPS live parent",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                opening.render(&mut pass);
            });
            assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            renders += 1;
            let before = opening.motion.model.scroll.value();
            tick(
                &mut opening,
                &mut frame,
                3.0,
                Buttons {
                    repeat: 1 << 11,
                    ..Buttons::default()
                },
                &[],
            );
            let after = opening.motion.model.scroll.value();
            assert!(after > before);
            assert_eq!(opening.motion.scrollbar.position, before as i32);
            tick(&mut opening, &mut frame, 3.0, Buttons::default(), &[]);
            tick(&mut opening, &mut frame, 0.0, Buttons::default(), &[]);
            if touch {
                let logical = if retreat {
                    glam::vec2(1800.0, 60.0)
                } else {
                    glam::vec2(200.0, 208.0)
                };
                let point = (logical * glam::vec2(size.0 as f32 / 1920.0, size.1 as f32 / 1080.0))
                    .as_ivec2();
                tick(
                    &mut opening,
                    &mut frame,
                    0.0,
                    Buttons::default(),
                    &[PointerEvent {
                        kind: PointerKind::Down,
                        id: 0,
                        position: point,
                    }],
                );
                tick(
                    &mut opening,
                    &mut frame,
                    0.0,
                    Buttons::default(),
                    &[PointerEvent {
                        kind: PointerKind::Up,
                        id: 0,
                        position: point,
                    }],
                );
            } else {
                tick(
                    &mut opening,
                    &mut frame,
                    0.0,
                    Buttons {
                        pressed: if retreat { 1 << 25 } else { 1 << 24 },
                        ..Buttons::default()
                    },
                    &[],
                );
            }
            assert!(opening.motion.model.closing);
            assert!(opening.motion.request.is_none());
            assert!(sounds.lock().bank.is_some());
            for _ in 0..8 {
                tick(&mut opening, &mut frame, 1.0, Buttons::default(), &[]);
            }
            let request = opening
                .take_scene_request()
                .expect("TIPS callback did not queue its scene");
            assert!(opening.take_scene_request().is_none());
            match request {
                SceneRequest::Story { data, flags } => {
                    if retreat {
                        assert!(has);
                        assert_eq!(flags, 1);
                        assert_eq!(data, input);
                    } else {
                        assert_eq!(flags, 0);
                        assert_eq!(data.progress.active_index(), usize::from(has));
                        assert_eq!(data.progress.active_entry().random_seed, 0x8badf00d);
                        assert_eq!(data.progress.active_entry().save_position, 0);
                        assert_eq!(data.progress.active_entry().replay_target, 0);
                        if has {
                            assert_eq!(data.date, input.date);
                            assert_eq!(data.progress.entries[0], input.progress.entries[0]);
                            assert_eq!(
                                data.progress.entries[1].response_data,
                                input.progress.entries[1].response_data
                            );
                            assert_eq!(data.progress.extra_word, input.progress.extra_word);
                            assert_eq!(data.trailing_values, input.trailing_values);
                            assert_eq!(data.byte_variables, input.byte_variables);
                        } else {
                            assert_eq!(data.date, chosen);
                            assert_eq!(data.progress.extra_word, 0);
                            assert!(
                                data.trailing_values.is_empty() && data.byte_variables.is_empty()
                            );
                        }
                    }
                }
                SceneRequest::Title { selected, mode } => {
                    assert!(!has && retreat && !alternate);
                    assert_eq!((selected, mode), (6, u32::MAX));
                }
                SceneRequest::Origin { .. } => panic!("unexpected alternate retreat"),
            }
            assert_eq!(dates.load(Ordering::SeqCst), usize::from(!has && !retreat));
            assert_eq!(seeds.load(Ordering::SeqCst), usize::from(!retreat));
            assert_eq!(opening.motion.model.data, has.then(|| input.clone()));
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
            drop(opening);
            owners += 1;
        }
    }
    for selected in 0..6 {
        let opening = Opening::new(globals(&server), None, selected, 0, None, (1280, 720)).unwrap();
        let handles = opening.retirements();
        drop(opening);
        assert!(handles.iter().all(|h| h.is_finished()));
        owners += 1;
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "TIPS opening:{owners} real owners/{renders}720p-1080p parents;real atlas/mask/First-retirement/input-next-frame/BGM77-168/scrollbar pre-clock/controller-contact/native cues/full requests and fresh-source services;pending producers retired,the default audio output,no window/saves"
    );
}
