use bitflags::bitflags;
use glam::{Mat4, Vec2, Vec3, Vec4, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_render::{LayerBlendType, LayerFragmentShader};

use crate::render::centered_projection_matrix;

pub mod higurashi;

bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct ComposeFlags: i32 {
        const IGNORE_CAMERA_POSITION = 0b00000001;
        const DONT_INHERIT_TRANSFORM = 0b00000010;
        const DONT_INHERIT_WOBBLE = 0b00000100;
    }
}

/// Native a1990: one rounded multiply followed by three ordered FMAs per lane.
pub(crate) fn multiply_transforms(left: Mat4, right: Mat4) -> Mat4 {
    let left = left.to_cols_array();
    let right = right.to_cols_array();
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            let value = left[row] * right[column * 4];
            let value = left[4 + row].mul_add(right[column * 4 + 1], value);
            let value = left[8 + row].mul_add(right[column * 4 + 2], value);
            result[column * 4 + row] = left[12 + row].mul_add(right[column * 4 + 3], value);
        }
    }
    Mat4::from_cols_array(&result)
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TransformParams {
    pub transform: Mat4,
    /// Camera position used when composing this layer's children.
    pub camera_position: Vec3,
    /// This translation is always inherited by children, but not applied to the layer itself
    pub unconditionally_inherited_translation: Vec2,
    /// This is a translation coming from the parent wobbler and its inheritance can be controlled with FLAG_2
    pub wobble_translation: Vec2,
}

impl TransformParams {
    pub fn compose_with(&mut self, composed_with: &Self, flags: ComposeFlags) {
        let some_origin = if flags.contains(ComposeFlags::IGNORE_CAMERA_POSITION) {
            Vec3::ZERO
        } else {
            composed_with.camera_position
        };

        // extract distance from the camera and apply scaling based on it
        let z_distance = (self.transform.w_axis.z - some_origin.z) * 0.001;

        let distance_scale = if z_distance <= 0.0 {
            0.0
        } else {
            self.transform = multiply_transforms(
                Mat4::from_translation((-some_origin).with_z(1.0)),
                self.transform,
            );

            1.0 / z_distance
        };

        self.transform = multiply_transforms(
            Mat4::from_scale(vec3(distance_scale, distance_scale, 1.0)),
            self.transform,
        );
        // the z translation has served its purpose, zero it out
        self.transform.w_axis.z = 0.0;

        if !flags.contains(ComposeFlags::DONT_INHERIT_TRANSFORM) {
            self.transform = multiply_transforms(composed_with.transform, self.transform);
        }
        if !flags.contains(ComposeFlags::DONT_INHERIT_WOBBLE) {
            self.transform = multiply_transforms(
                Mat4::from_translation(composed_with.wobble_translation.extend(0.0)),
                self.transform,
            );
        }

        self.transform = multiply_transforms(
            Mat4::from_translation(
                composed_with
                    .unconditionally_inherited_translation
                    .extend(0.0),
            ),
            self.transform,
        );
    }

    pub fn compute_total_translation(&self) -> Mat4 {
        multiply_transforms(
            Mat4::from_translation(self.wobble_translation.extend(0.0)),
            self.transform,
        )
    }

    pub fn compute_final_transform(&self) -> Mat4 {
        multiply_transforms(
            centered_projection_matrix(),
            self.compute_total_translation(),
        )
    }

    pub fn compute_top_left_transform(&self) -> Mat4 {
        // Native Quizc81d0/Cake44b90 compose the centered projection with
        // a final -960,-540 translation, then submit uncentered vertices.
        // Moving this translation into vertices changes affine rounding.
        multiply_transforms(
            self.compute_final_transform(),
            Mat4::from_translation(vec3(-960.0, -540.0, 0.0)),
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DrawableParams {
    pub color_multiplier: FloatColor4,
    pub blend_type: LayerBlendType,
    pub fragment_shader: LayerFragmentShader,
    pub shader_param: Vec4,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DrawableClipMode {
    /// Don't clip anything
    None,
    /// Clip, applying the [`TransformParams`] to the coordinates
    Clip,
    /// Clip, ignoring the [`TransformParams`] and using the screen coordinates directly
    ClipIgnoreTransform,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DrawableClipParams {
    pub mode: DrawableClipMode,
    /// xy - top left, zw - width height
    pub area: Vec4,
}

impl DrawableClipParams {
    /// Native 0x732f0 marks the rectangle then inverts only stencil bit0x40.
    /// Regular layer comparisons reject the resulting outside mask, while
    /// retaining all lower bits used to order layers.
    pub fn begin(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        transform: &TransformParams,
    ) {
        use shin_core::primitives::color::UnormColor;
        use shin_render::{
            ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
            StencilFunction, StencilOperation,
            quad_vertices::build_quad_vertices,
            shaders::types::{buffer::VertexSource, vertices::PosColVertex},
        };
        if self.mode == DrawableClipMode::None
            || self.area.to_array().iter().any(|v| v.is_nan())
            || self.area.z == 0.0
            || self.area.w == 0.0
        {
            return;
        }
        let matrix = match self.mode {
            DrawableClipMode::Clip => transform.compute_final_transform(),
            DrawableClipMode::ClipIgnoreTransform => centered_projection_matrix(),
            DrawableClipMode::None => unreachable!(),
        };
        let vertices = build_quad_vertices(|uv| PosColVertex {
            position: (Vec2::new(self.area.x, self.area.y)
                + uv * Vec2::new(self.area.z, self.area.w))
            .extend(0.0),
            color: UnormColor::WHITE,
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::NoColor)
                .depth_stencil(Self::stencil(
                    StencilFunction::Always,
                    StencilOperation::Replace,
                    StencilOperation::Replace,
                ))
                .build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform: matrix,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
        self.fullscreen(
            pass,
            StencilFunction::Equal,
            StencilOperation::Replace,
            StencilOperation::Zero,
        );
    }

    pub fn end(&self, pass: &mut shin_render::render_pass::RenderPass) {
        use shin_render::{StencilFunction, StencilOperation};
        if self.mode != DrawableClipMode::None {
            self.fullscreen(
                pass,
                StencilFunction::Always,
                StencilOperation::Zero,
                StencilOperation::Zero,
            );
        }
    }

    fn stencil(
        function: shin_render::StencilFunction,
        fail: shin_render::StencilOperation,
        success: shin_render::StencilOperation,
    ) -> shin_render::DepthStencilState {
        use shin_render::{DepthStencilState, StencilMask, StencilPipelineState, StencilState};
        DepthStencilState {
            depth: Default::default(),
            stencil: StencilState {
                stencil_reference: 0x40,
                pipeline: StencilPipelineState {
                    function,
                    stencil_fail_operation: fail,
                    depth_fail_operation: success,
                    pass_operation: success,
                    stencil_read_mask: StencilMask::ClipOnly,
                    stencil_write_mask: StencilMask::ClipOnly,
                },
            },
        }
    }

    fn fullscreen(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        function: shin_render::StencilFunction,
        fail: shin_render::StencilOperation,
        success: shin_render::StencilOperation,
    ) {
        use shin_render::{
            ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
            shaders::types::{buffer::VertexSource, vertices::PosVertex},
        };
        let vertices = [
            PosVertex {
                position: vec3(-1.0, -1.0, 0.0),
            },
            PosVertex {
                position: vec3(3.0, -1.0, 0.0),
            },
            PosVertex {
                position: vec3(-1.0, 3.0, 0.0),
            },
        ];
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::NoColor)
                .depth_stencil(Self::stencil(function, fail, success))
                .build(
                    RenderProgramWithArguments::Clear {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        color: FloatColor4::BLACK,
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }
}

#[cfg(test)]
mod clip_native_tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_clip_property_order_modes_and_reserved_stencil_mask() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // Property reads use62,63,64,65; comparisons sort62/64 and63/65.
        for (address, expected) in [
            (0x78a3c, 0x5282af08u32),
            (0x78a50, 0x5282ba08),
            (0x78a60, 0x5282c508),
            (0x78a70, 0x5282d008),
            (0x78a94, 0x1e2b2120),
            (0x78aa8, 0x1e2a2100),
            (0x73304, 0x7100091f),
            (0x7330c, 0x7100051f),
            (0x7dae4, 0x940001ab), // masked group setup before clipping
            (0x7daf0, 0x97ffd600), // begin clip
            (0x7dec0, 0x97ffd5b4), // end clip before mask cleanup
            (0x7dee4, 0x5290100a), // mask cleanup uses0x8080
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        // Reserved0x40 stencil reference/read/write mask and cleanup stencilZero.
        for (address, expected) in [(0x73430, 0x52880808u32), (0x735d0, 0x5288080a)] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }
}

#[cfg(test)]
mod native_transform_tests;
