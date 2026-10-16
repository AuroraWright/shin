//! Independent ray/plane oracle for native captured-screen episode doors.
use crate::render::PreRenderContext;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_episode_perspective_doors_gpu_against_ray_plane_oracle() {
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
        Some("Episode captured prior-frame artwork"),
        winit::dpi::PhysicalSize::new(bg.width(), bg.height()),
        format,
        bg.as_raw(),
    );
    for (case, elapsed, tint) in [
        ("closed", 0.0f32, 255u8),
        ("wait_boundary", 60.0, 255),
        ("begin_open", 120.0, 255),
        ("half_open", 180.0, 255),
        ("late_open", 210.0, 255),
        ("fully_open", 240.0, 255),
        ("black_mask", 180.0, 0),
        ("tinted", 180.0, 153),
    ] {
        let mut motion = crate::title::episode_transition::EpisodeMotion::new(0);
        motion.update(shin_core::time::Ticks::from_f32(elapsed));
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Episode native doors reference",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            super::render_doors(
                source.as_source(),
                motion.frame(),
                shin_core::primitives::color::UnormColor::from_rgba(tint, tint, tint, 255),
                &mut pass,
            );
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        // Derive motion independently, then intersect each camera ray with
        // the left/right hinged plane. No production matrices are used.
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
        let mut worst = 0u8;
        let mut checked = 0;
        for y in (0..1080usize).step_by(3) {
            for x in (0..1920usize).step_by(3) {
                let dx = (x as f64 + 0.5 - 960.0) / focal;
                let dy = (y as f64 + 0.5 - 540.0) / focal;
                let mut uv = None;
                let mut boundary = false;
                for right in [false, true] {
                    let sign = if right { -1.0 } else { 1.0 };
                    let q = (960.0 + sign * dx * camera_z) / (cos - sign * dx * sin);
                    let t = camera_z + sin * q;
                    let py = dy * t;
                    if t > 0.0 && q >= -0.1 && q <= 960.1 && py.abs() <= 540.1 {
                        if q.abs() < 0.1
                            || (q - 960.0).abs() < 0.1
                            || (py.abs() - 540.0).abs() < 0.1
                        {
                            boundary = true;
                        }
                        if (0.0..=960.0).contains(&q) && py.abs() <= 540.0 {
                            uv = Some((
                                if right { 1.0 - q / 1920.0 } else { q / 1920.0 },
                                (py + 540.0) / 1080.0,
                            ));
                        }
                    }
                }
                if boundary {
                    continue;
                }
                let expected = uv
                    .map(|(u, v)| sample(bg.as_raw(), bg.width(), bg.height(), u as f32, v as f32))
                    .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let i = (y * 1920 + x) * 4;
                for c in 0..3 {
                    let reference = (expected[c] * tint as f32).round().clamp(0.0, 255.0) as u8;
                    let error = pixels[i + c].abs_diff(reference);
                    worst = worst.max(error);
                    assert!(
                        error <= 4,
                        "Episode {case} pixel{x},{y},channel{c}: GPU{} reference{reference} error{error} uv{uv:?}",
                        pixels[i + c]
                    );
                }
                checked += 1;
            }
        }
        eprintln!("Episode perspective doors {case}: {checked} RGB pixels, maxerror{worst}");
        if case == "half_open" {
            std::fs::write(
                std::env::temp_dir().join("shin-episode-doors-half-open.rgba"),
                &pixels,
            )
            .unwrap();
        }
    }
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_episode_doors_projection_matrix_order_hinges_and_uv() {
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
        (0xe4594, 0xbd46850e),
        (0xe4598, 0x9102c280),
        (0xe45a0, 0x1e2e080c),
        (0xe45b4, 0x9400e2a3),
        (0xe45bc, 0xbd4a8900),
        (0xe45c4, 0xbd473d01),
        (0xe45cc, 0xbd467903),
        (0xe45d8, 0x97fef5c2),
        (0xe45ec, 0x97fef6a1),
        (0xe45fc, 0x97fef4e5),
        (0xe462c, 0xbd484102),
        (0xe4634, 0x97fef67f),
        (0xe4644, 0x97fef4d3),
        (0xe4678, 0x97fef66e),
        (0xe4688, 0x97fef4c2),
        (0xe46b8, 0x97ffac32),
        (0xe46f0, 0x97fef650),
        (0xe46fc, 0x97fef60d),
        (0xe470c, 0x97fef4a1),
        (0xe473c, 0xbd48890f),
        (0xe4748, 0x97fef63a),
        (0xe4758, 0x97fef48e),
        (0xe4788, 0x97fef482),
        (0xe47c0, 0xbd488d0c),
        (0xe47c8, 0xbd454d0d),
        (0xe47e0, 0x97ff8934),
        (0xe47f8, 0x97ff8942),
        (0xe4820, 0x97ff897c),
        (0xe483c, 0x1e2e8940),
        (0xe4860, 0x97fef5b4),
        (0xe4878, 0x4eab1d60),
        (0xe48a4, 0x97fef5e3),
        (0xe48b4, 0x97fef437),
        (0xe48e4, 0x97fef42b),
        (0xe492c, 0x97ff88e1),
        (0xe4944, 0x97ff88ef),
        (0xe496c, 0x97ff8929),
        (0xa1998, 0x4f809044),
        (0xa19d0, 0x4fa01a24),
        (0xa1d58, 0xb9002e68),
        (0xa1d74, 0xb9003e7f),
        (0xa1d78, 0x1e201820),
        (0xa1f8c, 0xbd000a61),
        (0xa1f90, 0xbd002269),
        (0xa2050, 0xbd003100),
        (0xa2058, 0xbd003501),
        (0xa205c, 0xbd003902),
        (0x159758, 0x44f00000),
        (0x159614, 0x44870000),
        (0x159684, 0x3fc90fdb),
        (0x159a88, 0x3f7d821c),
        (0x15973c, 0x3fe38e39),
        (0x159678, 0x461c4000),
        (0x159840, 0xc47a0000),
        (0x159624, 0x44700000),
        (0x159888, 0xc4700000),
        (0x15988c, 0xc4070000),
        (0x15954c, 0x44070000),
    ] {
        assert_eq!(
            word(address),
            expected,
            "native episode geometry {address:x}"
        );
    }
}
