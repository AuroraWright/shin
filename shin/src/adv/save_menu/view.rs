//! SaveMenu d7250 artwork and clipping. The screen supplies slot text/previews.
use crate::adv::{
    characters::text::CharacterText,
    characters::view::CharacterTextures,
    save_menu::{SaveMenuModel, SaveMenuPhase},
};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};

pub const CONTENT_CLIP: Vec4 = vec4(136.0, 183.0, 1648.0, 740.0);

#[derive(Debug, Clone, PartialEq)]
pub struct SaveArtwork {
    pub texture: &'static str,
    pub rect: Vec4,
    pub uv: Vec4,
    pub tint: Vec4,
    pub blend: ColorBlendType,
}
impl SaveArtwork {
    fn new(texture: &'static str, rect: Vec4, uv: Vec4) -> Self {
        Self {
            texture,
            rect,
            uv,
            tint: Vec4::ONE,
            blend: ColorBlendType::Layer1,
        }
    }
    pub(crate) fn render(
        &self,
        atlas: &CharacterTextures,
        pass: &mut RenderPass,
        canvas: Mat4,
        builder: RenderRequestBuilder,
    ) {
        QuadVertices::new()
            .with_box(self.rect.x, self.rect.y, self.rect.z, self.rect.w)
            .with_tex_box(self.uv.x, self.uv.y, self.uv.z, self.uv.w)
            .with_color(
                FloatColor4::from_rgba(self.tint.x, self.tint.y, self.tint.z, self.tint.w)
                    .into_unorm(),
            )
            .render_sprite(
                pass,
                builder.color_blend_type(self.blend),
                atlas.get(self.texture).as_source(),
                canvas,
            );
    }
}

pub fn window_slices() -> Vec<SaveArtwork> {
    let xs = [0.0, 80.0, 1744.0, 1824.0];
    let ys = [0.0, 80.0, 810.0, 898.0];
    (0..3)
        .flat_map(|r| {
            (0..3).map(move |c| {
                let mut draw = SaveArtwork::new(
                    "window",
                    vec4(
                        48.0 + xs[c],
                        112.0 + ys[r],
                        48.0 + xs[c + 1],
                        112.0 + ys[r + 1],
                    ),
                    vec4(
                        xs[c] / 1824.0,
                        ys[r] / 898.0,
                        xs[c + 1] / 1824.0,
                        ys[r + 1] / 898.0,
                    ),
                );
                if r == 1 && c == 1 {
                    draw.blend = ColorBlendType::Opaque;
                }
                draw
            })
        })
        .collect()
}

pub fn row_focus() -> Vec<SaveArtwork> {
    let xs = [-24.0, 488.0, 1112.0, 1624.0];
    let ys = [-5.0, 7.0, 155.0, 167.0];
    let us = [0.0, 0.4, 0.6, 1.0];
    let vs = [0.0, 0.375, 0.625, 1.0];
    (0..3)
        .flat_map(|r| {
            (0..3).map(move |c| {
                let mut draw = SaveArtwork::new(
                    "focus",
                    vec4(xs[c], ys[r], xs[c + 1], ys[r + 1]),
                    vec4(us[c], vs[r], us[c + 1], vs[r + 1]),
                );
                draw.tint.w = 0.75;
                draw
            })
        })
        .collect()
}

pub fn row_divider() -> Vec<SaveArtwork> {
    let xs = [-24.0, -16.0, 1616.0, 1624.0];
    let us = [0.0, 0.25, 0.75, 1.0];
    (0..3)
        .map(|c| {
            SaveArtwork::new(
                "line",
                vec4(xs[c], -19.0, xs[c + 1], -5.0),
                vec4(us[c], 0.0, us[c + 1], 1.0),
            )
        })
        .collect()
}

/// Native d8550 uses36x48 glyphs with28px advance and x-4/y+2 overhang.
pub fn slot_number(slot: usize) -> Vec<SaveArtwork> {
    let text = format!("{}.", slot + 1);
    let mut x = 316.0;
    text.bytes()
        .map(|ch| {
            let (u, width) = if ch == b'.' {
                (396.0, 18.0)
            } else {
                ((ch - b'0') as f32 * 36.0, 36.0)
            };
            let left = x;
            x += width - 8.0;
            SaveArtwork::new(
                "num",
                vec4(left, 2.0, left + width, 50.0),
                vec4(u / 512.0, 0.4, (u + width) / 512.0, 1.0),
            )
        })
        .collect()
}

/// Native d8770 date glyphs24x32,19px advance; slash24x32, colon14x32.
pub fn slot_date(game: &shin_core::format::save::GameData) -> Vec<SaveArtwork> {
    let text = game.date_time.format("%Y/%m/%d %_H:%M").to_string();
    let width: f32 = text
        .bytes()
        .map(|c| {
            if c.is_ascii_digit() {
                19.0
            } else if c == b'/' {
                19.0
            } else if c == b':' {
                9.0
            } else {
                16.0
            }
        })
        .sum();
    let mut x = 1592.0 - width;
    let mut result = Vec::new();
    for c in text.bytes() {
        let spec = if c.is_ascii_digit() {
            Some(((c - b'0') as f32 * 24.0, 24.0))
        } else if c == b'/' {
            Some((240.0, 24.0))
        } else if c == b':' {
            Some((264.0, 14.0))
        } else {
            None
        };
        if let Some((u, w)) = spec {
            result.push(SaveArtwork::new(
                "num",
                vec4(x - 2.5, 18.0, x - 2.5 + w, 50.0),
                vec4(u / 512.0, 0.0, (u + w) / 512.0, 0.4),
            ));
            x += w - 5.0;
        } else {
            x += 16.0;
        }
    }
    result
}

fn render_edges(atlas: &CharacterTextures, pass: &mut RenderPass, transform: Mat4) {
    for (y1, y2, v1, v2, a, b) in [
        (183.0, 188.0, 71.0 / 898.0, 76.0 / 898.0, 1.0, 0.0),
        (918.0, 923.0, 806.0 / 898.0, 811.0 / 898.0, 0.0, 1.0),
    ] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|p| PosColTexVertex {
            position: vec3(136.0 + 1648.0 * p.x, y1 + (y2 - y1) * p.y, 0.0),
            texture_position: vec2(88.0 / 1824.0 + 1648.0 / 1824.0 * p.x, v1 + (v2 - v1) * p.y),
            color: FloatColor4::from_rgba(1.0, 1.0, 1.0, a + (b - a) * p.y).into_unorm(),
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: atlas.get("window").as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

fn footer(model: &SaveMenuModel) -> Vec<SaveArtwork> {
    let y = 1100.0 - 88.0 * model.progress();
    let occupied = model.slots()[model.selected()].is_some();
    let rows: Vec<(usize, bool)> = if model.is_save() {
        vec![(1, true), (2, true), (4, occupied), (0, true)]
    } else {
        vec![(1, true), (3, occupied), (0, true)]
    };
    let table = [
        ([7, 0], 206.0),
        ([4, -1], 72.0),
        ([3, -1], 110.0),
        ([3, -1], 106.0),
        ([6, -1], 74.0),
    ];
    let mut right = 1872.0;
    let mut result = Vec::new();
    for (row, enabled) in rows {
        let (keys, width) = table[row];
        let left = right - width;
        let mut label = SaveArtwork::new(
            "help_saveload",
            vec4(left, y, right, y + 48.0),
            vec4(0.0, row as f32 / 5.0, width / 320.0, (row + 1) as f32 / 5.0),
        );
        if !enabled {
            label.tint = vec4(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0);
        }
        let tint = label.tint;
        result.push(label);
        let mut x = left + 2.0;
        for key in keys.into_iter().filter(|&k| k >= 0) {
            x -= 48.0;
            let mut icon = SaveArtwork::new(
                "key",
                vec4(x, y, x + 48.0, y + 48.0),
                vec4(
                    (key & 7) as f32 / 8.0,
                    (key >> 3) as f32 / 2.0,
                    (key & 7) as f32 / 8.0 + 0.125,
                    (key >> 3) as f32 / 2.0 + 0.5,
                ),
            );
            icon.tint = tint;
            result.push(icon);
        }
        right = x - 16.0;
    }
    result
}

/// The slot callback draws authored text and a reconstructed preview and returns
/// true if the preview was ready. Native nodata artwork covers a pending preview.
pub fn render(
    model: &SaveMenuModel,
    atlas: &CharacterTextures,
    system: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
    mut slot_content: impl FnMut(&mut RenderPass, usize, Mat4, RenderRequestBuilder) -> bool,
) {
    if matches!(
        model.phase(),
        SaveMenuPhase::Loading | SaveMenuPhase::Closed
    ) {
        return;
    }
    let p = model.progress();
    let top = -160.0 + 160.0 * p;
    let normal = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
    let mut bg = SaveArtwork::new(
        "bg",
        vec4(0.0, 0.0, 1920.0, 1080.0),
        vec4(0.0, 0.0, 1.0, 1.0),
    );
    bg.tint.w = p;
    bg.render(atlas, pass, canvas, normal);
    for (rect, uv) in [
        (
            vec4(0.0, top, 1920.0, top + 160.0),
            vec4(0.0, 0.0, 1.0, 0.5),
        ),
        (
            vec4(0.0, 1080.0 - 160.0 * p, 1920.0, 1240.0 - 160.0 * p),
            vec4(0.0, 0.5, 1.0, 1.0),
        ),
    ] {
        let mut obi = SaveArtwork::new("obi", rect, uv);
        obi.tint.w = 0.25;
        obi.blend = ColorBlendType::Layer2;
        obi.render(atlas, pass, canvas, normal);
    }
    let body = canvas * Mat4::from_translation(vec3(1920.0 * (1.0 - p), 0.0, 0.0));
    for draw in window_slices() {
        draw.render(atlas, pass, body, normal);
    }
    let (clip, builder) = CharacterText::begin_clip(pass, body, CONTENT_CLIP);
    for slot in model.visible_slots() {
        let transform = body
            * Mat4::from_translation(vec3(
                160.0,
                193.0 + slot as f32 * 186.0 - model.scroll(),
                0.0,
            ));
        if slot > 0 {
            for draw in row_divider() {
                draw.render(atlas, pass, transform, builder);
            }
        }
        if slot == model.selected() {
            for draw in row_focus() {
                draw.render(atlas, pass, transform, builder);
            }
        }
        let mut bar = SaveArtwork::new(
            "bar",
            vec4(32.0, 0.0, 1600.0, 48.0),
            vec4(0.0, 0.0, 1.0, 1.0),
        );
        bar.blend = ColorBlendType::Opaque;
        bar.render(atlas, pass, transform, builder);
        for draw in slot_number(slot) {
            draw.render(atlas, pass, transform, builder);
        }
        if let Some(game) = &model.slots()[slot] {
            for draw in slot_date(game) {
                draw.render(atlas, pass, transform, builder);
            }
        }
        if !slot_content(pass, slot, transform, builder) {
            let mut nodata = SaveArtwork::new(
                "nodata",
                vec4(6.0, 0.0, 294.0, 162.0),
                vec4(0.0, 0.0, 1.0, 1.0),
            );
            nodata.blend = ColorBlendType::Opaque;
            nodata.render(atlas, pass, transform, builder);
        }
        if model.row_flash(slot) > 0.0 {
            let vertices = shin_render::quad_vertices::build_quad_vertices(|p| PosColVertex {
                position: vec3(-24.0 + 1648.0 * p.x, -5.0 + 172.0 * p.y, 0.0),
                color: FloatColor4::from_rgba(1.0, 1.0, 1.0, model.row_flash(slot)).into_unorm(),
            });
            pass.run(builder.color_blend_type(ColorBlendType::Layer1).build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
    clip.end(pass);
    render_edges(atlas, pass, body);
    for (rect, uv) in scrollbar_slices(model.scroll()) {
        SaveArtwork::new("scroll", rect, uv).render(system, pass, body, normal);
    }
    SaveArtwork::new(
        if model.is_save() {
            "caption_save"
        } else {
            "caption_load"
        },
        vec4(480.0, top, 1440.0, top + 128.0),
        vec4(0.0, 0.0, 1.0, 1.0),
    )
    .render(atlas, pass, canvas, normal);
    QuadVertices::new()
        .with_box(1764.0, top + 32.0, 1848.0, top + 116.0)
        .with_tex_box(0.0, 0.0, 1.0, 1.0)
        .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, 1.0).into_unorm())
        .render_button(
            pass,
            normal,
            atlas.get("close").as_source(),
            canvas,
            Vec4::ZERO,
            vec4(1.0, 1.0, 1.0, model.close_flash()),
        );
    for draw in footer(model) {
        draw.render(atlas, pass, canvas, normal);
    }
}

/// Shared native eee50 vertical scroller, located50px left of the Tips scroller.
pub fn scrollbar_slices(scroll: f32) -> Vec<(Vec4, Vec4)> {
    crate::adv::tips::view::scrollbar_slices(18586.0, scroll)
        .into_iter()
        .map(|(r, uv)| (r - vec4(50.0, 0.0, 50.0, 0.0), uv))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_number_and_date_overhang_and_alignment() {
        let one = slot_number(0);
        assert_eq!(one.len(), 2);
        assert_eq!(one[0].rect, vec4(316.0, 2.0, 352.0, 50.0));
        assert_eq!(one[0].uv, vec4(36.0 / 512.0, 0.4, 72.0 / 512.0, 1.0));
        assert_eq!(one[1].rect, vec4(344.0, 2.0, 362.0, 50.0));
        assert_eq!(one[1].uv, vec4(396.0 / 512.0, 0.4, 414.0 / 512.0, 1.0));
        let hundred = slot_number(99);
        assert_eq!(hundred.len(), 4);
        assert_eq!(hundred[3].rect.x, 400.0);
        let mut game = crate::adv::VmState::new().snapshot_game_data();
        game.date_time = "2026-10-07T03:04:05".parse().unwrap();
        let date = slot_date(&game);
        assert_eq!(
            date.len(),
            14,
            "date contains12 digits,two slashes andone colon minus space-padded hour digit"
        );
        assert_eq!(date.last().unwrap().rect, vec4(1570.5, 18.0, 1594.5, 50.0));
        assert_eq!(date.iter().filter(|s| s.uv.x == 240.0 / 512.0).count(), 2);
        assert_eq!(date.iter().filter(|s| s.uv.x == 264.0 / 512.0).count(), 1);
    }

    #[test]
    fn footer_groups_preserve_mode_occupancy_and_two_cursor_icons() {
        let data = shin_core::format::save::SaveData::default();
        let save = SaveMenuModel::new(&data, Some(crate::adv::VmState::new().snapshot_game_data()));
        let load = SaveMenuModel::new(&data, None);
        let save_draws = footer(&save);
        let load_draws = footer(&load);
        assert_eq!(save_draws.len(), 9);
        assert_eq!(load_draws.len(), 7);
        assert_eq!(save_draws[0].rect, vec4(1800.0, 1100.0, 1872.0, 1148.0));
        assert_eq!(
            save_draws[4].tint,
            vec4(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0)
        );
        assert_eq!(load_draws[2].tint, save_draws[4].tint);
        assert_eq!(save_draws[8].rect.x, save_draws[7].rect.x - 48.0);
        assert_eq!(window_slices()[4].blend, ColorBlendType::Opaque);
        assert_eq!(window_slices()[8].rect, vec4(1792.0, 922.0, 1872.0, 1010.0));
        assert_eq!(CONTENT_CLIP, vec4(136.0, 183.0, 1648.0, 740.0));
    }
}
