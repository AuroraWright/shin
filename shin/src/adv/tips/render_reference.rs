//! Independent supplied-asset/native-geometry GPU oracle for TipsMode.
use crate::adv::{characters::view::CharacterTextures, tips::TipsModel, tips::text::TipsText, tips::view};
use crate::render::PreRenderContext;
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    render_pass::RenderPass,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::sync::Arc;

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_tips_gpu_artwork_text_and_native_geometry() {
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
        label: Some("Tips reference depth"),
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
        label: Some("Tips reference target"),
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
        let mut b = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut b).unwrap();
        b
    };
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let original =
        shin_core::format::texture_archive::read_texture_archive(&read("/tips.txa")).unwrap();
    let shared =
        shin_core::format::texture_archive::read_texture_archive(&read("/systex.txa")).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/tips.txa").unwrap();
    let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
    let sample = |sys: bool, name: &str, u: f32, v: f32| -> [f32; 4] {
        let source = if sys { &shared } else { &original };
        let im = &source.textures[source.name_to_index[name]];
        let x = u * im.width() as f32 - 0.5;
        let y = v * im.height() as f32 - 0.5;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        std::array::from_fn(|channel| {
            let p = |dx: f32, dy: f32| {
                im.get_pixel(
                    (ix + dx).clamp(0.0, im.width() as f32 - 1.0) as u32,
                    (iy + dy).clamp(0.0, im.height() as f32 - 1.0) as u32,
                )[channel] as f32
                    / 255.0
            };
            (p(0.0, 0.0) * (1.0 - fx) + p(1.0, 0.0) * fx) * (1.0 - fy)
                + (p(0.0, 1.0) * (1.0 - fx) + p(1.0, 1.0) * fx) * fy
        })
    };
    let ids = [1, 2, 7];
    let text = TipsText::load(&server, &scenario.info_tables().tips_info[1]);
    let mut checked = 0;
    let mut max_error = 0.0_f32;
    for case in 0..8 {
        let mut model = TipsModel::new(&scenario.info_tables().tips_info, -1, Some(&ids)).unwrap();
        for row in 0..3 {
            model.set_row_height(row, if case == 7 { 600.0 } else { 2000.25 });
        }
        let p = match case {
            0 => 0.0,
            1 => 0.25,
            2 => 0.5,
            _ => 1.0,
        };
        model.update(p * 10.0);
        let mut focus = [1.0, 0.0, 0.0];
        let mut flash = [0.0; 3];
        let mut close_flash = 0.0;
        if case == 4 {
            let mut input = enum_map::EnumMap::default();
            input[crate::app::AppAction::AnyDown] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
            model.input(&input);
            model.update(2.0);
            focus = [0.8, 0.5, 0.0];
        }
        if case == 5 {
            use crate::adv::input_area::InputAreaTarget;
            model.probe(glam::vec2(100.0, 380.0));
            model.press(glam::Vec2::ZERO);
            model.update(1.0);
            focus = [0.9, 0.0, 0.25];
            flash[2] = 0.2;
        }
        if case == 6 {
            use crate::adv::input_area::InputAreaTarget;
            model.probe(glam::vec2(1800.0, 80.0));
            model.press(glam::Vec2::ZERO);
            model.update(1.0);
            close_flash = 0.2;
            model.reset();
            model.probe(glam::vec2(700.0, 500.0));
            model.drag(glam::vec2(700.0, 500.0), glam::vec2(700.0, 99.1));
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Tips native artwork reference",
            );
            pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
            view::render(
                &model,
                &text,
                &atlas,
                &system,
                &mut pass,
                crate::render::top_left_projection_matrix(),
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let shift = 1920.0 * (1.0 - p);
        let top = -160.0 + 160.0 * p;
        for y in (1..1080).step_by(5) {
            for x in (1..1920).step_by(5) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let local = px - shift;
                // Font/divider pixels have a separate full-frame scalar oracle below.
                if (602.0..1834.0).contains(&local) && (183.0..923.0).contains(&py) {
                    continue;
                }
                let mut color = [0.0_f32; 3];
                let mut draw = |sys: bool,
                                name: &str,
                                r: [f32; 4],
                                uv: [f32; 4],
                                alpha: f32,
                                mode: u8,
                                select: f32,
                                flash: f32,
                                tint: f32| {
                    if px < r[0] || px >= r[2] || py < r[1] || py >= r[3] {
                        return;
                    }
                    let source = sample(
                        sys,
                        name,
                        uv[0] + (uv[2] - uv[0]) * (px - r[0]) / (r[2] - r[0]),
                        uv[1] + (uv[3] - uv[1]) * (py - r[1]) / (r[3] - r[1]),
                    );
                    let a = source[3] * alpha;
                    for c in 0..3 {
                        let rgb =
                            ((source[c] * (1.0 - select) + select) * (1.0 - flash) + flash) * tint;
                        color[c] = if mode == 1 {
                            rgb
                        } else {
                            rgb * a + color[c] * if mode == 3 { 1.0 } else { 1.0 - a }
                        };
                        color[c] = (color[c].clamp(0.0, 1.0) * 255.0).round() / 255.0;
                    }
                };
                draw(
                    false,
                    "bg",
                    [0.0, 0.0, 1920.0, 1080.0],
                    [0.0, 0.0, 1.0, 1.0],
                    (p * 255.0).round() / 255.0,
                    2,
                    0.0,
                    0.0,
                    1.0,
                );
                draw(
                    false,
                    "obi",
                    [0.0, top, 1920.0, top + 160.0],
                    [0.0, 0.0, 1.0, 0.5],
                    64.0 / 255.0,
                    3,
                    0.0,
                    0.0,
                    1.0,
                );
                let bottom = 1080.0 - 160.0 * p;
                draw(
                    false,
                    "obi",
                    [0.0, bottom, 1920.0, bottom + 160.0],
                    [0.0, 0.5, 1.0, 1.0],
                    64.0 / 255.0,
                    3,
                    0.0,
                    0.0,
                    1.0,
                );
                let xs = [0.0, 80.0, 1840.0, 1920.0];
                let ys = [0.0, 80.0, 810.0, 898.0];
                for row in 0..3 {
                    for col in 0..3 {
                        draw(
                            false,
                            "window",
                            [
                                xs[col] + shift,
                                ys[row] + 112.0,
                                xs[col + 1] + shift,
                                ys[row + 1] + 112.0,
                            ],
                            [
                                xs[col] / 1920.0,
                                ys[row] / 898.0,
                                xs[col + 1] / 1920.0,
                                ys[row + 1] / 898.0,
                            ],
                            1.0,
                            if row == 1 && col == 1 { 1 } else { 2 },
                            0.0,
                            0.0,
                            1.0,
                        );
                    }
                }
                draw(
                    false,
                    "separator",
                    [568.0 + shift, 203.0, 586.0 + shift, 903.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    0.0,
                    1.0,
                );
                for row in 0..3 {
                    let y = 193.0 + row as f32 * 88.0;
                    let band =
                        scenario.info_tables().tips_info[ids[row] as usize].title_index as f32;
                    draw(
                        false,
                        "menu",
                        [90.0 + shift, y, 548.0 + shift, y + 88.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        2,
                        focus[row] * 0.5,
                        flash[row],
                        1.0,
                    );
                    draw(
                        false,
                        "items",
                        [90.0 + shift, y + 12.0, 548.0 + shift, y + 76.0],
                        [0.0, band / 26.0, 1.0, (band + 1.0) / 26.0],
                        1.0,
                        2,
                        focus[row] * 0.5,
                        flash[row],
                        1.0,
                    );
                }
                for (a, b, alpha) in [
                    (183.0, 188.0, 1.0 - (py - 183.0) / 5.0),
                    (918.0, 923.0, (py - 918.0) / 5.0),
                ] {
                    draw(
                        false,
                        "window",
                        [86.0 + shift, a, 1834.0 + shift, b],
                        [
                            86.0 / 1920.0,
                            (a - 112.0) / 898.0,
                            1834.0 / 1920.0,
                            (b - 112.0) / 898.0,
                        ],
                        alpha,
                        2,
                        0.0,
                        0.0,
                        1.0,
                    );
                }
                if case != 7 {
                    let scroll = if case == 6 { 400.0 } else { 0.0 };
                    let start = 714.0 * scroll / 2001.0;
                    let end = 714.0 * (scroll + 730.0) / 2001.0;
                    for (u, sy) in [
                        (0.0, [188.0, 196.0, 910.0, 918.0]),
                        (
                            0.5,
                            [188.0 + start, 196.0 + start, 196.0 + end, 204.0 + end],
                        ),
                    ] {
                        for band in 0..3 {
                            draw(
                                true,
                                "scroll",
                                [1838.0 + shift, sy[band], 1854.0 + shift, sy[band + 1]],
                                [u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25],
                                1.0,
                                2,
                                0.0,
                                0.0,
                                1.0,
                            );
                        }
                    }
                }
                draw(
                    false,
                    "caption",
                    [480.0, top, 1440.0, top + 128.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    0.0,
                    1.0,
                );
                draw(
                    false,
                    "close",
                    [1764.0, top + 32.0, 1848.0, top + 116.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    close_flash,
                    1.0,
                );
                let hy = 1100.0 - 88.0 * p;
                for (left, right, ly, width, button, tint) in [
                    (1800.0, 1872.0, 48.0, 72.0, 4, 1.0),
                    (
                        1568.0,
                        1742.0,
                        144.0,
                        174.0,
                        9,
                        if case == 7 { 128.0 / 255.0 } else { 1.0 },
                    ),
                    (1304.0, 1510.0, 0.0, 206.0, 1, 1.0),
                ] {
                    draw(
                        false,
                        "help_tips",
                        [left, hy, right, hy + 48.0],
                        [0.0, ly / 192.0, width / 320.0, (ly + 48.0) / 192.0],
                        1.0,
                        2,
                        0.0,
                        0.0,
                        tint,
                    );
                    let u = (button & 7) as f32 / 8.0;
                    let v = (button >> 3) as f32 / 2.0;
                    draw(
                        false,
                        "key",
                        [left - 48.0, hy, left, hy + 48.0],
                        [u, v, u + 0.125, v + 0.5],
                        1.0,
                        2,
                        0.0,
                        0.0,
                        tint,
                    );
                }
                for channel in 0..3 {
                    let error = (pixels[(y * 1920 + x) * 4 + channel] as f32
                        - color[channel] * 255.0)
                        .abs();
                    max_error = max_error.max(error);
                    assert!(
                        error <= 2.1,
                        "Tips artwork case{case} ({x},{y}) channel{channel}: actual{} wanted{} error{error}",
                        pixels[(y * 1920 + x) * 4 + channel],
                        color[channel] * 255.0
                    );
                }
                checked += 1;
            }
        }
        if case == 3 {
            let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
            for px in pixels.chunks_exact(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write("/tmp/shin-tips-screen-reference.ppm", ppm).unwrap();
        }
    }
    eprintln!("Tips artwork: {checked} native/scalar samples,8 cases,max RGB error{max_error}");
    super::text::assert_reference_tips_text(&mut context, &server, &scenario);
}
