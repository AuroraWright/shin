#[cfg(test)]
use std::sync::Arc;
use super::focus_line_model::FocusLineRays;
use crate::{
    asset::effects::EffectsAtlasBorrow,
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Mat4, Vec2};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, quad_vertices::QuadVertices,
    render_pass::RenderPass, shaders::types::RenderClone,
};
#[cfg(test)]
use crate::asset::system::AssetServer;

#[derive(Clone, RenderClone)]
pub struct FocusLineLayerImpl {
    textures: EffectsAtlasBorrow,
    offset: Vec2,
    rays: FocusModel,
}
#[derive(Clone, Debug)]
enum FocusModel {
    Legacy(FocusLineRays),
    Ger(crate::adv::gerokasu2::procedural_particles::GerFocusRays),
}
impl FocusModel {
    fn rays(&self) -> &[super::FocusRay] {
        match self {
            Self::Legacy(m) => &m.rays,
            Self::Ger(m) => &m.rays,
        }
    }
    fn update(&mut self, delta: shin_core::time::Ticks) {
        match self {
            Self::Legacy(m) => m.update(delta),
            Self::Ger(m) => m.update(delta),
        }
    }
    #[cfg(test)]
    fn reference_seed(&self) -> u32 {
        match self {
            Self::Legacy(m) => m.reference_seed(),
            Self::Ger(m) => m.reference_seed(),
        }
    }
    #[cfg(test)]
    fn legacy(&self) -> &FocusLineRays {
        let Self::Legacy(m) = self else {
            panic!("Legacy focus fixture")
        };
        m
    }
    #[cfg(test)]
    fn legacy_mut(&mut self) -> &mut FocusLineRays {
        let Self::Legacy(m) = self else {
            panic!("Legacy focus fixture")
        };
        m
    }
}

impl std::fmt::Debug for FocusLineLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusLineLayer")
            .field("offset", &self.offset)
            .field("rays", &self.rays)
            .finish()
    }
}

pub type FocusLineLayer = NewDrawableLayerWrapper<FocusLineLayerImpl>;
impl FocusLineLayer {
    #[cfg(test)]
    pub(super) fn reference_owner_rays(&self) -> &FocusLineRays {
        self.inner_ref().rays.legacy()
    }
    #[cfg(test)]
    pub(super) fn reference_owner_rays_mut(&mut self) -> &mut FocusLineRays {
        self.inner_mut().rays.legacy_mut()
    }
    #[cfg(test)]
    pub(super) fn reference_owner_offset(&self) -> Vec2 {
        self.inner_ref().offset
    }
    #[cfg(test)]
    pub(crate) fn reference_constructor_state(&self) -> (usize, u32, u32) {
        (
            self.inner_ref().textures.reference_identity(),
            self.reference_dissolve_seed(),
            self.inner_ref().rays.reference_seed(),
        )
    }
    #[cfg(test)]
    pub async fn load(server: &AssetServer, x: i32, y: i32) -> Self {
        let textures = server
            .effects_atlas_for("focusline")
            .await
            .expect("Failed to load focus line texture");
        Self::new(textures, x, y)
    }

    pub(crate) fn new(textures: EffectsAtlasBorrow, x: i32, y: i32) -> Self {
        Self::from_inner_constructor(|| FocusLineLayerImpl {
            textures,
            offset: Vec2::new(x as f32, y as f32),
            rays: FocusModel::Legacy(FocusLineRays::new(crate::engine_random::next_seed())),
        })
    }
    pub(crate) fn new_gerokasu2(textures: EffectsAtlasBorrow, offset: Vec2) -> Self {
        Self::from_inner_constructor(|| FocusLineLayerImpl {
            textures,
            offset,
            rays: FocusModel::Ger(
                crate::adv::gerokasu2::procedural_particles::GerFocusRays::new(
                    crate::engine_random::next_seed(),
                ),
            ),
        })
    }
}
impl AdvUpdatable for FocusLineLayerImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        // Native67544 gates base properties, then6769c advances every ray
        // without a separate animation-permission or property27 gate.
        self.rays.update(context.delta_ticks);
    }
}
impl NewDrawableLayerNeedsSeparatePass for FocusLineLayerImpl {}
impl NewDrawableLayerFastForward for FocusLineLayerImpl {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayer for FocusLineLayerImpl {
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
        let translation = Mat4::from_translation(self.offset.extend(0.0));
        let ger = matches!(self.rays, FocusModel::Ger(_));
        let matrix = if ger {
            crate::layer::render_params::multiply_transforms(
                transform.compute_final_transform(),
                translation,
            )
        } else {
            transform.compute_final_transform() * translation
        };
        let textures = self.textures.read();
        for ray in self.rays.rays() {
            let color = drawable
                .color_multiplier
                .with_alpha(drawable.color_multiplier.a * ray.alpha.value());
            let color = drawable
                .fragment_shader
                .simplify(drawable.shader_param)
                .evaluate(color, drawable.shader_param)
                .into_unorm();
            // Native6777c..677c4: x150..2070,y-15..15, reversed horizontal UV.
            QuadVertices::new()
                .with_box(150.0, -15.0, 2070.0, 15.0)
                .with_tex_box(1.0, 0.0, 0.0, 1.0)
                .with_color(color)
                .render_sprite(
                    pass,
                    builder,
                    textures.required("focusline").as_source(),
                    if ger {
                        crate::layer::render_params::multiply_transforms(
                            matrix,
                            Mat4::from_rotation_z(ray.angle),
                        )
                    } else {
                        matrix * Mat4::from_rotation_z(ray.angle)
                    },
                );
        }
    }
}

#[cfg(test)]
mod tests;
