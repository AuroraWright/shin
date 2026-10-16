//! Ger13800 full-resolution horizontal/vertical blur and11D20 composition.
//! The later shader preserves native explicit vertex offsets and FMA grouping.
use crate::render::PreRenderContext;
use glam::{Mat4, Vec2, Vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};
pub struct BlurPacket {
    pub projection: [f32; 16],
    pub distances: [f32; 8],
    pub weights: [f32; 4],
    pub vertices: [f32; 16],
}
pub fn projection() -> [f32; 16] {
    [
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -0.5,
        0.,
        -1.,
        1.,
        0.5,
        1.,
    ]
}
pub fn blur_packets(width: u32, height: u32) -> [BlurPacket; 2] {
    std::array::from_fn(|pass| {
        let d = if pass == 0 { width } else { height } as f32;
        let mut distances = [0.; 8];
        for (i, v) in [0.75, 2.25, 3.75, 5.25].into_iter().enumerate() {
            distances[2 * i + pass] = v / d;
        }
        BlurPacket {
            projection: projection(),
            distances,
            weights: [0x3eceed6b, 0x3ea127be, 0x3e437dbe, 0x3db8afdf].map(f32::from_bits),
            vertices: [
                0., 0., 0., 0., 1920., 0., 1., 0., 0., 1080., 0., 1., 1920., 1080., 1., 1.,
            ],
        }
    })
}
pub struct BackdropPlan {
    pub render_root: bool,
    pub draw: bool,
    pub color: u32,
    pub blend: u32,
}
pub fn backdrop_plan(has_snapshot: bool, fade: f32) -> BackdropPlan {
    let p = if has_snapshot { fade } else { 0. };
    let grey = ((p * 0.5).clamp(0., 1.) * 255.) as u32;
    let alpha = (p.clamp(0., 1.) * 255.) as u32;
    BackdropPlan {
        render_root: p < 1.,
        draw: p > 0.,
        color: (alpha << 24) | (grey << 16) | (grey << 8) | grey,
        blend: if p < 1. { 5 } else { 1 },
    }
}
pub struct Backdrop {
    source: RenderTexture,
}
impl Backdrop {
    pub fn capture(ctx: &mut PreRenderContext, mut source: RenderTexture) -> Self {
        let mut scratch = ctx.new_render_texture("Ger modal blur scratch".into());
        let (w, h) = source.dimensions();
        for packet in blur_packets(w, h) {
            let vertices: [PosTexVertex; 4] = std::array::from_fn(|i| PosTexVertex {
                position: Vec2::new(packet.vertices[i * 4], packet.vertices[i * 4 + 1]),
                texture_position: Vec2::new(packet.vertices[i * 4 + 2], packet.vertices[i * 4 + 3]),
            });
            {
                let mut pass = ctx.begin_pass(scratch.as_texture_target(), None, "Ger modal blur");
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .build(
                            RenderProgramWithArguments::Gerokasu2Blur {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                texture: source.as_texture_source(),
                                transform: Mat4::from_cols_array(&packet.projection),
                                distances: [
                                    Vec4::from_slice(&packet.distances[..4]),
                                    Vec4::from_slice(&packet.distances[4..]),
                                ],
                                weights: Vec4::from_array(packet.weights),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
            std::mem::swap(&mut source, &mut scratch);
        }
        Self { source }
    }
    pub fn render(&self, pass: &mut RenderPass, fade: f32) {
        let plan = backdrop_plan(true, fade);
        if !plan.draw {
            return;
        }
        let bytes = plan.color.to_le_bytes();
        let color = UnormColor::from_rgba(bytes[2], bytes[1], bytes[0], bytes[3]);
        let mut projection = Mat4::from_cols_array(&projection());
        projection.z_axis.z = -0.5;
        projection.w_axis.z = 0.5;
        QuadVertices::new()
            .with_box(0., 0., 1920., 1080.)
            .with_tex_box(0., 0., 1., 1.)
            .with_color(color)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(if plan.blend == 5 {
                    ColorBlendType::LayerPremultiplied1
                } else {
                    ColorBlendType::Opaque
                }),
                self.source.as_texture_source(),
                projection,
            );
    }
}
