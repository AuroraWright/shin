//! Native four-level episode silhouette blur and final composite oracle.
use crate::render::PreRenderContext;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_episode_blur_pyramid_and_cover_gpu() {
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

    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
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
    let _ = server;
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

    let bg = &original.textures[original.name_to_index["bg"]];
    let source = shin_render::gpu_texture::GpuTexture::new_static_with_data(
        &device,
        &queue,
        Some("Episode captured artwork"),
        winit::dpi::PhysicalSize::new(bg.width(), bg.height()),
        format,
        bg.as_raw(),
    );
    let backdrop = super::EpisodeBackdrop::new(&context, (1920, 1080));
    for elapsed in [0.0f32, 120.0, 180.0, 210.0, 240.0, 300.0, 360.0] {
        let mut motion = super::super::episode_transition::EpisodeMotion::new(0);
        motion.update(shin_core::time::Ticks::from_f32(elapsed));
        let frame = motion.frame();
        backdrop.pre_render(source.as_source(), frame, &mut context);
        let progress = ((elapsed as f64 - 60.0) / 180.0).clamp(0.0, 1.0);
        let angle =
            (1.0 - (progress * std::f64::consts::FRAC_PI_2).cos()) * std::f64::consts::FRAC_PI_2;
        let offset = 500.0
            * (1.0
                - (((elapsed as f64 - 90.0) / 150.0).clamp(0.0, 1.0)
                    * std::f64::consts::FRAC_PI_2)
                    .cos());
        let camera_z = 1000.0 - offset;
        let focal = 540.0 / ((f32::from_bits(0x3f7d821c) as f64) * 0.5).tan();
        let (sin, cos) = angle.sin_cos();
        let ray = |px: f64, py: f64| {
            let dx = (px - 960.0) / focal;
            let dy = (py - 540.0) / focal;
            for right in [false, true] {
                let sign = if right { -1.0 } else { 1.0 };
                let q = (960.0 + sign * dx * camera_z) / (cos - sign * dx * sin);
                let t = camera_z + sin * q;
                let y = dy * t;
                if t > 0.0 && (0.0..=960.0).contains(&q) && y.abs() <= 540.0 {
                    return Some((
                        if right { 1.0 - q / 1920.0 } else { q / 1920.0 },
                        (y + 540.0) / 1080.0,
                    ));
                }
            }
            None
        };
        let mut masks = Vec::new();
        for (level, (width, height)) in [(480, 270), (240, 135), (120, 67), (60, 33)]
            .into_iter()
            .enumerate()
        {
            let data: Vec<u8> = (0..height)
                .flat_map(|y| {
                    (0..width).flat_map(move |x| {
                        let v = if ray(
                            (x as f64 + 0.5) * 1920.0 / width as f64,
                            (y as f64 + 0.5) * 1080.0 / height as f64,
                        )
                        .is_some()
                        {
                            0
                        } else {
                            255
                        };
                        [v, v, v, 255]
                    })
                })
                .collect();
            let mut current = data;
            for horizontal in [true, false] {
                let mut next = vec![0u8; current.len()];
                for y in 0..height {
                    for x in 0..width {
                        let u = (x as f32 + 0.5) / width as f32;
                        let v = (y as f32 + 0.5) / height as f32;
                        let mut value = 0.0;
                        for (distance, weight) in [
                            (0.75, 0x3eceed6bu32),
                            (2.25, 0x3ea127be),
                            (3.75, 0x3e437dbe),
                            (5.25, 0x3db8afdf),
                        ] {
                            for sign in [-1.0, 1.0] {
                                let (du, dv) = if horizontal {
                                    (sign * distance / width as f32, 0.0)
                                } else {
                                    (0.0, sign * distance / height as f32)
                                };
                                value += sample(&current, width, height, u + du, v + dv)[0]
                                    * f32::from_bits(weight)
                                    * 0.5;
                            }
                        }
                        let c = (value * 255.0).round().clamp(0.0, 255.0) as u8;
                        let i = ((y * width + x) * 4) as usize;
                        next[i..i + 4].copy_from_slice(&[c, c, c, 255]);
                    }
                }
                current = next;
            }
            let actual = crate::layer::user::readback_test_texture(
                &mut context,
                &backdrop.levels[level].0.texture,
            );
            let worst = actual
                .chunks_exact(4)
                .zip(current.chunks_exact(4))
                .map(|(a, b)| a[0].abs_diff(b[0]))
                .max()
                .unwrap();
            assert!(
                worst <= 3,
                "Episode{elapsed} pyramid{level} maxerror{worst}"
            );
            masks.push((current, width, height));
        }
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Episode final native composite",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            backdrop.render(source.as_source(), frame, &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let cover = (1.0 - (progress * std::f64::consts::FRAC_PI_2).cos()) as f32;
        let luminance = (1.0 - (elapsed - 240.0) / 120.0).clamp(0.0, 1.0);
        let gray = (luminance * 255.0) as u8;
        let alpha = (cover * 255.0) as u8;
        let mut worst = 0u8;
        let mut checked = 0;
        for y in (0..1080usize).step_by(3) {
            for x in (0..1920usize).step_by(3) {
                // Rasterization at a projected edge is ambiguous within a
                // fraction of a pixel; compare interiors, as in the door oracle.
                let dx = (x as f64 + 0.5 - 960.0) / focal;
                let dy = (y as f64 + 0.5 - 540.0) / focal;
                let boundary = [1.0, -1.0].into_iter().any(|sign| {
                    let q = (960.0 + sign * dx * camera_z) / (cos - sign * dx * sin);
                    let t = camera_z + sin * q;
                    let py = dy * t;
                    t > 0.0
                        && q >= -0.1
                        && q <= 960.1
                        && py.abs() <= 540.1
                        && (q.abs() < 0.1
                            || (q - 960.0).abs() < 0.1
                            || (py.abs() - 540.0).abs() < 0.1)
                });
                if boundary {
                    continue;
                }
                let uv = ray(x as f64 + 0.5, y as f64 + 0.5);
                let base = uv
                    .map(|(u, v)| sample(bg.as_raw(), bg.width(), bg.height(), u as f32, v as f32))
                    .unwrap_or([1.0; 4]);
                let mut expected = base.map(|v| (v * 255.0).round());
                for ((mask, w, h), tint) in masks.iter().zip([17.0f32, 34.0, 68.0, 136.0]) {
                    let value = sample(
                        mask,
                        *w,
                        *h,
                        (x as f32 + 0.5) / 1920.0,
                        (y as f32 + 0.5) / 1080.0,
                    )[0] * tint;
                    for c in 0..3 {
                        expected[c] = (expected[c] + value).round().min(255.0);
                    }
                }
                for c in 0..3 {
                    let reference = if cover >= 1.0 {
                        gray
                    } else {
                        (expected[c] + gray as f32 * alpha as f32 / 255.0)
                            .round()
                            .min(255.0) as u8
                    };
                    let error = pixels[(y * 1920 + x) * 4 + c].abs_diff(reference);
                    worst = worst.max(error);
                    assert!(
                        error <= 4,
                        "Episode{elapsed} pixel{x},{y},channel{c} GPU{} reference{reference} error{error}",
                        pixels[(y * 1920 + x) * 4 + c]
                    );
                }
                checked += 1;
            }
        }
        eprintln!(
            "Episode blur/cover tick{elapsed}: four pyramid masks and{checked} RGB pixels,maxerror{worst}"
        );
        if elapsed == 180.0 {
            std::fs::write(
                std::env::temp_dir().join("shin-episode-composite-180.rgba"),
                &pixels,
            )
            .unwrap();
        }
    }
    // Transfer a completed capture into the actual owner and retire its
    // artwork source. The owner must render from its retained texture.
    let mut prior = context.new_render_texture("Episode retained owner frame".into());
    {
        let mut pass = context.begin_pass(
            prior.as_texture_target(),
            None,
            "Episode prior frame capture",
        );
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(shin_core::primitives::color::UnormColor::WHITE)
            .render_sprite(
                &mut pass,
                shin_render::RenderRequestBuilder::new()
                    .color_blend_type(shin_render::ColorBlendType::Opaque),
                source.as_source(),
                crate::render::top_left_projection_matrix(),
            );
    }
    drop(source);
    let mut scene =
        super::super::episode_transition_scene::EpisodeScene::new(47, prior, &context, (960, 540));
    let render_scene = |scene: &super::super::episode_transition_scene::EpisodeScene,
                        context: &mut PreRenderContext| {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(context.depth_stencil),
            "Episode actual owner",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        scene.render(&mut pass);
    };
    scene.update(shin_core::time::Ticks::ZERO, &mut context);
    render_scene(&scene, &mut context);
    let retained = crate::layer::user::readback_test_texture(&mut context, &target);
    for y in (0..1080usize).step_by(3) {
        for x in (0..1920usize).step_by(3) {
            let expected = sample(
                bg.as_raw(),
                bg.width(),
                bg.height(),
                (x as f32 + 0.5) / 1920.0,
                (y as f32 + 0.5) / 1080.0,
            );
            for c in 0..3 {
                assert!(
                    retained[(y * 1920 + x) * 4 + c].abs_diff((expected[c] * 255.0).round() as u8)
                        <= 1
                );
            }
        }
    }
    assert!(scene.take_destination().is_none());
    scene.update(shin_core::time::Ticks::from_f32(359.5), &mut context);
    assert!(scene.take_destination().is_none());
    render_scene(&scene, &mut context);
    let near_end = crate::layer::user::readback_test_texture(&mut context, &target);
    assert!(
        near_end
            .chunks_exact(4)
            .all(|pixel| pixel[..3] == [1, 1, 1])
    );
    scene.update(shin_core::time::Ticks::from_f32(0.5), &mut context);
    let entry = scene
        .take_destination()
        .expect("Native same-frame360tick fresh entry");
    assert_eq!((entry.scenario_id, entry.save_position), (47, 0));
    assert!(entry.route_data.reader().read_choice().is_err());
    render_scene(&scene, &mut context);
    let terminal = crate::layer::user::readback_test_texture(&mut context, &target);
    assert!(
        terminal
            .chunks_exact(4)
            .all(|pixel| pixel[..3] == [0, 0, 0])
    );
    scene.update(shin_core::time::Ticks::ZERO, &mut context);
    assert!(scene.take_destination().is_none());
    eprintln!(
        "Actual episode owner: retired artwork, retained capture, physical960x540 pyramid, exact360tick black frame and single seeded Episode47 entry"
    );
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_episode_silhouette_blur_spacing_additive_weights_and_cover() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (address, expected) in [
        (0xe3f7c, 0x1e2703e8),
        (0xe3f84, 0x4ea81d00),
        (0xe3f88, 0x4ea81d01),
        (0xe3f8c, 0x4ea81d02),
        (0xe3f90, 0x4ea91d23),
        (0xe3fa8, 0x94000142),
        (0xe3fc4, 0x4ea81d00),
        (0xe3fec, 0x97ffad95),
        (0xe4028, 0x94000122),
        (0xe4068, 0x94000112),
        (0xe4074, 0x1e2d100b),
        (0xe4078, 0x1e20500c),
        (0xe407c, 0x1e21d00d),
        (0xe4080, 0x1e22b00e),
        (0xe409c, 0x52800576),
        (0xe411c, 0x1e201960),
        (0xe4134, 0x1e201980),
        (0xe414c, 0x1e2019a0),
        (0xe4164, 0x1e2019c0),
        (0xe4180, 0x1e201960),
        (0xe4198, 0x1e201980),
        (0xe41b0, 0x1e2019a0),
        (0xe41c8, 0x1e2019c0),
        (0xe420c, 0x97ffad8d),
        (0xe4210, 0x91006294),
        (0xe422c, 0x2a1603e0),
        (0xe4234, 0x320003e0),
        (0xe4298, 0x97ffad6a),
        (0xe42b4, 0x2a1603e0),
        (0xe42bc, 0x320003e0),
        (0xe4304, 0xf100131f),
        (0xe4550, 0x4ea91d20),
        (0xe4554, 0x4ea91d21),
        (0xe4558, 0x4ea91d22),
        (0xe455c, 0x4ea91d23),
        (0xe4584, 0x97ff89f3),
        (0xe49ec, 0x97fffeb1),
        (0xe4a54, 0x320007e0),
        (0xe4ab0, 0xbd47d100),
        (0xe4b30, 0xbd471500),
        (0xe4bb0, 0xbd48e900),
        (0xe4c30, 0xbd496d00),
        (0xe4c60, 0x1e282120),
        (0xe4c64, 0x1a9fb680),
        (0xe4c68, 0x97ffaac6),
        (0xe4c84, 0x4eaa1d40),
        (0xe4c88, 0x4ea91d21),
        (0xe4ca8, 0x97ff8822),
        (0xe4cb0, 0x97ff8828),
        (0x56740, 0x4ea11c23),
        (0x56744, 0x4ea01c01),
        (0x56748, 0x4ea01c02),
        (0x1597d0, 0x3d888889),
        (0x159714, 0x3e088889),
        (0x1598e8, 0x3e888889),
        (0x15996c, 0x3f088889),
    ] {
        assert_eq!(
            word(address),
            expected,
            "native episode composite {address:x}"
        );
    }
}
