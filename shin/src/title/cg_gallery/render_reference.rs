//! Original-bitmap oracle independent of CG renderer geometry helpers.
use crate::{adv::characters::view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_cg_gpu_artwork_clipping_counts_flash_scrollbar_and_footer() {
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
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
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
    let mut read = |name: &str| {
        let file = rom.find_file(name).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    #[derive(Clone)]
    struct CpuImage {
        width: u32,
        height: u32,
        data: Vec<u8>,
    }
    fn sample(im: &CpuImage, u: f32, v: f32) -> [f32; 4] {
        let x = u * im.width as f32 - 0.5;
        let y = v * im.height as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|c| {
            let p = |dx: f32, dy: f32| {
                let x = (ix + dx).clamp(0.0, im.width as f32 - 1.0) as u32;
                let y = (iy + dy).clamp(0.0, im.height as f32 - 1.0) as u32;
                im.data[((y * im.width + x) * 4) as usize + c] as f32 / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    }
    fn color(v: [f32; 4]) -> [f32; 4] {
        v.map(|v| (v * 255.0).clamp(0.0, 255.0) as u8 as f32 / 255.0)
    }
    fn over(dst: &mut [f32; 4], src: [f32; 4]) {
        for c in 0..3 {
            dst[c] = src[c] * src[3] + dst[c] * (1.0 - src[3]);
        }
        dst[3] = src[3] + dst[3] * (1.0 - src[3]);
    }
    fn quad(
        dst: &mut [f32; 4],
        im: &CpuImage,
        rect: [f32; 4],
        uv: [f32; 4],
        tint: [f32; 4],
        x: f32,
        y: f32,
    ) {
        if x < rect[0] || x >= rect[2] || y < rect[1] || y >= rect[3] {
            return;
        }
        let u = uv[0] + (uv[2] - uv[0]) * (x - rect[0]) / (rect[2] - rect[0]);
        let v = uv[1] + (uv[3] - uv[1]) * (y - rect[1]) / (rect[3] - rect[1]);
        let sampled = sample(im, u, v);
        over(dst, std::array::from_fn(|c| sampled[c] * tint[c]));
    }
    let scenario: Arc<shin_core::format::scenario::Scenario> =
        server.load_sync("/main.snr").unwrap();
    let info = &scenario.info_tables().picture_box_info;
    if std::env::var_os("SHIN_REFERENCE_GALLERY_COMPLETION_ONLY").is_none() {
        let original =
            shin_core::format::texture_archive::read_texture_archive(&read("/cgmode.txa")).unwrap();
        let cpu = original
            .textures
            .iter()
            .map(|im| CpuImage {
                width: im.width(),
                height: im.height(),
                data: im.as_raw().clone(),
            })
            .collect::<Vec<_>>();
        let atlas: Arc<CharacterTextures> = server.load_sync("/cgmode.txa").unwrap();
        let system_original =
            shin_core::format::texture_archive::read_texture_archive(&read("/systex.txa")).unwrap();
        let sc = &system_original.textures[system_original.name_to_index["scroll"]];
        let scroll_image = CpuImage {
            width: sc.width(),
            height: sc.height(),
            data: sc.as_raw().clone(),
        };
        let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();

        let im = |name: &str| &cpu[original.name_to_index[name]];
        for case in 0..4 {
            use shin_core::time::Ticks;
            let mut save = SaveData::default();
            if case > 0 {
                for (i, row) in info.iter().enumerate() {
                    for (j, id) in row.picture_ids.0.iter().enumerate() {
                        if case == 2 || ((case == 1 || i % 2 == 0) && j == 0) {
                            save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
                        }
                    }
                }
            }
            let mut model = super::CgGalleryList::new(info, &save).unwrap();
            if case == 1 {
                model.select(15, true);
                model.update(Ticks::from_f32(2.0));
                model.play();
                model.update(Ticks::from_f32(1.0));
            }
            if case == 2 {
                model.analog_scroll(23.125);
                model.update(Ticks::from_f32(1.0));
                model.play();
                model.update(Ticks::from_f32(2.0));
            }
            if case == 3 {
                model.select(118, true);
                model.update(Ticks::from_f32(4.0));
                model.pointer_probe(glam::vec2(1800.0, 80.0));
                model.pointer_press();
                model.update(Ticks::from_f32(2.0));
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "CG artwork reference",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::from_rgba(
                        255, 0, 255, 255,
                    )),
                    None,
                    None,
                );
                super::view::render_artwork(&model, &atlas, &system, &mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let mut worst = 0.0f32;
            let mut variant_samples = 0usize;
            for y in (1..1080).step_by(4) {
                for x in (1..1920).step_by(4) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut expected = sample(im("bg"), px / 1920.0, py / 1080.0);
                    if (136.0..1784.0).contains(&px) && (185.0..921.0).contains(&py) {
                        for (i, row) in model.rows().iter().enumerate() {
                            let (focus, flash) = model.feedback(i);
                            let scale = 1.0 + focus * 0.05;
                            let cx = 342.0 + (i % 4) as f32 * 412.0;
                            let cy = 311.0 + (i / 4) as f32 * 242.0 - model.scroll();
                            let rect = [
                                cx - 192.0 * scale,
                                cy - 108.0 * scale,
                                cx + 192.0 * scale,
                                cy + 108.0 * scale,
                            ];
                            if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                                continue;
                            }
                            expected = sample(
                                im(&row.thumbnail),
                                (px - rect[0]) / (384.0 * scale),
                                (py - rect[1]) / (216.0 * scale),
                            );
                            for c in 0..3 {
                                expected[c] = expected[c] * (1.0 - flash) + flash;
                            }
                            let unlocked = row.unlocked_count();
                            if unlocked == 0 {
                                quad(
                                    &mut expected,
                                    im("logo"),
                                    rect,
                                    [0.0, 0.0, 1.0, 1.0],
                                    [1.0; 4],
                                    px,
                                    py,
                                );
                            } else if unlocked < row.pictures.len() {
                                let text = format!("{unlocked}/{}", row.pictures.len());
                                let mut pen = 376.0 - text.len() as f32 * 19.0;
                                for ch in text.bytes() {
                                    let u = if ch == b'/' {
                                        240.0
                                    } else {
                                        (ch - b'0') as f32 * 24.0
                                    };
                                    let glyph_rect = [
                                        rect[0] + (pen - 2.5) * scale,
                                        rect[1] + 180.0 * scale,
                                        rect[0] + (pen + 21.5) * scale,
                                        rect[1] + 212.0 * scale,
                                    ];
                                    if px >= glyph_rect[0]
                                        && px < glyph_rect[2]
                                        && py >= glyph_rect[1]
                                        && py < glyph_rect[3]
                                    {
                                        variant_samples += 1;
                                    }
                                    quad(
                                        &mut expected,
                                        im("num"),
                                        [
                                            rect[0] + (pen - 2.5) * scale,
                                            rect[1] + 180.0 * scale,
                                            rect[0] + (pen + 21.5) * scale,
                                            rect[1] + 212.0 * scale,
                                        ],
                                        [u / 512.0, 0.0, (u + 24.0) / 512.0, 0.4],
                                        [1.0; 4],
                                        px,
                                        py,
                                    );
                                    pen += 19.0;
                                }
                            }
                            for alpha in [focus * 0.5, flash] {
                                over(&mut expected, color([1.0, 1.0, 1.0, alpha]));
                            }
                        }
                        let alpha = if py < 190.0 {
                            (190.0 - py) / 5.0
                        } else if py >= 916.0 {
                            (py - 916.0) / 5.0
                        } else {
                            0.0
                        };
                        if alpha > 0.0 {
                            let mut bg = sample(im("bg"), px / 1920.0, py / 1080.0);
                            bg[3] *= alpha;
                            over(&mut expected, bg);
                        }
                    }
                    let start = 710.0 * model.scroll().trunc() / 7260.0;
                    let end = 710.0 * (model.scroll().trunc() + 726.0) / 7260.0;
                    for (u, ys) in [
                        (0.0, [190.0, 198.0, 908.0, 916.0]),
                        (
                            0.5,
                            [190.0 + start, 198.0 + start, 198.0 + end, 206.0 + end],
                        ),
                    ] {
                        for band in 0..3 {
                            quad(
                                &mut expected,
                                &scroll_image,
                                [1788.0, ys[band], 1804.0, ys[band + 1]],
                                [u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25],
                                [1.0; 4],
                                px,
                                py,
                            );
                        }
                    }
                    if (1764.0..1848.0).contains(&px) && (32.0..116.0).contains(&py) {
                        let mut tex = sample(im("close"), (px - 1764.0) / 84.0, (py - 32.0) / 84.0);
                        for c in 0..3 {
                            tex[c] = tex[c] * (1.0 - model.close_flash()) + model.close_flash();
                        }
                        over(&mut expected, tex);
                    }
                    let mut right = 1872.0;
                    for (row, keys, width) in
                        [(1, [4, -1], 72.0), (2, [3, -1], 74.0), (0, [7, 0], 206.0)]
                    {
                        let rgb =
                            if row == 2 && model.rows()[model.selected()].unlocked_count() == 0 {
                                128.0 / 255.0
                            } else {
                                1.0
                            };
                        let tint = [rgb, rgb, rgb, 1.0];
                        let left = right - width;
                        quad(
                            &mut expected,
                            im("help_cgmode"),
                            [left, 1012.0, right, 1060.0],
                            [0.0, row as f32 / 4.0, width / 320.0, (row + 1) as f32 / 4.0],
                            tint,
                            px,
                            py,
                        );
                        let mut icon_left = left + 2.0;
                        for key in keys.into_iter().filter(|key| *key >= 0) {
                            icon_left -= 48.0;
                            let u = (key & 7) as f32 / 8.0;
                            let v = (key >> 3) as f32 / 2.0;
                            quad(
                                &mut expected,
                                im("key"),
                                [icon_left, 1012.0, icon_left + 48.0, 1060.0],
                                [u, v, u + 0.125, v + 0.5],
                                tint,
                                px,
                                py,
                            );
                        }
                        right = icon_left - 16.0;
                    }
                    for c in 0..3usize {
                        let error = (pixels[((y * 1920 + x) * 4) as usize + c] as f32
                            - expected[c] * 255.0)
                            .abs();
                        worst = worst.max(error);
                        assert!(error <= 3.0, "CG case{case} pixel{x},{y} c{c} error{error}");
                    }
                }
            }
            if case == 1 || case == 3 {
                assert!(
                    variant_samples > 0,
                    "partial variant glyphs sampled in case{case}"
                );
            }
            eprintln!(
                "CG GPU case{case}: max RGB error{worst}, variant glyph samples{variant_samples}"
            );
        }

        // 4b6c0 draws original-slot markers even before initial picture publication.
        assert_eq!(
            original.textures[original.name_to_index["check"]].dimensions(),
            (32, 40)
        );
        let index = info
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| row.picture_ids.0.len())
            .unwrap()
            .0;
        let single = info
            .iter()
            .position(|row| row.picture_ids.0.len() == 1)
            .unwrap();
        for case in 0..4 {
            let mut save = SaveData::default();
            if case > 0 {
                for (i, id) in info[index].picture_ids.0.iter().enumerate() {
                    if i % 2 == 0 {
                        save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
                    }
                }
            }
            let gallery = super::CgGalleryList::new(info, &save).unwrap();
            let row = &gallery.rows()[if case == 3 { single } else { index }];
            let mut model = crate::title::cg_viewer::CgViewerNavigation::new(row);
            if case == 1 {
                model.pointer_release(row);
            }
            if case == 2 {
                model.controller(
                    row,
                    crate::title::cg_viewer::ViewerControls {
                        indicator_toggle: true,
                        ..Default::default()
                    },
                );
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "CG variant indicator reference",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    None,
                    None,
                );
                crate::title::cg_viewer::view::render_indicators(&model, row, &atlas, &mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let mut worst = 0.0f32;
            for y in 1010..1080 {
                for x in 1250..1920 {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut expected = [0.0, 0.0, 0.0, 1.0];
                    if model.indicators() && row.pictures.len() >= 2 {
                        for (i, unlocked) in row.unlocked.iter().enumerate() {
                            let tint = if model.ordinal() == i as i32 {
                                [144.0 / 255.0, 1.0, 128.0 / 255.0, 1.0]
                            } else if *unlocked {
                                [1.0; 4]
                            } else {
                                [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0]
                            };
                            let left =
                                (1905 - 30 * row.pictures.len() as i32 + 30 * i as i32) as f32;
                            quad(
                                &mut expected,
                                im("check"),
                                [left, 1025.0, left + 32.0, 1065.0],
                                [0.0, 0.0, 1.0, 1.0],
                                tint,
                                px,
                                py,
                            );
                        }
                    }
                    for c in 0..3usize {
                        let error = (pixels[((y * 1920 + x) * 4) as usize + c] as f32
                            - expected[c] * 255.0)
                            .abs();
                        worst = worst.max(error);
                        assert!(
                            error <= 2.0,
                            "CG indicators case{case} pixel{x},{y} c{c} error{error}"
                        );
                    }
                }
            }
            eprintln!(
                "CG indicator GPU case{case}, slots={}, max RGB error{worst}",
                row.pictures.len()
            );
        }
    }
    // CG footer uses the original SimpleText font, weighted variant completion.
    let font: Arc<crate::asset::font::GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
    for mode in 0..3 {
        let mut save = SaveData::default();
        let first = info[0].picture_ids.0[0];
        for row in info {
            for id in &row.picture_ids.0 {
                if mode == 2 || (mode == 1 && *id == first) {
                    save.save_vectors.unlocked_cgs.add(id.into_i32() as usize);
                }
            }
        }
        let unlocked = info
            .iter()
            .flat_map(|row| &row.picture_ids.0)
            .filter(|id| {
                save.save_vectors
                    .unlocked_cgs
                    .contains(id.into_i32() as usize)
            })
            .count();
        let tenths = unlocked * 1000 / 299;
        let expected = if mode == 2 {
            "達成率:100％".to_owned()
        } else {
            format!("達成率:{}.{}％", tenths / 10, tenths % 10)
        };
        let gallery = super::CgGalleryList::new(info, &save).unwrap();
        assert_eq!(gallery.completion_label(), expected);
        let text = super::text::CgCompletionText::new(&server, &font, &gallery);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "CG completion font reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                None,
                None,
            );
            text.render(&mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut changed = 0usize;
        let mut white = 0usize;
        for (i, pixel) in pixels.chunks_exact(4).enumerate() {
            if pixel[..3] != [0, 0, 0] {
                changed += 1;
                let x = i % 1920;
                let y = i / 1920;
                assert!(
                    (40..700).contains(&x) && (1001..1080).contains(&y),
                    "CG completion escaped native region{x},{y}"
                );
                if pixel[0].abs_diff(255) <= 2
                    && pixel[1].abs_diff(255) <= 2
                    && pixel[2].abs_diff(255) <= 2
                {
                    white += 1;
                }
            }
        }
        assert!(
            changed > 1000 && white > 100,
            "CG original completion glyphs/fill missing"
        );
        eprintln!("CG completion {expected}: {changed} changed pixels, {white} white glyph pixels");
    }
    let empty = SaveData::default();
    let music_list =
        crate::title::music_gallery::MusicGalleryList::new(&scenario.info_tables().music_box_info, &empty)
            .unwrap();
    let music = crate::title::music_gallery::text::MusicCompletionText::new(&server, &font, &music_list);
    let movie_list =
        crate::title::movie_gallery::MovieGalleryList::new(&scenario.info_tables().movie_info, &empty)
            .unwrap();
    let movie = crate::title::movie_gallery::text::MovieCompletionText::new(&server, &font, &movie_list);
    for gallery in ["Music", "Movie"] {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::RenderTexture,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Gallery white completion footer",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                None,
                None,
            );
            if gallery == "Music" {
                music.render(&mut pass);
            } else {
                movie.render(&mut pass);
            }
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let white = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().all(|&c| c >= 253))
            .count();
        assert!(white > 100, "{gallery} completion glyph fill must be white");
        eprintln!("{gallery} completion: {white} white glyph pixels");
    }
}
