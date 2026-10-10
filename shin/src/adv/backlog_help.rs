//! Native MessageBrowser footer (a5600/a63c0), using the supplied atlas.
use super::backlog_view::BacklogTextures;
use crate::render::top_left_projection_matrix;
use glam::vec4;
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

// Native171498: primary key, secondary key, atlas Y, label width.
const HINTS: [(i32, i32, f32, f32); 5] = [
    (7, 0, 0.0, 206.0),
    (4, -1, 48.0, 72.0),
    (3, -1, 96.0, 140.0),
    (6, -1, 144.0, 240.0),
    (5, -1, 192.0, 266.0),
];
pub(super) fn render(
    pass: &mut RenderPass,
    textures: &BacklogTextures,
    progress: f32,
    replay_enabled: bool,
    menu_open: bool,
) {
    let top = 1100.0 - 88.0 * progress;
    let mut right = 1872.0;
    let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
    for id in [1, 2, 4, 3, 0] {
        // Browser+198 suppresses the chapter-list hint while its child exists.
        if id == 4 && menu_open {
            continue;
        }
        let (primary, secondary, atlas_y, width) = HINTS[id];
        let color = if id == 2 && !replay_enabled {
            UnormColor::from_rgba(128, 128, 128, 255)
        } else {
            UnormColor::WHITE
        };
        let left = right - width;
        QuadVertices::new()
            .with_box(left, top, right, top + 48.0)
            .with_tex_box(
                0.0,
                atlas_y / 288.0,
                width / 320.0,
                (atlas_y + 48.0) / 288.0,
            )
            .with_color(color)
            .render_sprite(
                pass,
                builder,
                textures.help_logview.as_source(),
                top_left_projection_matrix(),
            );
        let mut icon_left = left + 2.0;
        for key in [primary, secondary] {
            if key < 0 {
                continue;
            }
            icon_left -= 48.0;
            let u = (key & 7) as f32 / 8.0;
            let v = (key >> 3) as f32 / 2.0;
            QuadVertices::new()
                .with_box(icon_left, top, icon_left + 48.0, top + 48.0)
                .with_tex_box(u, v, u + 0.125, v + 0.5)
                .with_color(color)
                .render_sprite(
                    pass,
                    builder,
                    textures.key.as_source(),
                    top_left_projection_matrix(),
                );
        }
        right = icon_left - 16.0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_footer_metadata_visibility_and_slide() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (i, (primary, secondary, y, width)) in HINTS.iter().enumerate() {
            let at = 0x171498 + 256 + i * 16;
            let actual: [i32; 4] = std::array::from_fn(|j| {
                i32::from_le_bytes(exe[at + j * 4..at + j * 4 + 4].try_into().unwrap())
            });
            assert_eq!(actual, [*primary, *secondary, *y as i32, *width as i32]);
        }
        for (a, value) in [
            (0x1599d0, 1872.0f32),
            (0x159614, 1080.0),
            (0x159a24, -88.0),
            (0x159584, 48.0),
            (0x159444, 320.0),
            (0x159a78, 288.0),
            (0x159540, 1.0 / 6.0),
            (0x1599c4, -48.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                value
            );
        }
        for (a, word) in [
            (0xa567c, 0xb9419a68u32),
            (0xa6a5c, 0x375805c9),
            (0xa6a60, 0x36500729),
            (0xa5680, 0x350000c8),
            (0xa5648, 0x3400008a),
            (0xa5668, 0x12800001),
            (0xa65d0, 0x1e361000),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                word,
                "native footer{a:x}"
            );
        }
    }
}
