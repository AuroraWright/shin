//! Literal Ger793A0/79AAC packets in the original1920x1080 canvas.
use super::Controller;
#[derive(Clone, Debug)]
pub struct Packet {
    pub texture: String,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub flash: f32,
    pub matrix: [f32; 16],
    pub stencil: [u32; 5],
    pub blend: u32,
}
pub const DEFAULT: [u32; 5] = [7, 0, 0, 0, 0xffff];
pub const CLIP: [u32; 5] = [7, 0, 0, 3, 0xffffff];
pub const PAGE: [u32; 5] = [3, 0, 0, 0, 0x1ffff];
const FULL: [f32; 4] = [0., 0., 1., 1.];
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
fn push(
    out: &mut Vec<Packet>,
    texture: impl Into<String>,
    rect: [f32; 4],
    uv: [f32; 4],
    matrix: [f32; 16],
    color: u32,
    stencil: [u32; 5],
    blend: u32,
) {
    out.push(Packet {
        texture: texture.into(),
        rect,
        uv,
        color,
        flash: 0.,
        matrix,
        stencil,
        blend,
    });
}
fn hint(out: &mut Vec<Packet>, mut right: f32, y: f32, row: usize, swapped: bool) -> f32 {
    let (keys, v, width) = if row == 0 {
        ([7, 2], 0., 248.)
    } else {
        ([4, -1], 48., 76.)
    };
    right -= width;
    push(
        out,
        "help_manual",
        [right, y, right + width, y + 48.],
        [0., v / 96., width / 320., v / 96. + 0.5],
        projection(),
        u32::MAX,
        DEFAULT,
        2,
    );
    for mut key in keys {
        if key < 0 {
            continue;
        }
        if swapped {
            if key == 3 {
                key = 4;
            } else if key == 4 {
                key = 3;
            }
        }
        right -= 48.;
        let u = (key * 48) as f32 / 384.;
        push(
            out,
            "key",
            [right, y, right + 48., y + 48.],
            [u, 0., u + 0.125, 0.5],
            projection(),
            u32::MAX,
            DEFAULT,
            2,
        );
    }
    right - 10.
}
pub fn packets(model: &Controller, swapped: bool) -> Vec<Packet> {
    packets_for(
        model.resources_ready(),
        model.count,
        model.progress,
        model.scroll(),
        model.close_flash(),
        swapped,
    )
}
pub fn packets_for(
    ready: bool,
    count: usize,
    progress: f32,
    scroll: f32,
    flash: f32,
    swapped: bool,
) -> Vec<Packet> {
    if !ready {
        return vec![];
    }
    let mut out = vec![];
    let shell = projection();
    push(
        &mut out,
        "bg",
        [0., 0., 1920., 1080.],
        FULL,
        shell,
        (((progress * 255.) as u32).min(255) << 24) | 0xffffff,
        DEFAULT,
        2,
    );
    let mut page = shell;
    let offset = progress.mul_add(-1920., 1920.);
    page[12] = (offset * shell[0]) + shell[12];
    push(
        &mut out,
        "",
        [0., 162., 1920., 980.],
        FULL,
        page,
        0xffff0000,
        CLIP,
        0,
    );
    for i in 0..count {
        let offset = (i as i32).wrapping_mul(1500) as f32 - scroll;
        if offset >= 1920. || offset + 1920. <= 0. {
            continue;
        }
        let alpha = (1. - ((offset / 1500.) * 0.5).abs()).max(0.5);
        let color = (((alpha * 255.) as u32).min(255) << 24) | 0xffffff;
        let x = offset + 240.;
        push(
            &mut out,
            format!("{:03}", i + 1),
            [x, 166., x + 1440., 976.],
            FULL,
            page,
            color,
            PAGE,
            2,
        );
    }
    let y = progress.mul_add(150., -150.);
    for (t, x, right, blend) in [
        ("top", 0., 1920., 3),
        ("caption", 32., 576., 2),
        ("button_shadow", 1728., 1920., 2),
    ] {
        push(
            &mut out,
            t,
            [x, y, right, y + 192.],
            FULL,
            shell,
            u32::MAX,
            DEFAULT,
            blend,
        );
    }
    push(
        &mut out,
        "close",
        [1766., y + 46., 1874., y + 154.],
        [0., 0., 0.5, 1.],
        shell,
        u32::MAX,
        DEFAULT,
        2,
    );
    out.last_mut().unwrap().flash = flash;
    let y = progress.mul_add(-88., 1080.) + 20.;
    let right = hint(&mut out, 1872., y, 1, swapped);
    hint(&mut out, right, y, 0, swapped);
    out
}
impl Packet {
    /// Native PosColTex/PosCol upload, before shared rendering conversion.
    pub fn native_vertices(&self) -> Vec<u32> {
        let mut out = vec![];
        for (x, y, u, v) in [
            (self.rect[0], self.rect[1], self.uv[0], self.uv[1]),
            (self.rect[2], self.rect[1], self.uv[2], self.uv[1]),
            (self.rect[0], self.rect[3], self.uv[0], self.uv[3]),
            (self.rect[2], self.rect[3], self.uv[2], self.uv[3]),
        ] {
            out.extend([x.to_bits(), y.to_bits(), 0, self.color]);
            if !self.texture.is_empty() {
                out.extend([u.to_bits(), v.to_bits()]);
            }
        }
        out
    }
}
