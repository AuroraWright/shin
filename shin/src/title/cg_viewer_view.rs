//! Native CG viewer 4b6c0 variant indicators, drawn after the captured-scene fade.
use super::{cg_gallery::CgRow, cg_viewer::CgViewerNavigation, view::sprite};
use crate::adv::character_view::CharacterTextures;
use glam::{Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::render_pass::RenderPass;

fn indicators(model: &CgViewerNavigation, row: &CgRow) -> Vec<(Vec4, UnormColor)> {
    if !model.indicators() || row.pictures.len() < 2 {
        return Vec::new();
    }
    let left = 1905.0 - row.pictures.len() as f32 * 30.0;
    (0..row.pictures.len())
        .map(|i| {
            let x = left + i as f32 * 30.0;
            let color = if model.ordinal() == i as i32 {
                UnormColor::from_rgba(144, 255, 128, 255)
            } else if row.unlocked[i] {
                UnormColor::WHITE
            } else {
                UnormColor::from_rgba(128, 128, 128, 255)
            };
            (vec4(x, 1025.0, x + 32.0, 1065.0), color)
        })
        .collect()
}
pub(super) fn render_indicators(
    model: &CgViewerNavigation,
    row: &CgRow,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let transform = crate::render::top_left_projection_matrix();
    for (rect, color) in indicators(model, row) {
        sprite(
            pass,
            atlas.get("check").as_source(),
            rect,
            vec4(0.0, 0.0, 1.0, 1.0),
            color,
            transform,
        );
    }
}

#[cfg(test)]
mod tests;
