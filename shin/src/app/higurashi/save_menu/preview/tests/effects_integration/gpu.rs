//! Replay original CPU resource/state/draw events through the extracted GLSL.
//! The reference consumes original packets; it does not invoke effects::View.
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use wgpu::util::DeviceExt;

use crate::{
    app::higurashi::save_menu::preview::effects::tests::native::{Case, Event, Original},
    render::PreRenderContext,
};

struct Resource {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    kind: u32,
    extent: (u32, u32),
    retired: bool,
}
impl Resource {
    fn new(device: &wgpu::Device, kind: u32, width: u32, height: u32) -> Self {
        assert!(kind <= 1 && width > 0 && height > 0);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Original saved effect resource"),
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
            texture,
            view,
            kind,
            extent: (width, height),
            retired: false,
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
}
impl Program {
    fn new(device: &wgpu::Device, program: u32) -> Self {
        let (directory, name, buffers) = match program {
            1 => (
                "SHIN_HIGURASHI_POSTPROCESS_SHADERS",
                "native-postprocess-fill".to_owned(),
                3,
            ),
            6 | 7 | 8 | 9 | 10 | 11 => {
                let operation = [6, 9, 7, 8, 10, 11]
                    .iter()
                    .position(|&p| p == program)
                    .unwrap();
                (
                    "SHIN_HIGURASHI_POSTPROCESS_SHADERS",
                    format!("native-postprocess-{operation}"),
                    3,
                )
            }
            23 | 24 => (
                "SHIN_HIGURASHI_EFFECTS_SHADERS",
                format!("native-save-effect-{program}"),
                if program == 23 { 4 } else { 3 },
            ),
            value => panic!("Unreviewed original saved effect program{value}"),
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
                label: Some("Original saved effect GLSL"),
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
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: buffers as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: buffers as u32 + 1,
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
        Self {
            vertex: module("vert"),
            fragment: module("frag"),
            layout,
            pipeline_layout,
            buffers,
        }
    }
    fn pipeline(&self, device: &wgpu::Device, key: Key) -> wgpu::RenderPipeline {
        let attributes = if key.program == 1 {
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
        } else {
            vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }]
        };
        let blend = match key.blend {
            0 => shin_render::ColorBlendType::NoColor,
            1 => shin_render::ColorBlendType::Opaque,
            2 => shin_render::ColorBlendType::Layer1,
            3 => shin_render::ColorBlendType::Layer2,
            4 => shin_render::ColorBlendType::HigurashiLayer3,
            value => panic!("Unreviewed original saved effect blend{value}"),
        };
        let stencil = wgpu::StencilFaceState {
            compare: compare(key.stencil[0]),
            fail_op: stencil_operation(key.stencil[1]),
            depth_fail_op: stencil_operation(key.stencil[2]),
            pass_op: stencil_operation(key.stencil[3]),
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original saved effect packet pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
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
                    blend: Some(blend.into()),
                    write_mask: blend.into(),
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: match key.primitive {
                    0 => wgpu::PrimitiveTopology::TriangleList,
                    1 => wgpu::PrimitiveTopology::TriangleStrip,
                    value => panic!("Unreviewed original saved effect primitive{value}"),
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
        value => panic!("Unreviewed original comparison{value}"),
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
        value => panic!("Unreviewed original stencil operation{value}"),
    }
}

struct Renderer {
    programs: HashMap<u32, Program>,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
    linear: wgpu::Sampler,
    mosaic: wgpu::Sampler,
    dummy: Resource,
}
impl Renderer {
    fn new(device: &wgpu::Device) -> Self {
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                min_filter: filter,
                mag_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            programs: HashMap::new(),
            pipelines: HashMap::new(),
            linear: sampler(wgpu::FilterMode::Linear),
            mosaic: sampler(wgpu::FilterMode::Nearest),
            dummy: Resource::new(device, 0, 1, 1),
        }
    }
    fn draw(
        &mut self,
        context: &mut PreRenderContext,
        target: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        source: Option<&wgpu::TextureView>,
        mosaic: bool,
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
        let buffers = uniforms[..program.buffers]
            .iter()
            .map(|words| {
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Original packet uniform bytes"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original packet vertex bytes"),
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
        entries.push(wgpu::BindGroupEntry {
            binding: program.buffers as u32,
            resource: wgpu::BindingResource::TextureView(source.unwrap_or(&self.dummy.view)),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: program.buffers as u32 + 1,
            resource: wgpu::BindingResource::Sampler(if mosaic {
                &self.mosaic
            } else {
                &self.linear
            }),
        });
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
                multiview_mask: None,
                label: Some("Original saved effect packet"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target,
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
    fn clear(
        &mut self,
        context: &mut PreRenderContext,
        color: &Resource,
        depth: Option<&Resource>,
        viewport: (u32, u32),
        flags: u32,
        argb: u32,
        stencil: u32,
        z: f32,
    ) {
        assert!(flags <= 7 && z.is_finite() && (0.0..=1.0).contains(&z));
        if flags == 0 {
            return;
        }
        // Native a8fe0 clears through a full-viewport triangle. A reduced Mosaic
        // viewport must preserve pixels outside it; attachment LoadOp::Clear
        // would clear the entire1920x1080 owner instead.
        let rgba =
            ((argb >> 16) & 255) | (argb & 0xff00) | ((argb & 255) << 16) | (argb & 0xff000000);
        // Original a8fe0 puts its argument directly in clip Z. The extracted
        // vertex shader's single OpenGL/WGPU bridge performs the depth mapping.
        let zz = z.to_bits();
        let vertices = [-1f32, 1., 3., 1., -1., -3.].map(f32::to_bits);
        let geometry = [
            vertices[0],
            vertices[1],
            zz,
            rgba,
            vertices[2],
            vertices[3],
            zz,
            rgba,
            vertices[4],
            vertices[5],
            zz,
            rgba,
        ];
        let mut uniforms = [[0; 16]; 4];
        uniforms[0] = glam::Mat4::IDENTITY.to_cols_array().map(f32::to_bits);
        let packed = 255 | (255 << 8) | ((stencil & 255) << 16);
        let key = Key {
            program: 1,
            blend: flags & 1,
            primitive: 0,
            depth: depth.map(|_| [7, u32::from(flags & 2 != 0)]),
            stencil: [7, 0, 0, if flags & 4 != 0 { 2 } else { 0 }, packed],
        };
        self.draw(
            context,
            &color.view,
            depth.map(|v| &v.view),
            None,
            false,
            viewport,
            key,
            &uniforms,
            &geometry,
            3,
        );
    }
}

pub(super) fn replay(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    case: &Case,
    original: &Original,
) {
    let mut renderer = Renderer::new(context.device);
    let mut resources = HashMap::<u32, Resource>::new();
    let mut stack = Vec::<[u32; 4]>::new();
    let mut current = [0; 4];
    let mut depth_state = [7, 0];
    let mut samplers = HashMap::<u32, bool>::new();
    let mut pending_filter = None;
    let mut draw_count = 0;
    let mut copy_count = 0;
    for event in &original.events {
        match event {
            Event::Resource(v) => {
                assert!(
                    resources
                        .insert(v[0], Resource::new(context.device, v[1], v[2], v[3]))
                        .is_none()
                );
            }
            Event::InitialClear(v) => {
                let color = &resources[&v[0]];
                assert_eq!(color.kind, 0);
                renderer.clear(context, color, None, color.extent, 1, v[1], 0, 1.);
            }
            Event::Target(v) => {
                stack.push(current);
                current = *v;
                assert_eq!(resources[&v[0]].kind, 0);
                if v[1] != 0 {
                    assert_eq!(resources[&v[1]].kind, 1);
                }
            }
            Event::Viewport(v) => {
                current[2] = v[0];
                current[3] = v[1];
            }
            Event::Restore(v) => {
                assert_eq!(stack.pop().unwrap(), *v);
                current = *v;
            }
            Event::Clear(v) => {
                assert_eq!(current[0], v[0]);
                let color = &resources[&v[0]];
                let depth = (current[1] != 0).then(|| &resources[&current[1]]);
                renderer.clear(
                    context,
                    color,
                    depth,
                    (current[2], current[3]),
                    v[1],
                    v[2],
                    v[3],
                    f32::from_bits(v[4]),
                );
            }
            Event::Depth(v) => {
                assert!(v[0] <= 7 && v[1] <= 1);
                depth_state = *v;
            }
            Event::SamplerFilter(v) => {
                pending_filter = Some(match *v {
                    [5, 1] => false,
                    [4, 0] => true,
                    value => panic!("Unreviewed original sampler filters{value:?}"),
                });
            }
            Event::SamplerWrap(v) => {
                assert_eq!(*v, [7, 7, 7]);
                let id = 101 + samplers.len() as u32;
                assert!(
                    samplers
                        .insert(id, pending_filter.take().unwrap())
                        .is_none()
                );
            }
            Event::Bind(_, descriptor) => {
                if let Some(words) = descriptor {
                    assert_eq!(*words, [0, 0, 1, 0, 0]);
                }
            }
            Event::Copy(v) => {
                let src = &resources[&v[0]];
                let dst = &resources[&v[1]];
                assert_eq!((src.kind, dst.kind), (0, 0));
                assert_eq!(src.extent, dst.extent);
                context.encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &src.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &dst.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: src.extent.0,
                        height: src.extent.1,
                        depth_or_array_layers: 1,
                    },
                );
                copy_count += 1;
            }
            Event::Draw(draw) => {
                let d = &**draw;
                assert_eq!(current, [d[5], d[6], d[7], d[8]]);
                let color = &resources[&d[5]];
                assert_eq!(color.kind, 0);
                let depth = (d[6] != 0).then(|| &resources[&d[6]]);
                let texture = if d[3] == 1 {
                    None
                } else {
                    Some(&resources[&d[13]].view)
                };
                let stencil = d[16..21].try_into().unwrap();
                let key = Key {
                    program: d[3],
                    blend: d[4],
                    primitive: d[9],
                    depth: depth.map(|_| depth_state),
                    stencil,
                };
                assert!(d[11] % 4 == 0 && d[11] <= 96);
                let uniforms =
                    std::array::from_fn(|i| d[25 + i * 16..41 + i * 16].try_into().unwrap());
                let mosaic = if texture.is_some() {
                    samplers[&d[12]]
                } else {
                    false
                };
                renderer.draw(
                    context,
                    &color.view,
                    depth.map(|v| &v.view),
                    texture,
                    mosaic,
                    (d[7], d[8]),
                    key,
                    &uniforms,
                    &d[89..89 + d[11] as usize / 4],
                    d[10],
                );
                draw_count += 1;
            }
            Event::Drop(v) => {
                if v[1] == 0 {
                    assert!(!resources[&v[0]].retired);
                    resources.get_mut(&v[0]).unwrap().retired = true;
                }
            }
            Event::Owner(_)
            | Event::Keep(_)
            | Event::Lower(_, _)
            | Event::Render(_)
            | Event::Stencil(_)
            | Event::Flush(_) => {}
        }
    }
    assert!(stack.is_empty());
    assert_eq!(current, [0; 4]);
    assert_eq!(draw_count, original.summary[0]);
    assert_eq!(copy_count, original.summary[1]);
    assert_eq!(resources.len() as u32, original.summary[4]);
    assert!(resources.values().all(|v| v.retired));
    // Retain resource contents until publication. CPU retirement is independently
    // verified by the native oracle; these GPU handles must survive encoding.
    let color = &resources[&2];
    assert_eq!(color.extent, case.extent);
    assert_eq!(color.kind, 0);
    let mut uniforms = [[0; 16]; 4];
    uniforms[0] = [
        2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
    ]
    .map(f32::to_bits);
    uniforms[1][..4].copy_from_slice(&[1f32.to_bits(); 4]);
    let vertices = [0f32, 0., 0., 0., 2., 0., 2., 0., 0., 2., 0., 2.].map(f32::to_bits);
    renderer.draw(
        context,
        target,
        None,
        Some(&color.view),
        false,
        case.extent,
        Key {
            program: 6,
            blend: 1,
            primitive: 1,
            depth: None,
            stencil: [7, 0, 0, 0, 0xffff],
        },
        &uniforms,
        &vertices,
        3,
    );
}
