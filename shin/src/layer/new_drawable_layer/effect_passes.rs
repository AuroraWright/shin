use glam::Vec4;
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};

use crate::{
    layer::LayerProperties,
    render::{PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, centered_projection_matrix},
};

pub fn apply_ghosting(
    context: &mut PreRenderContext,
    props: &LayerProperties,
    render_texture_src: &mut RenderTexture,
    render_texture_prev_frame: &RenderTexture,
    alpha: f32,
) {
    let mut pass = context.begin_pass(
        render_texture_src.as_texture_target(),
        None,
        "NewDrawableLayer/ghosting",
    );

    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::LayerPremultiplied1)
            .build(
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &build_quad_vertices(|t| PosTexVertex {
                            position: ((t * 2.0) - 1.0) * VIRTUAL_CANVAS_SIZE_VEC / 2.0,
                            texture_position: t,
                        }),
                    },
                    texture: render_texture_prev_frame.as_texture_source(),
                    transform: centered_projection_matrix() * props.get_ghosting_transform(),
                    color_multiplier: FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).premultiply(),
                    fragment_shader_param: Vec4::ZERO,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

/// Match native helpers 0x78400/0x78620 and the two program43 passes.
/// Intermediate images occupy the first half of full-sized textures; keeping
/// their other pixels is significant for samples near that region's boundary.
pub fn apply_blur(
    context: &mut PreRenderContext,
    source: &mut RenderTexture,
    target: &mut RenderTexture,
    radius: f32,
    radius_y: f32,
) {
    let (width, height) = source.dimensions();
    let half_viewport = ((width >> 1) as f32, (height >> 1) as f32);
    for (stage, scale, step) in [
        (0, 1.0, Vec4::ZERO),
        (1, 0.5, Vec4::new(radius / 1920.0, 0.0, 0.0, 0.0)),
        (2, 0.5, Vec4::new(0.0, radius_y / 1080.0, 0.0, 0.0)),
        (3, 0.5, Vec4::ZERO),
    ] {
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: uv * scale,
        });
        {
            let mut pass = context.begin_pass(target.as_texture_target(), None, "Drawable blur");
            if stage != 3 {
                pass.set_viewport(half_viewport.0, half_viewport.1);
            }
            let program = if stage == 1 || stage == 2 {
                RenderProgramWithArguments::Blur {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    step,
                }
            } else {
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    color_multiplier: FloatColor4::WHITE,
                    fragment_shader_param: Vec4::ZERO,
                }
            };
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Opaque)
                    .build(program, DrawPrimitive::TrianglesStrip),
            );
        }
        std::mem::swap(source, target);
    }
}

pub fn apply_zoom_blur(
    context: &mut PreRenderContext,
    props: &LayerProperties,
    source: &mut RenderTexture,
    target: &mut RenderTexture,
    strength: f32,
) {
    use shin_core::vm::command::types::LayerProperty;
    let center = Vec4::new(
        (props.get_value(LayerProperty::Prop71) / 1920.0 + 0.5) * 0.5,
        (props.get_value(LayerProperty::Prop72) / 1080.0 + 0.5) * 0.5,
        0.0,
        0.0,
    );
    let mut factors = [1.0; 8];
    factors[1] = 1.0 / (1.0 + strength * 0.005);
    for i in 2..8 {
        factors[i] = factors[i - 1] * 0.99;
    }
    let scales = [
        Vec4::from_slice(&factors[..4]),
        Vec4::from_slice(&factors[4..]),
    ];
    let (width, height) = source.dimensions();
    for (stage, uv_scale) in [(0, 1.0), (1, 0.5), (2, 0.5)] {
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: uv * uv_scale,
        });
        {
            let mut pass =
                context.begin_pass(target.as_texture_target(), None, "Drawable zoom blur");
            if stage != 2 {
                pass.set_viewport((width >> 1) as f32, (height >> 1) as f32);
            }
            let program = if stage == 1 {
                RenderProgramWithArguments::ZoomBlur {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    center,
                    scales,
                }
            } else {
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    color_multiplier: FloatColor4::WHITE,
                    fragment_shader_param: Vec4::ZERO,
                }
            };
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Opaque)
                    .build(program, DrawPrimitive::TrianglesStrip),
            );
        }
        std::mem::swap(source, target);
    }
}

pub fn apply_dissolve(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    intensity: f32,
    mode: i32,
    seed: u32,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        DepthStencilState, StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    };
    let mesh = super::dissolve::DissolveMesh::new(intensity, mode, seed);
    let breakup = context
        .breakup_texture
        .expect("Dissolve requires the effects.txa breakup texture");
    let mut pass = context.begin_pass(
        target.as_texture_target(),
        Some(context.depth_stencil),
        "Drawable dissolve",
    );
    pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), Some(0), None);
    // Native first copies an intact triangle while marking stencil1. Pieces
    // use NotEqual1, so their overlapping circles cannot alter that region.
    if !mesh.intact.is_empty() {
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil(DepthStencilState {
                    depth: Default::default(),
                    stencil: StencilState {
                        pipeline: StencilPipelineState {
                            pass_operation: StencilOperation::Replace,
                            ..Default::default()
                        },
                        stencil_reference: 1,
                    },
                })
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::Layer {
                        output_kind: LayerShaderOutputKind::Layer,
                        fragment_shader: LayerFragmentShader::Default,
                        vertices: VertexSource::VertexData {
                            vertices: &mesh.intact,
                        },
                        texture: source.as_texture_source(),
                        transform: crate::render::top_left_projection_matrix(),
                        color_multiplier: FloatColor4::WHITE,
                        fragment_shader_param: Vec4::ZERO,
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }
    if !mesh.pieces.is_empty() {
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil(DepthStencilState {
                    depth: Default::default(),
                    stencil: StencilState {
                        pipeline: StencilPipelineState {
                            function: StencilFunction::NotEqual,
                            ..Default::default()
                        },
                        stencil_reference: 1,
                    },
                })
                .color_blend_type(ColorBlendType::LayerPremultiplied1)
                .build(
                    RenderProgramWithArguments::Dissolve {
                        vertices: VertexSource::VertexData {
                            vertices: &mesh.pieces,
                        },
                        texture: source.as_texture_source(),
                        breakup: breakup.as_source(),
                        transform: crate::render::top_left_projection_matrix(),
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }
}

pub fn apply_mosaic(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    size: i32,
) {
    let cell_size = size.wrapping_add(1) as f32;
    let x = cell_size / 1920.0;
    let y = cell_size / 1080.0;
    let vertices = build_quad_vertices(|uv| PosTexVertex {
        position: uv * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: uv,
    });
    let mut pass = context.begin_pass(target.as_texture_target(), None, "Drawable mosaic");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Mosaic {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    step: Vec4::new(x, y, 1.0 / x, 1.0 / y),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_zoom_blur_shader_scales_weights_and_signed_strength() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (offset, expected) in [
            (0x58, 0x4c58300c00070000u64), // subtract center x
            (0x68, 0x4c58300c00170101),    // subtract center y
            (0x98, 0x4c68100c00471108),    // first scale
            (0xe8, 0xd8320080c0d70c0e),    // second scale sample RGBA
            (0x188, 0x4c68101000170e14),   // second sample weight
            (0x1a8, 0x49a00a1000070614),   // first weighted sample accumulate
        ] {
            let start = 0x21d570 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
        for (address, expected) in [(0x159a8c, 0x3ba3d70au32), (0x15951c, 0x3f7d70a4)] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        for (address, expected) in [
            (0x764cc, 0x1e20c04au32), // abs only for dispatch
            (0x76c94, 0x1e200823),    // signed strength*.005
            (0x76c9c, 0x1e212860),    // +1
            (0x76ca0, 0x1e201820),    // reciprocal
            (0x76cb0, 0x1e210800),    // successive *.99
            (0x76bd4, 0xd291c728),    // first weight low bits
            (0x76bd8, 0xf2a7cc68),    // first weight high bits
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_blur_shader_weights_half_viewports_and_copy_helpers() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // Native shader takes half each weight before accumulating sample pairs.
        for (offset, expected) in [
            (0xa8, 0x4c68100c00171111u64),
            (0x168, 0x4c68100c00070c02),
            (0x218, 0x4c68100c0027120b),
            (0x238, 0x4c68100c00371209),
            (0x98, 0xd832008040370202),
        ] {
            let start = 0x1df170 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
        // Weight bit patterns, two half-viewports and down/up helper calls.
        for (address, expected) in [
            (0x76760, 0xd29dad69u32),
            (0x76764, 0xf2a7d9c9),
            (0x76768, 0xd28fb7ca),
            (0x7676c, 0xf2a7c86a),
            (0x76774, 0xf2c4f7c9),
            (0x76778, 0xf2e7d429),
            (0x7677c, 0xf2d5fbea),
            (0x76780, 0xf2e7b70a),
            (0x76838, 0x53017ee2),
            (0x7683c, 0x53017c03),
            (0x769d8, 0x53017ee2),
            (0x769dc, 0x53017c03),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_ripple_shader_normalizes_radius_and_subtracts_phase_before_frequency() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (offset, expected) in [
            (0x90, 0x5080000000870204u64), // SM52 MUFU SQRT
            (0x98, 0x51a2030c00670403),    // 2*radius-phase
            (0xb8, 0x4c68100c00470303),    // multiply frequency
            (0xd8, 0x5080000000570203),    // RSQ for direction
            (0x108, 0x49a0020c0057050a),   // radius+sin*amplitude
            (0x138, 0xd832008020570400),   // sample RGBA
        ] {
            let start = 0x20b0f0 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_raster_shader_uses_crossed_sines_and_phase_updates_are_unconditional() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let file = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(file).unwrap();
        for (offset, expected) in [
            (0x48, 0x51a0000c00470500u64),
            (0x50, 0x51a0008c00570401),
            (0x58, 0x1e240c90fdb70000),
            (0x68, 0x1e240c90fdb70101),
            (0x88, 0x5080000000170707),
            (0x90, 0x5080000000170606),
            (0x98, 0x49a0020c00070704),
            (0xa8, 0x49a0028c00170605),
            (0xb0, 0xd832008020570400),
        ] {
            let start = 0x209770 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
        // Phase blocks branch only on period==0, then add (1/period)*delta.
        // No animation-enabled branch exists between properties and phases.
        for (address, expected) in [
            (0x78890, 0x1e202008u32),
            (0x788a4, 0x1e280800),
            (0x788a8, 0x1e212800),
            (0x788ac, 0xbd004e60),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_mosaic_shader_uses_centered_floor_and_cell_centers() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let file = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(file).unwrap();
        // Program42: subtract center c3[0x10/14], floor scaled XY,
        // add0.5, scale by cell step, add the center, sample RGBA.
        for (offset, expected) in [
            (0x30, 0x4c58300c00470000u64),
            (0x38, 0x4c58300c00570101),
            (0x58, 0x5ca8148000470a04),
            (0x70, 0x5ca8148000570a05),
            (0x88, 0x3858103f00070406),
            (0x90, 0x3858103f00070507),
            (0xb0, 0xd832008020770600),
        ] {
            let start = 0x2057b0 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
    }
}

pub fn apply_raster(
    context: &mut PreRenderContext,
    props: &LayerProperties,
    source: &RenderTexture,
    target: &mut RenderTexture,
    phases: [f32; 3],
) {
    use shin_core::vm::command::types::LayerProperty::*;
    let spatial_frequency = |period: f32, canvas_extent: f32| {
        let normalized = period / canvas_extent;
        if normalized == 0.0 {
            0.0
        } else {
            1.0 / normalized
        }
    };
    let parameters = Vec4::new(
        props.get_value(RasterHorizontalAmplitude) / 1920.0,
        props.get_value(RasterVerticalAmplitude) / 1080.0,
        spatial_frequency(props.get_value(RasterHorizontalLPeriod), 1080.0),
        spatial_frequency(props.get_value(RasterVerticalLPeriod), 1920.0),
    );
    let vertices = build_quad_vertices(|uv| PosTexVertex {
        position: uv * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: uv,
    });
    let mut pass = context.begin_pass(target.as_texture_target(), None, "Drawable raster");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Raster {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    amplitude_frequency: parameters,
                    phase: Vec4::new(phases[0], phases[1], 0.0, 0.0),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

pub fn apply_ripple(
    context: &mut PreRenderContext,
    props: &LayerProperties,
    source: &RenderTexture,
    target: &mut RenderTexture,
    phase: f32,
) {
    use shin_core::vm::command::types::LayerProperty::*;
    // Native explicitly truncates the spatial period before normalization.
    let period = (props.get_value(RippleLPeriod) as i32) as f32 * 0.001;
    let frequency = if period == 0.0 { 0.0 } else { 1.0 / period };
    let vertices = build_quad_vertices(|uv| PosTexVertex {
        position: uv * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: uv,
    });
    let mut pass = context.begin_pass(target.as_texture_target(), None, "Drawable ripple");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Ripple {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: crate::render::top_left_projection_matrix(),
                    wave: Vec4::new(
                        frequency,
                        props.get_value(RippleAmplitude) / 1920.0,
                        phase,
                        0.0,
                    ),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
