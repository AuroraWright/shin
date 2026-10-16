//! Original85710 draw/text submissions, before GenericText glyph layout.
use super::Entry;
pub const FONT_SIZE: f32 = 48.;
pub const TEXT_WIDTH: f32 = 1280.;
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
pub const NO_STENCIL: [u32; 5] = [7, 0, 0, 0, 65535];
pub const CLIP_WRITE: [u32; 5] = [7, 0, 0, 3, 16777215];
/// Native later submits GenericText while compare=Equal/ref=1/write-mask=0.
pub const CLIP_TEXT: [u32; 5] = [2, 0, 0, 0, 0x01ffff];
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub texture: Option<&'static str>,
    pub rect: [f32; 4],
    pub colors: [u32; 4],
    pub matrix: [f32; 16],
    pub blend: u32,
    pub stencil: [u32; 5],
}
#[derive(Clone, Copy, Debug)]
pub enum Marker<'a> {
    Chapter(&'a str),
    Choice { title: &'a str, text: &'a str },
    Quiz,
    Other,
}
#[derive(Clone, Debug)]
pub struct TextRow {
    pub index: usize,
    pub text: String,
    pub color: [f32; 4],
    pub position: [f32; 2],
    pub anchor: [f32; 2],
    pub scale: [f32; 2],
}
pub struct Frame {
    pub panel: [Quad; 2],
    pub clip: Quad,
    pub text_matrix: [f32; 16],
    pub rows: Vec<TextRow>,
    pub header: Quad,
    pub borders: [Quad; 2],
    pub scrollbar: Option<[Quad; 2]>,
}
fn translated(mut m: [f32; 16], x: f32, y: f32) -> [f32; 16] {
    m[12] = PROJECTION[0] * x + m[12];
    m[13] = PROJECTION[5] * y + m[13];
    m
}
fn quad(rect: [f32; 4], colors: [u32; 4], matrix: [f32; 16]) -> Quad {
    Quad {
        texture: None,
        rect,
        colors,
        matrix,
        blend: 2,
        stencil: NO_STENCIL,
    }
}
pub fn row_text(entry: Entry, marker: Marker<'_>) -> (String, [f32; 4]) {
    match entry.kind {
        0 => ("先頭".into(), [0.875, 0.875, 1., 1.]),
        97 => ("カーソル位置".into(), [0.75, 1., 0.75, 1.]),
        98 => ("ゲーム内現在位置".into(), [0.75, 1., 0.75, 1.]),
        99 => ("カーソル位置/ゲーム内現在位置".into(), [0.75, 1., 0.75, 1.]),
        100 => ("後尾".into(), [0.875, 0.875, 1., 1.]),
        1 => match marker {
            Marker::Chapter(title) => (title.into(), [1., 0.75, 0.75, 1.]),
            Marker::Choice { title, text } => (
                if title.is_empty() {
                    text.into()
                } else {
                    format!("{title}　{text}")
                },
                [0.75, 1., 1., 1.],
            ),
            Marker::Quiz => ("クイズ".into(), [0.75, 1., 1., 1.]),
            Marker::Other => (">>謎<<".into(), [1.; 4]),
        },
        _ => (">>謎<<".into(), [1.; 4]),
    }
}
/// Render order: panel; clear stencil0; clip (blend0); rows (CLIP_TEXT);
/// restore NO_STENCIL; header, borders, scrollbar. Rows reuse GenericText's
/// normal font/parser service with width1280, size48, flags [0,1,0,0,0,0].
pub fn frame<'a>(
    progress: f32,
    scroll: f32,
    entries: &[Entry],
    selected: usize,
    first: usize,
    last: usize,
    marker: impl Fn(i32) -> Marker<'a>,
) -> Frame {
    let slide = (-progress).mul_add(1920., 1920.);
    let matrix = translated(PROJECTION, slide, 0.);
    let panel = [
        quad([420., 0., 1920., 1080.], [0xc0808080; 4], matrix),
        quad(
            [0., 0., 420., 1080.],
            [0x00808080, 0xc0808080, 0x00808080, 0xc0808080],
            matrix,
        ),
    ];
    let mut clip = quad([0., 240., 1920., 960.], [0xffff0000; 4], matrix);
    clip.blend = 0;
    clip.stencil = CLIP_WRITE;
    let text_matrix = translated(matrix, 0., -(scroll + -240.));
    let rows = entries
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, entry)| {
            let top = (index * 60) as f32 - scroll;
            if top + 60. <= 0. || top >= 720. {
                return None;
            }
            let (text, mut color) = row_text(
                entry,
                if entry.kind == 1 {
                    marker(entry.marker_index)
                } else {
                    Marker::Other
                },
            );
            if index != selected {
                for value in &mut color[..3] {
                    *value *= 0.7;
                }
            }
            if entry.row < first || entry.row > last {
                color[3] *= 0.5;
            }
            Some(TextRow {
                index,
                text,
                color,
                position: [1800., ((index * 60) | 3) as f32],
                anchor: [1., 0.],
                scale: [1., 1.],
            })
        })
        .collect();
    let mut header = quad([1152., 144., 1800., 240.], [u32::MAX; 4], matrix);
    header.texture = Some("chapter");
    let borders = [[420., 234., 1920., 240.], [420., 960., 1920., 966.]]
        .map(|r| quad(r, [0xffffff, u32::MAX, 0xffffff, u32::MAX], matrix));
    let scrollbar = if entries.len() > 12 {
        let height = entries.len() as i32 * 60;
        let position = (scroll as i32) * 720;
        let start = position / height;
        let end = (position + 720 * 720) / height;
        Some([
            quad([1808., 240., 1832., 960.], [0x60101040; 4], matrix),
            quad(
                [
                    1808.,
                    (240 + start) as f32,
                    1832.,
                    (240 + start) as f32 + (end - start) as f32,
                ],
                [0xc0c0c0c0; 4],
                matrix,
            ),
        ])
    } else {
        None
    };
    Frame {
        panel,
        clip,
        text_matrix,
        rows,
        header,
        borders,
        scrollbar,
    }
}
