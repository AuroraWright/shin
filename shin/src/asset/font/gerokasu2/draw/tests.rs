use super::*;
use shin_core::format::font::gerokasu2::{Gerokasu2Font, Gerokasu2FontHeader, Gerokasu2GlyphInfo};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

fn header(size: u16) -> Gerokasu2FontHeader {
    Gerokasu2FontHeader {
        version: 1,
        file_size: 0,
        flags: 0,
        page_count: 0,
        glyph_count: 0,
        metric_words: [size, 0, 70, 10, 41, 0],
    }
}
fn info(x: i16, y: i16, w: u16, h: u16) -> Gerokasu2GlyphInfo {
    Gerokasu2GlyphInfo {
        bearing_x: x,
        bearing_y: y,
        width: w,
        height: h,
        advance_sixteenths: 205,
        word_10: 0,
        identity: 123,
        mip_count: 1,
        decoded_size: u32::from(w) * u32::from(h),
    }
}
const COLORS: [[f32; 4]; 3] = [
    [1.0, 0.75, 0.25, 0.5],
    [0.0, 0.3, 0.9, 0.8],
    [0.0, 0.0, 0.0, 0.94],
];

#[test]
fn sdf_draw_rejects_unusable_inputs_and_keeps_padded_uv_and_unclamped_gradient() {
    let g = info(-300, 500, 8, 4).texture_geometry(0x1f408, &header(20));
    let d = GlyphDraw::new(&g, vec2(100.25, 50.5), vec2(0.8, 1.25), 50.0, false).unwrap();
    assert_eq!(d.vertices[0].position.z.to_bits(), (-0.0f32).to_bits());
    let last_position = d.vertices[3].position;
    assert_eq!(last_position.z, 1.0);
    assert!(
        d.vertices[0].color < 0.0,
        "Gradient extrapolation is retained"
    );
    assert_eq!(d.fill_thresholds, vec2(-0.075, 0.075));
    let ruby = GlyphDraw::new(&g, Vec2::ZERO, Vec2::ONE, 50.0, true).unwrap();
    assert!(ruby.vertices.iter().all(|v| v.color == 0.0));
    for scale in [Vec2::ZERO, vec2(-1.0, 1.0), vec2(f32::INFINITY, 1.0)] {
        assert!(GlyphDraw::new(&g, Vec2::ZERO, scale, 50.0, false).is_err());
    }
    assert!(GlyphDraw::new(&g, Vec2::NAN, Vec2::ONE, 50.0, false).is_err());
    for height in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(GlyphDraw::new(&g, Vec2::ZERO, Vec2::ONE, height, false).is_err());
    }
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut invalid = g;
        invalid.extra_metric = size;
        assert!(GlyphDraw::new(&invalid, Vec2::ZERO, Vec2::ONE, 50.0, false).is_err());
    }
}

struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    cases: usize,
}
impl Native {
    fn new() -> Self {
        let mut child =
            Command::new(std::env::var_os("SHIN_GEROKASU2_FONT_DRAW_ORACLE").expect("draw oracle"))
                .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("executable"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            cases: 0,
        }
    }
    fn compare(
        &mut self,
        g: GlyphTextureGeometry,
        position: Vec2,
        scale: Vec2,
        height: f32,
        ruby: bool,
    ) {
        let record = [
            1u32,
            1,
            g.identity,
            g.codepoint,
            g.origin[0] as u32,
            g.origin[1] as u32,
            g.dimensions[0],
            g.dimensions[1],
            g.advance.to_bits(),
            g.font_metrics[0] as u32,
            g.font_metrics[1] as u32,
            g.font_metrics[2] as u32,
            g.scale[0].to_bits(),
            g.scale[1].to_bits(),
            g.extra_metric.to_bits(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let mut row = Vec::new();
        for v in record {
            row.extend(v.to_le_bytes());
        }
        for v in [position.x, position.y, scale.x, scale.y, height] {
            row.extend(v.to_le_bytes());
        }
        row.extend(u32::from(ruby).to_le_bytes());
        for c in COLORS {
            for v in c {
                row.extend(v.to_le_bytes());
            }
        }
        assert_eq!(row.len(), 160);
        self.input.as_mut().unwrap().write_all(&row).unwrap();
        let mut prefix = [0u8; 88];
        if let Err(error) = self.output.read_exact(&mut prefix) {
            let status = self.child.wait().unwrap();
            panic!(
                "native draw case{} U+{:x}: {error}; {status}",
                self.cases, g.codepoint
            );
        }
        assert_eq!(&prefix[..4], b"GFD1");
        assert_eq!(u32::from_le_bytes(prefix[84..].try_into().unwrap()), 2);
        let d = GlyphDraw::new(&g, position, scale, height, ruby).unwrap();
        let expected = d
            .vertices
            .iter()
            .flat_map(|v| {
                let position = v.position;
                position.to_array().into_iter().chain([v.color])
            })
            .map(f32::to_bits)
            .collect::<Vec<_>>();
        let actual = prefix[4..84]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "native vertices case{} U+{:x}",
            self.cases, g.codepoint
        );
        for (outline, thresholds) in [(true, d.outline_thresholds), (false, d.fill_thresholds)] {
            let mut packet = [0u8; 128];
            self.output.read_exact(&mut packet).unwrap();
            let words = packet
                .chunks_exact(4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(&words[..2], &[5, 2]);
            assert_eq!(&words[2..4], &thresholds.to_array().map(f32::to_bits));
            let colors = if outline {
                [COLORS[2], COLORS[2]]
            } else {
                [COLORS[0], COLORS[1]]
            };
            assert_eq!(
                &words[4..12],
                &colors
                    .into_iter()
                    .flatten()
                    .map(f32::to_bits)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                &words[12..28],
                &Mat4::IDENTITY.to_cols_array().map(f32::to_bits)
            );
            assert_eq!(&words[28..], &[0, 1, 4, 0]);
        }
        self.cases += 1;
    }
    fn finish(mut self) -> usize {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        self.cases
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU font draw oracle"]
fn native_padded_quads_gradient_program5_and_thresholds_match_wide_signed_metrics() {
    let mut n = Native::new();
    for size in [1, 15, 20] {
        for (x, y) in [(-300, 500), (0, 0), (i16::MIN, i16::MAX)] {
            for (w, h) in [(1, 1), (80, 160), (1024, 8192)] {
                let g = info(x, y, w, h).texture_geometry(0x1f408, &header(size));
                for position in [
                    Vec2::ZERO,
                    vec2(100.25, -200.875),
                    vec2(-100000.0, 100000.0),
                ] {
                    for scale in [Vec2::ONE, vec2(0.8, 1.25), vec2(0.4, 0.4)] {
                        for height in [1.0, 50.0, 99.5] {
                            for ruby in [false, true] {
                                n.compare(g, position, scale, height, ruby);
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "GEROKASU2_FONT_DRAW_SYNTHETIC cases={} vertex_bytes=80 packets=2 program5_thresholds_colors_matrix=verified",
        n.finish()
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU font draw oracle"]
fn native_all_supplied_cached_geometry_reaches_sdf_quads_and_draw_packets() {
    use shin_core::{
        format::rom::RomReader, layout::font::TextFontMetrics,
        primitives::stateless_reader::StatelessFile,
    };
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut n = Native::new();
    let mut mappings = 0;
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Gerokasu2Font::new(bytes.into()).unwrap();
        let scale = 50.0 / font.line_height();
        for (cp, _) in font.codepoints() {
            n.compare(
                font.lookup(cp).info.texture_geometry(cp, font.header()),
                vec2((cp & 15) as f32 * 31.375, 201.25),
                vec2(scale * 0.9697, scale),
                50.0,
                cp & 16 != 0,
            );
            mappings += 1;
        }
        for cp in [0, 0x10ffff, 0x110000, u32::MAX] {
            n.compare(
                font.lookup(cp).info.texture_geometry(cp, font.header()),
                Vec2::ZERO,
                vec2(scale * 0.9697, scale),
                50.0,
                false,
            );
        }
    }
    println!(
        "GEROKASU2_FONT_DRAW_SUPPLIED mappings={mappings} cases={} vertex_bytes=80 packets=2 program5_thresholds_colors_matrix=verified",
        n.finish()
    );
}
