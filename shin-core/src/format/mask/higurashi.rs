//! Updated740e0 MSK3 v1:32-byte header,16-byte padded rows and optional
//! 12-bit LZ77 payload. MSK4 IDs/region metadata are absent in this format.
use anyhow::{Result, ensure};
use image::GrayImage;

pub fn read_higurashi_mask(source: &[u8]) -> Result<GrayImage> {
    ensure!(
        source.len() >= 32 && &source[..4] == b"MSK3",
        "Invalid Higurashi mask header"
    );
    let word = |at: usize| u32::from_le_bytes(source[at..at + 4].try_into().unwrap());
    ensure!(word(4) == 1, "Unsupported Higurashi mask version");
    ensure!(
        word(8) as usize == source.len(),
        "Higurashi mask file size mismatch"
    );
    let width = u16::from_le_bytes(source[12..14].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(source[14..16].try_into().unwrap()) as u32;
    ensure!(width != 0 && height != 0, "Empty Higurashi mask dimensions");
    let stride = (width as usize + 15) & !15;
    let expected = stride
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow::anyhow!("Higurashi mask size overflow"))?;
    let compressed = word(16) as usize;
    let data = &source[32..];
    let decoded;
    let pixels = if compressed == 0 {
        ensure!(data.len() == expected, "Higurashi raw mask size mismatch");
        data
    } else {
        ensure!(
            compressed == data.len(),
            "Higurashi compressed mask size mismatch"
        );
        // Validate token sizes/backreferences before using the shared native-
        // compatible decoder, which assumes a valid input stream.
        let mut pos = 0;
        let mut count = 0usize;
        while pos < data.len() {
            let map = data[pos];
            pos += 1;
            for bit in 0..8 {
                if pos == data.len() {
                    break;
                }
                if map & (1 << bit) == 0 {
                    pos += 1;
                    count += 1;
                } else {
                    ensure!(pos + 2 <= data.len(), "Truncated Higurashi mask reference");
                    let token = u16::from_be_bytes([data[pos], data[pos + 1]]);
                    pos += 2;
                    let distance = (token & 0xfff) as usize + 1;
                    ensure!(distance <= count, "Invalid Higurashi mask backreference");
                    count += (token >> 12) as usize + 3;
                }
                ensure!(
                    count <= expected,
                    "Higurashi mask expands beyond dimensions"
                );
            }
        }
        ensure!(count == expected, "Higurashi decoded mask size mismatch");
        let mut output = Vec::with_capacity(expected);
        super::super::lz77::decompress::<12>(data, &mut output);
        decoded = output;
        &decoded
    };
    let mut packed = Vec::with_capacity(width as usize * height as usize);
    for row in pixels.chunks_exact(stride) {
        packed.extend_from_slice(&row[..width as usize]);
    }
    GrayImage::from_raw(width, height, packed)
        .ok_or_else(|| anyhow::anyhow!("Invalid Higurashi mask image"))
}
