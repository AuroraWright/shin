//! Offscreen host-GPU checks against the reviewed program5 equation.
//! Native Maxwell reciprocal/FTZ and full message-frame parity are excluded.
use super::*;
use shin_render::{
    ColorBlendType,
    dynamic_buffer::DynamicBuffer,
    gpu_texture::GpuTexture,
    pipelines::PipelineStorage,
    shaders::types::{
        buffer::BytesAddress,
        texture::{TextureSamplerStore, TextureSource, TextureTarget, TextureTargetKind},
    },
};

pub(crate) struct Offscreen {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    signed_target: wgpu::Texture,
    signed_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    pipelines: PipelineStorage,
    signed_pipelines: PipelineStorage,
    dynamic: DynamicBuffer,
    samplers: TextureSamplerStore,
    _audio: shin_audio::AudioManager,
}

impl Offscreen {
    pub(crate) fn new() -> Self {
        let audio = shin_audio::AudioManager::new_for_tests();
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        println!(
            "GEROKASU2_FONT_GPU adapter={:?} audio=default output",
            adapter.get_info()
        );
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .unwrap();
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FNT6 offscreen shader check"),
            size: wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let signed_target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FNT6 signed sample probe"),
            size: wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let signed_view = signed_target.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FNT6 readback"),
            size: 8 * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            pipelines: PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm),
            signed_pipelines: PipelineStorage::new(
                device.clone(),
                wgpu::TextureFormat::Rgba16Float,
            ),
            dynamic: DynamicBuffer::new(device.clone(), BytesAddress::new(65536)),
            samplers: TextureSamplerStore::new(&device),
            device,
            queue,
            target,
            view,
            signed_target,
            signed_view,
            readback,
            _audio: audio,
        }
    }

    pub(crate) fn draw(&mut self, render: impl FnOnce(&mut RenderPass)) -> Vec<[u8; 4]> {
        self.draw_raw(false, render)
            .chunks_exact(4)
            .map(|pixel| pixel.try_into().unwrap())
            .collect()
    }

    pub(crate) fn samples(
        &mut self,
        source: TextureSource,
        vertices: &[TextVertex; 4],
        transform: Mat4,
    ) -> Vec<f32> {
        self.draw_raw(true, |pass| {
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Opaque)
                    .build(
                        RenderProgramWithArguments::Font {
                            vertices: VertexSource::VertexData { vertices },
                            glyph: source,
                            transform,
                            color1: FloatColor4::WHITE,
                            color2: FloatColor4::WHITE,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            )
        })
        .chunks_exact(8)
        .map(|pixel| half_to_float(u16::from_le_bytes(pixel[6..8].try_into().unwrap())))
        .collect()
    }

    fn draw_raw(&mut self, signed: bool, render: impl FnOnce(&mut RenderPass)) -> Vec<u8> {
        let (pipelines, target, view, row_bytes) = if signed {
            (
                &mut self.signed_pipelines,
                &self.signed_target,
                &self.signed_view,
                64,
            )
        } else {
            (&mut self.pipelines, &self.target, &self.view, 32)
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = RenderPass::new(
                pipelines,
                &mut self.dynamic,
                &self.samplers,
                &self.device,
                &mut encoder,
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view,
                },
                None,
                None,
                "FNT6 offscreen check",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                None,
                None,
            );
            render(&mut pass);
        }
        let mut uploads = self.device.create_command_encoder(&Default::default());
        self.dynamic.finish(&mut uploads);
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(8),
                },
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([uploads.finish(), encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = self.readback.slice(..).get_mapped_range().unwrap();
        let pixels = mapped
            .chunks_exact(256)
            .flat_map(|row| row[..row_bytes].iter().copied())
            .collect();
        drop(mapped);
        self.readback.unmap();
        self.dynamic.recall();
        pixels
    }
}

fn half_to_float(value: u16) -> f32 {
    let exponent = (value >> 10) & 31;
    let fraction = f32::from(value & 1023) / 1024.0;
    let magnitude = if exponent == 0 {
        fraction * 2.0f32.powi(-14)
    } else {
        (1.0 + fraction) * 2.0f32.powi(i32::from(exponent) - 15)
    };
    if value & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

pub(crate) fn expected(
    distance: f32,
    thresholds: Vec2,
    first: [f32; 4],
    second: [f32; 4],
    gradient: f32,
) -> [f32; 4] {
    let t = ((distance - thresholds.x) * (1.0 / (thresholds.y - thresholds.x))).clamp(0.0, 1.0);
    let coverage = (t * t) * (-2.0f32).mul_add(t, 3.0);
    if coverage < 0.001 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let mut rgba = std::array::from_fn(|c| (second[c] - first[c]).mul_add(gradient, first[c]));
    rgba[3] *= coverage;
    rgba
}

pub(crate) fn compare(actual: [u8; 4], expected: [f32; 4], label: &str) {
    for channel in 0..4 {
        let expected = expected[channel].clamp(0.0, 1.0) * 255.0;
        assert!(
            (f32::from(actual[channel]) - expected).abs() <= 1.2,
            "{label} channel{channel}: {} vs {expected}",
            actual[channel]
        );
    }
}

fn tint(v: [f32; 4]) -> FloatColor4 {
    FloatColor4::from_rgba(v[0], v[1], v[2], v[3])
}

fn quad(uv: Vec2, gradient: f32) -> [TextVertex; 4] {
    [
        vec2(-1.0, 1.0),
        vec2(1.0, 1.0),
        vec2(-1.0, -1.0),
        vec2(1.0, -1.0),
    ]
    .map(|p| TextVertex {
        position: vec4(p.x, p.y, uv.x, uv.y),
        color: gradient,
    })
}

fn shader(
    pass: &mut RenderPass,
    source: TextureSource,
    vertices: &[TextVertex; 4],
    thresholds: Vec2,
    first: [f32; 4],
    second: [f32; 4],
) {
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::FontSdf {
                    vertices: VertexSource::VertexData { vertices },
                    glyph: source,
                    transform: Mat4::IDENTITY,
                    color1: tint(first),
                    color2: tint(second),
                    thresholds,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

fn snorm(byte: u8) -> f32 {
    (f32::from(byte as i8) / 127.0).max(-1.0)
}

#[test]
#[ignore = "requires GPU and the default audio output"]
fn signed_samples_smoothstep_discard_gradient_and_legacy_coverage_render_offscreen() {
    let mut gpu = Offscreen::new();
    let colors = [
        ([0.13, 0.71, 0.37, 0.23], [0.83, 0.19, 0.59, 0.91]),
        ([0.91, 0.29, 0.61, 0.87], [0.07, 0.79, 0.17, 0.31]),
        ([0.27, 0.63, 0.41, 0.0001], [0.27, 0.63, 0.41, 0.0001]),
    ];
    let mut cases = 0;
    for byte in [
        -128i8, -127, -64, -18, -10, -9, -2, -1, 0, 1, 9, 10, 18, 64, 127,
    ] {
        let source = GpuTexture::new_static_with_data(
            &gpu.device,
            &gpu.queue,
            Some("Signed font sample"),
            winit::dpi::PhysicalSize::new(1, 1),
            wgpu::TextureFormat::R8Snorm,
            &[byte as u8],
        );
        for thresholds in [vec2(-1.0, 1.0), vec2(-0.075, 0.075), vec2(-0.14, -0.02)] {
            for gradient in [-0.25, 0.0, 0.5, 1.0, 1.25] {
                for (first, second) in colors {
                    let vertices = quad(vec2(0.5, 0.5), gradient);
                    let pixels = gpu.draw(|pass| {
                        shader(
                            pass,
                            source.as_source(),
                            &vertices,
                            thresholds,
                            first,
                            second,
                        )
                    });
                    let expected = expected(snorm(byte as u8), thresholds, first, second, gradient);
                    for actual in pixels {
                        compare(
                            actual,
                            expected,
                            &format!("signed{byte} thresholds{thresholds:?} gradient{gradient}"),
                        );
                    }
                    cases += 1;
                }
            }
        }
    }
    // Linear interpolation occurs after signed normalization. This also probes
    // just below/above the discard cutoff, with tint alpha below the cutoff.
    let source = GpuTexture::new_static_with_data(
        &gpu.device,
        &gpu.queue,
        Some("Signed font interpolation"),
        winit::dpi::PhysicalSize::new(2, 1),
        wgpu::TextureFormat::R8Snorm,
        &[129, 127],
    );
    let first = colors[2].0;
    let mut max_filter_error = 0.0f32;
    for t in [0.0, 0.01, 0.018, 0.019, 0.1, 0.25, 0.5, 0.75, 1.0] {
        let vertices = quad(vec2(0.25 + t * 0.5, 0.5), 0.0);
        let thresholds = vec2(-1.0, 1.0);
        let samples = gpu.samples(source.as_source(), &vertices, Mat4::IDENTITY);
        let pixels = gpu.draw(|pass| {
            shader(
                pass,
                source.as_source(),
                &vertices,
                thresholds,
                first,
                first,
            )
        });
        for (actual, distance) in pixels.into_iter().zip(samples) {
            let filter_error = (distance - (-1.0 + 2.0 * t)).abs();
            max_filter_error = max_filter_error.max(filter_error);
            assert!(
                filter_error <= 2.0 / 127.0,
                "Signed interpolation error{filter_error}"
            );
            compare(
                actual,
                expected(distance, thresholds, first, first, 0.0),
                &format!("signed bilinear{t}"),
            );
        }
    }
    let legacy = GpuTexture::new_static_with_data(
        &gpu.device,
        &gpu.queue,
        Some("Legacy unsigned glyph"),
        winit::dpi::PhysicalSize::new(1, 1),
        wgpu::TextureFormat::R8Unorm,
        &[173],
    );
    let vertices = quad(vec2(0.5, 0.5), 0.5);
    let (first, second) = colors[0];
    let pixels = gpu.draw(|pass| {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::Font {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        glyph: legacy.as_source(),
                        transform: Mat4::IDENTITY,
                        color1: tint(first),
                        color2: tint(second),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        )
    });
    let mut legacy_expected: [f32; 4] =
        std::array::from_fn(|c| (second[c] - first[c]).mul_add(0.5, first[c]));
    legacy_expected[3] *= 173.0 / 255.0;
    for actual in pixels {
        compare(actual, legacy_expected, "legacy unsigned coverage");
    }
    println!(
        "GEROKASU2_FONT_FILTER max_bilinear_error={max_filter_error} sample_bound={}",
        2.0 / 127.0
    );
    println!(
        "GEROKASU2_FONT_SDF_GPU constant_cases={cases} bilinear_cases=9 legacy_cases=1 pixels_per_case=64 channels_per_pixel=4 audio=default output"
    );
}

fn linear_sample(dimensions: (u32, u32), pixels: &[u8], uv: Vec2) -> f32 {
    let (width, height) = dimensions;
    let position = uv * vec2(width as f32, height as f32) - Vec2::splat(0.5);
    let base = position.floor();
    let fraction = position - base;
    let fetch = |x: i32, y: i32| {
        let x = x.clamp(0, width as i32 - 1) as usize;
        let y = y.clamp(0, height as i32 - 1) as usize;
        snorm(pixels[y * width as usize + x])
    };
    let x = base.x as i32;
    let y = base.y as i32;
    let top = fetch(x, y) * (1.0 - fraction.x) + fetch(x + 1, y) * fraction.x;
    let bottom = fetch(x, y + 1) * (1.0 - fraction.x) + fetch(x + 1, y + 1) * fraction.x;
    top * (1.0 - fraction.y) + bottom * fraction.y
}

#[test]
#[ignore = "requires supplied ROM, GPU and the default audio output"]
fn supplied_async_glyphs_upload_padding_and_draw_both_sdf_passes_offscreen() {
    use super::super::GpuGerokasu2Font;
    use shin_core::{
        format::{font::gerokasu2::Gerokasu2Font, rom::RomReader},
        primitives::stateless_reader::StatelessFile,
    };
    use std::{io::Read, sync::Arc};
    let mut gpu = Offscreen::new();
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut cases = 0;
    let mut max_filter_error = 0.0f32;
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font =
            Arc::new(GpuGerokasu2Font::new(Gerokasu2Font::new(bytes.into()).unwrap()).unwrap());
        let characters = ['A', 'A', 'あ', '※', '\0', '\u{10ffff}'];
        let handles = font
            .clone()
            .load_glyphs(gpu.device.clone(), gpu.queue.clone(), &characters);
        for (handle, cp) in handles.iter().zip(characters) {
            assert_eq!(handle.codepoint, cp);
            let geometry = handle.geometry().unwrap();
            let prepared = font
                .metrics()
                .lookup(u32::from(cp))
                .decode()
                .unwrap()
                .prepare_texture(u32::from(cp), font.metrics().header())
                .unwrap();
            assert_eq!(geometry.identity, prepared.geometry.identity);
            assert_eq!(geometry.dimensions, prepared.geometry.dimensions);
            let draw = GlyphDraw::new(
                geometry,
                -vec2(geometry.origin[0] as f32, geometry.origin[1] as f32),
                Vec2::ONE,
                50.0,
                true,
            )
            .unwrap();
            let transform = shin_render::shin_orthographic_projection_matrix(
                0.0,
                geometry.dimensions[0] as f32,
                geometry.dimensions[1] as f32,
                0.0,
                -1.0,
                1.0,
            );
            let color = [0.27, 0.63, 0.41, 0.57];
            let samples = gpu.samples(
                handle.as_texture_source().unwrap(),
                &draw.vertices,
                transform,
            );
            for outline in [true, false] {
                let pixels = gpu.draw(|pass| {
                    let builder =
                        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
                    if outline {
                        draw.render_outline(handle, pass, builder, transform, tint(color))
                            .unwrap();
                    } else {
                        draw.render_fill(
                            handle,
                            pass,
                            builder,
                            transform,
                            tint(color),
                            tint(color),
                        )
                        .unwrap();
                    }
                });
                for (index, actual) in pixels.into_iter().enumerate() {
                    let uv = vec2((index % 8) as f32 + 0.5, (index / 8) as f32 + 0.5) / 8.0;
                    let thresholds = if outline {
                        draw.outline_thresholds
                    } else {
                        draw.fill_thresholds
                    };
                    let image = &prepared.mipmaps[0];
                    let ideal = linear_sample(image.dimensions(), image.as_raw(), uv);
                    let distance = samples[index];
                    let error = (distance - ideal).abs();
                    max_filter_error = max_filter_error.max(error);
                    assert!(
                        error <= 2.0 / 127.0,
                        "{path} glyph{cp:?} pixel{index} signed sampling error{error}"
                    );
                    compare(
                        actual,
                        expected(distance, thresholds, color, color, 0.0),
                        &format!("{path} glyph{cp:?} outline{outline} pixel{index}"),
                    );
                }
                cases += 1;
            }
        }
        // Warm requests exercise cache ownership without a second upload.
        let warm = font
            .clone()
            .load_glyphs(gpu.device.clone(), gpu.queue.clone(), &characters);
        for (old, new) in handles.iter().zip(&warm) {
            assert!(std::ptr::eq(
                old.geometry().unwrap(),
                new.geometry().unwrap()
            ));
        }
    }
    println!(
        "GEROKASU2_FONT_FILTER max_supplied_error={max_filter_error} sample_bound={}",
        2.0 / 127.0
    );
    println!(
        "GEROKASU2_FONT_UPLOAD_GPU fonts=2 requested_glyphs=12 draw_cases={cases} warm_glyphs=12 pixels_per_case=64 channels_per_pixel=4 audio=default output"
    );
}
