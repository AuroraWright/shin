use std::{
    collections::BTreeSet,
    io::{Read, Write},
};

use super::*;

#[derive(Clone, Copy)]
enum Encoding {
    Raw,
    Deflate,
    Lz,
}

fn fixture(flags: u32, encoding: Encoding, fallback: bool, mipmaps: bool) -> Bytes {
    let pages: Vec<(u16, Vec<(u8, u16)>)> = if fallback {
        vec![
            (0, vec![(b'A', 0), (b'X', 42)]),
            (0x20, vec![(0x3b, 1)]),
            (0x1f6, vec![(0, 0)]),
        ]
    } else {
        vec![(0, vec![(b'A', 0), (b'X', 42)])]
    };
    let count = if fallback { 2 } else { 1 };
    let offset = HEADER_SIZE + pages.len() * PAGE_SIZE + count * DESCRIPTOR_SIZE;
    let mut bytes = b"FNT6".to_vec();
    for word in [1u32, 0, flags] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend((pages.len() as u16).to_le_bytes());
    bytes.extend((count as u16).to_le_bytes());
    for word in [20u16, 0, 70, 10, 41, 0] {
        bytes.extend(word.to_le_bytes());
    }
    for (tag, entries) in pages {
        bytes.extend(tag.to_le_bytes());
        let mut ids = [u16::MAX; 256];
        for (low, id) in entries {
            ids[usize::from(low)] = id;
        }
        for id in ids {
            bytes.extend(id.to_le_bytes());
        }
    }
    let mut payloads = Vec::new();
    for index in 0..count {
        let (width, height, levels, deltas) = if mipmaps {
            (8u16, 4u16, 2u32, vec![0; 40])
        } else if index == 0 {
            // Final alpha: [250, 4, 3, 6, 254, 127]. Both horizontal and
            // vertical reconstruction wrap, with horizontal deltas only on row 0.
            (3, 2, 1, vec![121, 10, 255, 12, 250, 124])
        } else {
            (2, 2, 1, vec![0, 126, 127, 2])
        };
        let (payload, compressed) = match encoding {
            Encoding::Raw => (deltas, false),
            Encoding::Deflate => {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&deltas).unwrap();
                (encoder.finish().unwrap(), true)
            }
            Encoding::Lz => {
                let compressed = if mipmaps {
                    // One zero literal, then an overlapping backward reference.
                    vec![2, 0, 0x90, 0]
                } else {
                    let mut result = Vec::new();
                    for literals in deltas.chunks(8) {
                        result.push(0);
                        result.extend(literals);
                    }
                    result
                };
                (compressed, true)
            }
        };
        for word in [-300i16, 500] {
            bytes.extend(word.to_le_bytes());
        }
        for word in [width, height, 205, 211] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend((0x8765_4321u32 + index as u32).to_le_bytes());
        let packed = ((levels - 1) << 24) | if compressed { payload.len() as u32 } else { 0 };
        bytes.extend(packed.to_le_bytes());
        bytes.extend(((offset + payloads.len()) as u32).to_le_bytes());
        payloads.extend(payload);
    }
    assert_eq!(bytes.len(), offset);
    bytes.extend(payloads);
    let length = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes.into()
}

#[test]
fn sparse_unicode_fractional_metrics_fallback_and_pixel_modes() {
    for (flags, encoding) in [
        (0, Encoding::Lz),
        (1, Encoding::Deflate),
        (2, Encoding::Lz),
        (3, Encoding::Deflate),
        (2, Encoding::Raw),
    ] {
        let font = Gerokasu2Font::new(fixture(flags, encoding, true, false)).unwrap();
        assert_eq!(
            font.header().native_owner_metrics(),
            [121, 70, 10, 41, 0, 41, 20]
        );
        let glyph = font.lookup(u32::from('A'));
        assert!(glyph.exact);
        assert_eq!((glyph.info.bearing_x, glyph.info.bearing_y), (-300, 500));
        assert_eq!(glyph.info.advance_width(), 12.8125);
        assert_eq!(glyph.info.word_10, 211);
        assert_eq!(
            glyph.decode().unwrap().mipmaps[0].as_raw(),
            &[250, 4, 3, 6, 254, 127]
        );
        assert!(font.lookup(u32::from('😀')).exact);
        for cp in [u32::from('B'), u32::from('X'), 0x10ffff] {
            let glyph = font.lookup(cp);
            assert!(!glyph.exact);
            assert_eq!(glyph.glyph_id, Some(GlyphId(1)));
            assert_eq!(
                glyph.decode().unwrap().mipmaps[0].as_raw(),
                &[129, 255, 0, 1]
            );
        }
        let invalid = font.lookup(0x110000);
        assert!(invalid.glyph_id.is_none());
        assert_eq!(invalid.decode().unwrap().mipmaps[0].as_raw(), &[129]);
        assert_eq!(
            font.codepoints().collect::<Vec<_>>(),
            [
                (65, GlyphId(0)),
                (0x203b, GlyphId(1)),
                (0x1f600, GlyphId(0))
            ]
        );
    }
    let font = Gerokasu2Font::new(fixture(1, Encoding::Deflate, false, false)).unwrap();
    assert!(font.lookup(0x203b).glyph_id.is_none());
    assert_eq!(
        font.lookup(0x203b).decode().unwrap().mipmaps[0].as_raw(),
        &[129]
    );
    for (flags, encoding) in [
        (0, Encoding::Lz),
        (1, Encoding::Deflate),
        (0, Encoding::Raw),
    ] {
        let font = Gerokasu2Font::new(fixture(flags, encoding, false, true)).unwrap();
        let glyph = font.lookup(65).decode().unwrap();
        assert_eq!(
            glyph
                .mipmaps
                .iter()
                .map(GrayImage::dimensions)
                .collect::<Vec<_>>(),
            [(8, 4), (4, 2)]
        );
        assert_eq!(
            glyph
                .mipmaps
                .iter()
                .flat_map(|image| image.as_raw())
                .copied()
                .collect::<Vec<_>>(),
            vec![129; 40]
        );
    }
}

#[test]
fn malformed_fnt6_tables_payloads_and_streams_return_errors() {
    let data = fixture(1, Encoding::Deflate, true, false);
    for length in 0..data.len() {
        let mut truncated = data[..length].to_vec();
        if length >= 12 {
            truncated[8..12].copy_from_slice(&(length as u32).to_le_bytes());
        }
        assert!(
            Gerokasu2Font::new(truncated.into()).is_err(),
            "accepted prefix {length}"
        );
    }
    let descriptors = HEADER_SIZE + 3 * PAGE_SIZE;
    for (position, value) in [
        (4, 2u32),
        (8, 1),
        (descriptors + 20, 4),
        (descriptors + 16, 0xffffff),
    ] {
        let mut damaged = data.to_vec();
        damaged[position..position + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Gerokasu2Font::new(damaged.into()).is_err());
    }
    let mut damaged = data.to_vec();
    damaged[HEADER_SIZE + PAGE_SIZE..HEADER_SIZE + PAGE_SIZE + 2]
        .copy_from_slice(&0u16.to_le_bytes());
    assert!(Gerokasu2Font::new(damaged.into()).is_err());
    let mut damaged = data.to_vec();
    damaged[descriptors + 4..descriptors + 6].copy_from_slice(&0u16.to_le_bytes());
    let font = Gerokasu2Font::new(damaged.into()).unwrap();
    assert!(font.lookup(65).decode().is_err());
    let mut damaged = fixture(0, Encoding::Raw, false, true).to_vec();
    let descriptor = HEADER_SIZE + PAGE_SIZE;
    damaged[descriptor + 4..descriptor + 8].copy_from_slice(&[3, 0, 3, 0]);
    let font = Gerokasu2Font::new(damaged.into()).unwrap();
    assert!(font.lookup(65).decode().is_err());
    let payload = b"\x01\x03\x00\xfc\xffabc"; // raw DEFLATE stored block, no zlib header
    assert_eq!(inflate_exact(payload, 3).unwrap(), b"abc");
    for length in 0..payload.len() {
        assert!(inflate_exact(&payload[..length], 3).is_err());
    }
    for length in [0, 2, 4] {
        assert!(inflate_exact(payload, length).is_err());
    }
    let mut trailing = payload.to_vec();
    trailing.push(0);
    assert!(inflate_exact(&trailing, 3).is_err());
}

#[test]
fn bounded_lz_reports_invalid_tokens_and_output_spans() {
    let stream = [2, 0, 0x90, 0];
    assert_eq!(
        lz77::decompress_checked::<10>(&stream, 40).unwrap(),
        vec![0; 40]
    );
    for (input, size) in [
        (&stream[..3], 40),
        (&[1, 0, 0][..], 3),
        (&stream[..], 39),
        (&stream[..], 41),
    ] {
        assert!(lz77::decompress_checked::<10>(input, size).is_err());
    }
    assert!(lz77::decompress_checked::<16>(&stream, 40).is_err());
    let mut legacy = Vec::new();
    lz77::decompress::<10>(&stream, &mut legacy);
    assert_eq!(legacy, lz77::decompress_checked::<10>(&stream, 40).unwrap());
}

fn compare_native(font: &Gerokasu2Font, codepoints: &[u32]) -> usize {
    use std::process::{Command, Stdio};
    let mut child = Command::new(
        std::env::var_os("SHIN_GEROKASU2_FONT_ORACLE").expect("SHIN_GEROKASU2_FONT_ORACLE"),
    )
    .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("SHIN_GEROKASU2_EXECUTABLE"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
    let mut request = child.stdin.take().unwrap();
    let mut response = child.stdout.take().unwrap();
    request.write_all(b"NFG1").unwrap();
    request
        .write_all(&(font.data.len() as u32).to_le_bytes())
        .unwrap();
    request.write_all(&font.data).unwrap();
    let mut total = 0;
    for &cp in codepoints {
        request.write_all(&cp.to_le_bytes()).unwrap();
        let mut header = [0u8; 48];
        response
            .read_exact(&mut header)
            .unwrap_or_else(|error| panic!("native font response U+{cp:04X}: {error}"));
        assert_eq!(&header[..4], b"NFO1");
        let lookup = font.lookup(cp);
        assert_eq!(
            u32_at(&header, 4),
            u32::from(lookup.exact),
            "lookup U+{cp:04X}"
        );
        let info = lookup.info;
        let mut expected = Vec::new();
        expected.extend(cp.to_le_bytes());
        expected.extend(info.bearing_x.to_le_bytes());
        expected.extend(info.bearing_y.to_le_bytes());
        expected.extend(info.width.to_le_bytes());
        expected.extend(info.height.to_le_bytes());
        expected.extend(info.advance_width().to_bits().to_le_bytes());
        expected.extend(info.mip_count.to_le_bytes());
        expected.extend(0u16.to_le_bytes());
        expected.extend(info.decoded_size.to_le_bytes());
        expected.extend(info.identity.to_le_bytes());
        assert_eq!(&header[8..36], &expected, "metadata U+{cp:04X}");
        let (flag, compressed, offset) = lookup
            .glyph_id
            .map(|id| {
                let descriptor = &font.descriptors[id.0 as usize];
                (
                    font.header.flags & 1,
                    descriptor.compressed_size as u32,
                    descriptor.data_offset as u32,
                )
            })
            .unwrap_or((0, 0, 0));
        assert_eq!(
            (
                u32_at(&header, 36),
                u32_at(&header, 40),
                u32_at(&header, 44)
            ),
            (flag, compressed, offset)
        );
        let mut pixels = vec![0; info.decoded_size as usize];
        response.read_exact(&mut pixels).unwrap();
        let glyph = lookup.decode().unwrap();
        let decoded = glyph
            .mipmaps
            .iter()
            .flat_map(|level| level.as_raw())
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(pixels, decoded, "pixel bytes U+{cp:04X}");
        total += pixels.len();
    }
    drop(request);
    assert_eq!(
        response.read(&mut [0u8; 1]).unwrap(),
        0,
        "unexpected native output"
    );
    assert!(child.wait().unwrap().success());
    total
}

#[test]
#[ignore = "requires supplied Gerokasu2 executable and guarded CPU font oracle"]
fn gerokasu2_fnt6_native_synthetic_lookup_and_pixels() {
    let mut cases = 0;
    for (flags, encoding) in [
        (0, Encoding::Lz),
        (1, Encoding::Deflate),
        (2, Encoding::Lz),
        (3, Encoding::Deflate),
        (2, Encoding::Raw),
    ] {
        let font = Gerokasu2Font::new(fixture(flags, encoding, true, false)).unwrap();
        let codepoints = [65, 65, 66, 66, 88, 0x203b, 0x1f600, 0x10ffff, 0x110001];
        compare_native(&font, &codepoints);
        cases += codepoints.len();
    }
    for (flags, encoding) in [
        (0, Encoding::Lz),
        (1, Encoding::Deflate),
        (0, Encoding::Raw),
    ] {
        let font = Gerokasu2Font::new(fixture(flags, encoding, false, true)).unwrap();
        let codepoints = [65, 65, 66, 66, 0x203b, 0x1f600];
        compare_native(&font, &codepoints);
        cases += codepoints.len();
    }
    println!(
        "FNT6_NATIVE_SYNTHETIC cases={cases} modes=raw,lz,deflate fallback=verified mips=verified supplementary=verified"
    );
}

#[test]
#[ignore = "requires supplied Gerokasu2 ROM/executable and guarded CPU font oracle"]
fn gerokasu2_fnt6_supplied_fonts_match_native_byte_for_byte() {
    use crate::format::rom::RomReader;
    use shin_primitives::stateless_reader::StatelessFile;
    use std::fs::File;
    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Gerokasu2Font::new(bytes.into()).unwrap();
        let mappings = font.codepoints().collect::<Vec<_>>();
        let ids = mappings.iter().map(|(_, id)| *id).collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), font.descriptors.len());
        let mut codepoints = mappings.iter().map(|&(cp, _)| cp).collect::<Vec<_>>();
        assert_eq!(codepoints.len(), 9964);
        assert_eq!(font.header.page_count, 125);
        let glyph_bytes = ids
            .iter()
            .map(|id| font.descriptors[id.0 as usize].info.decoded_size as usize)
            .sum::<usize>();
        // Repeated exact and fallback requests also exercise the native cache.
        codepoints.extend([65, 65, 0x1f600, 0x1f600, 0x110001]);
        let bytes = compare_native(&font, &codepoints);
        println!(
            "FNT6_NATIVE path={path} codepoints={} glyphs={} glyph_bytes={glyph_bytes} cases={} compared_bytes={bytes}",
            mappings.len(),
            ids.len(),
            codepoints.len()
        );
    }
}
