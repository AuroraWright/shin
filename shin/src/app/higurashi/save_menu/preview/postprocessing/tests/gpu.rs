use super::*;
use std::borrow::Cow;
use wgpu::util::DeviceExt;

pub(in crate::app::higurashi::save_menu::preview) struct NativeShader {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl NativeShader {
    pub fn new(device: &wgpu::Device, operation: Option<usize>, blend: u32) -> Self {
        let dir =
            std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_POSTPROCESS_SHADERS").unwrap());
        let suffix = operation.map_or_else(|| "fill".into(), |n| n.to_string());
        let module = |stage| {
            let raw = std::fs::read(dir.join(format!("native-postprocess-{suffix}.{stage}.spv")))
                .unwrap();
            let words = raw
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .collect();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Original updated framebuffer GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(words)),
            })
        };
        let vertex = module("vert");
        let fragment = module("frag");
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
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = if operation.is_some() {
            vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }]
        } else {
            vec![
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
            ]
        };
        let blend = match blend {
            1 => shin_render::ColorBlendType::Opaque,
            2 => shin_render::ColorBlendType::Layer1,
            3 => shin_render::ColorBlendType::Layer2,
            // Actual a8630 equation3=ReverseSubtract with SrcAlpha/One,
            // alpha Add with OneMinusDstAlpha/One; preserve the older path.
            4 => shin_render::ColorBlendType::HigurashiLayer3,
            _ => panic!("unreviewed native reference blend"),
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original framebuffer GLSL reference"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shin_render::TEXTURE_FORMAT,
                    blend: Some(blend.into()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
    }
    pub(super) fn draw(
        &self,
        context: &mut PreRenderContext,
        source: TextureSource,
        target: TextureTarget,
        draw: &Draw,
    ) {
        let buffers = draw.words[..48]
            .chunks_exact(16)
            .map(|values| {
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(values),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(
                    &draw.words[48..48 + draw.vertex_bytes as usize / 4],
                ),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut entries = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(source.view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(context.sampler_store.get(source.sampler)),
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
                label: Some("Original framebuffer shader"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..draw.count, 0..1);
    }
    pub fn draw_quad(
        &self,
        context: &mut PreRenderContext,
        source: TextureSource,
        target: TextureTarget,
        matrix: Mat4,
        color: Vec4,
        param: Vec4,
        vertices: &[PosTexVertex; 4],
    ) {
        let mut words = [0; 72];
        words[..16].copy_from_slice(&matrix.to_cols_array().map(f32::to_bits));
        words[16..20].copy_from_slice(&color.to_array().map(f32::to_bits));
        words[32..36].copy_from_slice(&param.to_array().map(f32::to_bits));
        words[48..64].copy_from_slice(bytemuck::cast_slice(vertices));
        self.draw(
            context,
            source,
            target,
            &Draw {
                program: 0,
                blend: 0,
                texture: 1,
                primitive: 1,
                count: 4,
                vertex_bytes: 64,
                words,
            },
        );
    }
}
