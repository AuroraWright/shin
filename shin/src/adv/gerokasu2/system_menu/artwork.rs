//! Original1CE00 CPU packets. Native program7 samples both row atlases; the
//! application supplies its renderer and prepared FNT6 measured text widths.
use super::Controller;
#[derive(Clone, Debug)]
pub struct Packet {
    pub texture: &'static str,
    pub texture2: Option<&'static str>,
    pub program: u32,
    pub transform: [f32; 16],
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub mix: [f32; 4],
    pub flash: [f32; 4],
    pub blend: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    SceneTitle,
    PlayTime,
}
#[derive(Clone, Debug)]
pub struct TextRequest {
    pub kind: TextKind,
    pub transform: [f32; 16],
    pub color: u32,
    pub outline: u32,
}
#[derive(Clone, Debug)]
pub struct Detail {
    pub panel: Packet,
    pub text: TextRequest,
}
pub const TEXT_FONT: &str = "/system.fnt";
pub const TEXT_LAYOUT_WIDTH: f32 = 490.;
pub const TEXT_SIZE: f32 = 38.;
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
fn translated(m: [f32; 16], x: f32, y: f32) -> [f32; 16] {
    let mut out = m;
    for i in 0..4 {
        out[12 + i] = (m[i] * x + m[4 + i] * y) + m[12 + i];
    }
    out
}
fn alpha(a: f32) -> u32 {
    ((a.clamp(0., 1.) * 255.) as u32) << 24
}
fn packet(texture: &'static str, rect: [f32; 4], uv: [f32; 4], a: f32) -> Packet {
    Packet {
        texture,
        texture2: None,
        program: 2,
        transform: projection(),
        rect,
        uv,
        color: alpha(a) | 0xffffff,
        mix: [0.; 4],
        flash: [0.; 4],
        blend: 2,
    }
}
pub fn packets(m: &Controller) -> Vec<Packet> {
    if m.phase < 1 {
        return vec![];
    }
    let mut out = Vec::new();
    for (i, r) in m.rows.iter().enumerate() {
        let slide = r.slide.value();
        let x = (slide - 1.).mul_add(504., 24.) - 24.;
        let y = 72. + i as f32 * 120. - 20.;
        out.push(packet(
            "sysmenu_shadow",
            [x, y, x + 512., y + 160.],
            [0., 0., 1., 1.],
            slide,
        ));
    }
    for (i, r) in m.rows.iter().enumerate() {
        let slide = r.slide.value();
        let x = (slide - 1.).mul_add(504., 24.);
        let y = 72. + i as f32 * 120.;
        let v = r.item as f32 * 112. / 560.;
        let mut q = packet(
            "sysmenu_normal",
            [x, y, x + 464., y + 112.],
            [0., v, 1., v + 0.2],
            slide,
        );
        q.texture2 = Some("sysmenu_focus");
        q.program = 7;
        let selected = if m.selected == i { 1. } else { 0. };
        q.mix = [selected, selected, selected, 1.];
        q.flash = [1., 1., 1., r.flash.value()];
        out.push(q);
    }
    out
}
/// Widths are ordered [scene title,play clock], including native SimpleText
/// layout padding. A zero/nonpositive width omits that entire info panel.
pub fn details(m: &Controller, widths: [f32; 2]) -> Vec<Detail> {
    if m.phase < 1 {
        return vec![];
    }
    let mut out = Vec::new();
    for (i, width) in widths.into_iter().enumerate() {
        if width <= 0. || !width.is_finite() {
            continue;
        }
        let transform = translated(
            projection(),
            -48. + m.detail_slide.value() * 120.,
            if i == 0 { 795. } else { 930. },
        );
        let mut panel = packet(
            "info",
            [1176., 0., 1932., 64.],
            [0., i as f32 * 0.5, 1., (i + 1) as f32 * 0.5],
            m.detail_fade.value(),
        );
        panel.transform = transform;
        out.push(Detail {
            panel,
            text: TextRequest {
                kind: if i == 0 {
                    TextKind::SceneTitle
                } else {
                    TextKind::PlayTime
                },
                transform: translated(transform, 1920. - width, 70.),
                color: alpha(m.detail_fade.value()) | 0xffffff,
                outline: alpha(m.detail_fade.value()),
            },
        });
    }
    out
}
impl Packet {
    pub fn native_vertices(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for (x, y, u, v) in [
            (self.rect[0], self.rect[1], self.uv[0], self.uv[1]),
            (self.rect[2], self.rect[1], self.uv[2], self.uv[1]),
            (self.rect[0], self.rect[3], self.uv[0], self.uv[3]),
            (self.rect[2], self.rect[3], self.uv[2], self.uv[3]),
        ] {
            out.extend([
                x.to_bits(),
                y.to_bits(),
                0,
                self.color,
                u.to_bits(),
                v.to_bits(),
            ]);
            if self.texture2.is_some() {
                out.extend([u.to_bits(), v.to_bits()]);
            }
        }
        out
    }
}
