//! Raw original programs1/2. No production Sprite/Fill program or blend helper
//! participates in the expected image; literal native uniforms/vertices do.
use super::*;
use crate::render::PreRenderContext;
use shin_render::shaders::types::texture::TextureSource;
use std::borrow::Cow;
use wgpu::util::DeviceExt;

pub(super) struct Shaders {
    layouts: [wgpu::BindGroupLayout; 2],
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
}
impl Shaders {
    pub fn new(device: &wgpu::Device) -> Self {
        let dir = std::path::PathBuf::from(
            std::env::var("SHIN_HIGURASHI_LIVE_TEMPORARY_SHADERS").unwrap(),
        );
        let layouts = std::array::from_fn(|operation| {
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }];
            if operation == 1 {
                entries.extend([
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ]);
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Original live widget descriptors"),
                entries: &entries,
            })
        });
        let pipelines = std::array::from_fn(|operation| {
            let module = |stage| {
                let bytes = std::fs::read(dir.join(format!(
                    "native-live-temporary-{}.{stage}.spv",
                    operation + 1
                )))
                .unwrap();
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Literal updated live widget GLSL"),
                    source: wgpu::ShaderSource::SpirV(Cow::Owned(
                        bytes
                            .chunks_exact(4)
                            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                            .collect(),
                    )),
                })
            };
            let vertex = module("vert");
            let fragment = module("frag");
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layouts[operation])],
                immediate_size: 0,
            });
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
            if operation == 1 {
                attributes.push(wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 16,
                    shader_location: 2,
                });
            }
            std::array::from_fn(|blend| {
                // Actual a8630 opaque1 and ordinary2 equations/factors. Native
                // packed colors are BGRA; the literal vertex source swizzles.
                let blend = if blend == 0 {
                    wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }
                } else {
                    wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }
                };
                let stencil = wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::GreaterEqual,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Replace,
                };
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Original live widget packet replay"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &vertex,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: if operation == 0 { 16 } else { 24 },
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &fragment,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: shin_render::DEPTH_STENCIL_FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: wgpu::StencilState {
                            front: stencil,
                            back: stencil,
                            read_mask: 255,
                            write_mask: 255,
                        },
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
        });
        Self { layouts, pipelines }
    }
    pub fn draw(
        &self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        draw: &Draw,
        texture: Option<TextureSource>,
    ) {
        assert!(matches!(draw.program, 1 | 2));
        assert!(matches!(draw.blend, 1 | 2));
        assert_eq!(draw.mask, [7, 1]);
        let operation = (draw.program - 1) as usize;
        let uniform = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&draw.uniforms[..16]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&draw.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }];
        if let Some(texture) = texture {
            assert_eq!(draw.program, 2);
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(
                        context.sampler_store.get(texture.sampler),
                    ),
                },
            ]);
        } else {
            assert_eq!(draw.program, 1);
        }
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layouts[operation],
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Literal live widget original packet"),
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
        pass.set_pipeline(&self.pipelines[operation][(draw.blend - 1) as usize]);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_stencil_reference(draw.mask[0]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..4, 0..1);
    }
}
