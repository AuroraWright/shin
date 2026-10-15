//! Updated7910c cursor slices and c0574 vertical scrollbar packets.
//! Keep the old engine's caps, strip topology and matrix operation order.
use super::browser_controls::Scrollbar;
use crate::{
    adv::character_view::CharacterTextures,
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use glam::{Mat4, Vec4, vec2, vec3};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

pub(super) fn parent_transform(slide: f32) -> Mat4 {
    multiply(
        glam::camera::rh::proj::opengl::orthographic(0.0, 1920.0, 1080.0, 0.0, -1.0, 1.0),
        Mat4::from_translation(vec3(1920.0 - slide * 1920.0, 0.0, 0.0)),
    )
}
pub(super) fn quad(rect: Vec4, uv: Vec4, color: UnormColor) -> [PosColTexVertex; 4] {
    std::array::from_fn(|i| PosColTexVertex {
        position: vec3(
            if i & 1 == 0 { rect.x } else { rect.z },
            if i & 2 == 0 { rect.y } else { rect.w },
            0.0,
        ),
        color,
        texture_position: vec2(
            if i & 1 == 0 { uv.x } else { uv.z },
            if i & 2 == 0 { uv.y } else { uv.w },
        ),
    })
}
pub(super) fn cursor(rect: Vec4, color: FloatColor4) -> [[PosColTexVertex; 4]; 9] {
    // Original4cc90 packs B,G,R,A; the browser passes white with tween alpha.
    let color = FloatColor4::from_rgba(color.b, color.g, color.r, color.a).into_unorm();
    let right = rect.x + rect.z;
    let bottom = rect.y + rect.w;
    let xs = [rect.x, rect.x + 16.0, right + -16.0, right];
    let ys = [rect.y, rect.y + 16.0, bottom + -16.0, bottom];
    let uv = [0.0, 0.25, 0.75, 1.0];
    std::array::from_fn(|i| {
        let x = i % 3;
        let y = i / 3;
        quad(
            Vec4::new(xs[x], ys[y], xs[x + 1], ys[y + 1]),
            Vec4::new(uv[x], uv[y], uv[x + 1], uv[y + 1]),
            color,
        )
    })
}
pub(crate) fn scrollbar(height: f32, state: Scrollbar) -> [[PosColTexVertex; 8]; 2] {
    let offset = state.position.wrapping_sub(state.first);
    let range = state.end.wrapping_sub(state.first) as f32;
    let track = height + -32.0;
    let start = track * (offset as f32 / range);
    let end = track * (state.visible.wrapping_add(offset) as f32 / range);
    let ys = [
        [-16.0, 0.0, track, track + 16.0],
        [start + -16.0, start, end, end + 16.0],
    ];
    std::array::from_fn(|strip| {
        std::array::from_fn(|i| PosColTexVertex {
            position: vec3(if i & 1 == 0 { 0.0 } else { 32.0 }, ys[strip][i / 2], 1.0),
            color: UnormColor::WHITE,
            texture_position: vec2((strip + (i & 1)) as f32 * 0.5, (i / 2) as f32 * 0.25),
        })
    })
}
pub(crate) fn scrollbar_transform(parent: Mat4, x: f32, y: f32) -> Mat4 {
    let local = multiply(
        Mat4::from_translation(vec3(x, y, 0.0)),
        multiply(Mat4::from_translation(vec3(0.0, 16.0, 0.0)), Mat4::IDENTITY),
    );
    multiply(parent, local)
}
pub(crate) fn sprite(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    name: &str,
    vertices: &[PosColTexVertex],
    matrix: Mat4,
) {
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData { vertices },
                    sprite: atlas.get(name).as_source(),
                    transform: HigurashiTransform::convert_clip_depth(matrix),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
pub(super) fn render_cursor(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    rect: Vec4,
    alpha: f32,
    matrix: Mat4,
) {
    for vertices in cursor(rect, FloatColor4::WHITE.with_alpha(alpha)) {
        sprite(pass, atlas, "cursor", &vertices, matrix);
    }
}
/// Native browser initializes vertical mode1 with height818. Its parent already
/// contains the slide offset; original787b0 adds that offset again to the origin.
pub(super) fn render_scrollbar(
    pass: &mut RenderPass,
    system: &CharacterTextures,
    state: Scrollbar,
    slide: f32,
) {
    // Measurements publish a nonempty document range before this can draw.
    if state.end <= state.first {
        return;
    }
    let parent = multiply(
        parent_transform(slide),
        Mat4::from_translation(vec3((1920.0 - slide * 1920.0) + 1848.0, 162.0, 0.0)),
    );
    let matrix = scrollbar_transform(parent, 0.0, 0.0);
    for vertices in scrollbar(818.0, state) {
        sprite(pass, system, "scroll", &vertices, matrix);
    }
}

#[cfg(test)]
mod tests;
