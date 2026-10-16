//! Original444b4 character selection artwork, rendered by the scene layer.
use super::Model;
use crate::{
    adv::{characters::view::CharacterTextures, higurashi::message_ui::HigurashiMessageTextures},
    asset::system::AssetServer,
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::Result;
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
use std::sync::Arc;

pub struct View {
    pub model: Arc<Mutex<Model>>,
    atlas: Arc<CharacterTextures>,
    message: Arc<HigurashiMessageTextures>,
    pub route_navigation: bool,
}
impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let model = self.model.lock();
        f.debug_struct("CharacterSelectionView")
            .field("rows", &model.rows.len())
            .field("phase", &model.phase)
            .finish_non_exhaustive()
    }
}
impl Clone for View {
    fn clone(&self) -> Self {
        Self {
            model: Arc::new(Mutex::new(self.model.lock().native_copy())),
            atlas: self.atlas.clone(),
            message: self.message.clone(),
            route_navigation: self.route_navigation,
        }
    }
}
#[derive(Clone, Debug)]
struct Packet {
    texture: u8,
    rect: Vec4,
    uv: Vec4,
    alpha: f32,
    flash: f32,
}
impl Packet {
    fn vertices(&self) -> [PosColTexVertex; 4] {
        build_quad_vertices(|c| PosColTexVertex {
            position: vec3(
                if c.x == 0.0 { self.rect.x } else { self.rect.z },
                if c.y == 0.0 { self.rect.y } else { self.rect.w },
                0.0,
            ),
            texture_position: vec2(
                if c.x == 0.0 { self.uv.x } else { self.uv.z },
                if c.y == 0.0 { self.uv.y } else { self.uv.w },
            ),
            color: FloatColor4::from_rgba(1.0, 1.0, 1.0, self.alpha).into_unorm(),
        })
    }
}
impl View {
    /// Asset preparation is not the434fc virtual scene-copy constructor.
    /// Preserve the prepared initial clocks when making a fresh live owner.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            model: Arc::new(Mutex::new(self.model.lock().clone())),
            atlas: self.atlas.clone(),
            message: self.message.clone(),
            route_navigation: self.route_navigation,
        }
    }
    /// Native intrusive retention refers to the same live layer. Clone remains
    /// the independent434fc scene copy used by transitions/reconstruction.
    pub fn retain(&self) -> Self {
        Self {
            model: self.model.clone(),
            atlas: self.atlas.clone(),
            message: self.message.clone(),
            route_navigation: self.route_navigation,
        }
    }
    pub async fn load(
        server: &AssetServer,
        message: Arc<HigurashiMessageTextures>,
        mask: u32,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(Model::new(mask)?)),
            atlas: server.load("/charsel.txa").await?,
            message,
            route_navigation: true,
        })
    }
    pub fn advance(&mut self, delta: Ticks) {
        self.model.lock().advance_layer(delta);
    }
    pub fn fast_forward(&mut self) {
        self.model.lock().fast_forward();
    }
    #[cfg(test)]
    pub(crate) fn reference_packets(&self) -> Vec<(u8, [PosColTexVertex; 4], f32)> {
        self.packets()
            .iter()
            .map(|packet| (packet.texture, packet.vertices(), packet.flash))
            .collect()
    }
    fn packets(&self) -> Vec<Packet> {
        let m = self.model.lock();
        let mut p = Vec::with_capacity(m.rows.len() + 10);
        let full = vec4(0.0, 0.0, 1.0, 1.0);
        let mut push = |texture, rect, uv, alpha, flash| {
            p.push(Packet {
                texture,
                rect,
                uv,
                alpha,
                flash,
            })
        };
        let opacity = m.clocks[3].value() * m.clocks[5].value();
        if opacity > 0.0 {
            let slide = m.clocks[2].value();
            let scale = m.clocks[4].value();
            let original = m.rows[m.selected as usize].original;
            let anchor = slide + 984.0;
            let width = scale * 690.0;
            let height = scale * 360.0;
            let left = (anchor + 690.0) - width;
            let top = 1014.0 - height;
            push(
                original as u8,
                vec4(left, top, left + width, top + height),
                full,
                opacity,
                0.0,
            );
            let left = anchor + 432.0;
            push(
                original as u8 + 4,
                vec4(left, 0.0, left + 312.0, 1080.0),
                full,
                opacity,
                0.0,
            );
        }
        for row in &m.rows {
            let y = (m.top + row.id as i32 * 150 + 3) as f32;
            let u = if row.id as i32 == m.selected {
                0.0
            } else {
                612.0 / 1224.0
            };
            let v = (row.original * 144) as f32 / 576.0;
            push(
                8,
                vec4(432.0, y, 1044.0, y + 144.0),
                vec4(u, v, u + 0.5, v + 0.25),
                row.alpha.value(),
                row.flash.value(),
            );
        }
        if m.selected >= 0 && self.route_navigation {
            let row = &m.rows[m.selected as usize];
            let mask = [33u8, 2, 52, 8][row.original];
            let y = (m.top + m.selected * 150 + 64) as f32;
            let x = m.clocks[6].value() * -300.0 + 992.0;
            let alpha = m.clocks[7].value() * row.alpha.value();
            push(
                10,
                vec4(x - 4.0, y + 36.0, x - 4.0 + 420.0, y + 68.0),
                full,
                alpha,
                0.0,
            );
            let mut left = x + (360 - mask.count_ones() as i32 * 60) as f32 * 0.5;
            for (i, (top, bottom)) in [
                (0x3b257eb5, 0x3e2814b0),
                (0x3e2d40a5, 0x3ea95fad),
                (0x3eabf5a8, 0x3efeb502),
                (0x3f00a57f, 0x3f2a052c),
                (0x3f2b5029, 0x3f54afd6),
                (0x3f55fad4, 0x3f7f5a81),
            ]
            .into_iter()
            .enumerate()
            {
                if mask & (1 << i) != 0 {
                    push(
                        11,
                        vec4(left, y, left + 64.0, y + 64.0),
                        vec4(
                            f32::from_bits(0x3c783e10),
                            f32::from_bits(top),
                            f32::from_bits(0x3f7c1f08),
                            f32::from_bits(bottom),
                        ),
                        alpha,
                        0.0,
                    );
                    left += 60.0;
                }
            }
        }
        let y = m.clocks[0].value() + 96.0;
        push(
            9,
            vec4(192.0, y, 1272.0, y + 180.0),
            full,
            m.clocks[1].value(),
            0.0,
        );
        p
    }
    fn matrix(properties: &HigurashiProperties, parent: &HigurashiTransform) -> Mat4 {
        let native = properties
            .composed_transform(parent)
            .compute_native_final_transform();
        multiply(
            multiply(native, Mat4::from_translation(vec3(-640.0, -720.0, 0.0))),
            Mat4::from_scale(vec3(2.0 / 3.0, 2.0 / 3.0, 0.0)),
        )
    }
    fn texture(&self, id: u8) -> &shin_render::gpu_texture::GpuTexture {
        match id {
            0..=3 => self.atlas.get(&format!("shadow{id}")),
            4..=7 => self.atlas.get(&format!("char{}", id - 4)),
            8 => self.atlas.get("name"),
            9 => self.atlas.get("caption"),
            10 => &self.message.navigation_arrows,
            11 => &self.message.navigation_icons,
            _ => unreachable!(),
        }
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if kind != PassKind::Transparent || !properties.is_visible() {
            return Ok(());
        }
        let transform = HigurashiTransform::convert_clip_depth(Self::matrix(properties, parent));
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil, true, false);
        for packet in self.packets() {
            let vertices = packet.vertices();
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let texture = self.texture(packet.texture).as_source();
            let program = if packet.flash > 0.0 {
                RenderProgramWithArguments::Button {
                    vertices,
                    texture,
                    transform,
                    flash1: vec4(1.0, 1.0, 1.0, packet.flash),
                    flash2: Vec4::ZERO,
                }
            } else {
                RenderProgramWithArguments::Sprite {
                    vertices,
                    sprite: texture,
                    transform,
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
