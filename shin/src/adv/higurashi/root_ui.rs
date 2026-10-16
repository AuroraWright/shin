//! Updated1382c auto/skip indicators, sliding navi buttons and event hint.
use super::input::{ReadState, message_requests, suppress_transient_audio};
use crate::adv::characters::view::CharacterTextures;
use glam::{Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

#[derive(Clone, Copy)]
pub struct State {
    pub modes: u32,
    pub vm: Option<ReadState>,
    pub forced: bool,
    pub read_only: bool,
    pub elapsed_seconds: f32,
    pub navi_slide: f32,
    pub pressed: i32,
    pub hint: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            modes: 0,
            vm: None,
            forced: false,
            read_only: false,
            elapsed_seconds: 0.0,
            navi_slide: 0.0,
            pressed: -1,
            hint: 0.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Texture {
    Adv,
    Navi,
    Skip,
}
impl Texture {
    fn name(self) -> &'static str {
        match self {
            Self::Adv => "adv",
            Self::Navi => "navi",
            Self::Skip => "skip",
        }
    }
}
pub struct Packet {
    pub texture: Texture,
    pub vertices: [PosColTexVertex; 4],
}
impl Packet {
    fn new(texture: Texture, rect: Vec4, uv: Vec4, color: UnormColor) -> Self {
        Self {
            texture,
            vertices: build_quad_vertices(|corner| PosColTexVertex {
                position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
                color,
                texture_position: vec2(uv.x + uv.z * corner.x, uv.y + uv.w * corner.y),
            }),
        }
    }
    pub fn render(&self, atlas: &CharacterTextures, pass: &mut RenderPass) {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &self.vertices,
                        },
                        sprite: atlas.get(self.texture.name()).as_source(),
                        transform: crate::render::top_left_projection_matrix(),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
pub fn packets(state: State) -> Vec<Packet> {
    let mut packets = Vec::with_capacity(8);
    let pulse = (state.elapsed_seconds * f32::from_bits(0x40c90fd0)).sin() * 0.25 + 0.75;
    let grey = vec4(0.25, 0.25, 0.25, 1.0);
    if state.modes & 2 != 0 {
        let color =
            if suppress_transient_audio(state.vm, state.modes, state.forced, state.read_only) {
                if state.read_only {
                    vec4(pulse, pulse, 1.0, 1.0)
                } else {
                    vec4(1.0, pulse, pulse, 1.0)
                }
            } else {
                grey
            };
        packets.push(Packet::new(
            Texture::Adv,
            vec4(1824.0, 24.0, 72.0, 72.0),
            vec4(0.0, 0.5, 1.0, 0.5),
            FloatColor4::from_vec4(color).into_unorm(),
        ));
    }
    if state.modes & 1 != 0 {
        let color =
            if message_requests(state.vm, state.modes, state.forced, state.read_only).autoplay {
                vec4(pulse, 1.0, pulse, 1.0)
            } else {
                grey
            };
        packets.push(Packet::new(
            Texture::Adv,
            vec4(
                if state.modes & 2 != 0 { 1744.0 } else { 1824.0 },
                24.0,
                72.0,
                72.0,
            ),
            vec4(0.0, 0.0, 1.0, 0.5),
            FloatColor4::from_vec4(color).into_unorm(),
        ));
    }
    if state.navi_slide != 0.0 {
        let offset = (1.0 - state.navi_slide) * 588.0;
        for (id, x, y, v) in [
            (0, 1332.0, 120.0, 0.0),
            (1, 1524.0, 120.0, 0.2),
            (2, 1716.0, 120.0, 0.4),
            (3, 1428.0, 252.0, 0.6),
            (4, 1620.0, 252.0, 0.8),
        ] {
            // Original disables backlog only while an attached VM is in an event.
            let color = if id == 2 && state.vm.is_some_and(|v| v.flags & 0x8000_0000 == 0) {
                UnormColor(0x80ffffff)
            } else {
                UnormColor::WHITE
            };
            packets.push(Packet::new(
                Texture::Navi,
                vec4(x + offset, y, 192.0, 120.0),
                vec4(if state.pressed == id { 0.5 } else { 0.0 }, v, 0.5, 0.2),
                color,
            ));
        }
    }
    if state.hint > 0.0 {
        // Updated Switch cc940 returns0: use the first half of the skip atlas.
        packets.push(Packet::new(
            Texture::Skip,
            vec4(16.0, 1000.0, 600.0, 64.0),
            vec4(0.0, 0.0, 1.0, 0.5),
            FloatColor4::from_rgba(1.0, 1.0, 1.0, state.hint).into_unorm(),
        ));
    }
    packets
}
pub fn render(state: State, atlas: &CharacterTextures, pass: &mut RenderPass) {
    for packet in packets(state) {
        packet.render(atlas, pass);
    }
}

#[cfg(test)]
pub(crate) mod tests;
