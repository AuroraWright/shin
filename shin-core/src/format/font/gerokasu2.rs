//! FNT6 v1 sparse Unicode fonts, verified against the supplied later engine.
//!
//! Native lookup: `5f2a0..5f5d0`; reconstruction: `5f600..5f9f8`.
//! Header bit 0 selects raw DEFLATE; other compressed glyphs use LZ77 with
//! ten offset bits. Both paths produce differential bytes before reconstruction.
//! FNT4 and its integer metrics remain separate.

use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use flate2::{Decompress, FlushDecompress, Status};
use image::GrayImage;

use super::GlyphId;
use crate::format::lz77;

const HEADER_SIZE: usize = 32;
const PAGE_SIZE: usize = 514;
const DESCRIPTOR_SIZE: usize = 24;
const FALLBACK_CODEPOINT: u32 = 0x203b;
pub mod texture;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gerokasu2FontHeader {
    pub version: u32,
    pub file_size: u32,
    pub flags: u32,
    pub page_count: u16,
    pub glyph_count: u16,
    /// Words at offsets 20..32. Their layout meanings are still under review.
    pub metric_words: [u16; 6],
}

impl Gerokasu2FontHeader {
    /// Exact owner fields initialized by the native constructor at `5f120`.
    /// This preserves the native u16 arithmetic without assigning guessed
    /// ascent/descent/leading names to the six source words.
    pub fn native_owner_metrics(&self) -> [u16; 7] {
        let [size, _, a, b, c, d] = self.metric_words;
        [
            a.wrapping_add(b).wrapping_add(c),
            a,
            b,
            c,
            d,
            c.wrapping_add(d),
            size,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gerokasu2GlyphInfo {
    pub bearing_x: i16,
    pub bearing_y: i16,
    pub width: u16,
    pub height: u16,
    /// Native `5f4b0` converts this unsigned value to pixels by dividing by 16.
    pub advance_sixteenths: u16,
    /// Descriptor word at offset 10; not consumed by the reviewed lookup.
    pub word_10: u16,
    /// Descriptor word at offset 12, copied unchanged into native metadata.
    pub identity: u32,
    pub mip_count: u16,
    pub decoded_size: u32,
}

impl Gerokasu2GlyphInfo {
    pub fn advance_width(&self) -> f32 {
        f32::from(self.advance_sixteenths) / 16.0
    }
}

struct CharacterPage {
    tag: u16,
    glyphs: [u16; 256],
}

struct Descriptor {
    info: Gerokasu2GlyphInfo,
    compressed_size: usize,
    data_offset: usize,
}

pub struct Gerokasu2Font {
    header: Gerokasu2FontHeader,
    pages: Vec<CharacterPage>,
    descriptors: Vec<Descriptor>,
    data: Bytes,
}

/// A lookup retains the owning font, so a descriptor cannot decode against
/// another font's payload. Missing valid codepoints use U+203B if available.
pub struct Gerokasu2GlyphLookup<'a> {
    font: &'a Gerokasu2Font,
    pub codepoint: u32,
    pub exact: bool,
    pub glyph_id: Option<GlyphId>,
    pub info: Gerokasu2GlyphInfo,
}

pub struct Gerokasu2Glyph {
    pub info: Gerokasu2GlyphInfo,
    /// Reconstructed native R8 bytes. Texture sampling/coverage semantics must
    /// be applied by the later renderer, rather than treating these as FNT4 alpha.
    pub mipmaps: Vec<GrayImage>,
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn decoded_size(width: u16, height: u16, count: u16) -> Result<u32> {
    // Native lookup sums the area shifted by two per level, rather than
    // separately multiplying the halved dimensions. Avoid oversized shifts.
    let mut area = u32::from(width) * u32::from(height);
    let mut size = 0u32;
    for _ in 0..count {
        size = size.checked_add(area).context("FNT6 glyph size overflow")?;
        area >>= 2;
    }
    Ok(size)
}

impl Gerokasu2Font {
    pub fn new(data: Bytes) -> Result<Self> {
        ensure!(data.len() >= HEADER_SIZE, "Truncated FNT6 header");
        ensure!(&data[..4] == b"FNT6", "Expected FNT6 font");
        let header = Gerokasu2FontHeader {
            version: u32_at(&data, 4),
            file_size: u32_at(&data, 8),
            flags: u32_at(&data, 12),
            page_count: u16_at(&data, 16),
            glyph_count: u16_at(&data, 18),
            metric_words: std::array::from_fn(|i| u16_at(&data, 20 + i * 2)),
        };
        ensure!(
            header.version == 1,
            "Unsupported FNT6 version {}",
            header.version
        );
        ensure!(
            header.file_size as usize == data.len(),
            "FNT6 file size mismatch"
        );
        let descriptors_offset = HEADER_SIZE + usize::from(header.page_count) * PAGE_SIZE;
        let table_end = descriptors_offset + usize::from(header.glyph_count) * DESCRIPTOR_SIZE;
        ensure!(
            table_end <= data.len(),
            "Truncated FNT6 character/descriptor tables"
        );
        let mut pages: Vec<CharacterPage> = Vec::with_capacity(header.page_count.into());
        for page in 0..usize::from(header.page_count) {
            let offset = HEADER_SIZE + page * PAGE_SIZE;
            let tag = u16_at(&data, offset);
            ensure!(tag <= 0x10ff, "FNT6 page lies outside Unicode range");
            if let Some(previous) = pages.last() {
                ensure!(previous.tag < tag, "FNT6 pages must be strictly sorted");
            }
            pages.push(CharacterPage {
                tag,
                glyphs: std::array::from_fn(|i| u16_at(&data, offset + 2 + i * 2)),
            });
        }
        let mut descriptors = Vec::with_capacity(header.glyph_count.into());
        for index in 0..usize::from(header.glyph_count) {
            let offset = descriptors_offset + index * DESCRIPTOR_SIZE;
            let packed = u32_at(&data, offset + 16);
            let width = u16_at(&data, offset + 4);
            let height = u16_at(&data, offset + 6);
            let mip_count = ((packed >> 24) + 1) as u16;
            let info = Gerokasu2GlyphInfo {
                bearing_x: u16_at(&data, offset) as i16,
                bearing_y: u16_at(&data, offset + 2) as i16,
                width,
                height,
                advance_sixteenths: u16_at(&data, offset + 8),
                word_10: u16_at(&data, offset + 10),
                identity: u32_at(&data, offset + 12),
                mip_count,
                decoded_size: decoded_size(width, height, mip_count)?,
            };
            let compressed_size = (packed & 0x00ff_ffff) as usize;
            let data_offset = u32_at(&data, offset + 20) as usize;
            let payload_size = if compressed_size == 0 {
                info.decoded_size as usize
            } else {
                compressed_size
            };
            ensure!(
                data_offset >= table_end,
                "FNT6 glyph {index} payload overlaps metadata"
            );
            ensure!(
                data.get(
                    data_offset
                        ..data_offset
                            .checked_add(payload_size)
                            .context("FNT6 payload size overflow")?
                )
                .is_some(),
                "FNT6 glyph {index} payload is outside the file"
            );
            descriptors.push(Descriptor {
                info,
                compressed_size,
                data_offset,
            });
        }
        Ok(Self {
            header,
            pages,
            descriptors,
            data,
        })
    }

    pub fn header(&self) -> &Gerokasu2FontHeader {
        &self.header
    }

    pub fn codepoints(&self) -> impl Iterator<Item = (u32, GlyphId)> + '_ {
        self.pages.iter().flat_map(move |page| {
            page.glyphs
                .iter()
                .enumerate()
                .filter_map(move |(low, &id)| {
                    (id < self.header.glyph_count).then_some((
                        (u32::from(page.tag) << 8) | low as u32,
                        GlyphId(u32::from(id)),
                    ))
                })
        })
    }

    fn exact_id(&self, codepoint: u32) -> Option<GlyphId> {
        if codepoint > 0x10ffff {
            return None;
        }
        let page = self
            .pages
            .binary_search_by_key(&((codepoint >> 8) as u16), |page| page.tag)
            .ok()?;
        let id = self.pages[page].glyphs[(codepoint & 255) as usize];
        (id < self.header.glyph_count).then_some(GlyphId(u32::from(id)))
    }

    pub fn lookup(&self, codepoint: u32) -> Gerokasu2GlyphLookup<'_> {
        let exact_id = self.exact_id(codepoint);
        // Native invalid codepoints return the 1x1 default directly, without
        // searching for the fallback. In-range unmapped indices try U+203B.
        let glyph_id = exact_id.or_else(|| {
            (codepoint <= 0x10ffff)
                .then(|| self.exact_id(FALLBACK_CODEPOINT))
                .flatten()
        });
        let info = glyph_id
            .map(|id| self.descriptors[id.0 as usize].info)
            .unwrap_or(Gerokasu2GlyphInfo {
                bearing_x: 0,
                bearing_y: 0,
                width: 1,
                height: 1,
                advance_sixteenths: 0,
                word_10: 0,
                identity: 0,
                mip_count: 1,
                decoded_size: 1,
            });
        Gerokasu2GlyphLookup {
            font: self,
            codepoint,
            exact: exact_id.is_some(),
            glyph_id,
            info,
        }
    }

    pub fn decode_glyph(&self, id: GlyphId) -> Result<Gerokasu2Glyph> {
        let descriptor = self
            .descriptors
            .get(id.0 as usize)
            .context("FNT6 glyph index outside descriptor table")?;
        let info = descriptor.info;
        // Native reconstruction assumes nonempty, compatible mip dimensions.
        // Odd sizes can make its area-based allocation exceed the image span;
        // reject those unauthored layouts until their caller contract is known.
        let mut dimensions = Vec::new();
        let (mut width, mut height) = (info.width, info.height);
        let mut image_size = 0usize;
        for _ in 0..info.mip_count {
            ensure!(width != 0 && height != 0, "FNT6 has an empty mip level");
            image_size += usize::from(width) * usize::from(height);
            dimensions.push((width, height));
            width >>= 1;
            height >>= 1;
        }
        ensure!(
            image_size == info.decoded_size as usize,
            "FNT6 mip image spans differ from native decoded size"
        );
        let offset = descriptor.data_offset;
        let mut pixels = if descriptor.compressed_size == 0 {
            self.data[offset..offset + image_size].to_vec()
        } else {
            let payload = &self.data[offset..offset + descriptor.compressed_size];
            if self.header.flags & 1 != 0 {
                inflate_exact(payload, image_size)?
            } else {
                lz77::decompress_checked::<10>(payload, image_size)
                    .context("Decoding FNT6 LZ glyph")?
            }
        };
        let mut offset = 0;
        let mut mipmaps = Vec::with_capacity(dimensions.len());
        for (width, height) in dimensions {
            let size = usize::from(width) * usize::from(height);
            let level = &mut pixels[offset..offset + size];
            reconstruct(level, usize::from(width));
            mipmaps.push(
                GrayImage::from_raw(u32::from(width), u32::from(height), level.to_vec()).unwrap(),
            );
            offset += size;
        }
        Ok(Gerokasu2Glyph { info, mipmaps })
    }
}

impl Gerokasu2GlyphLookup<'_> {
    pub fn decode(&self) -> Result<Gerokasu2Glyph> {
        match self.glyph_id {
            Some(id) => self.font.decode_glyph(id),
            // Native zero-fill then differential reconstruction subtracts 127
            // even for the 1x1 default. This unusual value is intentional.
            None => Ok(Gerokasu2Glyph {
                info: self.info,
                mipmaps: vec![GrayImage::from_raw(1, 1, vec![129]).unwrap()],
            }),
        }
    }
}

fn inflate_exact(payload: &[u8], size: usize) -> Result<Vec<u8>> {
    // One extra byte distinguishes a stream with excess output from a stream
    // that still needs to consume its end marker at the exact declared size.
    let capacity = size.checked_add(1).context("FNT6 DEFLATE size overflow")?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(capacity)
        .context("Allocating FNT6 pixels")?;
    pixels.resize(capacity, 0);
    let mut decoder = Decompress::new(false);
    let status = decoder
        .decompress(payload, &mut pixels, FlushDecompress::Finish)
        .context("Decoding raw FNT6 DEFLATE")?;
    if status != Status::StreamEnd
        || decoder.total_in() != payload.len() as u64
        || decoder.total_out() != size as u64
    {
        bail!("FNT6 DEFLATE stream or decoded size mismatch");
    }
    pixels.truncate(size);
    Ok(pixels)
}

fn reconstruct(pixels: &mut [u8], width: usize) {
    pixels[0] = pixels[0].wrapping_sub(127);
    for x in 1..width {
        pixels[x] = pixels[x].wrapping_add(pixels[x - 1]);
    }
    for i in width..pixels.len() {
        pixels[i] = pixels[i].wrapping_add(pixels[i - width]);
    }
}

#[cfg(test)]
mod tests;
