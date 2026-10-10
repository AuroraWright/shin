//! Native curtain pixel oracle and actual sequential resource worker.
use super::*;
use crate::render::PreRenderContext;
#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_start_opening_worker_and_native_curtain_gpu() {
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
    let file = rom.find_file("/curtain.msk").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let mask = shin_core::format::mask::read_mask(&bytes).unwrap().texels;
    let capture = shin_render::gpu_texture::GpuTexture::new_static_with_data(
        &device,
        &queue,
        Some("retained screen alpha reference"),
        winit::dpi::PhysicalSize::new(1, 1),
        format,
        &[128, 64, 32, 128],
    );
    let mut opening = ScenarioSelectOpening::new(server.clone());
    for elapsed in [-1.0f32, 0.0, 15.0, 30.0, 45.0, 60.0] {
        if elapsed == 0.0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while opening.mask.is_none() {
                opening.update_frame(Ticks::ZERO).unwrap();
                assert!(
                    std::time::Instant::now() < deadline,
                    "Start resource worker timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(opening.atlas().unwrap().contains("details_48"));
            assert_eq!(opening.clock.take_events(), [OpeningEvent::StopMusic]);
        } else if elapsed > 0.0 {
            opening.update_frame(Ticks::from_f32(15.0)).unwrap();
        }
        assert!(!opening.clock.accepts_input());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Native screen curtain reference",
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(shin_core::primitives::color::UnormColor::from_rgba(
                    24, 80, 200, 255,
                ))
                .fill(
                    &mut pass,
                    RenderRequestBuilder::new(),
                    top_left_projection_matrix(),
                );
            opening.render_curtain(capture.as_source(), &mut pass);
        }
        let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut worst = 0.0f32;
        let mut checks = 0;
        for y in (0..1080).step_by(3) {
            for x in (0..1920).step_by(3) {
                let mask_x = (x as f32 + 0.5) / 1920.0 * mask.width() as f32 - 0.5;
                let mask_y = (y as f32 + 0.5) / 1080.0 * mask.height() as f32 - 0.5;
                let ix = mask_x.floor();
                let iy = mask_y.floor();
                let fx = mask_x - ix;
                let fy = mask_y - iy;
                let texel = |dx: f32, dy: f32| {
                    let mx = (ix + dx).clamp(0.0, mask.width() as f32 - 1.0) as u32;
                    let my = (iy + dy).clamp(0.0, mask.height() as f32 - 1.0) as u32;
                    mask.as_raw()[(my * mask.width() + mx) as usize] as f32 / 255.0
                };
                let sampled = (texel(0.0, 0.0) * (1.0 - fx) + texel(1.0, 0.0) * fx) * (1.0 - fy)
                    + (texel(0.0, 1.0) * (1.0 - fx) + texel(1.0, 1.0) * fx) * fy;
                // Maxwell program32 clamps min+(max-min)*mask. Compare its
                // premultiplied overlay independently, including source alpha.
                let factor = if elapsed < 0.0 {
                    1.0
                } else {
                    (1.0 - elapsed / 60.0 * 3.0 + sampled * 2.0).clamp(0.0, 1.0)
                };
                for c in 0..3 {
                    let src = [128.0f32, 64.0, 32.0][c];
                    let bg = [24.0f32, 80.0, 200.0][c];
                    let expected = src * factor + bg * (1.0 - 128.0 / 255.0 * factor);
                    let actual = pixels[((y * 1920 + x) * 4 + c) as usize] as f32;
                    let error = (actual - expected).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 2.0,
                        "Curtain elapsed{elapsed} pixel{x},{y} c{c}: GPU{actual} CPU{expected} error{error}"
                    );
                }
                checks += 1;
            }
        }
        std::fs::write(
            format!("/private/tmp/shin-start-curtain-{elapsed}.rgba"),
            pixels,
        )
        .unwrap();
        println!("Native curtain elapsed{elapsed}: {checks} sampled pixels, maxRGBerror{worst}");
    }
    opening.update_frame(Ticks::ZERO).unwrap();
    assert!(opening.clock.accepts_input());
    assert_eq!(opening.clock.take_events(), [OpeningEvent::StartMusic]);
    opening.update_frame(Ticks::ZERO).unwrap();
    assert!(opening.clock.take_events().is_empty());
}
