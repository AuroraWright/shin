use super::sakura_particles::SakuraParticles;
use crate::{
    asset::{
        system::AssetServer,
        texture_archive::{TextureArchive, TextureArchiveBuilder},
    },
    layer::{
        DrawableLayer, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use parking_lot::Mutex;
use shin_core::vm::command::types::LayerProperty;
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};
use std::sync::Arc;

// Keep the native name lookup optional until a visible tile actually draws.
// In the supplied atlas Sakura is absent. Native118180 dereferences an invalid
// iterator in that case; report the missing resource instead of invoking UB.
#[derive(Debug, Default)]
struct SakuraTextures {
    sakura: Option<GpuTexture>,
    retained_textures: Vec<GpuTexture>,
}
impl TextureArchive for SakuraTextures {
    type Builder = Self;
}
impl TextureArchiveBuilder for SakuraTextures {
    type Output = Self;
    fn new() -> Self {
        Self::default()
    }
    fn add_texture(&mut self, name: &str, texture: GpuTexture) {
        if name == "Sakura" {
            self.sakura = Some(texture);
        } else {
            self.retained_textures.push(texture);
        }
    }
    fn build(self) -> Self {
        self
    }
}
impl SakuraTextures {
    fn required(&self) -> &GpuTexture {
        self.sakura.as_ref().expect(
            "Animation layer requires texture 'Sakura' in /effects.txa; the supplied Umineko atlas does not contain it",
        )
    }
}

#[derive(Debug)]
pub struct SakuraLayerImpl {
    textures: Arc<SakuraTextures>,
    // Native rendering marks particles dead. Each layer/clone owns this state;
    // sharing only GPU textures keeps render clones' expiry independent.
    particles: Mutex<SakuraParticles>,
}
impl Clone for SakuraLayerImpl {
    fn clone(&self) -> Self {
        Self {
            textures: self.textures.clone(),
            particles: Mutex::new(self.particles.lock().clone()),
        }
    }
}
impl RenderClone for SakuraLayerImpl {
    fn render_clone(&self, _context: &mut RenderCloneCtx) -> Self {
        self.clone()
    }
}
pub type SakuraLayer = NewDrawableLayerWrapper<SakuraLayerImpl>;
pub(crate) struct PreparedSakuraLayer {
    textures: Arc<SakuraTextures>,
    mode: i32,
}

impl PreparedSakuraLayer {
    pub(crate) fn construct(&self) -> SakuraLayer {
        let layer = SakuraLayer::from_inner_constructor(|| SakuraLayerImpl {
            textures: self.textures.clone(),
            particles: Mutex::new(
                SakuraParticles::new(self.mode, crate::engine_random::next_seed()).unwrap(),
            ),
        });
        // Native25550 calls the derived update500 times during warm-up. Its
        // first base effect update78820 materializes the property owner and
        // consumes seven RNG draws. Default effect periods are zero, so the
        // warm-up leaves effect phases and property clocks at their defaults.
        let _ = layer.properties().get_value(LayerProperty::RasterHorizontalTPeriod);
        layer
    }
}

impl SakuraLayer {
    #[cfg(test)]
    pub(crate) fn reference_constructor_state(&self) -> (usize, u32, u32) {
        (
            Arc::as_ptr(&self.inner_ref().textures) as usize,
            self.reference_dissolve_seed(),
            self.inner_ref().particles.lock().reference_seed(),
        )
    }
    pub async fn load(server: &AssetServer, mode: i32) -> Option<Self> {
        Self::prepare(server, mode)
            .await
            .map(|prepared| prepared.construct())
    }

    pub(crate) async fn prepare(server: &AssetServer, mode: i32) -> Option<PreparedSakuraLayer> {
        // Check before constructing either drawable base or derived RNG state.
        // Native25340 publishes a null pointer for unsigned modes>=6.
        if !(0..6).contains(&mode) {
            return None;
        }
        let textures = server
            .load("/effects.txa")
            .await
            .expect("Failed to load animation atlas");
        Some(PreparedSakuraLayer { textures, mode })
    }
}
impl AdvUpdatable for SakuraLayerImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.particles
            .get_mut()
            .update(context.delta_ticks.as_f32(), context.are_animations_allowed);
    }
}
impl NewDrawableLayerNeedsSeparatePass for SakuraLayerImpl {}
impl NewDrawableLayerFastForward for SakuraLayerImpl {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayer for SakuraLayerImpl {
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind == PassKind::Opaque {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, false, false)
            .color_blend_type(ColorBlendType::from_regular_layer(drawable.blend_type));
        let color = drawable
            .fragment_shader
            .simplify(drawable.shader_param)
            .evaluate(drawable.color_multiplier, drawable.shader_param)
            .into_unorm();
        self.particles.lock().render_particles(
            transform.compute_final_transform(),
            |matrix, sprite| {
                let [left, top, right, bottom] = sprite.bounds.map(|v| v as f32);
                let [u0, v0, u1, v1] = sprite.uv;
                QuadVertices::new()
                    .with_box(left, top, right, bottom)
                    .with_tex_box(u0, v0, u1, v1)
                    .with_color(color)
                    .render_sprite(pass, builder, self.textures.required().as_source(), matrix);
            },
        );
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod owner_tests;
