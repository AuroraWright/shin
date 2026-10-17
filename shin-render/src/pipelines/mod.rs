mod conversions;

use std::collections::HashMap;

use enum_iterator::Sequence;
use rustc_hash::FxHashMap;
use shin_render_shader_types::texture::TextureTargetKind;
use shin_render_shaders::{Shader, ShaderContext, ShaderName, TypedRenderPipeline};
use wgpu::RenderPipeline;

use crate::{
    ColorBlendType, CullFace, DEPTH_STENCIL_FORMAT, DepthStencilPipelineState, DrawPrimitive,
    TEXTURE_FORMAT,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub struct PipelineStorageKey {
    pub target_kind: TextureTargetKind,
    pub draw_primitive: DrawPrimitive,
    pub cull_face: CullFace,
    pub blend_type: ColorBlendType,
    pub depth_stencil: Option<DepthStencilPipelineState>,
}

impl PipelineStorageKey {
    fn create_pipeline(
        &self,
        device: &wgpu::Device,
        screen_texture_format: wgpu::TextureFormat,
        context: &ShaderContext,
    ) -> wgpu::RenderPipeline {
        let &PipelineStorageKey {
            target_kind,
            draw_primitive,
            cull_face,
            blend_type,
            depth_stencil,
        } = self;

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("Pipeline for {:?}", self)),
            layout: Some(&context.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &context.shader_module,
                entry_point: Some(context.shader_descriptor.vertex_entry),
                compilation_options: Default::default(),
                buffers: &[Some(context.shader_descriptor.vertex_buffer_layout.clone())],
            },
            primitive: wgpu::PrimitiveState {
                topology: draw_primitive.into(),
                // VertexSource binds 16-bit indices. WGPU requires the strip
                // pipeline to declare their format for indexed draws, including
                // the dialogue window; non-indexed strips can use it as well.
                strip_index_format: match draw_primitive {
                    DrawPrimitive::Triangles => None,
                    DrawPrimitive::TrianglesStrip => Some(wgpu::IndexFormat::Uint16),
                },
                // TODO: is this correct?
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: cull_face.into(),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: depth_stencil.map(|DepthStencilPipelineState { depth, stencil }| {
                wgpu::DepthStencilState {
                    format: DEPTH_STENCIL_FORMAT,
                    depth_write_enabled: Some(depth.write_enable),
                    depth_compare: Some(depth.function.into()),
                    stencil: wgpu::StencilState {
                        front: stencil.into(),
                        back: stencil.into(),
                        read_mask: stencil.stencil_read_mask.into(),
                        write_mask: stencil.stencil_write_mask.into(),
                    },
                    bias: wgpu::DepthBiasState::default(),
                }
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &context.shader_module,
                entry_point: Some(context.shader_descriptor.fragment_entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: match target_kind {
                        TextureTargetKind::Screen => screen_texture_format,
                        TextureTargetKind::RenderTexture => TEXTURE_FORMAT,
                    },
                    blend: Some(blend_type.into()),
                    write_mask: blend_type.into(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

struct ShaderContextStorage {
    shaders: FxHashMap<ShaderName, ShaderContext>,
}

impl ShaderContextStorage {
    pub fn new(device: &wgpu::Device) -> Self {
        let mut shaders = HashMap::default();
        for shader in enum_iterator::all::<ShaderName>() {
            let context = shader.descriptor().create_shader_context(device);
            shaders.insert(shader, context);
        }
        Self { shaders }
    }

    pub fn get(&self, shader: ShaderName) -> &ShaderContext {
        self.shaders.get(&shader).unwrap()
    }
}

pub struct PipelineStorage {
    device: wgpu::Device,
    screen_texture_format: wgpu::TextureFormat,
    shader_context: ShaderContextStorage,
    pipelines: FxHashMap<(ShaderName, PipelineStorageKey), wgpu::RenderPipeline>,
}

impl PipelineStorage {
    pub fn new(device: wgpu::Device, screen_texture_format: wgpu::TextureFormat) -> Self {
        let shader_context = ShaderContextStorage::new(&device);
        Self::warm_up(device.clone(), screen_texture_format);
        Self {
            device,
            screen_texture_format,
            shader_context,
            pipelines: FxHashMap::default(),
        }
    }

    /// Compile every shader once in the background.
    ///
    /// Pipelines are created on first use, and the first pipeline of each
    /// shader compiles it, which can take ~100ms when the driver's shader
    /// cache is cold. Later pipelines of the same shader reuse the driver's
    /// compiled code, so this keeps screens from stalling the first time they
    /// draw something new (e.g. mid-animation).
    fn warm_up(device: wgpu::Device, screen_texture_format: wgpu::TextureFormat) {
        // Driver caches are process-wide; later storages share the result.
        static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if cfg!(target_arch = "wasm32") || STARTED.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let key = PipelineStorageKey {
            target_kind: TextureTargetKind::Screen,
            draw_primitive: DrawPrimitive::TrianglesStrip,
            cull_face: CullFace::None,
            blend_type: ColorBlendType::Opaque,
            depth_stencil: None,
        };
        let spawned = std::thread::Builder::new()
            .name("shader warm-up".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                for shader in enum_iterator::all::<ShaderName>() {
                    let context = shader.descriptor().create_shader_context(&device);
                    key.create_pipeline(&device, screen_texture_format, &context);
                }
                tracing::debug!("Compiled all shaders in {:?}", started.elapsed());
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "Could not start shader warm-up");
        }
    }

    fn get_untyped(
        &mut self,
        key: PipelineStorageKey,
        name: ShaderName,
    ) -> (&ShaderContext, &RenderPipeline) {
        let context = self.shader_context.get(name);
        let pipeline = self.pipelines.entry((name, key)).or_insert_with(|| {
            key.create_pipeline(&self.device, self.screen_texture_format, context)
        });

        (context, pipeline)
    }

    // it is unfortunate that we have to take a &mut self here
    // this can lead to difficulties with borrowing
    // can introduce interior mutability if we need to
    pub fn get<S: Shader>(&mut self, key: PipelineStorageKey) -> TypedRenderPipeline<'_, S> {
        let (context, pipeline) = self.get_untyped(key, S::NAME);

        TypedRenderPipeline::new(context, pipeline)
    }
}

#[cfg(test)]
mod test {
    use enum_iterator::cardinality;

    use crate::pipelines::PipelineStorageKey;

    #[test]
    fn pipeline_storage_key_cardinality() {
        // currently 166723584
        // this is a big too much to create all of them ahead of time.
        // I think we should create a list of ones that should be pre-created (as an optimization) and then create the rest on demand.
        // This can lead to stuter, but what can you do?
        dbg!(cardinality::<PipelineStorageKey>());
    }
}
