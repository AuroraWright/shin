//! Signed FNT6 glyph upload and wider drawable geometry, separate from FNT4.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use shin_core::{
    format::font::gerokasu2::{
        Gerokasu2Font,
        texture::{GlyphTextureGeometry, PreparedGlyphTexture},
    },
    layout::font::TextFontMetrics,
};
use shin_render::{gpu_texture::GpuTexture, shaders::types::texture::TextureSource};

use crate::asset::system::cache::CacheHandle;
use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

pub(crate) mod cache;
pub mod draw;

pub struct GpuGerokasu2Font {
    font: Arc<Gerokasu2Font>,
    glyph_cache: Arc<cache::Gerokasu2GlyphCache<GpuGerokasu2Glyph>>,
    text_overrides: Arc<crate::asset::localization::TextOverrides>,
}

impl GpuGerokasu2Font {
    #[cfg(test)]
    pub fn new(font: Gerokasu2Font) -> Result<Self> {
        Self::with_cache(font, Arc::new(cache::Gerokasu2GlyphCache::default()))
    }

    /// Code-defined UI text through the asset server's `localized.toml`.
    /// Scenario text (story, choices, titles, history) stays authored.
    pub fn localize<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        self.text_overrides.translate(text)
    }

    /// Shared layout owns these CPU metrics independently of drawable handles.
    pub fn metrics(&self) -> Arc<Gerokasu2Font> {
        self.font.clone()
    }

    fn with_cache(
        font: Gerokasu2Font,
        glyph_cache: Arc<cache::Gerokasu2GlyphCache<GpuGerokasu2Glyph>>,
    ) -> Result<Self> {
        ensure!(
            font.line_height() > 0.0,
            "FNT6 font line height must be positive"
        );
        Ok(Self {
            font: Arc::new(font),
            glyph_cache,
            text_overrides: Arc::default(),
        })
    }

    pub fn load_glyphs(
        self: Arc<Self>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        characters: &[char],
    ) -> Vec<GpuGerokasu2GlyphHandle> {
        if characters.is_empty() {
            return Vec::new();
        }
        let batch = cache::request_glyphs(&self.font, &self.glyph_cache, characters);
        let handles = characters
            .iter()
            .copied()
            .zip(batch.handles)
            .map(|(_codepoint, handle)| GpuGerokasu2GlyphHandle {
                #[cfg(test)]
                codepoint: _codepoint,
                handle,
            })
            .collect();
        if !batch.requests.is_empty() {
            shin_tasks::compute::spawn_and_forget(move || {
                batch.requests.into_par_iter().for_each(|request| {
                    let codepoint = request.codepoint;
                    self.glyph_cache.complete_load(request, || {
                        let prepared = self
                            .font
                            .lookup(u32::from(codepoint))
                            .decode()?
                            .prepare_texture(u32::from(codepoint), self.font.header())?;
                        GpuGerokasu2Glyph::upload(&device, &queue, prepared)
                    });
                });
            });
        }
        handles
    }
}

impl TextFontMetrics for GpuGerokasu2Font {
    fn ascent(&self) -> f32 {
        self.font.ascent()
    }
    fn descent(&self) -> f32 {
        self.font.descent()
    }
    fn line_height(&self) -> f32 {
        self.font.line_height()
    }
    fn glyph_advance(&self, codepoint: char) -> Option<f32> {
        self.font.glyph_advance(codepoint)
    }
    fn baseline(&self, height: f32) -> f32 {
        self.font.baseline(height)
    }
}

impl Asset for GpuGerokasu2Font {
    fn attach_text_overrides(&mut self, overrides: Arc<crate::asset::localization::TextOverrides>) {
        self.text_overrides = overrides;
    }

    type Args = ();
    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        Self::with_cache(
            read_layout_font(data.read_all().await?)?,
            context.font_cache.gerokasu2.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct GpuGerokasu2GlyphHandle {
    /// Tests check that handles keep the requested character order.
    #[cfg(test)]
    pub codepoint: char,
    handle: CacheHandle<GpuGerokasu2Glyph>,
}
impl GpuGerokasu2GlyphHandle {
    pub fn geometry(&self) -> Result<&GlyphTextureGeometry> {
        Ok(&self.handle.try_wait_ref()?.geometry)
    }
    pub fn as_texture_source(&self) -> Result<TextureSource<'_>> {
        Ok(self.handle.try_wait_ref()?.as_texture_source())
    }
}

fn read_layout_font(bytes: Vec<u8>) -> Result<Gerokasu2Font> {
    let font = Gerokasu2Font::new(bytes.into()).context("Reading FNT6 font")?;
    // Native consumers load signed metrics. Reject unusable live line heights
    // before shared layout can enter a soft-wrap loop with infinite widths.
    ensure!(
        font.line_height() > 0.0,
        "FNT6 font line height must be positive"
    );
    Ok(font)
}

impl Asset for Gerokasu2Font {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        read_layout_font(data.read_all().await?)
    }
}

#[derive(Debug)]
pub struct GpuGerokasu2Glyph {
    texture: GpuTexture,
    pub geometry: GlyphTextureGeometry,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(ascent: u16, descent: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 32];
        bytes[..4].copy_from_slice(b"FNT6");
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&32u32.to_le_bytes());
        bytes[24..26].copy_from_slice(&ascent.to_le_bytes());
        bytes[26..28].copy_from_slice(&descent.to_le_bytes());
        bytes
    }

    #[test]
    fn gerokasu2_asset_rejects_zero_and_negative_signed_line_heights() {
        for (ascent, descent) in [(0, 0), (32768, 32767), (65535, 0)] {
            let parsed = Gerokasu2Font::new(font(ascent, descent).into()).unwrap();
            assert!(GpuGerokasu2Font::new(parsed).is_err());
            let error = read_layout_font(font(ascent, descent))
                .err()
                .expect("invalid live height");
            assert!(error.to_string().contains("line height must be positive"));
        }
    }

    #[test]
    fn gerokasu2_asset_retains_native_metrics_and_requires_fnt6() {
        let loaded = read_layout_font(font(70, 10)).unwrap();
        assert_eq!(
            (loaded.ascent(), loaded.descent(), loaded.line_height()),
            (70.0, 10.0, 80.0)
        );
        assert_eq!(loaded.glyph_advance('\u{10ffff}'), Some(0.0));
        assert_eq!(read_layout_font(font(65535, 2)).unwrap().line_height(), 1.0);
        let mut wrong_dialect = font(70, 10);
        wrong_dialect[..4].copy_from_slice(b"FNT4");
        assert!(read_layout_font(wrong_dialect).is_err());
    }

    #[test]
    fn gerokasu2_font_localizes_code_text_through_attached_overrides() {
        let mut font = GpuGerokasu2Font::new(Gerokasu2Font::new(font(70, 10).into()).unwrap()).unwrap();
        assert_eq!(font.localize("はい"), "はい", "no localized.toml keeps the source");
        font.attach_text_overrides(Arc::new(
            crate::asset::localization::TextOverrides::parse(
                "\"はい\" = \"Yes\"\n\"{m}分\" = \"{m} min\"\n\"{h}時間 {m}分\" = \"{h} h {m} min\"\n",
            )
            .unwrap(),
        ));
        assert_eq!(font.localize("はい"), "Yes");
        assert_eq!(font.localize("5分"), "5 min");
        assert_eq!(font.localize("12時間 5分"), "12 h 5 min");
        assert_eq!(font.localize("はいはい"), "はいはい", "no substring matches");
    }
}

impl GpuGerokasu2Glyph {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: PreparedGlyphTexture,
    ) -> Result<Self> {
        ensure!(
            !prepared.mipmaps.is_empty(),
            "FNT6 glyph has no texture mipmaps"
        );
        ensure!(
            prepared.mipmaps[0].dimensions()
                == (
                    prepared.geometry.dimensions[0],
                    prepared.geometry.dimensions[1]
                ),
            "FNT6 glyph texture dimensions mismatch"
        );
        let limit = device.limits().max_texture_dimension_2d;
        ensure!(
            prepared
                .geometry
                .dimensions
                .into_iter()
                .all(|n| n > 0 && n <= limit),
            "FNT6 glyph exceeds device texture limits"
        );
        let levels = prepared.mipmaps.iter().collect::<Vec<_>>();
        let texture = GpuTexture::new_static_from_signed_gray_mipped_image(
            device,
            queue,
            Some(&format!("FNT6Glyph({:08x})", prepared.geometry.identity)),
            &levels,
        );
        Ok(Self {
            texture,
            geometry: prepared.geometry,
        })
    }
    pub fn as_texture_source(&self) -> TextureSource<'_> {
        self.texture.as_source()
    }
}
