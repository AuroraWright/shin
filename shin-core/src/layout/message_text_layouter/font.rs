use std::sync::Arc;

use crate::format::font::{Font, GlyphInfo, GlyphTrait};

pub mod gerokasu2;

pub trait FontMetrics {
    fn get_ascent(&self) -> u32;

    fn get_descent(&self) -> u32;

    fn get_glyph_info(&self, codepoint: char) -> Option<GlyphInfo>;
}

/// Metrics consumed by layout, independent of a font file's glyph record.
/// FNT6 advances are fractional and its line metrics use signed native loads.
pub trait TextFontMetrics {
    fn ascent(&self) -> f32;
    fn descent(&self) -> f32;
    fn line_height(&self) -> f32;
    fn glyph_advance(&self, codepoint: char) -> Option<f32>;

    /// Keep the legacy operation order unless a game's consumer differs.
    fn baseline(&self, height: f32) -> f32 {
        height / (self.ascent() + self.descent()) * self.ascent()
    }
}

/// Existing engine fonts and callers retain their integer API and arithmetic.
impl<T: FontMetrics + ?Sized> TextFontMetrics for T {
    fn ascent(&self) -> f32 {
        self.get_ascent() as f32
    }
    fn descent(&self) -> f32 {
        self.get_descent() as f32
    }
    fn line_height(&self) -> f32 {
        (self.get_ascent() + self.get_descent()) as f32
    }
    fn glyph_advance(&self, codepoint: char) -> Option<f32> {
        self.get_glyph_info(codepoint)
            .map(|info| f32::from(info.advance_width))
    }
}

impl<T: FontMetrics> FontMetrics for Arc<T> {
    fn get_ascent(&self) -> u32 {
        (**self).get_ascent()
    }

    fn get_descent(&self) -> u32 {
        (**self).get_descent()
    }

    fn get_glyph_info(&self, codepoint: char) -> Option<GlyphInfo> {
        (**self).get_glyph_info(codepoint)
    }
}

impl<T: FontMetrics> FontMetrics for &T {
    fn get_ascent(&self) -> u32 {
        (**self).get_ascent()
    }

    fn get_descent(&self) -> u32 {
        (**self).get_descent()
    }

    fn get_glyph_info(&self, codepoint: char) -> Option<GlyphInfo> {
        (**self).get_glyph_info(codepoint)
    }
}

impl<G: GlyphTrait> FontMetrics for Font<G> {
    fn get_ascent(&self) -> u32 {
        Font::get_ascent(self) as u32
    }

    fn get_descent(&self) -> u32 {
        Font::get_descent(self) as u32
    }

    fn get_glyph_info(&self, codepoint: char) -> Option<GlyphInfo> {
        Font::try_get_glyph_for_character(self, codepoint.try_into().unwrap()).map(|v| v.get_info())
    }
}
