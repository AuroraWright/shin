//! Independent native-coordinate pixel oracle for Start chapter drawing.
use crate::{adv::character_view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_chapter_gpu_clipping_badges_and_flash() {
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
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
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

    if std::env::var_os("SHIN_REFERENCE_SCROLLBAR_ONLY").is_some() {
        let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
        let mut save = shin_core::format::save::SaveData::default();
        for slot in 1..=48 {
            save.persist_data.set(slot, 2);
        }
        let setup = super::scenario_select::ScenarioSelectSetup::from_save(&save);
        for group in &setup.groups {
            let max = (group.content_height - 670).max(0) as f32;
            let mut frames = Vec::new();
            for scroll in [0.0, max / 2.0, max] {
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "Start scrollbar reference",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    super::scenario_select_view::render_scrollbar(
                        group,
                        scroll,
                        &system,
                        &mut pass,
                        crate::render::top_left_projection_matrix(),
                    );
                }
                let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
                let mut lit = 0;
                for (i, pixel) in pixels.chunks_exact(4).enumerate() {
                    if pixel[..3] != [0, 0, 0] {
                        let (x, y) = (i % 1920, i / 1920);
                        assert!(
                            (536..552).contains(&x) && (246..916).contains(&y),
                            "scrollbar escaped viewport at{x},{y}"
                        );
                        lit += 1;
                    }
                }
                if max > 0.0 {
                    assert!(
                        lit > 1000,
                        "overflow tab{} missing system artwork",
                        group.id
                    );
                } else {
                    assert_eq!(lit, 0, "fitting tab{} should hide scrollbar", group.id);
                }
                frames.push(pixels);
            }
            if max > 0.0 {
                assert_ne!(frames[0], frames[1]);
                assert_ne!(frames[1], frames[2]);
            }
            eprintln!(
                "Start scrollbar tab{}: height{} top/middle/bottom and overflow visibility pass",
                group.id, group.content_height
            );
        }
        return;
    }

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
    let menu = &original.textures[original.name_to_index["menu"]];
    assert_eq!(menu.dimensions(), (1760, 880));
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
    for slot in 1..=48 {
        save.persist_data.set(
            slot,
            match slot % 6 {
                0 => -1,
                1 => 0,
                2 => 1,
                3 => 2,
                4 => 3,
                _ => 4,
            },
        );
    }
    let setup = super::scenario_select::ScenarioSelectSetup::from_save(&save);
    for (id, scroll) in [(0, 37.5f32), (3, 1559.25), (5, 0.0)] {
        let group = setup.groups.iter().find(|group| group.id == id).unwrap();
        let feedback: Vec<_> = (0..group.entries.len())
            .map(|index| super::scenario_select_view::ChapterFeedback {
                focus: (index % 3) as f32 * 0.5,
                flash: if index % 2 == 0 { 0.42 } else { 0.0 },
            })
            .collect();
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Start chapters native reference",
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(shin_core::primitives::color::UnormColor::BLACK)
                .fill(
                    &mut pass,
                    shin_render::RenderRequestBuilder::new(),
                    crate::render::top_left_projection_matrix(),
                );
            super::scenario_select_view::render_chapters(
                group,
                scroll,
                &feedback,
                &atlas,
                &mut pass,
                crate::render::top_left_projection_matrix(),
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        let mut checks = 0;
        // Independent native-coordinate oracle. Do not use production sprite descriptions.
        for y in (0..1080).step_by(3) {
            for x in (0..1920).step_by(3) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = [0.0f32; 3];
                if (241.0..921.0).contains(&py) {
                    for (index, entry) in group.entries.iter().enumerate() {
                        let top = 246.0 + index as f32 * 112.0 - scroll;
                        let menu_index = match id {
                            0 => index,
                            3 => index + 8,
                            5 => index + 30,
                            _ => unreachable!(),
                        };
                        let mut quad = |name: &str, rect: [f32; 4], uv: [f32; 4]| {
                            if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                                return;
                            }
                            let texture = &original.textures[original.name_to_index[name]];
                            let u = uv[0] + (uv[2] - uv[0]) * (px - rect[0]) / (rect[2] - rect[0]);
                            let v = uv[1] + (uv[3] - uv[1]) * (py - rect[1]) / (rect[3] - rect[1]);
                            let texel =
                                sample(texture.as_raw(), texture.width(), texture.height(), u, v);
                            let focus = (index % 3) as f32 * 0.25;
                            let flash = if index % 2 == 0 { 0.42 } else { 0.0 };
                            for c in 0..3 {
                                let a = texel[c] * (1.0 - focus) + focus;
                                let b = a * (1.0 - flash) + flash;
                                expected[c] = ((b * texel[3] + expected[c] * (1.0 - texel[3]))
                                    * 255.0)
                                    .round()
                                    / 255.0;
                            }
                        };
                        quad(
                            "menu",
                            [86.0, top, 526.0, top + 110.0],
                            [
                                ((menu_index / 8) as f32) * 0.25,
                                ((menu_index % 8) as f32) * 0.125,
                                ((menu_index / 8 + 1) as f32) * 0.25,
                                ((menu_index % 8 + 1) as f32) * 0.125,
                            ],
                        );
                        match entry.state {
                            0 => quad(
                                "lock",
                                [62.0, top + 10.0, 152.0, top + 100.0],
                                [0.0, 0.0, 1.0, 1.0],
                            ),
                            1 => quad(
                                "new",
                                [62.0, top + 35.0, 158.0, top + 75.0],
                                [0.0, 0.0, 1.0, 1.0],
                            ),
                            3 => quad(
                                "star",
                                [76.0, top + 33.0, 120.0, top + 77.0],
                                [0.0, 0.0, 1.0, 1.0],
                            ),
                            _ => {}
                        }
                    }
                }
                for c in 0..3 {
                    let actual = pixels[((y * 1920 + x) * 4 + c) as usize] as f32;
                    let error = (actual - expected[c] * 255.0).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Start group{id} pixel{x},{y} channel{c}: GPU{actual} CPU{} error{error}",
                        expected[c] * 255.0
                    );
                }
                checks += 1;
            }
        }
        std::fs::write(
            format!("/private/tmp/shin-start-chapters-{id}.rgba"),
            pixels,
        )
        .unwrap();
        println!("Start chapters group{id}: {checks} sampled pixels; maxRGBerror{worst}");
    }
}
