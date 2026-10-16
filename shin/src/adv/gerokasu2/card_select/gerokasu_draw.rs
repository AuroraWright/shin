//! Gerokasu CardSelect render 46C60. Layer space is centred, y down; the
//! G2 card camera (`draw::camera`) maps it 1:1 to pixels at z=0.
use std::sync::Arc;

use anyhow::Result;
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, gpu_texture::GpuTexture,
    quad_vertices::QuadVertices, render_pass::RenderPass, shaders::types::RenderClone,
};

use super::gerokasu::{GerokasuCardModel, Message, SLOT_HEROINE, SLOT_X};
use crate::{
    adv::gerokasu2::generic_text::Text,
    asset::{
        font::gerokasu2::GpuGerokasu2Font, system::AssetServer, texture_archive::TextureArchive,
    },
    layer::{
        DrawablePropertyPolicy, NewDrawableLayer, NewDrawableLayerFastForward,
        NewDrawableLayerNeedsSeparatePass, NewDrawableLayerWrapper,
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(TextureArchive)]
pub struct GerokasuCardTextures {
    #[txa(name = "bup_fu")]
    bup_fu: GpuTexture,
    #[txa(name = "bup_ka")]
    bup_ka: GpuTexture,
    #[txa(name = "bup_mi")]
    bup_mi: GpuTexture,
    #[txa(name = "bup_do")]
    bup_do: GpuTexture,
    #[txa(name = "cancel")]
    cancel: GpuTexture,
    #[txa(name = "card_back")]
    back: GpuTexture,
    #[txa(name = "card_focus")]
    focus: GpuTexture,
    #[txa(name = "card_prisoner")]
    prisoner: GpuTexture,
    #[txa(name = "card_pierrot")]
    pierrot: GpuTexture,
    #[txa(name = "card_judge1")]
    judge1: GpuTexture,
    #[txa(name = "card_judge2")]
    judge2: GpuTexture,
    #[txa(name = "cutin_bg")]
    cutin_bg: GpuTexture,
    #[txa(name = "cutin_fu")]
    cutin_fu: GpuTexture,
    #[txa(name = "cutin_ka")]
    cutin_ka: GpuTexture,
    #[txa(name = "cutin_mi")]
    cutin_mi: GpuTexture,
    #[txa(name = "cutin_do")]
    cutin_do: GpuTexture,
    #[txa(name = "cutin_fire")]
    cutin_fire: GpuTexture,
    #[txa(name = "effect")]
    effect: GpuTexture,
    #[txa(name = "icon_prisoner")]
    icon_prisoner: GpuTexture,
    #[txa(name = "icon_pierrot")]
    icon_pierrot: GpuTexture,
}
impl GerokasuCardTextures {
    fn face(&self, kind: u8) -> &GpuTexture {
        match kind {
            0 => &self.prisoner,
            1 => &self.pierrot,
            2 => &self.judge1,
            _ => &self.judge2,
        }
    }
    fn bust(&self, heroine: usize) -> &GpuTexture {
        [&self.bup_fu, &self.bup_ka, &self.bup_mi, &self.bup_do][heroine]
    }
    fn cutin(&self, heroine: usize) -> &GpuTexture {
        [&self.cutin_fu, &self.cutin_ka, &self.cutin_mi, &self.cutin_do][heroine]
    }
}

/// 17C510 bust table by heroine: w, h, ax, ay, bx, by.
const BUSTS: [[f32; 6]; 4] = [
    [550.0, 1336.0, -20.0, 520.0, 0.0, -30.0],
    [554.0, 1316.0, -30.0, 520.0, 0.0, -20.0],
    [465.0, 1252.0, -20.0, 560.0, 0.0, -20.0],
    [545.0, 1305.0, 0.0, 540.0, 0.0, -25.0],
];

/// Laid-out rejection messages, indexed by `Message::index`.
pub struct CardMessages {
    texts: Vec<(Text, Vec2)>,
}

/// Prepared assets for one CardSelect (archive chosen by progress).
pub struct GerokasuCardResources {
    pub textures: Arc<GerokasuCardTextures>,
    pub messages: Arc<CardMessages>,
}
impl GerokasuCardResources {
    pub async fn load(assets: &AssetServer, mask: i32) -> Result<Self> {
        let progress = (mask as u32 & 0xfff).count_ones();
        let font_path = crate::adv::gerokasu2::message_ui::medium_font_path(assets.context().game);
        let (textures, font) = futures::try_join!(
            assets.load::<GerokasuCardTextures, _>(format!("/cardselect{progress:02}.txa")),
            assets.load::<GpuGerokasu2Font, _>(font_path),
        )?;
        let mut texts = Vec::new();
        for message in Message::ALL {
            let (commands, lines, size) =
                shin_core::layout::gerokasu2::card_message_layouter(font.metrics())
                    .parse(&message.text());
            let text = Text::prepare(
                &assets.context().wgpu_device,
                &assets.context().wgpu_queue,
                &font,
                commands,
                lines,
                48.0,
            )?;
            texts.push((text, size + Vec2::splat(6.0)));
        }
        Ok(Self {
            textures,
            messages: Arc::new(CardMessages { texts }),
        })
    }
}

pub struct GerokasuCardRenderer {
    pub model: Arc<parking_lot::Mutex<GerokasuCardModel>>,
    textures: Arc<GerokasuCardTextures>,
    messages: Arc<CardMessages>,
}
impl RenderClone for GerokasuCardRenderer {
    fn render_clone(&self, _: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        Self {
            model: Arc::new(parking_lot::Mutex::new(
                self.model.lock().render_clone_model(),
            )),
            textures: self.textures.clone(),
            messages: self.messages.clone(),
        }
    }
}
pub type GerokasuCardLayer = NewDrawableLayerWrapper<GerokasuCardRenderer>;
pub fn construct(resources: &GerokasuCardResources, mask: i32, seed: u32) -> GerokasuCardLayer {
    let model = GerokasuCardModel::new(mask, seed);
    let textures = resources.textures.clone();
    let messages = resources.messages.clone();
    NewDrawableLayerWrapper::from_plain_layer_constructor(|| GerokasuCardRenderer {
        model: Arc::new(parking_lot::Mutex::new(model)),
        textures,
        messages,
    })
}
impl AdvUpdatable for GerokasuCardRenderer {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.model.lock().update(ctx.delta_ticks);
    }
}
impl NewDrawableLayerFastForward for GerokasuCardRenderer {
    fn fast_forward(&mut self) {
        self.model.lock().fast_forward();
    }
}
impl NewDrawableLayerNeedsSeparatePass for GerokasuCardRenderer {}

fn translate(x: f32, y: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, 0.0))
}
fn pulse(elapsed: f32, period: f32) -> f32 {
    0.6 + 0.2 * (std::f32::consts::PI * elapsed / period).sin()
}
fn wave(elapsed: f32, period: f32) -> f32 {
    0.5 + 0.5 * (std::f32::consts::PI * elapsed / period).sin()
}
/// Hand cards: `S(sc,-sc,1)·RotY(π·t4)·RotX(π·t3)·T(0,0,-1000)·Persp·
/// T(X/960,-Y/540,0)` (row-vector order), each card with its own centre.
fn hand_matrix(card: &super::gerokasu::Card) -> (Mat4, bool) {
    let pi = std::f32::consts::PI;
    let scale = card.value(2);
    let rotation =
        Mat4::from_rotation_x(pi * card.value(3)) * Mat4::from_rotation_y(pi * card.value(4));
    let perspective = glam::camera::rh::proj::directx::perspective(
        f32::from_bits(0x3f7d821c),
        f32::from_bits(0x3fe38e39),
        1.0,
        10000.0,
    );
    let matrix = Mat4::from_translation(Vec3::new(card.value(0) / 960.0, -card.value(1) / 540.0, 0.0))
        * perspective
        * Mat4::from_translation(Vec3::new(0.0, 0.0, -1000.0))
        * rotation
        * Mat4::from_scale(Vec3::new(scale, -scale, 1.0));
    let facing = rotation.transform_vector3(Vec3::Z).z >= 0.0;
    (matrix, facing)
}

impl NewDrawableLayer for GerokasuCardRenderer {
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
        let t = &*self.textures;
        let builder = |blend| {
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil, true, false)
                .color_blend_type(blend)
        };
        let normal = builder(ColorBlendType::Layer1);
        // Native blend mode 4 (1 opaque, 2 alpha, 3 add, 4 reverse subtract).
        let subtract = builder(ColorBlendType::HigurashiLayer3);
        let draw = |pass: &mut RenderPass,
                    builder: RenderRequestBuilder,
                    texture: &GpuTexture,
                    rect: Vec4,
                    uv: Vec4,
                    matrix: Mat4,
                    color: FloatColor4,
                    flash: f32| {
            if color.a <= 0.0 {
                return;
            }
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.z, rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(color.into_unorm())
                .render_button(
                    pass,
                    builder,
                    texture.as_source(),
                    matrix,
                    Vec4::new(1.0, 1.0, 1.0, flash),
                    Vec4::ZERO,
                );
        };
        let fill = |pass: &mut RenderPass, rect: Vec4, matrix: Mat4, color: FloatColor4| {
            if color.a <= 0.0 {
                return;
            }
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.z, rect.w)
                .with_color(color.into_unorm())
                .fill(pass, normal, matrix);
        };
        let full = Vec4::new(0.0, 0.0, 1.0, 1.0);
        let white = |a: f32| FloatColor4::WHITE.with_alpha(a);
        let camera = super::draw::camera();
        let layer = camera * transform.compute_total_translation();
        let screen = camera * translate(-960.0, -540.0);
        let elapsed = m.elapsed;
        let selected_kind = usize::try_from(m.selected)
            .ok()
            .and_then(|i| m.hand.get(i))
            .map(|c| c.kind);

        // 1. Busts.
        for slot in 0..4 {
            let h = SLOT_HEROINE[slot];
            let [w, bh, ax, ay, bx, by] = BUSTS[h];
            let matrix = layer
                * translate(
                    SLOT_X[slot] + bx + 0.65 * ax - w / 2.0,
                    by - 100.0 + 0.65 * ay - bh / 2.0,
                );
            let rect = Vec4::new(0.0, 0.0, w, bh);
            let dim = m.phase == 3
                && !m.placed.iter().any(|p| p.slot == slot)
                && selected_kind.is_some_and(|k| m.rejection(k, slot).is_some());
            let color = if dim {
                FloatColor4::from_rgba(0.5, 0.5, 0.5, 1.0)
            } else {
                white(1.0)
            };
            draw(
                pass,
                normal,
                t.bust(h),
                rect,
                full,
                matrix,
                color,
                m.slot_flash[slot].value(),
            );
            if m.phase == 3 && m.slot == slot as i32 {
                draw(
                    pass,
                    normal,
                    t.bust(h),
                    rect,
                    full,
                    matrix,
                    white(pulse(elapsed, 60.0)),
                    1.0,
                );
            }
        }
        // 2. Placed cards, flat.
        for p in &m.placed {
            let c = &p.card;
            let s = c.value(2);
            draw(
                pass,
                normal,
                t.face(c.kind),
                Vec4::new(-138.0, -196.0, 138.0, 196.0),
                full,
                layer * translate(c.value(0), c.value(1)) * Mat4::from_scale(Vec3::new(s, s, 1.0)),
                white(c.value(5)),
                c.value(6),
            );
        }
        // 3. Effect, always drawn; success raises its top edge.
        let k = wave(elapsed, 120.0);
        let e = m.effect.value();
        let screen_rect = Vec4::new(0.0, 0.0, 1920.0, 1080.0);
        draw(
            pass,
            subtract,
            &t.effect,
            screen_rect,
            Vec4::new(0.0, e, 0.5, 1.0),
            screen,
            white(k),
            0.0,
        );
        draw(
            pass,
            subtract,
            &t.effect,
            screen_rect,
            Vec4::new(0.5, e, 1.0, 1.0),
            screen,
            white(1.0 - k),
            0.0,
        );
        // 4. Done icons.
        let icons = m.icon_alpha.value();
        for slot in 0..4 {
            let h = SLOT_HEROINE[slot];
            for (count, texture, top) in [
                (m.done_prisoner[h], &t.icon_prisoner, 180.0),
                (m.done_pierrot[h], &t.icon_pierrot, 244.0),
            ] {
                for i in 0..count {
                    let x = SLOT_X[slot] - 40.0 * count as f32 + 80.0 * i as f32;
                    draw(
                        pass,
                        normal,
                        texture,
                        Vec4::new(x, top, x + 80.0, top + 114.0),
                        full,
                        layer,
                        white(icons),
                        0.0,
                    );
                }
            }
        }
        // 5. Hand cards.
        let card_rect = Vec4::new(-138.0, -196.0, 138.0, 196.0);
        for (i, c) in m.hand.iter().enumerate() {
            let (matrix, facing) = hand_matrix(c);
            let texture = if facing { t.face(c.kind) } else { &t.back };
            draw(
                pass,
                normal,
                texture,
                card_rect,
                full,
                matrix,
                white(c.value(5)),
                c.value(6),
            );
            if m.phase == 1 && m.selected == i as i32 {
                draw(
                    pass,
                    normal,
                    &t.focus,
                    card_rect,
                    full,
                    matrix,
                    white(c.value(5) * pulse(elapsed, 60.0)),
                    0.0,
                );
            }
        }
        // 6. Cancel button.
        let cancel = m.cancel_alpha.value();
        if cancel > 0.0 {
            draw(
                pass,
                normal,
                &t.cancel,
                Vec4::new(1534.0, 46.0, 1874.0, 154.0),
                full,
                screen,
                white(cancel),
                m.cancel_flash.value(),
            );
        }
        // 7. Cut-in.
        let band = m.cutin_band.value();
        if band > 0.0 {
            let matrix =
                layer * translate(0.0, -160.0) * Mat4::from_scale(Vec3::new(1.0, band, 1.0));
            let bg = Vec4::new(-960.0, -256.0, 960.0, 256.0);
            draw(pass, normal, &t.cutin_bg, bg, full, matrix, white(1.0), 0.0);
            let x = -600.0 + 2.0 * m.wobble[0].value() + m.cutin_slide.value();
            let y = -260.0 + 2.0 * m.wobble[1].value();
            draw(
                pass,
                normal,
                t.cutin(m.cutin_heroine),
                Vec4::new(x, y, x + 1200.0, y + 520.0),
                full,
                matrix,
                white(1.0),
                0.0,
            );
            let k = wave(elapsed, 60.0);
            draw(
                pass,
                subtract,
                &t.cutin_fire,
                bg,
                Vec4::new(0.0, 0.0, 0.5, 1.0),
                matrix,
                white(k),
                0.0,
            );
            draw(
                pass,
                subtract,
                &t.cutin_fire,
                bg,
                Vec4::new(0.5, 0.0, 1.0, 1.0),
                matrix,
                white(1.0 - k),
                0.0,
            );
            let black = FloatColor4::from_rgba(0.0, 0.0, 0.0, 1.0);
            fill(pass, Vec4::new(-960.0, 256.0, 960.0, 264.0), matrix, black);
            fill(pass, Vec4::new(-960.0, -264.0, 960.0, -256.0), matrix, black);
        }
        // 8. Message, centred on (0,-80).
        let alpha = m.message_alpha.value();
        if alpha > 0.0
            && let Some(message) = m.message
        {
            let (text, size) = &self.messages.texts[message.index()];
            let matrix = layer * translate(-size.x / 2.0, -80.0 - size.y / 2.0);
            if let Err(error) = text.render(pass, normal, matrix, Vec4::new(1.0, 1.0, 1.0, alpha)) {
                tracing::warn!("Ger CardSelect message: {error:#}");
            }
        }
        // 9. Black fade.
        let fade = m.fade.value();
        if fade > 0.0 {
            fill(
                pass,
                screen_rect,
                screen,
                FloatColor4::from_rgba(0.0, 0.0, 0.0, fade),
            );
        }
    }
}

#[cfg(test)]
#[path = "gerokasu_draw/gpu_tests.rs"]
mod gpu_tests;
