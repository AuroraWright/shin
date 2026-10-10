//! Original-ROM/GPU scene-host Title7 -> MusicMode -> Title7 roundtrip.
use super::*;
use crate::{adv::assets::AdvFonts, render::PreRenderContext};
use shin_core::{primitives::update::FrameId, time::Ticks};

pub(crate) fn reference_live_music_gallery_roundtrip<'a>(
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
    let first = scenario.info_tables().music_box_info[0].bgm_id;
    let second = scenario.info_tables().music_box_info[1].bgm_id;
    assert_ne!(first, second);
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
    save.persist_data.set(0, 1); // Native progression enables Title galleries.
    save.settings.v0_bgmvol = 37;
    save.settings.v3_sysvol = 25;
    save.save_vectors
        .unlocked_bgms
        .add(first.into_i32() as usize);
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let setup = TitleSetup::from_save(globals.save(), 7);
    assert_eq!(setup.items[setup.selected], TitleItem::Bgm);
    let audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
    let manager = audio.manager.clone();
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
                "Live MusicMode host reference",
            );
            host.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    fn music(host: &SceneHost) -> (bool, f32, bool, usize, Option<usize>) {
        let Some(Scene::Music(runtime)) = &host.scene else {
            panic!("Host is not MusicMode");
        };
        runtime.scene.reference_state()
    }
    let mut frame = FrameId::default();
    let full = (1920, 1080);
    let point = glam::Vec2::ZERO;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
        if host.reference_title().unwrap().motion.phase() == crate::title::TitlePhase::Active {
            break;
        }
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
    let source_track = host.reference_audio().bgm_player.reference_source().2;
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
    assert!(host.pending.is_none());
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
        Some(Destination::Action(TitleItem::Bgm))
    ));
    let previous = pixels(&host, ctx, target, view);
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
    assert_eq!(music(&host).1, 1.0);
    assert_eq!(music(&host).3, 0);
    assert_eq!(
        pixels(&host, ctx, target, view),
        previous,
        "MusicMode loading must preserve the complete previous Title frame"
    );
    while !music(&host).0 {
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
        assert!(host.pending.is_none());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(music(&host).1, 1.0);
    assert!(!music(&host).2);
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().0,
        None,
        "Artwork publication must stop Title music"
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        -3.0,
        &[],
        point,
        false,
        full,
    );
    assert!((music(&host).1 - 1.05).abs() < 0.00001);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        33.0,
        &[AppAction::Cancel],
        point,
        false,
        full,
    );
    assert_eq!(music(&host).1, 0.5);
    let half = pixels(&host, ctx, target, view);
    assert_ne!(half, previous);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        30.0,
        &[AppAction::Enter],
        point,
        false,
        full,
    );
    assert_eq!(music(&host).1, 0.0);
    assert_eq!(music(&host).4, None);
    let revealed = pixels(&host, ctx, target, view);
    //63180 supplies a float premultiplied color, with no56840 byte conversion.
    //Compare the full actual frame against independent native blend arithmetic.
    let capture_alpha = 0.5f32;
    for (index, (&actual, (&gallery, &title))) in
        half.iter().zip(revealed.iter().zip(&previous)).enumerate()
    {
        if index % 4 == 3 {
            continue;
        }
        let expected = gallery as f32 * (1.0 - capture_alpha) + title as f32 * capture_alpha;
        assert!(
            (actual as f32 - expected).abs() <= 2.0,
            "Live MusicMode half-fade byte{index}: {actual} vs {expected}"
        );
    }
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        -2.0,
        &[AppAction::Enter],
        point,
        false,
        full,
    );
    assert!(
        !music(&host).2,
        "New input task cannot run on its installation frame"
    );
    assert!((music(&host).1 - 1.0 / 30.0).abs() < 0.00001);
    assert_eq!(music(&host).4, None);
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
    let Some(Scene::Music(runtime)) = &host.scene else {
        unreachable!()
    };
    assert!(!runtime.scene.reference_capture_owned());
    assert_eq!(music(&host).4, Some(0));
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().0,
        Some(first)
    );
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().2,
        source_track
    );
    let shown = pixels(&host, ctx, target, view);
    assert!(shown.iter().zip(&previous).filter(|(a, b)| a != b).count() > 10000);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[AppAction::ConfigReset],
        point,
        false,
        full,
    );
    assert_eq!(music(&host).4, None);
    assert!(
        host.reference_audio()
            .bgm_player
            .get_wait_status()
            .is_empty()
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        4.0,
        &[AppAction::Right],
        point,
        false,
        full,
    );
    assert_eq!(music(&host).3, 1);
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
    assert_eq!(music(&host).4, None);
    assert!(
        !host
            .reference_save()
            .save_vectors
            .unlocked_bgms
            .contains(second.into_i32() as usize)
    );
    let close_point = glam::vec2(900.0, 40.0);
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        close_point,
        true,
        (960, 540),
    );
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        7.0,
        &[],
        close_point,
        false,
        (960, 540),
    );
    assert!(host.pending.is_none());
    step(
        &mut host,
        server,
        ctx,
        &mut frame,
        0.0,
        &[],
        close_point,
        false,
        (960, 540),
    );
    assert!(matches!(host.pending, Some(Destination::MusicBack(7))));
    assert!(matches!(host.scene, Some(Scene::Music(_))));
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
    assert!(host.reference_title().is_some());
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
        if host.reference_title().unwrap().motion.phase() == crate::title::TitlePhase::Active {
            break;
        }
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
        host.reference_title().unwrap().motion.frame().caption,
        Some(TitleItem::Bgm)
    );
    assert!(Arc::ptr_eq(&manager, &host.reference_audio().manager));
    let returned_save = host.reference_save();
    assert_eq!(returned_save.settings.v0_bgmvol, 37);
    assert_eq!(returned_save.settings.v3_sysvol, 25);
    assert_eq!(returned_save.persist_data.get(0), 1);
    assert!(
        returned_save
            .save_vectors
            .unlocked_bgms
            .contains(first.into_i32() as usize)
    );
    assert!(
        !returned_save
            .save_vectors
            .unlocked_bgms
            .contains(second.into_i32() as usize)
    );
    assert_eq!(
        host.reference_audio().bgm_player.reference_source().2,
        source_track
    );
    assert!(
        host.assets.is_none(),
        "Title gallery must not initialize ADV resources"
    );
    assert!(host.reference_play_ticks() >= 131.0);
    println!(
        "Live MusicMode host: Title7 capture/loading/fade/task gate, real BGM/stop/locked error, half-size pointer close, retained globals/audio and Title7 return pass"
    );
}
