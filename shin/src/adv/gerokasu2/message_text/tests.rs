use super::*;
use shin_core::{
    format::font::gerokasu2::{Gerokasu2FontHeader, Gerokasu2GlyphInfo},
    layout::font::TextFontMetrics,
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand, Stdio},
};

fn geometry(size: u16, bearing: (i16, i16), dimensions: (u16, u16)) -> GlyphTextureGeometry {
    Gerokasu2GlyphInfo {
        bearing_x: bearing.0,
        bearing_y: bearing.1,
        width: dimensions.0,
        height: dimensions.1,
        advance_sixteenths: 205,
        word_10: 0,
        identity: 123,
        mip_count: 1,
        decoded_size: u32::from(dimensions.0) * u32::from(dimensions.1),
    }
    .texture_geometry(
        0x1f408,
        &Gerokasu2FontHeader {
            version: 1,
            file_size: 0,
            flags: 0,
            page_count: 0,
            glyph_count: 0,
            metric_words: [size, 0, 70, 10, 41, 0],
        },
    )
}

#[test]
fn message_overdraw_scale_thresholds_and_fade_use_the_later_consumer() {
    let g = geometry(20, (3, 20), (50, 60));
    let draw = MessageGlyphDraw::new(&g, vec2(100.25, 201.5), vec2(0.8, 1.25), 43.75, 50.0, false)
        .unwrap();
    let position = draw.vertices[0].position;
    assert!(position.z < 0.0 && position.w < 0.0);
    assert!(draw.vertices[3].color > 1.0);
    let ruby = MessageGlyphDraw::new(&g, Vec2::ZERO, Vec2::ONE, 43.75, 50.0, true).unwrap();
    assert!(ruby.vertices.iter().all(|v| v.color == 0.0));
    for progress in [0.0, -1.0, f32::NAN] {
        assert!(MessageGlyphDraw::colors(UnormColor::WHITE, progress).is_none());
    }
    let [outline, fill] = MessageGlyphDraw::colors(UnormColor(0), 0.4).unwrap();
    assert_eq!(outline, FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.3));
    assert_eq!(fill, FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.4));
    let large_range = MessageGlyphDraw::new(
        &geometry(1, (0, 0), (1, 1)),
        Vec2::ZERO,
        vec2(0.4, 0.4),
        0.0,
        1.0,
        false,
    )
    .unwrap();
    assert!(
        large_range.fill_thresholds.y > 1.0,
        "Message fill has no upper clamp"
    );
    assert!(MessageGlyphDraw::new(&g, Vec2::ZERO, Vec2::ZERO, 0.0, 50.0, false).is_err());
    assert!(MessageGlyphDraw::new(&g, Vec2::ZERO, Vec2::ONE, f32::NAN, 50.0, false).is_err());
}

#[derive(Clone, Copy)]
struct Row {
    g: GlyphTextureGeometry,
    position: Vec2,
    scale: Vec2,
    baseline: f32,
    height: f32,
    progress: f32,
    color: UnormColor,
    ruby: bool,
}
impl Row {
    fn draw(&self) -> MessageGlyphDraw {
        MessageGlyphDraw::new(
            &self.g,
            self.position,
            self.scale,
            self.baseline,
            self.height,
            self.ruby,
        )
        .unwrap()
    }
    fn write(&self, input: &mut ChildStdin) {
        let g = self.g;
        let record = [
            1,
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
        for word in record {
            row.extend(word.to_le_bytes());
        }
        for value in [
            self.position.x,
            self.position.y,
            self.scale.x,
            self.scale.y,
            self.baseline,
            self.height,
            self.progress,
        ] {
            row.extend(value.to_le_bytes());
        }
        // Native prepared characters store ARGB; engine UnormColor stores RGBA bytes.
        let rgba = self.color.0;
        let argb = (rgba & 0xff00ff00) | ((rgba & 255) << 16) | ((rgba >> 16) & 255);
        row.extend(argb.to_le_bytes());
        row.extend(u32::from(self.ruby).to_le_bytes());
        assert_eq!(row.len(), 124);
        input.write_all(&row).unwrap();
    }
}

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    batches: usize,
    cases: usize,
    packets: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = ProcessCommand::new(
            std::env::var_os("SHIN_GEROKASU2_MESSAGE_TEXT_ORACLE").expect("message text oracle"),
        )
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
            batches: 0,
            cases: 0,
            packets: 0,
        }
    }
    fn batch(&mut self, rows: &[Row]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(&(rows.len() as u32).to_le_bytes()).unwrap();
        for row in rows {
            row.write(input);
        }
        let mut header = [0; 8];
        self.output.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"GMT1");
        assert_eq!(
            u32::from_le_bytes(header[4..].try_into().unwrap()) as usize,
            rows.len()
        );
        for (index, row) in rows.iter().enumerate() {
            let mut bytes = [0; 80];
            self.output.read_exact(&mut bytes).unwrap();
            let actual = bytes
                .chunks_exact(4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                .collect::<Vec<_>>();
            let expected = row
                .draw()
                .vertices
                .iter()
                .flat_map(|v| {
                    let position = v.position;
                    position.to_array().into_iter().chain([v.color])
                })
                .map(f32::to_bits)
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected,
                "batch{} glyph{index} U+{:x}",
                self.batches, row.g.codepoint
            );
        }
        let expected_packets = rows.iter().filter(|r| r.progress > 0.0).count() * 2;
        let mut count = [0; 4];
        self.output.read_exact(&mut count).unwrap();
        assert_eq!(u32::from_le_bytes(count) as usize, expected_packets);
        for outline in [true, false] {
            for (index, row) in rows.iter().enumerate() {
                let Some(colors) = MessageGlyphDraw::colors(row.color, row.progress) else {
                    continue;
                };
                let d = row.draw();
                let thresholds = if outline {
                    d.outline_thresholds
                } else {
                    d.fill_thresholds
                };
                let color = colors[usize::from(!outline)].into_vec4().to_array();
                let mut expected = vec![5, 2];
                expected.extend(thresholds.to_array().map(f32::to_bits));
                for _ in 0..2 {
                    expected.extend(color.map(f32::to_bits));
                }
                expected.extend(Mat4::IDENTITY.to_cols_array().map(f32::to_bits));
                expected.extend([index as u32 * 4, 1, 4, 0]);
                let mut bytes = [0; 128];
                self.output.read_exact(&mut bytes).unwrap();
                let actual = bytes
                    .chunks_exact(4)
                    .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual, expected,
                    "batch{} glyph{index} outline{outline}",
                    self.batches
                );
            }
        }
        self.batches += 1;
        self.cases += rows.len();
        self.packets += expected_packets;
    }
    fn finish(mut self, name: &str) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_MESSAGE_TEXT_{name} batches={} cases={} packets={} vertex_bytes={} packet_bytes={} original_quads_fade_thresholds_colors_pass_order=verified",
            self.batches,
            self.cases,
            self.packets,
            self.cases * 80,
            self.packets * 128
        );
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU message text oracle"]
fn native_message_quads_colors_fade_and_order_match_wide_signed_inputs() {
    let mut n = Native::new();
    n.batch(&[]);
    let mut rows = Vec::new();
    for size in [1, 15, 20] {
        for bearing in [(-300, 500), (0, 0), (i16::MIN, i16::MAX)] {
            for dimensions in [(1, 1), (80, 160), (1024, 8192)] {
                let g = geometry(size, bearing, dimensions);
                for position in [
                    Vec2::ZERO,
                    vec2(100.25, -200.875),
                    vec2(-100000.0, 100000.0),
                ] {
                    for scale in [Vec2::ONE, vec2(0.8, 1.25), vec2(0.4, 0.4), vec2(0.0, 0.5)] {
                        for (baseline, height) in [(43.75, 50.0), (-70.125, 99.5), (32767.25, 1.0)]
                        {
                            for ruby in [false, true] {
                                rows.push(Row {
                                    g,
                                    position,
                                    scale,
                                    baseline,
                                    height,
                                    progress: 0.4,
                                    color: UnormColor::from_rgba(49, 117, 160, 64),
                                    ruby,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    for chunk in rows.chunks(128) {
        n.batch(chunk);
    }
    for value in 0..256u32 {
        let g = geometry(20, (3, 20), (50, 60));
        let mut rows = Vec::new();
        for progress in [
            -1.0,
            0.0,
            -0.0,
            f32::NAN,
            0.001,
            0.125,
            0.4,
            1.0,
            2.0,
            f32::INFINITY,
        ] {
            rows.push(Row {
                g,
                position: Vec2::ZERO,
                scale: vec2(0.9697, 0.625),
                baseline: 43.75,
                height: 50.0,
                progress,
                color: UnormColor::from_rgba(
                    value as u8,
                    (255 - value) as u8,
                    (value * 37) as u8,
                    (value * 13) as u8,
                ),
                ruby: false,
            });
        }
        n.batch(&rows);
    }
    n.finish("SYNTHETIC");
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU message text oracle"]
fn native_every_supplied_mapping_reaches_message_quad_and_sdf_packets() {
    use shin_core::{
        format::{font::gerokasu2::Gerokasu2Font, rom::RomReader},
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
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Gerokasu2Font::new(bytes.into()).unwrap();
        let scale = 50.0 / font.line_height();
        let mut rows = Vec::new();
        for cp in font
            .codepoints()
            .map(|(cp, _)| cp)
            .chain([0, 0x10ffff, 0x110000, u32::MAX])
        {
            let progress = [-0.3, 0.0, 0.001, 0.125, 0.4, 1.0, f32::NAN, 2.0][rows.len() % 8];
            rows.push(Row {
                g: font.lookup(cp).info.texture_geometry(cp, font.header()),
                position: vec2((cp & 15) as f32 * 31.375, 201.25),
                scale: vec2(scale * 0.9697, scale),
                baseline: font.baseline(50.0),
                height: 50.0,
                progress,
                color: UnormColor::from_rgba(
                    cp as u8,
                    (cp >> 8) as u8,
                    cp.wrapping_mul(37) as u8,
                    cp.wrapping_mul(13) as u8,
                ),
                ruby: cp & 16 != 0,
            });
        }
        for chunk in rows.chunks(128) {
            n.batch(chunk);
        }
    }
    n.finish("SUPPLIED");
}
