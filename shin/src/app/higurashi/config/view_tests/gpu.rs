//! Independent literal original Config programs1/2/5 raw WGPU replay.
//! No Clip, production material shader, transform bridge, or render builder is
//! used here. Matrices/vertices are the original uploaded GL words; the literal
//! vertex GLSL applies only the already audited GL-to-WGPU depth bridge.
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use shin_render::shaders::types::texture::TextureSource;
use wgpu::util::DeviceExt;

use crate::{asset::picture::GpuPictureBlock, render::PreRenderContext};

#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub program: u32,
    pub blend: u32,
    pub primitive: u32,
    pub depth: [u32; 2],
    pub stencil: [u32; 5],
    /// Shader descriptor order, with precisely the uploaded meaningful words.
    /// Clear0 uses the native fragment-slot1 color as its sole descriptor0.
    pub uniforms: Vec<Vec<u32>>,
    pub vertices: Vec<u32>,
    pub count: u32,
    pub start: u32,
}
impl Draw {
    /// NEW native-leaf-clip-clear.c D50: six draw header words, depth2,
    /// stencil5, matrix16, fragment color4, vertexbytes1, vertices16.
    #[cfg(test)]
    #[allow(dead_code, reason = "Shared GPU fixture API; individual render suites use different subsets.")]
    pub(crate) fn from_clip_packet(words: [u32; 50]) -> Self {
        assert!(words[0] <= 1);
        assert_eq!(words[5], 0); // Native cullNone.
        let count = words[33] as usize / 4;
        assert_eq!(words[33] % 4, 0);
        assert_eq!(count, if words[0] == 0 { 9 } else { 16 });
        Self {
            program: words[0],
            blend: words[1],
            primitive: words[2],
            depth: words[6..8].try_into().unwrap(),
            stencil: words[8..13].try_into().unwrap(),
            uniforms: vec![if words[0] == 0 {
                words[29..33].to_vec()
            } else {
                words[13..29].to_vec()
            }],
            vertices: words[34..34 + count].to_vec(),
            count: words[3],
            start: words[4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    program: u32,
    blend: u32,
    primitive: u32,
    depth: [u32; 2],
    stencil: [u32; 4],
    masks: u32,
}
impl From<&Draw> for Key {
    fn from(draw: &Draw) -> Self {
        Self {
            program: draw.program,
            blend: draw.blend,
            primitive: draw.primitive,
            depth: draw.depth,
            stencil: draw.stencil[..4].try_into().unwrap(),
            masks: draw.stencil[4] & 0xffff,
        }
    }
}

struct Program {
    vertex: wgpu::ShaderModule,
    fragment: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    buffers: usize,
    textured: bool,
    stride: u64,
}
impl Program {
    fn new(device: &wgpu::Device, program: u32) -> Self {
        let (environment, stem, buffers, textured, stride) = match program {
            1 | 2 | 5 => (
                "SHIN_HIGURASHI_CONFIG_SHADERS",
                format!("native-config-{program}"),
                if program == 5 { 2 } else { 1 },
                program != 1,
                if program == 1 { 16 } else { 24 },
            ),
            value => panic!("Unreviewed original Config program {value}"),
        };
        let directory = PathBuf::from(std::env::var(environment).unwrap());
        let shader = |stage| {
            let bytes = std::fs::read(directory.join(format!("{stem}.{stage}.spv"))).unwrap();
            assert_eq!(bytes.len() % 4, 0);
            assert_eq!(&bytes[..4], &0x07230203u32.to_le_bytes());
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Literal original leaf clipping/material GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(
                    bytes
                        .chunks_exact(4)
                        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                        .collect(),
                )),
            })
        };
        let mut entries = (0..buffers)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>();
        if textured {
            entries.extend([
                wgpu::BindGroupLayoutEntry {
                    binding: buffers as u32,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: buffers as u32 + 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ]);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Original leaf clip/material uploaded descriptors"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Original leaf clip/material pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        Self {
            vertex: shader("vert"),
            fragment: shader("frag"),
            layout,
            pipeline_layout,
            buffers,
            textured,
            stride,
        }
    }

    fn pipeline(&self, device: &wgpu::Device, key: Key) -> wgpu::RenderPipeline {
        let attributes = match key.program {
            0 => vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            }],
            1 | 2 | 5 => {
                let mut attributes = vec![
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Unorm8x4,
                        offset: 12,
                        shader_location: 1,
                    },
                ];
                if key.program == 2 || key.program == 5 {
                    attributes.push(wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 16,
                        shader_location: 2,
                    });
                }
                attributes
            }
            _ => vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }],
        };
        let stencil = wgpu::StencilFaceState {
            compare: compare(key.stencil[0]),
            fail_op: stencil_operation(key.stencil[1]),
            depth_fail_op: stencil_operation(key.stencil[2]),
            pass_op: stencil_operation(key.stencil[3]),
        };
        assert!(key.depth[1] <= 1);
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original leaf clip/material raw depth and stencil packet"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: self.stride,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shin_render::TEXTURE_FORMAT,
                    blend: (key.blend != 0).then(|| native_blend(key.blend)),
                    write_mask: if key.blend == 0 {
                        wgpu::ColorWrites::empty()
                    } else {
                        wgpu::ColorWrites::ALL
                    },
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: match key.primitive {
                    0 => wgpu::PrimitiveTopology::TriangleList,
                    1 => wgpu::PrimitiveTopology::TriangleStrip,
                    value => panic!("Unreviewed original primitive {value}"),
                },
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: shin_render::DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(key.depth[1] != 0),
                depth_compare: Some(compare(key.depth[0])),
                stencil: wgpu::StencilState {
                    front: stencil,
                    back: stencil,
                    read_mask: key.masks & 255,
                    write_mask: (key.masks >> 8) & 255,
                },
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

fn compare(value: u32) -> wgpu::CompareFunction {
    match value {
        0 => wgpu::CompareFunction::Never,
        1 => wgpu::CompareFunction::Less,
        2 => wgpu::CompareFunction::Equal,
        3 => wgpu::CompareFunction::LessEqual,
        4 => wgpu::CompareFunction::Greater,
        5 => wgpu::CompareFunction::NotEqual,
        6 => wgpu::CompareFunction::GreaterEqual,
        7 => wgpu::CompareFunction::Always,
        value => panic!("Unreviewed original compare {value}"),
    }
}
fn stencil_operation(value: u32) -> wgpu::StencilOperation {
    match value {
        0 => wgpu::StencilOperation::Keep,
        1 => wgpu::StencilOperation::Zero,
        2 => wgpu::StencilOperation::Replace,
        3 => wgpu::StencilOperation::IncrementClamp,
        4 => wgpu::StencilOperation::DecrementClamp,
        5 => wgpu::StencilOperation::Invert,
        6 => wgpu::StencilOperation::IncrementWrap,
        7 => wgpu::StencilOperation::DecrementWrap,
        value => panic!("Unreviewed original stencil operation {value}"),
    }
}
fn native_blend(value: u32) -> wgpu::BlendState {
    // Original a8630 endpoint + SDK enum names. This is independent of the
    // shared ColorBlendType conversion, including older RGB ReverseSubtract.
    // https://smo.monsterdruide.one/docs/nvn_8h.html
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
    let (source, destination, operation) = match value {
        2 => (F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
        3 => (F::SrcAlpha, F::One, O::Add),
        4 => (F::SrcAlpha, F::One, O::ReverseSubtract),
        5 => (F::One, F::OneMinusSrcAlpha, O::Add),
        value => panic!("Unreviewed original blend {value}"),
    };
    wgpu::BlendState {
        color: BlendComponent {
            src_factor: source,
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

pub(crate) struct Renderer {
    programs: HashMap<u32, Program>,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
}
impl Renderer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            programs: HashMap::new(),
            pipelines: HashMap::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Original native leaf sampler min5/mag1/wrap7"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    pub(crate) fn draw(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        draw: &Draw,
        source: Option<TextureSource>,
        viewport: Option<[f32; 4]>,
    ) {
        assert!(draw.count > 0 && !draw.vertices.is_empty());
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original leaf uploaded vertex words"),
                contents: bytemuck::cast_slice(&draw.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.submit(
            context,
            target,
            draw,
            source,
            viewport,
            vertices.slice(..),
            None,
        );
    }

    #[cfg(test)]
    #[allow(dead_code, reason = "Shared GPU fixture API; individual render suites use different subsets.")]
    pub(crate) fn draw_block(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        draw: &Draw,
        block: &GpuPictureBlock,
        viewport: Option<[f32; 4]>,
    ) {
        assert!(matches!(draw.program, 6..=11) && draw.primitive == 0);
        let vertices = block.vertex_buffer.as_buffer_ref();
        let indices = block.index_buffer.as_buffer_ref();
        assert!(draw.start.checked_add(draw.count).unwrap() as usize <= indices.count());
        self.submit(
            context,
            target,
            draw,
            Some(block.texture.as_source()),
            viewport,
            vertices.as_wgpu_slice(),
            Some(indices.as_wgpu_slice()),
        );
    }

    fn submit(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        draw: &Draw,
        source: Option<TextureSource>,
        viewport: Option<[f32; 4]>,
        vertices: wgpu::BufferSlice<'_>,
        indices: Option<wgpu::BufferSlice<'_>>,
    ) {
        let key = Key::from(draw);
        let program = self
            .programs
            .entry(draw.program)
            .or_insert_with(|| Program::new(context.device, draw.program));
        let pipeline = self
            .pipelines
            .entry(key)
            .or_insert_with(|| program.pipeline(context.device, key));
        assert_eq!(program.buffers, draw.uniforms.len());
        assert_eq!(program.textured, source.is_some());
        let uniforms = draw
            .uniforms
            .iter()
            .map(|words| {
                assert!(matches!(words.len(), 4 | 8 | 16));
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Original native uploaded meaningful uniform words"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let mut entries = uniforms
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        if let Some(source) = source {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: program.buffers as u32,
                    resource: wgpu::BindingResource::TextureView(source.view),
                },
                wgpu::BindGroupEntry {
                    binding: program.buffers as u32 + 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ]);
        }
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Original native leaf clip/material bindings"),
                layout: &program.layout,
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Independent original native leaf packet replay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: context.depth_stencil.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        if let Some([x, y, width, height]) = viewport {
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices);
        pass.set_stencil_reference((draw.stencil[4] >> 16) & 255);
        let end = draw.start.checked_add(draw.count).unwrap();
        if let Some(indices) = indices {
            pass.set_index_buffer(indices, wgpu::IndexFormat::Uint16);
            pass.draw_indexed(draw.start..end, 0, 0..1);
        } else {
            pass.draw(draw.start..end, 0..1);
        }
    }
}
