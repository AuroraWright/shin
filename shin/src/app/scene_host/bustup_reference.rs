//! Original-ROM/GPU scene-host Title6 -> CgMode -> Title6 roundtrip.
use super::*;
use crate::{adv::assets::AdvFonts, render::PreRenderContext};
use shin_core::{
    format::scenario::instruction_elements::FromNumber, primitives::update::FrameId, time::Ticks,
};

pub(crate) fn reference_live_bustup_roundtrip<'a>(
    server: &'a Arc<AssetServer>,
    ctx: &mut PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
) {
    use crate::{
        adv::character_view::CharacterTextures, asset::effects::EffectTextures,
        layer::message_layer::MessageboxTextures,
    };
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
        effects: server.load_sync::<EffectTextures>("/effects.txa").unwrap(),
        _sysse: server.load_sync::<SysSe>("/sysse.bin").unwrap(),
    });
    let mut save = SaveData::default();
    save.persist_data.set(0, 31);
    save.settings.v0_bgmvol = 37;
    save.settings.v3_sysvol = 0;
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let setup = TitleSetup::from_save(globals.save(), 10);
    assert_eq!(setup.items[setup.selected], TitleItem::Bustup);
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
                "Live BustupMode host reference",
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }

    fn scene(host: &SceneHost) -> &BustupScene {
        let Some(Scene::Bustup(runtime)) = &host.scene else {
            panic!("Expected Bustup scene");
        };
        &runtime.scene
    }
    let before = host.reference_save();
    let track = host.reference_audio().bgm_player.reference_source().2;
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
        Some(Destination::Action(TitleItem::Bustup))
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
    assert_eq!(pixels(&host, ctx, target, view), title);
    while scene(&host).reference_opacity() > 0.0 {
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
    assert!(!scene(&host).reference_state().1);
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
    assert!(!scene(&host).reference_state().1);
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
    assert_eq!(
        scene(&host).reference_state(),
        (true, true, false, false, 1.0)
    );
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().0,
        Some(shin_core::format::scenario::info::BgmId::from_number(7))
    );
    let initial = pixels(&host, ctx, target, view);
    assert!(
        initial
            .chunks_exact(4)
            .any(|p| p[0].max(p[1]).max(p[2]) > 20)
    );
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
    assert!(scene(&host).reference_load_blocked());
    while scene(&host).reference_load_blocked() {
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
        assert!(!scene(&host).reference_state().1);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
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
    assert_eq!(scene(&host).reference_presentation().3, 1);
    let replaced = pixels(&host, ctx, target, view);
    assert!(
        initial
            .iter()
            .zip(&replaced)
            .filter(|(a, b)| a.abs_diff(**b) > 5)
            .count()
            > 1000
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
    assert!(scene(&host).reference_state().2);
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
    assert!(scene(&host).reference_state().2);
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
    assert!(scene(&host).reference_state().2);
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
    assert_eq!(scene(&host).reference_presentation().1, 10.0);
    assert_eq!(scene(&host).reference_state().4, 0.0);
    let viewer = pixels(&host, ctx, target, view);
    assert_ne!(viewer, replaced);
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
    assert!(scene(&host).reference_state().2);
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
    assert!(scene(&host).reference_state().2);
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
    assert_eq!(
        scene(&host).reference_state(),
        (true, true, false, false, 1.0)
    );
    assert_eq!(scene(&host).reference_presentation(), (-200.0, 0.0, 0.0, 1));
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        7.0,
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
        &[],
        point,
        false,
        full,
    );
    assert!(matches!(host.pending, Some(Destination::BustupBack(10))));
    assert!(matches!(host.scene, Some(Scene::Bustup(_))));
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
    assert!(matches!(host.scene, Some(Scene::Title(_))));
    assert_eq!(
        TitleSetup::from_save(&host.reference_save(), 10).items
            [host.reference_title().unwrap().motion.selected()],
        TitleItem::Bustup
    );
    let mut after = host.reference_save();
    assert!(after.play_seconds > before.play_seconds);
    after.play_seconds = before.play_seconds;
    assert_eq!(after.encode(), before.encode());
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().2,
        track
    );
    assert_eq!(
        host.reference_audio().system_player.track.id(),
        system_track
    );
    assert!(host.assets.is_none());
    assert!(host.error.is_none());
    eprintln!(
        "Live Title10/Bustup: original async PIC/BUP publications, exact previous-frame capture,60tick opening/BGM7, completion-frame input gates, real replacement fade, shared viewer centering/pan/return, deferred Title10 and retained globals/audio pass"
    );
}
