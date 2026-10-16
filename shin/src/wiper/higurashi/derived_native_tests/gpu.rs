//! NEW literal original2/21/22 WIPE packet replay. The proven standalone
//! native pipeline/clear/blend transport is copied into this new file; source
//! textures are explicit fixtures, and no production wiper/material is called.
use crate::render::native_gpu_state::{compare, native_blend, stencil_operation};
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use wgpu::util::DeviceExt;

use crate::{
    render::PreRenderContext,
    wiper::higurashi::derived_native_tests::{Event, Row},
};

struct Resource {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Resource {
    fn new(device: &wgpu::Device, kind: u32, width: u32, height: u32) -> Self {
        assert!(kind <= 1 && width > 0 && height > 0);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Original derived WIPE resource"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if kind == 0 {
                shin_render::TEXTURE_FORMAT
            } else {
                shin_render::DEPTH_STENCIL_FORMAT
            },
            usage: if kind == 0 {
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            },
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            _texture: texture,
            view,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    program: u32,
    blend: u32,
    primitive: u32,
    depth: Option<[u32; 2]>,
    stencil: [u32; 5],
}
struct Program {
    vertex: wgpu::ShaderModule,
    fragment: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    buffers: usize,
    textures: usize,
}
impl Program {
    fn new(device: &wgpu::Device, program: u32) -> Self {
        let (directory, name, buffers, textures) = match program {
            0 | 1 => (
                "SHIN_HIGURASHI_LEAF_CLIP_SHADERS",
                format!("native-leaf-clip-{program}"),
                1,
                0,
            ),
            2 | 21 | 22 => (
                "SHIN_HIGURASHI_DERIVED_WIPE_SHADERS",
                format!("native-derived-wipe-{program}"),
                if program == 2 { 1 } else { 2 },
                match program {
                    2 => 1,
                    21 => 2,
                    22 => 3,
                    _ => unreachable!(),
                },
            ),
            value => panic!("Unreviewed original derived WIPE program{value}"),
        };
        let directory = PathBuf::from(std::env::var(directory).unwrap());
        let module = |stage| {
            let raw = std::fs::read(directory.join(format!("{name}.{stage}.spv"))).unwrap();
            assert_eq!(raw.len() % 4, 0);
            let words = raw
                .chunks_exact(4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                .collect();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Original derived WIPE GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(words)),
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
        for unit in 0..textures {
            entries.extend([
                wgpu::BindGroupLayoutEntry {
                    binding: (buffers + 2 * unit) as u32,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: (buffers + 2 * unit + 1) as u32,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ]);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        Self {
            vertex: module("vert"),
            fragment: module("frag"),
            layout,
            pipeline_layout,
            buffers,
            textures,
        }
    }
    fn pipeline(&self, device: &wgpu::Device, key: Key) -> wgpu::RenderPipeline {
        let attributes = if key.program == 0 {
            vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            }]
        } else if key.program == 1 {
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
        } else if key.program == 2 {
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
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 16,
                    shader_location: 2,
                },
            ]
        } else if key.program == 22 {
            vec![
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 8,
                    shader_location: 1,
                },
            ]
        } else {
            vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }]
        };
        let stencil = wgpu::StencilFaceState {
            compare: compare(key.stencil[0]),
            fail_op: stencil_operation(key.stencil[1]),
            depth_fail_op: stencil_operation(key.stencil[2]),
            pass_op: stencil_operation(key.stencil[3]),
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original derived WIPE packet pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: match key.program {
                        0 => 12,
                        2 | 22 => 24,
                        _ => 16,
                    },
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
                    value => panic!("Unreviewed original derived WIPE primitive{value}"),
                },
                ..Default::default()
            },
            depth_stencil: key.depth.map(|depth| wgpu::DepthStencilState {
                format: shin_render::DEPTH_STENCIL_FORMAT,
                depth_write_enabled: Some(depth[1] != 0),
                depth_compare: Some(compare(depth[0])),
                stencil: wgpu::StencilState {
                    front: stencil,
                    back: stencil,
                    read_mask: key.stencil[4] & 255,
                    write_mask: (key.stencil[4] >> 8) & 255,
                },
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

struct Renderer {
    programs: HashMap<u32, Program>,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
}
impl Renderer {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            programs: HashMap::new(),
            pipelines: HashMap::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Native min5/mag1/wrap7"),
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
    fn draw(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        sources: &[&wgpu::TextureView],
        viewport: (u32, u32),
        key: Key,
        uniforms: &[[u32; 16]; 4],
        vertices: &[u32],
        count: u32,
    ) {
        assert!(viewport.0 > 0 && viewport.1 > 0 && count > 0);
        assert_eq!(key.depth.is_some(), depth.is_some());
        let program = self
            .programs
            .entry(key.program)
            .or_insert_with(|| Program::new(context.device, key.program));
        let pipeline = self
            .pipelines
            .entry(key)
            .or_insert_with(|| program.pipeline(context.device, key));
        assert_eq!(sources.len(), program.textures);
        let buffers = uniforms[..program.buffers]
            .iter()
            .map(|words| {
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Original derived WIPE packet uniform bytes"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original derived WIPE packet vertex bytes"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut entries = buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        for (unit, source) in sources.iter().enumerate() {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: (program.buffers + 2 * unit) as u32,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: (program.buffers + 2 * unit + 1) as u32,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ]);
        }
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &program.layout,
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Original derived WIPE packet"),
                multiview_mask: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth.map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_viewport(0., 0., viewport.0 as f32, viewport.1 as f32, 0., 1.);
        pass.set_stencil_reference((key.stencil[4] >> 16) & 255);
        pass.draw(0..count, 0..1);
    }
}

/// Original isolated D row. Source IDs1/2/3 mean outgoing/incoming/Mask.
/// Mask dimensions/format/pixels are explicit supplied fixtures. Raw native
/// matrix/vertices/uniforms/blends drive rendering without normalized helpers.
pub(crate) fn replay(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    row: &Row,
    sources: [&wgpu::TextureView; 3],
) {
    assert!(matches!(row.operation, super::Operation::Draw(_)));
    let mut renderer = Renderer::new(context.device);
    let mut depth = None;
    let mut current = [0; 4];
    let mut stack = vec![];
    let mut depth_state = [7, 0];
    for event in &row.events {
        match event {
            Event::Resource(v) if v[1] == 1 => {
                assert!(depth.is_none());
                depth = Some((v[0], Resource::new(context.device, 1, v[2], v[3])));
            }
            Event::Target(v) => {
                stack.push(current);
                current = *v;
                assert_eq!(depth.as_ref().unwrap().0, v[1]);
            }
            Event::Restore(v) => {
                assert_eq!(stack.pop().unwrap(), *v);
                current = *v;
            }
            Event::Viewport(v) => {
                current[2] = v[0];
                current[3] = v[1];
            }
            Event::Depth(v) => depth_state = *v,
            Event::Clear(v) => {
                assert_eq!(current[0], v[0]);
                let d = &depth.as_ref().unwrap().1;
                // The native output color is the supplied observation target;
                // only its standalone depth attachment is allocated here.
                let mut uniforms = [[0; 16]; 4];
                uniforms[0][..4].copy_from_slice(
                    &[
                        ((v[2] >> 16) & 255) as f32 / 255.,
                        ((v[2] >> 8) & 255) as f32 / 255.,
                        (v[2] & 255) as f32 / 255.,
                        (v[2] >> 24) as f32 / 255.,
                    ]
                    .map(f32::to_bits),
                );
                let z = f32::from_bits(v[4]);
                let vertices = [-1f32, 1., z, 3., 1., z, -1., -3., z].map(f32::to_bits);
                renderer.draw(
                    context,
                    target,
                    Some(&d.view),
                    &[],
                    (current[2], current[3]),
                    Key {
                        program: 0,
                        blend: v[1] & 1,
                        primitive: 0,
                        depth: Some([7, u32::from(v[1] & 2 != 0)]),
                        stencil: [
                            7,
                            0,
                            0,
                            if v[1] & 4 != 0 { 2 } else { 0 },
                            0xffff | ((v[3] & 255) << 16),
                        ],
                    },
                    &uniforms,
                    &vertices,
                    3,
                );
            }
            Event::Draw(words) => {
                let d = &**words;
                assert_eq!(current, [d[5], d[6], d[7], d[8]]);
                assert_eq!(d[12], 101);
                let count = match d[3] {
                    2 => 1,
                    21 => 2,
                    22 => 3,
                    v => panic!("Unreviewed original derivedWipe program{v}"),
                };
                let views = (0..count)
                    .map(|unit| {
                        let id = d[13 + unit];
                        assert!((1..=3).contains(&id));
                        sources[(id - 1) as usize]
                    })
                    .collect::<Vec<_>>();
                let uniforms =
                    std::array::from_fn(|i| d[25 + i * 16..41 + i * 16].try_into().unwrap());
                renderer.draw(
                    context,
                    target,
                    Some(&depth.as_ref().unwrap().1.view),
                    &views,
                    (d[7], d[8]),
                    Key {
                        program: d[3],
                        blend: d[4],
                        primitive: d[9],
                        depth: Some(depth_state),
                        stencil: d[16..21].try_into().unwrap(),
                    },
                    &uniforms,
                    &d[89..89 + d[11] as usize / 4],
                    d[10],
                );
            }
            _ => {}
        }
    }
    assert!(stack.is_empty());
    assert_eq!(current, [0; 4]);
}
