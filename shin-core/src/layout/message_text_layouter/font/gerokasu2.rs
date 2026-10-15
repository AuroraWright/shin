//! Later-engine line metrics and fractional advances at the shared layouter.
use std::sync::Arc;

use crate::format::font::gerokasu2::{Gerokasu2Font, Gerokasu2FontHeader};

use super::TextFontMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gerokasu2LineMetrics {
    pub ascent: i32,
    pub descent: i32,
}

impl Gerokasu2LineMetrics {
    pub fn from_header(header: &Gerokasu2FontHeader) -> Self {
        let owner = header.native_owner_metrics();
        // Native on-char 91e60/64, ruby 933d0/4, and finalizer 91408/0c.
        Self {
            ascent: i32::from(owner[1] as i16),
            descent: i32::from(owner[2] as i16),
        }
    }

    pub fn line_height(self) -> f32 {
        (self.ascent + self.descent) as f32
    }

    pub fn baseline(self, height: f32) -> f32 {
        // Native finalizer 91414..1c computes the ratio first, then multiplies
        // at 91670 (or 91584 for an empty line). Preserve that rounding order.
        (self.ascent as f32 / self.line_height()) * height
    }
}

impl TextFontMetrics for Gerokasu2Font {
    fn ascent(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(self.header()).ascent as f32
    }
    fn descent(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(self.header()).descent as f32
    }
    fn line_height(&self) -> f32 {
        Gerokasu2LineMetrics::from_header(self.header()).line_height()
    }
    fn glyph_advance(&self, codepoint: char) -> Option<f32> {
        Some(self.lookup(u32::from(codepoint)).info.advance_width())
    }
    fn baseline(&self, height: f32) -> f32 {
        Gerokasu2LineMetrics::from_header(self.header()).baseline(height)
    }
}

// Concrete forwarding avoids overlap with the legacy FontMetrics blanket
// implementation, including its Arc and reference forwarding implementations.
impl TextFontMetrics for Arc<Gerokasu2Font> {
    fn ascent(&self) -> f32 {
        self.as_ref().ascent()
    }
    fn descent(&self) -> f32 {
        self.as_ref().descent()
    }
    fn line_height(&self) -> f32 {
        self.as_ref().line_height()
    }
    fn glyph_advance(&self, codepoint: char) -> Option<f32> {
        self.as_ref().glyph_advance(codepoint)
    }
    fn baseline(&self, height: f32) -> f32 {
        self.as_ref().baseline(height)
    }
}

impl TextFontMetrics for &Gerokasu2Font {
    fn ascent(&self) -> f32 {
        (**self).ascent()
    }
    fn descent(&self) -> f32 {
        (**self).descent()
    }
    fn line_height(&self) -> f32 {
        (**self).line_height()
    }
    fn glyph_advance(&self, codepoint: char) -> Option<f32> {
        (**self).glyph_advance(codepoint)
    }
    fn baseline(&self, height: f32) -> f32 {
        (**self).baseline(height)
    }
}

#[cfg(test)]
mod tests;
