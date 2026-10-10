use super::decompress;
use std::{
    io::{BufReader, BufWriter, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Lz77Oracle {
    child: Option<Child>,
    input: Option<BufWriter<ChildStdin>>,
    output: BufReader<ChildStdout>,
    response: Vec<u8>,
    streams: usize,
    bytes: u64,
}
impl Lz77Oracle {
    fn new() -> Self {
        let executable = std::env::var_os("SHIN_REFERENCE_EXECUTABLE")
            .expect("SHIN_REFERENCE_EXECUTABLE must point to supplied executable");
        let oracle = std::env::var_os("SHIN_REFERENCE_LZ77_ORACLE")
            .expect("SHIN_REFERENCE_LZ77_ORACLE must point to compiled native LZ77 oracle");
        let mut child = Command::new(oracle)
            .arg(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Launch guarded native LZ77 oracle");
        Self {
            input: Some(BufWriter::new(child.stdin.take().unwrap())),
            output: BufReader::new(child.stdout.take().unwrap()),
            child: Some(child),
            response: Vec::new(),
            streams: 0,
            bytes: 0,
        }
    }
    fn compare<const BITS: u32>(&mut self, compressed: &[u8], decoded: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(b"NLZ1").unwrap();
        input
            .write_all(&u32::try_from(compressed.len()).unwrap().to_le_bytes())
            .unwrap();
        input.write_all(&BITS.to_le_bytes()).unwrap();
        input.write_all(compressed).unwrap();
        input.flush().unwrap();
        let mut size = [0; 4];
        self.output
            .read_exact(&mut size)
            .expect("Native LZ77 response header");
        assert_eq!(
            u32::from_le_bytes(size) as usize,
            decoded.len(),
            "Native decoded size, offsetbits={BITS}"
        );
        self.response.resize(decoded.len(), 0);
        self.output
            .read_exact(&mut self.response)
            .expect("Native LZ77 output bytes");
        if let Some(byte) = decoded.iter().zip(&self.response).position(|(a, b)| a != b) {
            panic!(
                "Native LZ77 mismatch: offsetbits={BITS},byte={byte},rust={},native={}",
                decoded[byte], self.response[byte]
            );
        }
        self.streams += 1;
        self.bytes += decoded.len() as u64;
    }
    fn finish(mut self) -> (usize, u64) {
        drop(self.input.take());
        let status = self.child.take().unwrap().wait().unwrap();
        assert!(status.success(), "Native LZ77 process exited {status}");
        (self.streams, self.bytes)
    }
}
impl Drop for Lz77Oracle {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn supplied_lz77_oracle() -> &'static Option<std::sync::Mutex<Option<Lz77Oracle>>> {
    static ORACLE: std::sync::OnceLock<Option<std::sync::Mutex<Option<Lz77Oracle>>>> =
        std::sync::OnceLock::new();
    ORACLE.get_or_init(|| {
        (std::env::var("SHIN_REFERENCE_COMPARE_NATIVE_LZ77").as_deref() == Ok("1"))
            .then(|| std::sync::Mutex::new(Some(Lz77Oracle::new())))
    })
}
pub(super) fn compare_native_lz77_if_enabled<const BITS: u32>(compressed: &[u8], decoded: &[u8]) {
    if let Some(oracle) = supplied_lz77_oracle() {
        oracle
            .lock()
            .unwrap()
            .as_mut()
            .expect("Native LZ77 audit already finished")
            .compare::<BITS>(compressed, decoded);
    }
}
pub(crate) fn finish_native_lz77_comparison() -> Option<(usize, u64)> {
    supplied_lz77_oracle().as_ref().map(|oracle| {
        oracle
            .lock()
            .unwrap()
            .take()
            .expect("Native LZ77 audit already finished")
            .finish()
    })
}

#[derive(Clone, Copy)]
enum Token {
    Literal(u8),
    Reference { distance: u16, length: u16 },
}
fn pack<const BITS: u32>(tokens: &[Token]) -> Vec<u8> {
    let mut compressed = Vec::new();
    for group in tokens.chunks(8) {
        let flags = group.iter().enumerate().fold(0, |map, (i, token)| {
            map | if matches!(token, Token::Reference { .. }) {
                1 << i
            } else {
                0
            }
        });
        compressed.push(flags);
        for token in group {
            match *token {
                Token::Literal(value) => compressed.push(value),
                Token::Reference { distance, length } => {
                    assert!(distance > 0 && u32::from(distance) <= 1 << BITS);
                    assert!(length >= 3 && u32::from(length - 3) < 1 << (16 - BITS));
                    let value = ((length - 3) << BITS) | (distance - 1);
                    compressed.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
    }
    compressed
}
fn cases<const BITS: u32>(oracle: &mut Lz77Oracle) {
    let mut compare = |source: &[u8]| {
        let mut decoded = Vec::new();
        decompress::<BITS>(source, &mut decoded);
        oracle.compare::<BITS>(source, &decoded);
        // The public API appends to output. Independent blocks must give the
        // same suffix when an unrelated prefix already exists.
        let mut appended = vec![0x91, 0x27, 0xff];
        decompress::<BITS>(source, &mut appended);
        assert_eq!(&appended[..3], &[0x91, 0x27, 0xff]);
        assert_eq!(&appended[3..], decoded);
    };
    for count in [0, 1, 7, 8, 9, 15, 16, 17, 255, 256, 4096] {
        let literals: Vec<_> = (0..count).map(|i| Token::Literal((i * 71) as u8)).collect();
        compare(&pack::<BITS>(&literals));
    }
    let maximum_distance = 1 << BITS;
    let maximum_length = (1 << (16 - BITS)) + 2;
    for distance in [1, 2, 3, 7, 8, 9, 17, 33, 63, 64, 65, 127, maximum_distance] {
        for length in 3..=maximum_length {
            let mut tokens: Vec<_> = (0..distance)
                .map(|i| Token::Literal((usize::from(i) * 139 + 23) as u8))
                .collect();
            for _ in 0..17 {
                tokens.push(Token::Reference { distance, length });
            }
            compare(&pack::<BITS>(&tokens));
        }
    }
    for position in 0..8 {
        let mut tokens = vec![Token::Literal(b'A')];
        tokens.extend((0..position).map(|i| Token::Literal(i as u8)));
        tokens.push(Token::Reference {
            distance: 1,
            length: maximum_length,
        });
        while tokens.len() % 8 != 0 {
            tokens.push(Token::Literal(0x91));
        }
        let mut source = pack::<BITS>(&tokens);
        compare(&source);
        source.extend_from_slice(&[1, 0xff]); // partial final reference
        compare(&source);
    }
    for source in [
        vec![0xff],
        vec![1, 0xff],
        vec![2, b'A', 0xff],
        vec![0, b'A', 1, 0xff],
    ] {
        compare(&source);
    }
}

#[test]
#[ignore = "requires supplied executable and compiled native LZ77 oracle; no GPU/audio"]
fn reference_native_lz77_decoder() {
    let mut oracle = Lz77Oracle::new();
    cases::<10>(&mut oracle);
    cases::<12>(&mut oracle);
    let (streams, bytes) = oracle.finish();
    eprintln!(
        "Native LZ77 decoder: {streams} streams, {bytes} exact output bytes; offsets10/12, all encoded lengths, overlaps, flag boundaries, partial tails and guards checked"
    );
}

#[test]
fn truncated_final_reference_preserves_decoded_prefix() {
    fn check<const BITS: u32>() {
        for (source, expected) in [
            (&[1, 0xff][..], &[][..]),
            (&[2, b'A', 0xff][..], &[b'A'][..]),
            (&[2, b'A', 0, 0][..], &b"AAAA"[..]),
        ] {
            let mut decoded = vec![0x91, 0x27];
            decompress::<BITS>(source, &mut decoded);
            assert_eq!(&decoded[..2], &[0x91, 0x27]);
            assert_eq!(&decoded[2..], expected);
        }
    }
    check::<10>();
    check::<12>();
}

#[test]
#[ignore = "requires supplied ROM/executable and native LZ77 oracle with SHIN_REFERENCE_COMPARE_NATIVE_LZ77=1; no GPU/audio"]
fn reference_rom_all_fonts_and_masks_native_lz77() {
    use crate::{
        format::{
            font::{GlyphMipLevel, read_lazy_font},
            mask::read_mask,
            rom::{IndexEntry, RomReader},
        },
        primitives::stateless_reader::StatelessFile,
    };
    assert_eq!(
        std::env::var("SHIN_REFERENCE_COMPARE_NATIVE_LZ77").as_deref(),
        Ok("1")
    );
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let files: Vec<_> = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) => Some((path, *file)),
            _ => None,
        })
        .collect();
    assert_eq!(files.len(), 109_791);
    let mut fonts = 0;
    let mut masks = 0;
    let mut glyphs = 0;
    let mut glyph_pixels = 0u64;
    let mut mask_pixels = 0u64;
    let started = std::time::Instant::now();
    let mut next_progress = started;
    for (path, file) in files {
        if file.size() < 4 {
            continue;
        }
        let mut reader = rom.open_file(file).unwrap();
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        if magic != *b"FNT4" && magic != *b"MSK4" {
            continue;
        }
        let mut data = magic.to_vec();
        reader.read_to_end(&mut data).unwrap();
        if magic == *b"FNT4" {
            let font = read_lazy_font(&mut std::io::Cursor::new(&data))
                .unwrap_or_else(|e| panic!("Read all glyphs of {path}: {e}"));
            for glyph in font.get_glyphs().values() {
                let glyph = glyph.decompress();
                for level in [
                    GlyphMipLevel::Level0,
                    GlyphMipLevel::Level1,
                    GlyphMipLevel::Level2,
                    GlyphMipLevel::Level3,
                ] {
                    glyph_pixels += glyph.get_image(level).as_raw().len() as u64;
                }
                glyphs += 1;
            }
            fonts += 1;
        } else {
            let mask = read_mask(&data).unwrap_or_else(|e| panic!("Decode mask {path}: {e:#}"));
            mask_pixels += mask.texels.as_raw().len() as u64;
            masks += 1;
        }
        if std::time::Instant::now() >= next_progress {
            eprintln!(
                "All native font/mask LZ77: {fonts}fonts/{glyphs}glyphs,{masks}masks,{:.1}s",
                started.elapsed().as_secs_f64()
            );
            next_progress = std::time::Instant::now() + std::time::Duration::from_secs(10);
        }
    }
    assert!(fonts > 0 && masks > 0 && glyphs > 0);
    let (streams, bytes) = finish_native_lz77_comparison().unwrap();
    assert!(streams > 0 && bytes > 0);
    println!(
        "Complete supplied-ROM font/mask native LZ77: {fonts}fonts,{glyphs}glyphs/{glyph_pixels}mip pixels,{masks}masks/{mask_pixels}visible pixels,{streams}compressed streams/{bytes}exact decoded bytes; {:.3}s",
        started.elapsed().as_secs_f64()
    );
}
