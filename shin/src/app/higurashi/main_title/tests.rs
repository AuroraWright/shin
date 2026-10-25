use super::*;
mod parent;
mod save_menu;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn higurashi_main_title_cached_mode_preserves_native_signed_arguments() {
    for prior in [0, 1, 2, 3, i32::MAX] {
        for argument in [0, 1, 2, 3, i32::MAX as u32, u32::MAX, 0x80000000] {
            let mut last = prior;
            let mode = resolve_mode(argument, &mut last);
            let expected = if (argument as i32) < 0 {
                prior
            } else {
                argument as i32
            };
            assert_eq!(last, expected);
            assert_eq!(mode, expected as u32 & 1);
        }
    }
}

#[test]
#[ignore = "actual updated First/readiness/overlay/title tweens/background and logo predicates/menu factory boundaries"]
fn higurashi_main_title_opening_matches_native() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for selected in [i32::MIN, -2, -1, 0, 9, 10, 11, i32::MAX] {
        for mode in 0..2 {
            for captured in [false, true] {
                for initial_status in 0..4 {
                    for pattern in [
                        [0.0, 3.0, 1.0, 0.125],
                        [30.0, 0.0, 60.0, 10.0],
                        [0.25, 3.0, 3.0, 1.0],
                    ] {
                        protocol.push_str(&format!("N {selected} {mode} {}\nB\n", captured as u32));
                        let mut model = Motion::new(selected, mode, captured);
                        model.clocks[4]
                            .control(1.0, Ticks::from_f32(90.0), 0)
                            .unwrap();
                        let (
                            mut starts,
                            mut bgm,
                            mut voices,
                            mut menus,
                            mut kind,
                            mut variant,
                            mut selection,
                        ) = (0, 0, 0, 0, 0, 0, 0);
                        for index in 0..104 {
                            let status = if index == 0 {
                                initial_status
                            } else if index < 4 {
                                [0, 1, 3][(index - 1) % 3]
                            } else {
                                2
                            };
                            let delta = pattern[index % 4];
                            protocol.push_str(&format!("U {status} {delta}\n"));
                            let actions = model.advance(status, Ticks::from_f32(delta)).unwrap();
                            if let Some(id) = actions.bgm {
                                starts += 1;
                                bgm = id as u32;
                            }
                            if actions.voice {
                                voices += 1;
                            }
                            if let Some(request) = model.pending.take() {
                                menus += 1;
                                match request {
                                    MenuRequest::Top { selected } => {
                                        kind = 1;
                                        variant = 0;
                                        selection = selected as u32;
                                    }
                                    MenuRequest::Extra {
                                        variant: v,
                                        selected,
                                    } => {
                                        kind = 2;
                                        variant = v;
                                        selection = selected as u32;
                                    }
                                }
                            }
                            let stage = match model.phase {
                                Phase::First => 0,
                                Phase::BackgroundGate => 1,
                                Phase::LogoGate => 2,
                                Phase::Menu => 3,
                            };
                            let mut words = vec![
                                model.first_phase as u32,
                                stage,
                                model.overlay.active as u32,
                                model.overlay.elapsed.to_bits(),
                                model.overlay.duration.to_bits(),
                                model.overlay.mask_present as u32,
                                starts,
                                bgm,
                                voices,
                                menus,
                                kind,
                                variant,
                                selection,
                            ];
                            for clock in &model.clocks {
                                words.extend([clock.value().to_bits(), clock.is_waiting() as u32]);
                            }
                            expected.push(words);
                        }
                        protocol.push_str("D\n");
                        owners += 1;
                    }
                }
            }
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MAIN_TITLE_OPENING").unwrap())
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
        assert_eq!(fields.next(), Some("O"));
        assert_eq!(
            fields
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native title opening frame{index}"
        );
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Higurashi main title opening:{owners} owners/{} observations/{} exact words;exact future2,30/60-tick First capture,100-tick background gate,30+10-tick logo gate,title call,mode0/1 BGM158/74,Top/Extra raw destinations and independent1-tick blackout pass",
        expected.len(),
        expected.len() * 23
    );
}

#[test]
#[ignore = "actual bootstrap handoff/title atlas/mask/BGM/title call and captured transition at720p/1080p;offscreen Metal and the default audio output"]
fn higurashi_main_title_opening_uses_real_handoff_assets_audio_and_capture() {
    use crate::{
        adv::higurashi::{
            input::Buttons, reference_gpu_assets as gpu_assets,
            reference_render_to_texture as render_to_texture,
        },
        app::higurashi::{load_game::Paths, scene_host::SceneHost, title_demo::TitleRequest},
        audio::SystemSoundPlayer,
    };
    use parking_lot::Mutex;
    use shin_core::primitives::{color::UnormColor, update::FrameId};
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut host = SceneHost::new(
        server.clone(),
        manager.clone(),
        sounds.clone(),
        Arc::new(Paths {
            current: None,
            trial: None,
        }),
    );
    let mut id = FrameId::default();
    let mut number = 0;
    let transition = loop {
        id.advance();
        number += 1;
        let result = host
            .update(
                id,
                Ticks::from_f32(3.0),
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &crate::adv::input_area::PointerFrame {
                    events: &[],
                    number,
                    time_seconds: number as f64 / 60.0,
                },
                &queue,
            )
            .unwrap();
        if let Some(transition) = result {
            break transition;
        }
        assert!(
            Instant::now() < deadline,
            "startup/movie did not yield real title factory"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(
        transition.request,
        TitleRequest {
            selected: -1,
            mode: 0
        }
    );
    let common = transition.globals.common.clone();
    let clock = transition.globals.clock.ticks();
    let mut capture = None;
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        // First samples a separately retained previous frame. Use opaque black,
        // matching the movie's fully faded final composed frame.
        let resize = shin_render::resize::SurfaceResizeSource::new(
            shin_render::resize::ViewportParams::both(winit::dpi::PhysicalSize::new(1280, 720)),
        );
        let mut snapshot = RenderTexture::new(
            device.clone(),
            resize.handle(),
            "Higurashi title previous frame".into(),
        );
        let mut pass = context.begin_pass(
            snapshot.as_texture_target(),
            None,
            "Higurashi title source capture",
        );
        pass.clear(Some(UnormColor::BLACK), None, None);
        drop(pass);
        capture = Some(snapshot);
    });
    let mut opening = Opening::new(transition, capture);
    assert!(opening.atlas.is_none());
    while !opening.ready {
        opening.update(Ticks::ZERO).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let atlas = opening.atlas.as_ref().unwrap();
    for name in ["title_bg", "credit", "logo", "logo_plus", "light1"] {
        assert!(atlas.contains(name), "actual title atlas lacks{name}");
    }
    assert!(opening.mask.is_some());
    assert_eq!(opening.motion.overlay.duration, 60.0);
    assert_eq!(opening.frame().background_offset, -200.0);
    assert!(Arc::ptr_eq(&common, &opening.globals.common));
    assert!(Arc::ptr_eq(&sounds, &opening.globals.sounds));
    assert_eq!(
        opening.globals.clock.ticks(),
        clock,
        "root host owns elapsed play clock"
    );
    assert_eq!(opening.globals.last_title_mode, 0);
    assert!(opening.take_menu_request().is_none());
    assert_ne!(
        opening.globals.data.save_vectors.unlocked_bgms[158 / 32] & (1 << (158 % 32)),
        0
    );
    let retire = opening.job.as_ref().unwrap().retirement.clone();
    let mut frames = 0;
    for (elapsed, expected) in [(0.0, 0u8), (30.0, 128), (60.0, 255)] {
        opening.motion.overlay.elapsed = elapsed;
        opening.overlay.as_mut().unwrap().clock = opening.motion.overlay.clone();
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    None,
                    "Higurashi title First captured fade",
                );
                pass.clear(Some(UnormColor::WHITE), None, None);
                opening.render_transition(&mut pass);
            });
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|p| p[..3].iter().all(|&c| c.abs_diff(expected) <= 1) && p[3] == 255)
            );
            frames += 1;
        }
    }
    // Resume the actual motion,including the check-before-update boundary.
    opening.motion.overlay.elapsed = 0.0;
    opening.update(Ticks::from_f32(60.0)).unwrap();
    assert!(opening.take_menu_request().is_none());
    opening.update(Ticks::from_f32(40.0)).unwrap();
    assert!(opening.take_menu_request().is_none());
    opening.update(Ticks::ZERO).unwrap();
    assert!(opening.overlay.is_none() && opening.mask.is_none() && opening.job.is_none());
    assert!(retire.is_finished());
    assert_eq!(opening.frame().logo_alpha, 0.0);
    opening.update(Ticks::from_f32(30.0)).unwrap();
    opening.update(Ticks::from_f32(10.0)).unwrap();
    assert_eq!(opening.frame().logo_alpha, 1.0);
    assert_eq!(opening.frame().logo_offset, 0.0);
    assert!(opening.take_menu_request().is_none());
    opening.update(Ticks::ZERO).unwrap();
    assert_eq!(
        opening.take_menu_request(),
        Some(MenuRequest::Top { selected: -1 })
    );
    assert!(opening.take_menu_request().is_none());
    while opening.globals.audio.status_bits() & 1 != 0
        || opening.globals.audio.bgm_status() & 1 != 0
    {
        opening.update(Ticks::ZERO).unwrap();
        assert!(Instant::now() < deadline, "title audio did not prepare");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_ne!(
        opening.globals.audio.status_bits() & 2,
        0,
        "actual S00/title_call voice did not play"
    );
    assert_ne!(
        opening.globals.audio.bgm_status() & 2,
        0,
        "actual title BGM158 did not play"
    );
    assert!(opening.take_menu_request().is_none());
    // Return route retains the global audio owner and must not restart same-ID BGM.
    opening.globals.last_title_mode = 2;
    let globals = opening.into_globals();
    let mut returned = Opening::new(
        TitleTransition {
            request: TitleRequest {
                selected: 11,
                mode: u32::MAX,
            },
            globals,
        },
        None,
    );
    assert_eq!(returned.globals.last_title_mode, 2);
    assert_eq!(returned.motion.mode, 0);
    while !returned.ready {
        returned.update(Ticks::ZERO).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(returned.motion.overlay.mask_present);
    assert_eq!(returned.motion.overlay.duration, 30.0);
    assert_ne!(returned.globals.audio.bgm_status() & 2, 0);
    returned.update(Ticks::from_f32(30.0)).unwrap();
    assert!(returned.take_menu_request().is_none());
    for _ in 0..3 {
        returned.update(Ticks::ZERO).unwrap();
    }
    assert_eq!(
        returned.take_menu_request(),
        Some(MenuRequest::Extra {
            variant: 1,
            selected: 1
        })
    );
    let mut pending = Opening::new(
        TitleTransition {
            request: TitleRequest {
                selected: 0,
                mode: 0,
            },
            globals: returned.into_globals(),
        },
        None,
    );
    let retirement = pending.job.take().unwrap().retirement.clone();
    drop(pending);
    assert!(retirement.is_finished());
    eprintln!(
        "Higurashi main title opening integration:actual startup/movie handoff,title atlas/default mask,BGM158/title call,global audio/clock/font retention,Top(-1)/Extra(1,1),pending/completed worker retirement and{frames}720p/1080p captured-fade frames pass"
    );
}

#[test]
fn patchless_original_snr_uses_native_no_append_save_gate_without_blocking_load() {
    use shin_core::format::scenario::higurashi::HigurashiVariant::{Original, Updated};
    assert!(!append_story_available(false, Original));
    assert!(!append_story_available(true, Original));
    assert!(!append_story_available(false, Updated));
    assert!(append_story_available(true, Updated));
}

#[test]
#[ignore = "actual mounted base-only/base+append title atlas fallback drawing; offscreen GPU, no audio"]
fn patchless_title_actual_base_atlas_keeps_common_top_category_and_quiz_artwork() {
    use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo};
    let (device, queue, _) = reference_gpu_assets();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    for append in [false, true] {
        let mut io = LayeredAssetIo::new();
        if append {
            io.try_with_rom(root.join("game-data/higurashi/append.rom")).unwrap();
        }
        io.try_with_rom(root.join("game-data/higurashi/data.rom")).unwrap();
        let server = AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Higurashi,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        let atlas = server.load_sync::<CharacterTextures>("/title.txa").unwrap();
        assert!(!atlas.contains("o_menu"));
        assert!(!atlas.contains("logo_plus"));
        assert!(!atlas.contains("s_menu_plus"));
        assert!(!atlas.contains("s_menu_lock_plus"));
        assert!(!atlas.contains("q_menu_plus"));
        assert_eq!(atlas.get("t_menu").size().height, 1040);
        for name in ["logo", "s_menu", "s_menu_lock", "q_menu"] {
            assert!(atlas.contains(name));
        }
        let top = top_menu::TopMenu::new(false, false, 8, (1920, 1080));
        let cat = cat_menu::CatMenu::new(0, 1, 0, (1920, 1080));
        let gallery = cat_menu::CatMenu::new(1, 0, 5, (1920, 1080));
        let quiz = quiz_menu::QuizMenu::new((1920, 1080));
        for size in [(1280, 720), (1920, 1080)] {
            reference_render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    None,
                    "Patchless actual title atlas fallbacks",
                );
                top.render(&mut pass, &atlas);
                cat.render(&mut pass, &atlas, append);
                gallery.render(&mut pass, &atlas, append);
                quiz.render(&mut pass, &atlas, append);
            });
        }
    }
}
