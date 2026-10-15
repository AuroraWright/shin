//! Independent native row geometry and supplied-atlas GPU oracle.
use super::{
    character_view::CharacterTextures,
    config_view,
    config_widgets::{CONFIG_ROWS, ConfigWidget},
};
use crate::render::PreRenderContext;
use shin_core::{format::save::Settings, primitives::color::UnormColor, time::Ticks};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::Arc;

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_config_gpu_rows_native_geometry_and_flashes() {
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
        label: Some("Config rows reference depth"),
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
        label: Some("Config rows reference target"),
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
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/config.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/config.txa").unwrap();
    let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
        let im = &original.textures[original.name_to_index[name]];
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
    // Independently transcribed constructors59098..5a9c8 and row render57340/58380.
    // Tuple is title band, slider subtype or -1 for buttons, label bands.
    let pages: [&[(u8, i8, [u8; 2])]; 3] = [
        &[
            (0, 0, [0, 0]),
            (1, 0, [0, 0]),
            (2, 0, [0, 0]),
            (3, 0, [0, 0]),
            (4, -1, [1, 0]),
            (18, -1, [1, 0]),
        ],
        &[(8, 2, [0, 0]), (11, -1, [1, 0]), (15, 1, [0, 0])],
        &[
            (10, 2, [0, 0]),
            (16, -1, [5, 4]),
            (19, -1, [5, 4]),
            (17, -1, [5, 4]),
            (12, -1, [1, 0]),
        ],
    ];
    let mut checked = 0;
    let mut max_error = 0.0_f32;
    for page in 0..3 {
        for case in 0..4 {
            let value = [0, 100, 50, 75][case];
            let enabled = case == 1 || case == 3;
            let settings = Settings {
                v0_bgmvol: value,
                v1_sfxvol: value,
                v2_voicevol: value,
                v3_sysvol: value,
                v4_voicefocus: enabled,
                v5_voicepanapot: enabled,
                v9_msgspeed: value,
                v10_skipspeed: value,
                v11_disallowskipunread: enabled,
                v13_msgwinalpha: value,
                v14_showroutenavi: enabled,
                v15: enabled,
                v17_showscenetitle: enabled,
                v18_showsongtitle: enabled,
                ..Default::default()
            };
            let rows = pages[page];
            let focus = match case {
                1 => Some(1),
                2 => Some(0),
                3 => Some(rows.len() - 1),
                _ => None,
            };
            let mut widgets: Vec<_> = CONFIG_ROWS[page]
                .iter()
                .map(|&s| ConfigWidget::new(s, &settings))
                .collect();
            if let Some(index) = focus {
                widgets[index].focus(true, case != 1);
                if case == 2 {
                    widgets[index].update(Ticks::from_f32(2.0));
                }
                if case == 3 {
                    let y = index as f32 * 102.0 + 40.0;
                    widgets[index].probe(1140.0, y);
                    widgets[index].press_captured(1140.0, y);
                    widgets[index].update(Ticks::from_f32(2.0));
                }
            }
            let (ox, oy) = if case == 3 {
                (616.25, 238.25)
            } else {
                (136.0, 252.0)
            };
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "Config native rows reference",
                );
                pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
                config_view::render_rows(
                    &widgets,
                    &atlas,
                    &mut pass,
                    crate::render::top_left_projection_matrix()
                        * glam::Mat4::from_translation(glam::vec3(ox, oy, 0.0)),
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            if page == 0 && case == 1 {
                std::fs::write("/private/tmp/shin-config-rows-reference.rgba", &pixels).unwrap();
            }
            for y in (241..930).step_by(3) {
                for x in (131..1919).step_by(3) {
                    let px = x as f32 + 0.5 - ox;
                    let py = y as f32 + 0.5 - oy;
                    let mut color = [0.0_f32; 3];
                    let mut draw = |name: &str,
                                    r: [f32; 4],
                                    uv: [f32; 4],
                                    alpha: f32,
                                    selection: f32,
                                    flash: f32| {
                        if px < r[0] || px >= r[2] || py < r[1] || py >= r[3] {
                            return;
                        }
                        let source = sample(
                            name,
                            uv[0] + (uv[2] - uv[0]) * (px - r[0]) / (r[2] - r[0]),
                            uv[1] + (uv[3] - uv[1]) * (py - r[1]) / (r[3] - r[1]),
                        );
                        let a = source[3] * alpha;
                        for c in 0..3 {
                            let flashed =
                                (source[c] * (1.0 - selection) + selection) * (1.0 - flash) + flash;
                            color[c] = ((flashed * a + color[c] * (1.0 - a)).clamp(0.0, 1.0)
                                * 255.0)
                                .round()
                                / 255.0;
                        }
                    };
                    for (i, &(title, subtype, labels)) in rows.iter().enumerate() {
                        let y = i as f32 * 102.0;
                        let focused = focus == Some(i);
                        let selection = if focused {
                            if case == 1 { 0.5 } else { 0.25 }
                        } else {
                            0.0
                        };
                        if focused {
                            for r in 0..3 {
                                for c in 0..3 {
                                    let xs = [0.0, 512.0, 1136.0, 1648.0];
                                    let ys = [y, y + 12.0, y + 76.0, y + 88.0];
                                    let us = [0.0, 0.4, 0.6, 1.0];
                                    let vs = [0.0, 0.375, 0.625, 1.0];
                                    draw(
                                        "focus",
                                        [xs[c], ys[r], xs[c + 1], ys[r + 1]],
                                        [us[c], vs[r], us[c + 1], vs[r + 1]],
                                        191.0 / 255.0,
                                        0.0,
                                        0.0,
                                    );
                                }
                            }
                        }
                        draw(
                            "left",
                            [24.0, y + 8.0, 776.0, y + 80.0],
                            [0.0, title as f32 / 20.0, 1.0, title as f32 / 20.0 + 0.05],
                            1.0,
                            0.0,
                            0.0,
                        );
                        if subtype < 0 {
                            for choice in [1, 0] {
                                let x = 1132.0 + choice as f32 * 252.0;
                                let selected = choice == usize::from(enabled);
                                let u = if selected { 0.5 } else { 0.0 };
                                let v = labels[choice] as f32 / 10.0;
                                let select = if selected { selection } else { 0.0 };
                                let flash = if case == 3 && focused && choice == 0 {
                                    0.4
                                } else {
                                    0.0
                                };
                                draw(
                                    if selected { "button_on" } else { "button_off" },
                                    [x, y, x + 240.0, y + 88.0],
                                    [0.0, 0.0, 1.0, 1.0],
                                    1.0,
                                    select,
                                    flash,
                                );
                                draw(
                                    "right",
                                    [x, y + 12.0, x + 240.0, y + 76.0],
                                    [u, v, u + 0.5, v + 0.1],
                                    1.0,
                                    select,
                                    flash,
                                );
                            }
                        } else {
                            let v = (subtype as f32 + 1.0) / 4.0;
                            draw(
                                "slider",
                                [1504.0, y + 4.0, 1624.0, y + 52.0],
                                [312.0 / 648.0, v, 432.0 / 648.0, v + 0.25],
                                1.0,
                                0.0,
                                0.0,
                            );
                            draw(
                                "slider",
                                [976.0, y + 4.0, 1096.0, y + 52.0],
                                [120.0 / 648.0, v, 240.0 / 648.0, v + 0.25],
                                1.0,
                                0.0,
                                0.0,
                            );
                            draw(
                                "slider",
                                [976.0, y + 40.0, 1624.0, y + 88.0],
                                [0.0, 0.0, 1.0, 0.25],
                                1.0,
                                0.0,
                                0.0,
                            );
                            let x = 976.0 + value as f32 * 6.0;
                            draw(
                                "slider",
                                [x, y + 5.0, x + 48.0, y + 77.0],
                                [48.0 / 648.0, 0.625, 96.0 / 648.0, 1.0],
                                1.0,
                                selection,
                                0.0,
                            );
                        }
                    }
                    for i in 0..rows.len() {
                        let y = i as f32 * 102.0 + 88.0;
                        for c in 0..3 {
                            let xs = [0.0, 8.0, 1640.0, 1648.0];
                            let us = [0.0, 0.25, 0.75, 1.0];
                            draw(
                                "line",
                                [xs[c], y, xs[c + 1], y + 14.0],
                                [us[c], 0.0, us[c + 1], 1.0],
                                1.0,
                                0.0,
                                0.0,
                            );
                        }
                    }
                    let offset = (y * 1920 + x) * 4;
                    for c in 0..3 {
                        let error = (pixels[offset + c] as f32 - color[c] * 255.0).abs();
                        max_error = max_error.max(error);
                        assert!(
                            error <= 2.0,
                            "Config page{page} case{case} pixel{x},{y} channel{c}: GPU{} scalar{},error{error}",
                            pixels[offset + c],
                            color[c] * 255.0
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    eprintln!(
        "Config rows: {checked} independent native/scalar samples,12 cases,max RGB error{max_error}"
    );
}

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_config_gpu_shell_transitions_tabs_and_footer() {
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
        label: Some("Config rows reference depth"),
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
        label: Some("Config rows reference target"),
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
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/config.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/config.txa").unwrap();
    let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
        let im = &original.textures[original.name_to_index[name]];
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
    use super::{config::ConfigModel, input_area::InputAreaTarget};
    let mut checked = 0;
    let mut max_error = 0.0_f32;
    for case in 0..8 {
        let p = match case {
            0 => 0.0,
            1 => 0.25,
            2 => 0.5,
            _ => 1.0,
        };
        let mut model = ConfigModel::new(Settings::default());
        model.update(p * 10.0);
        let mut selection = [1.0, 0.0, 0.0];
        let mut flashes = [0.0; 3];
        let mut close_flash = 0.0;
        let mut reset_flash = 0.0;
        if case == 4 || case == 5 {
            let index = if case == 4 { 1 } else { 2 };
            model.change_tab(index);
            model.update(2.0);
            selection = [0.5, 0.0, 0.0];
            selection[index] = 0.5;
        }
        if case == 6 {
            model.probe(glam::vec2(1800.0, 60.0));
            model.press(glam::Vec2::ZERO);
            model.update(2.0);
            close_flash = 0.4;
        }
        if case == 7 {
            model.probe(glam::vec2(1550.0, 60.0));
            model.press(glam::Vec2::ZERO);
            model.update(2.0);

            model.reset();
            model.probe(glam::vec2(900.0, 160.0));
            model.press(glam::Vec2::ZERO);
            model.update(1.0);
            flashes[1] = 0.2;
            reset_flash = 0.6;
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Config native shell reference",
            );
            pass.clear(Some(UnormColor::BLACK), Some(128), Some(1.0));
            config_view::render(
                &model,
                &atlas,
                &mut pass,
                crate::render::top_left_projection_matrix(),
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        if case == 3 {
            std::fs::write("/private/tmp/shin-config-shell-reference.rgba", &pixels).unwrap();
        }
        let shift = 1920.0 * (1.0 - p);
        let top = -160.0 + 160.0 * p;
        for y in (1..1080).step_by(4) {
            for x in (1..1920).step_by(4) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let local = px - shift;
                // Row artwork has an independent1.64million-sample oracle above.
                if (136.0..1784.0).contains(&local) && (252.0..918.0).contains(&py) {
                    continue;
                }
                let mut color = [0.0_f32; 3];
                let mut draw = |name: &str,
                                rect: [f32; 4],
                                uv: [f32; 4],
                                alpha: f32,
                                blend: u8,
                                mix: f32,
                                flash: f32| {
                    if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                        return;
                    }
                    let u = uv[0] + (uv[2] - uv[0]) * (px - rect[0]) / (rect[2] - rect[0]);
                    let v = uv[1] + (uv[3] - uv[1]) * (py - rect[1]) / (rect[3] - rect[1]);
                    let first = sample(name, u, v);
                    let source = if name == "tab_off" {
                        let second = sample("tab_on", u, v);
                        std::array::from_fn(|c| first[c] * (1.0 - mix) + second[c] * mix)
                    } else {
                        first
                    };
                    let a = source[3] * alpha;
                    for c in 0..3 {
                        let rgb = source[c] * (1.0 - flash) + flash;
                        color[c] = if blend == 1 {
                            rgb
                        } else {
                            rgb * a + color[c] * if blend == 3 { 1.0 } else { 1.0 - a }
                        };
                        color[c] = (color[c].clamp(0.0, 1.0) * 255.0).round() / 255.0;
                    }
                };
                draw(
                    "bg",
                    [0.0, 0.0, 1920.0, 1080.0],
                    [0.0, 0.0, 1.0, 1.0],
                    (p * 255.0).floor() / 255.0,
                    2,
                    0.0,
                    0.0,
                );
                draw(
                    "obi",
                    [0.0, top, 1920.0, top + 160.0],
                    [0.0, 0.0, 1.0, 0.5],
                    63.0 / 255.0,
                    3,
                    0.0,
                    0.0,
                );
                draw(
                    "obi",
                    [0.0, 1080.0 - 160.0 * p, 1920.0, 1240.0 - 160.0 * p],
                    [0.0, 0.5, 1.0, 1.0],
                    63.0 / 255.0,
                    3,
                    0.0,
                    0.0,
                );
                for r in 0..3 {
                    for c in 0..3 {
                        let xs = [0.0, 80.0, 1744.0, 1824.0];
                        let ys = [0.0, 80.0, 746.0, 834.0];
                        draw(
                            "window",
                            [
                                shift + 48.0 + xs[c],
                                176.0 + ys[r],
                                shift + 48.0 + xs[c + 1],
                                176.0 + ys[r + 1],
                            ],
                            [
                                xs[c] / 1824.0,
                                ys[r] / 834.0,
                                xs[c + 1] / 1824.0,
                                ys[r + 1] / 834.0,
                            ],
                            1.0,
                            if c == 1 && r == 1 { 1 } else { 2 },
                            0.0,
                            0.0,
                        );
                    }
                }
                for (y1, y2, a, b) in [(247.0, 252.0, 1.0, 0.0), (918.0, 923.0, 0.0, 1.0)] {
                    let alpha = a + (b - a) * (py - y1) / (y2 - y1);
                    draw(
                        "window",
                        [shift + 136.0, y1, shift + 1784.0, y2],
                        [
                            88.0 / 1824.0,
                            (y1 - 176.0) / 834.0,
                            1736.0 / 1824.0,
                            (y2 - 176.0) / 834.0,
                        ],
                        alpha,
                        2,
                        0.0,
                        0.0,
                    );
                }
                let mut order = [0, 1, 2];
                order.sort_by(|&a, &b| selection[a].partial_cmp(&selection[b]).unwrap());
                for index in order {
                    let extra_x = 22.5 * selection[index];
                    let extra_y = 5.0625 * selection[index];
                    let x = shift + 510.0 + index as f32 * 300.0;
                    draw(
                        "tab_off",
                        [
                            x - extra_x,
                            144.0 - extra_y,
                            x + 300.0 + extra_x,
                            211.5 + extra_y,
                        ],
                        [0.0, index as f32 / 3.0, 1.0, index as f32 / 3.0 + 1.0 / 3.0],
                        1.0,
                        2,
                        selection[index],
                        flashes[index],
                    );
                }
                draw(
                    "caption",
                    [480.0, top, 1440.0, top + 128.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    0.0,
                );
                draw(
                    "close",
                    [1764.0, top + 32.0, 1848.0, top + 116.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    close_flash,
                );
                draw(
                    "default",
                    [1446.0, top + 32.0, 1752.0, top + 116.0],
                    [0.0, 0.0, 1.0, 1.0],
                    1.0,
                    2,
                    0.0,
                    reset_flash,
                );
                let y = 1100.0 - 88.0 * p;
                let mut right = 1872.0;
                for (row, key, width) in [
                    (1, 4, 72.0),
                    (3, 6, 174.0),
                    (2, 7, 236.0),
                    (4, 2, 174.0),
                    (0, 1, 206.0),
                ] {
                    let left = right - width;
                    draw(
                        "help_config",
                        [left, y, right, y + 48.0],
                        [0.0, row as f32 / 5.0, width / 320.0, (row + 1) as f32 / 5.0],
                        1.0,
                        2,
                        0.0,
                        0.0,
                    );
                    draw(
                        "key",
                        [left - 46.0, y, left + 2.0, y + 48.0],
                        [
                            (key & 7) as f32 / 8.0,
                            (key >> 3) as f32 / 2.0,
                            (key & 7) as f32 / 8.0 + 0.125,
                            (key >> 3) as f32 / 2.0 + 0.5,
                        ],
                        1.0,
                        2,
                        0.0,
                        0.0,
                    );
                    right = left - 62.0;
                }
                let offset = (((py - 0.5) as usize) * 1920 + x) * 4;
                for c in 0..3 {
                    let error = (pixels[offset + c] as f32 - color[c] * 255.0).abs();
                    max_error = max_error.max(error);
                    assert!(
                        error <= 2.0,
                        "Config shell case{case} pixel{x},{y} channel{c}: GPU{} scalar{},error{error}",
                        pixels[offset + c],
                        color[c] * 255.0
                    );
                }
                checked += 1;
            }
        }
    }
    eprintln!(
        "Config shell: {checked} independent native/scalar samples,8 cases,max RGB error{max_error}"
    );
}
