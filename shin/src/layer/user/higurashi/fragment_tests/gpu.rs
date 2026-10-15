//! Independent original GLSL replay of native PIC/BUP block index packets.
//! This does not call the production block renderer or its shader/output policy.
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use wgpu::util::DeviceExt;

use crate::{asset::picture::GpuPictureBlock, render::PreRenderContext};

#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub program: u32,
    pub blend: u32,
    /// Original u16 index units, already selected by native6a04c/a8cd0.
    pub index_start: u32,
    pub index_count: u32,
    pub matrix: [u32; 16],
    pub color: [u32; 4],
    pub param: [u32; 4],
    pub stencil: u32,
}

struct Program {
    vertex: wgpu::ShaderModule,
    fragment: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}
impl Program {
    fn new(device: &wgpu::Device, program: u32) -> Self {
        let operation = [6, 9, 7, 8, 10, 11]
            .iter()
            .position(|&value| value == program)
            .unwrap_or_else(|| panic!("Unreviewed native PIC/BUP program {program}"));
        let directory = PathBuf::from(
            std::env::var("SHIN_HIGURASHI_PRIMARY_BLOCK_SHADERS")
                .expect("Original primary block SPIR-V directory"),
        );
        let shader = |stage| {
            let bytes = std::fs::read(
                directory.join(format!("native-primary-block-{operation}.{stage}.spv")),
            )
            .unwrap();
            assert_eq!(bytes.len() % 4, 0);
            assert_eq!(&bytes[..4], &0x07230203u32.to_le_bytes());
            let words = bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Original PIC/BUP ordinary GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(words)),
            })
        };
        let mut entries = (0..3)
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
            .collect::<Vec<_>>();
        entries.extend([
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Original PIC/BUP uniform and texture layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Original PIC/BUP pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        Self {
            vertex: shader("vert"),
            fragment: shader("frag"),
            layout,
            pipeline_layout,
        }
    }

    fn pipeline(&self, device: &wgpu::Device, blend: u32) -> wgpu::RenderPipeline {
        let ordering = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Greater,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Replace,
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original PIC/BUP native block packet"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex,
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
                module: &self.fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shin_render::TEXTURE_FORMAT,
                    blend: Some(native_blend(blend)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: shin_render::DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: ordering,
                    back: ordering,
                    read_mask: 255,
                    write_mask: 255,
                },
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

fn native_blend(value: u32) -> wgpu::BlendState {
    // Original a8630 endpoint: blend4 RGB equation3=ReverseSubtract,
    // alpha equation1=Add, factors5/2/8/2=SrcAlpha/One/OneMinusDstAlpha/One.
    // NVN SDK enums: https://smo.monsterdruide.one/docs/nvn_8h.html
    // Keep this independent of the shared engine's Layer3 blend conversion.
    use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    if value == 1 {
        return BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
        };
    }
    let (destination, operation) = match value {
        2 => (BlendFactor::OneMinusSrcAlpha, BlendOperation::Add),
        3 => (BlendFactor::One, BlendOperation::Add),
        4 => (BlendFactor::One, BlendOperation::ReverseSubtract),
        _ => panic!("Unreviewed native PIC/BUP blend {value}"),
    };
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: destination,
            operation,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::OneMinusDstAlpha,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
    }
}

/// Preserve the root test's background and depth/stencil clear between blocks.
/// Each draw uses the actual native subrange and straight original GLSL output.
pub(crate) fn replay(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    block: &GpuPictureBlock,
    draws: &[Draw],
) {
    let sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Original PIC/BUP default sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let vertices = block.vertex_buffer.as_buffer_ref();
    let indices = block.index_buffer.as_buffer_ref();
    let mut programs = HashMap::new();
    let mut pipelines = HashMap::new();
    for draw in draws {
        assert!(draw.index_count > 0);
        let end = draw.index_start.checked_add(draw.index_count).unwrap();
        assert!(end as usize <= indices.count());
        assert!(draw.stencil <= 255);
        let program = programs
            .entry(draw.program)
            .or_insert_with(|| Program::new(context.device, draw.program));
        let pipeline = pipelines
            .entry((draw.program, draw.blend))
            .or_insert_with(|| program.pipeline(context.device, draw.blend));
        let uniforms = [&draw.matrix[..], &draw.color[..], &draw.param[..]].map(|words| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Original PIC/BUP uploaded uniform words"),
                    contents: bytemuck::cast_slice(words),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        });
        let mut entries = uniforms
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        entries.extend([
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(block.texture.as_source().view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]);
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Original PIC/BUP native block bindings"),
                layout: &program.layout,
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Original PIC/BUP indexed block replay"),
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.as_wgpu_slice());
        pass.set_index_buffer(indices.as_wgpu_slice(), wgpu::IndexFormat::Uint16);
        pass.set_stencil_reference(draw.stencil);
        pass.draw_indexed(draw.index_start..end, 0, 0..1);
    }
}
