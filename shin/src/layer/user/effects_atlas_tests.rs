//! Real resource identities and retirement, including encoded GPU work after
//! atlas CPU retirement. No reference-count model or substitute owner tokens.
use super::*;
use crate::{
    asset::{
        effects::{EffectTextures, EffectsAtlasBorrow},
        system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
        texture_archive::TextureArchiveBuilder,
    },
    render::PreRenderContext,
};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    PassKind,
    dynamic_buffer::DynamicBuffer,
    gpu_texture::GpuTexture,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};

fn identity(layer: &UserLayer) -> usize {
    match layer {
        UserLayer::Rain(l) => l.reference_constructor_state().0,
        UserLayer::FocusLine(l) => l.reference_constructor_state().0,
        UserLayer::Animation(l) => l.reference_constructor_state().0,
        _ => panic!("Expected actual particle layer"),
    }
}
fn synthetic(device: &wgpu::Device, queue: &wgpu::Queue, color: [u8; 4]) -> Arc<EffectTextures> {
    let mut textures = EffectTextures::default();
    for name in [
        "Sakura",
        "rain",
        "hanabira",
        "snow",
        "goldsnow",
        "focusline",
        "breakup",
        "unknown-authored-name",
    ] {
        textures.add_texture(
            name,
            GpuTexture::new_static_with_data(
                device,
                queue,
                Some(name),
                winit::dpi::PhysicalSize::new(1, 1),
                wgpu::TextureFormat::Rgba8Unorm,
                &color,
            ),
        );
    }
    Arc::new(textures)
}
fn draw_focus(layers: &[UserLayer], context: &mut PreRenderContext, target: &wgpu::Texture) {
    let view = target.create_view(&Default::default());
    let mut pass = context.begin_pass(
        TextureTarget {
            view: &view,
            kind: TextureTargetKind::Screen,
        },
        None,
        "Shared atlas draw",
    );
    pass.clear(Some(UnormColor::BLACK), None, None);
    layers[1].render(&mut pass, &Default::default(), 0, PassKind::Transparent);
}
fn colored(pixels: &[u8], channel: usize) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| p[channel] > 200 && p[2 - channel] < 5)
        .count()
}

#[test]
#[ignore = "supplied original/1.03 ROMs and offscreen Metal; no audio/window/save writes"]
fn reference_shared_effects_atlas_identity_publication_and_retirement() {
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut cases = 0;
    for updated in [false, true] {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("umineko_switch_files/data.rom"))
            .unwrap();
        if updated {
            io.try_with_rom(root.join("umineko_switch_files/patch.rom"))
                .unwrap();
        }
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
        let handles: Vec<EffectsAtlasBorrow> = shin_tasks::block_on(futures::future::join_all(
            (0..16).map(|_| server.effects_atlas()),
        ))
        .into_iter()
        .map(Result::unwrap)
        .collect();
        let first = handles[0].reference_identity();
        let original_weak = handles[0].reference_weak();
        assert!(handles.iter().all(|h| h.reference_identity() == first));
        assert_eq!(original_weak.strong_count(), 1);
        assert_eq!(server.reference_requested_paths(), ["/effects.txa"]);
        assert!(
            handles[0].read().get("Sakura").is_none(),
            "Supplied atlas lacks exact Sakura"
        );
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shared atlas depth"),
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
        let view = depth.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            device: &device,
            queue: &queue,
            breakup_texture: None,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };
        // Real provided texture render/update/clone checks; no GPU pipeline changes.
        for mode in 0..4 {
            let mut rain =
                shin_tasks::block_on(rain_layer::RainLayer::load(&server, mode, 100, 1500));
            assert_eq!(rain.reference_constructor_state().0, first);
            rain.assert_reference_render(&mut context, &server);
        }
        let mut focus =
            shin_tasks::block_on(focus_line_layer::FocusLineLayer::load(&server, -200, 100));
        focus.assert_reference_render(&mut context, &server);
        for mode in 0..6 {
            let mut animation =
                shin_tasks::block_on(sakura_layer::SakuraLayer::load(&server, mode)).unwrap();
            assert_eq!(animation.reference_constructor_state().0, first);
            animation.assert_reference_render(&mut context, &server, mode);
        }
        assert_eq!(
            original_weak.strong_count(),
            1,
            "Last particle destruction does not retire engine atlas"
        );
        let prepared_animation =
            shin_tasks::block_on(sakura_layer::SakuraLayer::prepare(&server, 0)).unwrap();
        let mut focus = focus_line_layer::FocusLineLayer::new(handles[0].clone(), -200, 100);
        focus.reference_owner_rays_mut().rays.truncate(1);
        focus.reference_owner_rays_mut().rays[0]
            .alpha
            .fast_forward_to(1.0);
        focus.reference_owner_rays_mut().rays[0].angle = 0.0;
        let mut layers: Vec<UserLayer> = vec![
            rain_layer::RainLayer::new(handles[0].clone(), 0, 100, 1500).into(),
            focus.into(),
            prepared_animation.construct().into(),
        ];
        let copies: Vec<_> = layers
            .iter()
            .map(|l| l.render_clone(&mut context.render_clone_ctx()))
            .collect();
        assert!(layers.iter().chain(&copies).all(|l| identity(l) == first));
        assert_eq!(original_weak.strong_count(), 1);
        // Exercise the same factory/publication API as the real boot worker,
        // including replacement of a still-live manager with live borrowers.
        let initialized = shin_tasks::block_on(server.initialize_effects_atlas()).unwrap();
        let initialized_id = initialized.reference_identity();
        let initialized_weak = initialized.reference_weak();
        assert_ne!(initialized_id, first);
        assert_eq!(original_weak.strong_count(), 0);
        assert!(
            layers
                .iter()
                .chain(&copies)
                .all(|l| identity(l) == initialized_id)
        );
        assert_eq!(initialized_weak.strong_count(), 1);
        let red = synthetic(&device, &queue, [255, 0, 0, 255]);
        let red_weak = Arc::downgrade(&red);
        server.reference_effects_atlas().publish(red);
        assert_eq!(
            initialized_weak.strong_count(),
            0,
            "Replaced supplied atlas retires with live prepared/layer/copy borrowers"
        );
        let red_id = handles[0].reference_identity();
        assert_ne!(red_id, first);
        assert!(layers.iter().chain(&copies).all(|l| identity(l) == red_id));
        assert!(handles[0].read().get("unknown-authored-name").is_some());
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shared atlas replacement pixels"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        draw_focus(&layers, &mut context, &target);
        let red_pixels = movie_layer::readback_test_texture(&mut context, &target);
        assert!(colored(&red_pixels, 0) > 1000);
        assert_eq!(colored(&red_pixels, 2), 0);
        let blue = synthetic(&device, &queue, [0, 0, 255, 255]);
        let blue_weak = Arc::downgrade(&blue);
        server.reference_effects_atlas().publish(blue);
        assert_eq!(red_weak.strong_count(), 0);
        let blue_id = handles[0].reference_identity();
        assert_ne!(blue_id, red_id);
        assert!(layers.iter().chain(&copies).all(|l| identity(l) == blue_id));
        draw_focus(&copies, &mut context, &target);
        // Retire actual atlas CPU resources before submitting already encoded
        // draws. wgpu commands must retain bindings until execution completes.
        server.reference_effects_atlas().clear();
        assert_eq!(blue_weak.strong_count(), 0);
        let blue_pixels = movie_layer::readback_test_texture(&mut context, &target);
        assert_eq!(colored(&blue_pixels, 0), 0);
        assert_eq!(colored(&blue_pixels, 2), colored(&red_pixels, 0));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handles[0].read())).is_err()
        );
        // Republish through the actual async loader; surviving weak borrowers
        // observe the new resource, including factories prepared before clear.
        let restored = shin_tasks::block_on(server.effects_atlas()).unwrap();
        let restored_weak = restored.reference_weak();
        let restored_id = restored.reference_identity();
        assert!(
            layers
                .iter()
                .chain(&copies)
                .all(|l| identity(l) == restored_id)
        );
        let rebuilt = prepared_animation.construct();
        assert_eq!(rebuilt.reference_constructor_state().0, restored_id);
        assert_eq!(restored_weak.strong_count(), 1);
        drop(rebuilt);
        assert_eq!(
            server.reference_requested_paths(),
            ["/effects.txa", "/effects.txa", "/effects.txa"]
        );
        drop(context);
        dynamic.finish(&mut encoder);
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU work must complete before readback or retirement");
        dynamic.recall();
        drop(server);
        assert_eq!(
            restored_weak.strong_count(),
            0,
            "Engine retirement frees actual atlas despite live layers/copies"
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| restored.read())).is_err()
        );
        layers.clear();
        drop(copies);
        drop(handles);
        drop(prepared_animation);
        drop(restored);
        cases += 1;
    }
    println!(
        "Shared effects atlas: {cases} ROM versions; 32 concurrent borrowers; all three actual particle layers/copies/factories; real atlas Weak retirement; red/blue replacement and encoded GPU work after CPU clear; no audio/window/saves"
    );
}
