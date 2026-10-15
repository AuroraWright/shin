//! Native ScenarioSelectBridge e44b0 perspective captured-screen doors.
use super::episode_transition::EpisodeFrame;
use glam::{Mat4, vec3, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
    shaders::types::texture::TextureSource,
};

pub(super) fn door_transforms(frame: EpisodeFrame) -> [Mat4; 2] {
    // Native row matrices compose in reverse order after conversion to glam.
    // e45d8: GL perspective, e45ec: Y inversion, e4634: camera Z=-1000.
    let camera = glam::camera::rh::proj::opengl::perspective(f32::from_bits(0x3f7d821c), 1920.0 / 1080.0, 1.0, 10000.0,
    )
        * Mat4::from_scale(vec3(1.0, -1.0, 1.0))
        * Mat4::from_translation(vec3(0.0, 0.0, -1000.0))
        * Mat4::from_translation(vec3(0.0, 0.0, frame.door_offset));
    let angle = frame.rotation_progress * std::f32::consts::FRAC_PI_2;
    [
        camera
            * Mat4::from_translation(vec3(-960.0, 0.0, 0.0))
            * Mat4::from_rotation_y(angle)
            * Mat4::from_translation(vec3(960.0, 0.0, 0.0)),
        camera
            * Mat4::from_translation(vec3(960.0, 0.0, 0.0))
            * Mat4::from_rotation_y(-angle)
            * Mat4::from_translation(vec3(-960.0, 0.0, 0.0)),
    ]
}

/// Draw only the perspective doors; the owner supplies capture/blur/cover passes.
pub(super) fn render_doors(
    source: TextureSource<'_>,
    frame: EpisodeFrame,
    color: UnormColor,
    pass: &mut RenderPass,
) {
    for (transform, rect, uv) in [
        (
            door_transforms(frame)[0],
            vec4(-960.0, -540.0, 0.0, 540.0),
            vec4(0.0, 0.0, 0.5, 1.0),
        ),
        (
            door_transforms(frame)[1],
            vec4(0.0, -540.0, 960.0, 540.0),
            vec4(0.5, 0.0, 1.0, 1.0),
        ),
    ] {
        QuadVertices::new()
            .with_box(rect.x, rect.y, rect.z, rect.w)
            .with_tex_box(uv.x, uv.y, uv.z, uv.w)
            .with_color(color)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                source,
                transform,
            );
    }
}

#[cfg(test)]
#[path = "episode_transition_view_reference.rs"]
mod reference;
