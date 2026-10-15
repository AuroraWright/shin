//! Actual-ROM paused loading, started-frame draw, skip and cover retirement.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    render::PreRenderContext,
};
use shin_core::format::scenario::instruction_elements::FromNumber;
use shin_core::{format::save::SaveData, primitives::update::FrameId, time::Ticks};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};
#[test]
#[ignore = "requires supplied ROM, FFmpeg and GPU (SHIN_REFERENCE_ROM)"]
fn reference_movie_playback_owner_loading_skip_render_and_retirement() {
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
            ..Default::default()
        }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let scenario: Arc<Scenario> =
        shin_tasks::block_on(server.load(crate::asset::asset_paths::SCENARIO)).unwrap();
    let info = scenario
        .info_tables()
        .movie_info
        .first()
        .expect("Native startup movie0");
    eprintln!("Native gallery movie: {info:?}");
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference depth"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Startup reference pixels"),
        size: depth.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), shin_render::TEXTURE_FORMAT);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ctx = PreRenderContext {
        device: &device,
        queue: &queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };

    let mut save = SaveData::default();
    save.settings.v0_bgmvol = 37;
    save.settings.v3_sysvol = 0;
    let globals = GlobalData::open(None, Some(save)).unwrap();
    let before = globals.save().encode();
    let mut audio = GlobalAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()));
    let system_track = audio.system_player.track.id();
    let bgm_track = audio.bgm_player.reference_source().2;
    let mut id = FrameId::default();
    let mut number = 0u32;
    let mut now = 0.0f64;
    fn pixels(
        scene: &MoviePlaybackScene,
        ctx: &mut PreRenderContext,
        target: &wgpu::Texture,
    ) -> Vec<u8> {
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: &view,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "Movie child reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::WHITE),
                Some(0),
                Some(1.0),
            );
            scene.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(ctx, target)
    }
    for mode in 0..3 {
        let mut scene = MoviePlaybackScene::new(
            server.clone(),
            &scenario,
            MovieId::from_number(0),
            &globals,
            &audio,
            (1920, 1080),
        )
        .unwrap();
        //Independent white background proves paused/loading frame stays hidden.
        let p = pixels(&scene, &mut ctx, &target);
        assert!(p.iter().all(|v| *v == 255));
        let mut step = |scene: &mut MoviePlaybackScene,
                        delta: f32,
                        action: Option<AppAction>,
                        events: &[crate::adv::input_area::PointerEvent]| {
            id.advance();
            number += 1;
            now += delta.max(0.0) as f64 / 60.0;
            let mut state = EnumMap::default();
            if let Some(a) = action {
                state[a] = ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            scene
                .update(
                    &UpdateContext {
                        frame_started_at: std::time::Instant::now(),
                        frame_id: id,
                        delta_ticks: Ticks::from_f32(delta),
                        asset_server: &server,
                        pre_render: &mut ctx,
                    },
                    &PointerFrame {
                        number,
                        time_seconds: now,
                        events,
                    },
                    &state,
                    (1920, 1080),
                    &globals,
                    &mut audio,
                )
                .unwrap();
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        //Zero time holds cover even if worker publishes. Inputs cannot start/skip.
        while scene.player.is_none() {
            step(&mut scene, 0.0, Some(AppAction::Cancel), &[]);
            assert!(!scene.motion.renders_movie());
            assert!(std::time::Instant::now() < deadline, "Movie worker timeout");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        step(&mut scene, 15.0, Some(AppAction::Cancel), &[]);
        assert_eq!(scene.motion.cover_opacity(), 0.5);
        drop(step);
        let p = pixels(&scene, &mut ctx, &target);
        for px in p.chunks_exact(4) {
            assert_eq!(px, [128, 128, 128, 255]);
        }
        //Reborrow context after GPU readback.
        let mut step = |scene: &mut MoviePlaybackScene,
                        delta: f32,
                        action: Option<AppAction>,
                        events: &[crate::adv::input_area::PointerEvent]| {
            id.advance();
            number += 1;
            now += delta.max(0.0) as f64 / 60.0;
            let mut state = EnumMap::default();
            if let Some(a) = action {
                state[a] = ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            scene
                .update(
                    &UpdateContext {
                        frame_started_at: std::time::Instant::now(),
                        frame_id: id,
                        delta_ticks: Ticks::from_f32(delta),
                        asset_server: &server,
                        pre_render: &mut ctx,
                    },
                    &PointerFrame {
                        number,
                        time_seconds: now,
                        events,
                    },
                    &state,
                    (1920, 1080),
                    &globals,
                    &mut audio,
                )
                .unwrap();
        };
        step(&mut scene, 15.0, Some(AppAction::Cancel), &[]);
        assert!(!scene.motion.renders_movie());
        step(&mut scene, 0.0, Some(AppAction::Cancel), &[]);
        assert!(scene.motion.renders_movie());
        assert_eq!(scene.motion.movie_opacity(), 1.0);
        assert!(
            scene
                .player
                .as_ref()
                .unwrap()
                .get_retained_frame()
                .is_some()
        );
        drop(step);
        let cues = audio.system_player.sounds.len();
        let image = pixels(&scene, &mut ctx, &target);
        //The opening may begin on authored black pixels. Compare against the
        //actual decoded frame drawn alone at the native full-screen extent.
        let view = target.create_view(&Default::default());
        {
            let mut pass = ctx.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: &view,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "Movie direct reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::WHITE),
                Some(0),
                Some(1.0),
            );
            let frame = scene.player.as_ref().unwrap().get_retained_frame().unwrap();
            let size = frame.get_frame().get_size();
            frame.render(
                &mut pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                top_left_projection_matrix()
                    * Mat4::from_scale(glam::vec3(1920.0 / size.x, 1080.0 / size.y, 1.0)),
            );
        }
        let direct = crate::layer::user::readback_test_texture(&mut ctx, &target);
        assert_eq!(image, direct);
        let mut step = |scene: &mut MoviePlaybackScene,
                        delta: f32,
                        action: Option<AppAction>,
                        events: &[crate::adv::input_area::PointerEvent]| {
            id.advance();
            number += 1;
            now += delta.max(0.0) as f64 / 60.0;
            let mut state = EnumMap::default();
            if let Some(a) = action {
                state[a] = ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            scene
                .update(
                    &UpdateContext {
                        frame_started_at: std::time::Instant::now(),
                        frame_id: id,
                        delta_ticks: Ticks::from_f32(delta),
                        asset_server: &server,
                        pre_render: &mut ctx,
                    },
                    &PointerFrame {
                        number,
                        time_seconds: now,
                        events,
                    },
                    &state,
                    (960, 540),
                    &globals,
                    &mut audio,
                )
                .unwrap();
        };
        if mode == 0 {
            step(&mut scene, 15.0, Some(AppAction::Cancel), &[]);
            assert_eq!(scene.motion.movie_opacity(), 0.5);
            assert!((scene.applied_volume.0 - 0.185).abs() < 0.00001);
            step(&mut scene, 15.0, Some(AppAction::Cancel), &[]);
        } else if mode == 1 {
            use crate::adv::input_area::{PointerEvent, PointerKind};
            //Fast full-canvas tap consumes simultaneous controller Cancel.
            let events = [
                PointerEvent {
                    kind: PointerKind::Down,
                    id: 0,
                    position: glam::ivec2(400, 200),
                },
                PointerEvent {
                    kind: PointerKind::Up,
                    id: 0,
                    position: glam::ivec2(400, 200),
                },
            ];
            step(&mut scene, 0.0, Some(AppAction::Cancel), &events);
            assert_eq!(scene.motion.movie_opacity(), 1.0);
            step(&mut scene, 7.0, None, &[]);
            //New fades already advance on their input-dispatch frame.
            step(&mut scene, 8.0, None, &[]);
            assert_eq!(scene.motion.movie_opacity(), 0.5);
            step(&mut scene, 15.0, None, &[]);
        } else {
            //Native status observation is before input; retained frame stays
            //visible until Stop on next task poll, even when decoder finishes.
            scene.player.as_ref().unwrap().terminate();
            step(&mut scene, 0.0, Some(AppAction::Cancel), &[]);
            assert!(scene.motion.renders_movie());
        }
        step(&mut scene, 0.0, Some(AppAction::Cancel), &[]);
        assert!(!scene.motion.renders_movie());
        assert_eq!(scene.motion.cover_opacity(), 1.0);
        assert_eq!(scene.player.as_ref().unwrap().is_finished(), mode == 2);
        step(&mut scene, 15.0, Some(AppAction::Cancel), &[]);
        assert_eq!(scene.motion.cover_opacity(), 0.5);
        assert!(!scene.is_finished());
        drop(step);
        assert_eq!(
            audio.system_player.sounds.len(),
            cues + usize::from(mode != 2)
        );
        let p = pixels(&scene, &mut ctx, &target);
        for px in p.chunks_exact(4) {
            assert_eq!(px, [128, 128, 128, 255]);
        }
        id.advance();
        number += 1;
        scene
            .update(
                &UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: id,
                    delta_ticks: Ticks::from_f32(15.0),
                    asset_server: &server,
                    pre_render: &mut ctx,
                },
                &PointerFrame {
                    number,
                    time_seconds: now,
                    events: &[],
                },
                &EnumMap::default(),
                (960, 540),
                &globals,
                &mut audio,
            )
            .unwrap();
        assert!(!scene.is_finished());
        id.advance();
        number += 1;
        scene
            .update(
                &UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: id,
                    delta_ticks: Ticks::ZERO,
                    asset_server: &server,
                    pre_render: &mut ctx,
                },
                &PointerFrame {
                    number,
                    time_seconds: now,
                    events: &[],
                },
                &EnumMap::default(),
                (960, 540),
                &globals,
                &mut audio,
            )
            .unwrap();
        assert!(scene.is_finished());
        let p = pixels(&scene, &mut ctx, &target);
        assert!(p.iter().all(|v| *v == 255));
        assert_eq!(globals.save().encode(), before);
        assert_eq!(audio.system_player.track.id(), system_track);
        assert_eq!(audio.bgm_player.reference_source().2, bgm_track);
    }
}
