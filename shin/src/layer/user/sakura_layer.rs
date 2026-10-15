use super::sakura_particles::{SakuraArithmetic, SakuraParticles};
use crate::{
    asset::{effects::EffectsAtlasBorrow, system::AssetServer},
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
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};

#[derive(Debug)]
pub struct SakuraLayerImpl {
    textures: EffectsAtlasBorrow,
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
    textures: EffectsAtlasBorrow,
    mode: i32,
    arithmetic: SakuraArithmetic,
}

impl PreparedSakuraLayer {
    pub(crate) fn construct(&self) -> SakuraLayer {
        let mut layer = SakuraLayer::from_inner_constructor(|| SakuraLayerImpl {
            textures: self.textures.clone(),
            particles: Mutex::new(
                SakuraParticles::new_with_arithmetic(
                    self.mode,
                    crate::engine_random::next_seed(),
                    self.arithmetic,
                )
                .unwrap(),
            ),
        });
        if self.arithmetic == SakuraArithmetic::Gerokasu2 {
            layer.set_properties(crate::layer::LayerProperties::with_policy(
                crate::layer::PropertyPolicy::Gerokasu2,
            ));
        }
        // Native25550 calls the derived update500 times during warm-up. Its
        // first base effect update78820 materializes the property owner and
        // consumes seven RNG draws. Default effect periods are zero, so the
        // warm-up leaves effect phases and property clocks at their defaults.
        let _ = layer
            .properties()
            .get_value(LayerProperty::RasterHorizontalTPeriod);
        layer
    }
}

impl SakuraLayer {
    #[cfg(test)]
    pub(crate) fn reference_constructor_state(&self) -> (usize, u32, u32) {
        (
            self.inner_ref().textures.reference_identity(),
            self.reference_dissolve_seed(),
            self.inner_ref().particles.lock().reference_seed(),
        )
    }
    pub(crate) async fn prepare_checked(
        server: &AssetServer,
        mode: i32,
    ) -> anyhow::Result<Option<PreparedSakuraLayer>> {
        if !(0..6).contains(&mode) {
            return Ok(None);
        }
        let textures = server.effects_atlas_for("Sakura").await?;
        Ok(Some(PreparedSakuraLayer {
            textures,
            mode,
            arithmetic: SakuraArithmetic::Legacy,
        }))
    }
    pub(crate) async fn prepare_checked_gerokasu2(
        server: &AssetServer,
        mode: i32,
    ) -> anyhow::Result<Option<PreparedSakuraLayer>> {
        Ok(Self::prepare_checked(server, mode)
            .await?
            .map(|mut prepared| {
                prepared.arithmetic = SakuraArithmetic::Gerokasu2;
                prepared
            }))
    }
    // Conditional owner/geometry references construct the native factory even
    // when supplied art lacks Sakura. Production preparation validates it above.
    #[cfg(test)]
    pub async fn load(server: &AssetServer, mode: i32) -> Option<Self> {
        Self::prepare(server, mode)
            .await
            .map(|prepared| prepared.construct())
    }

    #[cfg(test)]
    pub(crate) async fn prepare(server: &AssetServer, mode: i32) -> Option<PreparedSakuraLayer> {
        // Check before constructing either drawable base or derived RNG state.
        // Native25340 publishes a null pointer for unsigned modes>=6.
        if !(0..6).contains(&mode) {
            return None;
        }
        let textures = server
            .effects_atlas()
            .await
            .expect("Failed to load animation atlas");
        Some(PreparedSakuraLayer {
            textures,
            mode,
            arithmetic: SakuraArithmetic::Legacy,
        })
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
        let textures = self.textures.read();
        self.particles.lock().render_particles(
            transform.compute_final_transform(),
            |matrix, sprite| {
                let [left, top, right, bottom] = sprite.bounds.map(|v| v as f32);
                let [u0, v0, u1, v1] = sprite.uv;
                QuadVertices::new()
                    .with_box(left, top, right, bottom)
                    .with_tex_box(u0, v0, u1, v1)
                    .with_color(color)
                    .render_sprite(
                        pass,
                        builder,
                        textures.required("Sakura").as_source(),
                        matrix,
                    );
            },
        );
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod owner_tests;
