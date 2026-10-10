use super::*;
use crate::render::PreRenderContext;

type NativeCase = (f32, i32, f32, [f32; 2]);
include!("native_cases.rs");
#[test]
fn scanline_progress_offsets_and_all_strip_vertices_match_native() {
    for (progress, exponent, shaped, offsets) in NATIVE_CASES {
        assert_eq!(shaped_progress(progress, exponent as f32), shaped);
        assert_eq!(strip_offsets(progress, exponent as f32), offsets);
    }
    for (strip, position, uv) in NATIVE_GEOMETRY {
        assert_eq!(strip_geometry(strip, 0.0), (position, uv));
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_scanline_factory_power_branches_strip_loop_and_texture_modulation() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 6]) * 4, 0x21cdc);
    for (address, expected) in [
        (0x21cdc, 0xbd400280u32),
        (0x21ce0, 0x5e21d801u32),
        (0x21cf0, 0x9403e7d4u32),
        (0x11bc8c, 0xbd001008u32),
        (0x11bcb4, 0x17ff021fu32),
        (0xdc568, 0xbd002688u32),
        (0xdc7a8, 0x1e202028u32),
        (0xdc7ac, 0x540000e5u32),
        (0xdc7b4, 0x1e2a3900u32),
        (0xdc7b8, 0x1e214021u32),
        (0xdc7bc, 0x94010121u32),
        (0xdc7c0, 0x1e20390au32),
        (0xdc7c8, 0x5400008du32),
        (0xdc7d0, 0x9401011cu32),
        (0xdc818, 0x321d03e0u32),
        (0xdc820, 0x320003e0u32),
        (0xdc82c, 0xaa1403e1u32),
        (0xdc898, 0xaa1303e1u32),
        (0xdc8a8, 0x1e280940u32),
        (0xdc8ac, 0x1e20390bu32),
        (0xdc8bc, 0x1e21280au32),
        (0xdc8d4, 0x7200027fu32),
        (0xdc8d8, 0x1e2b0d41u32),
        (0xdc8e4, 0x1e282821u32),
        (0xdc8ec, 0x1e291800u32),
        (0xdc8f8, 0x1e2d2800u32),
        (0xdc938, 0x11000ab5u32),
        (0xdc93c, 0x7108727fu32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
    for (offset, expected) in [
        (0x48, 0xd832008020570400u64),
        (0x50, 0x4c68100c00070000u64),
        (0x58, 0x4c68100c00170101u64),
        (0x68, 0x4c68100c00270202u64),
        (0x70, 0x4c68100c00370303u64),
    ] {
        let address = 0x1f0ff0 + offset + 256;
        assert_eq!(
            u64::from_le_bytes(exe[address..address + 8].try_into().unwrap()),
            expected
        );
    }
    for (address, expected) in [
        (0x159758, 1920.0f32),
        (0x159614, 1080.0),
        (0x1596d0, -1920.0),
        (0x159aac, f32::from_bits(0x3af2b9d6)),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}

#[test]
#[ignore = "requires GPU"]
fn reference_scanline_gpu_native_alternating_strips_signed_power_and_native_uvs() {
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
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        64, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Scanline reference depth"),
        size: wgpu::Extent3d {
            width: 64,
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
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Scanline reference target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 1080,
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
    let old = make_input("Scanline old", false);
    let new = make_input("Scanline new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    if std::env::var("SHIN_REFERENCE_ROM").is_ok() {
        let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
            shin_tasks::block_on(server.load("/main.snr")).unwrap();
        let loaded = shin_tasks::block_on(crate::wiper::AnyWiper::load(
            &server,
            &scenario,
            shin_core::vm::command::types::WiperType::Scanline,
            Ticks::from_f32(10.0),
            (3, 0, 0, 0, 0, 0, 0, 0),
        ));
        let crate::wiper::AnyWiper::Scanline(wiper) = loaded else {
            panic!("Scanline factory routing")
        };
        assert_eq!(wiper.inner.exponent, 3.0);
    }
    // Exercise the installed variant's update/clone/fast-forward lifecycle.
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::primitives::update::FrameId;
    let mut timer = AnyWiper::Scanline(ScanlineWiper::new(Ticks::from_f32(10.0), 3));
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
        let AnyWiper::Scanline(wiper) = &timer else {
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
    for (progress, exponent, _, offsets) in NATIVE_CASES {
        let implementation = ScanlineWiperImpl {
            exponent: exponent as f32,
        };
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Scanline reference",
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
        for y in 0..1080 {
            for x in 0..64 {
                let old_y = (((y as f32 + 0.5) / 1080.0) * 64.0 - 0.5).clamp(0.0, 63.0);
                let mut expected = [16.0 + x as f32 * 2.0, 32.0 + old_y * 2.0, 128.0, 160.0];
                let strip = (y / 2) as usize;
                let world_x = (x as f32 + 0.5) * 30.0;
                let incoming_x = world_x - offsets[strip & 1];
                if (0.0..1920.0).contains(&incoming_x) {
                    let geometry = NATIVE_GEOMETRY[strip];
                    let fraction = if y & 1 == 0 { 0.25 } else { 0.75 };
                    let v = geometry.2[1] * (1.0 - fraction) + geometry.2[3] * fraction;
                    let new_x = (incoming_x / 1920.0 * 64.0 - 0.5).clamp(0.0, 63.0);
                    let new_y = (v * 64.0 - 0.5).clamp(0.0, 63.0);
                    // Native blend1 replaces RGBA, even when the target
                    // texture has partial alpha; there is no fade.
                    expected = [170.0, 15.0 + new_x, 40.0 + new_y * 2.0, 128.0];
                }
                let pixel = &pixels[((y * 64 + x) * 4) as usize..][..4];
                for channel in 0..4 {
                    assert!(
                        (f32::from(pixel[channel]) - expected[channel]).abs() <= 1.5,
                        "p{progress} exponent{exponent} at{x},{y}: {pixel:?}, expected{expected:?}"
                    );
                }
                checked += 1;
            }
        }
    }
    println!(
        "Scanline native GPU oracle: {checked} RGBA pixels,56 signed-exponent/phase cases at full1080 virtual rows; alternating strip coverage, incoming/source UVs, native lower-V arithmetic and opaque RGBA replacement pass"
    );
}
