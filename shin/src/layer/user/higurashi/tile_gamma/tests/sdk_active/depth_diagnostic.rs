//! Isolated native-packet diagnostics. Synthetic predicate/control draws are
//! explicit and do not replace the original candidate/reference comparison.
use super::*;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore},
    },
};

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    draws: &[gpu::Draw],
    attachment_clear: bool,
    copy_depth: bool,
    size: (u32, u32),
) -> (Vec<u8>, Option<Vec<u8>>) {
    let samplers = TextureSamplerStore::new(device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        size.0, size.1,
    )));
    let make = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Isolated TileGamma depth diagnostic"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let target = make(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = make(
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | if copy_depth {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            },
    );
    let view = target.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = crate::render::PreRenderContext {
        device,
        queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    initialize(&mut context, &view, 0);
    if attachment_clear {
        let _pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Diagnostic exact attachment0.5 clear"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.5),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                ..Default::default()
            });
    }
    let mut renderer = gpu::Renderer::with_depth_format(device, format);
    for draw in draws {
        renderer.draw(&mut context, &view, draw, None, None);
    }
    let depth_buffer = if copy_depth {
        assert_eq!(format, wgpu::TextureFormat::Depth32FloatStencil8);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Diagnostic float depth readback"),
            size: (size.0 * size.1 * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        context.encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &depth,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.0 * 4),
                    rows_per_image: Some(size.1),
                },
            },
            depth.size(),
        );
        Some(buffer)
    } else {
        None
    };
    let rgba = crate::layer::user::readback_test_texture(&mut context, &target);
    let values = depth_buffer.map(|buffer| {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        buffer.slice(..).get_mapped_range().unwrap().to_vec()
    });
    (rgba, values)
}
fn diagnostic_color(draw: &gpu::Draw) -> gpu::Draw {
    let mut d = draw.clone();
    d.blend = 1;
    for i in (3..16).step_by(4) {
        d.vertices[i] = bgra(0xff40_80ff);
    }
    d
}
fn mask(image: &[u8], baseline: &[u8]) -> Vec<bool> {
    image
        .chunks_exact(4)
        .zip(baseline.chunks_exact(4))
        .map(|(a, b)| a != b)
        .collect()
}
#[test]
#[ignore = "Isolated case968 Metal depth/stencil diagnostic, explicit float readback, no audio/window/save"]
fn higurashi_tile_gamma_zero_area_depth_diagnostic() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    assert!(
        adapter
            .features()
            .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
    );
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::DEPTH32FLOAT_STENCIL8,
        ..Default::default()
    }))
    .unwrap();
    let out = PathBuf::from(std::env::var("SHIN_HIGURASHI_DEPTH_DIAGNOSTIC").unwrap());
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("adapter.txt"),
        format!(
            "{:?}\nfeatures={:?}\n",
            device.adapter_info(),
            device.features()
        ),
    )
    .unwrap();
    let rows: serde_json::Value =
        serde_json::from_slice(&fs::read(directory().join("gpu-cases.json")).unwrap()).unwrap();
    let selected = std::env::var("SHIN_HIGURASHI_DEPTH_CASE")
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(968);
    let size = (
        std::env::var("SHIN_HIGURASHI_DEPTH_WIDTH")
            .map(|s| s.parse().unwrap())
            .unwrap_or(1280),
        std::env::var("SHIN_HIGURASHI_DEPTH_HEIGHT")
            .map(|s| s.parse().unwrap())
            .unwrap_or(720),
    );
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["index"] == selected)
        .unwrap();
    verify(row);
    let native: Vec<_> = row["draws"].as_array().unwrap().iter().map(draw).collect();
    let mut report = Vec::new();
    for (format, label) in [
        (shin_render::DEPTH_STENCIL_FORMAT, "default"),
        (wgpu::TextureFormat::Depth32FloatStencil8, "float32"),
    ] {
        for attachment in [false, true] {
            let prefix = format!(
                "{label}-{}",
                if attachment { "attachment" } else { "shader" }
            );
            let mut base = if attachment {
                vec![native[1].clone()]
            } else {
                vec![native[0].clone(), native[1].clone()]
            };
            let baseline = render(&device, &queue, format, &base, attachment, false, size).0;
            fs::write(out.join(format!("{prefix}-baseline.rgba")), &baseline).unwrap();
            let mut probe = diagnostic_color(&native[2]);
            probe.depth = [7, 0];
            probe.stencil = [2, 0, 0, 0, 0x7ffff];
            let mut no_tile = base.clone();
            no_tile.push(probe.clone());
            let image = render(&device, &queue, format, &no_tile, attachment, false, size).0;
            assert_eq!(image, baseline, "{prefix}: noTile contaminated stencil");
            let mut never = base.clone();
            let mut tile = native[2].clone();
            tile.depth = [0, 0];
            never.push(tile);
            never.push(probe.clone());
            let image = render(&device, &queue, format, &never, attachment, false, size).0;
            assert_eq!(image, baseline, "{prefix}: depthNever altered stencil");
            let mut predicate_masks = Vec::new();
            let mut counts = Vec::new();
            let mut first = None;
            for (function, name) in [(7, "always"), (1, "less"), (2, "equal"), (4, "greater")] {
                let mut d = diagnostic_color(&native[2]);
                d.depth = [function, 0];
                d.stencil = [7, 0, 0, 0, 0xffff];
                let mut plan = base.clone();
                plan.push(d);
                let (image, depth) = render(
                    &device,
                    &queue,
                    format,
                    &plan,
                    attachment,
                    label == "float32",
                    size,
                );
                let bitmap = mask(&image, &baseline);
                let count = bitmap.iter().filter(|v| **v).count();
                if first.is_none() {
                    first = bitmap.iter().position(|v| *v);
                }
                counts.push(count);
                predicate_masks.push(bitmap.clone());
                fs::write(out.join(format!("{prefix}-{name}.rgba")), image).unwrap();
                fs::write(
                    out.join(format!("{prefix}-{name}.mask")),
                    bitmap.iter().map(|v| u8::from(*v)).collect::<Vec<_>>(),
                )
                .unwrap();
                if let Some(depth) = depth {
                    fs::write(out.join(format!("{prefix}-{name}.depth")), depth).unwrap();
                }
            }
            assert!(counts[0] > 0, "positive Always control lost geometry");
            for i in 0..predicate_masks[0].len() {
                let n = predicate_masks[1..].iter().filter(|m| m[i]).count();
                assert_eq!(
                    n,
                    usize::from(predicate_masks[0][i]),
                    "{prefix}: predicatepartition atpixel{i}"
                );
            }
            let mut always = base.clone();
            let mut tile = native[2].clone();
            tile.depth = [7, 0];
            always.push(tile);
            always.push(probe.clone());
            let image = render(&device, &queue, format, &always, attachment, false, size).0;
            assert_eq!(
                mask(&image, &baseline),
                predicate_masks[0],
                "{prefix}: AlwaysTile stencil coverage"
            );
            if label == "float32" {
                let mut written = base.clone();
                let mut tile = diagnostic_color(&native[2]);
                tile.depth = [7, 1];
                tile.stencil = [7, 0, 0, 0, 0xffff];
                written.push(tile);
                let (_, depth) = render(&device, &queue, format, &written, attachment, true, size);
                fs::write(
                    out.join(format!("{prefix}-tile-written.depth")),
                    depth.unwrap(),
                )
                .unwrap();
            }
            base.push(native[2].clone());
            let natural = render(&device, &queue, format, &base, attachment, false, size).0;
            base.push(probe);
            let image = render(&device, &queue, format, &base, attachment, false, size).0;
            let footprint = mask(&image, &natural);
            let count = footprint.iter().filter(|v| **v).count();
            assert_eq!(
                footprint, predicate_masks[1],
                "{prefix}: originalLess Tile stencil footprint"
            );
            fs::write(out.join(format!("{prefix}-natural.rgba")), natural).unwrap();
            fs::write(out.join(format!("{prefix}-probe.rgba")), image).unwrap();
            let item = serde_json::json!({"case":selected,"source":row["source_index"],"width":size.0,"height":size.1,"variant":prefix,"predicate_pixel_counts":{"always":counts[0],"less":counts[1],"equal":counts[2],"greater":counts[3]},"original_probe_changed_pixels":count,"first_covered_pixel":first,"negative_controls":["noTile","Never"],"predicate_partition":true});
            println!("{item}");
            report.push(item);
        }
    }
    fs::write(
        out.join("diagnostic.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
