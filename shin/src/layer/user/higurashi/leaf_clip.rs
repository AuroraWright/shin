//! Older696d0/6a994 leaf clipping and6a1cc block visibility.
//!
//! This isolated proposal is not registered by the parent module yet. The
//! associated direct original CPU oracle verifies rectangles, clip packets,
//! depth/stencil requests, projected corners and the visibility return.
use glam::{Mat4, vec3};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DepthFunction, DepthState, DepthStencilState, DrawPrimitive,
    RenderProgramWithArguments, RenderRequestBuilder, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColVertex},
};

use crate::layer::{
    higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
};

use super::native_float::{add as native_add, sub as native_sub};

/// Nativea5f10 initializes the four-float sentinel before any layer render.
const NO_CLIP: f32 = f32::from_bits(0x7fc0_0000);

#[derive(Debug, Copy, Clone)]
pub(crate) struct Clip(pub [f32; 4]);

impl Clip {
    /// Native696d0 samples raw clocks44..47, orders endpoints using FCMP GT,
    /// and returns an all-NaN sentinel when they cover the logical canvas.
    pub(crate) fn from_properties(properties: &HigurashiProperties) -> Self {
        let p = properties.values();
        Self::from_endpoints([p[44], p[45], p[46], p[47]])
    }

    pub(crate) fn from_endpoints([x0, y0, x1, y1]: [f32; 4]) -> Self {
        let (left, right) = if x0 > x1 { (x1, x0) } else { (x0, x1) };
        let (top, bottom) = if y0 > y1 { (y1, y0) } else { (y0, y1) };
        if bottom >= 720.0 && right >= 1280.0 && left <= 0.0 && top <= 0.0 {
            Self([NO_CLIP; 4])
        } else {
            Self([left, top, native_sub(right, left), native_sub(bottom, top)])
        }
    }

    /// 6a994 rejects any NaN, including the full-canvas sentinel. It accepts
    /// zero/negative extents and infinities; GPU clipping determines coverage.
    pub(crate) fn active(self) -> bool {
        self.0.iter().all(|value| !value.is_nan())
    }

    pub(crate) fn native_projection() -> Mat4 {
        // Actual74d34(0,1280,720,0,-1,1), including native signed zeros.
        Mat4::from_cols_array(&[
            f32::from_bits(0x3acc_cccd),
            0.0,
            0.0,
            0.0,
            0.0,
            f32::from_bits(0xbb36_0b61),
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            -1.0,
            1.0,
            -0.0,
            1.0,
        ])
    }

    pub(crate) fn vertices(self) -> [PosColVertex; 4] {
        let [left, top, width, height] = self.0;
        let right = native_add(left, width);
        let bottom = native_add(top, height);
        [
            vec3(left, top, -1.0),
            vec3(right, top, -1.0),
            vec3(left, bottom, -1.0),
            vec3(right, bottom, -1.0),
        ]
        .map(|position| PosColVertex {
            position,
            color: UnormColor(0x80ff_ffff),
        })
    }

    /// Execute immediately after native-equivalent pass routing/visibility
    /// checks, before a leaf's material draw. This is not a group/plane clip.
    /// The return value belongs in that leaf's depth_stencil_shorthand call.
    pub(crate) fn prepare(self, pass: &mut RenderPass) -> bool {
        if !self.active() {
            return false;
        }
        // Nativea8fe0 flags2 writes raw GL depth0 with a triangle respecting
        // the current viewport. WGPU's native bridge maps this to0.5.
        pass.clear(None, None, Some(0.5));
        let vertices = self.vertices();
        pass.run(RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::NoColor)
            .depth_stencil(DepthStencilState {
                depth: DepthState { function: DepthFunction::Always, write_enable: true },
                stencil: StencilState::default(),
            })
            .build(RenderProgramWithArguments::Fill {
                vertices: VertexSource::VertexData { vertices: &vertices },
                transform: HigurashiTransform::convert_clip_depth(Self::native_projection()),
            }, DrawPrimitive::TrianglesStrip));
        true
    }
}

/// Native GPU-block+8/+a/+c/+e. Signed origins and unsigned texture extents
/// must be retained from asset construction; rectangle vertices alone are not
/// the native conservative block bounds.
#[derive(Debug, Copy, Clone)]
pub(crate) struct BlockBounds {
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl BlockBounds {
    pub(crate) fn projected_corners(self, matrix: Mat4) -> [[f32; 3]; 4] {
        let left = f32::from(self.x);
        let top = f32::from(self.y);
        let right = (i32::from(self.x) + i32::from(self.width)) as f32;
        let bottom = (i32::from(self.y) + i32::from(self.height)) as f32;
        let m = matrix.to_cols_array();
        [[left, top], [right, top], [right, bottom], [left, bottom]].map(|[x, y]| {
            // Original74ff8 does two FMLAs, then ADD, then one reciprocal.
            // The z0 FMLA is significant for exceptional matrix elements.
            let transformed: [f32; 4] = std::array::from_fn(|i| {
                let xy = m[i + 4].mul_add(y, m[i] * x);
                m[i + 8].mul_add(0.0, xy) + m[i + 12]
            });
            let inverse_w = 1.0 / transformed[3];
            std::array::from_fn(|i| transformed[i] * inverse_w)
        })
    }

    /// Native6a04c supplies [-1,-1,+1,+1]. Invoke only after finding at least
    /// one nonempty native index range, before uniforms or GPU bindings.
    pub(crate) fn visible(self, native_matrix: Mat4) -> bool {
        self.visible_in(native_matrix, [-1.0, -1.0, 1.0, 1.0])
    }

    pub(crate) fn visible_in(self, native_matrix: Mat4, viewport: [f32; 4]) -> bool {
        projected_visible(
            self.projected_corners(native_matrix).map(|p| [p[0], p[1]]),
            viewport,
        )
    }
}

fn inside([x, y]: [f32; 2], [left, top, right, bottom]: [f32; 4]) -> bool {
    // HI and LT both reject unordered FCMPs in the native point tests.
    left <= x && right >= x && top <= y && bottom >= y
}

fn intersects(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let edge = [d[0] - c[0], d[1] - c[1]];
    let denominator = edge[0] * delta[1] - edge[1] * delta[0];
    if denominator == 0.0 {
        return false;
    }
    let offset = [a[0] - c[0], a[1] - c[1]];
    let t = (edge[1] * offset[0] - edge[0] * offset[1]) / denominator;
    // Native FCCMP PL,#0 / B.GT accepts unordered t/u after division.
    if t < 0.0 || t > 1.0 {
        return false;
    }
    let u = (offset[0] * delta[1] - offset[1] * delta[0]) / denominator;
    !(u < 0.0 || u > 1.0)
}

fn projected_visible(points: [[f32; 2]; 4], viewport: [f32; 4]) -> bool {
    if points.iter().any(|&p| inside(p, viewport)) {
        return true;
    }
    // FMINNM/FMAXNM initializes from TL, then GT/MI selects the other
    // corners. NaNs do not replace a finite bound; ±FLT_MAX are literal.
    let mut minimum = [points[0][0].min(f32::MAX), points[0][1].min(f32::MAX)];
    let mut maximum = [points[0][0].max(-f32::MAX), points[0][1].max(-f32::MAX)];
    for point in &points[1..] {
        for axis in 0..2 {
            if minimum[axis] > point[axis] {
                minimum[axis] = point[axis];
            }
            if maximum[axis] < point[axis] {
                maximum[axis] = point[axis];
            }
        }
    }
    let centroid = std::array::from_fn(|axis| {
        (((points[0][axis] + 0.0) + points[1][axis]) + points[2][axis] + points[3][axis]) * 0.25
    });
    if inside(centroid, viewport) {
        return true;
    }
    if maximum[0] < viewport[0]
        || viewport[2] < minimum[0]
        || maximum[1] < viewport[1]
        || viewport[3] < minimum[1]
    {
        return false;
    }
    let [left, top, right, bottom] = viewport;
    let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
    for i in 0..4 {
        for j in 0..4 {
            if intersects(
                points[j],
                points[(j + 1) % 4],
                corners[i],
                corners[(i + 1) % 4],
            ) {
                return true;
            }
        }
    }
    // This second native phase tests both quad diagonals against all four
    // viewport edges; it is required when the quad fully contains the view.
    for i in 0..4 {
        for [a, b] in [[0, 2], [1, 3]] {
            if intersects(points[a], points[b], corners[i], corners[(i + 1) % 4]) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod call_site_tests;
#[cfg(test)]
pub(crate) mod gpu_tests;
#[cfg(test)]
mod tests;
