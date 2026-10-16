//! Native53ab0/504a0/50e8c quads, before the shared GPU submission boundary.
use super::{ConfigController, Control, HEIGHT, ROWS, VISIBLE};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Canvas,
    Rows,
    Headings,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    pub texture: &'static str,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub alpha: f32,
    pub flash: f32,
    pub space: Space,
}
fn push(o: &mut Vec<Quad>, texture: &'static str, rect: [f32; 4], uv: [f32; 4], space: Space) {
    o.push(Quad {
        texture,
        rect,
        uv,
        alpha: 1.,
        flash: 0.,
        space,
    });
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
pub fn quads(m: &ConfigController, game: crate::cli::Game) -> Vec<Quad> {
    let mut o = vec![];
    push(&mut o, "bg", [0., 0., 1920., 1080.], FULL, Space::Canvas);
    o.last_mut().unwrap().alpha = m.progress;
    for (i, r) in ROWS.iter().enumerate() {
        let y = r.y as f32;
        if i == m.selected {
            for (t, b, v, w) in [
                (y, y + 12., 0., 0.375),
                (y + 12., y + 68., 0.375, 0.625),
                (y + 68., y + 80., 0.625, 1.),
            ] {
                push(
                    &mut o,
                    "focus",
                    [0., t, 1600., b],
                    [0., v, 1., w],
                    Space::Rows,
                );
                o.last_mut().unwrap().alpha = 192. / 255.;
            }
        }
        let v = (r.title as f32 * 72.) / 1440.;
        push(
            &mut o,
            "left",
            [40., y + 4., 792., y + 76.],
            [0., v, 1., v + 0.05],
            Space::Rows,
        );
        match r.control {
            Control::Buttons(labels) => {
                for c in [1usize, 0] {
                    let x = 1094. + 236. * c as f32;
                    let selected = m.value(i) as usize == c;
                    let u = if i == m.selected && selected {
                        2. / 3.
                    } else if selected {
                        1. / 3.
                    } else {
                        0.
                    };
                    let v = labels[c] as f32 * 80. / 640.;
                    push(
                        &mut o,
                        "right",
                        [x, y, x + 224., y + 80.],
                        [u, v, u + 1. / 3., v + 0.125],
                        Space::Rows,
                    );
                }
            }
            Control::Slider(kind) => {
                let v = (kind as f32 * 48. + 48.) / 192.;
                push(
                    &mut o,
                    "slider",
                    [1474., y + 16., 1570., y + 64.],
                    [
                        f32::from_bits(0x3efcd6ea),
                        v,
                        f32::from_bits(0x3f24587e),
                        v + 0.25,
                    ],
                    Space::Rows,
                );
                push(
                    &mut o,
                    "slider",
                    [842., y + 16., 1490., y + 64.],
                    [0., 0., 1., 0.25],
                    Space::Rows,
                );
                push(
                    &mut o,
                    "slider",
                    [762., y + 16., 858., y + 64.],
                    [
                        f32::from_bits(0x3e4a4588),
                        v,
                        f32::from_bits(0x3eb0fcd7),
                        v + 0.25,
                    ],
                    Space::Rows,
                );
                let x = m.value(i) as f32 * 6. + 842.;
                let u = if i == m.selected { 48. / 648. } else { 0. };
                push(
                    &mut o,
                    "slider",
                    [x, y + 8., x + 48., y + 72.],
                    [u, 2. / 3., u + 48. / 648., 1.],
                    Space::Rows,
                );
            }
        }
    }
    for y in [
        160., 246., 332., 418., 664., 750., 996., 1082., 1168., 1254., 1340.,
    ] {
        push(
            &mut o,
            "separator",
            [0., y, 1600., y + 6.],
            FULL,
            Space::Rows,
        );
    }
    for (i, (top, bottom)) in [(0., 504.), (504., 836.), (836., 1340.)]
        .into_iter()
        .enumerate()
    {
        let y = if top - m.scroll() >= 0. {
            top - m.scroll()
        } else {
            (bottom - m.scroll() - 80.).min(0.)
        };
        let v = i as f32 * 80. / 320.;
        push(
            &mut o,
            "category",
            [0., y, 1600., y + 80.],
            [0., v, 1., v + 0.25],
            Space::Headings,
        );
    }
    let y = m.progress.mul_add(150., -150.);
    for (t, x, right) in [
        ("top", 0., 1920.),
        ("caption", 32., 576.),
        ("button_shadow", 1376., 1920.),
    ] {
        push(&mut o, t, [x, y, right, y + 192.], FULL, Space::Canvas);
    }
    for (reset, t, x, right) in [
        (false, "close", 1766., 1874.),
        (true, "default", 1404., 1744.),
    ] {
        push(
            &mut o,
            t,
            [x, y + 46., right, y + 154.],
            [0., 0., 0.5, 1.],
            Space::Canvas,
        );
        o.last_mut().unwrap().flash = m.flash(reset);
    }
    if game == crate::cli::Game::Gerokasu {
        // Gerokasu 57974..57A0C: bottom band slides with the hints.
        let y = m.progress.mul_add(-88., 1080.);
        push(
            &mut o,
            "bottom",
            [0., y, 1920., y + 88.],
            FULL,
            Space::Canvas,
        );
    }
    let y = m.progress.mul_add(-88., 1100.);
    let mut right = 1872.;
    let controls = match ROWS[m.selected].control {
        Control::Slider(_) => [(1, 76., 4), (3, 180., 6), (4, 180., 2), (0, 214., 1)],
        Control::Buttons(_) => [(1, 76., 4), (3, 180., 6), (4, 180., 2), (0, 214., 1)],
    };
    for (band, width, key) in controls {
        let left = right - width;
        let v = band as f32 * 48. / 240.;
        push(
            &mut o,
            "help_config",
            [left, y, right, y + 48.],
            [0., v, width / 320., v + 0.2],
            Space::Canvas,
        );
        let u = key as f32 * 48. / 384.;
        push(
            &mut o,
            "key",
            [left - 48., y, left, y + 48.],
            [u, 0., u + 0.125, 0.5],
            Space::Canvas,
        );
        right = left - 58.;
    }
    o
}
pub fn offset(m: &ConfigController, space: Space) -> [f32; 2] {
    let x = m.progress.mul_add(-1920., 1920.);
    match space {
        Space::Canvas => [0., 0.],
        Space::Rows => [x + 160., 178. - m.scroll()],
        Space::Headings => [x + 160., 178.],
    }
}
pub fn clip(space: Space) -> Option<[f32; 4]> {
    match space {
        Space::Canvas => None,
        Space::Rows => Some([160., 258., 1760., 974.]),
        Space::Headings => Some([160., 178., 1760., 974.]),
    }
}
pub fn scrollbar_metrics(m: &ConfigController) -> (f32, f32, f32) {
    (HEIGHT, m.scroll(), VISIBLE)
}
