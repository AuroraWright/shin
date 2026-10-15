use super::*;
use crate::render::PreRenderContext;

type NativeCase = (f32, f32, f32, f32, f32, f32, f32, [f32; 4], [f32; 4]);
include!("native_cases.rs");
#[test]
fn ripple_uniforms_match_native_inverse_width_cosine_envelope_and_phase() {
    for (progress, cx, cy, aspect, width, amplitude, phase, center, ripple) in NATIVE_CASES {
        let implementation = RippleWiperImpl {
            center: vec2(cx, cy),
            aspect,
            width,
            amplitude,
            phase_rate: 1.0 / 60.0,
            phase,
        };
        let (actual_center, actual_ripple) = implementation.parameters(progress);
        assert_eq!(actual_center.to_array(), center);
        assert_eq!(actual_ripple.to_array(), ripple);
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_ripple_factory_phase_clock_envelope_shader_warp_and_blend() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 7]) * 4, 0x21b8c);
    for (address, expected) in [
        (0x21b90, 0xbd497505u32),
        (0x21b98, 0x1e2c1001u32),
        (0x21b9c, 0x1e2e1003u32),
        (0x21ba0, 0x1e2a1004u32),
        (0x21ba4, 0xbd479906u32),
        (0x21bb8, 0x9403e842u32),
        (0x11bd34, 0xbd002009u32),
        (0x11bd3c, 0xbd002408u32),
        (0x11bd80, 0x17fed3b4u32),
        (0xd0ca4, 0x1e281800u32),
        (0xd0cb0, 0x2d04b28du32),
        (0xd0cb4, 0x2d05aa8bu32),
        (0xd0cbc, 0x2d068289u32),
        (0xd0cd8, 0xb9003e9fu32),
        (0xd10a4, 0x360000b4u32),
        (0xd10ac, 0x1e280800u32),
        (0xd10b0, 0x1e202820u32),
        (0xd10b4, 0xbd003e60u32),
        (0xd111c, 0x1e202008u32),
        (0xd1120, 0x54000060u32),
        (0xd1128, 0x1e201820u32),
        (0xd1138, 0x1e283920u32),
        (0xd1148, 0x1e204d00u32),
        (0xd114c, 0x1e210800u32),
        (0xd1150, 0x94012fc4u32),
        (0xd1154, 0x1e203920u32),
        (0xd115c, 0x1e200823u32),
        (0xd1168, 0x528004e0u32),
        (0xd1178, 0x320003e0u32),
        (0xd11c4, 0xaa1403e1u32),
        (0xd11d4, 0xaa1303e1u32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
    for (offset, expected) in [
        (0x58, 0x4c58300c00070000u64),
        (0x68, 0x4c58300c00170101u64),
        (0x70, 0x4c68100c00270000u64),
        (0x78, 0x4c68100c00370101u64),
        (0x90, 0x59a0010000170102u64),
        (0x98, 0x5080000000870204u64),
        (0xa8, 0x51a2030c00670403u64),
        (0xb8, 0x4c68100c00470303u64),
        (0xc8, 0x1e240c90fdb70303u64),
        (0xd8, 0x5080000000570203u64),
        (0xe8, 0x5080000000170705u64),
        (0xf8, 0x49a0020c00570505u64),
        (0x128, 0x51a0030c00070008u64),
        (0x130, 0x51a0048c00170409u64),
        (0x138, 0xd832008040970806u64),
        (0x148, 0xd83200a020970800u64),
        (0x150, 0x49a1030c00770606u64),
        (0x178, 0x49a0030c00770000u64),
        (0x188, 0x49a0038c00770101u64),
        (0x190, 0x49a0020c00770202u64),
        (0x198, 0x49a0028c00770303u64),
    ] {
        let address = 0x2182f0 + offset + 256;
        assert_eq!(
            u64::from_le_bytes(exe[address..address + 8].try_into().unwrap()),
            expected,
            "shader39+{offset:x}"
        );
    }
    for (address, expected) in [
        (0x159974, 0.2f32),
        (0x159798, 60.0),
        (0x159880, f32::from_bits(0x40490fd0)),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}

#[test]
#[ignore = "requires GPU"]
fn reference_ripple_gpu_native_radial_warp_time_phase_envelope_and_rgba_blend() {
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
        label: Some("Ripple reference depth"),
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
        label: Some("Ripple reference target"),
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
    let old = make_input("Ripple old", false);
    let new = make_input("Ripple new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    if std::env::var("SHIN_REFERENCE_ROM").is_ok() {
        let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
            shin_tasks::block_on(server.load("/main.snr")).unwrap();
        let loaded = shin_tasks::block_on(crate::wiper::AnyWiper::load(
            &server,
            &scenario,
            shin_core::vm::command::types::WiperType::Ripple,
            Ticks::from_f32(10.0),
            (173, -3, 42, 9, 99, 123, 50, -1),
        ));
        let crate::wiper::AnyWiper::Ripple(wiper) = loaded else {
            panic!("Ripple factory routing")
        };
        assert_eq!(wiper.inner.center, vec2(0.5, 0.5));
        assert_eq!(wiper.inner.aspect, 1.0);
        assert_eq!(wiper.inner.width, 0.25);
        assert_eq!(wiper.inner.amplitude, 0.2);
        assert_eq!(wiper.inner.phase_rate, 1.0 / 60.0);
        assert_eq!(wiper.inner.phase, 0.0);
    }
    // Exercise the installed variant's update/clone/fast-forward lifecycle.
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::primitives::update::FrameId;
    let mut timer = AnyWiper::Ripple(RippleWiper::new(Ticks::from_f32(10.0)));
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
        let AnyWiper::Ripple(wiper) = &timer else {
            panic!()
        };
        assert!((wiper.state.get_progress() - expected).abs() < 0.00001);
        assert!((wiper.inner.phase - expected * 10.0 / 60.0).abs() < 0.000001);
        let phase = wiper.inner.phase;
        timer.update(&AdvUpdateContext {
            frame_id: lifecycle_frame,
            delta_ticks: Ticks::from_f32(60.0),
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        let AnyWiper::Ripple(wiper) = &timer else {
            panic!()
        };
        assert_eq!(wiper.inner.phase, phase);
        assert!((wiper.state.get_progress() - expected).abs() < 0.00001);
    }
    use shin_render::shaders::types::RenderClone;
    let mut copy = timer.render_clone(&mut context.render_clone_ctx());
    copy.fast_forward();
    assert!(!copy.is_running());
    assert!(timer.is_running());
    let AnyWiper::Ripple(original) = &timer else {
        panic!()
    };
    let original_phase = original.inner.phase;
    let AnyWiper::Ripple(cloned) = &copy else {
        panic!()
    };
    assert_eq!(cloned.inner.phase, original_phase);
    copy.update(&AdvUpdateContext {
        frame_id: lifecycle_frame,
        delta_ticks: Ticks::from_f32(60.0),
        asset_server: &server,
        device: &device,
        queue: &queue,
        are_animations_allowed: true,
        voice_callback: None,
        movie_volume_settings: Default::default(),
    });
    let AnyWiper::Ripple(cloned) = &copy else {
        panic!()
    };
    assert!((cloned.inner.phase - (original_phase + 1.0)).abs() < 0.000001);
    let AnyWiper::Ripple(original) = &timer else {
        panic!()
    };
    assert_eq!(original.inner.phase, original_phase);
    let mut checked = 0;
    // Zero metric has native undefined reciprocal sampling; verify its
    // uniforms above and exercise every finite metric here, including width0.
    for (progress, cx, cy, aspect, width, amplitude, phase, center, ripple) in
        NATIVE_CASES.into_iter().filter(|c| c.3 != 0.0)
    {
        let implementation = RippleWiperImpl {
            center: vec2(cx, cy),
            aspect,
            width,
            amplitude,
            phase_rate: 1.0 / 60.0,
            phase,
        };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Ripple reference",
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
                let angle = (2.0 * radius - f64::from(ripple[2]))
                    * f64::from(ripple[0])
                    * f64::from(std::f32::consts::TAU);
                let displaced = radius + angle.sin() * f64::from(ripple[1]);
                let sample_u = f64::from(cx) + dx / radius * displaced / f64::from(center[2]);
                let sample_v = f64::from(cy) + dy / radius * displaced / f64::from(center[3]);
                let tx = ((sample_u * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                let ty = ((sample_v * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                let old = [16.0 + tx * 2.0, 32.0 + ty * 2.0, 128.0, 160.0];
                let new = [170.0, 15.0 + tx, 40.0 + ty * 2.0, 128.0];
                let expected: [f32; 4] = std::array::from_fn(|c| {
                    (old[c] * (1.0 - progress) + new[c] * progress).clamp(0.0, 255.0)
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
        "Ripple native GPU oracle: {checked} RGBA pixels,42 metric/center/width/time/phase cases; radial oscillation, cosine amplitude envelope, linear RGBA fade, opaque output and permitted phase clock pass"
    );
}
