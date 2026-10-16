use super::*;

// Exercise actual SceneHost factory/error/input/title handoff with supplied assets.
pub(in super::super) fn verify_scene_load_errors<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    original: &Arc<Scenario>,
    target: &wgpu::Texture,
) {
    use crate::adv::global_data::GlobalData;
    use crate::{
        app::{AppAction, SceneHost},
        audio::GlobalAudio,
    };
    use glam::Vec2;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    fn tick<'a>(
        host: &mut SceneHost,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        action: Option<AppAction>,
        pointer: Vec2,
        held: bool,
        extent: (u16, u16),
    ) {
        let mut input = enum_map::EnumMap::default();
        if let Some(action) = action {
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            pointer * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
            pointer,
            extent,
            0.0,
            1.0 / 60.0,
        )
        .unwrap();
        let encoder = std::mem::replace(
            ctx.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        let mut upload = ctx.device.create_command_encoder(&Default::default());
        ctx.dynamic_buffer.finish(&mut upload);
        ctx.queue.submit([upload.finish(), encoder.finish()]);
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    fn image(host: &SceneHost, ctx: &mut PreRenderContext, target: &wgpu::Texture) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(ctx.depth_stencil),
                "Scene load error notice",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    let scenario = source(original, true, None);
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let game = |position| {
        GameData::from_entry(GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: position,
            route_data: RouteRecorder::default().snapshot(),
        })
    };
    for (name, title, synchronous, mouse, resume) in [
        ("title-async-keyboard", true, false, false, false),
        ("title-invalid-position-mouse", true, true, true, false),
        ("live-async-mouse", false, false, true, false),
        ("live-resume-async-keyboard", false, false, false, true),
    ] {
        let directory =
            std::env::temp_dir().join(format!("shin-scene-error-{}-{name}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("progress.dat");
        let mut saved = SaveData::default();
        saved.save_menu_position = 99;
        saved.manual_save_slots[99] = Some(game(1));
        saved.auto_save_slot = Some(game(1));
        saved.play_seconds = 123;
        saved.settings.v3_sysvol = 0;
        saved.persist_data.set(28, 456);
        let saved = SaveData::decode(&saved.encode()).unwrap();
        std::fs::write(&path, saved.encode()).unwrap();
        let extent = if mouse { (3840, 2160) } else { (1280, 720) };
        let mut frame = FrameId::default();
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&scenario, 0, 42),
        );
        if !title {
            live.install_global_data(GlobalData::open(Some(path.clone()), None).unwrap());
            live.load_game_data(&saved, &game(1), server.clone())
                .unwrap();
            finish(&mut live, context, server, &mut frame);
            live.adv_state.acquire_audio_pause();
            live.adv_state.acquire_audio_pause();
            live.sample_modal_pointer(Vec2::ZERO, extent, true, 1.0 / 60.0);
            if resume {
                live.vm_state.resume_position = Some(4);
                live.queue_resume_scene(&mut UpdateContext {
                    frame_started_at: Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                });
                assert_eq!(
                    live.pending_restore.as_ref().unwrap().kind,
                    crate::adv::SceneLoadKind::Resume
                );
            } else {
                live.load_game_data(&saved, &game(4), server.clone())
                    .unwrap();
            }
        }
        let mut host = SceneHost::new(live, assets.clone(), server.clone());
        if title {
            host.reference_enter_saved_scene(
                GlobalData::open(Some(path.clone()), None).unwrap(),
                GlobalAudio::new(audio.clone()),
                game(if synchronous { u32::MAX } else { 4 }),
            )
            .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while host.reference_adv().unwrap().restore_error_dialog.is_none() {
            assert!(
                Instant::now() < deadline,
                "{name}: failure was not reported"
            );
            tick(
                &mut host,
                context,
                server,
                &mut frame,
                None,
                Vec2::ZERO,
                true,
                extent,
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let failure = host
            .reference_adv()
            .unwrap()
            .saved_scene_load_error()
            .unwrap()
            .to_owned();
        if !synchronous {
            assert!(
                failure.contains('!'),
                "{name}: expected missing modified mask: {failure}"
            );
        }
        let pc = host.reference_adv().unwrap().scripter.position();
        let position = host.reference_adv().unwrap().vm_state.save_position;
        let route = host.reference_adv().unwrap().vm_state.route.bit_position();
        let command = format!("{:?}", host.reference_adv().unwrap().current_command);
        let disk = std::fs::read(&path).unwrap();
        let baseline = host.reference_save();
        let pauses = if title { 0 } else { 2 };
        for _ in 0..40 {
            tick(
                &mut host,
                context,
                server,
                &mut frame,
                None,
                Vec2::ZERO,
                true,
                extent,
            );
            let live = host.reference_adv().unwrap();
            assert_eq!(
                (
                    live.scripter.position(),
                    live.vm_state.save_position,
                    live.vm_state.route.bit_position()
                ),
                (pc, position, route)
            );
            assert_eq!(live.adv_state.audio_pause_count, pauses + 1);
            assert!(live.pending_title_setup().is_none());
        }
        let with_notice = image(&host, context, target);
        let dialog = host
            .reference_adv_mut()
            .unwrap()
            .restore_error_dialog
            .take()
            .unwrap();
        let without_notice = image(&host, context, target);
        host.reference_adv_mut().unwrap().restore_error_dialog = Some(dialog);
        let changed = with_notice
            .chunks_exact(4)
            .zip(without_notice.chunks_exact(4))
            .filter(|(a, b)| a[..3] != b[..3])
            .count();
        let glyphs = (400..700)
            .flat_map(|y| (400..1520).map(move |x| (y * 1920 + x) * 4))
            .filter(|&i| {
                with_notice[i..i + 3].iter().all(|&v| v > 245)
                    && with_notice[i..i + 3] != without_notice[i..i + 3]
            })
            .count();
        assert!(
            changed > 8000 && glyphs > 100,
            "{name}: notice must render, changed{changed}, glyphs{glyphs}"
        );
        let button = host
            .reference_adv()
            .unwrap()
            .restore_error_dialog
            .as_ref()
            .unwrap()
            .reference_button();
        // Inherited release+Enter and Cancel must not dismiss an acknowledgement.
        tick(
            &mut host,
            context,
            server,
            &mut frame,
            Some(AppAction::Enter),
            button,
            false,
            extent,
        );
        tick(
            &mut host,
            context,
            server,
            &mut frame,
            Some(AppAction::Cancel),
            button,
            false,
            extent,
        );
        assert!(host.reference_adv().unwrap().restore_error_dialog.is_some());
        if mouse {
            tick(
                &mut host, context, server, &mut frame, None, button, true, extent,
            );
            tick(
                &mut host, context, server, &mut frame, None, button, false, extent,
            );
        } else {
            tick(
                &mut host,
                context,
                server,
                &mut frame,
                Some(AppAction::Enter),
                button,
                false,
                extent,
            );
        }
        for _ in 0..100 {
            let live = host.reference_adv().unwrap();
            assert_eq!(
                (
                    live.scripter.position(),
                    live.vm_state.save_position,
                    live.vm_state.route.bit_position()
                ),
                (pc, position, route)
            );
            assert_eq!(format!("{:?}", live.current_command), command);
            if live.restore_error_dialog.is_none() {
                break;
            }
            tick(
                &mut host, context, server, &mut frame, None, button, false, extent,
            );
        }
        assert!(
            host.reference_adv().unwrap().restore_error_dialog.is_none(),
            "{name}: acknowledgement did not close"
        );
        assert_eq!(
            host.reference_adv().unwrap().adv_state.audio_pause_count,
            pauses
        );
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        let latest = host.reference_save();
        assert_eq!(latest.save_menu_position, baseline.save_menu_position);
        assert_eq!(
            format!("{:?}", latest.manual_save_slots[99]),
            format!("{:?}", baseline.manual_save_slots[99])
        );
        assert_eq!(latest.persist_data.get(28), baseline.persist_data.get(28));
        if title {
            // The final acknowledgement input was consumed by ADV. The queued
            // handoff applies on the next frame and preserves global ownership.
            tick(
                &mut host, context, server, &mut frame, None, button, false, extent,
            );
            assert!(host.reference_adv().is_none());
            let deadline = Instant::now() + Duration::from_secs(10);
            while !host.reference_title().unwrap().is_loaded() {
                assert!(Instant::now() < deadline, "Title reload stalled");
                tick(
                    &mut host, context, server, &mut frame, None, button, false, extent,
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let latest = host.reference_save();
            let setup =
                crate::title::TitleSetup::from_save(&latest, crate::title::TitleItem::Load as i32);
            assert_eq!(
                host.reference_title().unwrap().motion.selected(),
                setup.selected
            );
            assert_eq!(std::fs::read(&path).unwrap(), disk);
            host.reference_request_saved_scene(game(1));
        } else {
            let live = host.reference_adv_mut().unwrap();
            live.adv_state.release_audio_pause();
            live.adv_state.release_audio_pause();
            let saved = live.save_menu_data();
            live.load_game_data(&saved, &game(1), server.clone())
                .unwrap();
        }
        tick(
            &mut host, context, server, &mut frame, None, button, false, extent,
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while host.reference_adv().unwrap().is_loading_saved_scene() {
            assert!(Instant::now() < deadline, "{name}: valid retry stalled");
            tick(
                &mut host, context, server, &mut frame, None, button, false, extent,
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            host.reference_adv()
                .unwrap()
                .saved_scene_load_error()
                .is_none()
        );
        assert_eq!(host.reference_adv().unwrap().vm_state.save_position, 1);
        assert_eq!(host.reference_save().save_menu_position, 99);
        drop(host);
        std::fs::remove_dir_all(&directory).unwrap();
        eprintln!(
            "Scene failure {name}: notice pixels{changed}, glyphs{glyphs}; pointer priority, frozen VM/route/command, disk/manual slot, owned pause, title handoff and valid retry pass"
        );
    }
}
