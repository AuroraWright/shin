//! Original B3BF0/B44C0/B46B0/B48B0/C12A0 artwork, independent of GPU resources.
use glam::{Vec2, Vec4, vec2, vec4};
use shin_core::format::save::gerokasu2::SaveDate;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Sprite,
    Thumbnail(usize),
    Window { selected: bool },
    Button { flash: f32 },
    Fill,
    Text(usize),
}
#[derive(Clone, Debug)]
pub struct Packet {
    pub kind: Kind,
    pub texture: &'static str,
    pub rect: Vec4,
    pub uv: Vec4,
    pub color: u32,
}
fn quad(texture: &'static str, rect: Vec4, uv: Vec4, color: u32) -> Packet {
    Packet {
        kind: Kind::Sprite,
        texture,
        rect,
        uv,
        color,
    }
}
fn digits(out: &mut Vec<Packet>, text: &str, origin: Vec2, big: bool) {
    let mut x = origin.x;
    for c in text.bytes() {
        let (u, v, w, h) = if big {
            if c.is_ascii_digit() {
                ((c - b'0') as f32 * 30., 32., 30., 40.)
            } else if c == b'.' {
                (400., 40., 16., 56.)
            } else {
                x += 16.;
                continue;
            }
        } else if (b'/'..=b':').contains(&c) {
            ((c - b'/') as f32 * 16. + 160., 0., 16., 28.)
        } else {
            x += 10.;
            continue;
        };
        // The small atlas is ordered digits, colon, slash. Its jump table remaps '/'.
        let u = if !big {
            if c.is_ascii_digit() {
                (c - b'0') as f32 * 16.
            } else if c == b'/' {
                176.
            } else {
                160.
            }
        } else {
            u
        };
        let left = x - 1.;
        let top = origin.y + 2.;
        out.push(quad(
            "num",
            vec4(left, top, left + w, top + h),
            vec4(u / 416., v / 96., u / 416. + w / 416., v / 96. + h / 96.),
            0xffccbbaa,
        ));
        x += w - 2.;
    }
}
pub fn plan_with_mode(
    mode: crate::adv::gerokasu2::save_menu::Mode,
    progress: f32,
    scroll: f32,
    selected: usize,
    dates: &[Option<SaveDate>; 100],
    row_flash: impl Fn(usize) -> f32,
    close_flash: f32,
    game: crate::cli::Game,
) -> Vec<Packet> {
    let mut out = vec![quad(
        "bg",
        vec4(0., 0., 1920., 1080.),
        vec4(0., 0., 1., 1.),
        0x00ffffff | ((progress * 255.) as u32) << 24,
    )];
    let slide = 1920f32.mul_add(-progress, 1920.);
    for i in 0..100 {
        let baseline = i as f32 * 201. - (scroll - 176.);
        if baseline >= 1080. || baseline + 197. < 0. {
            continue;
        }
        let x = 160. + slide;
        let y = baseline + 4.;
        for (top, bottom, v0, v1) in [
            (-4., 8., 0., 0.375),
            (8., 181., 0.375, 0.625),
            (181., 193., 0.625, 1.),
        ] {
            let mut p = quad(
                "window_normal",
                vec4(x, y + top, x + 1600., y + bottom),
                vec4(0., v0, 1., v1),
                0xc0ffffff,
            );
            p.kind = Kind::Window {
                selected: i == selected,
            };
            out.push(p);
        }
        out.push(quad(
            "vline",
            vec4(x + 532., y + 20., x + 535., y + 170.),
            vec4(0., 0., 1., 1.),
            u32::MAX,
        ));
        digits(
            &mut out,
            &format!("{:03}", i + 1),
            vec2(x + 390., y + 74.),
            true,
        );
        if let Some(date) = dates[i] {
            digits(
                &mut out,
                &format!(
                    "{:04}/{:02}/{:02} {:2}:{:02}",
                    date.year, date.month, date.day, date.hour, date.minute
                ),
                vec2(x + 564., y + 139.),
                false,
            );
        }
        out.push(Packet {
            kind: Kind::Text(i),
            texture: "",
            rect: vec4(slide, y, 0., 0.),
            uv: Vec4::ZERO,
            color: u32::MAX,
        });
        let mut thumbnail = quad(
            "nodata",
            vec4(x + 6., y, x + 342., y + 189.),
            vec4(0., 0., 1., 1.),
            u32::MAX,
        );
        thumbnail.kind = Kind::Thumbnail(i);
        out.push(thumbnail);
        let flash = row_flash(i);
        if flash > 0. {
            let mut p = quad(
                "",
                vec4(x, y, x + 1600., y + 189.),
                Vec4::ZERO,
                0x00ffffff | ((flash * 255.) as u32) << 24,
            );
            p.kind = Kind::Fill;
            out.push(p);
        }
    }
    // Native truncates the copied view position, then scales over the full800px
    // document track. Eight-pixel caps extend the track above/below that mapping.
    let height = 20096.;
    let extent = 800.;
    let position = scroll.trunc();
    let x = 1770. + slide;
    let y = 176.;
    let track = extent - 16.;
    let start = track * position / height;
    let end = track * (position + extent) / height;
    for (u, ys) in [
        (0., [y, y + 8., y + extent - 8., y + extent]),
        (
            0.5,
            [y + start, y + start + 8., y + end + 8., y + end + 16.],
        ),
    ] {
        for band in 0..3 {
            out.push(quad(
                "scroll",
                vec4(x, ys[band], x + 16., ys[band + 1]),
                vec4(u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25),
                u32::MAX,
            ));
        }
    }
    let top = 150f32.mul_add(progress, -150.);
    for (name, x, w) in [
        ("top", 0., 1920.),
        (
            if mode == crate::adv::gerokasu2::save_menu::Mode::Save {
                "caption_save"
            } else {
                "caption_load"
            },
            32.,
            544.,
        ),
        ("button_shadow", 1728., 192.),
    ] {
        out.push(quad(
            name,
            vec4(x, top, x + w, top + 192.),
            vec4(0., 0., 1., 1.),
            u32::MAX,
        ));
    }
    let mut p = quad(
        "close",
        vec4(1766., top + 46., 1874., top + 154.),
        vec4(0., 0., 0.5, 1.),
        u32::MAX,
    );
    p.kind = Kind::Button { flash: close_flash };
    out.push(p);
    if game == crate::cli::Game::Gerokasu {
        // Gerokasu B58F0..B5988: bottom band slides with the hints.
        let y = (-88f32).mul_add(progress, 1080.);
        out.push(quad(
            "bottom",
            vec4(0., y, 1920., y + 88.),
            vec4(0., 0., 1., 1.),
            u32::MAX,
        ));
    }
    let y = (-88f32).mul_add(progress, 1100.);
    let mut right = 1872.;
    let load = [
        (1, 80., [4, -1], true),
        (3, 112., [3, -1], dates[selected].is_some()),
        (0, 214., [7, 0], true),
    ];
    let save = [
        (1, 80., [4, -1], true),
        (2, 116., [3, -1], true),
        (4, 82., [6, -1], dates[selected].is_some()),
        (0, 214., [7, 0], true),
    ];
    let hints: &[_] = if mode == crate::adv::gerokasu2::save_menu::Mode::Save {
        &save
    } else {
        &load
    };
    for &(row, width, keys, enabled) in hints {
        let color = if enabled { u32::MAX } else { 0xff808080 };
        right -= width;
        out.push(quad(
            "help_saveload",
            vec4(right, y, right + width, y + 48.),
            vec4(0., row as f32 * 0.2, width / 320., (row + 1) as f32 * 0.2),
            color,
        ));
        for key in keys.into_iter().filter(|&v| v >= 0) {
            right -= 48.;
            out.push(quad(
                "key",
                vec4(right, y, right + 48., y + 48.),
                vec4(key as f32 * 0.125, 0., (key + 1) as f32 * 0.125, 0.5),
                color,
            ));
        }
        right -= 10.;
    }
    out
}
