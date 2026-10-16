//! Literal original4A8F0 gallery shell and cell packets; native1920x1080 canvas.
use super::Controller;
#[derive(Clone, Debug)]
pub struct Packet {
    pub texture: String,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub blend: u32,
    pub flash: f32,
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn quad(
    out: &mut Vec<Packet>,
    name: impl Into<String>,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
    blend: u32,
) {
    out.push(Packet {
        texture: name.into(),
        rect,
        uv,
        color,
        blend,
        flash: 0.,
    });
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
fn hint(out: &mut Vec<Packet>, mut right: f32, row: usize, color: u32) -> f32 {
    let (key, v, width) = match row {
        0 => (0, 0., 214.),
        1 => (4, 48., 80.),
        2 => (3, 96., 80.),
        _ => (7, 144., 246.),
    };
    right -= width;
    quad(
        out,
        "help_cgmode",
        [right, 1012., right + width, 1060.],
        [0., v / 192., width / 320., v / 192. + 0.25],
        color,
        2,
    );
    right -= 46.;
    let u = key as f32 * 48. / 384.;
    quad(
        out,
        "key",
        [right, 1012., right + 48., 1060.],
        [u, 0., u + 0.125, 0.5],
        color,
        2,
    );
    right - 10.
}
fn number(out: &mut Vec<Packet>, number: usize, left: f32) {
    let text = format!("{number}");
    let mut x = left;
    for b in text.bytes() {
        let u = ((b - b'0') as f32 * 30.) / 416.;
        quad(
            out,
            "num",
            [x, 54., x + 30., 94.],
            [
                u,
                f32::from_bits(0x3eaaaaab),
                u + f32::from_bits(0x3d93b13b),
                0.75,
            ],
            0xffbbbbbb,
            2,
        );
        x += 30.;
    }
}
pub fn packets(s: &Controller) -> Vec<Packet> {
    let mut out = vec![];
    if !s.ready {
        return out;
    }
    quad(&mut out, "bg", [0., 0., 1920., 1080.], FULL, u32::MAX, 1);
    for i in 0..12 {
        let Some(e) = s.entries.get(s.page * 12 + i) else {
            break;
        };
        let grow = (s.focus[i] + (-0.5)) * 0.5;
        let gx = grow * 75.;
        let gy = grow * 42.;
        let x = (-gx).mul_add(0.5, s.positions[i].value());
        let y = (-gy).mul_add(0.5, (222 + (i >> 2) * 252) as f32);
        let rect = [x, y, x + (gx + 384.), y + (gy + 216.)];
        quad(
            &mut out,
            if e.locked() { "logo" } else { &e.name },
            rect,
            FULL,
            u32::MAX,
            1,
        );
        out.last_mut().unwrap().flash = s.hot[i].value();
        if s.cell() == i {
            let angle = s.spin * f32::from_bits(0x40490fd0);
            let a = (angle + angle).sin().mul_add(0.2, 0.4);
            quad(
                &mut out,
                "",
                rect,
                FULL,
                0xffffff | (((a * 255.) as u32).min(255) << 24),
                2,
            );
        }
    }
    let e = &s.entries[s.selected()];
    if e.pictures.len() >= 2 && !e.locked() {
        let mut x = (1830 - 36 * e.pictures.len() as i32) as f32;
        for unlocked in &e.unlocked {
            let u = if *unlocked { 0.25 } else { 0. };
            quad(
                &mut out,
                "check",
                [x, 948., x + 52., 1000.],
                [u, 0., u + 0.25, 1.],
                u32::MAX,
                2,
            );
            x += 36.;
        }
    }
    for (name, rect, blend) in [
        ("top", [0., 0., 1920., 191.], 3),
        ("caption", [32., 0., 576., 192.], 2),
        ("button_shadow", [1728., 0., 1920., 192.], 2),
    ] {
        quad(&mut out, name, rect, FULL, u32::MAX, blend);
    }
    quad(
        &mut out,
        "close",
        [1766., 46., 1874., 154.],
        [0., 0., 0.5, 1.],
        u32::MAX,
        2,
    );
    out.last_mut().unwrap().flash = s.close_flash.value();
    quad(
        &mut out,
        "numbg",
        [1632., 48., 1760., 112.],
        FULL,
        u32::MAX,
        2,
    );
    number(&mut out, s.page + 1, 1646.);
    number(&mut out, s.pages(), 1706.);
    let r = hint(&mut out, 1872., 1, u32::MAX);
    let r = hint(
        &mut out,
        r,
        2,
        if e.locked() { 0xff808080 } else { u32::MAX },
    );
    let r = hint(&mut out, r, 3, u32::MAX);
    hint(&mut out, r, 0, u32::MAX);
    out
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
            v.extend([x.to_bits(), y.to_bits(), 0, self.color]);
            if !self.texture.is_empty() {
                v.extend([u.to_bits(), w.to_bits()]);
            }
        }
        v
    }
}
