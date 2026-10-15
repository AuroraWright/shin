//! Older Tile geometry/color path from updated d6860. Authored rectangles are
//! logical canvas units, unlike PIC/BUP's device-scaled asset geometry.
use glam::{Mat4, Vec3, Vec4, vec3};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerBlendType, LayerFragmentShader, PassKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColVertex},
};

use crate::layer::render_params::DrawableParams;

#[derive(Debug, Clone)]
pub(super) struct Tile {
    pub color: FloatColor4,
    pub rect: Vec4,
}

/// d68b4/4cc14 clamps the tint product before6add4. Unsimplified Gamma's
/// native CPU branch preserves incoming float registers; the owner dispatches
/// it through tile_gamma's separately validated clip and packed-color path.
fn shaded_color(color: FloatColor4, drawable: &DrawableParams) -> FloatColor4 {
    let color = (color.into_vec4() * drawable.color_multiplier.into_vec4())
        .min(Vec4::ONE)
        .max(Vec4::ZERO);
    let p = drawable.shader_param;
    let shader = drawable.fragment_shader.simplify(p);
    let result = match shader {
        LayerFragmentShader::Default => color,
        LayerFragmentShader::Gamma => {
            unreachable!("Unsimplified Gamma requires the validated Tile owner path")
        }
        LayerFragmentShader::Mono => {
            // 4cc38 preserves three multiplies and two additions; dot() can
            // fuse them and change the packed byte at a quantization boundary.
            let luma = (color.x * 0.299 + color.y * 0.587) + color.z * 0.114;
            Vec3::splat(luma).extend(color.w) * p.truncate().extend(1.0)
        }
        LayerFragmentShader::Fill => color + (p - color) * Vec3::splat(p.w).extend(0.0),
        LayerFragmentShader::Fill2 => color + (Vec4::splat(p.w) - color) * p.truncate().extend(0.0),
        LayerFragmentShader::Negative => {
            ((Vec3::ONE - color.truncate()) * p.truncate()).extend(color.w)
        }
    };
    FloatColor4::from_vec4(result)
}

impl Tile {
    pub fn pass_kind(&self, drawable: &DrawableParams) -> PassKind {
        // Native d68e8/B.LT sends unordered (NaN) alpha to transparent.
        // Comparing the product directly preserves that decision and the
        // finite clamp's >=1 threshold, including positive infinity.
        let alpha = self.color.a * drawable.color_multiplier.a;
        if drawable.blend_type == LayerBlendType::Type1 && alpha >= 1.0 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        }
    }

    pub fn color_blend_type(&self, drawable: &DrawableParams) -> ColorBlendType {
        if self.pass_kind(drawable) == PassKind::Opaque {
            ColorBlendType::Opaque
        } else {
            // Original a8630 blend4 uses SrcAlpha with ReverseSubtract.
            ColorBlendType::from_higurashi_layer(drawable.blend_type)
        }
    }

    pub fn vertices(&self, drawable: &DrawableParams) -> [PosColVertex; 4] {
        let color: UnormColor = shaded_color(self.color, drawable).into_unorm();
        let left = self.rect.x;
        let right = left + self.rect.z;
        let top = self.rect.y;
        let bottom = top + self.rect.w;
        [
            vec3(left, top, 0.0),
            vec3(right, top, 0.0),
            vec3(left, bottom, 0.0),
            vec3(right, bottom, 0.0),
        ]
        .map(|position| PosColVertex { position, color })
    }


    #[cfg(test)]
    pub fn render(
        &self,
        pass: &mut RenderPass,
        drawable: &DrawableParams,
        transform: Mat4,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        self.render_clipped(pass, drawable, transform, stencil_ref, kind, false);
    }

    pub fn render_clipped(
        &self,
        pass: &mut RenderPass,
        drawable: &DrawableParams,
        transform: Mat4,
        stencil_ref: u8,
        kind: PassKind,
        clipped: bool,
    ) {
        // The older Tile draw checks raw owner visibility separately. A zero
        // base alpha still reaches its transparent draw, unlike the later Tile.
        if kind != self.pass_kind(drawable) {
            return;
        }
        let blend = self.color_blend_type(drawable);
        let vertices = self.vertices(drawable);
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, clipped)
                .color_blend_type(blend)
                .build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
