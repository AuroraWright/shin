use super::*;
use std::{
    io::{BufReader, BufWriter, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct DictionaryOracle {
    child: Option<Child>,
    input: Option<BufWriter<ChildStdin>>,
    output: BufReader<ChildStdout>,
    response: Vec<u8>,
    cases: usize,
    pixels: u64,
}

impl DictionaryOracle {
    fn new() -> Self {
        let executable = std::env::var_os("SHIN_REFERENCE_EXECUTABLE")
            .expect("SHIN_REFERENCE_EXECUTABLE must point to the supplied executable");
        let oracle = std::env::var_os("SHIN_REFERENCE_DICTIONARY_ORACLE")
            .expect("SHIN_REFERENCE_DICTIONARY_ORACLE must point to the compiled C oracle");
        let mut child = Command::new(oracle)
            .arg(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Launch guarded native dictionary oracle");
        Self {
            input: Some(BufWriter::new(child.stdin.take().unwrap())),
            output: BufReader::new(child.stdout.take().unwrap()),
            child: Some(child),
            response: Vec::new(),
            cases: 0,
            pixels: 0,
        }
    }

    fn compare(
        &mut self,
        image: &RgbaImage,
        palette: &[Rgba8; 256],
        indices: &[u8],
        alpha: Option<&[u8]>,
        stride: usize,
        output_padding: u32,
    ) {
        assert_eq!(indices.len(), stride * image.height() as usize);
        if let Some(alpha) = alpha {
            assert_eq!(alpha.len(), indices.len());
        }
        let input = self.input.as_mut().unwrap();
        input.write_all(b"NDI1").unwrap();
        for field in [
            image.width(),
            image.height(),
            stride as u32,
            image.width() * 4 + output_padding,
            u32::from(alpha.is_some()),
        ] {
            input.write_all(&field.to_le_bytes()).unwrap();
        }
        input.write_all(bytemuck::bytes_of(palette)).unwrap();
        input.write_all(indices).unwrap();
        if let Some(alpha) = alpha {
            input.write_all(alpha).unwrap();
        }
        input.flush().unwrap();
        let mut size = [0; 4];
        self.output
            .read_exact(&mut size)
            .expect("Native dictionary response header");
        assert_eq!(u32::from_le_bytes(size) as usize, image.as_raw().len());
        self.response.resize(image.as_raw().len(), 0);
        self.output
            .read_exact(&mut self.response)
            .expect("Native dictionary RGBA output");
        if let Some(byte) = image
            .as_raw()
            .iter()
            .zip(&self.response)
            .position(|(a, b)| a != b)
        {
            let pixel = byte / 4;
            panic!(
                "Native dictionary mismatch: width={},height={},stride={stride},separate_alpha={},pixel={pixel},rust={:?},native={:?}",
                image.width(),
                image.height(),
                alpha.is_some(),
                &image.as_raw()[pixel * 4..pixel * 4 + 4],
                &self.response[pixel * 4..pixel * 4 + 4],
            );
        }
        self.cases += 1;
        self.pixels += u64::from(image.width()) * u64::from(image.height());
    }

    fn finish(mut self) -> (usize, u64) {
        drop(self.input.take());
        let status = self.child.take().unwrap().wait().unwrap();
        assert!(
            status.success(),
            "Native dictionary process exited {status}"
        );
        (self.cases, self.pixels)
    }
}

impl Drop for DictionaryOracle {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// An explicit opt-in lets the existing complete ROM audit compare every
// decoded block at the public decoder boundary. Parallel block workers share
// one persistent native process; serialization keeps requests paired.
fn supplied_dictionary_oracle() -> &'static Option<std::sync::Mutex<Option<DictionaryOracle>>> {
    static ORACLE: std::sync::OnceLock<Option<std::sync::Mutex<Option<DictionaryOracle>>>> =
        std::sync::OnceLock::new();
    ORACLE.get_or_init(|| {
        (std::env::var("SHIN_REFERENCE_COMPARE_NATIVE_DICTIONARY").as_deref() == Ok("1"))
            .then(|| std::sync::Mutex::new(Some(DictionaryOracle::new())))
    })
}

pub(super) fn compare_native_dictionary_if_enabled(
    image: &RgbaImage,
    palette: &[Rgba8; 256],
    indices: &[u8],
    alpha: Option<&[u8]>,
    stride: usize,
) {
    if let Some(oracle) = supplied_dictionary_oracle() {
        oracle
            .lock()
            .unwrap()
            .as_mut()
            .expect("Native audit already finished")
            .compare(image, palette, indices, alpha, stride, 19);
    }
}

pub(crate) fn finish_native_dictionary_comparison() -> Option<(usize, u64)> {
    supplied_dictionary_oracle().as_ref().map(|oracle| {
        oracle
            .lock()
            .unwrap()
            .take()
            .expect("Native audit already finished")
            .finish()
    })
}

#[test]
#[ignore = "requires supplied executable and compiled native dictionary oracle; no GPU/audio"]
fn reference_native_dictionary_image_decoder() {
    let mut oracle = DictionaryOracle::new();
    let palette: [Rgba8; 256] = std::array::from_fn(|index| Rgba8 {
        r: index as u8,
        g: (index * 71) as u8,
        b: (index * 139 + 23) as u8,
        a: (index * 97) as u8,
    });
    for width in [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 63, 64, 65, 255, 256, 257,
    ] {
        for height in [0, 1, 2, 5] {
            for padding in [0, 3, 16] {
                let stride = ((width as usize + 3) & !3) + padding;
                let mut indices = vec![0xde; stride * height as usize];
                let mut alphas = vec![0xad; indices.len()];
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        indices[y * stride + x] = (x + y * width as usize) as u8;
                        alphas[y * stride + x] = (255usize.wrapping_sub(x * 37 + y * 103)) as u8;
                    }
                }
                for alpha in [None, Some(alphas.as_slice())] {
                    let mut image = RgbaImage::new(width, height);
                    decode_dict(
                        &mut image,
                        &palette,
                        &indices,
                        alpha,
                        width as usize,
                        stride,
                    );
                    for output_padding in [0, 1, 19] {
                        oracle.compare(&image, &palette, &indices, alpha, stride, output_padding);
                    }
                }
            }
        }
    }
    let (cases, pixels) = oracle.finish();
    eprintln!(
        "Native dictionary decoder: {cases} cases, {pixels} exact RGBA pixels; inline/separate alpha, all256indices, empty dimensions and input/output guards checked"
    );
}

#[test]
fn dictionary_empty_dimensions_return_without_reading_input() {
    let palette = [Rgba8::default(); 256];
    for (width, height) in [(0, 0), (0, 3), (5, 0)] {
        for alpha in [None, Some([].as_slice())] {
            let mut image = RgbaImage::new(width, height);
            decode_dict(&mut image, &palette, &[], alpha, width as usize, 0);
            assert!(image.as_raw().is_empty());
        }
    }
}

#[test]
fn differential_rows_wrap_each_channel_and_ignore_padding() {
    // Three one-pixel rows. Deliberately different padding distinguishes
    // vertical channel deltas from a horizontal or whole-buffer accumulator.
    let source = [
        250, 10, 128, 255, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 10, 255, 128, 1, 201,
        202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 255, 2, 1, 255, 31, 32, 33, 34, 35,
        36, 37, 38, 39, 40, 41, 42,
    ];
    let mut image = RgbaImage::new(1, 3);
    read_texture(&source, 0, &mut image, false, false);
    assert_eq!(
        image.as_raw(),
        &[250, 10, 128, 255, 4, 9, 0, 0, 3, 11, 1, 255]
    );

    // The flag for a separate dictionary alpha stream is irrelevant to the
    // differential path: alpha deltas are interleaved with RGB.
    let mut inline = RgbaImage::new(1, 3);
    read_texture(&source, 0, &mut inline, false, true);
    assert_eq!(inline, image);

    // Exercise the public LZ77 -> differential pipeline, not only the helper.
    let compressed: Vec<_> = source
        .chunks(8)
        .flat_map(|chunk| std::iter::once(0).chain(chunk.iter().copied()))
        .collect();
    let mut decompressed = RgbaImage::new(1, 3);
    read_texture(
        &compressed,
        compressed.len(),
        &mut decompressed,
        false,
        true,
    );
    assert_eq!(decompressed, image);
}

#[test]
fn differential_vector_boundaries_and_single_row() {
    for width in [1, 2, 3, 4, 5, 15, 16, 17, 63, 64, 65] {
        let row_bytes = width as usize * 4;
        let stride = (row_bytes + 15) & !15;
        let mut source = vec![0xde; stride * 5];
        let mut expected = vec![0; row_bytes * 5];
        for y in 0..5 {
            for byte in 0..row_bytes {
                // Define target pixels first and encode their differences.
                // Both operands vary by row/channel/pixel, with many wraps.
                let value = (byte * 71 + y * 139 + y * byte * 3) as u8;
                expected[y * row_bytes + byte] = value;
                let previous = if y == 0 {
                    0
                } else {
                    expected[(y - 1) * row_bytes + byte]
                };
                source[y * stride + byte] = value.wrapping_sub(previous);
            }
        }
        let mut image = RgbaImage::new(width, 5);
        read_texture(&source, 0, &mut image, false, true);
        assert_eq!(image.as_raw(), &expected, "width={width}");
        let mut first_row = RgbaImage::new(width, 1);
        read_texture(&source[..stride], 0, &mut first_row, false, true);
        assert_eq!(first_row.as_raw(), &expected[..row_bytes]);
    }
}

#[test]
fn differential_empty_dimensions() {
    for (width, height) in [(0, 0), (0, 3), (5, 0)] {
        let mut image = RgbaImage::new(width, height);
        read_texture(&[], 0, &mut image, false, true);
        assert!(image.as_raw().is_empty());
    }
}

#[test]
#[ignore = "requires supplied executable and compiled native differential oracle; no GPU or audio"]
fn reference_native_differential_image_decoder() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let executable = std::env::var_os("SHIN_REFERENCE_EXECUTABLE")
        .expect("SHIN_REFERENCE_EXECUTABLE must point to the supplied executable");
    let oracle = std::env::var_os("SHIN_REFERENCE_DIFFERENTIAL_ORACLE")
        .expect("SHIN_REFERENCE_DIFFERENTIAL_ORACLE must point to the compiled C oracle");
    let mut state = 0x8315_20cb_d394_fe67u64;
    let mut cases = 0;
    let mut pixels = 0;
    for width in [1, 2, 3, 4, 5, 15, 16, 17, 63, 64, 65] {
        for height in [1, 2, 3, 4, 7, 8] {
            for source_padding in [0, 16] {
                let stride = ((width as usize * 4 + 15) & !15) + source_padding;
                let source: Vec<u8> = (0..stride * height as usize)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        state as u8
                    })
                    .collect();
                let mut image = RgbaImage::new(width, height);
                if source_padding == 0 {
                    read_texture(&source, 0, &mut image, false, true);
                } else {
                    decode_differential(&mut image, &source, stride);
                }
                for output_padding in [0, 4, 19] {
                    let mut child = Command::new(&oracle)
                        .arg(&executable)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .expect("Launch guarded native differential oracle");
                    let mut input = child.stdin.take().unwrap();
                    input.write_all(b"NDF1").unwrap();
                    for field in [width, height, stride as u32, width * 4 + output_padding] {
                        input.write_all(&field.to_le_bytes()).unwrap();
                    }
                    input.write_all(&source).unwrap();
                    drop(input);
                    let native = child.wait_with_output().unwrap();
                    assert!(
                        native.status.success(),
                        "Native failed: {}",
                        String::from_utf8_lossy(&native.stderr)
                    );
                    assert_eq!(
                        image.as_raw(),
                        &native.stdout,
                        "width={width},height={height},source_stride={stride},output_padding={output_padding}"
                    );
                    cases += 1;
                    pixels += width as usize * height as usize;
                }
            }
        }
    }
    eprintln!(
        "Native differential image decoder: {cases} cases, {pixels} exact RGBA pixel comparisons; source/destination/scratch guards checked"
    );
}
