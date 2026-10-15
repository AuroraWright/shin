//! Updated older layer transforms. Property numbers and composition flags are
//! separate from Umineko's later 90-property, three-flag convention.
use super::multiply_transforms;
use glam::{Mat4, Vec2, Vec3, vec2, vec3};

pub const PROPERTY_COUNT: usize = 65;

// Original74ed0 calls sinf and cosf separately. Outlining preserves those
// services when optimized code would otherwise combine them into sin_cos;
// Darwin's combined service differs by an f32 ULP for some authored angles.
#[inline(never)]
fn rotation_sine(angle: f32) -> f32 {
    angle.sin()
}
#[inline(never)]
fn rotation_cosine(angle: f32) -> f32 {
    angle.cos()
}
pub(crate) fn native_rotation_z(angle: f32) -> Mat4 {
    let sine = rotation_sine(angle);
    let cosine = rotation_cosine(angle);
    Mat4::from_cols_array(&[
        cosine, sine, 0.0, 0.0, -sine, cosine, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ])
}

/// Packed native transform at updated6914c: matrix, child camera, inherited
/// translation and wobble. Positive script Y and camera values are negated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HigurashiTransform {
    pub transform: Mat4,
    pub camera: Vec3,
    pub inherited: Vec2,
    pub wobble: Vec2,
}

impl Default for HigurashiTransform {
    fn default() -> Self {
        Self {
            transform: Mat4::IDENTITY,
            camera: Vec3::ZERO,
            inherited: Vec2::ZERO,
            wobble: Vec2::ZERO,
        }
    }
}

impl HigurashiTransform {
    /// Extract the local matrix with already evaluated native X/Y/rotation
    /// oscillator samples. Oscillator clocks and the older property owner are
    /// supplied by the command bridge; later Wobbler modes must not be assumed.
    pub fn from_properties(p: &[f32; PROPERTY_COUNT], samples: [f32; 3]) -> Self {
        let factor = 0.001f32;
        let scale_origin = vec3(p[10], p[11], 0.0);
        let rotation_origin = vec3(p[14], p[15], 0.0);
        let rotation = (p[16] * factor + samples[2] * (p[34] * factor)) * std::f32::consts::TAU;
        let translation = vec3(
            ((p[0] + p[3]) - p[17]) + p[14],
            (-(p[1] + p[4]) - p[18]) + p[15],
            p[2] - 1000.0,
        );
        let mut transform =
            multiply_transforms(Mat4::from_translation(-scale_origin), Mat4::IDENTITY);
        transform = multiply_transforms(
            Mat4::from_scale(vec3(p[12] * factor, p[13] * factor, 1.0)),
            transform,
        );
        transform = multiply_transforms(
            Mat4::from_translation(scale_origin - rotation_origin),
            transform,
        );
        transform = multiply_transforms(native_rotation_z(rotation), transform);
        transform = multiply_transforms(Mat4::from_translation(translation), transform);
        Self {
            transform,
            camera: -vec3(p[60], p[61], p[62]),
            inherited: -vec2(p[63], p[64]),
            wobble: vec2(samples[0] * p[28], samples[1] * p[31]),
        }
    }

    pub fn compose_flags(p: &[f32; PROPERTY_COUNT]) -> u32 {
        p[22] as u32
    }

    /// Bit0 bypasses camera/depth and parent matrix together; bit1 suppresses
    /// parent wobble. Later Umineko assigns these to three independent bits.
    pub fn compose_with(&mut self, parent: &Self, flags: u32) {
        let inherited = Mat4::from_translation(parent.inherited.extend(0.0));
        if flags & 1 == 0 {
            // Updated69268..69274 loads only parent camera X/Y and
            // explicitly supplies zero Z to the translation constructor.
            self.transform = multiply_transforms(
                Mat4::from_translation(parent.camera.with_z(0.0)),
                self.transform,
            );
            let distance = self.transform.w_axis.z + 1000.0;
            let scale = if distance == 0.0 {
                distance
            } else {
                1000.0 / distance
            };
            self.transform =
                multiply_transforms(Mat4::from_scale(vec3(scale, scale, 1.0)), self.transform);
            self.transform.w_axis.z = 0.0;
            self.transform = multiply_transforms(inherited, self.transform);
            self.transform = multiply_transforms(parent.transform, self.transform);
        } else {
            self.transform = multiply_transforms(inherited, self.transform);
        }
        if flags & 2 == 0 {
            self.wobble += parent.wobble;
        }
    }

    /// Bottom-centered 1280x720 OpenGL projection from updated690d8..690f4.
    /// Keeping the raw matrix is useful for native instruction comparisons.
    pub fn compute_native_final_transform(&self) -> Mat4 {
        let projection = Mat4::from_cols_array(&[
            2.0 / 1280.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / 720.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            -0.0,
            -1.0,
            -0.0,
            1.0,
        ]);
        let projection =
            multiply_transforms(projection, Mat4::from_translation(self.wobble.extend(0.0)));
        multiply_transforms(projection, self.transform)
    }

    /// Convert only clip Z to wgpu's 0..1 range, retaining native XY geometry.
    pub fn compute_final_transform(&self) -> Mat4 {
        Self::convert_clip_depth(self.compute_native_final_transform())
    }

    /// Updated picture9adf4 and bustup36a08 subtract the signed header origin
    /// before converting authored 1920x1080 geometry into the 1280x720 canvas.
    /// PIC placements are appended to this matrix, inside that conversion.
    pub fn compute_native_asset_transform(&self, origin: (i16, i16)) -> Mat4 {
        let translation = Mat4::from_translation(vec3(-origin.0 as f32, -origin.1 as f32, 0.0));
        let scale = Mat4::from_scale(vec3(1280.0 / 1920.0, 720.0 / 1080.0, 1.0));
        multiply_transforms(
            self.compute_native_final_transform(),
            multiply_transforms(scale, translation),
        )
    }


    /// Message ctor81d10/render84f6c:1920x1080 top-left geometry, zero Z.
    pub fn compute_native_message_transform(&self, offset: Vec2) -> Mat4 {
        let geometry = multiply_transforms(
            Mat4::from_translation(vec3(-640.0, -720.0, 0.0)),
            Mat4::from_scale(vec3(2.0 / 3.0, 2.0 / 3.0, 0.0)),
        );
        multiply_transforms(
            multiply_transforms(self.compute_native_final_transform(), geometry),
            Mat4::from_translation(offset.extend(0.0)),
        )
    }

    pub fn compute_message_transform(&self, offset: Vec2) -> Mat4 {
        Self::convert_clip_depth(self.compute_native_message_transform(offset))
    }

    /// Tile d69dc..d6a0c appends an identity geometry matrix rather than the
    /// device conversion/header translation used for PIC/BUP.
    pub fn compute_native_tile_transform(&self) -> Mat4 {
        multiply_transforms(self.compute_native_final_transform(), Mat4::IDENTITY)
    }

    pub fn compute_tile_transform(&self) -> Mat4 {
        Self::convert_clip_depth(self.compute_native_tile_transform())
    }

    pub(crate) fn convert_clip_depth(native: Mat4) -> Mat4 {
        let conversion = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
        ]);
        multiply_transforms(conversion, native)
    }

    #[cfg(test)]
    fn words(&self) -> [f32; 23] {
        let mut out = [0.0; 23];
        out[..16].copy_from_slice(&self.transform.to_cols_array());
        out[16..19].copy_from_slice(&self.camera.to_array());
        out[19..21].copy_from_slice(&self.inherited.to_array());
        out[21..23].copy_from_slice(&self.wobble.to_array());
        out
    }
    #[cfg(test)]
    fn from_words(words: [f32; 23]) -> Self {
        Self {
            transform: Mat4::from_cols_array(words[..16].try_into().unwrap()),
            camera: Vec3::from_slice(&words[16..19]),
            inherited: Vec2::from_slice(&words[19..21]),
            wobble: Vec2::from_slice(&words[21..23]),
        }
    }
}

#[cfg(test)]
mod tests;
