use super::super::super::Gameplay;
use super::*;
use crate::{
    adv::{
        higurashi::{
            audio::HigurashiAudio, input::Buttons, music_commands::MusicCommands,
            reference_gpu_assets, reference_render_to_texture,
        },
        input_area::{PointerEvent, PointerFrame, PointerKind},
        play_clock::PlayClock,
    },
    app::higurashi::{first_boot::CommonAssets, scene_host::Globals},
    audio::SystemSoundPlayer,
};
use parking_lot::Mutex;
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::time::{Duration, Instant};
#[test]
#[ignore = "actual authored BGM->Gameplay event consumption->title rendering/settings at720p1080p; offscreen Metal/the default audio output, no window or user saves"]
fn higurashi_gameplay_music_title_uses_live_bgm_events_and_setting() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut code = authored.raw()[0x2545e..0x25465].to_vec();
    code.extend([0x86, 1, 0, 0, 1, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let at = authored.header.code_offset as usize;
    raw[at..at + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let common = Arc::new(CommonAssets {
        scenario,
        font: server.load_sync("/newrodin.fnt").unwrap(),
        system: server.load_sync("/systex.txa").unwrap(),
        message: server.load_sync("/msgtex.txa").unwrap(),
        effects: server.load_sync("/effects.txa").unwrap(),
        sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    for size in [(1280, 720), (1920, 1080)] {
        let mut saved = SaveData::default();
        saved.settings.common.v15 = false; // Umineko field deliberately opposes Higurashi.
        saved.settings.common.v18_showsongtitle = true;
        let globals = Globals {
            storage: None,
            runtime_saves: None,
            common: common.clone(),
            data: saved,
            clock: PlayClock::from_saved_seconds(0),
            platform_flag: true,
            last_title_mode: 0,
            audio: HigurashiAudio::new(manager.clone(), server.clone(), authored.clone()),
            server: server.clone(),
            manager: manager.clone(),
            sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone()))),
        };
        let mut game = None;
        reference_render_to_texture(&device, &queue, size, |ctx, _| {
            let mut initial = ctx.new_render_texture("Music title regression initial".into());
            {
                let mut pass = ctx.begin_pass(
                    initial.as_texture_target(),
                    Some(ctx.depth_stencil),
                    "Music title initial",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            }
            game = Some(
                Gameplay::new(
                    globals,
                    super::super::super::tests::fresh(),
                    0,
                    initial,
                    || [1, 2, 3, 4],
                )
                .unwrap(),
            );
        });
        let mut game = game.unwrap();
        let mut frame = 0;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            frame += 1;
            shin_tasks::block_on(game.update(
                Ticks::from_f32(1.0),
                true,
                Buttons::default(),
                &PointerFrame {
                    events: &[],
                    number: frame,
                    time_seconds: frame as f64 / 60.0,
                },
                super::super::super::tests::policy(),
                super::super::super::tests::audio(),
            ))
            .unwrap();
            if game.music_title.title.is_some()
                && game.snapshot.is_none()
                && !game.host().unwrap().has_modes()
                && game.host().unwrap().ui.menu.indicator.transition.value() >= 20.0
                && game.host().unwrap().ui.menu.indicator.opacity >= 0.9
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "BGM title did not arrive through live Gameplay"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let h = game.host().unwrap();
        assert!(h.ui.menu.indicator.transition.is_waiting());
        assert!(game.host_mut().unwrap().audio.pop_music_event().is_none());
        assert_ne!(
            game.globals.data.save_vectors.unlocked_bgms[114 / 32] & (1 << (114 % 32)),
            0
        );
        let capture = |game: &mut Gameplay| {
            reference_render_to_texture(&device, &queue, size, |ctx, view| {
                game.prepare(ctx).unwrap();
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        view,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(ctx.depth_stencil),
                    "Music title live frame",
                );
                game.render(&mut pass).unwrap();
            })
        };
        // Exercise the real Config InputArea, widget binding and owner publication.
        // The earlier test wrote v15 directly and therefore missed this regression.
        let set_title = |game: &mut Gameplay, enabled: bool| {
            use crate::app::higurashi::{config::owner::Owner, save_menu::Environment};
            let mut config = Owner::new(
                Environment {
                    common: common.clone(),
                    server: server.clone(),
                    sounds: game.globals.sounds.clone(),
                    storage: None,
                },
                &game.globals.data,
                0,
                false,
                (size.0 as u16, size.1 as u16),
            )
            .unwrap();
            let mut number = 0;
            let mut poll = |config: &mut Owner, events: &[PointerEvent]| {
                number += 1;
                config
                    .poll(
                        &mut game.globals.data,
                        None,
                        &mut false,
                        Ticks::from_f32(1.0),
                        true,
                        Buttons::default(),
                        &PointerFrame {
                            events,
                            number,
                            time_seconds: number as f64 / 60.0,
                        },
                    )
                    .unwrap();
            };
            let deadline = Instant::now() + Duration::from_secs(15);
            while !config.reference_input_ready() {
                poll(&mut config, &[]);
                assert!(Instant::now() < deadline, "Config did not open");
                std::thread::sleep(Duration::from_millis(1));
            }
            let event = |kind, x: f32, y: f32| PointerEvent {
                kind,
                id: 1,
                position: glam::IVec2::new(
                    (x * size.0 as f32 / 1920.0).round() as i32,
                    (y * size.1 as f32 / 1080.0).round() as i32,
                ),
            };
            // Scroll the actual settings list to its song-title row by contact.
            for kind in [PointerKind::Down, PointerKind::Move, PointerKind::Cancel] {
                let y = if kind == PointerKind::Down {
                    900.0
                } else {
                    250.0
                };
                poll(&mut config, &[event(kind, 500.0, y)]);
            }
            while !config.reference_input_ready() {
                poll(&mut config, &[]);
                assert!(Instant::now() < deadline, "Config scroll did not settle");
            }
            let state = config.state();
            let row = state.rows.iter().find(|r| r.title_band == 16).unwrap();
            let y = row.y as f32 - state.scroll + 260.0;
            assert!((210.0..980.0).contains(&y), "Song-title row offscreen: {y}");
            let x = if enabled { 1600.0 } else { 1350.0 };
            for kind in [PointerKind::Down, PointerKind::Up] {
                poll(&mut config, &[event(kind, x, y)]);
            }
            assert_eq!(game.globals.data.settings.common.v18_showsongtitle, enabled);
        };
        game.globals.data.settings.common.v15 = true;
        set_title(&mut game, false);
        assert!(game.globals.data.settings.common.v15);
        let off = capture(&mut game);
        set_title(&mut game, true);
        assert!(game.globals.data.settings.common.v15);
        let mut on;
        loop {
            on = capture(&mut game);
            let changed = on
                .chunks_exact(4)
                .zip(off.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            if changed > 50 {
                break;
            }
            assert!(Instant::now() < deadline, "No live song-title pixels");
            std::thread::sleep(Duration::from_millis(2));
        }
        game.globals.data.settings.common.v15 = false;
        assert_eq!(
            on,
            capture(&mut game),
            "Umineko v15 must not affect Higurashi title"
        );
        game.globals.data.settings.common.v15 = true;
        set_title(&mut game, false);
        assert_eq!(
            off,
            capture(&mut game),
            "Config off removes the title again"
        );
        let height = 100 * size.1 as usize / 1080;
        assert_eq!(
            &on[height * size.0 as usize * 4..],
            &off[height * size.0 as usize * 4..],
            "Setting affects only top-left music overlay"
        );
        let before = game.host().unwrap().ui.menu.indicator.transition.value();
        game.host_mut()
            .unwrap()
            .audio
            .play_bgm(114, 0, 0, 1.0)
            .unwrap();
        assert!(
            game.host_mut().unwrap().audio.pop_music_event().is_none(),
            "Same BGM must not restart label"
        );
        assert_eq!(
            game.host().unwrap().ui.menu.indicator.transition.value(),
            before
        );
        game.host_mut().unwrap().audio.stop_bgm(0);
        let mut clock = game.host().unwrap().ui.menu.clone();
        game.music_title
            .consume(
                &mut game.host.as_mut().unwrap().audio,
                &mut clock,
                &mut game.globals.data,
                &common.font,
                server.context(),
            )
            .unwrap();
        clock.advance(Ticks::from_f32(1000.0));
        assert!(clock.indicator.transition.is_idle());
        println!(
            "Live BGM title {}x{}:real authored BGM event consumed,top-left pixels present,actual Config contact off/on/off with fixed v15 and independent v15 invariance,same-ID retention,stop/expiry,unlocks,zero storage writes",
            size.0, size.1
        );
    }
}
