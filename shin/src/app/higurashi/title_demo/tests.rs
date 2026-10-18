use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "original updated monitor/skip/fade predicate/tween/render gain and packed color"]
fn higurashi_title_demo_motion_and_render_policy_match_native() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for mode in 0..2 {
        for status in 0..4 {
            for permission in [false, true] {
                for captured in [false, true] {
                    for pressed in std::iter::once(0)
                        .chain((0..32).map(|bit| 1u32 << bit))
                        .chain([0x01000008])
                    {
                        protocol.push_str(&format!("N {mode}\n"));
                        let mut motion = Motion::new(mode);
                        motion.publish(true);
                        let mut cues = 0;
                        for (s, p, c, b, delta, percent) in [
                            (status, permission, captured, pressed, 1.0, 40),
                            (1, false, true, 0, 0.0, 37),
                            (1, true, false, 0, 89.0, 100),
                            (1, true, false, 0, 1.0, 255),
                            (2, false, true, 0, 3.0, 0),
                        ] {
                            protocol.push_str(&format!(
                                "U {s} {} {} {b} {delta} {percent}\n",
                                p as u32, c as u32
                            ));
                            if motion.poll(s, p, c, b, false).unwrap() {
                                cues += 1;
                            }
                            motion.update(Ticks::from_f32(delta));
                            let opacity = motion.opacity.value();
                            let packed =
                                0x00ffffff | ((opacity.max(0.0) * 255.0) as u8 as u32) << 24;
                            expected.push(vec![
                                opacity.to_bits(),
                                motion.opacity.is_waiting() as u32,
                                matches!(motion.phase, Phase::Fading | Phase::Queued) as u32,
                                (motion.phase == Phase::Queued) as u32,
                                cues,
                                motion.gain(percent).to_bits(),
                                opacity.to_bits(),
                                (motion.phase == Phase::Fading) as u32,
                                packed,
                            ]);
                        }
                        owners += 1;
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TITLE_DEMO").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (index, (line, words)) in text.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("D"));
        assert_eq!(
            fields
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native demo frame{index}"
        );
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Higurashi TitleDemo:{owners} owners/{} observations/{} exact words;monitor/90-tick queued fade/completion/input gate/audio gain/packed movie alpha pass",
        expected.len(),
        expected.len() * 9
    );
}

#[test]
fn higurashi_title_demo_selection_unlocks_and_absent_movie_follow_older_schema() {
    let mut data = SaveData::default();
    assert_eq!(mode(1, &data), 0);
    assert_eq!(mode(0, &data), 0);
    unlock(&mut data.save_vectors.unlocked_movies, 0);
    assert_eq!(mode(3, &data), 1);
    assert_eq!(mode(2, &data), 0);
    let d = Descriptor {
        index: 9,
        path: "unused".into(),
        bgm: 87,
        cg: 21,
    };
    d.record(&mut data);
    assert!(contains(&data.save_vectors.unlocked_movies, 9));
    assert!(contains(&data.save_vectors.unlocked_bgms, 87));
    assert!(contains(&data.save_vectors.unlocked_cgs, 21));
    assert_eq!(data.save_vectors.unlocked_movies.len(), 256);
    let bytes = data.encode().unwrap();
    d.record(&mut data);
    assert_eq!(data.encode().unwrap(), bytes);
    let mut motion = Motion::new(1);
    motion.publish(false);
    assert_eq!(
        motion.pending.take(),
        Some(TitleRequest {
            selected: -1,
            mode: 1
        })
    );
    assert!(!motion.poll(1, true, false, 1 << 24, true).unwrap());
    assert!(motion.pending.is_none());
}

#[test]
#[ignore = "actual updated opening MP4,FFmpeg,offscreen GPU at720p/1080p and exact Loopback Audio"]
fn higurashi_title_demo_plays_real_movie_and_retires_decoders() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    sounds.lock().set_settings_volume(40);
    let scenario: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut data = SaveData::default();
    let info = descriptor(&scenario, 0).unwrap();
    let original = data.encode().unwrap();
    let mut demo = TitleDemo::new(
        server.clone(),
        manager.clone(),
        sounds.clone(),
        &scenario,
        &data,
        0,
    );
    assert_eq!(
        data.encode().unwrap(),
        original,
        "constructor must await initializer before recording unlocks"
    );
    let mut id = FrameId::default();
    let mut number = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        id.advance();
        number += 1;
        demo.update(
            id,
            Ticks::ZERO,
            true,
            Buttons::default(),
            &PointerFrame {
                events: &[],
                number,
                time_seconds: 0.0,
            },
            &queue,
            &mut data,
        )
        .unwrap();
        if demo
            .player
            .as_ref()
            .is_some_and(|p| p.get_retained_frame().is_some())
        {
            break;
        }
        assert!(Instant::now() < deadline, "opening movie did not prepare");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(contains(&data.save_vectors.unlocked_movies, 0));
    if info.bgm >= 0 {
        assert!(contains(
            &data.save_vectors.unlocked_bgms,
            info.bgm as usize
        ));
    }
    if info.cg >= 0 {
        assert!(contains(&data.save_vectors.unlocked_cgs, info.cg as usize));
    }
    assert!(demo.take_title_request().is_none());
    assert!(!demo.player.as_ref().unwrap().is_paused());
    let mut frames = 0;
    for opacity in [1.0, 0.37] {
        demo.motion.opacity.fast_forward_to(opacity);
        for size in [(1280, 720), (1920, 1080)] {
            let scene = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi opening movie",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::WHITE),
                    Some(0),
                    Some(1.0),
                );
                demo.render(&mut pass, &data);
            });
            let direct = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi movie transport reference",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::WHITE),
                    Some(0),
                    Some(1.0),
                );
                let frame = demo.player.as_ref().unwrap().get_retained_frame().unwrap();
                let size = frame.get_frame().get_size();
                frame.render_with_options(
                    &mut pass,
                    RenderRequestBuilder::new().color_blend_type(if opacity < 1.0 {
                        ColorBlendType::Layer1
                    } else {
                        ColorBlendType::Opaque
                    }),
                    top_left_projection_matrix()
                        * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / size.y, 1.0)),
                    false,
                    Vec4::new(1.0, 1.0, 1.0, packed_alpha(opacity)),
                );
            });
            assert_eq!(scene, direct);
            assert!(scene.chunks_exact(4).any(|p| p != [255, 255, 255, 255]));
            frames += 1;
        }
    }
    demo.motion.opacity.fast_forward_to(1.0);
    id.advance();
    number += 1;
    demo.update(
        id,
        Ticks::from_f32(1.0),
        true,
        Buttons {
            pressed: 1 << 25,
            ..Default::default()
        },
        &PointerFrame {
            events: &[],
            number,
            time_seconds: 0.0,
        },
        &queue,
        &mut data,
    )
    .unwrap();
    assert!(
        demo.motion.phase == Phase::Playing,
        "native B does not skip this scene"
    );
    id.advance();
    number += 1;
    demo.update(
        id,
        Ticks::from_f32(1.0),
        true,
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
        &PointerFrame {
            events: &[],
            number,
            time_seconds: 0.0,
        },
        &queue,
        &mut data,
    )
    .unwrap();
    assert!(demo.motion.phase == Phase::Fading);
    assert!(!sounds.lock().sounds.is_empty());
    for delta in [89.0, 1.0] {
        id.advance();
        number += 1;
        demo.update(
            id,
            Ticks::from_f32(delta),
            true,
            Buttons::default(),
            &PointerFrame {
                events: &[],
                number,
                time_seconds: 0.0,
            },
            &queue,
            &mut data,
        )
        .unwrap();
    }
    assert_eq!(
        demo.take_title_request(),
        Some(TitleRequest {
            selected: -1,
            mode: 0
        })
    );
    let retirement = demo.loading.as_ref().unwrap().retirement.clone();
    drop(demo);
    assert!(retirement.is_finished());
    // A not-yet-published player must also retire before its initializer/result.
    let demo = TitleDemo::new(server, manager, sounds, &scenario, &data, 0);
    let retirement = demo.initial.as_ref().unwrap().retirement.clone();
    drop(demo);
    assert!(retirement.is_finished());
    eprintln!(
        "Higurashi TitleDemo integration:{frames} movie/reference pairs at720p/1080p;actual{},first-frame publication,metadata unlocks,B ignored,A/cue/90-tick fade/title(-1,0),complete/pending producer and decoder retirement pass",
        info.path
    );
}
