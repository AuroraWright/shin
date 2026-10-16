//! Original80DB0 browser atlas packets and83620 pointer geometry.
//! The host owns glyph textures, private child rendering and audio/modal events.
#[path = "view/glyph.rs"]
pub mod glyph;
#[path = "view/pointer.rs"]
pub mod pointer;
#[cfg(test)]
#[path = "view/tests.rs"]
mod tests;

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
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub index: usize,
    pub top: f32,
    pub height: f32,
    pub kind: u8,
    pub voice_count: u8,
    pub message_arg: i32,
    pub flash: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Presentation {
    pub progress: f32,
    pub scroll: f32,
    pub selected: usize,
    pub chapter_open: bool,
    pub swapped: bool,
    pub close_flash: f32,
}
#[derive(Clone, Debug)]
pub struct Packet {
    pub texture: &'static str,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: u32,
    pub blend: u32,
    pub flash: f32,
    pub matrix: [f32; 16],
}
pub struct RowPackets {
    pub frame: Vec<Packet>,
    pub text_matrix: [f32; 16],
    pub portrait: Option<Packet>,
}
pub struct Frame {
    pub background: Packet,
    pub rows: Vec<RowPackets>,
    pub scrollbar: Vec<StripPacket>,
    pub chrome: Vec<Packet>,
}
#[derive(Clone, Copy, Debug)]
pub struct ScrollVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct StripPacket {
    pub matrix: [f32; 16],
    pub vertices: [ScrollVertex; 8],
}
#[derive(Clone, Copy, Debug)]
pub struct ScrollRange {
    pub position: i32,
    pub length: i32,
    pub minimum: i32,
    pub maximum: i32,
}
impl ScrollRange {
    ///80BE8 truncates scroll, floors document minimum and ceils its bottom.
    pub fn from_document(scroll: f32, minimum: f32, bottom: f32) -> Self {
        let position = scroll as i32;
        let maximum = bottom.ceil() as i32;
        Self {
            position,
            length: 832.min(maximum.saturating_sub(position)),
            minimum: minimum.floor() as i32,
            maximum,
        }
    }
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn packet(texture: &'static str, rect: [f32; 4], matrix: [f32; 16]) -> Packet {
    Packet {
        texture,
        rect,
        uv: FULL,
        color: u32::MAX,
        blend: 2,
        flash: 0.,
        matrix,
    }
}
///7E5E8 starts each column with FMUL and then accumulates FMLAs. For these
/// diagonal parents, translation is a rounded product followed by addition.
/// Fusing the nonzero product with parent translation changes observed bits.
fn translate(mut parent: [f32; 16], x: f32, y: f32) -> [f32; 16] {
    parent[12] = PROJECTION[0] * x + parent[12];
    parent[13] = PROJECTION[5] * y + parent[13];
    parent
}
fn sliding(progress: f32) -> [f32; 16] {
    translate(PROJECTION, (-progress).mul_add(1920., 1920.), 0.)
}
pub fn text_matrix(progress: f32, scroll: f32, row_top: f32) -> [f32; 16] {
    translate(sliding(progress), 190., (row_top + 18.) - (scroll - 160.))
}
///82280's visible kind color. Worker measurement uses999 for every kind.
pub fn row_color(kind: u8) -> i32 {
    match kind {
        1 => 977,
        2 => 799,
        3 => 977,
        4 => 979,
        _ => 999,
    }
}
///80F98 upper_bound then one preceding global row; an allowed range beginning
/// above zero can therefore request its immediate predecessor as overscan.
pub fn cache_start(rows: &[Row], allowed: std::ops::Range<usize>, scroll: f32) -> usize {
    if allowed.start >= allowed.end || allowed.end > rows.len() {
        return allowed.end.min(rows.len());
    }
    let origin = scroll - 160.;
    let mut left = allowed.start;
    let mut count = allowed.end - allowed.start;
    while count != 0 {
        let half = count / 2;
        let middle = left + half;
        if rows[middle].top > origin {
            count = half;
        } else {
            left = middle + 1;
            count -= half + 1;
        }
    }
    left.saturating_sub(1)
}
///81074 preserves subtraction before comparison; algebraic scroll+920 can
/// round differently. Creation includes equality, then887C0 may retire it.
pub fn should_create(row: &Row, scroll: f32) -> bool {
    row.top - (scroll - 160.) <= 1080.
}
///887C0 retires the cached resources at either exact outer boundary.
pub fn retain(row: &Row, scroll: f32) -> bool {
    let relative = row.top - (scroll - 160.);
    relative < 1080. && relative + row.height > 0.
}
pub fn row_packets(view: &Presentation, row: Row) -> RowPackets {
    let matrix = translate(sliding(view.progress), 160., row.top - (view.scroll - 160.));
    let xs = [0., 10., 1590., 1600.];
    let ys = [0., 10., row.height - 10., row.height];
    let us = [0., 0.00625, 0.99375, 1.];
    let vs = [0., 0.3125, 0.6875, 1.];
    let mut frame = Vec::with_capacity(9);
    for y in 0..3 {
        for x in 0..3 {
            let mut p = packet(
                if row.index == view.selected {
                    "window_focus"
                } else {
                    "window_normal"
                },
                [xs[x], ys[y], xs[x + 1], ys[y + 1]],
                matrix,
            );
            p.uv = [us[x], vs[y], us[x + 1], vs[y + 1]];
            frame.push(p);
        }
    }
    let text_matrix = text_matrix(view.progress, view.scroll, row.top);
    let portrait = if row.kind == 0 && row.voice_count != 0 {
        usize::try_from(row.message_arg)
            .ok()
            .and_then(|i| {
                PORTRAITS.get(i).copied()
            })
            .map(|texture| {
                let y = ((row.height - 36.) - 196.) * 0.5;
                let mut p = packet(texture, [0., y, 196., y + 196.], text_matrix);
                p.flash = row.flash;
                p
            })
    } else {
        None
    };
    RowPackets {
        frame,
        text_matrix,
        portrait,
    }
}
/// 81C64..81C70: relative-offset table 0x1019A8, indexed by message_arg.
pub const PORTRAITS: [&str; 9] = [
    "voice_remo",
    "voice_sian",
    "voice_mura",
    "voice_siro",
    "voice_koro",
    "voice_koro_jc",
    "voice_other",
    "voice_hanp",
    "voice_other",
];
pub fn background(progress: f32) -> Packet {
    let mut p = packet("bg", [0., 0., 1920., 1080.], PROJECTION);
    p.color = ((progress * 255.).clamp(0., 255.) as u32) << 24 | 0xffffff;
    p
}
pub fn scrollbar(progress: f32, range: ScrollRange) -> Vec<StripPacket> {
    let height = range.maximum as i64 - range.minimum as i64;
    let position = range.position as i64 - range.minimum as i64;
    if height <= i64::from(range.length)
        || range.length < 0
        || position < 0
        || position + i64::from(range.length) > height
    {
        return vec![];
    }
    let slide = (-progress).mul_add(1920., 1920.);
    let matrix = translate(translate(sliding(progress), slide + 1848., 160.), 0., 8.);
    let start = (position as f32 / height as f32) * 816.;
    let end = ((position + i64::from(range.length)) as f32 / height as f32) * 816.;
    [
        (0., [-8., 0., 816., 824.]),
        (0.5, [start - 8., start, end, end + 8.]),
    ]
    .into_iter()
    .map(|(u, ys)| StripPacket {
        matrix,
        vertices: std::array::from_fn(|i| ScrollVertex {
            position: [if i & 1 == 0 { 0. } else { 16. }, ys[i / 2], 1.],
            uv: [if i & 1 == 0 { u } else { u + 0.5 }, (i / 2) as f32 * 0.25],
        }),
    })
    .collect()
}
pub fn chrome(view: &Presentation, selected_voiced_message: bool) -> Vec<Packet> {
    let y = view.progress.mul_add(150., -150.);
    let mut out = vec![
        packet("top", [0., y, 1920., y + 192.], PROJECTION),
        packet("caption", [32., y, 576., y + 192.], PROJECTION),
        packet("button_shadow", [1728., y, 1920., y + 192.], PROJECTION),
        packet(
            "close",
            [1766., y + 46., 1874., (y + 46.) + 108.],
            PROJECTION,
        ),
    ];
    out[0].blend = 3;
    out[3].uv = [0., 0., 0.5, 1.];
    out[3].flash = view.close_flash;
    let y = view.progress.mul_add(-88., 1080.) + 20.;
    let mut right = 1872.;
    const HINTS: [(i32, i32, i32, f32); 5] = [
        (7, 0, 0, 214.),
        (4, -1, 48, 80.),
        (3, -1, 96, 148.),
        (6, -1, 144, 248.),
        (5, -1, 192, 278.),
    ];
    for id in [1, 2, 4, 3, 0] {
        if id == 4 && view.chapter_open {
            continue;
        }
        let (first, second, v, width) = HINTS[id];
        let left = right - width;
        let color = if id != 2 || selected_voiced_message {
            u32::MAX
        } else {
            0xff808080
        };
        let v = v as f32 / 288.;
        let mut p = packet("help_logview", [left, y, right, y + 48.], PROJECTION);
        p.color = color;
        p.uv = [0., v, width / 320., v + 1. / 6.];
        out.push(p);
        right = left;
        for mut key in [first, second] {
            if key < 0 {
                continue;
            }
            if view.swapped && matches!(key, 3 | 4) {
                key = 7 - key;
            }
            right -= 48.;
            let u = key as f32 * 0.125;
            let mut p = packet("key", [right, y, right + 48., y + 48.], PROJECTION);
            p.color = color;
            p.uv = [u, 0., u + 0.125, 0.5];
            out.push(p);
        }
        right -= 10.;
    }
    out
}
/// Host order: background; each row frame/glyphs/portrait; scrollbar; child108;
/// chrome; childD0. The caller supplies the native visible/overscan row cache.
pub fn frame(
    view: &Presentation,
    rows: &[Row],
    range: Option<ScrollRange>,
    selected_voiced_message: bool,
) -> Frame {
    Frame {
        background: background(view.progress),
        rows: rows.iter().map(|&row| row_packets(view, row)).collect(),
        scrollbar: range.map_or_else(Vec::new, |r| scrollbar(view.progress, r)),
        chrome: chrome(view, selected_voiced_message),
    }
}
