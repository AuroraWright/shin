//! Native ScenarioSelect background, close, footer and edge strip GPU oracle.
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_frame_footer_close_and_chapter_edges_gpu() {
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
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
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

    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(
                std::env::var("SHIN_REFERENCE_PATCH_ROM")
                    .or_else(|_| std::env::var("SHIN_REFERENCE_ROM"))
                    .unwrap(),
            )
            .unwrap(),
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
    let edge_data: Vec<u8> = (0..4 * 4)
        .flat_map(|i| [((i % 4) * 60) as u8, ((i / 4) * 60) as u8, 200u8, 255u8])
        .collect();
    let capture = shin_render::gpu_texture::GpuTexture::new_static_with_data(
        &device,
        &queue,
        Some("native chapter edge source"),
        winit::dpi::PhysicalSize::new(4, 4),
        format,
        &edge_data,
    );
    let updated = atlas.get("help_snrsel").size().height == 240;
    for (case, groups, state, flash, unlock) in [
        ("multiple", 6usize, 2i16, 0.42f32, false),
        ("locked", 1, 0, 0.0, true),
        ("negative", 3, -1, 0.7, true),
    ] {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Start frame native reference",
            );
            let projection = crate::render::top_left_projection_matrix();
            super::scenario_select_view::render_background(&atlas, &mut pass, projection);
            super::scenario_select_view::render_footer(
                &atlas, groups, state, unlock, &mut pass, projection,
            );
            super::scenario_select_view::render_close(&atlas, flash, &mut pass, projection);
            super::scenario_select_view::render_chapter_edges(
                capture.as_source(),
                &mut pass,
                projection,
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        let mut checks = 0;
        let mut ys: Vec<_> = (0..1080usize).step_by(3).collect();
        ys.extend(241..246);
        ys.extend(916..921);
        ys.sort_unstable();
        ys.dedup();
        for y in ys {
            for x in (0..1920usize).step_by(3) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let bg = &original.textures[original.name_to_index["bg"]];
                let sampled = sample(
                    bg.as_raw(),
                    bg.width(),
                    bg.height(),
                    px / 1920.0,
                    py / 1080.0,
                );
                let mut expected: [f32; 3] =
                    std::array::from_fn(|c| (sampled[c] * 255.0).round() / 255.0);
                let mut blend = |rect: [f32; 4],
                                 uv: [f32; 4],
                                 data: &[u8],
                                 width: u32,
                                 height: u32,
                                 tint: f32,
                                 white: f32,
                                 gradient: Option<bool>| {
                    if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                        return;
                    }
                    let fx = (px - rect[0]) / (rect[2] - rect[0]);
                    let fy = (py - rect[1]) / (rect[3] - rect[1]);
                    let mut t = sample(
                        data,
                        width,
                        height,
                        uv[0] + fx * (uv[2] - uv[0]),
                        uv[1] + fy * (uv[3] - uv[1]),
                    );
                    if let Some(invert) = gradient {
                        t[3] *= if invert { fy } else { 1.0 - fy };
                    }
                    for c in 0..3 {
                        let color = (t[c] * (1.0 - white) + white) * tint;
                        expected[c] =
                            ((color * t[3] + expected[c] * (1.0 - t[3])) * 255.0).round() / 255.0;
                    }
                };
                let mut right = 1872.0;
                for (row, key, width) in [
                    (1usize, 4usize, 72.0f32),
                    (3, 3, 74.0),
                    (2, 7, 236.0),
                    (0, 1, 206.0),
                    (4, 14, 134.0),
                ] {
                    if row == 4 && (!updated || !unlock) {
                        continue;
                    }
                    if row == 2 && groups == 1 {
                        continue;
                    }
                    let tint = if row == 3 && state == 0 {
                        128.0 / 255.0
                    } else {
                        1.0
                    };
                    let t = &original.textures[original.name_to_index["help_snrsel"]];
                    let left = right - width;
                    blend(
                        [left, 1012.0, right, 1060.0],
                        [
                            0.0,
                            row as f32 * 48.0 / t.height() as f32,
                            width / 320.0,
                            (row + 1) as f32 * 48.0 / t.height() as f32,
                        ],
                        t.as_raw(),
                        t.width(),
                        t.height(),
                        tint,
                        0.0,
                        None,
                    );
                    let t = &original.textures[original.name_to_index["key"]];
                    let icon = left - 46.0;
                    blend(
                        [icon, 1012.0, icon + 48.0, 1060.0],
                        [
                            (key % 8) as f32 / 8.0,
                            (key / 8) as f32 / 2.0,
                            (key % 8 + 1) as f32 / 8.0,
                            (key / 8 + 1) as f32 / 2.0,
                        ],
                        t.as_raw(),
                        t.width(),
                        t.height(),
                        tint,
                        0.0,
                        None,
                    );
                    right = icon - 16.0;
                }
                let t = &original.textures[original.name_to_index["close"]];
                blend(
                    [1764.0, 32.0, 1848.0, 116.0],
                    [0.0, 0.0, 1.0, 1.0],
                    t.as_raw(),
                    t.width(),
                    t.height(),
                    1.0,
                    flash,
                    None,
                );
                for (top, bottom, uvtop, uvbottom, invert) in [
                    (241.0, 246.0, 23.0 / 726.0, 28.0 / 726.0, false),
                    (916.0, 921.0, 698.0 / 726.0, 703.0 / 726.0, true),
                ] {
                    blend(
                        [54.0, top, 526.0, bottom],
                        [6.0 / 1824.0, uvtop, 478.0 / 1824.0, uvbottom],
                        &edge_data,
                        4,
                        4,
                        1.0,
                        0.0,
                        Some(invert),
                    );
                }
                for c in 0..3 {
                    let actual = pixels[(y * 1920 + x) * 4 + c] as f32;
                    let error = (actual - expected[c] * 255.0).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Start frame {case} pixel{x},{y} channel{c}: GPU{actual} CPU{} error{error}",
                        expected[c] * 255.0
                    );
                }
                checks += 1;
            }
        }
        std::fs::write(format!("/private/tmp/shin-start-frame-{case}.rgba"), pixels).unwrap();
        println!("Start frame {case}: {checks} sampled pixels; maxRGBerror{worst}");
    }
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_start_footer_rows_predicates_icons_and_edge_alpha() {
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
        (0xde284, 0x320003e2),
        (0xde288, 0x9400039a),
        (0xde2b8, 0x7100011f),
        (0xde2c4, 0x5a9f0101),
        (0xde2c8, 0x320007e2),
        (0xde2cc, 0x94000389),
        (0xde2f0, 0xf100091f),
        (0xde2f4, 0x540000c3),
        (0xde304, 0x321f03e2),
        (0xde308, 0x9400037a),
        (0xde31c, 0x2a1f03e2),
        (0xde320, 0x94000374),
        (0xdf13c, 0x8b34d2f4),
        (0xdf158, 0x1e2c392a),
        (0xdf1ac, 0x1e201982),
        (0xdf1bc, 0x1e231821),
        (0xdf1f0, 0x1e20294a),
        (0xdf20c, 0x97fd2045),
        (0xdf210, 0x37f80380),
        (0xdf218, 0xbd49c500),
        (0xdf23c, 0x12000a88),
        (0xdf244, 0x53037e88),
        (0xdf284, 0xb9400500),
        (0xdf288, 0x97fd2026),
        (0xdf28c, 0x37f80380),
        (0xdf300, 0x1e202940),
        (0xfb9f0, 0x2a1f03e0),
        (0xfb9f4, 0xd65f03c0),
        (0xde3a0, 0x91088260),
        (0xde3cc, 0x97ffa2d1),
        (0xde95c, 0xb812c3a8),
        (0xde964, 0xb815c3b4),
        (0xde9b4, 0xb812c3b4),
        (0xde9bc, 0xb815c3b5),
    ] {
        assert_eq!(word(a), expected, "native frame instruction {a:x}");
    }
    for (i, row) in [
        [1i32, -1, 0, 206],
        [4, -1, 48, 72],
        [7, -1, 96, 236],
        [3, -1, 144, 74],
    ]
    .iter()
    .enumerate()
    {
        for (j, &value) in row.iter().enumerate() {
            assert_eq!(word(0x1738e8 + i * 16 + j * 4), value as u32);
        }
    }
    for (a, value) in [
        (0x1599d0, 1872.0f32),
        (0x15965c, 1012.0),
        (0x159584, 48.0),
        (0x159a1c, 192.0),
        (0x159444, 320.0),
        (0x1599c4, -48.0),
        (0x159978, 54.0),
        (0x159ae0, 241.0),
        (0x159918, 526.0),
        (0x1593cc, 246.0),
        (0x1599d8, 916.0),
        (0x159ac8, 921.0),
        (0x1599b8, 6.0 / 1824.0),
        (0x159794, 23.0 / 726.0),
        (0x1593d0, 478.0 / 1824.0),
        (0x1595d0, 28.0 / 726.0),
        (0x15980c, 698.0 / 726.0),
        (0x1596b0, 703.0 / 726.0),
    ] {
        assert_eq!(word(a), value.to_bits(), "native frame float {a:x}");
    }
    for (a, name) in [
        (0x162dc4, "help_snrsel"),
        (0x15c0bf, "key"),
        (0x15c0b9, "close"),
        (0x15a496, "bg"),
    ] {
        assert_eq!(&exe[a + 256..a + 256 + name.len()], name.as_bytes());
    }
}
