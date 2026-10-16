//! NEW independent retained ADV-cache event replay. Transport is copied from
//! the frozen original Screen helper into this NEW file; native modal23 adds
//! four uniform bindings and one texture. No production Scene/cache runs.
use crate::render::native_gpu_state::{compare, native_blend, stencil_operation};
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use wgpu::util::DeviceExt;

use crate::{
    adv::higurashi::temporal::adv_cache_pixels::{Corpus, Event, Observation},
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
            label: Some("Original full Screen resource"),
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
            6..=11 => {
                let operation = [6, 9, 7, 8, 10, 11]
                    .iter()
                    .position(|&p| p == program)
                    .unwrap();
                (
                    "SHIN_HIGURASHI_PRIMARY_BLOCK_SHADERS",
                    format!("native-primary-block-{operation}"),
                    3,
                    1,
                )
            }
            21 => (
                "SHIN_HIGURASHI_LIVE_AFTERIMAGE_SHADERS",
                "native-live-afterimage-21".to_owned(),
                2,
                2,
            ),
            23 => (
                "SHIN_HIGURASHI_EFFECTS_SHADERS",
                "native-save-effect-23".to_owned(),
                4,
                1,
            ),
            value => panic!("Unreviewed original ADV-cache program{value}"),
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
                label: Some("Original full Screen GLSL"),
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
            label: Some("Original full Screen packet pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: if key.program == 0 { 12 } else { 16 },
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
                        label: Some("Original full Screen packet uniform bytes"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original full Screen packet vertex bytes"),
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
                multiview_mask: None,
                label: Some("Original full Screen packet"),
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
        assert!(flags <= 7 && z.is_finite());
        if flags == 0 {
            return;
        }
        // Actuala8fe0 uses literal program0, viewport-sized triangle and the raw
        // clip-Z argument. Its literal shader alone performs GL-depth bridging.
        let mut uniforms = [[0u32; 16]; 4];
        uniforms[0][..4].copy_from_slice(
            &[
                ((argb >> 16) & 255) as f32 / 255.,
                ((argb >> 8) & 255) as f32 / 255.,
                (argb & 255) as f32 / 255.,
                (argb >> 24) as f32 / 255.,
            ]
            .map(f32::to_bits),
        );
        let vertices = [-1f32, 1., z, 3., 1., z, -1., -3., z].map(f32::to_bits);
        self.draw(
            context,
            &color.view,
            depth.map(|v| &v.view),
            &[],
            viewport,
            Key {
                program: 0,
                blend: flags & 1,
                primitive: 0,
                depth: depth.map(|_| [7, u32::from(flags & 2 != 0)]),
                stencil: [
                    7,
                    0,
                    0,
                    if flags & 4 != 0 { 2 } else { 0 },
                    0xffff | ((stencil & 255) << 16),
                ],
            },
            &uniforms,
            &vertices,
            3,
        );
    }
}

/// Replays the complete original sequence prefix, preserving every resource,
/// copy and draw before the named observation. Logical CPU retirement is checked
/// separately; GPU handles remain alive until the encoded observation is read.
/// Publishing uses literal program6 white/opaque at exactly the observed extent.
/// This helper never invokes production history or framebuffer implementation.
pub(crate) fn replay(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    corpus: &Corpus,
    observation: &Observation,
) {
    let sequence = corpus.profile_for(observation);
    let selected_resource = corpus.resource_for(observation);
    let mut renderer = Renderer::new(context.device);
    let mut resources = HashMap::<u32, Resource>::new();
    let mut current = [0u32; 4];
    let mut stack = vec![];
    let mut depth_state = [7, 0];
    let mut filters = HashMap::<u32, [u32; 2]>::new();
    let mut pending_filter = None;
    for row in &corpus.rows[sequence.first_row..=observation.row] {
        for event in &row.events {
            match event {
                Event::Resource(v) => {
                    assert!(
                        resources
                            .insert(v[0], Resource::new(context.device, v[1], v[2], v[3]))
                            .is_none()
                    )
                }
                Event::InitialClear(v) => {
                    let color = &resources[&v[0]];
                    assert_eq!(color.kind, 0);
                    renderer.clear(context, color, None, color.extent, 1, v[1], 0, 1.)
                }
                Event::Target(v) => {
                    stack.push(current);
                    current = *v;
                    assert_eq!(resources[&v[0]].kind, 0);
                    if v[1] != 0 {
                        assert_eq!(resources[&v[1]].kind, 1)
                    }
                }
                Event::Viewport(v) => {
                    current[2] = v[0];
                    current[3] = v[1]
                }
                Event::Restore(v) => {
                    assert_eq!(stack.pop().unwrap(), *v);
                    current = *v
                }
                Event::Clear(v) => {
                    assert_eq!(current[0], v[0]);
                    renderer.clear(
                        context,
                        &resources[&v[0]],
                        (current[1] != 0).then(|| &resources[&current[1]]),
                        (current[2], current[3]),
                        v[1],
                        v[2],
                        v[3],
                        f32::from_bits(v[4]),
                    )
                }
                Event::Depth(v) => {
                    assert!(v[0] <= 7 && v[1] <= 1);
                    depth_state = *v
                }
                Event::SamplerFilter(v) => {
                    assert_eq!(*v, [5, 1]);
                    pending_filter = Some(*v)
                }
                Event::SamplerWrap(v) => {
                    assert_eq!(*v, [7, 7, 7]);
                    let id = 101 + filters.len() as u32;
                    assert!(filters.insert(id, pending_filter.take().unwrap()).is_none())
                }
                Event::Bind(v, descriptor) => {
                    assert!(v[0] < 3);
                    if let Some(words) = descriptor {
                        assert_eq!(*words, [1, 1, 1, 0, 0])
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
                }
                Event::Draw(d) => {
                    let d = &**d;
                    assert_eq!(current, [d[5], d[6], d[7], d[8]]);
                    assert!(d[11] % 4 == 0 && d[11] <= 96);
                    let color = &resources[&d[5]];
                    assert_eq!(color.kind, 0);
                    let depth = (d[6] != 0).then(|| &resources[&d[6]]);
                    let texture_count = match d[3] {
                        0 | 1 => 0,
                        6..=11 => 1,
                        21 => 2,
                        23 => 1,
                        value => panic!("Unreviewed original full Screen draw{value}"),
                    };
                    let sources = (0..texture_count)
                        .map(|i| {
                            let r = &resources[&d[13 + i]];
                            assert_eq!(r.kind, 0);
                            assert!(!r.retired);
                            &r.view
                        })
                        .collect::<Vec<_>>();
                    if !sources.is_empty() {
                        assert_eq!(filters[&d[12]], [5, 1])
                    }
                    let uniforms =
                        std::array::from_fn(|i| d[25 + i * 16..41 + i * 16].try_into().unwrap());
                    renderer.draw(
                        context,
                        &color.view,
                        depth.map(|v| &v.view),
                        &sources,
                        (d[7], d[8]),
                        Key {
                            program: d[3],
                            blend: d[4],
                            primitive: d[9],
                            depth: depth.map(|_| depth_state),
                            stencil: d[16..21].try_into().unwrap(),
                        },
                        &uniforms,
                        &d[89..89 + d[11] as usize / 4],
                        d[10],
                    );
                }
                Event::Drop(v) => {
                    if v[1] == 0 {
                        let resource = resources.get_mut(&v[0]).unwrap();
                        assert!(!resource.retired);
                        resource.retired = true;
                        // Original CPU zero-ref retirement closes this ID. Only the
                        // selected final output must remain available to publish.
                        // Encoded commands retain their own WGPU references; this
                        // map cannot keep every old1080p output/copy alive as well.
                        if v[0] != selected_resource {
                            resources.remove(&v[0]).unwrap();
                        }
                    }
                }
                Event::Capture(v) => {
                    assert_eq!(resources[&v[2]].extent, (v[4], v[5]));
                    assert_eq!(resources[&v[2]].extent, resources[&v[3]].extent);
                    if v[0] != 2 {
                        assert_eq!([v[4], v[5]], sequence.constructor_extent);
                    } else {
                        assert_eq!([v[4], v[5]], sequence.constructor_extent.map(|n| n / 2));
                    }
                }
                Event::Keep(_)
                | Event::Stencil(_)
                | Event::Flush(_)
                | Event::Render(_)
                | Event::Constructor(_)
                | Event::Leaf(_)
                | Event::Update(_)
                | Event::Output(_)
                | Event::VirtualMethods(_)
                | Event::Phase(_)
                | Event::PhaseEnd(_)
                | Event::Window(_)
                | Event::MessageBuffer(_)
                | Event::Control(_)
                | Event::Audio(_)
                | Event::Ui(_)
                | Event::Navigation(_) => {}
            }
        }
    }
    assert!(stack.is_empty());
    assert_eq!(current, [0; 4]);
    let source = &resources[&selected_resource];
    assert_eq!(
        source.extent,
        (observation.extent[0], observation.extent[1])
    );
    assert_eq!(source.kind, 0);
    let mut uniforms = [[0u32; 16]; 4];
    uniforms[0] = [
        2f32, 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., 0., 1.,
    ]
    .map(f32::to_bits);
    uniforms[1][..4].copy_from_slice(&[1f32.to_bits(); 4]);
    let vertices = [0f32, 0., 0., 0., 2., 0., 2., 0., 0., 2., 0., 2.].map(f32::to_bits);
    renderer.draw(
        context,
        target,
        None,
        &[&source.view],
        source.extent,
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
