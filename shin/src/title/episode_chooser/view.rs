//! Chooser rows10f6c0, rendered after Title black and before retained viewers.
use crate::title::{
    episode_chooser::EpisodeChooser,
    view::{fill, rgba, sprite},
};
use crate::adv::characters::view::CharacterTextures;
use glam::{Mat4, vec4};
use shin_render::{render_pass::RenderPass, shaders::types::texture::TextureSource};

pub(crate) fn render(
    chooser: &EpisodeChooser,
    atlas: &CharacterTextures,
    background: TextureSource<'_>,
    pass: &mut RenderPass,
) {
    let transform: Mat4 = crate::render::top_left_projection_matrix();
    for (episode, row) in chooser.rows() {
        if row.opacity.abs() < f32::EPSILON {
            continue;
        }
        let body = vec4(row.x, row.y, row.x + 400.0, row.y + 72.0);
        let r = 0.2157 + (1.0 - 0.2157) * row.focus;
        let gb = 0.1059 + (0.1608 - 0.1059) * row.focus;
        fill(
            pass,
            vec4(row.x, row.y + 1.0, row.x + 400.0, row.y + 71.0),
            rgba(r, gb, gb, 1.0),
            transform,
        );
        sprite(
            pass,
            atlas.get("menu2").as_source(),
            vec4(row.x + 80.0, row.y + 4.0, row.x + 400.0, row.y + 68.0),
            vec4(0.0, episode as f32 / 8.0, 1.0, (episode as f32 + 1.0) / 8.0),
            shin_core::primitives::color::UnormColor::WHITE,
            transform,
        );
        if row.flash > 0.0 {
            fill(pass, body, rgba(1.0, 1.0, 1.0, row.flash), transform);
        }
        if row.opacity < 1.0 {
            sprite(
                pass,
                background,
                body,
                vec4(
                    row.x / 1920.0,
                    row.y / 1080.0,
                    (row.x + 400.0) / 1920.0,
                    (row.y + 72.0) / 1080.0,
                ),
                rgba(1.0, 1.0, 1.0, 1.0 - row.opacity),
                transform,
            );
        }
    }
}
