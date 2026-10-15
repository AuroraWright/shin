//! FNT6 glyph geometry and signed R8 texture preparation.
//! Native cache-miss setup:5fb6c..5fc28; native format2 is R8 SNORM.
use anyhow::{Result, ensure};
use image::{GrayImage, Luma};

use super::{Gerokasu2FontHeader, Gerokasu2Glyph, Gerokasu2GlyphInfo};

/// Fields supplied to drawable glyphs, preserving wider metrics and advances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphTextureGeometry {
    pub codepoint: u32,
    pub identity: u32,
    pub origin: [i32; 2],
    pub dimensions: [u32; 2],
    pub advance: f32,
    /// Native signed loads of owner words1..3; consumer names remain scoped.
    pub font_metrics: [i32; 3],
    pub scale: [f32; 2],
    /// Native unsigned owner word6 converted to float at5fc1c..5fc24.
    pub extra_metric: f32,
    pub border: u32,
}

impl Gerokasu2GlyphInfo {
    pub fn texture_geometry(
        &self,
        codepoint: u32,
        header: &Gerokasu2FontHeader,
    ) -> GlyphTextureGeometry {
        // Variable AArch64 shifts mask their amount to five bits. Geometry
        // retains the original words; texture preparation checks usable sizes.
        let border = 1u32.wrapping_shl(u32::from(self.mip_count).wrapping_sub(1));
        let margin = border.wrapping_mul(2);
        let owner = header.native_owner_metrics();
        GlyphTextureGeometry {
            codepoint,
            identity: self.identity,
            origin: [
                i32::from(self.bearing_x).wrapping_sub(border as i32),
                (-i32::from(self.bearing_y)).wrapping_sub(border as i32),
            ],
            dimensions: [
                u32::from(self.width).wrapping_add(margin),
                u32::from(self.height).wrapping_add(margin),
            ],
            advance: self.advance_width(),
            font_metrics: [
                i32::from(owner[1] as i16),
                i32::from(owner[2] as i16),
                i32::from(owner[3] as i16),
            ],
            scale: [1.0, 1.0],
            extra_metric: f32::from(owner[6]),
            border,
        }
    }
}

/// Byte-preserving R8 SNORM mipmaps. 0x81 is the native negative padding value.
/// Clamping or rebiasing bytes before interpolation changes edge sampling.
pub struct PreparedGlyphTexture {
    pub geometry: GlyphTextureGeometry,
    pub mipmaps: Vec<GrayImage>,
}

impl Gerokasu2Glyph {
    pub fn prepare_texture(
        &self,
        codepoint: u32,
        header: &Gerokasu2FontHeader,
    ) -> Result<PreparedGlyphTexture> {
        ensure!(
            self.info.mip_count > 0 && self.info.mip_count < 32,
            "Unsupported FNT6 texture mip count"
        );
        ensure!(
            self.mipmaps.len() == usize::from(self.info.mip_count),
            "FNT6 texture mip count mismatch"
        );
        let geometry = self.info.texture_geometry(codepoint, header);
        let width = u32::from(self.info.width)
            .checked_add(
                geometry
                    .border
                    .checked_mul(2)
                    .ok_or_else(|| anyhow::anyhow!("FNT6 border overflow"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("FNT6 texture width overflow"))?;
        let height = u32::from(self.info.height)
            .checked_add(
                geometry
                    .border
                    .checked_mul(2)
                    .ok_or_else(|| anyhow::anyhow!("FNT6 border overflow"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("FNT6 texture height overflow"))?;
        let mut mipmaps = Vec::with_capacity(self.mipmaps.len());
        for (level, source) in self.mipmaps.iter().enumerate() {
            let source_size = (
                u32::from(self.info.width) >> level,
                u32::from(self.info.height) >> level,
            );
            ensure!(
                source_size.0 > 0 && source_size.1 > 0 && source.dimensions() == source_size,
                "FNT6 texture source mip mismatch"
            );
            let border = geometry.border >> level;
            let dimensions = (width >> level, height >> level);
            ensure!(
                source_size.0.checked_add(border * 2) == Some(dimensions.0)
                    && source_size.1.checked_add(border * 2) == Some(dimensions.1),
                "FNT6 padded mip dimensions mismatch"
            );
            let mut padded = GrayImage::from_pixel(dimensions.0, dimensions.1, Luma([0x81]));
            image::imageops::replace(&mut padded, source, i64::from(border), i64::from(border));
            mipmaps.push(padded);
        }
        Ok(PreparedGlyphTexture { geometry, mipmaps })
    }
}

#[cfg(test)]
mod tests;
