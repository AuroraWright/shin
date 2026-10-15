//! FNT6 padded glyph quads and native signed-distance fill/outline thresholds.
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, vec2, vec4};
use shin_core::format::font::gerokasu2::texture::GlyphTextureGeometry;
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::TextVertex},
};

use super::GpuGerokasu2GlyphHandle;

pub struct GlyphDraw {
    pub vertices: [TextVertex; 4],
    pub fill_thresholds: Vec2,
    pub outline_thresholds: Vec2,
}

impl GlyphDraw {
    pub fn new(
        glyph: &GlyphTextureGeometry,
        position: Vec2,
        scale: Vec2,
        text_height: f32,
        is_ruby: bool,
    ) -> Result<Self> {
        ensure!(
            position.is_finite() && scale.is_finite() && scale.cmpgt(Vec2::ZERO).all(),
            "FNT6 draw position/scale is invalid"
        );
        ensure!(
            text_height.is_finite()
                && text_height > 0.0
                && glyph.extra_metric.is_finite()
                && glyph.extra_metric > 0.0,
            "FNT6 draw height/font size must be positive"
        );
        ensure!(
            glyph
                .dimensions
                .into_iter()
                .all(|n| n > 0 && n <= i32::MAX as u32),
            "FNT6 drawable dimensions are invalid"
        );

        // Native d4884..d4980 uses separate products/additions and cached padded
        // dimensions. Preserve the right/bottom order instead of adding size
        // to an already-positioned top-left corner.
        let bearing = vec2(glyph.origin[0] as f32, glyph.origin[1] as f32) * scale;
        let pixels = vec2(glyph.dimensions[0] as f32, glyph.dimensions[1] as f32) * scale;
        let start = position + bearing;
        let end = position + (pixels + bearing);
        let uv_start = vec2(-0.0 * glyph.scale[0], -0.0 * glyph.scale[1]);
        let uv_end = Vec2::from_array(glyph.scale);
        let (top, bottom) = if is_ruby {
            (0.0, 0.0)
        } else {
            let y = glyph.origin[1].wrapping_add(glyph.font_metrics[0]);
            let end = y.wrapping_add(glyph.dimensions[1] as i32);
            let inverse_height = 1.0 / text_height;
            (y as f32 * inverse_height, end as f32 * inverse_height)
        };
        let vertices = [
            TextVertex {
                position: vec4(start.x, start.y, uv_start.x, uv_start.y),
                color: top,
            },
            TextVertex {
                position: vec4(end.x, start.y, uv_end.x, uv_start.y),
                color: top,
            },
            TextVertex {
                position: vec4(start.x, end.y, uv_start.x, uv_end.y),
                color: bottom,
            },
            TextVertex {
                position: vec4(end.x, end.y, uv_end.x, uv_end.y),
                color: bottom,
            },
        ];
        // Native d4d88..d4d9c uses the unscaled unsigned owner word6 for fill.
        // Outline d4c74..d4c9c additionally divides by the glyph vertical scale.
        let fill = 1.5 / glyph.extra_metric;
        let fill_thresholds = vec2((-fill).max(-1.0), fill.min(1.0));
        let center = (-2.0 / glyph.extra_metric) / scale.y + 0.0;
        let radius = (1.5 / glyph.extra_metric) / scale.y;
        let outline_thresholds = vec2((center - radius).max(-1.0), (radius + center).max(-1.0));
        Ok(Self {
            vertices,
            fill_thresholds,
            outline_thresholds,
        })
    }

    /// Draw outlines for the whole text batch before drawing its fills.
    pub fn render_outline(
        &self,
        glyph: &GpuGerokasu2GlyphHandle,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        color: FloatColor4,
    ) -> Result<()> {
        self.render(
            glyph,
            pass,
            builder,
            transform,
            color,
            color,
            self.outline_thresholds,
        )
    }

    pub fn render_fill(
        &self,
        glyph: &GpuGerokasu2GlyphHandle,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        color1: FloatColor4,
        color2: FloatColor4,
    ) -> Result<()> {
        self.render(
            glyph,
            pass,
            builder,
            transform,
            color1,
            color2,
            self.fill_thresholds,
        )
    }

    fn render(
        &self,
        glyph: &GpuGerokasu2GlyphHandle,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        color1: FloatColor4,
        color2: FloatColor4,
        thresholds: Vec2,
    ) -> Result<()> {
        pass.run(builder.build(
            RenderProgramWithArguments::FontSdf {
                vertices: VertexSource::VertexData {
                    vertices: &self.vertices,
                },
                glyph: glyph.as_texture_source()?,
                transform,
                color1,
                color2,
                thresholds,
            },
            DrawPrimitive::TrianglesStrip,
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod gpu_tests;
