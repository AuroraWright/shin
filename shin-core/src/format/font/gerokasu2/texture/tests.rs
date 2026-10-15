use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

fn header(metrics: [u16; 6]) -> Gerokasu2FontHeader {
    Gerokasu2FontHeader {
        version: 1,
        file_size: 0,
        flags: 0,
        page_count: 0,
        glyph_count: 0,
        metric_words: metrics,
    }
}
fn info() -> Gerokasu2GlyphInfo {
    Gerokasu2GlyphInfo {
        bearing_x: -300,
        bearing_y: 500,
        width: 8,
        height: 4,
        advance_sixteenths: 205,
        word_10: 0,
        identity: 123,
        mip_count: 2,
        decoded_size: 40,
    }
}

#[test]
fn gerokasu2_prepared_mips_preserve_signed_bytes_borders_and_fractional_geometry() {
    let info = info();
    let sources = vec![
        GrayImage::from_raw(
            8,
            4,
            (0..32)
                .map(|i| [0, 127, 128, 129, 254, 255][i % 6])
                .collect(),
        )
        .unwrap(),
        GrayImage::from_raw(4, 2, vec![129, 255, 0, 1, 127, 128, 129, 254]).unwrap(),
    ];
    let glyph = Gerokasu2Glyph {
        info,
        mipmaps: sources.clone(),
    };
    let prepared = glyph
        .prepare_texture(0x1f408, &header([20, 0, 70, 10, 41, 0]))
        .unwrap();
    assert_eq!(prepared.geometry.origin, [-302, -502]);
    assert_eq!(prepared.geometry.dimensions, [12, 8]);
    assert_eq!(prepared.geometry.advance, 12.8125);
    for (level, (padded, source)) in prepared.mipmaps.iter().zip(&sources).enumerate() {
        let border = 2 >> level;
        for (x, y, pixel) in padded.enumerate_pixels() {
            let expected = if x >= border
                && y >= border
                && x < border + source.width()
                && y < border + source.height()
            {
                source.get_pixel(x - border, y - border).0[0]
            } else {
                0x81
            };
            assert_eq!(pixel.0[0], expected);
        }
    }
    assert_eq!(glyph.mipmaps, sources);
    let mut invalid = info;
    invalid.mip_count = 32;
    assert!(
        Gerokasu2Glyph {
            info: invalid,
            mipmaps: vec![]
        }
        .prepare_texture(65, &header([20, 0, 70, 10, 41, 0]))
        .is_err()
    );
    let mut invalid = info;
    invalid.width = 9;
    assert!(
        Gerokasu2Glyph {
            info: invalid,
            mipmaps: sources
        }
        .prepare_texture(65, &header([20, 0, 70, 10, 41, 0]))
        .is_err()
    );
}

struct NativeGeometry {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    count: usize,
}
impl NativeGeometry {
    fn new() -> Self {
        let mut child = Command::new(
            std::env::var_os("SHIN_GEROKASU2_FONT_GEOMETRY_ORACLE").expect("geometry oracle"),
        )
        .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("executable"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"GFG1").unwrap();
        Self {
            input: Some(input),
            output: child.stdout.take().unwrap(),
            child,
            count: 0,
        }
    }
    fn compare(&mut self, header: &Gerokasu2FontHeader, info: Gerokasu2GlyphInfo, cp: u32) {
        let mut row = Vec::new();
        for word in header.native_owner_metrics() {
            row.extend(word.to_le_bytes());
        }
        row.extend(0u16.to_le_bytes());
        row.extend(cp.to_le_bytes());
        row.extend(info.bearing_x.to_le_bytes());
        row.extend(info.bearing_y.to_le_bytes());
        row.extend(info.width.to_le_bytes());
        row.extend(info.height.to_le_bytes());
        row.extend(info.advance_width().to_le_bytes());
        row.extend(info.mip_count.to_le_bytes());
        row.extend(0u16.to_le_bytes());
        row.extend(info.decoded_size.to_le_bytes());
        row.extend(info.identity.to_le_bytes());
        row.extend(0u32.to_le_bytes());
        assert_eq!(row.len(), 48);
        self.input.as_mut().unwrap().write_all(&row).unwrap();
        let mut output = [0; 92];
        self.output.read_exact(&mut output).unwrap();
        assert_eq!(&output[..4], b"GEO1");
        let g = info.texture_geometry(cp, header);
        let mut expected = [0u32; 22];
        expected[..15].copy_from_slice(&[
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
        ]);
        let actual = output[4..]
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "native geometry case{} U+{cp:04x} {info:?}",
            self.count
        );
        self.count += 1;
    }
    fn finish(mut self) -> usize {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        self.count
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU font geometry oracle"]
fn gerokasu2_font_geometry_native_signed_metrics_mip_shifts_and_fractional_advances() {
    let mut native = NativeGeometry::new();
    for metrics in [
        [20, 0, 70, 10, 41, 0],
        [65535, 0, 32767, 32768, 65535, 1],
        [1, 0, 65535, 1, 32768, 65535],
    ] {
        let header = header(metrics);
        for x in [i16::MIN, -300, -1, 0, 1, 500, i16::MAX] {
            for y in [i16::MIN, -300, -1, 0, 1, 500, i16::MAX] {
                for (advance, width) in [(0, 0), (1, 1), (205, 3), (u16::MAX, u16::MAX)] {
                    for mip_count in [0, 1, 2, 16, 31, 32, 256] {
                        let mut info = info();
                        info.bearing_x = x;
                        info.bearing_y = y;
                        info.width = width;
                        info.height = width;
                        info.advance_sixteenths = advance;
                        info.mip_count = mip_count;
                        native.compare(&header, info, 0x1f408);
                    }
                }
            }
        }
    }
    println!(
        "GEROKASU2_FONT_GEOMETRY_SYNTHETIC cases={} record_bytes=88 all_geometry_words=verified",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU font geometry oracle"]
fn gerokasu2_font_geometry_all_supplied_mappings_and_fallbacks() {
    use crate::{
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
    let mut native = NativeGeometry::new();
    let mut mappings = 0;
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Gerokasu2Font::new(bytes.into()).unwrap();
        for (cp, _) in font.codepoints() {
            native.compare(font.header(), font.lookup(cp).info, cp);
            mappings += 1;
        }
        for cp in [0, 0x10ffff, 0x110000, u32::MAX] {
            native.compare(font.header(), font.lookup(cp).info, cp);
        }
        // Check actual supplied glyph data through the padded texture path;
        // the unchanged decoder already has exhaustive native pixel evidence.
        for cp in [u32::from('A'), u32::from('あ'), u32::from('※'), 0x110000] {
            let prepared = font
                .lookup(cp)
                .decode()
                .unwrap()
                .prepare_texture(cp, font.header())
                .unwrap();
            assert_eq!(
                prepared.mipmaps[0].dimensions(),
                (
                    prepared.geometry.dimensions[0],
                    prepared.geometry.dimensions[1]
                )
            );
            assert_eq!(prepared.mipmaps[0].get_pixel(0, 0).0[0], 0x81);
        }
    }
    println!(
        "GEROKASU2_FONT_GEOMETRY_SUPPLIED mappings={mappings} cases={} record_bytes=88 all_geometry_words=verified",
        native.finish()
    );
}
