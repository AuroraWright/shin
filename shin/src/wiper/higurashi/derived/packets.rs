//! Native projection/vertices retain older floating expression order.
use crate::layer::render_params::{
    higurashi::{HigurashiTransform, native_rotation_z},
    multiply_transforms as multiply,
};
use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::TextureSource,
        vertices::{MaskVertex, PosColTexVertex, PosTexVertex},
    },
};

pub(crate) enum Packet {
    Default {
        transform: Mat4,
        vertices: [PosTexVertex; 4],
        alpha: f32,
    },
    Mask {
        transform: Mat4,
        vertices: [MaskVertex; 4],
        minmax: Vec2,
    },
    Sprite {
        transform: Mat4,
        vertices: [PosColTexVertex; 4],
        incoming: bool,
        blend: ColorBlendType,
    },
}
pub(super) fn orthographic(width: f32, height: f32, centered: bool) -> Mat4 {
    Mat4::from_cols_array(&[
        2.0 / width,
        0.,
        0.,
        0.,
        0.,
        -2.0 / height,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        if centered { -0. } else { -1. },
        if centered { 0. } else { 1. },
        -0.,
        1.,
    ])
}
pub(super) fn quad(size: Vec2, z: f32, alpha: u8) -> [PosColTexVertex; 4] {
    [(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|(x, y)| PosColTexVertex {
        position: vec3(
            if x == 0. { -size.x / 2. } else { size.x / 2. },
            if y == 0. { -size.y / 2. } else { size.y / 2. },
            z,
        ),
        texture_position: vec2(x, y),
        color: UnormColor::from_rgba(255, 255, 255, alpha),
    })
}
pub(super) fn turn_matrix(angle: f32) -> Mat4 {
    let z = native_rotation_z(angle);
    let sine = z.x_axis.y;
    let cosine = z.x_axis.x;
    let rotation = Mat4::from_cols_array(&[
        cosine, 0., -sine, 0., 0., 1., 0., 0., sine, 0., cosine, 0., 0., 0., 0., 1.,
    ]);
    // Original74db0 divides the depth terms directly. glam's GL helper
    // multiplies by a reciprocal, which changes the native packet words.
    let focal = 1. / (f32::from_bits(0x3f7d821c) * 0.5).tan();
    let near = 1.0f32;
    let far = 10000.0f32;
    let depth = near - far;
    let projection = Mat4::from_cols_array(&[
        focal / f32::from_bits(0x3fe38e39),
        0.,
        0.,
        0.,
        0.,
        focal,
        0.,
        0.,
        0.,
        0.,
        (near + far) / depth,
        -1.,
        0.,
        0.,
        ((near + near) * far) / depth,
        0.,
    ]);
    let projection = multiply(projection, Mat4::from_translation(vec3(0., 0., -1000.)));
    let projection = multiply(projection, Mat4::from_scale(vec3(1., -1., 1.)));
    multiply(projection, rotation)
}
impl Packet {
    pub(crate) fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        incoming: TextureSource,
        outgoing: TextureSource,
        mask: Option<TextureSource>,
    ) {
        match self {
            Self::Default {
                transform,
                vertices,
                alpha,
            } => pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
                RenderProgramWithArguments::WiperHigurashiDefault {
                    vertices: VertexSource::VertexData { vertices },
                    texture_source: outgoing,
                    texture_target: incoming,
                    transform: HigurashiTransform::convert_clip_depth(*transform),
                    alpha: *alpha,
                },
                DrawPrimitive::TrianglesStrip,
            )),
            Self::Mask {
                transform,
                vertices,
                minmax,
            } => pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
                RenderProgramWithArguments::WiperHigurashiMask {
                    vertices: VertexSource::VertexData { vertices },
                    texture_source: outgoing,
                    texture_target: incoming,
                    texture_mask: mask.unwrap(),
                    transform: HigurashiTransform::convert_clip_depth(*transform),
                    minmax: *minmax,
                },
                DrawPrimitive::TrianglesStrip,
            )),
            Self::Sprite {
                transform,
                vertices,
                incoming: take_incoming,
                blend,
            } => pass.run(builder.color_blend_type(*blend).build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData { vertices },
                    sprite: if *take_incoming { incoming } else { outgoing },
                    transform: HigurashiTransform::convert_clip_depth(*transform),
                },
                DrawPrimitive::TrianglesStrip,
            )),
        }
    }
}
