//! Actual updated picker input, native final callback boundary and retained host.
use super::*;
impl SceneHost {
    pub(crate) fn reference_scenario_unlock_roundtrip<'a>(
        server: &'a Arc<AssetServer>,
        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        target: &wgpu::Texture,
    ) {
        use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
        let folder =
            std::env::temp_dir().join(format!("shin-updated-unlock-host-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("progress.dat");
        let mut save = SaveData::default();
        save.persist_data.set(0, -5);
        save.persist_data.set(1, 3);
        save.persist_data.set(2, -1);
        save.persist_data.set(9, 2);
        save.persist_data.set(50, 32767);
        save.settings.v0_bgmvol = 31;
        save.settings.v3_sysvol = 45;
        save.play_seconds = 17;
        let original = save.encode();
        std::fs::write(&path, &original).unwrap();
        let globals = GlobalData::open(Some(path.clone()), None).unwrap();
        let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
        let manager = audio.manager.clone();
        let system_track = audio.system_player.track.id();
        let bgm_track = audio.bgm_player.reference_source().2;
        let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
        let previous = ctx.new_render_texture("Updated picker smoke previous screen".into());
        let scene = ScenarioSelectScene::new(server.clone(), &save, previous, ctx, (1920, 1080));
        let mut host = Self {
            scene: Some(Scene::Picker(PickerRuntime {
                scene,
                globals,
                audio,
                extent: (1920, 1080),
            })),
            pending: None,
            assets: Some(assets),
            common: None,
            server: server.clone(),
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        };
        let mut frame = FrameId::default();
        let mut step = |host: &mut Self,
                        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
                        delta: f32,
                        actions: &[AppAction]| {
            let mut input = EnumMap::default();
            for &action in actions {
                input[action] = ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            frame.advance();
            host.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    pre_render: ctx,
                },
                input,
                glam::vec2(-100.0, -100.0),
                glam::vec2(-100.0, -100.0),
                (1920, 1080),
                0.0,
                delta as f64 / 60.0,
            )
            .unwrap();
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.reference_picker().unwrap().is_loaded() {
            assert!(std::time::Instant::now() < deadline);
            step(&mut host, ctx, 0.0, &[]);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        step(&mut host, ctx, 60.0, &[]);
        step(&mut host, ctx, 0.0, &[]);
        step(&mut host, ctx, 0.0, &[AppAction::AnyDown]);
        let old_selection = host
            .reference_picker()
            .unwrap()
            .reference_groups_and_selection();
        assert_eq!(old_selection, (vec![0, 1], (0, 1)));
        fn pixels(
            host: &SceneHost,
            ctx: &mut crate::render::PreRenderContext,
            target: &wgpu::Texture,
        ) -> Vec<u8> {
            let view = target.create_view(&Default::default());
            {
                let mut pass = ctx.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(ctx.depth_stencil),
                    "Updated picker footer",
                );
                host.render(&mut pass);
            }
            crate::layer::user::readback_test_texture(ctx, target)
        }
        let before = pixels(&host, ctx, target);
        // Native dialog defaults to No. Neither the intermediate callback nor
        // its final No may change globals or queue a replacement.
        step(&mut host, ctx, 0.0, &[AppAction::UnlockScenarios]);
        assert!(!host.reference_picker().unwrap().accepts_input());
        for _ in 0..24 {
            step(&mut host, ctx, 1.0, &[]);
        }
        step(&mut host, ctx, 1.0, &[AppAction::Enter]);
        for _ in 0..35 {
            step(&mut host, ctx, 1.0, &[]);
            assert!(host.pending.is_none());
        }
        assert!(host.reference_picker().unwrap().accepts_input());
        assert_eq!(host.reference_save().persist_data.0, save.persist_data.0);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        step(&mut host, ctx, 0.0, &[AppAction::UnlockScenarios]);
        for _ in 0..24 {
            step(&mut host, ctx, 1.0, &[]);
        }
        // Render the real warning text, including the native @z75 second line.
        pixels(&host, ctx, target);
        step(&mut host, ctx, 0.0, &[AppAction::Left]);
        step(&mut host, ctx, 1.0, &[AppAction::Enter]);
        assert!(host.pending.is_none());
        for _ in 0..40 {
            step(&mut host, ctx, 1.0, &[]);
            if host.pending.is_some() {
                break;
            }
            assert_eq!(host.reference_save().persist_data.0, save.persist_data.0);
        }
        assert!(matches!(host.pending, Some(Destination::PickerRefresh)));
        assert_eq!(
            host.reference_picker()
                .unwrap()
                .reference_groups_and_selection(),
            old_selection
        );
        let unlocked = host.reference_save();
        assert_eq!(unlocked.persist_data.get(0), 24);
        for slot in 1..=25 {
            assert_eq!(
                unlocked.persist_data.get(slot),
                if save.persist_data.get(slot) == 0 {
                    1
                } else {
                    save.persist_data.get(slot)
                }
            );
        }
        assert_eq!(unlocked.persist_data.get(50), 32767);
        assert_eq!(std::fs::read(&path).unwrap(), original); // no immediate SaveGame
        let ticks = host.reference_play_ticks();
        step(&mut host, ctx, 0.0, &[]); // apply queued native factory next frame
        assert_eq!(host.reference_play_ticks(), ticks);
        assert_eq!(
            host.reference_picker()
                .unwrap()
                .reference_groups_and_selection(),
            (vec![0, 1, 2, 3], (0, 0))
        );
        assert!(!host.reference_picker().unwrap().accepts_input());
        assert!(Arc::ptr_eq(&manager, &host.reference_audio().manager));
        assert_eq!(
            host.reference_audio().system_player.track.id(),
            system_track
        );
        assert_eq!(
            host.reference_audio().bgm_player.reference_source().2,
            bgm_track
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        while !host.reference_picker().unwrap().is_loaded() {
            assert!(std::time::Instant::now() < deadline);
            step(&mut host, ctx, 0.0, &[]);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        step(&mut host, ctx, 60.0, &[]);
        step(&mut host, ctx, 0.0, &[]);
        let after = pixels(&host, ctx, target);
        std::fs::write("/tmp/shin-updated-unlock-before.rgba", &before).unwrap();
        std::fs::write("/tmp/shin-updated-unlock-after.rgba", &after).unwrap();
        // Native helper places each icon at label-left +2 -48; the overlap
        // contributes two pixels per preceding row. With >=2 groups, the
        // fifth label/icon occupies856..1036 rather than a nonoverlap layout.
        let mut changed = 0;
        for y in 1012..1060 {
            for x in 0..1920 {
                let off = (y * 1920 + x) * 4;
                if before[off..off + 4] != after[off..off + 4] {
                    assert!((856..1036).contains(&x), "Unrelated footer pixel {x},{y}");
                    changed += 1;
                }
            }
        }
        assert!(changed > 100);
        step(&mut host, ctx, 0.0, &[AppAction::UnlockScenarios]);
        assert!(host.reference_picker().unwrap().accepts_input());
        assert!(host.pending.is_none());
        // Real mouse Yes/No on the scenario-start dialog at non-unit pixel scales.
        // The host receives physical pixels for picker InputArea and logical canvas
        // coordinates for the generic dialog; swapping these breaks both clicks.
        for (button, extent) in [(1, (3840u16, 2160u16)), (0, (1280, 720))] {
            step(&mut host, ctx, 7.0, &[AppAction::Enter]);
            step(&mut host, ctx, 0.0, &[]);
            for _ in 0..24 {
                step(&mut host, ctx, 1.0, &[]);
            }
            if button == 1 {
                // Native release inertia depends on this frame's elapsed seconds,
                // not PointerFrame's accumulated uptime. Make that distinction large.
                let mut pointer = |host: &mut Self, point: glam::Vec2, held: bool, seconds: f64| {
                    let mut input = EnumMap::<AppAction, ActionState>::default();
                    input[AppAction::PointerClick].is_held = held;
                    host.update(
                        &mut UpdateContext {
                            frame_started_at: std::time::Instant::now(),
                            frame_id: FrameId::default(),
                            delta_ticks: Ticks::ZERO,
                            asset_server: server,
                            pre_render: ctx,
                        },
                        input,
                        point,
                        point,
                        (1920, 1080),
                        0.0,
                        seconds,
                    )
                    .unwrap();
                };
                pointer(&mut host, glam::vec2(-100.0, -100.0), false, 3600.0);
                let origin = host.reference_picker().unwrap().reference_dialog_origin();
                let press = origin + glam::vec2(20.0, 20.0);
                pointer(&mut host, press, true, 1.0 / 60.0);
                pointer(&mut host, press + glam::vec2(40.0, 0.0), true, 1.0 / 60.0);
                pointer(&mut host, press + glam::vec2(40.0, 0.0), false, 1.0 / 60.0);
                let after = host.reference_picker().unwrap().reference_dialog_origin();
                assert!(
                    (after.x - origin.x - 80.04).abs() < 0.01,
                    "Picker drag release should retain native frame-based inertia: {origin:?} -> {after:?}"
                );
                assert!(host.pending.is_none());
                // A new window press stops the fling, preserving stable hit coordinates.
                let stop = after + glam::vec2(20.0, 20.0);
                pointer(&mut host, stop, true, 1.0 / 60.0);
                pointer(&mut host, stop, false, 1.0 / 60.0);
            }
            let canvas = host
                .reference_picker()
                .unwrap()
                .reference_dialog_button_center(button);
            let physical = canvas * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
            // Resize while idle before the press; native input suppresses held
            // presses across a resize to avoid accidental confirmation.
            for held in [false, true, false] {
                let mut input = EnumMap::default();
                input[AppAction::PointerClick] = ActionState {
                    is_held: held,
                    is_clicked: held,
                    is_clicked_or_repeated: held,
                    ..Default::default()
                };
                host.update(
                    &mut UpdateContext {
                        frame_started_at: std::time::Instant::now(),
                        frame_id: FrameId::default(),
                        delta_ticks: Ticks::ZERO,
                        asset_server: server,
                        pre_render: ctx,
                    },
                    input,
                    physical,
                    canvas,
                    extent,
                    0.0,
                    1.0 / 60.0,
                )
                .unwrap();
            }
            assert!(
                host.pending.is_none(),
                "Mouse result must await final callback"
            );
            for _ in 0..35 {
                step(&mut host, ctx, 1.0, &[]);
                if host.pending.is_some() {
                    break;
                }
            }
            if button == 1 {
                assert!(host.pending.is_none());
                assert!(host.reference_picker().unwrap().accepts_input());
            } else {
                assert!(matches!(host.pending, Some(Destination::Episode(0))));
            }
        }
        println!(
            "Actual picker scenario-start dialog: mouse No at3840x2160 and Yes at1280x720 pass through host canvas mapping/final callback"
        );
        let final_save = host.reference_save();
        drop(host);
        let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk.persist_data.0, final_save.persist_data.0);
        assert_eq!(disk.play_seconds, final_save.play_seconds);
        assert_eq!(disk.settings.v0_bgmvol, 31);
        assert_eq!(disk.settings.v3_sysvol, 45);
        std::fs::remove_dir_all(folder).unwrap();
        println!(
            "Updated live picker on Loopback: default No unchanged, final Yes zero-only writes, deferred fresh picker/selection reset/four groups, retained global clock/audio tracks, {changed} removed hint pixels confined to native footer box, disabled repeat shortcut and shutdown save roundtrip pass"
        );
    }
}
