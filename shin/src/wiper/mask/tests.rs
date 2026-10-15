use super::*;
use crate::render::PreRenderContext;

#[test]
fn mask_strength_native_integer_clamp_multiply_bits_and_retained_flags() {
    use shin_core::format::scenario::instruction_elements::FromNumber;
    // Native SCVTF/FMUL instruction results, including333 where division
    // by1000 gives a different last bit from multiplication by0.001.
    for (raw, bits) in [
        (-2147483648, 0x3a83126fu32),
        (-1000, 0x3a83126fu32),
        (-1, 0x3a83126fu32),
        (0, 0x3f800000u32),
        (1, 0x3a83126fu32),
        (137, 0x3e0c49bbu32),
        (333, 0x3eaa7efau32),
        (999, 0x3f7fbe78u32),
        (1000, 0x3f800000u32),
        (1001, 0x3f800000u32),
        (2147483647, 0x3f800000u32),
    ] {
        assert_eq!(MaskParam::from_number(raw).0.to_bits(), bits);
    }
    for raw in [i32::MIN, -1, 0, 8, 0x40000015, i32::MAX] {
        assert_eq!(MaskFlags::from_number(raw).bits(), raw);
    }
}
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_mask_metadata_fallback_and_parameter_conversion() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0xa1858, 0x1e20194bu32),
        (0xa1860, 0x1e2a296c),
        (0xa1868, 0x1e2c0801),
        (0xa186c, 0x1e203940),
        (0xa1870, 0x1e213941),
        (0xa1874, 0x1e200980),
        (0xa187c, 0x721e011f),
        (0xa1884, 0x1e212962),
        (0xa1888, 0x1e2b3803),
        (0xa188c, 0x1e200c20),
        (0xa1890, 0x1e230c41),
        (0xa18dc, 0x37180068),
        (0xa18f8, 0x1e201900),
        (0xa190c, 0x1e201920),
        (0xa1950, 0x528004a0),
        (0x7e080, 0x36200128),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
    for (offset, expected) in [
        (0x50, 0xd82200cff0b70a0au64),
        (0x58, 0xd83200a040970806),
        (0x68, 0xd832008020970800),
        (0x70, 0x4c59100c00170c0c),
        (0x88, 0x51a4060c00070a0c),
        (0x90, 0x59a1030000c70606),
        (0xb8, 0x59a0030000070c00),
        (0xd8, 0x59a0028000370c03),
    ] {
        let address = 0x216970 + offset + 256;
        assert_eq!(
            u64::from_le_bytes(exe[address..address + 8].try_into().unwrap()),
            expected
        );
    }
    assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 1]) * 4, 0x21c14);
    for (address, expected) in [
        (0xea230, 0x37f80101u32),
        (0xea23c, 0xd343fd29u32),
        (0xea240, 0x6b01013fu32),
        (0xea244, 0x540000adu32),
        (0xea250, 0xaa1f03e0u32),
        (0xea258, 0xaa1f03e0u32),
        (0x21c1c, 0x94032185u32),
        (0x21c20, 0xb4000ce0u32),
        (0x21dbc, 0x4ea81d00u32),
        (0x21dc8, 0x9403e522u32),
        (0x21e84, 0x34000268u32),
        (0x21e88, 0x7100051fu32),
        (0x21e8c, 0x1a9fc508u32),
        (0x21e90, 0x710fa11fu32),
        (0x21e98, 0x1a89b108u32),
        (0x21e9c, 0x1e220100u32),
        (0x21ea8, 0x1e210801u32),
        (0x21ed0, 0x1e2e1001u32),
        (0x21ed4, 0xb9400a81u32),
        (0x11b2dc, 0x2a0103f5u32),
        (0x11b300, 0x2a1503e2u32),
        (0x11b39c, 0xb9002e75u32),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
}

#[test]
#[ignore = "requires GPU"]
fn reference_mask_factory_invalid_metadata_default_pixels_and_valid_retained_flags() {
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
        label: Some("Mask factory reference depth"),
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
        label: Some("Mask factory reference target"),
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
    let old = make_input("Mask fallback old", false);
    let new = make_input("Mask fallback new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    let scenario: std::sync::Arc<shin_core::format::scenario::Scenario> =
        shin_tasks::block_on(server.load("/main.snr")).unwrap();
    let count = scenario.info_tables().mask_info.len() as i32;
    assert!(count >= 2);
    let empty_server = std::sync::Arc::new(AssetServer::new(
        LayeredAssetIo::new().into(),
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
    use crate::wiper::{AnyWiper, Wiper};
    use shin_core::format::scenario::instruction_elements::UntypedNumberArray;
    use shin_core::{primitives::update::FrameId, vm::command::types::WiperType};
    use shin_render::shaders::types::RenderClone;
    for ty in [i32::MIN, -1, 10, i32::MAX] {
        assert!(
            shin_tasks::block_on(AnyWiper::load_raw(
                &empty_server,
                &scenario,
                ty,
                Ticks::from_f32(60.0),
                (i32::MIN, i32::MIN, -1, 0, 0, 0, 0, 0),
            ))
            .is_none()
        );
    }
    assert!(matches!(
        shin_tasks::block_on(AnyWiper::load_raw(
            &empty_server,
            &scenario,
            0,
            Ticks::from_f32(60.0),
            UntypedNumberArray::default(),
        )),
        Some(AnyWiper::Default(_))
    ));
    // Real factories for all ten kinds: timer and effect bodies may run only
    // once per frame; unlike property copies, wipe copies reset the marker.
    let progress = |wiper: &AnyWiper| match wiper {
        AnyWiper::Default(w) => w.state.get_progress(),
        AnyWiper::Mask(w) => w.state.get_progress(),
        AnyWiper::Scroll(w) => w.state.get_progress(),
        AnyWiper::Zoom(w) => w.state.get_progress(),
        AnyWiper::Turn(w) => w.state.get_progress(),
        AnyWiper::Wave(w) => w.state.get_progress(),
        AnyWiper::Scanline(w) => w.state.get_progress(),
        AnyWiper::Ripple(w) => w.state.get_progress(),
        AnyWiper::Whirl(w) => w.state.get_progress(),
        AnyWiper::Glass(w) => w.state.get_progress(),
    };
    let render_wiper = |wiper: &AnyWiper, context: &mut PreRenderContext| {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Wipe frame guard",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            wiper.render(
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
            );
        }
        crate::layer::user::readback_test_texture(context, &target)
    };
    for kind in 0..=9 {
        let ty = num_traits::FromPrimitive::from_i32(kind).unwrap();
        let mut wiper = shin_tasks::block_on(AnyWiper::load(
            &server,
            &scenario,
            ty,
            Ticks::from_f32(10.0),
            (1, 333, 0, 0, 0, 0, 0, 0),
        ));
        let mut frame = FrameId::default();
        let update = |wiper: &mut AnyWiper, frame_id, delta, allowed| {
            wiper.update(&AdvUpdateContext {
                frame_id,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: &server,
                device: &device,
                queue: &queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
        };
        let initial_pixels = render_wiper(&wiper, &mut context);
        update(&mut wiper, frame, 9.0, true);
        assert_eq!(progress(&wiper), 0.0);
        assert_eq!(
            render_wiper(&wiper, &mut context),
            initial_pixels,
            "New wipe{kind} advanced in frame zero"
        );
        let mut initial_copy = wiper.render_clone(&mut context.render_clone_ctx());
        update(&mut initial_copy, frame, 9.0, true);
        assert_eq!(progress(&initial_copy), 0.0);
        assert_eq!(
            render_wiper(&initial_copy, &mut context),
            initial_pixels,
            "Copied wipe{kind} advanced in frame zero"
        );
        frame.advance();
        update(&mut wiper, frame, 2.0, true);
        assert!((progress(&wiper) - 0.2).abs() < 0.00001);
        let pixels = render_wiper(&wiper, &mut context);
        update(&mut wiper, frame, 9.0, false);
        update(&mut wiper, frame, 9.0, true);
        assert!((progress(&wiper) - 0.2).abs() < 0.00001);
        assert_eq!(
            render_wiper(&wiper, &mut context),
            pixels,
            "Wipe{kind} repeated frame changes effect"
        );
        let mut copy = wiper.render_clone(&mut context.render_clone_ctx());
        update(&mut copy, frame, 1.0, true);
        assert!((progress(&copy) - 0.3).abs() < 0.00001);
        assert!((progress(&wiper) - 0.2).abs() < 0.00001);
        let copied_pixels = render_wiper(&copy, &mut context);
        update(&mut copy, frame, 8.0, true);
        assert_eq!(
            render_wiper(&copy, &mut context),
            copied_pixels,
            "Copied wipe{kind} repeated frame changes effect"
        );
        frame.advance();
        update(&mut wiper, frame, 2.0, false);
        let paused_pixels = render_wiper(&wiper, &mut context);
        update(&mut wiper, frame, 9.0, true);
        assert!((progress(&wiper) - 0.2).abs() < 0.00001);
        assert_eq!(
            render_wiper(&wiper, &mut context),
            paused_pixels,
            "Paused-first wipe{kind} consumed frame twice"
        );
        frame.advance();
        update(&mut wiper, frame, -1.0, true);
        assert!((progress(&wiper) - 0.1).abs() < 0.00001);
    }
    println!(
        "All ten wipe factories: once-per-frame timer/effect rendering, paused-first visit, same-frame copy reset, duplicate copy visit and negative delta pass"
    );
    let mut checked = 0;
    for id in [
        i32::MIN,
        -32769,
        -1,
        count,
        count + 1,
        32768,
        65535,
        i32::MAX,
    ] {
        let mut loaded = shin_tasks::block_on(AnyWiper::load(
            &empty_server,
            &scenario,
            WiperType::Mask,
            Ticks::from_f32(10.0),
            (id, i32::MIN, -1, 0, 0, 0, 0, 0),
        ));
        let AnyWiper::Default(wiper) = &loaded else {
            panic!("invalid ID{id} must select Default")
        };
        assert_eq!(wiper.state.get_progress(), 0.0);
        let mut lifecycle_frame = FrameId::default();
        for (delta, allowed, expected) in [(2.0, true, 0.2), (9.0, false, 0.2), (-1.0, true, 0.1)] {
            lifecycle_frame.advance();
            loaded.update(&AdvUpdateContext {
                frame_id: lifecycle_frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: &empty_server,
                device: &device,
                queue: &queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
            let AnyWiper::Default(wiper) = &loaded else {
                panic!()
            };
            assert!((wiper.state.get_progress() - expected).abs() < 0.000001);
        }
        let mut copy = loaded.render_clone(&mut context.render_clone_ctx());
        copy.fast_forward();
        assert!(!copy.is_running());
        assert!(loaded.is_running());
        for progress in [0.0f32, 0.125, 0.37, 0.5, 0.71, 0.75, 0.9999, 1.0] {
            let mut frame = loaded.render_clone(&mut context.render_clone_ctx());
            let AnyWiper::Default(wiper) = &mut frame else {
                panic!()
            };
            wiper
                .state
                .update(true, Ticks::from_f32(progress * 10.0 - 1.0));
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
                frame.render(
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
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            for y in 0..64 {
                for x in 0..64 {
                    let old = [16.0 + x as f32 * 2.0, 32.0 + y as f32 * 2.0, 128.0, 160.0];
                    let new = [170.0, 15.0 + x as f32, 40.0 + y as f32 * 2.0, 128.0];
                    let expected: [f32; 4] = std::array::from_fn(|c| {
                        // Native program36 performs two FFMA stages on the
                        // normalized sampled channels, including alpha.
                        let source = old[c] / 255.0;
                        let outgoing = (-progress).mul_add(source, source);
                        progress.mul_add(new[c] / 255.0, outgoing) * 255.0
                    });
                    let pixel = &pixels[((y * 64 + x) * 4) as usize..][..4];
                    for channel in 0..4 {
                        assert!(
                            (f32::from(pixel[channel]) - expected[channel]).abs() <= 1.5,
                            "p{progress} invalid ID{id} at{x},{y}: {pixel:?}, expected{expected:?}"
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    for duration in [0.0, -10.0] {
        let fallback = shin_tasks::block_on(AnyWiper::load(
            &empty_server,
            &scenario,
            WiperType::Mask,
            Ticks::from_f32(duration),
            (-1, 0, i32::MAX, 0, 0, 0, 0, 0),
        ));
        assert!(!fallback.is_running());
    }
    for flags in [0x10, 0x40000010, -1] {
        let loaded = shin_tasks::block_on(AnyWiper::load(
            &server,
            &scenario,
            WiperType::Mask,
            Ticks::from_f32(10.0),
            (1, 333, flags, 0, 0, 0, 0, 0),
        ));
        let AnyWiper::Mask(mask) = loaded else {
            panic!("valid metadata must retain Mask")
        };
        assert_eq!(mask.inner.param2.0.to_bits(), 0x3eaa7efau32);
        assert_eq!(mask.inner.flags.bits(), flags);
        assert!(mask.is_running());
    }
    println!(
        "Mask fallback GPU oracle: {checked} RGBA pixels; eight invalid signed IDs, ignored strength/flags, eight phases per ID, empty asset IO and Default lifecycle pass"
    );
}

include!("native_cases.rs");
#[test]
fn mask_weights_match_executed_native_normal_and_inverted_arithmetic() {
    for &(strength, progress, flags, expected) in NATIVE_MASK_WEIGHTS {
        assert_eq!(
            mask_weights(
                MaskParam(strength),
                MaskFlags::from_bits_retain(flags),
                progress
            )
            .to_array()
            .map(f32::to_bits),
            expected
        );
    }
}

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_mask_native_sampling_flags_and_weight_pixels() {
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
    let root = std::env::temp_dir().join(format!("shin-mask-wipe-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let (width, height) = (384u32, 216u32);
    let texels: Vec<u8> = (0..height)
        .flat_map(|y| (0..width).map(move |x| ((x * 13 + y * 7 + (x / 31) * 47) % 256) as u8))
        .collect();
    let payload_size = 4 + width * height;
    let mut bytes = b"MSK4".to_vec();
    for word in [
        1,
        36 + payload_size + 32,
        0,
        width | (height << 16),
        36,
        payload_size,
        36 + payload_size,
        32,
    ] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&texels);
    for word in [1u32, 0, 0, 0, 0, 0] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for edge in [0u16, 0, (width - 1) as u16, (height - 1) as u16] {
        bytes.extend_from_slice(&edge.to_le_bytes());
    }
    std::fs::write(root.join("oracle.msk"), bytes).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&root).unwrap();
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
        label: Some("Mask factory reference depth"),
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
        label: Some("Mask factory reference target"),
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
    let old = make_input("Mask fallback old", false);
    let new = make_input("Mask fallback new", true);
    let old_view = old.create_view(&Default::default());
    let new_view = new.create_view(&Default::default());
    let mask: std::sync::Arc<crate::asset::mask::MaskTexture> =
        shin_tasks::block_on(server.load("/oracle.msk")).unwrap();
    let mut inputs = vec![("synthetic".to_owned(), mask, width, height, texels)];
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path: &str| {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let mut paths: std::collections::BTreeSet<_> = scenario
        .info_tables()
        .mask_info
        .iter()
        .map(|m| m.path())
        .collect();
    paths.insert("/default.msk".to_owned());
    for path in paths {
        let decoded = shin_core::format::mask::read_mask(&read(&path)).unwrap();
        let mask =
            shin_tasks::block_on(server.load::<crate::asset::mask::MaskTexture, _>(&path)).unwrap();
        let (width, height) = decoded.texels.dimensions();
        inputs.push((path, mask, width, height, decoded.texels.into_raw()));
    }
    let asset_count = inputs.len() - 1;
    let mut checked = 0;
    for (label, mask, width, height, texels) in inputs {
        let strengths: &[i32] = if label == "synthetic" {
            &[0, 1, 137, 333, 999, 1000]
        } else {
            &[333]
        };
        let phases: &[f32] = if label == "synthetic" {
            &[0.0, 0.125, 0.5, 0.75, 1.0]
        } else {
            &[0.5]
        };
        let sample = |u: f32, v: f32| {
            let x = u * width as f32 - 0.5;
            let y = v * height as f32 - 0.5;
            let ix = x.floor() as i32;
            let iy = y.floor() as i32;
            let tx = x - x.floor();
            let ty = y - y.floor();
            let at = |x: i32, y: i32| {
                f32::from(
                    texels[(y.clamp(0, height as i32 - 1) as u32 * width
                        + x.clamp(0, width as i32 - 1) as u32) as usize],
                ) / 255.0
            };
            let a = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
            let b = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
            a * (1.0 - ty) + b * ty
        };
        for &raw in strengths {
            // Native integer-clamp, SCVTF, multiply0.001; not production lowering.
            let strength = if raw == 0 {
                1.0
            } else {
                raw as f32 * f32::from_bits(0x3a83126f)
            };
            for flags in (0..32).chain([0x4000000f, -1]) {
                for &progress in phases {
                    let wiper = MaskWiperImpl {
                        mask: mask.clone(),
                        param2: MaskParam(strength),
                        flags: MaskFlags::from_bits_retain(flags),
                    };
                    {
                        let mut pass = context.begin_pass(
                            TextureTarget {
                                view: &view,
                                kind: TextureTargetKind::Screen,
                            },
                            Some(context.depth_stencil),
                            "Native Mask oracle",
                        );
                        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                        wiper.render(
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
                    let inv = 1.0 / strength;
                    let span = inv + 1.0;
                    let first = if flags & 4 == 0 {
                        1.0 - progress * span
                    } else {
                        span * (1.0 - progress)
                    };
                    let last = if flags & 4 == 0 {
                        first + inv
                    } else {
                        first - inv
                    };
                    for y in 0..64 {
                        for x in 0..64 {
                            let mut u = (x as f32 + 0.5) / 64.0;
                            let mut v = (y as f32 + 0.5) / 64.0;
                            if flags & 1 != 0 {
                                u = 1.0 - u;
                            }
                            if flags & 2 != 0 {
                                v = 1.0 - v;
                            }
                            if flags & 8 != 0 {
                                u *= 1920.0 / width as f32;
                                v *= 1080.0 / height as f32;
                            }
                            // Original Maxwell FFMA.SAT gives the outgoing weight.
                            let old_weight =
                                sample(u, v).mul_add(last - first, first).clamp(0.0, 1.0);
                            let old = [16.0 + x as f32 * 2.0, 32.0 + y as f32 * 2.0, 128.0, 160.0];
                            let new = [170.0, 15.0 + x as f32, 40.0 + y as f32 * 2.0, 128.0];
                            let actual = &pixels[((y * 64 + x) * 4) as usize..][..4];
                            for c in 0..4 {
                                let expected = old[c] * old_weight + new[c] * (1.0 - old_weight);
                                assert!(
                                    (f32::from(actual[c]) - expected).abs() <= 2.0,
                                    "Mask{label} raw{raw} flags{flags:x} p{progress} at{x},{y}: {actual:?} expected{expected} weight{old_weight}"
                                );
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
    println!(
        "Mask native GPU oracle: {checked} RGBA pixels; synthetic six strengths/five phases plus{asset_count} supplied assets at333/half-progress,34 flag words each pass"
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_mask_loader_failure_is_not_ready_or_null_success() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    // Loader vptr2233b8: ready+20 dispatches the base wrapper, which invokes
    // wait+30 with timeout0. Only state2 is ready; state3 records an exception.
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|o| {
            let word = |a: usize| u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
            (word(o), word(o + 16))
        })
        .collect();
    assert_eq!(relocs[&0x226538], 0x2233a8);
    assert_eq!(relocs[&0x2233d8], 0x11b210);
    assert_eq!(relocs[&0x2233e8], 0x11b8c0);
    assert_eq!(relocs[&0x2233f8], 0x11b900);
    for (address, expected) in NATIVE_MASK_LOADER_WORDS {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native{address:x}"
        );
    }
}

#[test]
#[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); no GPU required"]
fn reference_rom_all_mask_metadata_assets_decode() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut read = |path: &str| {
        let file = rom
            .find_file(path)
            .unwrap_or_else(|e| panic!("Missing{path}: {e}"));
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let mut paths: std::collections::BTreeSet<_> = scenario
        .info_tables()
        .mask_info
        .iter()
        .map(|m| m.path())
        .collect();
    paths.insert("/default.msk".to_owned());
    let mut pixels = 0u64;
    let mut rectangles = 0usize;
    let mut dimensions = std::collections::BTreeSet::new();
    for path in &paths {
        let mask = shin_core::format::mask::read_mask(&read(path))
            .unwrap_or_else(|e| panic!("Decode{path}: {e}"));
        let (w, h) = mask.texels.dimensions();
        assert!(w > 0 && h > 0, "Empty mask{path}");
        dimensions.insert((w, h));
        // Asset::load creates four vertices per rectangle with u16 indices.
        // Confirm the supplied assets fit that renderer's actual index width.
        assert!(
            mask.regions.rects.len() * 4 <= 65536,
            "Mask vertex index overflow{path}"
        );
        for r in &mask.regions.rects {
            assert!(
                r.from_x <= r.to_x && r.from_y <= r.to_y,
                "Inverted rectangle{path}"
            );
        }
        pixels += u64::from(w) * u64::from(h);
        rectangles += mask.regions.rects.len();
    }
    assert!(!paths.is_empty());
    println!(
        "All Mask assets: {} metadata entries, {} unique paths, {pixels} decoded texels, {rectangles} rectangles, dimensions{dimensions:?}",
        scenario.info_tables().mask_info.len(),
        paths.len()
    );
}

const NATIVE_MASK_LOADER_WORDS: [(usize, u32); 23] = [
    (0x11b214, 0xf9401902),
    (0x11b218, 0xaa1f03e1),
    (0x11b21c, 0xd61f0040),
    (0x11b8d0, 0x97fc3454),
    (0x11b8d4, 0x7100081f),
    (0x11b8d8, 0x1a9f17e0),
    (0x28a44, 0x9400ecc3),
    (0x28a48, 0xb9401e60),
    (0x28dd0, 0x320007f9),
    (0x28dd4, 0x321f03fa),
    (0x28e6c, 0xd63f0100),
    (0x28e7c, 0x889ffd00),
    (0x28e8c, 0xb9001d1a),
    (0x28f10, 0x9403ced0),
    (0x28f1c, 0xb9001d19),
    (0x11b92c, 0x7100081f),
    (0x11b930, 0x54000421),
    (0x11b990, 0x97fe1604),
    (0x11b9b4, 0xf900027f),
    (0x666c, 0xf9401108),
    (0x6674, 0x36001160),
    (0x8f84, 0xf9401108),
    (0x8f8c, 0x360051e0),
];
