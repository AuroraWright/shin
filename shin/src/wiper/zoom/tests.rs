use super::*;
use crate::render::PreRenderContext;

type NativeCase = (f32, i32, f32, f32, u8);
include!("native_cases.rs");
#[test]
fn zoom_geometry_matches_executed_native_float_instructions() {
    for (progress, raw, incoming, outgoing, alpha) in NATIVE_CASES {
        let parameter = raw as f32 * 0.001;
        assert_eq!(scales(parameter, parameter, progress), (incoming, outgoing));
        assert_eq!((progress * 255.0) as u8, alpha);
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_zoom_factory_scales_draw_order_and_alpha_packing() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 3]) * 4, 0x21b08);
    for (address, expected) in [
        (0x21b08, 0x2d400a80u32),
        (0x21b10, 0x5e21d800u32),
        (0x21b18, 0x1e230801u32),
        (0x21b1c, 0x5e21d840u32),
        (0x21b20, 0x1e230802u32),
        (0x21b30, 0x9403e7f8u32),
        (0x11c250, 0xbd002689u32),
        (0x11c254, 0xbd002a88u32),
        (0x11c588, 0x1e242821u32),
        (0x11c58c, 0x1e210941u32),
        (0x11c598, 0x1e28282bu32),
        (0x11c59c, 0x1e242800u32),
        (0x11c5a0, 0x1e2a3901u32),
        (0x11c5a4, 0x1e200820u32),
        (0x11c5ac, 0x1e282809u32),
        (0x11c6bc, 0x321f03e0u32),
        (0x11c6c4, 0x320003e0u32),
        (0x11c6ec, 0x4ea81d00u32),
        (0x11c6f0, 0x4eaa1d41u32),
        (0x11c7a4, 0x321f03e0u32),
        (0x11c7ac, 0x321f03e0u32),
        (0x5685c, 0x6ea1b800u32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
    for (address, expected) in [
        (0x159888, -960.0f32),
        (0x159624, 960.0),
        (0x15954c, 540.0),
        (0x15988c, -540.0),
        (0x159a58, 0.001),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}

#[test]
#[ignore = "requires GPU"]
fn reference_zoom_gpu_native_scale_pairs_quantized_alpha_and_uncovered_pixels() {
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache};
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
        },
    };
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
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
    if let Ok(rom) = std::env::var("SHIN_REFERENCE_ROM") {
        io.try_with_rom(rom).unwrap();
    }

    let server = std::sync::Arc::new(AssetServer::new(
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
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(64, 64)));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference depth"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
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
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), format);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        breakup_texture: None,
        device: &device,
        queue: &queue,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };

    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureSampler, TextureSource};
    let make_input = |label, incoming: bool| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut pixels = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                pixels.extend_from_slice(&if incoming {
                    [170, 15 + x, 40 + y * 2, 128]
                } else {
                    [16 + x * 2, 32 + y * 2, 128, 160]
                });
            }
        }
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(64),
            },
            texture.size(),
        );
        texture
    };
    let old = make_input("Zoom old", false);
    let new = make_input("Zoom new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    if std::env::var("SHIN_REFERENCE_ROM").is_ok() {
        let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
            shin_tasks::block_on(server.load("/main.snr")).unwrap();
        let loaded = shin_tasks::block_on(crate::wiper::AnyWiper::load(
            &server,
            &scenario,
            shin_core::vm::command::types::WiperType::Zoom,
            Ticks::from_f32(10.0),
            (500, 2000, 0, 0, 0, 0, 0, 0),
        ));
        let crate::wiper::AnyWiper::Zoom(wiper) = loaded else {
            panic!("Zoom factory routing")
        };
        assert_eq!(wiper.inner.incoming_scale, 0.5);
        assert_eq!(wiper.inner.outgoing_scale, 2.0);
    }
    // Exercise the installed variant's update/clone/fast-forward lifecycle.
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::primitives::update::FrameId;
    let mut timer = AnyWiper::Zoom(ZoomWiper::new(Ticks::from_f32(10.0), 500, 2000));
    let mut lifecycle_frame = FrameId::default();
    for (delta, allowed, expected) in [(2.0, true, 0.2), (9.0, false, 0.2), (-1.0, true, 0.1)] {
        lifecycle_frame.advance();
        timer.update(&AdvUpdateContext {
            frame_id: lifecycle_frame,
            delta_ticks: Ticks::from_f32(delta),
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: allowed,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        let AnyWiper::Zoom(wiper) = &timer else {
            panic!()
        };
        assert!((wiper.state.get_progress() - expected).abs() < 0.00001);
    }
    use shin_render::shaders::types::RenderClone;
    let mut copy = timer.render_clone(&mut context.render_clone_ctx());
    copy.fast_forward();
    assert!(!copy.is_running());
    assert!(timer.is_running());
    let cases = NATIVE_CASES;
    let mut checked = 0;
    for progress in [0.0, 0.125, 0.25, 0.5, 0.75, 1.0] {
        for incoming in [-1000, 0, 500, 1000, 1375, 2000, 3000] {
            for outgoing in [-1000, 0, 500, 1000, 1375, 2000, 3000] {
                let old_case = cases
                    .iter()
                    .find(|c| c.0 == progress && c.1 == outgoing)
                    .unwrap();
                let new_case = cases
                    .iter()
                    .find(|c| c.0 == progress && c.1 == incoming)
                    .unwrap();
                let implementation = ZoomWiperImpl {
                    incoming_scale: incoming as f32 * 0.001,
                    outgoing_scale: outgoing as f32 * 0.001,
                };
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Zoom reference",
                    );
                    pass.clear(
                        Some(UnormColor::from_rgba(13, 29, 41, 73)),
                        Some(0),
                        Some(1.0),
                    );
                    implementation.render(
                        &mut pass,
                        RenderRequestBuilder::new().depth_stencil_shorthand(1, true, false),
                        TextureSource {
                            view: &new_view,
                            sampler: TextureSampler::Linear,
                        },
                        TextureSource {
                            view: &old_view,
                            sampler: TextureSampler::Linear,
                        },
                        progress,
                    );
                }
                let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
                for y in 0..64 {
                    for x in 0..64 {
                        let mut expected = [13.0, 29.0, 41.0, 73.0];
                        let sample = |scale: f32| {
                            if scale == 0.0 {
                                return None;
                            }
                            let u = ((x as f32 + 0.5) / 64.0 - 0.5) / scale + 0.5;
                            let v = ((y as f32 + 0.5) / 64.0 - 0.5) / scale + 0.5;
                            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                                return None;
                            }
                            Some((
                                (u * 64.0 - 0.5).clamp(0.0, 63.0),
                                (v * 64.0 - 0.5).clamp(0.0, 63.0),
                            ))
                        };
                        if let Some((ox, oy)) = sample(old_case.3) {
                            expected = [16.0 + ox * 2.0, 32.0 + oy * 2.0, 128.0, 160.0];
                        }
                        if let Some((nx, ny)) = sample(new_case.2) {
                            let alpha = (128.0 / 255.0) * (f32::from(new_case.4) / 255.0);
                            for (channel, value) in
                                [170.0, 15.0 + nx, 40.0 + ny * 2.0].into_iter().enumerate()
                            {
                                expected[channel] =
                                    value * alpha + expected[channel] * (1.0 - alpha);
                            }
                            expected[3] = 255.0 * alpha + expected[3] * (1.0 - alpha);
                        }
                        let pixel = &pixels[((y * 64 + x) * 4) as usize..][..4];
                        for channel in 0..4 {
                            assert!(
                                (f32::from(pixel[channel]) - expected[channel]).abs() <= 1.5,
                                "p{progress} in{incoming} out{outgoing} at{x},{y}: {pixel:?}, expected{expected:?}"
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    println!(
        "Zoom native GPU oracle: {checked} RGBA pixels,294 scale-pair/phase cases; centered/mirrored UVs, zero scales, opacity truncation and retained uncovered pixels pass"
    );
}
