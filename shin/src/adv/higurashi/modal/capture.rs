//! Updated150b8 program23: fixed half-authored-size, two horizontal passes.
use crate::render::PreRenderContext;
use glam::{Mat4, Vec2, Vec4};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

pub const EXTENT: (u32, u32) = (960, 540);

pub struct Packet {
    pub projection: Mat4,
    pub distances: [Vec4; 2],
    pub color: Vec4,
    pub weights: Vec4,
    pub vertices: [PosTexVertex; 4],
}

pub fn packets() -> [Packet; 2] {
    std::array::from_fn(|pass| Packet {
        projection: Mat4::from_cols_array(&[
            f32::from_bits(0x3a888889),
            0.0,
            0.0,
            0.0,
            0.0,
            f32::from_bits(0xbaf2b9d6),
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
        ]),
        distances: if pass == 0 {
            [
                Vec4::from_array([0x3a888889, 0, 0x3b088889, 0].map(f32::from_bits)),
                Vec4::from_array([0x3b4ccccd, 0, 0, 0].map(f32::from_bits)),
            ]
        } else {
            [
                Vec4::from_array([0x3b088889, 0, 0x3b888889, 0].map(f32::from_bits)),
                Vec4::from_array([0x3bcccccd, 0, 0, 0].map(f32::from_bits)),
            ]
        },
        color: Vec4::ONE,
        weights: Vec4::from_array(
            [0x3e5d4adf, 0x3e434a38, 0x3e06387e, 0x3d8fafb1].map(f32::from_bits),
        ),
        vertices: build_quad_vertices(|uv| PosTexVertex {
            position: uv * Vec2::new(1920.0, 1080.0),
            texture_position: uv,
        }),
    })
}

impl Packet {
    pub fn render(&self, pass: &mut RenderPass, source: TextureSource) {
        let mut projection = self.projection;
        // Native OpenGL clip depth -> WGPU; all native XY words stay intact.
        projection.z_axis.z *= 0.5;
        projection.w_axis.z = 0.5;
        pass.run(RenderRequestBuilder::new().build(
            RenderProgramWithArguments::HigurashiBlur {
                vertices: VertexSource::VertexData {
                    vertices: &self.vertices,
                },
                texture: source,
                transform: projection,
                distances: self.distances,
                color: self.color,
                weights: self.weights,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}

/// Consume the already prepared ScreenLayer cache. No scene redraw, message,
/// overlay or CPU readback is part of native150b8.
pub fn capture(context: &mut PreRenderContext, source: &RenderTexture) -> RenderTexture {
    capture_with_extent(context, source, EXTENT)
}
/// Physical SDK dimension service. Normal updated getters supply960x540;
/// component reference profiles explicitly override allocation dimensions.
pub(crate) fn capture_with_extent(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    extent: (u32, u32),
) -> RenderTexture {
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        extent.0, extent.1,
    )));
    let mut first = RenderTexture::new(
        context.device.clone(),
        resize.handle(),
        "Higurashi modal blur first".into(),
    );
    let mut second = RenderTexture::new(
        context.device.clone(),
        resize.handle(),
        "Higurashi modal blur second".into(),
    );
    let [a, b] = packets();
    {
        let mut pass = context.begin_pass(
            first.as_texture_target(),
            None,
            "Higurashi modal first pass",
        );
        a.render(&mut pass, source.as_texture_source());
    }
    {
        let mut pass = context.begin_pass(
            second.as_texture_target(),
            None,
            "Higurashi modal second pass",
        );
        b.render(&mut pass, first.as_texture_source());
    }
    second
}

#[cfg(test)]
mod tests;
