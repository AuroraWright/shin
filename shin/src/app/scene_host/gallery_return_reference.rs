//! Focused real submenu close -> captured Title fade and input handoff.
use super::*;
use crate::{adv::assets::AdvFonts, render::PreRenderContext};
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};

pub(super) fn check<'a>(
    server: &'a Arc<AssetServer>,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    let scenario = server.load_sync("/main.snr").unwrap();
    let common = Arc::new(CommonAssets {
        scenario,
        fonts: AdvFonts {
            system_font: server.load_sync("/system.fnt").unwrap(),
            medium_font: server.load_sync("/newrodin-medium.fnt").unwrap(),
            bold_font: server.load_sync("/newrodin-bold.fnt").unwrap(),
        },
        messagebox: server.load_sync("/msgtex.txa").unwrap(),
        _system: server.load_sync("/systex.txa").unwrap(),
        effects: shin_tasks::block_on(server.effects_atlas()).unwrap(),
        _sysse: server.load_sync("/sysse.bin").unwrap(),
    });
    fn step<'a>(
        host: &mut SceneHost,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        frame: &mut FrameId,
        delta: f32,
        point: glam::Vec2,
        held: bool,
        extent: (u16, u16),
        action: Option<AppAction>,
    ) {
        let mut input = EnumMap::<AppAction, ActionState>::default();
        input[AppAction::PointerClick].is_held = held;
        if let Some(action) = action {
            input[action].is_clicked = true;
            input[action].is_clicked_or_repeated = true;
        }
        frame.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: ctx,
            },
            input,
            point,
            point,
            extent,
            0.0,
            1.0 / 60.0,
        )
        .unwrap();
    }
    fn draw(host: &SceneHost, ctx: &mut PreRenderContext, target: &wgpu::Texture) -> Vec<u8> {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(ctx.depth_stencil),
                "Gallery return fade reference",
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
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let exe = std::fs::read(
        std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).with_file_name(
            if updated {
                "uncompressed_executable_update"
            } else {
                "uncompressed_executable"
            },
        ),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    let shift = if updated { 0x8c0 } else { 0 };
    assert_eq!(word(0x110b08 + shift), 0x321e0fe1); //return duration60
    assert_eq!(word(0x110b14 + shift), 0x52800b41); //initial duration90
    let shared_shift = if updated { 0xa0 } else { 0 };
    assert_eq!(word(0x62f4c - shared_shift), 0x1e202840); //float addition
    assert_eq!(word(0x62f54 - shared_shift), 0xbd000400); //elapsed publication
    for (item, extent) in [
        (TitleItem::Start, (1280, 720)),
        (TitleItem::Cg, (960, 540)),
        (TitleItem::Bgm, (1920, 1080)),
        (TitleItem::Movie, (960, 540)),
        (TitleItem::Bustup, (1920, 1080)),
    ] {
        if std::env::var_os("SHIN_REFERENCE_START_RETURN_ONLY").is_some()
            && item != TitleItem::Start
        {
            continue;
        }
        let mut save = SaveData::default();
        save.persist_data.set(0, 31);
        let setup = TitleSetup::from_save(&save, item as i32);
        let point = (setup.row_origin(0).unwrap() + glam::vec2(100.0, 20.0))
            * glam::vec2(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
        let manager = audio.manager.clone();
        let mut host = SceneHost {
            scene: Some(Scene::Title(TitleRuntime {
                scene: TitleScene::new(server.clone(), setup),
                globals: GlobalData::open(None, Some(save)).unwrap(),
                audio,
                extent,
            })),
            pending: None,
            assets: None,
            common: Some(common.clone()),
            server: server.clone(),
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        };
        let mut frame = FrameId::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !host.reference_title().unwrap().is_loaded() {
            step(
                &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(
            !host.reference_title().unwrap().motion.accepts_input(),
            "Initial entrance unchanged"
        );
        match item {
            TitleItem::Start => {
                host.pending = Some(Destination::Action(TitleItem::Start));
                step(
                    &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
                );
            }
            TitleItem::Cg => host.enter_cg(ctx, extent).unwrap(),
            TitleItem::Bgm => host.enter_music(ctx, extent).unwrap(),
            TitleItem::Movie => host.enter_movie(ctx, extent).unwrap(),
            TitleItem::Bustup => host.enter_bustup(ctx, extent).unwrap(),
            _ => unreachable!(),
        }
        loop {
            let ready = match host.scene.as_ref().unwrap() {
                Scene::Picker(s) => s.scene.accepts_input(),
                Scene::Cg(s) => s.scene.reference_state().2,
                Scene::Music(s) => s.scene.reference_state().2,
                Scene::Movie(s) => s.scene.reference_state().2,
                Scene::Bustup(s) => s.scene.reference_state().1,
                _ => false,
            };
            if ready {
                break;
            }
            step(
                &mut host, server, ctx, &mut frame, 30.0, point, false, extent, None,
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            0.0,
            point,
            false,
            extent,
            Some(AppAction::Cancel),
        );
        // Keyboard Back in Start has no close-button flash; its next frame
        // queues the factory immediately. Galleries wait for their flash.
        if item != TitleItem::Start {
            step(
                &mut host, server, ctx, &mut frame, 10.0, point, false, extent, None,
            );
        }
        step(
            &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
        );
        assert!(host.pending.is_some(), "Real close must queue Title return");
        let outgoing = draw(&host, ctx, target);
        step(
            &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
        );
        assert_eq!(
            host.reference_title().unwrap().reference_return_capture(),
            Some(1.0)
        );
        assert_eq!(
            draw(&host, ctx, target),
            outgoing,
            "Outgoing frame retained before resources publish"
        );
        while !host.reference_title().unwrap().is_loaded() {
            step(
                &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
            );
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            draw(&host, ctx, target),
            outgoing,
            "Fade starts with complete outgoing submenu"
        );
        let selected = host.reference_title().unwrap().motion.selected();
        step(
            &mut host, server, ctx, &mut frame, 30.0, point, true, extent, None,
        );
        assert_eq!(
            host.reference_title().unwrap().reference_return_capture(),
            Some(0.5)
        );
        let middle = draw(&host, ctx, target);
        assert_eq!(host.reference_title().unwrap().motion.selected(), selected);
        assert!(!host.reference_title().unwrap().motion.accepts_input());
        step(
            &mut host, server, ctx, &mut frame, 30.0, point, true, extent, None,
        );
        assert_eq!(
            host.reference_title().unwrap().reference_return_capture(),
            Some(0.0)
        );
        let end = draw(&host, ctx, target);
        let changed = outgoing
            .chunks_exact(4)
            .zip(end.chunks_exact(4))
            .filter(|(a, b)| a[..3] != b[..3])
            .count();
        assert!(
            changed > 10000,
            "Return must visibly change submenu to Title: {changed}"
        );
        let error = outgoing
            .chunks_exact(4)
            .zip(end.chunks_exact(4))
            .zip(middle.chunks_exact(4))
            .flat_map(|((a, b), m)| {
                (0..3).map(move |i| (m[i] as f32 - (a[i] as f32 + b[i] as f32) * 0.5).abs())
            })
            .fold(0.0_f32, f32::max);
        assert!(error <= 2.0, "Half-time compositing error{error}");
        for _ in 0..3 {
            step(
                &mut host, server, ctx, &mut frame, 0.0, point, true, extent, None,
            );
        }
        assert_eq!(
            host.reference_title().unwrap().reference_return_capture(),
            None
        );
        assert!(host.reference_title().unwrap().motion.accepts_input());
        step(
            &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
        );
        assert_eq!(
            host.reference_title().unwrap().motion.selected(),
            selected,
            "Inherited release must not click through"
        );
        step(
            &mut host, server, ctx, &mut frame, 1.0, point, true, extent, None,
        );
        step(
            &mut host, server, ctx, &mut frame, 1.0, point, false, extent, None,
        );
        assert_eq!(
            host.reference_title().unwrap().motion.selected(),
            0,
            "Fresh first click after fade registers"
        );
        for _ in 0..3 {
            step(
                &mut host, server, ctx, &mut frame, 0.0, point, true, extent, None,
            );
            step(
                &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
            );
        }
        step(
            &mut host, server, ctx, &mut frame, 40.0, point, false, extent, None,
        );
        step(
            &mut host, server, ctx, &mut frame, 0.0, point, false, extent, None,
        );
        assert!(matches!(
            host.pending,
            Some(Destination::Action(TitleItem::Start))
        ));
        let Some(Scene::Title(title)) = host.scene.as_mut() else {
            unreachable!()
        };
        assert!(title.scene.motion.take_destination().is_none());
        assert!(Arc::ptr_eq(&manager, &host.reference_audio().manager));
        assert!(host.error.is_none());
        eprintln!(
            "{item:?} return {extent:?}: actual close/capture,60tick fade {changed} changed pixels max midpoint error{error}, inherited contact and first fresh click pass"
        );
    }
}
