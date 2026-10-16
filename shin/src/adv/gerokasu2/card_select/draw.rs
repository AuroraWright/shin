use super::CardSelectModel;
use crate::{
    asset::texture_archive::TextureArchive,
    layer::{
        DrawablePropertyPolicy, NewDrawableLayer, NewDrawableLayerFastForward,
        NewDrawableLayerNeedsSeparatePass, NewDrawableLayerWrapper,
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Mat4, Vec3, Vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, gpu_texture::GpuTexture,
    quad_vertices::QuadVertices, render_pass::RenderPass, shaders::types::RenderClone,
};
use std::sync::Arc;
#[derive(TextureArchive)]
pub struct CardTextures {
    #[txa(name = "caption")]
    caption: GpuTexture,
    #[txa(name = "card_back")]
    back: GpuTexture,
    #[txa(name = "card_cat")]
    cat: GpuTexture,
    #[txa(name = "card_hare")]
    hare: GpuTexture,
    #[txa(name = "card_hat")]
    hat: GpuTexture,
    #[txa(name = "card_queen")]
    queen: GpuTexture,
}
impl CardTextures {
    fn face(&self, id: u32) -> &GpuTexture {
        match id {
            0 => &self.cat,
            1 => &self.hare,
            2 => &self.hat,
            3 => &self.queen,
            _ => unreachable!(),
        }
    }
}
pub struct CardRenderer {
    pub model: Arc<parking_lot::Mutex<CardSelectModel>>,
    textures: Arc<CardTextures>,
}
impl RenderClone for CardRenderer {
    fn render_clone(&self, _: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        Self {
            model: Arc::new(parking_lot::Mutex::new(
                self.model.lock().render_clone_model(),
            )),
            textures: self.textures.clone(),
        }
    }
}
pub type CardLayer = NewDrawableLayerWrapper<CardRenderer>;
pub fn construct(textures: Arc<CardTextures>, mask: i32, seed: u32) -> anyhow::Result<CardLayer> {
    let model = CardSelectModel::new(mask, seed)?;
    Ok(NewDrawableLayerWrapper::from_plain_layer_constructor(
        || CardRenderer {
            model: Arc::new(parking_lot::Mutex::new(model)),
            textures,
        },
    ))
}
pub(super) fn camera() -> Mat4 {
    glam::camera::rh::proj::directx::perspective(
        f32::from_bits(0x3f7d821c),
        f32::from_bits(0x3fe38e39),
        10.0,
        10000.0,
    ) * Mat4::from_translation(Vec3::new(0.0, 0.0, -1000.0))
        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
}
fn card_local(card: &super::Card) -> Mat4 {
    let v = |i: usize| card.tweens[i].value();
    Mat4::from_translation(Vec3::new(v(2) - 960.0, v(3) - 540.0, 0.0))
        * Mat4::from_rotation_y(v(4) * f32::from_bits(0x40c90fd0))
        * Mat4::from_scale(Vec3::new(v(5), v(5), 1.0))
}
impl AdvUpdatable for CardRenderer {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.model.lock().update(ctx.delta_ticks);
    }
}
impl NewDrawableLayerFastForward for CardRenderer {
    fn fast_forward(&mut self) {
        self.model.lock().fast_forward();
    }
}
impl NewDrawableLayerNeedsSeparatePass for CardRenderer {}
impl NewDrawableLayer for CardRenderer {
    fn property_policy(&self) -> DrawablePropertyPolicy {
        DrawablePropertyPolicy::TransformOnly
    }
    fn renders_pass(&self, kind: PassKind) -> bool {
        kind == PassKind::Transparent
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        _: &DrawableParams,
        _: &DrawableClipParams,
        stencil: u8,
        kind: PassKind,
    ) {
        if kind != PassKind::Transparent {
            return;
        }
        let m = self.model.lock();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil, true, false)
            .color_blend_type(ColorBlendType::Layer1);
        let draw = |pass: &mut RenderPass,
                    texture: &GpuTexture,
                    rect: Vec4,
                    matrix: Mat4,
                    alpha: f32,
                    flash: f32,
                    press: f32| {
            if alpha <= 0.0 {
                return;
            }
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.z, rect.w)
                .with_color(FloatColor4::WHITE.with_alpha(alpha).into_unorm())
                .render_button(
                    pass,
                    builder,
                    texture.as_source(),
                    matrix,
                    Vec4::new(1.0, 1.0, 1.0, flash),
                    Vec4::new(1.0, 1.0, 1.0, press),
                );
        };
        let parent = transform.compute_total_translation();
        if !m.automatic {
            draw(
                pass,
                &self.textures.caption,
                Vec4::new(
                    800.0,
                    260.0 + m.caption_y.value(),
                    1760.0,
                    388.0 + m.caption_y.value(),
                ),
                camera() * parent * Mat4::from_translation(Vec3::new(-960.0, -540.0, 0.0)),
                m.caption_alpha.value(),
                0.0,
                0.0,
            );
        }
        for card in &m.cards {
            let turn = card.tweens[4].value();
            let texture = if (turn * f32::from_bits(0x40c90fd0)).cos() >= 0.0 {
                self.textures.face(card.face)
            } else {
                &self.textures.back
            };
            draw(
                pass,
                texture,
                Vec4::new(-138.0, -196.0, 138.0, 196.0),
                camera() * parent * card_local(card),
                card.tweens[6].value(),
                card.tweens[0].value() * 0.5,
                card.tweens[1].value(),
            );
        }
    }
}
