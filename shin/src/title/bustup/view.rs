//! Native BustupMode38940 menu artwork and39330..39ae0 draw helpers.
//! The live owner renders its background/BUP group before this artwork and its
//! child tasks/capture after it. Controls visibility is scene tweenerF0.
use crate::title::{
    bustup::catalog::BustupCatalog,
    bustup::menu::BustupMenu,
    view::{rgba, sprite},
};
use crate::adv::characters::view::CharacterTextures;
use glam::{Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::render_pass::RenderPass;

struct Draw {
    name: String,
    rect: Vec4,
    uv: Vec4,
    tint: UnormColor,
    flash: Option<f32>,
}
fn draw(
    name: impl Into<String>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: Vec4,
    tint: UnormColor,
) -> Draw {
    Draw {
        name: name.into(),
        rect: vec4(x, y, x + w, y + h),
        uv,
        tint,
        flash: None,
    }
}
fn number_glyphs(out: &mut Vec<Draw>, current: usize, count: usize, x: f32, y: f32) {
    // 39810 takes zero-based current and count-1, formatting both plus one.
    let advance = if current > 8 { 48.0 } else { 32.0 } + if count > 9 { 32.0 } else { 16.0 };
    let mut left = x + 56.0 - advance * 0.5;
    let tint = rgba(0.8, 0.73333335, 0.6666667, 1.0);
    for ch in format!("{}/{count}", current + 1).bytes() {
        let index = if ch == b'/' { 10 } else { ch - b'0' };
        let u = (index as f32 * 32.0) / 352.0;
        out.push(draw(
            "num",
            left - 1.0,
            y + 36.0,
            18.0,
            40.0,
            vec4(u, 0.0, u + 18.0 / 352.0, 1.0),
            tint,
        ));
        left += 16.0;
    }
}
fn artwork(menu: &BustupMenu, catalog: &BustupCatalog, controls: f32) -> Vec<Draw> {
    let mut out = Vec::new();
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    let white = UnormColor::WHITE;
    let (c, o, p, e) = menu.selection().indices();
    let indices = [c, o, p, e, menu.selection().background_index()];
    let counts = [
        catalog.characters.len(),
        catalog.characters[c].outfits.len(),
        catalog.characters[c].outfits[o].poses.len(),
        catalog.characters[c].outfits[o].poses[p].expressions.len(),
        catalog.backgrounds.len(),
    ];
    let flashes = menu.arrow_values();
    for row in 0..5 {
        let large = row == 0 || row == 4;
        let size = if large { 144.0 } else { 112.0 };
        let x = menu.panel_offset() + if large { 1504.0 } else { 1520.0 };
        let y = [248.0, 392.0, 504.0, 616.0, 728.0][row];
        let icon = if large { 0.0 } else { 1.0 };
        let focused = menu.row() as usize == row;
        out.push(draw(
            "icon",
            x,
            y,
            size,
            size,
            vec4(icon * 0.5, 0.0, icon * 0.5 + size / 288.0, size / 288.0),
            white,
        ));
        match row {
            0 => out.push(draw(
                format!("char{:02}", c + 1),
                x,
                y,
                144.0,
                144.0,
                full,
                white,
            )),
            4 => out.push(draw(
                format!("bg{:03}", indices[4] + 1),
                x,
                y,
                144.0,
                144.0,
                full,
                white,
            )),
            _ => number_glyphs(&mut out, indices[row], counts[row], x, y),
        }
        if focused {
            out.push(draw(
                "icon",
                x,
                y,
                size,
                size,
                vec4(
                    icon * 0.5,
                    0.5,
                    icon * 0.5 + size / 288.0,
                    0.5 + size / 288.0,
                ),
                rgba(1.0, 1.0, 1.0, 0.3),
            ));
        }
        let text_y = [0.0, 2.0, 3.0, 4.0, 5.0][row] * 32.0 / 192.0;
        out.push(draw(
            "text",
            x + size * 0.5 - 72.0,
            y + size - 36.0,
            144.0,
            32.0,
            vec4(0.0, text_y, 1.0, text_y + 1.0 / 6.0),
            white,
        ));
        let arrow_y = [280.0, 408.0, 520.0, 632.0, 760.0][row];
        for side in 0..2 {
            let arrow_x = menu.panel_offset()
                + if large {
                    [1424.0, 1648.0][side]
                } else {
                    [1440.0, 1632.0][side]
                };
            let enabled = if side == 0 {
                indices[row] > 0
            } else {
                indices[row] + 1 < counts[row]
            };
            let tint = if enabled {
                white
            } else {
                rgba(0.4, 0.4, 0.4, 1.0)
            };
            let u = side as f32 * 0.5;
            let mut base = draw(
                "arrow",
                arrow_x,
                arrow_y,
                80.0,
                80.0,
                vec4(u, 0.0, u + 0.5, 0.5),
                tint,
            );
            base.flash = Some(flashes[row * 2 + side]);
            out.push(base);
            if focused {
                out.push(draw(
                    "arrow",
                    arrow_x,
                    arrow_y,
                    80.0,
                    80.0,
                    vec4(u, 0.5, u + 0.5, 1.0),
                    rgba(1.0, 1.0, 1.0, 0.3),
                ));
            }
        }
    }
    if controls > 0.0 {
        let y = controls * 150.0 - 150.0 + 32.0;
        for (i, (name, x)) in [("close", 1764.0), ("hidden", 1668.0)]
            .into_iter()
            .enumerate()
        {
            let mut button = draw(name, x, y, 84.0, 84.0, full, white);
            button.flash = Some(menu.button_values()[i]);
            out.push(button);
        }
        let y = 1080.0 + controls * (-88.0) + 20.0;
        let mut right = 1872.0;
        for row in [1, 2, 3, 0] {
            let (keys, width) = [
                ([1, -1], 214.0),
                ([4, -1], 80.0),
                ([3, -1], 180.0),
                ([7, 2], 214.0),
            ][row];
            let left = right - width;
            out.push(draw(
                "help_bupmode",
                left,
                y,
                width,
                48.0,
                vec4(0.0, row as f32 / 4.0, width / 320.0, (row + 1) as f32 / 4.0),
                white,
            ));
            let mut icon_left = left;
            for key in keys.into_iter().filter(|k| *k >= 0) {
                icon_left -= 46.0;
                let u = (key & 7) as f32 / 8.0;
                let v = (key >> 3) as f32 / 2.0;
                out.push(draw(
                    "key",
                    icon_left,
                    y,
                    48.0,
                    48.0,
                    vec4(u, v, u + 0.125, v + 0.5),
                    white,
                ));
            }
            right = icon_left - 10.0;
        }
    }
    out
}
pub(crate) fn render_artwork(
    menu: &BustupMenu,
    catalog: &BustupCatalog,
    controls: f32,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let transform = crate::render::top_left_projection_matrix();
    for item in artwork(menu, catalog, controls) {
        if let Some(flash) = item.flash {
            atlas.render_button(
                pass,
                &item.name,
                item.rect,
                item.uv,
                item.tint,
                Vec4::ZERO,
                vec4(1.0, 1.0, 1.0, flash),
                transform,
            );
        } else {
            sprite(
                pass,
                atlas.get(&item.name).as_source(),
                item.rect,
                item.uv,
                item.tint,
                transform,
            );
        }
    }
}
#[cfg(test)]
mod tests;
