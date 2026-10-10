//! Native shared capture62ea0/63180: floating premultiplied fade and retirement.
use crate::render::top_left_projection_matrix;
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};
pub(super) struct CapturedScene {
    texture: Option<RenderTexture>,
}
impl CapturedScene {
    pub fn new(texture: RenderTexture) -> Self {
        Self {
            texture: Some(texture),
        }
    }
    pub fn retire(&mut self) {
        self.texture = None;
    }
    #[cfg(test)]
    pub fn is_owned(&self) -> bool {
        self.texture.is_some()
    }
    pub fn render(&self, pass: &mut RenderPass, opacity: f32) {
        if opacity <= 0.0 {
            return;
        }
        let texture = self
            .texture
            .as_ref()
            .expect("Visible retired scene capture");
        //62ea0 clears the mask;63180 blend5 consumes premultiplied float RGBA.
        //6320c..63214 uses56730, preserving opacity without byte quantization.
        let vertices = build_quad_vertices(|uv| PosTexVertex {
            position: uv * glam::vec2(1920.0, 1080.0),
            texture_position: uv,
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::LayerPremultiplied1)
                .build(
                    RenderProgramWithArguments::Layer {
                        output_kind: LayerShaderOutputKind::Layer,
                        fragment_shader: LayerFragmentShader::Default,
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: texture.as_texture_source(),
                        transform: top_left_projection_matrix(),
                        color_multiplier: FloatColor4::from_rgba(
                            opacity, opacity, opacity, opacity,
                        ),
                        fragment_shader_param: glam::Vec4::ZERO,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_shared_capture_float_color_signed_clock_and_music_resource_retirement() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x331d0, 0x94003618),
        (0x331f0, 0x94038fe4),
        (0x3328c, 0x91167908),
        (0x332c4, 0x940035db),
        (0x332e4, 0x9401b477),
        (0x32734, 0x9400c177),
        (0x32738, 0x91008288),
        (0x32754, 0xd63f0100),
        (0x32824, 0x7100081f),
        (0x32830, 0x321e0fe1),
        (0x32834, 0x9400c19b),
        (0x32844, 0x321e0fe1),
        (0x32848, 0x940004fa),
        (0x3285c, 0x9400c1ad),
        (0x32868, 0x97fffb06),
        (0x3286c, 0x12800008),
        (0x3287c, 0x9400c1b1),
        (0x62ed4, 0xc85ffd00),
        (0x62ef4, 0x39000268),
        (0x62f4c, 0x1e202840),
        (0x62f54, 0xbd000400),
        (0x62f5c, 0xbd000401),
        (0x62f94, 0x14000003),
        (0x631b0, 0x528000a0),
        (0x6320c, 0x1e211800),
        (0x63210, 0x1e203900),
        (0x63214, 0x97ffcd47),
        (0x56730, 0x4ea01c01),
        (0x56734, 0x4ea01c02),
        (0x56738, 0x4ea01c03),
        (0x62d44, 0xc85ffd00),
        (0x62d5c, 0xd63f0100),
    ] {
        assert_eq!(word(a), expected, "Native shared capture{a:x}");
    }
    let name = b"/default.msk\0";
    assert_eq!(&exe[0x15e59e + 256..0x15e59e + 256 + name.len()], name);
}
