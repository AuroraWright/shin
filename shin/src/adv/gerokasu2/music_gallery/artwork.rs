//!35410/35DBC original BGMMode atlas rows, chrome and controller hints.
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
#[derive(Clone, Copy, Debug)]
pub struct ScrollVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct ScrollbarPacket {
    pub matrix: [f32; 16],
    pub vertices: [ScrollVertex; 8],
}
pub const PROJECTION: [f32; 16] = [
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
];
fn quad(
    out: &mut Vec<Packet>,
    texture: impl Into<String>,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
    blend: u32,
) {
    out.push(Packet {
        texture: texture.into(),
        rect,
        uv,
        color,
        blend,
        flash: 0.,
    });
}
fn alpha(value: f32) -> u32 {
    ((value * 255.).clamp(0., 255.) as u32) << 24
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
/// C12A0 performs division before scaling its784px track. Preserve the local
/// vertices and composed parent/8px-cap translation instead of flattening bands.
pub fn scrollbar(model: &Controller) -> Vec<ScrollbarPacket> {
    if model.document_height <= 800
        || model.scrollbar_position < 0
        || model.scrollbar_position.saturating_add(800) > model.document_height
    {
        return vec![];
    }
    let height = model.document_height as f32;
    let start = (model.scrollbar_position as f32 / height) * 784.;
    let end = ((model.scrollbar_position + 800) as f32 / height) * 784.;
    let mut matrix = PROJECTION;
    matrix[12] = PROJECTION[0].mul_add(1848., PROJECTION[12]);
    matrix[13] = PROJECTION[5].mul_add(8., PROJECTION[5].mul_add(176., PROJECTION[13]));
    [
        (0., [-8., 0., 784., 792.]),
        (0.5, [start - 8., start, end, end + 8.]),
    ]
    .into_iter()
    .map(|(u, ys)| ScrollbarPacket {
        matrix,
        vertices: std::array::from_fn(|i| ScrollVertex {
            position: [if i & 1 == 0 { 0. } else { 16. }, ys[i / 2], 1.],
            uv: [if i & 1 == 0 { u } else { u + 0.5 }, (i / 2) as f32 * 0.25],
        }),
    })
    .collect()
}
fn hint(out: &mut Vec<Packet>, mut right: f32, id: usize, color: u32, swapped: bool) -> f32 {
    const ROWS: [(i32, i32, f32); 4] = [(0, -1, 214.), (4, -1, 80.), (3, -1, 82.), (6, -1, 82.)];
    let (first, second, width) = ROWS[id];
    let left = right - width;
    quad(
        out,
        "help_bgmmode",
        [left, 1012., right, 1060.],
        [0., id as f32 / 4., width / 320., id as f32 / 4. + 0.25],
        color,
        2,
    );
    right = left + 2.;
    for key in [first, second].into_iter().filter(|v| *v >= 0) {
        let key = if swapped {
            match key {
                3 => 4,
                4 => 3,
                _ => key,
            }
        } else {
            key
        };
        right -= 48.;
        let u = (key & 7) as f32 * 0.125;
        let v = (key >> 3) as f32 * 0.5;
        quad(
            out,
            "key",
            [right, 1012., right + 48., 1060.],
            [u, v, u + 0.125, v + 0.5],
            color,
            2,
        );
    }
    right - 10.
}
pub fn packets(model: &Controller, swapped: bool) -> Vec<Packet> {
    let mut out = vec![];
    quad(&mut out, "bg", [0., 0., 1920., 1080.], FULL, u32::MAX, 1);
    let origin = model.scroll() + -176.;
    for (i, _) in model.rows.iter().enumerate() {
        let y = ((i / 2) * 100) as f32 - origin;
        if !(-100.0..1080.0).contains(&y) {
            continue;
        }
        let x = if i & 1 == 0 { 160. } else { 960. };
        let y = (y + 6.) + -11.;
        quad(
            &mut out,
            "shadow",
            [x, y, x + 800., y + 110.],
            FULL,
            u32::MAX,
            2,
        );
    }
    for (i, row) in model.rows.iter().enumerate() {
        let y = ((i / 2) * 100) as f32 - origin;
        if !(-100.0..1080.0).contains(&y) {
            continue;
        }
        let x = if i & 1 == 0 { 160. } else { 960. };
        let y = y + 6.;
        let rect = [x, y, x + 800., y + 88.];
        let v = if model.playing == i as i32 { 0.5 } else { 0. };
        quad(&mut out, "plate", rect, [0., v, 1., v + 0.5], u32::MAX, 2);
        if model.selected == i as i32 {
            let angle = model.elapsed * f32::from_bits(0x40490fd0);
            let blink = (angle + angle).sin().mul_add(0.2, 0.4);
            quad(&mut out, "cur", rect, FULL, alpha(blink) | 0xffffff, 2);
        }
        let v = f32::from(row.name % 32) * 64. * f32::from_bits(0x3a000000);
        quad(
            &mut out,
            format!("title{}", row.name / 32),
            [x + 80., y + 12., x + 720., y + 76.],
            [0., v, 1., v + 0.03125],
            u32::MAX,
            2,
        );
        let flash = row.flash.value();
        if flash > 0. {
            quad(&mut out, "flash", rect, FULL, alpha(flash) | 0xffffff, 2);
        }
    }
    // Top strip covers rows that move behind the caption. Native uses blend3.
    quad(&mut out, "top", [0., 0., 1920., 192.], FULL, u32::MAX, 3);
    quad(
        &mut out,
        "caption",
        [32., 0., 544., 192.],
        FULL,
        u32::MAX,
        2,
    );
    quad(
        &mut out,
        "button_shadow",
        [1728., 0., 1920., 192.],
        FULL,
        u32::MAX,
        2,
    );
    quad(
        &mut out,
        "close",
        [1766., 46., 1874., 154.],
        [0., 0., 0.5, 1.],
        u32::MAX,
        2,
    );
    out.last_mut().unwrap().flash = model.flash();
    let mut right = hint(&mut out, 1872., 1, u32::MAX, swapped);
    right = hint(
        &mut out,
        right,
        3,
        if model.playing >= 0 {
            u32::MAX
        } else {
            0xff808080
        },
        swapped,
    );
    right = hint(
        &mut out,
        right,
        2,
        if model.rows[model.selected as usize].unlocked {
            u32::MAX
        } else {
            0xff808080
        },
        swapped,
    );
    hint(&mut out, right, 0, u32::MAX, swapped);
    out
}
