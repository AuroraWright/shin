//! Native ManualViewer shell, clipped pages, and authored controller hints.
use crate::adv::{
    characters::view::CharacterTextures,
    manual::{ManualModel, ManualPhase},
};
use crate::layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams};
use glam::{Mat4, vec2, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

/// Draw authored pages under the native slide transform and y162..980 stencil.
/// The owner draws the backdrop, caption, close button and controller hints separately.
pub fn render_pages(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let transform =
        canvas * Mat4::from_translation(vec3(1920.0 * (1.0 - model.progress()), 0.0, 0.0));
    let clip = DrawableClipParams {
        mode: DrawableClipMode::Clip,
        area: vec4(0.0, 162.0, 1920.0, 818.0),
    };
    clip.begin(
        pass,
        &TransformParams {
            transform: crate::render::centered_projection_matrix().inverse() * transform,
            ..Default::default()
        },
    );
    let builder = RenderRequestBuilder::new()
        .color_blend_type(ColorBlendType::Layer1)
        .depth_stencil(DepthStencilState {
            stencil: StencilState {
                stencil_reference: 0,
                pipeline: StencilPipelineState {
                    function: StencilFunction::Equal,
                    stencil_read_mask: StencilMask::ClipOnly,
                    ..Default::default()
                },
            },
            ..Default::default()
        });
    for page in 0..model.count() {
        let (rect, alpha) = model.page_artwork(page).unwrap();
        if rect.x >= 1920.0 || rect.x + rect.z <= 0.0 {
            continue;
        }
        let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm();
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
            texture_position: vec2(corner.x, corner.y),
            color,
        });
        pass.run(builder.build(
            RenderProgramWithArguments::Sprite {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                sprite: textures.get(&format!("{:03}", page + 1)).as_source(),
                transform,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
    clip.end(pass);
}

fn sprite(
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    name: &str,
    rect: glam::Vec4,
    uv: glam::Vec4,
    alpha: f32,
    blend: ColorBlendType,
    canvas: Mat4,
) {
    let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm();
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
        position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
        texture_position: vec2(uv.x + uv.z * corner.x, uv.y + uv.w * corner.y),
        color,
    });
    pass.run(RenderRequestBuilder::new().color_blend_type(blend).build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: textures.get(name).as_source(),
            transform: canvas,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
/// Native85ab4..85c64 backdrop fades; the two quarter-alpha additive bars slide.
pub fn render_background(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let p = model.progress();
    sprite(
        textures,
        pass,
        "bg",
        vec4(0.0, 0.0, 1920.0, 1080.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        p,
        ColorBlendType::Layer1,
        canvas,
    );
    sprite(
        textures,
        pass,
        "obi",
        vec4(0.0, -160.0 + 160.0 * p, 1920.0, 160.0),
        vec4(0.0, 0.0, 1.0, 0.5),
        0.25,
        ColorBlendType::Layer2,
        canvas,
    );
    sprite(
        textures,
        pass,
        "obi",
        vec4(0.0, 1080.0 - 160.0 * p, 1920.0, 160.0),
        vec4(0.0, 0.5, 1.0, 0.5),
        0.25,
        ColorBlendType::Layer2,
        canvas,
    );
}
/// Native85f94..860b0 caption and close button share the top-bar vertical slide.
pub fn render_foreground(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let top = -160.0 + 160.0 * model.progress();
    sprite(
        textures,
        pass,
        "caption",
        vec4(480.0, top, 960.0, 128.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        1.0,
        ColorBlendType::Layer1,
        canvas,
    );
    textures.render_button(
        pass,
        "close",
        vec4(1764.0, top + 32.0, 1848.0, top + 116.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        shin_core::primitives::color::UnormColor::WHITE,
        glam::Vec4::ZERO,
        vec4(1.0, 1.0, 1.0, model.close_flash()),
        canvas,
    );
}

/// Native86140 lays labels and button icons out from right to left.
/// Native27320 consults fb9f0 for a possible 3/4 swap; the supplied executable's
/// fb9f0 returns zero, so the atlas indices below are unchanged.
fn render_hint_row(
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    right: f32,
    y: f32,
    row: usize,
    canvas: Mat4,
) -> f32 {
    // Native170740: first button, optional second button, label y, label width.
    let (buttons, label_y, width) = match row {
        0 => ([7, 2], 0.0, 236.0),
        1 => ([4, -1], 48.0, 72.0),
        _ => unreachable!(),
    };
    let mut left = right - width;
    sprite(
        textures,
        pass,
        "help_manual",
        vec4(left, y, width, 48.0),
        vec4(0.0, label_y / 96.0, width / 320.0, 0.5),
        1.0,
        ColorBlendType::Layer1,
        canvas,
    );
    left += 2.0;
    for button in buttons {
        if button < 0 {
            continue;
        }
        left -= 48.0;
        sprite(
            textures,
            pass,
            "key",
            vec4(left, y, 48.0, 48.0),
            vec4(
                (button & 7) as f32 / 8.0,
                (button >> 3) as f32 / 2.0,
                0.125,
                0.5,
            ),
            1.0,
            ColorBlendType::Layer1,
            canvas,
        );
    }
    left - 16.0
}

/// Native860b4..860fc: close hint first, then page hint, with a shared slide.
pub fn render_hints(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let y = 1100.0 - 88.0 * model.progress();
    let right = render_hint_row(textures, pass, 1872.0, y, 1, canvas);
    render_hint_row(textures, pass, right, y, 0, canvas);
}

/// Native order: backdrop/bars, clipped pages, caption/close, controller hints.
pub fn render(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    render_background(model, textures, pass, canvas);
    render_pages(model, textures, pass, canvas);
    render_foreground(model, textures, pass, canvas);
    render_hints(model, textures, pass, canvas);
}

#[cfg(test)]
mod tests;
