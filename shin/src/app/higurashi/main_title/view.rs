//! Updated deb80/de9c0 parent draw order and byte-quantized sprite tint.
use super::{Frame, particles::Particles};
use crate::{adv::character_view::CharacterTextures, layer::render_params::multiply_transforms};
use glam::{Mat4, vec3};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Artwork {
    Fill,
    Background,
    Light,
    Credit,
    Logo,
    LogoPlus,
}
impl Artwork {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fill => "",
            Self::Background => "title_bg",
            Self::Light => "light1",
            Self::Credit => "credit",
            Self::Logo => "logo",
            Self::LogoPlus => "logo_plus",
        }
    }
    #[cfg(test)]
    pub fn id(self) -> u32 {
        match self {
            Self::Fill => 0,
            Self::Background => 1,
            Self::Light => 2,
            Self::Credit => 3,
            Self::Logo => 4,
            Self::LogoPlus => 5,
        }
    }
}
pub(super) struct Packet {
    pub artwork: Artwork,
    pub opaque: bool,
    pub rect: [f32; 4],
    pub color: u32,
    pub matrix: Mat4,
}
pub(super) fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2.0 / 1920.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / 1080.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        -1.0,
        1.0,
        -0.0,
        1.0,
    ])
}
fn alpha(value: f32) -> u32 {
    u32::from((value.max(0.0) * 255.0) as u8) << 24
}
pub(super) fn packets(
    atlas_present: bool,
    plus: bool,
    frame: &Frame,
    particles: &Particles,
) -> Vec<Packet> {
    let matrix = projection();
    let mut packets = Vec::new();
    if atlas_present {
        packets.push(Packet {
            artwork: Artwork::Background,
            opaque: true,
            rect: [
                0.0,
                frame.background_offset,
                1920.0,
                frame.background_offset + 1280.0,
            ],
            color: u32::MAX,
            matrix,
        });
        for particle in &particles.items {
            packets.push(Packet {
                artwork: Artwork::Light,
                opaque: false,
                rect: [-20.0, -20.0, 20.0, 20.0],
                color: 0x7fffffff,
                matrix: particle.matrix(matrix),
            });
        }
        packets.push(Packet {
            artwork: Artwork::Credit,
            opaque: false,
            rect: [690.0, 972.0, 1230.0, 1072.0],
            color: u32::MAX,
            matrix,
        });
        if frame.logo_alpha > 0.0 {
            let y = frame.logo_offset + 12.0;
            packets.push(Packet {
                artwork: if plus {
                    Artwork::LogoPlus
                } else {
                    Artwork::Logo
                },
                opaque: false,
                rect: [56.0, y, 1136.0, y + 432.0],
                color: 0x00ffffff | alpha(frame.logo_alpha),
                matrix,
            });
        }
    }
    if frame.black_alpha > 0.0 {
        packets.push(Packet {
            artwork: Artwork::Fill,
            opaque: false,
            rect: [0.0, 0.0, 1920.0, 1080.0],
            color: alpha(frame.black_alpha),
            matrix,
        });
    }
    packets
}
impl Packet {
    pub fn render(&self, pass: &mut RenderPass, atlas: Option<&CharacterTextures>) {
        let [left, top, right, bottom] = self.rect;
        let bytes = self.color.to_le_bytes();
        let color = UnormColor::from_rgba(bytes[2], bytes[1], bytes[0], bytes[3]);
        let vertices = QuadVertices::new()
            .with_box(left, top, right, bottom)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .with_color(color);
        let builder = RenderRequestBuilder::new().color_blend_type(if self.opaque {
            ColorBlendType::Opaque
        } else {
            ColorBlendType::Layer1
        });
        // Preserve the native packet matrix;only adapt GL clip-depth at draw.
        let clip =
            Mat4::from_translation(vec3(0.0, 0.0, 0.5)) * Mat4::from_scale(vec3(1.0, 1.0, 0.5));
        let transform = multiply_transforms(clip, self.matrix);
        if self.artwork == Artwork::Fill {
            vertices.fill(pass, builder, transform);
        } else {
            vertices.render_sprite(
                pass,
                builder,
                atlas
                    .expect("Title sprite requires atlas")
                    .get(self.artwork.name())
                    .as_source(),
                transform,
            );
        }
    }
    #[cfg(test)]
    pub fn words(&self) -> Vec<u32> {
        let mut words = vec![
            self.artwork.id(),
            if self.opaque { 1 } else { 2 },
            if self.artwork == Artwork::Fill { 1 } else { 2 },
        ];
        words.extend(self.matrix.to_cols_array().map(f32::to_bits));
        words.push(if self.artwork == Artwork::Fill {
            16
        } else {
            24
        });
        let [left, top, right, bottom] = self.rect;
        for (x, y, u, v) in [
            (left, top, 0.0_f32, 0.0_f32),
            (right, top, 1.0, 0.0),
            (left, bottom, 0.0, 1.0),
            (right, bottom, 1.0, 1.0),
        ] {
            words.extend([x.to_bits(), y.to_bits(), 0, self.color]);
            if self.artwork != Artwork::Fill {
                words.extend([u.to_bits(), v.to_bits()]);
            }
        }
        words
    }
}
