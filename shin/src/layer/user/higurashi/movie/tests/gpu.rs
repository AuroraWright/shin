//! Original packet/GLSL reference; no later Movie/block/effect expected renderer.
use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{borrow::Cow, collections::HashMap, path::PathBuf};
use wgpu::util::DeviceExt;

struct Shader {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    conversion: bool,
}
fn native_blend(value: u32) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as O};
    if value == 1 {
        return wgpu::BlendState {
            color: BlendComponent {
                src_factor: F::One,
                dst_factor: F::Zero,
                operation: O::Add,
            },
            alpha: BlendComponent {
                src_factor: F::One,
                dst_factor: F::Zero,
                operation: O::Add,
            },
        };
    }
    let (destination, operation) = match value {
        2 => (F::OneMinusSrcAlpha, O::Add),
        3 => (F::One, O::Add),
        4 => (F::One, O::ReverseSubtract),
        _ => panic!("Unknown original Movie blend{value}"),
    };
    wgpu::BlendState {
        color: BlendComponent {
            src_factor: F::SrcAlpha,
            dst_factor: destination,
            operation,
        },
        alpha: BlendComponent {
            src_factor: F::OneMinusDstAlpha,
            dst_factor: F::One,
            operation: O::Add,
        },
    }
}
impl Shader {
    fn new(device: &wgpu::Device, draw: &[u32], format: wgpu::TextureFormat) -> Self {
        let conversion = draw[3] == 20;
        let (directory, stem) = if conversion {
            (
                "SHIN_HIGURASHI_LIVE_MOVIE_SHADERS",
                "native-live-movie-20".to_owned(),
            )
        } else {
            let operation = [6, 9, 7, 8, 10, 11]
                .iter()
                .position(|&p| p == draw[3])
                .unwrap();
            (
                "SHIN_HIGURASHI_PRIMARY_BLOCK_SHADERS",
                format!("native-primary-block-{operation}"),
            )
        };
        let directory = PathBuf::from(std::env::var(directory).unwrap());
        let module = |stage| {
            let raw = std::fs::read(directory.join(format!("{stem}.{stage}.spv"))).unwrap();
            assert_eq!(raw.len() % 4, 0);
            assert_eq!(&raw[..4], &0x07230203u32.to_le_bytes());
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Literal original Movie GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(
                    raw.chunks_exact(4)
                        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                        .collect(),
                )),
            })
        };
        let mut entries: Vec<_> = (0..3)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let textures = if conversion { 2 } else { 1 };
        for index in 0..textures {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 3 + index,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3 + textures,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Original Movie raw descriptors"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        assert_eq!(&draw[9..12], &[1, 4, 64]); // Original triangle-strip / XYUV64.
        let stencil = &draw[16..21];
        let face = if conversion {
            // Program20 has no depth resource. The first captured packet
            // retains zeroed stencil state, which is inactive for this pass.
            assert_eq!(draw[6], 0);
            wgpu::StencilFaceState::IGNORE
        } else {
            wgpu::StencilFaceState {
                compare: match stencil[0] {
                    4 => wgpu::CompareFunction::Greater,
                    7 => wgpu::CompareFunction::Always,
                    _ => panic!("Unreviewed original Movie stencil"),
                },
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: match stencil[3] {
                    0 => wgpu::StencilOperation::Keep,
                    2 => wgpu::StencilOperation::Replace,
                    _ => panic!("Unreviewed Movie stencil operation"),
                },
            }
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original Movie conversion/final packet"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module("vert"),
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module("frag"),
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(native_blend(draw[4])),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: (!conversion).then_some(wgpu::DepthStencilState {
                format: shin_render::DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: stencil[4] & 255,
                    write_mask: (stencil[4] >> 8) & 255,
                },
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            conversion,
        }
    }
    fn draw(
        &self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        draw: &[u32],
        textures: &[TextureSource],
    ) {
        assert_eq!(textures.len(), if self.conversion { 2 } else { 1 });
        let uniforms: Vec<_> = (0..3)
            .map(|slot| {
                let count = draw[21 + slot] as usize / 4;
                assert!(matches!(count, 4 | 12 | 16));
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Original Movie exact uploaded uniforms"),
                        contents: bytemuck::cast_slice(
                            &draw[25 + 16 * slot..25 + 16 * slot + count],
                        ),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original Movie XYUV64 bytes"),
                contents: bytemuck::cast_slice(&draw[89..105]),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Original Movie NVN min5/mag1/wrap7"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let mut entries: Vec<_> = uniforms
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        entries.extend(
            textures
                .iter()
                .enumerate()
                .map(|(index, texture)| wgpu::BindGroupEntry {
                    binding: 3 + index as u32,
                    resource: wgpu::BindingResource::TextureView(texture.view),
                }),
        );
        entries.push(wgpu::BindGroupEntry {
            binding: 3 + textures.len() as u32,
            resource: wgpu::BindingResource::Sampler(&sampler),
        });
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Literal Movie native packet replay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: (!self.conversion).then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: context.depth_stencil.view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                    },
                ),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        if !self.conversion {
            pass.set_stencil_reference((draw[20] >> 16) & 255);
        }
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..draw[10], 0..1);
    }
}
struct Shaders(HashMap<(u32, u32, wgpu::TextureFormat), Shader>);
impl Shaders {
    fn draw(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        draw: &[u32],
        textures: &[TextureSource],
    ) {
        self.0
            .entry((draw[3], draw[4], format))
            .or_insert_with(|| Shader::new(context.device, draw, format))
            .draw(context, target, draw, textures);
    }
}
fn nv12((width, height): (u32, u32), seed: usize) -> shin_video::Nv12Frame {
    let mut y = vec![0; (width * height) as usize];
    for row in 0..height {
        for column in 0..width {
            y[(row * width + column) as usize] =
                (16 + (column * 13 + row * 7 + (seed as u32 * 31)) % 220) as u8;
        }
    }
    let mut uv = vec![0; (width * height / 2) as usize];
    for row in 0..height / 2 {
        for column in 0..width / 2 {
            let index = (row * width + column * 2) as usize;
            uv[index] = (32 + (column * 19 + row * 11 + seed as u32 * 17) % 192) as u8;
            uv[index + 1] = (32 + (column * 5 + row * 23 + seed as u32 * 29) % 192) as u8;
        }
    }
    shin_video::Nv12Frame {
        size: winit::dpi::PhysicalSize::new(width, height),
        y_plane: y,
        uv_plane: uv,
    }
}
fn compare(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let changed = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(
        changed, 0,
        "{label}:channels differing{changed},max difference{maximum}"
    );
}
struct FinalCase {
    conversion: Vec<u32>,
    final_draw: Vec<u32>,
    packet: Packet,
}
fn captured() -> (Vec<Vec<u32>>, Vec<FinalCase>) {
    let mut input = profiles();
    let mut conversions = Vec::new();
    let mut finals = Vec::new();
    for (index, line) in OUTPUT.lines().enumerate() {
        let mut native = Native::new(line);
        let mut frame = None;
        let mut conversion = None;
        let mut final_packet = None;
        while let Some((tag, words)) = native.event() {
            match tag {
                "P" => {
                    frame = (words[9] != 0).then(|| FrameInfo {
                        size: vec2(f32::from_bits(words[13]), f32::from_bits(words[14])),
                        origin: vec2(f32::from_bits(words[17]), f32::from_bits(words[18])),
                        extent: vec2(f32::from_bits(words[19]), f32::from_bits(words[20])),
                    })
                }
                "Q" => {
                    let mut properties = HigurashiProperties::from_seeds([0; 4]);
                    for (property, value) in input[index]
                        .renders
                        .pop_front()
                        .unwrap()
                        .into_iter()
                        .enumerate()
                    {
                        properties
                            .tweener_mut(property)
                            .unwrap()
                            .fast_forward_to(value as f32);
                    }
                    let kind = if words[0] == 1 {
                        PassKind::Opaque
                    } else {
                        PassKind::Transparent
                    };
                    final_packet = frame.and_then(|frame| {
                        Packet::new(frame, &properties, &HigurashiTransform::default(), kind)
                            .unwrap()
                    });
                }
                "D" if words[3] == 20 => {
                    conversion = Some(words.clone());
                    conversions.push(words);
                }
                "D" => finals.push(FinalCase {
                    conversion: conversion.clone().unwrap(),
                    final_draw: words,
                    packet: final_packet.take().unwrap(),
                }),
                _ => {}
            }
        }
    }
    (conversions, finals)
}

#[test]
#[ignore = "Original program20 NV12/ordinary6..11 GLSL SPIR-V and headless GPU; no audio/window/save"]
fn higurashi_movie_original_two_stage_rgba_pixels_at_720p_and_1080p() {
    let (device, queue, _) = reference_gpu_assets();
    let (conversions, finals) = captured();
    assert_eq!((conversions.len(), finals.len()), (82, 76));
    let mut shaders = Shaders(HashMap::new());
    for (index, original) in conversions.iter().enumerate() {
        let extent = (original[7], original[8]);
        let source = shin_video::VideoFrameTexture::new(&device, nv12(extent, index).size);
        source.write_data_nv12(&queue, &nv12(extent, index));
        let actual = reference_render_to_texture(&device, &queue, extent, |context, target| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Higurashi Movie full-frame conversion",
            );
            ConversionPacket::new(extent).render(
                &mut pass,
                source.get_y_source(),
                source.get_uv_source(),
            );
        });
        let expected = reference_render_to_texture(&device, &queue, extent, |context, target| {
            shaders.draw(
                context,
                target,
                wgpu::TextureFormat::Rgba8Unorm,
                original,
                &[source.get_y_source(), source.get_uv_source()],
            );
        });
        compare(
            &actual,
            &expected,
            &format!("Movie conversion{index} {extent:?}"),
        );
        assert!(
            actual
                .chunks_exact(4)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 3,
            "Movie conversion{index}: nonuniform decoded planes must project visibly"
        );
    }
    let background = UnormColor::from_rgba(173, 127, 91, 113);
    let mut frames = 0;
    for (index, case) in finals.iter().enumerate() {
        let extent = (case.conversion[7], case.conversion[8]);
        let decoded = nv12(extent, index + 123);
        let source = shin_video::VideoFrameTexture::new(&device, decoded.size);
        source.write_data_nv12(&queue, &decoded);
        // These final-stage planes use different bytes from the82 conversion
        // probes. Check the intermediate independently before assigning any
        // final-frame mismatch to the ordinary shader or blend state.
        let intermediate_actual =
            reference_render_to_texture(&device, &queue, extent, |context, target| {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Movie final-case conversion isolation",
                );
                ConversionPacket::new(extent).render(
                    &mut pass,
                    source.get_y_source(),
                    source.get_uv_source(),
                );
            });
        let intermediate_expected =
            reference_render_to_texture(&device, &queue, extent, |context, target| {
                shaders.draw(
                    context,
                    target,
                    wgpu::TextureFormat::Rgba8Unorm,
                    &case.conversion,
                    &[source.get_y_source(), source.get_uv_source()],
                );
            });
        compare(
            &intermediate_actual,
            &intermediate_expected,
            &format!("Movie final-case conversion{index} {extent:?}"),
        );
        for output in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, output, |context, target| {
                let mut cache = Cache::new(&device, extent);
                {
                    let mut pass = context.begin_pass(
                        cache.texture.as_texture_target(),
                        None,
                        "Movie intermediate",
                    );
                    ConversionPacket::new(extent).render(
                        &mut pass,
                        source.get_y_source(),
                        source.get_uv_source(),
                    );
                }
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Movie ordinary final output",
                );
                pass.clear(Some(background), Some(0), Some(1.));
                case.packet.render(
                    &mut pass,
                    cache.texture.as_texture_source(),
                    ((case.final_draw[20] >> 16) & 255) as u8,
                );
            });
            let expected =
                reference_render_to_texture(&device, &queue, output, |context, target| {
                    let mut cache = Cache::new(&device, extent);
                    let intermediate = cache.texture.as_texture_target();
                    shaders.draw(
                        context,
                        intermediate.view,
                        shin_render::TEXTURE_FORMAT,
                        &case.conversion,
                        &[source.get_y_source(), source.get_uv_source()],
                    );
                    {
                        let mut pass = context.begin_pass(
                            TextureTarget {
                                view: target,
                                kind: TextureTargetKind::Screen,
                            },
                            Some(context.depth_stencil),
                            "Original Movie background",
                        );
                        pass.clear(Some(background), Some(0), Some(1.));
                    }
                    shaders.draw(
                        context,
                        target,
                        wgpu::TextureFormat::Rgba8Unorm,
                        &case.final_draw,
                        &[cache.texture.as_texture_source()],
                    );
                });
            compare(
                &actual,
                &expected,
                &format!("Movie two-stage{index} {output:?}"),
            );
            frames += 1;
        }
    }
    assert_eq!(frames, 152);
    eprintln!(
        "Higurashi Movie pixels:82 original conversion packets and152 two-stage720p/1080p frames, nonuniform NV12, literal original20/6..11 GLSL; no original SDK/audio/window/save claim"
    );
}
