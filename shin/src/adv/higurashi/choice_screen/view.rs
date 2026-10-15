use super::{Model, Spec, split_badges};
use crate::{
    adv::higurashi::{
        browser_navigation::labels::{Glyphs, Layout},
        choice::ChoiceRequest,
        input::{Buttons, OverlayInput},
        message_ui::HigurashiMessageTextures,
        touch::Overlay,
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::Result;
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{layout::LayoutParams, primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
use std::{collections::HashMap, sync::Arc};

pub struct Screen {
    pub model: Model,
    pub properties: HigurashiProperties,
    glyphs: HashMap<usize, Glyphs>,
    textures: Arc<HigurashiMessageTextures>,
    pub route_navigation: bool,
}
impl Screen {
    pub fn new(
        server: &AssetServer,
        font: Arc<GpuFontLazy>,
        textures: Arc<HigurashiMessageTextures>,
        request: &ChoiceRequest,
        seen: u32,
        seeds: [u32; 4],
        shuffle_seed: Option<u32>,
    ) -> Result<Self> {
        let mut screen = Self::new_ui(server, font, textures, &request.variants,
            request.visibility as u32, seen, seeds, shuffle_seed)?;
        // Live5848 sets optional layer property5 after the UI constructor.
        screen.properties.control(5, 1.0, Ticks::ZERO, 0)?;
        Ok(screen)
    }
    /// Directc0dbc/c0e30 UI construction has no script destination or response
    /// owner. Its base retains native defaultpriority; the caller owns overrides.
    pub(crate) fn new_ui(
        server: &AssetServer,
        font: Arc<GpuFontLazy>,
        textures: Arc<HigurashiMessageTextures>,
        labels: &[String],
        visibility: u32,
        seen: u32,
        seeds: [u32; 4],
        shuffle_seed: Option<u32>,
    ) -> Result<Self> {
        let context = server.context();
        let mut glyphs = HashMap::new();
        let mut specs = Vec::new();
        for (original, text) in labels
            .iter()
            .enumerate()
            .filter(|(i, _)| visibility & (1 << i) != 0)
        {
            let (badges, label) = split_badges(text);
            let layout = Layout::from_script(
                font.clone(),
                label,
                LayoutParams {
                    layout_width: 996.0,
                    text_size: 48.0,
                    perform_soft_breaks: false,
                    ..Default::default()
                },
            )?;
            let height = layout.size.y + 16.0;
            glyphs.insert(
                original,
                Glyphs::from_layout(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    font.clone(),
                    layout,
                ),
            );
            specs.push(Spec {
                original,
                label: label.into(),
                badges,
                seen: seen & (1 << original) != 0,
                height,
            });
        }
        let model = Model::new(specs, visibility, shuffle_seed)?;
        let properties = HigurashiProperties::from_seeds(seeds);
        Ok(Self {
            model,
            properties,
            glyphs,
            textures,
            route_navigation: true,
        })
    }
    pub fn advance(&mut self, delta: Ticks) {
        // c1f20 overrides the base update and advances only window clocks.
        self.model.advance(delta);
    }
    pub fn render(&self, pass: &mut RenderPass, stencil: u8, kind: PassKind) -> Result<()> {
        self.render_with_parent(pass, &HigurashiTransform::default(), stencil, kind)
    }
    pub(crate) fn render_with_parent(&self, pass: &mut RenderPass,
        parent: &HigurashiTransform, stencil: u8, kind: PassKind) -> Result<()> {
        if kind != PassKind::Transparent || !self.properties.is_visible() {
            return Ok(());
        }
        let parent = self.parent_transform_with(parent);
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil, true, false);
        for row in &self.model.rows {
            let native = self.row_transform(parent, row);
            let alpha = row.alpha.value() * self.model.opacity;
            for (y0, y1, v0, v1) in [
                (-8.0, 0.0, 0.0, 0.1),
                (0.0, row.spec.height, 0.1, 0.9),
                (row.spec.height, row.spec.height + 8.0, 0.9, 1.0),
            ] {
                quad(
                    pass,
                    &self.textures.selection,
                    vec4(360.0, y0, 1560.0, y1),
                    vec4(0.0, v0, 1.0, v1),
                    alpha,
                    native,
                    builder,
                );
            }
            let focus = row.focus.value();
            let color = vec4(focus, focus, focus, alpha);
            let tint = if row.spec.seen {
                vec4(0.75, 0.75, 1.0, 1.0)
            } else {
                Vec4::ONE
            };
            let glyph = &self.glyphs[&row.spec.original];
            let text = self.text_transform(native, glyph.layout.size);
            glyph.render_gradient_at(
                pass,
                text,
                color,
                color * tint,
                vec4(0.0, 0.0, 0.0, alpha),
                builder,
            );
        }
        if self.model.selected >= 0 && self.route_navigation {
            let row = &self.model.rows[self.model.selected as usize];
            let mask = row.spec.badges & 63;
            if mask != 0 {
                let y = row.y + (row.spec.height - 64.0) * 0.5;
                let x = self.model.badge_slide.value() * -300.0 + 1480.0;
                let alpha = self.model.badge_alpha.value() * self.model.opacity;
                quad(
                    pass,
                    &self.textures.navigation_arrows,
                    vec4(x - 4.0, y + 36.0, x - 4.0 + 420.0, y + 68.0),
                    vec4(0.0, 0.0, 1.0, 1.0),
                    alpha,
                    parent,
                    builder,
                );
                let mut left = x + (360 - (mask.count_ones() as i32) * 60) as f32 * 0.5;
                let v = [
                    [0x3b257eb5, 0x3e2814b0],
                    [0x3e2d40a5, 0x3ea95fad],
                    [0x3eabf5a8, 0x3efeb502],
                    [0x3f00a57f, 0x3f2a052c],
                    [0x3f2b5029, 0x3f54afd6],
                    [0x3f55fad4, 0x3f7f5a81],
                ];
                for (i, [top, bottom]) in v.into_iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        quad(
                            pass,
                            &self.textures.navigation_icons,
                            vec4(left, y, left + 64.0, y + 64.0),
                            vec4(
                                f32::from_bits(0x3c783e10),
                                f32::from_bits(top),
                                f32::from_bits(0x3f7c1f08),
                                f32::from_bits(bottom),
                            ),
                            alpha,
                            parent,
                            builder,
                        );
                        left += 60.0;
                    }
                }
            }
        }
        Ok(())
    }
    fn parent_transform(&self) -> Mat4 {
        self.parent_transform_with(&HigurashiTransform::default())
    }
    fn parent_transform_with(&self, parent: &HigurashiTransform) -> Mat4 {
        let native = self
            .properties
            .composed_transform(parent)
            .compute_native_final_transform();
        // c2248/c2294 append translation and scale separately to the final
        // matrix. Grouping the geometry first loses bits for transformed UI.
        multiply(
            multiply(native, Mat4::from_translation(vec3(-640.0, -720.0, 0.0))),
            Mat4::from_scale(vec3(2.0 / 3.0, 2.0 / 3.0, 0.0)),
        )
    }
    fn row_transform(&self, parent: Mat4, row: &super::Row) -> Mat4 {
        multiply(
            parent,
            Mat4::from_translation(vec3(row.x.value(), row.y, 0.0)),
        )
    }
    fn text_transform(&self, native: Mat4, size: Vec2) -> Mat4 {
        multiply(
            multiply(native, Mat4::from_translation(vec3(0.0, 8.0, 0.0))),
            multiply(
                Mat4::from_translation(vec3(960.0, 0.0, 0.0)),
                multiply(
                    Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
                    Mat4::from_translation(vec3(-size.x * 0.5, -(size.y * 0.0), 0.0)),
                ),
            ),
        )
    }
}
fn quad(
    pass: &mut RenderPass,
    texture: &GpuTexture,
    rect: Vec4,
    uv: Vec4,
    alpha: f32,
    native: Mat4,
    builder: RenderRequestBuilder,
) {
    let vertices = sprite_vertices(rect, uv, alpha);
    pass.run(builder.build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: texture.as_source(),
            transform: HigurashiTransform::convert_clip_depth(native),
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
fn sprite_vertices(rect: Vec4, uv: Vec4, alpha: f32) -> [PosColTexVertex; 4] {
    build_quad_vertices(|c| PosColTexVertex {
        position: vec3(
            if c.x == 0.0 { rect.x } else { rect.z },
            if c.y == 0.0 { rect.y } else { rect.w },
            0.0,
        ),
        texture_position: vec2(
            if c.x == 0.0 { uv.x } else { uv.z },
            if c.y == 0.0 { uv.y } else { uv.w },
        ),
        color: FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm(),
    })
}
pub struct Shared(pub Arc<Mutex<Screen>>);
impl Overlay for Shared {
    fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
    fn input_permissions(&self) -> OverlayInput {
        self.0.lock().model.permissions()
    }
    fn probe(&mut self, p: Vec2) -> bool {
        self.0.lock().model.probe(p)
    }
    fn press(&mut self, p: Vec2) {
        self.0.lock().model.press(p);
    }
    fn release(&mut self, p: Vec2) {
        self.0.lock().model.release(p);
    }
    fn cancel(&mut self) {}
    fn set_hidden(&mut self, hidden: bool) {
        self.0.lock().model.hidden = hidden;
    }
    fn input(&mut self, permission: bool, buttons: Buttons) {
        self.0.lock().model.input(permission, buttons);
    }
    fn advance(&mut self, delta: Ticks) {
        self.0.lock().advance(delta);
    }
    fn fast_forward(&mut self) {
        self.0.lock().model.fast_forward();
    }
    fn take_cues(&mut self) -> Vec<&'static str> {
        self.0.lock().model.take_cues()
    }
    fn priority(&self) -> f32 {
        self.0.lock().properties.values()[5]
    }
    fn render(&self, pass: &mut RenderPass, stencil: u8, kind: PassKind) -> Result<()> {
        self.0.lock().render(pass, stencil, kind)
    }
}

#[cfg(test)]
mod tests;
