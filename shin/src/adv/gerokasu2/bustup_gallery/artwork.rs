//! Ger3D380 five selectors and shell. Pure native CPU packets, no transport.
use super::Controller;
fn vec4(x: f32, y: f32, z: f32, w: f32) -> [f32; 4] {
    [x, y, z, w]
}
fn rgba(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let byte = |v: f32| ((v.clamp(0., 1.) * 255.) as u32).min(255);
    byte(b) | (byte(g) << 8) | (byte(r) << 16) | (byte(a) << 24)
}
#[derive(Clone, Debug)]
pub struct Packet {
    pub texture: String,
    pub blend: u32,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub flash: Option<f32>,
}
fn draw(
    name: impl Into<String>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: [f32; 4],
    tint: u32,
) -> Packet {
    Packet {
        texture: name.into(),
        blend: 2,
        rect: vec4(x, y, x + w, y + h),
        uv,
        color: tint,
        flash: None,
    }
}
fn number_glyphs(out: &mut Vec<Packet>, current: usize, count: usize, x: f32, y: f32) {
    // 39810 takes zero-based current and count-1, formatting both plus one.
    let advance = if current > 8 { 48.0 } else { 32.0 } + if count > 9 { 32.0 } else { 16.0 };
    let mut left = x + 56.0 - advance * 0.5;
    let tint = rgba(0.8, 0.73333335, 0.6666667, 1.0);
    for ch in format!("{}/{count}", current + 1).bytes() {
        let index = if ch == b'/' { 10 } else { ch - b'0' };
        let u = (index as f32 * 32.0) / 352.0;
        out.push(draw(
            "num",
            left - 3.0,
            y + 36.0,
            24.0,
            40.0,
            vec4(u, 0.0, (index as f32 * 32. + 24.) / 352., 1.0),
            tint,
        ));
        left += 16.0;
    }
}
pub fn packets(menu: &Controller, game: crate::cli::Game) -> Vec<Packet> {
    let gerokasu = game == crate::cli::Game::Gerokasu;
    if !menu.ready {
        return vec![];
    }
    let catalog = &menu.catalog;
    let controls = menu.controls_visibility.value();
    let mut out = Vec::new();
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    let white = u32::MAX;
    let (c, o, p, e) = menu.selection.indices();
    let indices = [c, o, p, e, menu.selection.background_index()];
    let counts = [
        catalog.characters.len(),
        catalog.characters[c].outfits.len(),
        catalog.characters[c].outfits[o].poses.len(),
        catalog.characters[c].outfits[o].poses[p].expressions.len(),
        catalog.backgrounds.len(),
    ];
    let flashes = std::array::from_fn::<_, 10, _>(|i| menu.arrows[i].value());
    for row in 0..5 {
        let large = row == 0 || row == 4;
        let size = if large { 144.0 } else { 112.0 };
        let x = menu.panel_offset.value() + if large { 1504.0 } else { 1520.0 };
        let y = [216.0, 360.0, 472.0, 584.0, 696.0][row];
        let icon = if large { 0.0 } else { 1.0 };
        let focused = menu.focus == row;
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
        let arrow_y = [248.0, 376.0, 488.0, 600.0, 728.0][row];
        for side in 0..2 {
            let arrow_x = menu.panel_offset.value()
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
        let y = controls.mul_add(150., -150.);
        let mut q = draw("top", 0., y, 1920., 192., full, white);
        // Gerokasu 37D60 keeps blend 2 for the top band.
        if !gerokasu {
            q.blend = 3;
        }
        out.push(q);
        out.push(draw("caption", 32., y, 512., 192., full, white));
        out.push(draw("button_shadow", 1600., y, 320., 192., full, white));
        for (i, (name, x)) in [("close", 1766.), ("hidden", 1636.)]
            .into_iter()
            .enumerate()
        {
            let mut q = draw(name, x, y + 46., 108., 108., vec4(0., 0., 0.5, 1.), white);
            q.flash = Some(if i == 0 {
                menu.close_flash.value()
            } else {
                menu.confirm_flash.value()
            });
            out.push(q);
        }
        if gerokasu {
            // Gerokasu 380C8..38164: bottom band slides with the hints.
            let y = controls.mul_add(-88., 1080.);
            out.push(draw("bottom", 0., y, 1920., 88., full, white));
        }
        let y = controls.mul_add(-88., 1080.) + 20.;
        let mut right = 1872.;
        for row in [1, 2, 3, 0] {
            let (keys, width) = [
                ([1, -1], 214.),
                ([4, -1], 80.),
                ([3, -1], 180.),
                ([2, -1], 214.),
            ][row];
            let left = right - width;
            out.push(draw(
                "help_bupmode",
                left,
                y,
                width,
                48.,
                vec4(0., row as f32 / 4., width / 320., (row + 1) as f32 / 4.),
                white,
            ));
            let mut x = left;
            for key in keys.into_iter().filter(|k| *k >= 0) {
                x -= 46.;
                let u = (key & 7) as f32 / 8.;
                let v = (key >> 3) as f32 / 2.;
                out.push(draw(
                    "key",
                    x,
                    y,
                    48.,
                    48.,
                    vec4(u, v, u + 0.125, v + 0.5),
                    white,
                ));
            }
            right = x - 10.;
        }
    }
    out
}
pub fn projection() -> [f32; 16] {
    [
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -0.5,
        0.,
        -1.,
        1.,
        0.5,
        1.,
    ]
}
impl Packet {
    pub fn native_vertices(&self) -> Vec<u32> {
        let mut v = vec![];
        for (x, y, u, w) in [
            (self.rect[0], self.rect[1], self.uv[0], self.uv[1]),
            (self.rect[2], self.rect[1], self.uv[2], self.uv[1]),
            (self.rect[0], self.rect[3], self.uv[0], self.uv[3]),
            (self.rect[2], self.rect[3], self.uv[2], self.uv[3]),
        ] {
            v.extend([
                x.to_bits(),
                y.to_bits(),
                0,
                self.color,
                u.to_bits(),
                w.to_bits(),
            ]);
        }
        v
    }
}
