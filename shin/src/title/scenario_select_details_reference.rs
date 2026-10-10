//! Native details snapshot and interrupted crossfade GPU oracle.
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_details_capture_background_and_interrupted_crossfade() {
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
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
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
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference depth"),
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
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Title reference target"),
        size: wgpu::Extent3d {
            width: 1920,
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

    use shin_core::{
        format::{rom::RomReader, save::SaveData},
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
    let file = rom.find_file("/snrsel.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/snrsel.txa").unwrap();
    fn sample(data: &[u8], width: u32, height: u32, u: f32, v: f32) -> [f32; 4] {
        let x = u * width as f32 - 0.5;
        let y = v * height as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|c| {
            let p = |dx: f32, dy: f32| {
                let x = (ix + dx).clamp(0.0, width as f32 - 1.0) as u32;
                let y = (iy + dy).clamp(0.0, height as f32 - 1.0) as u32;
                data[((y * width + x) * 4) as usize + c] as f32 / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    }
    let mut save = SaveData::default();
    for slot in [2, 9, 25] {
        save.persist_data.set(slot, 2);
    }
    let setup = crate::title::scenario_select::ScenarioSelectSetup::from_save(&save);
    let mut details =
        super::ScenarioDetails::new(&context, winit::dpi::PhysicalSize::new(1920, 1080));
    let mut cpu_current = vec![0u8; 1824 * 726 * 4];
    let mut cpu_previous = None;
    for (case, id, entry_index, name, fade, capture) in [
        ("initial", 0u8, 0usize, "details_00", 0.0f32, false),
        ("start", 0, 1, "details_01", 1.0, true),
        ("half", 0, 1, "details_01", 0.5, false),
        ("interrupted", 0, 2, "details_48", 0.75, true),
        ("fraction", 3, 0, "details_24", 0.333, true),
        ("complete", 1, 0, "details_08", 0.0, false),
    ] {
        if capture {
            details.capture_previous(&mut context);
            cpu_previous = Some(cpu_current.clone());
        }
        let entry = &setup.groups.iter().find(|g| g.id == id).unwrap().entries[entry_index];
        assert_eq!(entry.details_name(), name);
        details.pre_render(&atlas, entry, fade, &mut context);
        let background = &original.textures[original.name_to_index["bg"]];
        let art = &original.textures[original.name_to_index[name]];
        let alpha = (fade * 255.0).floor() / 255.0;
        for y in 0..726usize {
            for x in 0..1824usize {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut out = sample(
                    background.as_raw(),
                    background.width(),
                    background.height(),
                    0.025 + (0.97499996 - 0.025) * px / 1824.0,
                    0.20185184 + (0.87407404 - 0.20185184) * py / 726.0,
                );
                for c in 0..4 {
                    out[c] = (out[c] * 255.0).round() / 255.0;
                }
                let blend = |out: &mut [f32; 4], src: [f32; 4]| {
                    for c in 0..3 {
                        out[c] =
                            ((src[c] * src[3] + out[c] * (1.0 - src[3])) * 255.0).round() / 255.0;
                    }
                    out[3] = ((src[3] * (1.0 - out[3]) + out[3]) * 255.0).round() / 255.0;
                };
                if (528.0..1814.0).contains(&px) && (15.0..711.0).contains(&py) {
                    blend(
                        &mut out,
                        sample(
                            art.as_raw(),
                            art.width(),
                            art.height(),
                            (px - 528.0) / 1286.0,
                            (py - 15.0) / 696.0,
                        ),
                    );
                }
                let i = (y * 1824 + x) * 4;
                if fade > 0.0 {
                    if let Some(prev) = &cpu_previous {
                        blend(
                            &mut out,
                            [
                                prev[i] as f32 / 255.0,
                                prev[i + 1] as f32 / 255.0,
                                prev[i + 2] as f32 / 255.0,
                                prev[i + 3] as f32 / 255.0 * alpha,
                            ],
                        );
                    }
                }
                for c in 0..4 {
                    cpu_current[i + c] = (out[c] * 255.0).round() as u8;
                }
            }
        }
        let capture_pixels =
            crate::layer::user::readback_test_texture(&mut context, &details.current.texture);
        let mut capture_worst = 0u8;
        for y in (0..726usize).step_by(3) {
            for x in (0..1824usize).step_by(3) {
                for c in 0..4 {
                    let i = (y * 1824 + x) * 4 + c;
                    let error = capture_pixels[i].abs_diff(cpu_current[i]);
                    capture_worst = capture_worst.max(error);
                    assert!(
                        error <= 3,
                        "details {case} capture{x},{y} channel{c}: GPU{} CPU{} error{error}",
                        capture_pixels[i],
                        cpu_current[i]
                    );
                }
            }
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Start details composite",
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(shin_core::primitives::color::UnormColor::BLACK)
                .fill(
                    &mut pass,
                    shin_render::RenderRequestBuilder::new(),
                    crate::render::top_left_projection_matrix(),
                );
            details.render(&mut pass, crate::render::top_left_projection_matrix());
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0u8;
        for y in (0..1080usize).step_by(3) {
            for x in (0..1920usize).step_by(3) {
                for c in 0..3 {
                    let expected = if (48..1872).contains(&x) && (218..944).contains(&y) {
                        let i = ((y - 218) * 1824 + x - 48) * 4;
                        (cpu_current[i + c] as f32 * cpu_current[i + 3] as f32 / 255.0).round()
                            as u8
                    } else {
                        0
                    };
                    let actual = pixels[(y * 1920 + x) * 4 + c];
                    let error = actual.abs_diff(expected);
                    worst = worst.max(error);
                    assert!(
                        error <= 3,
                        "details {case} screen{x},{y} channel{c}: GPU{actual} CPU{expected} error{error}"
                    );
                }
            }
        }
        std::fs::write(
            format!("/private/tmp/shin-start-details-{case}.rgba"),
            pixels,
        )
        .unwrap();
        println!(
            "Start details {case}: capture maxRGBerror{capture_worst}; screen230400 sampled pixels maxRGBerror{worst}"
        );
    }
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_start_details_target_crop_snapshot_and_quantized_fade() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0xdd384, 0x1e290020),
        (0xdd390, 0x1e290001),
        (0xdd39c, 0x97ffcc81),
        (0xdde80, 0x97ffc514),
        (0xddee8, 0x320003e0),
        (0xddf24, 0x97ffa363),
        (0xddf4c, 0x97ffa36d),
        (0xddf64, 0x321f03e0),
        (0xddf90, 0x7100055f),
        (0xddfa8, 0x321c07e2),
        (0xddffc, 0x97ffa32d),
        (0xde018, 0x97ffa33a),
        (0xde04c, 0x910143e0),
        (0xde054, 0x910b6268),
        (0xde068, 0x1e202008),
        (0xde0a8, 0x97ffa302),
        (0xde0d4, 0x4eaa1d43),
        (0xde0e8, 0x97fde1d6),
        (0xde0f4, 0x97ffa30f),
        (0xdfd64, 0x97ffc2fb),
        (0xdfdc8, 0x97ff6d46),
        (0xdfdd0, 0x1e249001),
        (0xdfde0, 0x97ff6f08),
        (0x56854, 0x6e21dc00),
        (0x5685c, 0x6ea1b800),
        (0x56868, 0x2e214800),
    ] {
        assert_eq!(word(a), expected, "native details instruction {a:x}");
    }
    for (a, expected) in [
        (0x159574, 1824.0f32),
        (0x159860, 726.0f32),
        (0x1593c4, 0.02500000037252903f32),
        (0x159808, 0.20185184478759766f32),
        (0x159710, 0.9749999642372131f32),
        (0x1593c8, 0.8740740418434143f32),
        (0x159830, 528.0f32),
        (0x1595cc, 1814.0f32),
        (0x159a10, 711.0f32),
        (0x159584, 48.0f32),
        (0x15984c, 218.0f32),
        (0x1599d0, 1872.0f32),
        (0x1594b0, 944.0f32),
    ] {
        assert_eq!(word(a), expected.to_bits(), "native details float {a:x}");
    }
}
