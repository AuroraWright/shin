use super::*;
use crate::render::PreRenderContext;

type NativeCase = (f32, f32, f32, f32, f32, [f32; 4], [f32; 4]);
include!("native_cases.rs");
#[test]
fn wave_uniforms_match_executed_native_metric_extent_and_radius_instructions() {
    for (progress, cx, cy, aspect, width, center, front) in NATIVE_CASES {
        let implementation = WaveWiperImpl {
            center: vec2(cx, cy),
            aspect,
            width,
        };
        let (actual_center, actual_front) = implementation.parameters(progress);
        assert_eq!(actual_center.to_array(), center);
        assert_eq!(actual_front.to_array(), front);
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_wave_factory_uniforms_shader_warp_blend_and_highlight() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 5]) * 4, 0x21c64);
    for (address, expected) in [
        (0x21c64, 0x1e2c1001u32),
        (0x21c68, 0x1e2e1003u32),
        (0x21c6c, 0x1e2a1004u32),
        (0x21c74, 0x4ea11c22u32),
        (0x21c80, 0x9403e7c8u32),
        (0x11bbec, 0xbd00100bu32),
        (0x11bbf0, 0xbd00140au32),
        (0x11bbf8, 0xbd001809u32),
        (0x11bc00, 0xbd001c08u32),
        (0x11bc3c, 0x17fffa75u32),
        (0x11a65c, 0xbd00268bu32),
        (0x11a660, 0xbd002a8au32),
        (0x11a664, 0xbd002e89u32),
        (0x11a670, 0xbd003288u32),
        (0x11a9dc, 0xaa1403e1u32),
        (0x11a9f0, 0xaa1503e1u32),
        (0x11a9fc, 0x528004c0u32),
        (0x11aa04, 0x320003e0u32),
        (0x11aa5c, 0x1e292000u32),
        (0x11aa6c, 0x1e201883u32),
        (0x11aaa0, 0x1e264ce2u32),
        (0x11aab0, 0x1e244c61u32),
        (0x11aac4, 0x1e21c020u32),
        (0x11aad8, 0x1e200900u32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
    for (offset, expected) in [
        (0x48, 0x4c58300c00070d0cu64),
        (0x50, 0x4c58300c00170e0bu64),
        (0x58, 0x4c68100c00270c0cu64),
        (0x68, 0x4c68100c00370b0bu64),
        (0x78, 0x59a0050000b70b0au64),
        (0x88, 0x5080000000870a09u64),
        (0x90, 0x4c59100c00470908u64),
        (0x98, 0x5c68100000170808u64),
        (0xa8, 0x5bb383800ff70807u64),
        (0xb0, 0x36b683bf8008080fu64),
        (0xb8, 0xd832008040e00d06u64),
        (0xc8, 0xd83200a020e10d00u64),
        (0x128, 0x1e240c90fdb70800u64),
        (0x130, 0x5080000000570a0au64),
        (0x168, 0x5080000000170200u64),
        (0x188, 0x4c68100c00570000u64),
        (0x190, 0x4c68100c00670000u64),
        (0x198, 0x32a004be80070009u64),
        (0x1c8, 0x51a0000c00170b0bu64),
        (0x1d0, 0x51a0008c00070c0cu64),
        (0x1d8, 0xd832008040b70c06u64),
        (0x1e8, 0xd83200a020b70c00u64),
        (0x1f0, 0x1e240490fdb70809u64),
        (0x238, 0x59a0030000070800u64),
        (0x268, 0x49a0000400070900u64),
        (0x278, 0x49a0010400070902u64),
    ] {
        let address = 0x219d70 + offset + 256;
        assert_eq!(
            u64::from_le_bytes(exe[address..address + 8].try_into().unwrap()),
            expected,
            "shader38+{offset:x}"
        );
    }
    assert_eq!(
        f32::from_le_bytes(exe[0x21a0f0 + 256..0x21a0f4 + 256].try_into().unwrap()),
        0.1
    );
}

#[test]
#[ignore = "requires GPU"]
fn reference_wave_gpu_native_radial_warp_linear_blend_and_rgb_highlight() {
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
        label: Some("Wave reference depth"),
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
        label: Some("Wave reference target"),
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
    let old = make_input("Wave old", false);
    let new = make_input("Wave new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    if std::env::var("SHIN_REFERENCE_ROM").is_ok() {
        let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
            shin_tasks::block_on(server.load("/main.snr")).unwrap();
        let loaded = shin_tasks::block_on(crate::wiper::AnyWiper::load(
            &server,
            &scenario,
            shin_core::vm::command::types::WiperType::Wave,
            Ticks::from_f32(10.0),
            (173, -3, 42, 9, 99, 123, 50, -1),
        ));
        let crate::wiper::AnyWiper::Wave(wiper) = loaded else {
            panic!("Wave factory routing")
        };
        assert_eq!(wiper.inner.center, vec2(0.5, 0.5));
        assert_eq!(wiper.inner.aspect, 1.0);
        assert_eq!(wiper.inner.width, 0.25);
    }
    // Exercise the installed variant's update/clone/fast-forward lifecycle.
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::primitives::update::FrameId;
    let mut timer = AnyWiper::Wave(WaveWiper::new(Ticks::from_f32(10.0)));
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
        let AnyWiper::Wave(wiper) = &timer else {
            panic!()
        };
        assert!((wiper.state.get_progress() - expected).abs() < 0.00001);
    }
    use shin_render::shaders::types::RenderClone;
    let mut copy = timer.render_clone(&mut context.render_clone_ctx());
    copy.fast_forward();
    assert!(!copy.is_running());
    assert!(timer.is_running());
    let mut checked = 0;
    // A zero metric makes the native shader's reciprocal undefined; verify
    // that case's uniforms above, and exercise all finite metrics here.
    for (progress, cx, cy, aspect, width, center, front) in
        NATIVE_CASES.into_iter().filter(|c| c.3 != 0.0)
    {
        let implementation = WaveWiperImpl {
            center: vec2(cx, cy),
            aspect,
            width,
        };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Wave reference",
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
                let u = (x as f64 + 0.5) / 64.0;
                let v = (y as f64 + 0.5) / 64.0;
                let dx = (u - f64::from(center[0])) * f64::from(center[2]);
                let dy = (v - f64::from(center[1])) * f64::from(center[3]);
                let radius = dx.hypot(dy);
                let phase = (f64::from(front[0]) - radius) / f64::from(front[1]);
                let (sample_u, sample_v, fade, highlight) = if phase <= 0.0 {
                    (u, v, 0.0, 0.0)
                } else if phase >= 1.0 {
                    (u, v, 1.0, 0.0)
                } else {
                    let displaced = radius
                        + (phase * f64::from(std::f32::consts::TAU)).sin()
                            * f64::from(width)
                            * f64::from(progress)
                            * 0.25;
                    (
                        f64::from(cx) + dx / radius * displaced / f64::from(center[2]),
                        f64::from(cy) + dy / radius * displaced / f64::from(center[3]),
                        phase,
                        (phase * f64::from(std::f32::consts::PI)).sin() * 0.1 * 255.0,
                    )
                };
                let tx = ((sample_u * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                let ty = ((sample_v * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                let old = [16.0 + tx * 2.0, 32.0 + ty * 2.0, 128.0, 160.0];
                let new = [170.0, 15.0 + tx, 40.0 + ty * 2.0, 128.0];
                let expected: [f32; 4] = std::array::from_fn(|c| {
                    old[c] * (1.0 - fade as f32)
                        + new[c] * fade as f32
                        + if c < 3 { highlight as f32 } else { 0.0 }
                });
                let pixel = &pixels[((y * 64 + x) * 4) as usize..][..4];
                for channel in 0..4 {
                    assert!(
                        (f32::from(pixel[channel]) - expected[channel]).abs() <= 1.5,
                        "p{progress} center{cx},{cy} aspect{aspect} width{width} at{x},{y}: {pixel:?}, expected{expected:?}"
                    );
                }
                checked += 1;
            }
        }
    }
    println!(
        "Wave native GPU oracle: {checked} RGBA pixels,42 metric/center/width/phase cases; radial distortion, linear fade, RGB-only highlight, opaque output and native parameter conversion pass"
    );
}
