//! Original-ROM/GPU scene-host Title6 -> CgMode -> Title6 roundtrip.
use super::*;
use crate::{adv::assets::AdvFonts, render::PreRenderContext};
use shin_core::{
    format::scenario::instruction_elements::FromNumber, primitives::update::FrameId, time::Ticks,
};

pub(crate) fn reference_live_cg_gallery_roundtrip<'a>(
    server: &'a Arc<AssetServer>,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
) {
    use crate::{adv::characters::view::CharacterTextures, layer::message_layer::MessageboxTextures};
    use shin_core::format::{save::SaveData, scenario::Scenario, sysse::SysSe};
    let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
    let common = Arc::new(CommonAssets {
        scenario: scenario.clone(),
        fonts: AdvFonts {
            system_font: server.load_sync("/system.fnt").unwrap(),
            medium_font: server.load_sync("/newrodin-medium.fnt").unwrap(),
            bold_font: server.load_sync("/newrodin-bold.fnt").unwrap(),
        },
        messagebox: server
            .load_sync::<MessageboxTextures>("/msgtex.txa")
            .unwrap(),
        _system: server
            .load_sync::<CharacterTextures>("/systex.txa")
            .unwrap(),
        effects: shin_tasks::block_on(server.effects_atlas()).unwrap(),
        _sysse: server.load_sync::<SysSe>("/sysse.bin").unwrap(),
    });
    let mut save = SaveData::default();
    save.persist_data.set(0, 1);
    save.settings.v0_bgmvol = 37;
    save.settings.v3_sysvol = 0;
    for id in &scenario.info_tables().picture_box_info[0].picture_ids.0 {
        save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
    }
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let setup = TitleSetup::from_save(globals.save(), 6);
    assert_eq!(setup.items[setup.selected], TitleItem::Cg);
    let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
    let mut host = SceneHost {
        scene: Some(Scene::Title(TitleRuntime {
            scene: TitleScene::new(server.clone(), setup),
            globals,
            audio,
            extent: (1920, 1080),
        })),
        pending: None,
        assets: None,
        common: Some(common),
        server: server.clone(),
        pointer: Default::default(),
        error: None,
        restoring_from_title: false,
        viewer_axes: glam::Vec2::ZERO,
    };
    fn step<'a>(
        host: &mut SceneHost,
        server: &'a Arc<AssetServer>,
        ctx: &mut PreRenderContext<'a, '_, '_, '_>,
        id: &mut FrameId,
        delta: f32,
        actions: &[AppAction],
        point: glam::Vec2,
        held: bool,
        extent: (u16, u16),
    ) {
        let mut input = EnumMap::default();
        for action in actions {
            input[*action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                is_held: true,
                ..Default::default()
            };
        }
        input[AppAction::PointerClick].is_held = held;
        id.advance();
        host.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: *id,
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
    fn pixels(
        host: &SceneHost,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) -> Vec<u8> {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        {
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view,
                },
                Some(ctx.depth_stencil),
                "Live CgMode host reference",
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }

    fn cg(host: &SceneHost) -> &CgGalleryScene {
        let Some(Scene::Cg(runtime)) = &host.scene else {
            panic!("Expected live CG gallery")
        };
        &runtime.scene
    }
    let before = host.reference_save();
    let bgm_track = host.reference_audio().bgm_player.reference_source().2;
    let system_track = host.reference_audio().system_player.track.id();
    let mut frame = FrameId::default();
    let full = (1920, 1080);
    let point = glam::Vec2::ZERO;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !host.reference_title().unwrap().is_loaded() {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            0.0,
            &[],
            point,
            false,
            full,
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for _ in 0..12 {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            30.0,
            &[],
            point,
            false,
            full,
        );
    }
    assert_eq!(
        host.reference_title().unwrap().motion.phase(),
        crate::title::TitlePhase::Active
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        7.0,
        &[AppAction::Enter],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert!(matches!(
        host.pending,
        Some(Destination::Action(TitleItem::Cg))
    ));
    let title = pixels(&host, ctx, target, view);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    assert!(matches!(host.scene, Some(Scene::Cg(_))));
    assert_eq!(pixels(&host, ctx, target, view), title);
    while cg(&host).reference_state().1 != 0.0 || !cg(&host).reference_state().0 {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            10.0,
            &[AppAction::Cancel],
            point,
            false,
            full,
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Fade is complete but native waiter has not installed input yet.
    assert!(!cg(&host).reference_state().2);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    assert!(!cg(&host).reference_state().2);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert!(cg(&host).reference_state().2);
    assert!(!cg(&host).reference_capture_owned());
    let gallery = pixels(&host, ctx, target, view);
    assert_ne!(gallery, title);
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().0,
        Some(shin_core::format::scenario::info::BgmId::from_number(6))
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Enter],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        8.0,
        &[AppAction::Right, AppAction::Enter],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        8.0,
        &[AppAction::Right, AppAction::Enter],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert_eq!(cg(&host).reference_state().3, 0);
    assert_eq!(cg(&host).reference_state().4, 1);
    while !cg(&host).reference_state().5 {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            0.0,
            &[AppAction::Cancel],
            point,
            false,
            full,
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!cg(&host).reference_child_state().unwrap().1);
    // Gallery remains beneath the first published PIC's capture.
    let publication = pixels(&host, ctx, target, view);
    for y in (100..950).step_by(20) {
        for x in (100..1800).step_by(20) {
            let i = (y * 1920 + x) * 4;
            assert_eq!(&publication[i..i + 4], &gallery[i..i + 4]);
        }
    }
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        30.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    assert!(!cg(&host).reference_child_state().unwrap().1);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert!(cg(&host).reference_child_state().unwrap().0);
    let picture = pixels(&host, ctx, target, view);
    assert_ne!(picture, gallery);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::CharacterVariant],
        point,
        false,
        full,
    );
    assert!(cg(&host).reference_child_state().unwrap().3);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        10.0,
        &[],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    assert!(cg(&host).reference_child_state().unwrap().1);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        30.0,
        &[],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert_eq!(cg(&host).reference_state().4, 1); // Empty child retires on next parent poll.
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        point,
        false,
        full,
    );
    assert_eq!(cg(&host).reference_state().4, 0);
    assert!(cg(&host).reference_state().2);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Right],
        point,
        false,
        full,
    );
    assert_eq!(cg(&host).reference_state().3, 1);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        4.0,
        &[],
        point,
        false,
        full,
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Enter],
        point,
        false,
        full,
    );
    for _ in 0..3 {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            10.0,
            &[],
            point,
            false,
            full,
        );
    }
    assert_eq!(cg(&host).reference_state().4, 0); // Locked row produces only error cue.
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    for _ in 0..4 {
        step(
            &mut host,
            server,
            ctx,
            &mut frame,
            10.0,
            &[],
            point,
            false,
            full,
        );
    }
    assert!(matches!(host.scene, Some(Scene::Title(_))));
    assert_eq!(
        TitleSetup::from_save(&host.reference_save(), 6).items
            [host.reference_title().unwrap().motion.selected()],
        TitleItem::Cg
    );
    assert!(host.assets.is_none());
    assert!(host.error.is_none());
    let mut after = host.reference_save();
    assert!(host.reference_play_ticks() > 0.0);
    assert!(after.play_seconds > before.play_seconds);
    after.play_seconds = before.play_seconds;
    assert!(
        after.encode() == before.encode(),
        "CG changed save data beyond global play clock"
    );
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().2,
        bgm_track
    );
    assert_eq!(
        host.reference_audio().system_player.track.id(),
        system_track
    );
    eprintln!(
        "Live CG Title6/gallery loading/fade/BGM6/viewer capture/fit/close/retirement/locked error/Title6 return pass"
    );
}
