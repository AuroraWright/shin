//! Updated77254 content order: retained rows, voice feedback, deduplicated
//! boundaries and the vertical scrollbar. Shell/child drawing is separate.
use super::{
    browser_controls::Scrollbar,
    browser_decorations::{self as decorations, parent_transform},
    browser_glyphs::{BrowserGlyphCache, PositionedRow, native_transform},
    browser_history::Item,
};
use crate::{
    adv::characters::view::CharacterTextures, layer::render_params::higurashi::HigurashiTransform,
};
use glam::{Mat4, Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

pub(super) enum Command<'a> {
    Cursor { row: &'a PositionedRow, alpha: f32 },
    Glyphs(&'a PositionedRow),
    Voice { row: &'a PositionedRow, flash: f32 },
    Line(f32),
}
/// The original cache vector is stable across frames and newly prepared rows
/// append. Its order also determines which copy of a shared boundary draws.
fn visit<'a>(
    cache: &BrowserGlyphCache,
    rows: &'a [PositionedRow],
    scroll: f32,
    selected: usize,
    flashes: impl Fn(u32) -> (f32, f32),
    mut emit: impl FnMut(Command<'a>),
) {
    let retained: Vec<_> = cache
        .indices()
        .filter_map(|i| rows.iter().find(|r| r.row.index == i))
        .collect();
    for &row in &retained {
        if row.row.index as usize == selected {
            emit(Command::Cursor { row, alpha: 0.5 });
        }
        emit(Command::Glyphs(row));
        let (jump, voice) = flashes(row.row.index);
        if matches!(row.row.item, Item::Message { .. }) && row.voice_count != 0 {
            emit(Command::Voice { row, flash: voice });
        }
        if jump > 0.0 {
            emit(Command::Cursor { row, alpha: jump });
        }
    }
    let mut drawn = Vec::new();
    for row in retained {
        for y in [
            row.top - (scroll - 162.0),
            (row.top + row.height) - (scroll - 162.0),
        ] {
            if (-4.0..1084.0).contains(&y) && !drawn.contains(&y) {
                emit(Command::Line(y));
                drawn.push(y);
            }
        }
    }
}
fn cursor_rect(row: &PositionedRow, scroll: f32) -> Vec4 {
    vec4(147.0, row.top - (scroll - 162.0), 1626.0, row.height)
}
fn voice_vertices(height: f32) -> [PosColTexVertex; 4] {
    let y = ((height + -36.0) + -76.0) * 0.5;
    decorations::quad(
        vec4(0.0, y, 76.0, y + 76.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        UnormColor::WHITE,
    )
}
fn line_vertices(y: f32) -> [[PosColTexVertex; 4]; 3] {
    let xs = [147.0, 155.0, 1765.0, 1773.0];
    let uv = [0.0, 0.25, 0.75, 1.0];
    std::array::from_fn(|i| {
        decorations::quad(
            vec4(xs[i], y + -8.0, xs[i + 1], y + 8.0),
            vec4(uv[i], 0.0, uv[i + 1], 1.0),
            UnormColor::WHITE,
        )
    })
}
fn render_voice(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    row: &PositionedRow,
    scroll: f32,
    slide: f32,
    flash: f32,
) {
    let vertices = voice_vertices(row.height);
    let matrix = native_transform(row.top, scroll, slide);
    if flash > 0.0 {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Button {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: atlas.get("voice").as_source(),
                        transform: HigurashiTransform::convert_clip_depth(matrix),
                        // Updated9ba18 uses one flash vector. Desktop Button's second
                        // vector carries it; the first is neutral.
                        flash1: Vec4::ZERO,
                        flash2: vec4(1.0, 1.0, 1.0, flash),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    } else {
        decorations::sprite(pass, atlas, "voice", &vertices, matrix);
    }
}
pub(super) fn render_rows(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    cache: &BrowserGlyphCache,
    rows: &[PositionedRow],
    scroll: f32,
    slide: f32,
    selected: usize,
    flashes: impl Fn(u32) -> (f32, f32),
) {
    let matrix = parent_transform(slide);
    visit(
        cache,
        rows,
        scroll,
        selected,
        flashes,
        |command| match command {
            Command::Cursor { row, alpha } => {
                decorations::render_cursor(pass, atlas, cursor_rect(row, scroll), alpha, matrix)
            }
            Command::Glyphs(row) => cache
                .get(row.row.index)
                .unwrap()
                .render(pass, row.top, scroll, slide),
            Command::Voice { row, flash } => render_voice(pass, atlas, row, scroll, slide, flash),
            Command::Line(y) => {
                for vertices in line_vertices(y) {
                    decorations::sprite(pass, atlas, "line", &vertices, matrix);
                }
            }
        },
    );
}
pub(super) fn render(
    pass: &mut RenderPass,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    cache: &BrowserGlyphCache,
    rows: &[PositionedRow],
    scroll: f32,
    slide: f32,
    selected: usize,
    flashes: impl Fn(u32) -> (f32, f32),
    scrollbar: Scrollbar,
) {
    render_rows(pass, atlas, cache, rows, scroll, slide, selected, flashes);
    decorations::render_scrollbar(pass, system, scrollbar, slide);
}

#[cfg(test)]
mod tests;
