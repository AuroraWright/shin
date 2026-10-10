use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};

#[derive(Debug, Clone)]
pub struct WhirlWiperImpl {
    direction: i32,
    aspect: f32,
}

impl AdvUpdatable for WhirlWiperImpl {
    fn update(&mut self, _context: &AdvUpdateContext) {}
}

impl TimedWiper for WhirlWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.direction = words[0] as i32;
        self.aspect = f32::from_bits(words[1]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.direction == other.direction
            && super::timed::batch_float_matches(self.aspect, other.aspect)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    ) {
        let transform = normalized_projection_matrix();

        let vertices = &build_quad_vertices(|t| PosTexVertex {
            position: t,
            texture_position: t,
        });

        pass.run(
            render_request_builder
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::WiperWhirl {
                        vertices: VertexSource::VertexData { vertices },
                        texture_source,
                        texture_target,
                        transform,
                        effect: glam::Vec4::new(
                            progress,
                            if self.direction == 1 { 1.0 } else { -1.0 },
                            1.0,
                            if self.aspect.abs() < f32::EPSILON {
                                1.0
                            } else {
                                1.0 / self.aspect
                            },
                        ),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

pub type WhirlWiper = TimedWiperWrapper<WhirlWiperImpl>;

impl WhirlWiper {
    pub fn new(duration: Ticks, direction: i32, aspect: f32) -> Self {
        Self::from_inner(WhirlWiperImpl { direction, aspect }, duration)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_whirl_factory_and_shader_match_native() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 8]) * 4, 0x21d4c);
        for (offset, expected) in [
            (0xb8, 0x5080000000870303u64), // UV radius sqrt
            (0xe8, 0x1e241fb53d270304),    //10pi
            (0x158, 0x1e23e99999a70402),   //.3 pulse
            (0x170, 0x1e240c90fdb70202),   //TAU
            (0x1d8, 0xd832008040a70906),   //old scene sample
            (0x1e8, 0xd83200a020a70900),   //new scene sample
        ] {
            let a = 0x21b9f0 + 256 + offset;
            assert_eq!(
                u64::from_le_bytes(exe[a..a + 8].try_into().unwrap()),
                expected
            );
        }
    }
}
