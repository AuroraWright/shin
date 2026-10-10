//! CGMode 48350 artwork. Completion text and captured frame belong to the scene owner.
use super::{
    cg_gallery::CgGalleryList,
    view::{rgba, sprite},
};
use crate::adv::character_view::CharacterTextures;
use glam::{Vec4, vec2, vec3, vec4};
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
        rect.y.max(185.0),
        rect.z.min(1784.0),
        rect.w.min(921.0),
    );
    if crop.x >= crop.z || crop.y >= crop.w {
        return None;
    }
    let x = |v| uv.x + (uv.z - uv.x) * (v - rect.x) / (rect.z - rect.x);
    let y = |v| uv.y + (uv.w - uv.y) * (v - rect.y) / (rect.w - rect.y);
    Some((crop, vec4(x(crop.x), y(crop.y), x(crop.z), y(crop.w))))
}
fn thumbnail(index: usize, scroll: f32, focus: f32) -> Vec4 {
    let scale = 1.0 + focus * 0.05;
    let cx = 342.0 + (index % 4) as f32 * 412.0;
    let cy = 311.0 + (index / 4) as f32 * 242.0 - scroll;
    vec4(
        cx - 192.0 * scale,
        cy - 108.0 * scale,
        cx + 192.0 * scale,
        cy + 108.0 * scale,
    )
}
/// 48c70 right-aligns advances (glyph width minus five) then draws at x-2.5,y+2.
fn variant_glyphs(unlocked: usize, total: usize, rect: Vec4) -> Vec<(Vec4, Vec4)> {
    let text = format!("{unlocked}/{total}");
    let mut x = 376.0 - text.len() as f32 * 19.0;
    let scale = (rect.z - rect.x) / 384.0;
    text.bytes()
        .map(|ch| {
            let u = if ch == b'/' {
                240.0
            } else {
                (ch - b'0') as f32 * 24.0
            };
            let dst = vec4(
                rect.x + (x - 2.5) * scale,
                rect.y + 180.0 * scale,
                rect.x + (x + 21.5) * scale,
                rect.y + 212.0 * scale,
            );
            x += 19.0;
            (dst, vec4(u / 512.0, 0.0, (u + 24.0) / 512.0, 32.0 / 80.0))
        })
        .collect()
}
fn scrollbar(height: f32, scroll: f32) -> Vec<(Vec4, Vec4)> {
    let height = height.ceil();
    let scroll = scroll.trunc();
    if height <= 726.0 || scroll < 0.0 || scroll + 726.0 > height {
        return Vec::new();
    }
    let start = 710.0 * scroll / height;
    let end = 710.0 * (scroll + 726.0) / height;
    let mut slices = Vec::new();
    for (u, ys) in [
        (0.0, [190.0, 198.0, 908.0, 916.0]),
        (
            0.5,
            [190.0 + start, 198.0 + start, 198.0 + end, 206.0 + end],
        ),
    ] {
        for band in 0..3 {
            slices.push((
                vec4(1788.0, ys[band], 1804.0, ys[band + 1]),
                vec4(u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25),
            ));
        }
    }
    slices
}
pub(super) fn render_artwork(
    model: &CgGalleryList,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let transform = crate::render::top_left_projection_matrix();
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .render_sprite(pass, opaque, atlas.get("bg").as_source(), transform);
    for (i, row) in model.rows().iter().enumerate() {
        let (focus, flash) = model.feedback(i);
        let rect = thumbnail(i, model.scroll(), focus);
        let Some((crop, uv)) = clipped(rect, full) else {
            continue;
        };
        let quad = QuadVertices::new().with_box(crop.x, crop.y, crop.z, crop.w);
        // 4880c draws a float Button flash on the opaque thumbnail before
        // the locked/count overlay and the later byte-packed flat flashes.
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: vec3(
                crop.x + (crop.z - crop.x) * corner.x,
                crop.y + (crop.w - crop.y) * corner.y,
                0.0,
            ),
            texture_position: vec2(
                uv.x + (uv.z - uv.x) * corner.x,
                uv.y + (uv.w - uv.y) * corner.y,
            ),
            color: UnormColor::WHITE,
        });
        pass.run(opaque.build(
            RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: atlas.get(&row.thumbnail).as_source(),
                transform,
                flash1: Vec4::ZERO,
                flash2: vec4(1.0, 1.0, 1.0, flash),
            },
            DrawPrimitive::TrianglesStrip,
        ));
        let unlocked = row.unlocked_count();
        if unlocked > 0 && unlocked < row.pictures.len() {
            for (dst, uv) in variant_glyphs(unlocked, row.pictures.len(), rect) {
                if let Some((dst, uv)) = clipped(dst, uv) {
                    sprite(
                        pass,
                        atlas.get("num").as_source(),
                        dst,
                        uv,
                        UnormColor::WHITE,
                        transform,
                    );
                }
            }
        }
        if unlocked == 0 {
            sprite(
                pass,
                atlas.get("logo").as_source(),
                crop,
                uv,
                UnormColor::WHITE,
                transform,
            );
        }
        for alpha in [focus * 0.5, flash] {
            if alpha > 0.0 {
                quad.with_color(rgba(1.0, 1.0, 1.0, alpha)).fill(
                    pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                    transform,
                );
            }
        }
    }
    for (top, bottom, a, b) in [(185.0, 190.0, 255.0, 0.0), (916.0, 921.0, 0.0, 255.0)] {
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
    for (rect, uv) in scrollbar(model.content_height(), model.scroll()) {
        sprite(
            pass,
            system.get("scroll").as_source(),
            rect,
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
    let mut right = 1872.0;
    for (row, keys, width) in [(1, [4, -1], 72.0), (2, [3, -1], 74.0), (0, [7, 0], 206.0)] {
        let tint = if row == 2 && model.rows()[model.selected()].unlocked_count() == 0 {
            UnormColor::from_rgba(128, 128, 128, 255)
        } else {
            UnormColor::WHITE
        };
        let left = right - width;
        sprite(
            pass,
            atlas.get("help_cgmode").as_source(),
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
mod tests;
