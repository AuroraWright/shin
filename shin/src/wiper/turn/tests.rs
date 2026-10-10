use super::*;
use crate::render::PreRenderContext;

type NativeCase = (f32, i32, f32, u8);
include!("native_cases.rs");
#[test]
fn turn_direction_angle_and_alpha_match_executed_native_branches() {
    for (progress, direction, angle, alpha) in NATIVE_CASES {
        assert_eq!(rotation_and_opacity(direction, progress), (angle, alpha));
        // Independent scalar projection of the native centered quad. This
        // checks the matrix composition, Y inversion and perspective divide.
        let (sine, cosine) = (f64::from(angle).sin(), f64::from(angle).cos());
        let focal = 1.0 / (f64::from(0.99026656f32) * 0.5).tan();
        for x in [-960.0f32, 0.0, 960.0] {
            for y in [-540.0f32, 0.0, 540.0] {
                let clip = turn_transform(angle) * glam::vec4(x, y, 0.0, 1.0);
                let w = 1000.0 + f64::from(x) * sine;
                let expected = [
                    focal / f64::from(1920.0f32 / 1080.0) * f64::from(x) * cosine / w,
                    -focal * f64::from(y) / w,
                ];
                for i in 0..2 {
                    assert!((f64::from(clip[i] / clip.w) - expected[i]).abs() < 0.00002);
                }
            }
        }
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_turn_factory_direction_draw_order_and_perspective() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 4]) * 4, 0x219c8);
    for (address, expected) in [
        (0x219c8, 0xb9400280u32),
        (0x219d8, 0x9403e82eu32),
        (0x11babc, 0xb9001013u32),
        (0x11bb04, 0xb9401000u32),
        (0x11bb08, 0x17ffec12u32),
        (0x116b8c, 0xb90026b3u32),
        (0x116ea4, 0x340000c8u32),
        (0x116eac, 0x1e28380bu32),
        (0x116eb0, 0xaa1403f3u32),
        (0x116eb4, 0xaa1503f4u32),
        (0x116ec0, 0x4ea81d0bu32),
        (0x116ec4, 0x1e202908u32),
        (0x116f68, 0x321f03e0u32),
        (0x116f70, 0x320003e0u32),
        (0x116f7c, 0xaa1403e1u32),
        (0x116fb0, 0x4eab1d63u32),
        (0x116fe0, 0x97fcfddcu32),
        (0x11703c, 0x97fe2b29u32),
        (0x117054, 0x97fe2bf7u32),
        (0x117098, 0x97fe2bf6u32),
        (0x1170d8, 0x1e200900u32),
        (0x1170dc, 0x97fe2b95u32),
        (0x117120, 0x321f03e0u32),
        (0x117128, 0x321f03e0u32),
        (0x117134, 0xaa1303e1u32),
        (0xa1d58, 0xb9002e68u32),
        (0xa1d64, 0xbd002a61u32),
        (0xa1d78, 0x1e201820u32),
        (0xa1f88, 0x1e214121u32),
        (0xa1f8c, 0xbd000a61u32),
        (0xa1f90, 0xbd002269u32),
        (0x5685c, 0x6ea1b800u32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
    for (address, expected) in [
        (0x159a88, 0.99026656f32),
        (0x15973c, 1920.0 / 1080.0),
        (0x159678, 10000.0),
        (0x159840, -1000.0),
        (0x159684, std::f32::consts::FRAC_PI_2),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}

#[test]
#[ignore = "requires GPU"]
fn reference_turn_gpu_native_direction_branches_quantized_alpha_and_uncovered_pixels() {
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
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Ok(rom) = std::env::var("SHIN_REFERENCE_ROM") {
        io.try_with_rom(rom).unwrap();
    }

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
    let old = make_input("Turn old", false);
    let new = make_input("Turn new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    if std::env::var("SHIN_REFERENCE_ROM").is_ok() {
        let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
            shin_tasks::block_on(server.load("/main.snr")).unwrap();
        let loaded = shin_tasks::block_on(crate::wiper::AnyWiper::load(
            &server,
            &scenario,
            shin_core::vm::command::types::WiperType::Turn,
            Ticks::from_f32(10.0),
            (-1, 0, 0, 0, 0, 0, 0, 0),
        ));
        let crate::wiper::AnyWiper::Turn(wiper) = loaded else {
            panic!("Turn factory routing")
        };
        assert_eq!(wiper.inner.direction, -1);
    }
    // Exercise the installed variant's update/clone/fast-forward lifecycle.
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::primitives::update::FrameId;
    let mut timer = AnyWiper::Turn(TurnWiper::new(Ticks::from_f32(10.0), -1));
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
        let AnyWiper::Turn(wiper) = &timer else {
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
    for (progress, direction, angle, packed_alpha) in NATIVE_CASES {
        let implementation = TurnWiperImpl { direction };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Turn reference",
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
                let background = if direction == 0 {
                    [16.0 + x as f32 * 2.0, 32.0 + y as f32 * 2.0, 128.0, 160.0]
                } else {
                    [170.0, 15.0 + x as f32, 40.0 + y as f32 * 2.0, 128.0]
                };
                let mut expected = background;
                // Intersect the screen ray with the rotated image plane;
                // do not invert or reuse the implementation's matrix.
                let (sine, cosine) = (f64::from(angle).sin(), f64::from(angle).cos());
                let focal = 1.0 / (f64::from(0.99026656f32) * 0.5).tan();
                let qx = (f64::from(x) + 0.5) / 32.0 - 1.0;
                let qy = (f64::from(y) + 0.5) / 32.0 - 1.0;
                let object_x =
                    1000.0 * qx / (focal / f64::from(1920.0f32 / 1080.0) * cosine - qx * sine);
                let distance = 1000.0 + object_x * sine;
                let object_y = qy * distance / focal;
                if distance >= 1.0
                    && (-960.0..960.0).contains(&object_x)
                    && (-540.0..540.0).contains(&object_y)
                {
                    let tx = (((object_x / 1920.0 + 0.5) * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                    let ty = (((object_y / 1080.0 + 0.5) * 64.0 - 0.5) as f32).clamp(0.0, 63.0);
                    let overlay = if direction == 0 {
                        [170.0, 15.0 + tx, 40.0 + ty * 2.0, 128.0]
                    } else {
                        [16.0 + tx * 2.0, 32.0 + ty * 2.0, 128.0, 160.0]
                    };
                    let alpha = (overlay[3] / 255.0) * (f32::from(packed_alpha) / 255.0);
                    for c in 0..3 {
                        expected[c] = overlay[c] * alpha + expected[c] * (1.0 - alpha);
                    }
                    expected[3] = 255.0 * alpha + expected[3] * (1.0 - alpha);
                }
                let pixel = &pixels[((y * 64 + x) * 4) as usize..][..4];
                for channel in 0..4 {
                    assert!(
                        (f32::from(pixel[channel]) - expected[channel]).abs() <= 1.5,
                        "p{progress} direction{direction} at{x},{y}: {pixel:?}, expected{expected:?}"
                    );
                }
                checked += 1;
            }
        }
    }
    println!(
        "Turn native GPU oracle: {checked} RGBA pixels,32 direction/phase cases; perspective ray-plane UVs, swapped textures, quantized opacity and native nonzero direction behavior pass"
    );
}
