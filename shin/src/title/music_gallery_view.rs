//! MusicMode artwork309c0. Completion text and captured-scene overlay belong
//! to the resource owner; all geometry here is in the native1920x1080 canvas.
use super::{music_gallery::MusicGallery, view::sprite};
use crate::adv::character_view::CharacterTextures;
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

fn clipped(rect: Vec4, uv: Vec4) -> Option<(Vec4, Vec4)> {
    let crop = vec4(
        rect.x.max(136.0),
        rect.y.max(183.0),
        rect.z.min(1784.0),
        rect.w.min(923.0),
    );
    if crop.x >= crop.z || crop.y >= crop.w {
        return None;
    }
    let map_x = |x| uv.x + (uv.z - uv.x) * (x - rect.x) / (rect.z - rect.x);
    let map_y = |y| uv.y + (uv.w - uv.y) * (y - rect.y) / (rect.w - rect.y);
    Some((
        crop,
        vec4(map_x(crop.x), map_y(crop.y), map_x(crop.z), map_y(crop.w)),
    ))
}

pub(super) fn render_artwork(
    model: &MusicGallery,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let transform = crate::render::top_left_projection_matrix();
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            atlas.get("bg").as_source(),
            transform,
        );
    let list = model.list();
    for (i, row) in list.rows().iter().enumerate() {
        let x = 150.0 + (i % 2) as f32 * 820.0;
        let y = 188.0 + (i / 2) as f32 * 92.0 - list.scroll();
        let (focus, flash) = list.feedback(i);
        let plate = if list.playing() == Some(i) {
            "menu_on"
        } else {
            "menu_off"
        };
        let title = format!("title{}", row.name_index / 32);
        let title_v = (row.name_index % 32) as f32 / 32.0;
        for (name, rect, uv) in [
            (plate, vec4(x, y, x + 800.0, y + 88.0), full),
            (
                title.as_str(),
                vec4(x + 40.0, y + 14.0, x + 760.0, y + 78.0),
                vec4(0.0, title_v, 1.0, title_v + 1.0 / 32.0),
            ),
        ] {
            if let Some((rect, uv)) = clipped(rect, uv) {
                atlas.render_button(
                    pass,
                    name,
                    rect,
                    uv,
                    UnormColor::WHITE,
                    vec4(1.0, 1.0, 1.0, focus * 0.5),
                    vec4(1.0, 1.0, 1.0, flash),
                    transform,
                );
            }
        }
    }
    //30f1c..30ff8 covers the five-pixel clipped edges using matching bg UVs.
    for (top, bottom, a, b) in [(183.0, 188.0, 255.0, 0.0), (918.0, 923.0, 0.0, 255.0)] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            let x = 136.0 + 1648.0 * corner.x;
            let y = top + (bottom - top) * corner.y;
            PosColTexVertex {
                position: vec3(x, y, 0.0),
                texture_position: vec2(x / 1920.0, y / 1080.0),
                color: UnormColor::from_rgba(255, 255, 255, (a + (b - a) * corner.y) as u8),
            }
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: atlas.get("bg").as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
    //eee50 uses the same730-pixel scroller as Tips; Music translates it50px left.
    for (rect, uv) in crate::adv::tips_view::scrollbar_slices(list.content_height(), list.scroll())
    {
        sprite(
            pass,
            system.get("scroll").as_source(),
            rect - vec4(50.0, 0.0, 50.0, 0.0),
            uv,
            UnormColor::WHITE,
            transform,
        );
    }
    atlas.render_button(
        pass,
        "close",
        vec4(1764.0, 32.0, 1848.0, 116.0),
        full,
        UnormColor::WHITE,
        Vec4::ZERO,
        vec4(1.0, 1.0, 1.0, model.close_flash()),
        transform,
    );
    render_footer(model, atlas, pass, transform);
}
fn render_footer(
    model: &MusicGallery,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    //16585c stores first/second icon, labelY and width. fb9f0 returns0,
    // so27320 does not swap authored icons3/4.
    let hints = [
        ([7, 0], 206.0),
        ([4, -1], 72.0),
        ([3, -1], 76.0),
        ([6, -1], 74.0),
    ];
    let list = model.list();
    let mut right = 1872.0;
    for row in [1usize, 3, 2, 0] {
        let (keys, width) = hints[row];
        let enabled = match row {
            3 => list.playing().is_some(),
            2 => list.rows()[list.selected()].unlocked,
            _ => true,
        };
        let tint = if enabled {
            UnormColor::WHITE
        } else {
            UnormColor::from_rgba(128, 128, 128, 255)
        };
        let left = right - width;
        sprite(
            pass,
            atlas.get("help_bgmmode").as_source(),
            vec4(left, 1012.0, right, 1060.0),
            vec4(0.0, row as f32 / 4.0, width / 320.0, (row + 1) as f32 / 4.0),
            tint,
            transform,
        );
        let mut icon_left = left + 2.0;
        for key in keys.into_iter().filter(|key| *key >= 0) {
            icon_left -= 48.0;
            let u = (key & 7) as f32 / 8.0;
            let v = (key >> 3) as f32 / 2.0;
            sprite(
                pass,
                atlas.get("key").as_source(),
                vec4(icon_left, 1012.0, icon_left + 48.0, 1060.0),
                vec4(u, v, u + 0.125, v + 0.5),
                tint,
                transform,
            );
        }
        right = icon_left - 16.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipped_rows_keep_texture_coordinates_at_native_viewport_edges() {
        let (rect, uv) =
            clipped(vec4(150.0, 150.0, 950.0, 238.0), vec4(0.0, 0.0, 1.0, 1.0)).unwrap();
        assert_eq!(rect, vec4(150.0, 183.0, 950.0, 238.0));
        assert_eq!(uv, vec4(0.0, 33.0 / 88.0, 1.0, 1.0));
        let (rect, uv) = clipped(
            vec4(190.0, 894.0, 910.0, 958.0),
            vec4(0.0, 0.5, 1.0, 0.53125),
        )
        .unwrap();
        assert_eq!(rect, vec4(190.0, 894.0, 910.0, 923.0));
        assert_eq!(uv.w, 0.5 + 0.03125 * 29.0 / 64.0);
        assert!(clipped(vec4(150.0, 95.0, 950.0, 183.0), Vec4::ONE).is_none());
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_music_artwork_geometry_and_button_program() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0x30c84, 0x5280668b),
            (0x30c94, 0x91022129),
            (0x30ca4, 0x1e2c1001),
            (0x30cdc, 0x121b5129),
            (0x30ce0, 0x4b09011a),
            (0x30dbc, 0x94025869),
            (0x30dfc, 0x1e2129a1),
            (0x30e00, 0x1e222821),
            (0x30e18, 0x531a6748),
            (0x30e24, 0x1e210801),
            (0x30eb4, 0x9402582b),
            (0x3100c, 0xbd458100),
            (0x31038, 0x9402f786),
            (0x31138, 0x94025776),
        ] {
            assert_eq!(word(a), expected, "native music artwork{a:x}");
        }
        for (a, value) in [
            (0x15997c, 40.0f32),
            (0x159980, 720.0),
            (0x159900, 64.0),
            (0x159940, 88.0),
            (0x15953c, 0.03125),
            (0x159580, 1788.0),
            (0x159480, 1764.0),
            (0x159694, 32.0),
            (0x15971c, 1848.0),
            (0x159658, 116.0),
            (0x1599d0, 1872.0),
            (0x15965c, 1012.0),
        ] {
            assert_eq!(word(a), value.to_bits(), "native music constant{a:x}");
        }
        for (i, row) in [
            [7i32, 0, 0, 206],
            [4, -1, 48, 72],
            [3, -1, 96, 76],
            [6, -1, 144, 74],
        ]
        .iter()
        .enumerate()
        {
            for (j, v) in row.iter().enumerate() {
                assert_eq!(word(0x16585c + i * 16 + j * 4), *v as u32);
            }
        }
    }
}
