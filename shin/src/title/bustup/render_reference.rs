//! Original bitmap oracle: no production Bustup artwork or geometry helpers.
use crate::{adv::characters::view::CharacterTextures, render::PreRenderContext};
use std::sync::Arc;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_bustup_gpu_original_artwork_focus_flash_counts_and_slides() {
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

    use crate::title::{
        bustup::catalog::BustupCatalog,
        bustup::menu::{BustupMenu, Controls},
    };
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
        time::Ticks,
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
        let mut b = Vec::new();
        rom.open_file(rom.find_file(name).unwrap())
            .unwrap()
            .read_to_end(&mut b)
            .unwrap();
        b
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

    let original =
        shin_core::format::texture_archive::read_texture_archive(&read("/bupmode.txa")).unwrap();
    let images = original
        .textures
        .iter()
        .map(|im| CpuImage {
            width: im.width(),
            height: im.height(),
            data: im.as_raw().clone(),
        })
        .collect::<Vec<_>>();
    let atlas: Arc<CharacterTextures> = server.load_sync("/bupmode.txa").unwrap();
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let catalog = BustupCatalog::new(&scenario.info_tables().character_box_info).unwrap();
    let input = |menu: &mut BustupMenu, controls| {
        let mut eligible = true;
        menu.controller(&catalog, &mut eligible, false, controls);
    };
    struct Quad {
        name: String,
        rect: [f32; 4],
        uv: [f32; 4],
        tint: [f32; 4],
        flash: f32,
    }
    for case in 0..6 {
        let mut menu = BustupMenu::new(&catalog);
        if case == 1 || case == 2 {
            let (ci, oi, pi) = catalog
                .characters
                .iter()
                .enumerate()
                .flat_map(|(ci, c)| {
                    c.outfits.iter().enumerate().flat_map(move |(oi, o)| {
                        o.poses
                            .iter()
                            .enumerate()
                            .map(move |(pi, p)| (ci, oi, pi, p))
                    })
                })
                .find(|(_, _, _, p)| p.expressions.len() >= 12)
                .map(|(ci, oi, pi, _)| (ci, oi, pi))
                .unwrap();
            for _ in 0..ci {
                input(
                    &mut menu,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            input(
                &mut menu,
                Controls {
                    down: true,
                    ..Default::default()
                },
            );
            for _ in 0..oi {
                input(
                    &mut menu,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            input(
                &mut menu,
                Controls {
                    down: true,
                    ..Default::default()
                },
            );
            for _ in 0..pi {
                input(
                    &mut menu,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            input(
                &mut menu,
                Controls {
                    down: true,
                    ..Default::default()
                },
            );
            let n = if case == 1 {
                10
            } else {
                catalog.characters[ci].outfits[oi].poses[pi]
                    .expressions
                    .len()
                    - 1
            };
            for _ in 0..n {
                input(
                    &mut menu,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            assert_eq!(menu.selection().indices(), (ci, oi, pi, n));
            menu.update(Ticks::from_f32(2.0));
            assert!(menu.arrow_values()[7] > 0.0);
        }
        if case == 3 {
            for _ in 0..4 {
                input(
                    &mut menu,
                    Controls {
                        down: true,
                        ..Default::default()
                    },
                );
            }
            for _ in 1..catalog.backgrounds.len() {
                input(
                    &mut menu,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            menu.pointer_probe(&catalog, glam::vec2(1800.0, 60.0));
            menu.pointer_press();
            menu.update(Ticks::from_f32(2.0));
            assert_eq!(
                menu.selection().background_index(),
                catalog.backgrounds.len() - 1
            );
            assert!(menu.button_values()[0] > 0.0);
        }
        let controls = if case == 4 {
            0.5
        } else if case == 5 {
            0.0
        } else {
            1.0
        };
        let shift = if case == 4 {
            500.0 * (std::f32::consts::FRAC_PI_4).sin()
        } else if case == 5 {
            500.0
        } else {
            0.0
        };
        if case >= 4 {
            input(
                &mut menu,
                Controls {
                    confirm: true,
                    ..Default::default()
                },
            );
            menu.update(Ticks::from_f32(if case == 4 { 15.0 } else { 30.0 }));
        }
        // Independent native helper inputs, fixed rectangles, texture dimensions
        // and blend equations. Never consume the renderer's Draw list.
        let mut quads = Vec::new();
        let mut add = |name: String,
                       x: f32,
                       y: f32,
                       w: f32,
                       h: f32,
                       uv: [f32; 4],
                       tint: [f32; 4],
                       flash: f32| {
            quads.push(Quad {
                name,
                rect: [x, y, x + w, y + h],
                uv,
                tint: color(tint),
                flash,
            });
        };
        let (c, o, p, e) = menu.selection().indices();
        let current = [c, o, p, e, menu.selection().background_index()];
        let totals = [
            catalog.characters.len(),
            catalog.characters[c].outfits.len(),
            catalog.characters[c].outfits[o].poses.len(),
            catalog.characters[c].outfits[o].poses[p].expressions.len(),
            catalog.backgrounds.len(),
        ];
        eprintln!(
            "Bustup GPU case{case}: selected{current:?}, counts{totals:?}, controls{controls}"
        );
        let focus = menu.row() as usize;
        for (row, (x, y, size, u, text)) in [
            (1504.0, 248.0, 144.0, 0.0, 0),
            (1520.0, 392.0, 112.0, 0.5, 2),
            (1520.0, 504.0, 112.0, 0.5, 3),
            (1520.0, 616.0, 112.0, 0.5, 4),
            (1504.0, 728.0, 144.0, 0.0, 5),
        ]
        .into_iter()
        .enumerate()
        {
            let x = x + shift;
            add(
                "icon".into(),
                x,
                y,
                size,
                size,
                [u, 0.0, u + size / 288.0, size / 288.0],
                [1.0; 4],
                0.0,
            );
            if row == 0 || row == 4 {
                add(
                    if row == 0 {
                        format!("char{:02}", c + 1)
                    } else {
                        format!("bg{:03}", current[4] + 1)
                    },
                    x,
                    y,
                    144.0,
                    144.0,
                    [0.0, 0.0, 1.0, 1.0],
                    [1.0; 4],
                    0.0,
                );
            } else {
                let span = if current[row] >= 9 { 48.0 } else { 32.0 }
                    + if totals[row] >= 10 { 32.0 } else { 16.0 };
                let mut pen = x + 56.0 - span * 0.5;
                for byte in format!("{}/{}", current[row] + 1, totals[row]).bytes() {
                    let column = if byte == b'/' { 10 } else { byte - b'0' } as f32;
                    add(
                        "num".into(),
                        pen - 1.0,
                        y + 36.0,
                        18.0,
                        40.0,
                        [
                            column * 32.0 / 352.0,
                            0.0,
                            (column * 32.0 + 18.0) / 352.0,
                            1.0,
                        ],
                        [0.8, 0.73333335, 0.6666667, 1.0],
                        0.0,
                    );
                    pen += 16.0;
                }
            }
            if focus == row {
                add(
                    "icon".into(),
                    x,
                    y,
                    size,
                    size,
                    [u, 0.5, u + size / 288.0, 0.5 + size / 288.0],
                    [1.0, 1.0, 1.0, 0.3],
                    0.0,
                );
            }
            add(
                "text".into(),
                x + size / 2.0 - 72.0,
                y + size - 36.0,
                144.0,
                32.0,
                [0.0, text as f32 / 6.0, 1.0, (text + 1) as f32 / 6.0],
                [1.0; 4],
                0.0,
            );
            let (left, right, ay) = [
                (1424.0, 1648.0, 280.0),
                (1440.0, 1632.0, 408.0),
                (1440.0, 1632.0, 520.0),
                (1440.0, 1632.0, 632.0),
                (1424.0, 1648.0, 760.0),
            ][row];
            for (side, ax) in [left, right].into_iter().enumerate() {
                let enabled = if side == 0 {
                    current[row] > 0
                } else {
                    current[row] + 1 < totals[row]
                };
                let tint = if enabled {
                    [1.0; 4]
                } else {
                    [0.4, 0.4, 0.4, 1.0]
                };
                let u = side as f32 / 2.0;
                add(
                    "arrow".into(),
                    ax + shift,
                    ay,
                    80.0,
                    80.0,
                    [u, 0.0, u + 0.5, 0.5],
                    tint,
                    menu.arrow_values()[row * 2 + side],
                );
                if focus == row {
                    add(
                        "arrow".into(),
                        ax + shift,
                        ay,
                        80.0,
                        80.0,
                        [u, 0.5, u + 0.5, 1.0],
                        [1.0, 1.0, 1.0, 0.3],
                        0.0,
                    );
                }
            }
        }
        if controls > 0.0 {
            for (i, (name, x)) in [("close", 1764.0), ("hidden", 1668.0)]
                .into_iter()
                .enumerate()
            {
                add(
                    name.into(),
                    x,
                    controls * 150.0 - 118.0,
                    84.0,
                    84.0,
                    [0.0, 0.0, 1.0, 1.0],
                    [1.0; 4],
                    menu.button_values()[i],
                );
            }
            let y = 1100.0 - controls * 88.0;
            for (row, left, width, keys) in [
                (1, 1792.0, 80.0, [4, -1]),
                (2, 1556.0, 180.0, [3, -1]),
                (3, 1286.0, 214.0, [7, 2]),
                (0, 970.0, 214.0, [1, -1]),
            ] {
                add(
                    "help_bupmode".into(),
                    left,
                    y,
                    width,
                    48.0,
                    [0.0, row as f32 / 4.0, width / 320.0, (row + 1) as f32 / 4.0],
                    [1.0; 4],
                    0.0,
                );
                for (i, key) in keys.into_iter().filter(|k| *k >= 0).enumerate() {
                    let u = (key & 7) as f32 / 8.0;
                    let v = (key >> 3) as f32 / 2.0;
                    add(
                        "key".into(),
                        left - (i + 1) as f32 * 46.0,
                        y,
                        48.0,
                        48.0,
                        [u, v, u + 0.125, v + 0.5],
                        [1.0; 4],
                        0.0,
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
                "Bustup original artwork reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::from_rgba(
                    23, 47, 71, 255,
                )),
                None,
                None,
            );
            super::view::render_artwork(&menu, &catalog, controls, &atlas, &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        let mut checks = 0;
        for y in (1..1080).step_by(6) {
            for x in (1..1920).step_by(6) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut expected = [23.0 / 255.0, 47.0 / 255.0, 71.0 / 255.0, 1.0];
                for q in &quads {
                    let r = q.rect;
                    if px < r[0] || px >= r[2] || py < r[1] || py >= r[3] {
                        continue;
                    }
                    let uv = q.uv;
                    let u = uv[0] + (uv[2] - uv[0]) * (px - r[0]) / (r[2] - r[0]);
                    let v = uv[1] + (uv[3] - uv[1]) * (py - r[1]) / (r[3] - r[1]);
                    let mut sampled = sample(&images[original.name_to_index[&q.name]], u, v);
                    for channel in 0..3 {
                        sampled[channel] = (sampled[channel] + (1.0 - sampled[channel]) * q.flash)
                            * q.tint[channel];
                    }
                    sampled[3] *= q.tint[3];
                    over(&mut expected, sampled);
                }
                for channel in 0..3 {
                    let error = (pixels[((y * 1920 + x) * 4) as usize + channel] as f32
                        - expected[channel] * 255.0)
                        .abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 3.0,
                        "Bustup case{case} pixel{x},{y} channel{channel} error{error}"
                    );
                }
                checks += 1;
            }
        }
        eprintln!("Bustup original artwork GPU case{case}: {checks} pixels, maxRGBerror{worst}");
    }
    crate::app::SceneHost::reference_bustup_roundtrip(&server, &mut context, &target, &view);
}
