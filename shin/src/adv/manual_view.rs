//! Native ManualViewer shell, clipped pages, and authored controller hints.
use super::{
    character_view::CharacterTextures,
    manual::{ManualModel, ManualPhase},
};
use crate::layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams};
use glam::{Mat4, vec2, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

/// Draw authored pages under the native slide transform and y162..980 stencil.
/// The owner draws the backdrop, caption, close button and controller hints separately.
pub fn render_pages(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let transform =
        canvas * Mat4::from_translation(vec3(1920.0 * (1.0 - model.progress()), 0.0, 0.0));
    let clip = DrawableClipParams {
        mode: DrawableClipMode::Clip,
        area: vec4(0.0, 162.0, 1920.0, 818.0),
    };
    clip.begin(
        pass,
        &TransformParams {
            transform: crate::render::centered_projection_matrix().inverse() * transform,
            ..Default::default()
        },
    );
    let builder = RenderRequestBuilder::new()
        .color_blend_type(ColorBlendType::Layer1)
        .depth_stencil(DepthStencilState {
            stencil: StencilState {
                stencil_reference: 0,
                pipeline: StencilPipelineState {
                    function: StencilFunction::Equal,
                    stencil_read_mask: StencilMask::ClipOnly,
                    ..Default::default()
                },
            },
            ..Default::default()
        });
    for page in 0..model.count() {
        let (rect, alpha) = model.page_artwork(page).unwrap();
        if rect.x >= 1920.0 || rect.x + rect.z <= 0.0 {
            continue;
        }
        let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm();
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
            texture_position: vec2(corner.x, corner.y),
            color,
        });
        pass.run(builder.build(
            RenderProgramWithArguments::Sprite {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                sprite: textures.get(&format!("{:03}", page + 1)).as_source(),
                transform,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
    clip.end(pass);
}

fn sprite(
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    name: &str,
    rect: glam::Vec4,
    uv: glam::Vec4,
    alpha: f32,
    blend: ColorBlendType,
    canvas: Mat4,
) {
    let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm();
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
        position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
        texture_position: vec2(uv.x + uv.z * corner.x, uv.y + uv.w * corner.y),
        color,
    });
    pass.run(RenderRequestBuilder::new().color_blend_type(blend).build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: textures.get(name).as_source(),
            transform: canvas,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
/// Native85ab4..85c64 backdrop fades; the two quarter-alpha additive bars slide.
pub fn render_background(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let p = model.progress();
    sprite(
        textures,
        pass,
        "bg",
        vec4(0.0, 0.0, 1920.0, 1080.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        p,
        ColorBlendType::Layer1,
        canvas,
    );
    sprite(
        textures,
        pass,
        "obi",
        vec4(0.0, -160.0 + 160.0 * p, 1920.0, 160.0),
        vec4(0.0, 0.0, 1.0, 0.5),
        0.25,
        ColorBlendType::Layer2,
        canvas,
    );
    sprite(
        textures,
        pass,
        "obi",
        vec4(0.0, 1080.0 - 160.0 * p, 1920.0, 160.0),
        vec4(0.0, 0.5, 1.0, 0.5),
        0.25,
        ColorBlendType::Layer2,
        canvas,
    );
}
/// Native85f94..860b0 caption and close button share the top-bar vertical slide.
pub fn render_foreground(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let top = -160.0 + 160.0 * model.progress();
    sprite(
        textures,
        pass,
        "caption",
        vec4(480.0, top, 960.0, 128.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        1.0,
        ColorBlendType::Layer1,
        canvas,
    );
    textures.render_button(
        pass,
        "close",
        vec4(1764.0, top + 32.0, 1848.0, top + 116.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        shin_core::primitives::color::UnormColor::WHITE,
        glam::Vec4::ZERO,
        vec4(1.0, 1.0, 1.0, model.close_flash()),
        canvas,
    );
}

/// Native86140 lays labels and button icons out from right to left.
/// Native27320 consults fb9f0 for a possible 3/4 swap; the supplied executable's
/// fb9f0 returns zero, so the atlas indices below are unchanged.
fn render_hint_row(
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    right: f32,
    y: f32,
    row: usize,
    canvas: Mat4,
) -> f32 {
    // Native170740: first button, optional second button, label y, label width.
    let (buttons, label_y, width) = match row {
        0 => ([7, 2], 0.0, 236.0),
        1 => ([4, -1], 48.0, 72.0),
        _ => unreachable!(),
    };
    let mut left = right - width;
    sprite(
        textures,
        pass,
        "help_manual",
        vec4(left, y, width, 48.0),
        vec4(0.0, label_y / 96.0, width / 320.0, 0.5),
        1.0,
        ColorBlendType::Layer1,
        canvas,
    );
    left += 2.0;
    for button in buttons {
        if button < 0 {
            continue;
        }
        left -= 48.0;
        sprite(
            textures,
            pass,
            "key",
            vec4(left, y, 48.0, 48.0),
            vec4(
                (button & 7) as f32 / 8.0,
                (button >> 3) as f32 / 2.0,
                0.125,
                0.5,
            ),
            1.0,
            ColorBlendType::Layer1,
            canvas,
        );
    }
    left - 16.0
}

/// Native860b4..860fc: close hint first, then page hint, with a shared slide.
pub fn render_hints(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    if model.phase() == ManualPhase::Closed {
        return;
    }
    let y = 1100.0 - 88.0 * model.progress();
    let right = render_hint_row(textures, pass, 1872.0, y, 1, canvas);
    render_hint_row(textures, pass, right, y, 0, canvas);
}

/// Native order: backdrop/bars, clipped pages, caption/close, controller hints.
pub fn render(
    model: &ManualModel,
    textures: &CharacterTextures,
    pass: &mut RenderPass,
    canvas: Mat4,
) {
    render_background(model, textures, pass, canvas);
    render_pages(model, textures, pass, canvas);
    render_foreground(model, textures, pass, canvas);
    render_hints(model, textures, pass, canvas);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{PreRenderContext, top_left_projection_matrix};
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_manual_controller_hints_and_identity_button_mapping() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x860b4, 0xf0000688),
            (0x860b8, 0xbd40c260),
            (0x860bc, 0xaa1303e0),
            (0x860c0, 0xbd4a2501),
            (0x860c4, 0xf0000688),
            (0x860c8, 0x1e210803),
            (0x860cc, 0x1e269001),
            (0x860d0, 0x1e282860),
            (0x860d4, 0x1e212808),
            (0x860d8, 0xbd49d100),
            (0x860dc, 0x4ea81d01),
            (0x860e0, 0x2a1403e1),
            (0x860e4, 0x320003e2),
            (0x860e8, 0x94000016),
            (0x860ec, 0x4ea81d01),
            (0x860f0, 0xaa1303e0),
            (0x860f4, 0x2a1403e1),
            (0x860f8, 0x2a1f03e2),
            (0x860fc, 0x94000011),
            (0x86180, 0x940102bc),
            (0x86184, 0xd0000757),
            (0x86188, 0x911d02f7),
            (0x8618c, 0x8b34d2f4),
            (0x86190, 0xbd400e80),
            (0x86194, 0x5e21d80c),
            (0x86198, 0x9102e2b5),
            (0x8619c, 0xc8dffea0),
            (0x861a0, 0xf00006a1),
            (0x861a4, 0x91287c21),
            (0x861a8, 0x1e2c392a),
            (0x861ac, 0x940247f5),
            (0x861b0, 0xaa0003e1),
            (0x861b4, 0x2a1f03e0),
            (0x861b8, 0xaa1f03e2),
            (0x861bc, 0x94012581),
            (0x861c0, 0xf0000688),
            (0x861c4, 0xbd45850b),
            (0x861c8, 0x1e2b2909),
            (0x861cc, 0x1e2c2942),
            (0x861d0, 0x4eaa1d40),
            (0x861d4, 0x4ea81d01),
            (0x861d8, 0x910003e0),
            (0x861dc, 0x4ea91d23),
            (0x861e0, 0x940102b4),
            (0x861e4, 0xbd400a80),
            (0x861e8, 0x5e21d801),
            (0x861ec, 0xf0000688),
            (0x861f0, 0xbd481903),
            (0x861f4, 0xf0000688),
            (0x861f8, 0xbd444500),
            (0x861fc, 0x1e201982),
            (0x86200, 0x1e2703e0),
            (0x86204, 0x1e2c100c),
            (0x86208, 0x1e202842),
            (0x8620c, 0x1e231821),
            (0x86210, 0x1e2c2823),
            (0x86214, 0x910003e0),
            (0x86218, 0x940102ba),
            (0x8621c, 0x910003e0),
            (0x86220, 0x2a1303e1),
            (0x86224, 0x940102c3),
            (0x86228, 0x910003e0),
            (0x8622c, 0x940102f9),
            (0x86230, 0xc8dffea0),
            (0x86234, 0x1e201000),
            (0x86238, 0xd00006a1),
            (0x8623c, 0x9102fc21),
            (0x86240, 0x1e20294a),
            (0x86244, 0x940247cf),
            (0x86248, 0xaa0003e1),
            (0x8624c, 0x2a1f03e0),
            (0x86250, 0xaa1f03e2),
            (0x86254, 0x9401255b),
            (0x86258, 0xb9400280),
            (0x8625c, 0x97fe8431),
            (0x86260, 0x37f80360),
            (0x86264, 0xf0000688),
            (0x86268, 0xbd49c500),
            (0x8626c, 0x1e20294a),
            (0x86270, 0x1e2b2942),
            (0x86274, 0x4eaa1d40),
            (0x86278, 0x4ea81d01),
            (0x8627c, 0x4ea91d23),
            (0x86280, 0x2a0003f4),
            (0x86284, 0x910003e0),
            (0x86288, 0x9401028a),
            (0x8628c, 0x12000a88),
            (0x86290, 0x1e220100),
            (0x86294, 0x53037e88),
            (0x86298, 0x1e220101),
            (0x8629c, 0x1e281002),
            (0x862a0, 0x1e220800),
            (0x862a4, 0x1e2c0821),
            (0x862a8, 0x1e222802),
            (0x862ac, 0x1e2c2823),
            (0x862b0, 0x910003e0),
            (0x862b4, 0x94010293),
            (0x862b8, 0x910003e0),
            (0x862bc, 0x2a1303e1),
            (0x862c0, 0x9401029c),
            (0x862c4, 0x910003e0),
            (0x862c8, 0x940102d2),
            (0x862cc, 0x8b1612e8),
            (0x862d0, 0xb9400500),
            (0x862d4, 0x97fe8413),
            (0x862d8, 0x37f80360),
            (0x862dc, 0xf0000688),
            (0x862e0, 0xbd49c500),
            (0x862e4, 0x1e20294a),
            (0x862e8, 0x1e2b2942),
            (0x862ec, 0x4eaa1d40),
            (0x862f0, 0x4ea81d01),
            (0x862f4, 0x4ea91d23),
            (0x862f8, 0x2a0003f4),
            (0x862fc, 0x910003e0),
            (0x86300, 0x9401026c),
            (0x86304, 0x12000a88),
            (0x86308, 0x1e220100),
            (0x8630c, 0x53037e88),
            (0x86310, 0x1e220101),
            (0x86314, 0x1e281002),
            (0x86318, 0x1e220800),
            (0x8631c, 0x1e2c0821),
            (0x86320, 0x1e222802),
            (0x86324, 0x1e2c2823),
            (0x86328, 0x910003e0),
            (0x8632c, 0x94010275),
            (0x86330, 0x910003e0),
            (0x86334, 0x2a1303e1),
            (0x86338, 0x9401027e),
            (0x8633c, 0x910003e0),
            (0x86340, 0x940102b4),
            (0x86344, 0x1e361000),
            (0x86348, 0x1e202940),
            (0x27320, 0xf81e0ff3),
            (0x27324, 0xa9017bfd),
            (0x27328, 0x910043fd),
            (0x2732c, 0x2a0003f3),
            (0x27330, 0x940351b0),
            (0x27334, 0x360000c0),
            (0x27338, 0x7100127f),
            (0x2733c, 0x54000100),
            (0x27340, 0x71000e7f),
            (0x27344, 0x54000041),
            (0x27348, 0x321e03f3),
            (0x2734c, 0xa9417bfd),
            (0x27350, 0x2a1303e0),
            (0x27354, 0xf84207f3),
            (0x27358, 0xd65f03c0),
            (0x2735c, 0x320007f3),
            (0x27360, 0xa9417bfd),
            (0x27364, 0x2a1303e0),
            (0x27368, 0xf84207f3),
            (0xfb9f0, 0x2a1f03e0),
            (0xfb9f4, 0xd65f03c0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction at {address:x}"
            );
        }
        for (offset, expected) in [7_i32, 2, 0, 236, 4, -1, 48, 72].into_iter().enumerate() {
            let a = 0x170740 + 256 + offset * 4;
            assert_eq!(
                i32::from_le_bytes(exe[a..a + 4].try_into().unwrap()),
                expected
            );
        }
        for (address, expected) in [
            (0x159584, 48.0_f32),
            (0x159818, 96.0),
            (0x159444, 320.0),
            (0x1599c4, -48.0),
            (0x159a24, -88.0),
            (0x1599d0, 1872.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_manual_backdrop_bars_caption_and_close() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x85ad4, 0x4eab1d60),
            (0x85ad8, 0x4eab1d61),
            (0x85adc, 0x4ead1da2),
            (0x85ae0, 0x4eac1d83),
            (0x85ae4, 0x910303e0),
            (0x85ae8, 0x94010472),
            (0x85aec, 0x4eab1d60),
            (0x85af0, 0x4eab1d61),
            (0x85af4, 0x4ea91d22),
            (0x85af8, 0x4ea91d23),
            (0x85afc, 0x910303e0),
            (0x85b00, 0x94010480),
            (0x85b04, 0xbd40c263),
            (0x85b08, 0x4ea91d20),
            (0x85b0c, 0x4ea91d21),
            (0x85b10, 0x4ea91d22),
            (0x85b14, 0x97ff430f),
            (0x85b18, 0x910103e0),
            (0x85b1c, 0x2d0807e0),
            (0x85b20, 0x2d090fe2),
            (0x85b24, 0x97ff4347),
            (0x85b28, 0x2a0003e1),
            (0x85b2c, 0x910303e0),
            (0x85b30, 0x94010480),
            (0x85b34, 0x910303e0),
            (0x85b38, 0x940104b6),
            (0x85b3c, 0x900006a8),
            (0x85b40, 0xbd40c260),
            (0x85b44, 0x320007e0),
            (0x85b48, 0xbd437d08),
            (0x85b4c, 0x900006a8),
            (0x85b50, 0xbd4af901),
            (0x85b54, 0x1e280800),
            (0x85b58, 0x1e21280a),
            (0x85b5c, 0x320007f4),
            (0x85b60, 0x94012708),
            (0x85b84, 0x1e282943),
            (0x85b88, 0x4eab1d60),
            (0x85b8c, 0x4eaa1d41),
            (0x85b90, 0x4ead1da2),
            (0x85b94, 0x910303e0),
            (0x85b98, 0x4ead1daa),
            (0x85b9c, 0x94010445),
            (0x85ba0, 0x1e2c100e),
            (0x85ba4, 0x4eab1d60),
            (0x85ba8, 0x4eab1d61),
            (0x85bac, 0x4ea91d22),
            (0x85bb0, 0x910303e0),
            (0x85bb4, 0x4eae1dc3),
            (0x85bb8, 0x94010452),
            (0x85bbc, 0x1e2a100d),
            (0x85bc0, 0x4ea91d20),
            (0x85bc4, 0x4ea91d21),
            (0x85bc8, 0x4ea91d22),
            (0x85bcc, 0x4ead1da3),
            (0x85bd0, 0x97ff42e0),
            (0x85bd4, 0x910103e0),
            (0x85bd8, 0x2d0807e0),
            (0x85bdc, 0x2d090fe2),
            (0x85be0, 0x97ff4318),
            (0x85be4, 0x2a0003e1),
            (0x85be8, 0x910303e0),
            (0x85bec, 0x94010451),
            (0x85bf0, 0x910303e0),
            (0x85bf4, 0x94010487),
            (0x85bf8, 0xbd40c260),
            (0x85bfc, 0x1e280803),
            (0x85c00, 0x4eab1d60),
            (0x85c04, 0x1e233981),
            (0x85c08, 0x1e282823),
            (0x85c0c, 0x4eaa1d42),
            (0x85c10, 0x910303e0),
            (0x85c14, 0x94010427),
            (0x85c18, 0x4eab1d60),
            (0x85c1c, 0x4eae1dc1),
            (0x85c20, 0x4ea91d22),
            (0x85c24, 0x4ea91d23),
            (0x85c28, 0x910303e0),
            (0x85c2c, 0x94010435),
            (0x85c30, 0x4ea91d20),
            (0x85c34, 0x4ea91d21),
            (0x85c38, 0x4ea91d22),
            (0x85c3c, 0x4ead1da3),
            (0x85c40, 0x97ff42c4),
            (0x85c44, 0x910103e0),
            (0x85c48, 0x2d0807e0),
            (0x85c4c, 0x2d090fe2),
            (0x85c50, 0x97ff42fc),
            (0x85c54, 0x2a0003e1),
            (0x85c58, 0x910303e0),
            (0x85c5c, 0x94010435),
            (0x85c60, 0x910303e0),
            (0x85c64, 0x9401046b),
            (0x85f94, 0x900006a8),
            (0x85f98, 0xbd47b900),
            (0x85f9c, 0x900006a8),
            (0x85fa0, 0x1e202963),
            (0x85fa4, 0xbd47ed00),
            (0x85fa8, 0x900006a8),
            (0x85fac, 0xbd49d502),
            (0x85fb0, 0x4eab1d61),
            (0x85fb4, 0x910303e0),
            (0x85fb8, 0x9401033e),
            (0x85fbc, 0x4eaa1d40),
            (0x85fc0, 0x4eaa1d41),
            (0x85fc4, 0x4ea91d22),
            (0x85fc8, 0x4ea91d23),
            (0x85fcc, 0x910303e0),
            (0x85fd0, 0x9401034c),
            (0x85fd4, 0x4ea91d20),
            (0x85fd8, 0x4ea91d21),
            (0x85fdc, 0x4ea91d22),
            (0x85fe0, 0x4ea91d23),
            (0x85fe4, 0x97ff41db),
            (0x85fe8, 0x910103e0),
            (0x85fec, 0x2d0807e0),
            (0x85ff0, 0x2d090fe2),
            (0x85ff4, 0x97ff4213),
            (0x85ff8, 0x2a0003e1),
            (0x85ffc, 0x910303e0),
            (0x86000, 0x9401034c),
            (0x86004, 0x910303e0),
            (0x86008, 0x94010382),
            (0x8600c, 0xc8dffe80),
            (0x86010, 0xd00006a1),
            (0x86014, 0x9102e421),
            (0x86018, 0x9402485a),
            (0x8601c, 0xaa0003e1),
            (0x86020, 0x2a1f03e0),
            (0x86024, 0xaa1f03e2),
            (0x86028, 0x940125e6),
            (0x8602c, 0xf0000688),
            (0x86030, 0xbd469500),
            (0x86034, 0xf0000688),
            (0x86038, 0x1e202961),
            (0x8603c, 0xbd47ad00),
            (0x86040, 0xf0000688),
            (0x86044, 0x1e202823),
            (0x86048, 0xbd448100),
            (0x8604c, 0xf0000688),
            (0x86050, 0xbd471d02),
            (0x86054, 0x910303e0),
            (0x86058, 0x94010316),
            (0x8605c, 0x4eaa1d40),
            (0x86060, 0x4eaa1d41),
            (0x86064, 0x4ea91d22),
            (0x86068, 0x4ea91d23),
            (0x8606c, 0x910303e0),
            (0x86070, 0x94010324),
            (0x86074, 0x12800014),
            (0x86078, 0x910303e0),
            (0x8607c, 0x2a1403e1),
            (0x86080, 0x9401032c),
            (0x86084, 0x9105c260),
            (0x86088, 0x9400d506),
            (0x8608c, 0x4ea01c03),
            (0x86090, 0x4ea91d20),
            (0x86094, 0x4ea91d21),
            (0x86098, 0x4ea91d22),
            (0x8609c, 0x97ff41ad),
            (0x860a0, 0x910303e0),
            (0x860a4, 0x910103e1),
            (0x860a8, 0x2d0807e0),
            (0x860ac, 0x2d090fe2),
            (0x860b0, 0x94010398),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, value) in [
            (0x15937c, 160.0_f32),
            (0x159af8, -160.0),
            (0x1597b8, 128.0),
            (0x1597ec, 480.0),
            (0x1599d4, 1440.0),
            (0x159694, 32.0),
            (0x1597ac, 84.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_manual_page_transform_stencil_and_culling() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x85c68, 0xbd40c260),
            (0x85c6c, 0x1e2a0800),
            (0x85c70, 0x1e203940),
            (0x85c74, 0x4eab1d61),
            (0x85c78, 0x4eab1d62),
            (0x85c7c, 0x910003e8),
            (0x85c80, 0x940070ec),
            (0x85c84, 0x910103e8),
            (0x85c88, 0x910003e0),
            (0x85c8c, 0x910203e1),
            (0x85c90, 0x94006f40),
            (0x85c94, 0xa94523e9),
            (0x85c98, 0x910203e1),
            (0x85c9c, 0xa90923e9),
            (0x85ca0, 0x2a1f03e0),
            (0x85ca4, 0xa94423ea),
            (0x85ca8, 0x321a03e2),
            (0x85cac, 0xa90823ea),
            (0x85cb0, 0xa94723e9),
            (0x85cb4, 0xa90b23e9),
            (0x85cb8, 0xa94623ea),
            (0x85cbc, 0xa90a23ea),
            (0x85cc0, 0x940126e0),
            (0x85cf4, 0x32000be8),
            (0x85cf8, 0xb26007e9),
            (0x85cfc, 0x32003ff6),
            (0x85d00, 0xa90027e8),
            (0x85d04, 0x32001fe8),
            (0x85d08, 0x910003e0),
            (0x85d0c, 0x790023f6),
            (0x85d10, 0x39004be8),
            (0x85d14, 0x9401266f),
            (0x85d18, 0xd2e86448),
            (0x85d1c, 0xf90023e8),
            (0x85d20, 0xd2a89e08),
            (0x85d24, 0xf2e88ea8),
            (0x85d28, 0xb2503fe9),
            (0x85d2c, 0x52a88eaa),
            (0x85d30, 0xa90727e8),
            (0x85d34, 0xd2a89e08),
            (0x85d38, 0xf2e86448),
            (0x85d3c, 0xa904a3e9),
            (0x85d40, 0x32103fe8),
            (0x85d44, 0x910103e0),
            (0x85d48, 0x321a03e1),
            (0x85d4c, 0xb90067ea),
            (0x85d50, 0xf90037e9),
            (0x85d54, 0xb9005bff),
            (0x85d58, 0xf805c3e8),
            (0x85d5c, 0x940126d5),
            (0x85d60, 0x320003e0),
            (0x85d64, 0x321e03e1),
            (0x85d68, 0x2a1f03e2),
            (0x85d6c, 0x9401270d),
            (0x85d70, 0x910003e0),
            (0x85d74, 0x9401297f),
            (0x85d78, 0x910003e0),
            (0x85d7c, 0xa9007ff4),
            (0x85d80, 0x790023f6),
            (0x85d84, 0x39004bf5),
            (0x85d88, 0x94012652),
            (0x85d8c, 0x321f03e0),
            (0x85d90, 0x9401267c),
            (0x85de4, 0xaa1503e0),
            (0x85de8, 0x9400d5ae),
            (0x85dec, 0x1b187e88),
            (0x85df0, 0x1e220101),
            (0x85df4, 0x1e203820),
            (0x85df8, 0x1e28280e),
            (0x85dfc, 0x1e2a21c0),
            (0x85e00, 0x5400070a),
            (0x85e04, 0x1e2d29cf),
            (0x85e08, 0x1e2021e8),
            (0x85e0c, 0x540006a9),
            (0x85e10, 0xbc5cc3a1),
            (0x85e14, 0x1e211800),
            (0x85e18, 0x1e2c1001),
            (0x85e1c, 0x1e210800),
            (0x85e20, 0x1e20c000),
            (0x85e24, 0x1e203920),
            (0x85e28, 0x11000694),
            (0x85e2c, 0x910103e0),
            (0x85e30, 0xaa1603e1),
            (0x85e34, 0x2a1403e2),
            (0x85e38, 0x4eaa1d4b),
            (0x85e3c, 0x1e21480a),
            (0x85e40, 0x94025b70),
            (0x85e44, 0x9102e268),
            (0x85e48, 0xc8dffd00),
            (0x85e4c, 0x910103e1),
            (0x85e50, 0x940248cc),
            (0x85e54, 0xaa0003e1),
            (0x85e58, 0x2a1f03e0),
            (0x85e5c, 0xaa1f03e2),
            (0x85e60, 0x94012658),
            (0x85e64, 0xbc5c83a1),
            (0x85e68, 0x4eae1dc0),
            (0x85e6c, 0x4eaf1de2),
            (0x85e70, 0x4eac1d83),
            (0x85e74, 0x910303e0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        // Native clip quad embeds float162,1920,980 as immediate bit patterns.
        assert_eq!(f32::from_bits(0x43220000), 162.0);
        assert_eq!(f32::from_bits(0x44f00000), 1920.0);
        assert_eq!(f32::from_bits(0x44750000), 980.0);
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_manual_pages_match_supplied_artwork_and_native_clip() {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::{
            dynamic_buffer::DynamicBuffer,
            pipelines::PipelineStorage,
            resize::{SurfaceResizeSource, ViewportParams},
            shaders::types::{
                buffer::BytesAddress,
                texture::{
                    DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
                },
            },
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) = shin_tasks::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            }))
        .unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = std::sync::Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let mut screen = super::super::manual_screen::ManualScreen::new(server.clone());
        assert!(screen.model().is_none());
        let mut loading_input =
            enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
        loading_input[crate::app::AppAction::Cancel].is_clicked = true;
        screen.input(&loading_input);
        assert!(screen.take_sounds().is_empty());
        let textures = screen.wait_reference_assets().unwrap();
        assert_eq!(screen.model().unwrap().count(), 24);
        assert_eq!(screen.model().unwrap().progress(), 0.0);
        assert_eq!(
            screen
                .update(shin_core::time::Ticks::from_f32(10.0))
                .unwrap(),
            None
        );
        assert_eq!(screen.model().unwrap().phase(), ManualPhase::Active);
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Manual page test depth"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Manual page test target"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), format);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };

        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/manual.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
            let im = &atlas.textures[atlas.name_to_index[name]];
            let x = u * im.width() as f32 - 0.5;
            let y = v * im.height() as f32 - 0.5;
            let ix = x.floor();
            let iy = y.floor();
            let fx = x - ix;
            let fy = y - iy;
            std::array::from_fn(|channel| {
                let pixel = |dx: f32, dy: f32| {
                    im.get_pixel(
                        (ix + dx).clamp(0.0, im.width() as f32 - 1.0) as u32,
                        (iy + dy).clamp(0.0, im.height() as f32 - 1.0) as u32,
                    )[channel] as f32
                        / 255.0
                };
                (pixel(0.0, 0.0) * (1.0 - fx) + pixel(1.0, 0.0) * fx) * (1.0 - fy)
                    + (pixel(0.0, 1.0) * (1.0 - fx) + pixel(1.0, 1.0) * fx) * fy
            })
        };

        let mut checked = 0;
        let mut max_error = 0.0_f32;
        for case in 0..29 {
            let mut model = ManualModel::new(24).unwrap();
            let (scroll, progress) = if case < 24 {
                (case as f32 * 1500.0, 1.0)
            } else {
                match case {
                    24 => (750.0, 1.0),
                    25 => (1000.0, 1.0),
                    26 => (12300.0, 1.0),
                    27 => (0.0, 0.25),
                    _ => (0.0, 0.0),
                }
            };
            if progress == 1.0 {
                model.update(10.0);
                model.pointer_probe(vec2(0.0, 500.0));
                model.pointer_drag(-scroll);
            } else {
                model.update(progress * 10.0);
            }
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Native manual page clip",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                render_pages(&model, &textures, &mut pass, top_left_projection_matrix());
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let shift = 1920.0 * (1.0 - progress);
            for y in (1..1080).step_by(7) {
                for x in (1..1920).step_by(7) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut expected = [0.0_f32; 3];
                    if px >= shift && px < shift + 1920.0 && py >= 162.0 && py < 980.0 {
                        for page in 0..24 {
                            let offset = page as f32 * 1500.0 - scroll;
                            let left = 240.0 + offset + shift;
                            if px >= left && px < left + 1440.0 && py >= 166.0 && py < 976.0 {
                                let source = sample(
                                    &format!("{:03}", page + 1),
                                    (px - left) / 1440.0,
                                    (py - 166.0) / 810.0,
                                );
                                let alpha = ((1.0 - (offset / 1500.0 * 0.5).abs()).max(0.5)
                                    * 255.0)
                                    .round()
                                    / 255.0;
                                expected = std::array::from_fn(|c| {
                                    (source[c] * source[3] * alpha * 255.0).round()
                                });
                            }
                        }
                    }
                    for c in 0..3 {
                        let error = (pixels[(y * 1920 + x) * 4 + c] as f32 - expected[c]).abs();
                        max_error = max_error.max(error);
                        assert!(
                            error <= 1.1,
                            "case{case},({x},{y}),channel{c}:{} versus{}",
                            pixels[(y * 1920 + x) * 4 + c],
                            expected[c]
                        );
                    }
                    assert_eq!(pixels[(y * 1920 + x) * 4 + 3], 255);
                    checked += 1;
                }
            }
        }
        let mut shell_samples = 0;
        let mut shell_error = 0.0_f32;
        for (progress, flash_ticks) in [
            (0.0, None),
            (0.25, None),
            (0.5, None),
            (0.75, None),
            (1.0, None),
            (1.0, Some(1.0)),
            (1.0, Some(3.5)),
        ] {
            let mut model = ManualModel::new(24).unwrap();
            model.update(progress * 10.0);
            let flash = flash_ticks
                .map(|ticks| {
                    model.pointer_probe(vec2(1800.0, 80.0));
                    model.pointer_press();
                    model.update(ticks);
                    ticks * 0.2
                })
                .unwrap_or(0.0);
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Native manual shell",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                *screen.model_mut().unwrap() = model;
                screen.render(&mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let top = -160.0 + 160.0 * progress;
            for y in (1..1080).step_by(7) {
                for x in (1..1920).step_by(7) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut color = [0.0_f32; 3];
                    let mut draw = |name: &str,
                                    r: [f32; 4],
                                    uv: [f32; 4],
                                    alpha: f32,
                                    add: bool,
                                    flash: f32| {
                        if px < r[0] || px >= r[2] || py < r[1] || py >= r[3] {
                            return;
                        }
                        let source = sample(
                            name,
                            uv[0] + (uv[2] - uv[0]) * (px - r[0]) / (r[2] - r[0]),
                            uv[1] + (uv[3] - uv[1]) * (py - r[1]) / (r[3] - r[1]),
                        );
                        let a = source[3] * (alpha * 255.0).round() / 255.0;
                        for c in 0..3 {
                            let rgb = source[c] * (1.0 - flash) + flash;
                            color[c] = (rgb * a + color[c] * if add { 1.0 } else { 1.0 - a })
                                .clamp(0.0, 1.0);
                            color[c] = (color[c] * 255.0).round() / 255.0;
                        }
                    };
                    draw(
                        "bg",
                        [0.0, 0.0, 1920.0, 1080.0],
                        [0.0, 0.0, 1.0, 1.0],
                        progress,
                        false,
                        0.0,
                    );
                    draw(
                        "obi",
                        [0.0, top, 1920.0, top + 160.0],
                        [0.0, 0.0, 1.0, 0.5],
                        0.25,
                        true,
                        0.0,
                    );
                    let bottom = 1080.0 - 160.0 * progress;
                    draw(
                        "obi",
                        [0.0, bottom, 1920.0, bottom + 160.0],
                        [0.0, 0.5, 1.0, 1.0],
                        0.25,
                        true,
                        0.0,
                    );
                    let shift = 1920.0 * (1.0 - progress);
                    for page in 0..2 {
                        let left = 240.0 + page as f32 * 1500.0 + shift;
                        draw(
                            &format!("{:03}", page + 1),
                            [left, 166.0, left + 1440.0, 976.0],
                            [0.0, 0.0, 1.0, 1.0],
                            if page == 0 { 1.0 } else { 0.5 },
                            false,
                            0.0,
                        );
                    }
                    draw(
                        "caption",
                        [480.0, top, 1440.0, top + 128.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        false,
                        0.0,
                    );
                    draw(
                        "close",
                        [1764.0, top + 32.0, 1848.0, top + 116.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        false,
                        flash,
                    );
                    let hint_y = 1100.0 - 88.0 * progress;
                    // Independent native row endpoints and atlas coordinates.
                    draw(
                        "help_manual",
                        [1800.0, hint_y, 1872.0, hint_y + 48.0],
                        [0.0, 0.5, 72.0 / 320.0, 1.0],
                        1.0,
                        false,
                        0.0,
                    );
                    draw(
                        "key",
                        [1754.0, hint_y, 1802.0, hint_y + 48.0],
                        [0.5, 0.0, 0.625, 0.5],
                        1.0,
                        false,
                        0.0,
                    );
                    draw(
                        "help_manual",
                        [1502.0, hint_y, 1738.0, hint_y + 48.0],
                        [0.0, 0.0, 236.0 / 320.0, 0.5],
                        1.0,
                        false,
                        0.0,
                    );
                    draw(
                        "key",
                        [1456.0, hint_y, 1504.0, hint_y + 48.0],
                        [0.875, 0.0, 1.0, 0.5],
                        1.0,
                        false,
                        0.0,
                    );
                    draw(
                        "key",
                        [1408.0, hint_y, 1456.0, hint_y + 48.0],
                        [0.25, 0.0, 0.375, 0.5],
                        1.0,
                        false,
                        0.0,
                    );
                    for c in 0..3 {
                        let error =
                            (pixels[(y * 1920 + x) * 4 + c] as f32 - color[c] * 255.0).abs();
                        shell_error = shell_error.max(error);
                        assert!(
                            error <= 2.1,
                            "manual shell p{progress},flash{flash},({x},{y}),channel{c}:error{error}"
                        );
                    }
                    assert_eq!(pixels[(y * 1920 + x) * 4 + 3], 255);
                    shell_samples += 1;
                }
            }
        }
        eprintln!(
            "Manual shell: {shell_samples} decoded/composited samples,7 cases,max RGB error{shell_error}"
        );
        *screen.model_mut().unwrap() = ManualModel::new(24).unwrap();
        screen
            .update(shin_core::time::Ticks::from_f32(10.0))
            .unwrap();
        // Native input delivery keeps close-button capture despite unrelated Up
        // coordinates, and consumes simultaneous controller Cancel each frame.
        use super::super::input_area::{PointerEvent, PointerFrame, PointerKind};
        for (number, time_seconds, events) in [
            (
                0,
                0.0,
                vec![PointerEvent {
                    kind: PointerKind::Down,
                    id: 1,
                    position: glam::ivec2(1800, 80),
                }],
            ),
            (1, 0.15, vec![]),
            (
                2,
                0.16,
                vec![PointerEvent {
                    kind: PointerKind::Up,
                    id: 1,
                    position: glam::ivec2(0, 500),
                }],
            ),
        ] {
            screen.input_frame(
                &PointerFrame {
                    number,
                    time_seconds,
                    events: &events,
                },
                &loading_input,
            );
        }
        assert_eq!(screen.take_sounds(), ["decide"]);
        assert_eq!(
            screen
                .update(shin_core::time::Ticks::from_f32(7.0))
                .unwrap(),
            None
        );
        assert_eq!(
            screen.update(shin_core::time::Ticks::ZERO).unwrap(),
            Some(0)
        );
        assert_eq!(screen.model().unwrap().progress(), 1.0);
        assert_eq!(
            screen
                .update(shin_core::time::Ticks::from_f32(5.0))
                .unwrap(),
            None
        );
        assert_eq!(screen.model().unwrap().progress(), 0.5);
        assert_eq!(
            screen
                .update(shin_core::time::Ticks::from_f32(5.0))
                .unwrap(),
            Some(1)
        );
        assert_eq!(screen.update(shin_core::time::Ticks::ZERO).unwrap(), None);
        assert_eq!(
            screen
                .update(shin_core::time::Ticks::from_f32(100.0))
                .unwrap(),
            None
        );
        super::super::modal_backdrop::assert_reference_pixels(&mut context, &target);
        assert!(checked > 1_000_000);
        eprintln!(
            "Manual pages: {checked} decoded/background samples,29 cases,max RGB error{max_error}"
        );
    }
}
