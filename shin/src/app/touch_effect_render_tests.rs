#[test]
#[ignore = "requires supplied ROM and Metal; no audio is opened"]
fn reference_touch_effect_native_art_and_mouse_render() {
    use crate::{
        asset::system::{cache::AssetCache, AssetLoadContext, AssetServer, LayeredAssetIo},
        render::PreRenderContext,
    };
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    use shin_render::quad_vertices::QuadVertices;
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let parent = std::path::Path::new(&rom).parent().unwrap();
    let original = std::fs::read(parent.join("uncompressed_executable")).unwrap();
    let update = std::fs::read(parent.join("uncompressed_executable_update")).unwrap();
    // Verify the recovered integration contracts exist in both executables;
    // this is a code audit, not a native GPU/scheduler equivalence claim.
    for (start, end) in [
        (0x1154a0, 0x1154a8),
        (0x1155e8, 0x115608),
        (0x115db4, 0x115dd4),
        (0x11615c, 0x116194),
    ] {
        let expected = &original[start + 0x100..end + 0x100];
        assert!(
            update.windows(expected.len()).any(|w| w == expected),
            "native contract {start:x}"
        );
    }
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(&rom).unwrap();
    let server = std::sync::Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let atlas: std::sync::Arc<CharacterTextures> =
        shin_tasks::block_on(server.load("/systex.txa")).unwrap();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Touch star overlay"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Touch overlay stencil"),
        size: target.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
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
    check_native_touch_star_packets(&mut ctx, &target, &view, &atlas, std::path::Path::new(&rom));
    let draw_on_background =
        |effect: &TouchEffect, ctx: &mut PreRenderContext, stencil, background| {
            {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(ctx.depth_stencil),
                    "Touch effect regression",
                );
                // The live surface always attaches depth/stencil. Earlier tests
                // omitted it, hiding a stencil comparison that rejected all stars.
                pass.clear(Some(background), Some(stencil), Some(0.0));
                effect.render(&mut pass, &atlas);
            }
            crate::layer::user::readback_test_texture(ctx, &target)
        };
    let draw_with_stencil = |effect: &TouchEffect, ctx: &mut PreRenderContext, stencil| {
        draw_on_background(effect, ctx, stencil, UnormColor::BLACK)
    };
    let draw =
        |effect: &TouchEffect, ctx: &mut PreRenderContext| draw_with_stencil(effect, ctx, 255);
    let mut effect = TouchEffect::with_seed(5489);
    let blank = draw(&effect, &mut ctx);
    effect.update(Ticks::ZERO, false, true, Vec2::new(450.0, 400.0));
    assert_eq!(draw(&effect, &mut ctx), blank);
    effect.update(Ticks::ZERO, true, true, Vec2::new(450.0, 400.0));
    effect.update(Ticks::ZERO, true, true, Vec2::new(470.0, 400.0));
    let clicked = draw(&effect, &mut ctx);
    // Story layers and UI clipping leave different stencil values behind.
    // Global stars must remain identical above all of them.
    for stencil in [0, 1, 64, 128] {
        assert_eq!(
            draw_with_stencil(&effect, &mut ctx, stencil),
            clicked,
            "Touch stars must ignore scene stencil {stencil}"
        );
    }
    let visible: Vec<_> = clicked
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, rgba)| rgba[..3].iter().any(|&v| v > 8))
        .map(|(i, _)| (i % 1920, i / 1920))
        .collect();
    assert!(
        visible.len() > 100,
        "Actual drag atlas must draw visible stars"
    );
    assert!(
        visible
            .iter()
            .all(|&(x, y)| (405..495).contains(&x) && (355..445).contains(&y)),
        "Native-sized stars centered at the mouse coordinate"
    );
    for step in 1..=15 {
        effect.update(
            Ticks::from_f32(1.0),
            true,
            true,
            Vec2::new(470.0 + step as f32 * 12.0, 400.0),
        );
    }
    assert!(effect.stars.len() > 40);
    let trail = draw(&effect, &mut ctx);
    assert_ne!(trail, clicked);
    // Every authored variant should be drawable with the atlas's 2x2 UVs.
    for variant in 0..4 {
        effect.stars.truncate(1);
        effect.stars[0].variant = variant;
        effect.stars[0].age = 0.0;
        effect.stars[0].position = Vec2::new(450.0, 400.0);
        effect.stars[0].angle = 0.0;
        effect.stars[0].scale = 0.75;
        let pixels = draw(&effect, &mut ctx);
        assert_ne!(pixels, blank, "Atlas variant {variant}");
        let center = (400 * 1920 + 450) * 4;
        let rgb = &pixels[center..center + 3];
        assert!(
            rgb.iter().max().unwrap() - rgb.iter().min().unwrap() > 10,
            "Switch movie capture retains the pastel fill on dark backgrounds"
        );
        let menu_pixels = draw_on_background(
            &effect,
            &mut ctx,
            255,
            UnormColor::from_rgba(183, 178, 144, 255),
        );
        assert_eq!(
            &menu_pixels[center..center + 3],
            &[255, 255, 255],
            "Native additive blending makes stars white on the bright main menu"
        );
        for (i, rgba) in pixels.chunks_exact(4).enumerate() {
            if rgba[0] > 8 {
                assert!((420..480).contains(&(i % 1920)));
                assert!((370..430).contains(&(i / 1920)));
            }
        }
    }
    effect.update(Ticks::from_f32(61.0), true, false, Vec2::ZERO);
    assert_eq!(draw(&effect, &mut ctx), blank);
    if let Some(path) = std::env::var_os("SHIN_TOUCH_TEST_PPM") {
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        ppm.extend(trail.chunks_exact(4).flat_map(|p| p[..3].iter().copied()));
        std::fs::write(path, ppm).unwrap();
    }
    // Program33 has a distinct shader and source-over blend. Verify its
    // recovered equation against independent bilinear source sampling.
    let native_fragment = &original[0x20e4f0 + 0x100..0x20e668 + 0x100];
    assert!(update
        .windows(native_fragment.len())
        .any(|w| w == native_fragment));
    let colors = [
        [32u8, 48, 64, 255],
        [96, 80, 112, 255],
        [64, 96, 80, 255],
        [128, 144, 128, 255],
    ];
    let image: Vec<u8> = colors.into_iter().flatten().collect();
    let source = shin_render::gpu_texture::GpuTexture::new_static_with_data(
        &device,
        &queue,
        Some("Tap ripple bilinear source"),
        winit::dpi::PhysicalSize::new(2, 2),
        shin_render::TEXTURE_FORMAT,
        &image,
    );
    let sample = |uv: Vec2| -> [f32; 4] {
        let t = (uv * 2.0 - Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE);
        std::array::from_fn(|channel| {
            let top = colors[0][channel] as f32 * (1.0 - t.x) + colors[1][channel] as f32 * t.x;
            let bottom = colors[2][channel] as f32 * (1.0 - t.x) + colors[3][channel] as f32 * t.x;
            (top * (1.0 - t.y) + bottom * t.y) / 255.0
        })
    };
    let mut tap = TouchEffect::with_seed(5489);
    tap.update(Ticks::ZERO, true, true, Vec2::new(960.0, 540.0));
    assert!(tap.stars.is_empty() && tap.ripples.is_empty());
    tap.update(Ticks::ZERO, true, false, Vec2::new(960.0, 540.0));
    assert_eq!(tap.ripples.len(), 1);
    for age in [0.25f32, 0.5, 0.75] {
        tap.ripples[0].age = age;
        let pixels = {
            let depth = ctx.depth_stencil;
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::RenderTexture,
                    view: &view,
                },
                Some(depth),
                "Native tap ripple equation",
            );
            pass.clear(Some(UnormColor::BLACK), Some(255), Some(1.0));
            QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .render_sprite(
                    &mut pass,
                    RenderRequestBuilder::new(),
                    source.as_source(),
                    crate::render::top_left_projection_matrix(),
                );
            tap.render_ripples(&mut pass, source.as_source());
            drop(pass);
            crate::layer::user::readback_test_texture(&mut ctx, &target)
        };
        let radius = 80.0 * (1.0 - (1.0 - age).powf(3.0));
        for y in (452..628).step_by(3) {
            for x in (872..1048).step_by(3) {
                let uv = Vec2::new((x as f32 + 0.5) / 1920.0, (y as f32 + 0.5) / 1080.0);
                let r = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - Vec2::new(960.0, 540.0))
                    .length()
                    / radius;
                let base = sample(uv);
                let mut expected = base;
                if r < 1.0 {
                    let wave = ((r - age) * std::f32::consts::TAU).sin();
                    let shifted = sample(uv + wave * Vec2::new(0.004, (1920.0 / 1080.0) * 0.004));
                    let opacity = (1.0 - r * r * r) * (1.0 - age * age);
                    let alpha = (shifted[3] + wave * 0.2) * opacity;
                    for c in 0..3 {
                        expected[c] = ((shifted[c] + wave * 0.2) * opacity
                            + base[c] * (1.0 - alpha))
                            .clamp(0.0, 1.0);
                    }
                }
                for c in 0..3 {
                    let got = pixels[(y * 1920 + x) * 4 + c] as i32;
                    let want = (expected[c] * 255.0).round() as i32;
                    assert!(
                        (got - want).abs() <= 2,
                        "Tap age{age} pixel{x},{y} channel{c}: {got} != {want}"
                    );
                }
            }
        }
    }
    tap.update(Ticks::from_f32(31.0), true, false, Vec2::new(960.0, 540.0));
    assert!(tap.ripples.is_empty() && tap.stars.is_empty());
    eprintln!(
        "Touch overlay: disabled/tap ripple/15 drag frames/4 variants/release expiry passed; {} click pixels",
        visible.len()
    );
}
