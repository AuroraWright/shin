//! Updated-Higurashi Tile Gamma, including nonfinite material and geometry.
//!
//! The real CPU dispatcher6add4 sends unsimplified Gamma5 directly to its
//! epilogue. Tile leaves rectangle bottom/Y in the first two color lanes.
//! Active masks leave SDK Fill residue that packs to zero in the other lanes;
//! no-mask returns retain actual clip width/height, including NaNs/infinities.
//! This is not texture Gamma/pow. Color packing and pre-Gamma pass selection
//! follow original complete Tile/SDK captures, including nonfinite inputs.
use anyhow::{Result, ensure};
use glam::{Mat4, Vec4, vec3};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, PassKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColVertex},
};

use super::{leaf_clip::Clip, native_float::add as native_add, tile::Tile};
use crate::layer::render_params::DrawableParams;

const CANONICAL_NAN_BITS: u32 = 0x7fc0_0000;

/// Validated color/geometry and the BEFORE-Gamma pass/blend decision.
///
/// Keeping packed bytes here avoids an RGBA-float round trip and a second
/// quantization. Construction happens only after the caller's native owner
/// visibility and requested-pass gates. No GPU calls occur during validation.
#[derive(Debug, Clone)]
pub(super) struct FullCanvas {
    rect: Vec4,
    color: UnormColor,
    pass_kind: PassKind,
    blend: ColorBlendType,
}

impl FullCanvas {
    /// Admit finite endpoints whose native ordering covers the logical canvas
    /// and produces the canonical four-NaN sentinel. !clip.active() alone is
    /// insufficient: authored NaNs can leave different caller color lanes.
    pub(super) fn new(
        tile: &Tile,
        drawable: &DrawableParams,
        endpoints: [f32; 4],
        clip: Clip,
    ) -> Result<Self> {
        ensure!(
            endpoints.into_iter().all(f32::is_finite)
                && Clip::from_endpoints(endpoints).0.map(f32::to_bits) == [CANONICAL_NAN_BITS; 4],
            "Higurashi Tile Gamma requires finite clip endpoints covering the logical canvas"
        );
        ensure!(
            clip.0.map(f32::to_bits) == [CANONICAL_NAN_BITS; 4],
            "Higurashi Tile Gamma requires the proven canonical four-NaN clip result"
        );
        Self::material(tile, drawable, [f32::from_bits(CANONICAL_NAN_BITS); 2])
    }

    /// Finite paths pack bottom/Y/zero/zero. Active clip reaches actual Fill
    /// BindProgram Q2/Q3 residue that quantizes to zero; fullcover retains
    /// canonical NaNs. Quiet-NaN no-mask clipping supplies actual width/height.
    fn material(tile: &Tile, drawable: &DrawableParams, tail: [f32; 2]) -> Result<Self> {
        ensure!(
            drawable.fragment_shader.simplify(drawable.shader_param) == LayerFragmentShader::Gamma,
            "Higurashi Tile Gamma helper requires unsimplified Gamma"
        );

        // Actual d6954..d69a8 performs one rounded FP32 ADD for bottom.
        // Native6a994's no-mask early return retains the getter's s2/s3.
        // Finite active clipping instead leaves SDK residue that packs to0.
        let bottom = native_add(tile.rect.w, tile.rect.y);
        let color = UnormColor::from_rgba(
            quantize_lane(bottom),
            quantize_lane(tile.rect.y),
            quantize_lane(tail[0]),
            quantize_lane(tail[1]),
        );
        Ok(Self {
            rect: tile.rect,
            color,
            // Original d68dc..d68f8 chooses these BEFORE6add4. In particular,
            // Gamma's emitted alpha does not turn an opaque Tile into a
            // transparent draw and does not select a different blend.
            pass_kind: tile.pass_kind(drawable),
            blend: tile.color_blend_type(drawable),
        })
    }

    #[cfg(test)]
    pub(super) fn pass_kind(&self) -> PassKind {
        self.pass_kind
    }

    #[cfg(test)]
    pub(super) fn color_blend_type(&self) -> ColorBlendType {
        self.blend
    }

    pub(super) fn vertices(&self) -> [PosColVertex; 4] {
        let left = self.rect.x;
        let right = native_add(self.rect.z, left);
        let top = self.rect.y;
        let bottom = native_add(self.rect.w, top);
        [
            vec3(left, top, 0.0),
            vec3(right, top, 0.0),
            vec3(left, bottom, 0.0),
            vec3(right, bottom, 0.0),
        ]
        .map(|position| PosColVertex {
            position,
            color: self.color,
        })
    }

    /// Same program1/quad/strict-stencil transport as older Tile, with the
    /// validated packed color. This domain never prepares a clip mask.
    pub(super) fn render(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        stencil_ref: u8,
        requested_pass: PassKind,
    ) {
        self.render_clipped(pass, transform, stencil_ref, requested_pass, false);
    }

    fn render_clipped(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        stencil_ref: u8,
        requested_pass: PassKind,
        clipped: bool,
    ) {
        if requested_pass != self.pass_kind {
            return;
        }
        let vertices = self.vertices();
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, clipped)
                .color_blend_type(self.blend)
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

    #[cfg(test)]
    fn returned_lanes(&self) -> [u32; 4] {
        [
            native_add(self.rect.w, self.rect.y).to_bits(),
            self.rect.y.to_bits(),
            CANONICAL_NAN_BITS,
            CANONICAL_NAN_BITS,
        ]
    }
}

/// Native no-mask path. Any NaN clip field skips preparation, retaining the
/// getter's width/height color lanes. These may themselves be infinite or NaN.
#[derive(Debug, Clone)]
pub(super) struct NoMask(FullCanvas);

impl NoMask {
    pub(super) fn new(
        tile: &Tile,
        drawable: &DrawableParams,
        endpoints: [f32; 4],
        clip: Clip,
    ) -> Result<Self> {
        if endpoints.into_iter().all(f32::is_finite) {
            return FullCanvas::new(tile, drawable, endpoints, clip).map(Self);
        }
        let actual_clip = Clip::from_endpoints(endpoints);
        ensure!(
            !actual_clip.active() && actual_clip.0.map(f32::to_bits) == clip.0.map(f32::to_bits),
            "Higurashi Tile Gamma no-mask clip requires the actual inactive getter result"
        );
        Ok(Self(FullCanvas::material(
            tile,
            drawable,
            [clip.0[2], clip.0[3]],
        )?))
    }

    pub(super) fn render(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        stencil_ref: u8,
        requested_pass: PassKind,
    ) {
        self.0.render(pass, transform, stencil_ref, requested_pass);
    }
}

/// Native active clipping, including infinities and overflowed extents.
/// Only NaN clip fields disable the mask; validation precedes GPU preparation.
#[derive(Debug, Clone)]
pub(super) struct ActiveClip {
    material: FullCanvas,
    clip: Clip,
}

impl ActiveClip {
    pub(super) fn new(tile: &Tile, drawable: &DrawableParams, endpoints: [f32; 4]) -> Result<Self> {
        let clip = Clip::from_endpoints(endpoints);
        ensure!(
            clip.active(),
            "Higurashi Tile Gamma active clip requires a non-NaN getter result"
        );
        Ok(Self {
            material: FullCanvas::material(
                tile,
                drawable,
                [f32::from_bits(CANONICAL_NAN_BITS); 2],
            )?,
            clip,
        })
    }

    pub(super) fn render(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        stencil_ref: u8,
        requested_pass: PassKind,
    ) {
        if requested_pass != self.material.pass_kind {
            return;
        }
        let clipped = self.clip.prepare(pass);
        self.material
            .render_clipped(pass, transform, stencil_ref, requested_pass, clipped);
    }
}

/// Original4cc90: FMAX with zero, rounded FP32*255, FCVTZU32, then unsigned
/// saturating16/8-bit narrows. Rust's saturating float-to-u32 conversion gives
/// the same final integer for finite lanes, NaNs and signed infinities.
/// The NaN branch expresses the observed integer result, not native FPSR.
fn quantize_lane(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    let nonnegative = if value > 0.0 { value } else { 0.0 };
    let scaled = nonnegative * 255.0;
    (scaled as u32).min(255) as u8
}

#[cfg(test)]
mod tests;
