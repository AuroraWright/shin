//! Gerokasu (the first game) ships Umineko-generation FNT4 coverage fonts,
//! while the shared later-engine text path draws FNT6 signed-distance glyphs.
//! This adapter exposes an FNT4 file through the FNT6 lookup/decode contract:
//! integer FNT4 metrics become the FNT6 owner words, and each coverage bitmap
//! is converted to a signed distance field with the same SNORM convention
//! (positive inside, `byte / 127 * SPREAD` pixels).
use anyhow::{Context, Result};
use image::GrayImage;

use crate::format::font::{FontLazy, GlyphMipLevel, GlyphTrait, read_lazy_font};

/// SDF normalization in level-0 pixels; becomes FNT6 owner word 6.
pub(super) const SPREAD: u16 = 16;
/// Spread margin kept inside each converted glyph box, so outlines (up to
/// about 5.5 px outside the strokes) are not cut by the native 0x81 padding.
const MARGIN: i32 = 8;
/// Four mip levels, matching the FNT4 source mip chain.
pub(super) const MIP_COUNT: u16 = 4;

pub(super) struct Fnt4Source {
    font: FontLazy,
}

pub(super) struct Fnt4Glyph {
    pub bearing_x: i16,
    pub bearing_y: i16,
    pub width: u16,
    pub height: u16,
    pub advance: u8,
}

fn round_up8(value: i32) -> u16 {
    (((value.max(8)) + 7) / 8 * 8) as u16
}

impl Fnt4Source {
    pub fn new(data: &[u8]) -> Result<Self> {
        let font =
            read_lazy_font(&mut std::io::Cursor::new(data)).context("Reading FNT4 font")?;
        Ok(Self { font })
    }

    pub fn ascent(&self) -> u16 {
        self.font.get_ascent()
    }

    pub fn descent(&self) -> u16 {
        self.font.get_descent()
    }

    pub fn glyph_count(&self) -> usize {
        self.font.get_glyphs().len()
    }

    /// FNT4 maps every BMP code unit; ids are dense from zero.
    pub fn mapping(&self) -> &[crate::format::font::GlyphId; 0x10000] {
        self.font.get_character_mapping()
    }

    pub fn glyph(&self, id: u32) -> Option<Fnt4Glyph> {
        let info = self
            .font
            .get_glyph(crate::format::font::GlyphId(id))?
            .get_info();
        Some(Fnt4Glyph {
            bearing_x: i16::from(info.bearing_x) - MARGIN as i16,
            bearing_y: i16::from(info.bearing_y) + MARGIN as i16,
            width: round_up8(i32::from(info.actual_width) + 2 * MARGIN),
            height: round_up8(i32::from(info.actual_height) + 2 * MARGIN),
            advance: info.advance_width,
        })
    }

    /// Signed-distance mip chain for one glyph, as reconstructed R8 bytes.
    pub fn decode(&self, id: u32) -> Result<Vec<GrayImage>> {
        let lazy = self
            .font
            .get_glyph(crate::format::font::GlyphId(id))
            .context("FNT4 glyph index outside table")?;
        let glyph = lazy.decompress();
        let info = lazy.get_info();
        let source = glyph.get_image(GlyphMipLevel::Level0);
        let width = usize::from(round_up8(i32::from(info.actual_width) + 2 * MARGIN));
        let height = usize::from(round_up8(i32::from(info.actual_height) + 2 * MARGIN));
        let coverage = |x: i32, y: i32| -> f32 {
            let (sx, sy) = (x - MARGIN, y - MARGIN);
            if sx < 0
                || sy < 0
                || sx >= i32::from(info.actual_width)
                || sy >= i32::from(info.actual_height)
                || sx as u32 >= source.width()
                || sy as u32 >= source.height()
            {
                0.0
            } else {
                f32::from(source.get_pixel(sx as u32, sy as u32).0[0]) / 255.0
            }
        };
        let level0 = signed_distance(width, height, coverage);
        let mut mips = Vec::with_capacity(usize::from(MIP_COUNT));
        let mut current = level0;
        let (mut w, mut h) = (width, height);
        for level in 0..MIP_COUNT {
            mips.push(
                GrayImage::from_raw(
                    w as u32,
                    h as u32,
                    current.iter().map(|&d| encode(d)).collect(),
                )
                .unwrap(),
            );
            if level + 1 == MIP_COUNT {
                break;
            }
            let (nw, nh) = (w / 2, h / 2);
            let mut next = vec![0.0; nw * nh];
            for y in 0..nh {
                for x in 0..nw {
                    next[y * nw + x] = (current[2 * y * w + 2 * x]
                        + current[2 * y * w + 2 * x + 1]
                        + current[(2 * y + 1) * w + 2 * x]
                        + current[(2 * y + 1) * w + 2 * x + 1])
                        / 4.0;
                }
            }
            current = next;
            (w, h) = (nw, nh);
        }
        Ok(mips)
    }
}

fn encode(distance: f32) -> u8 {
    ((distance / f32::from(SPREAD)).clamp(-1.0, 1.0) * 127.0).round() as i8 as u8
}

/// Coverage edges: partially covered pixels, plus both sides of hard edges.
/// A pixel's own edge offset is `a - 0.5`; other pixels add their distance.
fn signed_distance(width: usize, height: usize, coverage: impl Fn(i32, i32) -> f32) -> Vec<f32> {
    const RADIUS: i32 = 10;
    let a: Vec<f32> = (0..height as i32)
        .flat_map(|y| (0..width as i32).map(move |x| (x, y)))
        .map(|(x, y)| coverage(x, y))
        .collect();
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            0.0
        } else {
            a[y as usize * width + x as usize]
        }
    };
    let far = f32::from(SPREAD) * 2.0;
    let mut inside_distance = vec![far; width * height];
    let mut outside_distance = vec![far; width * height];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let value = at(x, y);
            let inside = value >= 0.5;
            let partial = value > 0.0 && value < 1.0;
            let hard = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| (at(x + dx, y + dy) >= 0.5) != inside);
            if !partial && !hard {
                continue;
            }
            let offset = value - 0.5;
            for dy in -RADIUS..=RADIUS {
                for dx in -RADIUS..=RADIUS {
                    let (px, py) = (x + dx, y + dy);
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                        continue;
                    }
                    let r = ((dx * dx + dy * dy) as f32).sqrt();
                    let index = py as usize * width + px as usize;
                    inside_distance[index] = inside_distance[index].min(r + offset);
                    outside_distance[index] = outside_distance[index].min(r - offset);
                }
            }
        }
    }
    (0..width * height)
        .map(|index| {
            if a[index] >= 0.5 {
                inside_distance[index].max(0.0)
            } else {
                -outside_distance[index].max(0.0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_distance_is_positive_inside_and_negative_outside() {
        // A 4x4 solid square at (8..12) inside a 24x24 box.
        let field = signed_distance(24, 24, |x, y| {
            if (8..12).contains(&x) && (8..12).contains(&y) {
                1.0
            } else {
                0.0
            }
        });
        let d = |x: usize, y: usize| field[y * 24 + x];
        assert!(d(9, 9) > 0.0 && d(10, 10) > 0.0);
        assert!(d(4, 4) < -3.0);
        // Hard edge: the inside pixel next to the edge sits half a pixel in.
        assert!((d(8, 10) - 0.5).abs() < 1e-4);
        assert!((d(7, 10) + 0.5).abs() < 1e-4);
        assert_eq!(encode(0.0), 0);
        assert_eq!(encode(100.0), 127);
        assert_eq!(encode(-100.0) as i8, -127);
    }
}
