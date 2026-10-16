//! Native row separators (a5f20) and selected-row focus patches (a6090).
use crate::adv::{backlog::model::Row, backlog::view::BacklogTextures};
use crate::{
    layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams},
    render::{centered_projection_matrix, top_left_projection_matrix},
};
use glam::{Mat4, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    quad_vertices::{QuadVertices, build_quad_vertices},
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

pub(crate) fn render_row(
    pass: &mut RenderPass,
    textures: &BacklogTextures,
    row: Row,
    scroll: f32,
    progress: f32,
    selected: bool,
) {
    let (parent, builder, clip) = begin_content(pass, progress);
    let y = 188.0 + row.top - scroll;
    let xs = [136.0, 144.0, 1776.0, 1784.0];
    let us = [0.0, 0.25, 0.75, 1.0];
    // Native separator is drawn even after the last row; viewport clips it.
    for column in 0..3 {
        QuadVertices::new()
            .with_box(
                xs[column],
                y + row.height,
                xs[column + 1],
                y + row.height + 14.0,
            )
            .with_tex_box(us[column], 0.0, us[column + 1], 1.0)
            .render_sprite(pass, builder, textures.line.as_source(), parent);
    }
    if selected {
        let xs = [136.0, 648.0, 1272.0, 1784.0];
        let ys = [y, y + 12.0, y + row.height - 12.0, y + row.height];
        let us = [0.0, 0.4, 0.6, 1.0];
        let vs = [0.0, 0.375, 0.625, 1.0];
        for row in 0..3 {
            for column in 0..3 {
                QuadVertices::new()
                    .with_box(xs[column], ys[row], xs[column + 1], ys[row + 1])
                    .with_tex_box(us[column], vs[row], us[column + 1], vs[row + 1])
                    .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, 0.75).into_unorm())
                    .render_sprite(pass, builder, textures.focus.as_source(), parent);
            }
        }
    }
    clip.end(pass);
}

fn begin_content(
    pass: &mut RenderPass,
    progress: f32,
) -> (Mat4, RenderRequestBuilder, DrawableClipParams) {
    let parent = top_left_projection_matrix()
        * Mat4::from_translation(vec3(0.0, 1920.0 * (1.0 - progress), 0.0));
    let clip = DrawableClipParams {
        mode: DrawableClipMode::Clip,
        area: vec4(136.0, 188.0, 1648.0, 730.0),
    };
    clip.begin(
        pass,
        &TransformParams {
            transform: centered_projection_matrix().inverse() * parent,
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
    (parent, builder, clip)
}

/// a5254..a52b8 fills the entire row white after text and voice; the separator
/// stays outside the flash. The browser viewport clips the same content pass.
pub(crate) fn render_confirmation_flash(
    pass: &mut RenderPass,
    row: Row,
    scroll: f32,
    progress: f32,
    flash: f32,
) {
    if flash <= 0.0 {
        return;
    }
    let (parent, builder, clip) = begin_content(pass, progress);
    let y = 188.0 + row.top - scroll;
    QuadVertices::new()
        .with_box(136.0, y, 1784.0, y + row.height)
        .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, flash).into_unorm())
        .fill(pass, builder, parent);
    clip.end(pass);
}

/// a51b4..a5250: 108x76 image in the left gutter, vertically centered.
/// Program5 recolors sampled RGB toward white but preserves texture alpha.
pub(crate) fn render_voice(
    pass: &mut RenderPass,
    textures: &BacklogTextures,
    row: Row,
    scroll: f32,
    progress: f32,
    flash: f32,
) {
    let (parent, builder, clip) = begin_content(pass, progress);
    let top = 188.0 + row.top - scroll + (row.height - 76.0) * 0.5;
    let vertices = build_quad_vertices(|corner| PosColTexVertex {
        position: vec3(168.0 + 108.0 * corner.x, top + 76.0 * corner.y, 0.0),
        texture_position: corner,
        color: shin_core::primitives::color::UnormColor::WHITE,
    });
    pass.run(builder.build(
        RenderProgramWithArguments::Button {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            texture: textures.voice.as_source(),
            transform: parent,
            flash1: glam::Vec4::ZERO,
            flash2: vec4(1.0, 1.0, 1.0, flash),
        },
        DrawPrimitive::TrianglesStrip,
    ));
    clip.end(pass);
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_voice_icon_and_feedback() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xa5180, 0x350006a8u32),
            (0xa518c, 0x34000648u32),
            (0xa519c, 0x912a6021u32),
            (0xa51bc, 0x1e212800u32),
            (0xa51c0, 0x1e2c1001u32),
            (0xa51cc, 0x1e202823u32),
            (0xa51d8, 0x940086b6u32),
            (0xa5224, 0x91024340u32),
            (0xa5250, 0x94008730u32),
            (0xa6edc, 0x34fffec8u32),
            (0xa6f24, 0x91024100u32),
            (0xa6f28, 0x9400513eu32),
            (0xa6f30, 0xbd45a500u32),
            (0xa6f38, 0xbd497508u32),
            (0xa6f40, 0x321a03e1u32),
            (0xa6f48, 0x940052aeu32),
            (0xa6f4c, 0x1e2703e0u32),
            (0xa6f5c, 0x940052a9u32),
            (0xa400c, 0x910242e0u32),
            (0xa4010, 0x94005e90u32),
            (0xc7054, 0x940021d3u32),
            (0xc7064, 0x940021f7u32),
            (0xc7074, 0x940021f3u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native voice icon {address:x}"
            );
        }
        for (address, value) in [
            (0x159528, 108.0f32),
            (0x159694, 32.0),
            (0x1596ac, -76.0),
            (0x159904, 76.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        assert_eq!(&exe[0x15da98 + 256..0x15da9d + 256], b"voice");
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_row_focus_and_separator() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xa4fa0, 0x940003e0u32),
            (0xa4fac, 0x6b09011fu32),
            (0xa4fdc, 0x1e2d1003u32),
            (0xa4ff8, 0x94000426u32),
            (0xa5f58, 0x1e241000u32),
            (0xa5f68, 0x1e341001u32),
            (0xa5f80, 0x913db421u32),
            (0xa5fa4, 0x1e259000u32),
            (0xa5fc8, 0x1e2a100cu32),
            (0xa600c, 0x1e2d100au32),
            (0xa60dc, 0xbd4a2d02u32),
            (0xa60ec, 0x1e251002u32),
            (0xa6120, 0x1e351000u32),
            (0xa6134, 0x91128c21u32),
            (0xa6184, 0xbd490d09u32),
            (0xa618c, 0x1e2b1003u32),
            (0xa61cc, 0xbd45390bu32),
            (0xa6250, 0x1e2c9003u32),
            (0xa62c8, 0x1e2c900fu32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native row decoration {address:x}"
            );
        }
        for (address, value) in [
            (0x159a74, 1648.0f32),
            (0x159a2c, 512.0),
            (0x159630, -512.0),
            (0x15990c, 0.4),
            (0x159538, 0.6),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        for (address, name) in [(0x15df6d, "line"), (0x15a4a3, "focus")] {
            assert_eq!(
                &exe[address + 256..address + 256 + name.len()],
                name.as_bytes()
            );
        }
    }
}
