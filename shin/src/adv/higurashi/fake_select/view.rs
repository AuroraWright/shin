use super::{LABELS, Model};
use crate::{
    adv::higurashi::{
        browser_navigation::labels::{Glyphs, Layout},
        input::OverlayInput,
        message_ui::HigurashiMessageTextures,
        touch::Overlay,
        voice_commands::VoiceCommands,
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::Result;
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{layout::LayoutParams, primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
use std::sync::Arc;

pub(crate) struct Screen {
    pub model: Model,
    pub properties: HigurashiProperties,
    glyphs: [Glyphs; 2],
    textures: Arc<HigurashiMessageTextures>,
}
impl Screen {
    pub fn new(
        server: &AssetServer,
        font: Arc<GpuFontLazy>,
        textures: Arc<HigurashiMessageTextures>,
        seeds: [u32; 4],
        raw_seed: u32,
    ) -> Result<Self> {
        let context = server.context();
        let mut glyphs = Vec::new();
        let mut heights = [0.; 2];
        for (index, label) in LABELS.iter().enumerate() {
            let layout = Layout::from_script(
                font.clone(),
                label,
                LayoutParams {
                    layout_width: 1000.,
                    text_size: 48.,
                    perform_soft_breaks: false,
                    ..Default::default()
                },
            )?;
            heights[index] = layout.size.y + 16.;
            glyphs.push(Glyphs::from_layout(
                &context.wgpu_device,
                &context.wgpu_queue,
                font.clone(),
                layout,
            ));
        }
        let mut properties = HigurashiProperties::from_seeds(seeds);
        properties.control(5, 1., Ticks::ZERO, 0)?;
        Ok(Self {
            model: Model::new(heights, raw_seed),
            properties,
            glyphs: glyphs.try_into().ok().unwrap(),
            textures,
        })
    }
    pub fn render(&mut self, pass: &mut RenderPass, stencil: u8, kind: PassKind) -> Result<()> {
        // Original57380 skips only the opaque pass; its own draw has no generic
        // base visibility test and advances the private jitter seed here.
        if kind != PassKind::Transparent {
            return Ok(());
        }
        let jitter = self.model.sample_jitter();
        let native = self
            .properties
            .composed_transform(&HigurashiTransform::default())
            .compute_native_final_transform();
        let parent = multiply(
            multiply(native, Mat4::from_translation(vec3(-640., -720., 0.))),
            Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
        );
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil, true, false);
        for (index, row) in self.model.rows.iter().enumerate() {
            let matrix = multiply(
                parent,
                Mat4::from_translation(vec3(jitter.x + row.x.value(), jitter.y + row.y, 0.)),
            );
            let alpha = row.alpha.value();
            for (top, bottom, uv_top, uv_bottom) in [
                (-8., 0., 0., 0.1),
                (0., row.height, 0.1, 0.9),
                (row.height, row.height + 8., 0.9, 1.),
            ] {
                let vertices = build_quad_vertices(|corner| PosColTexVertex {
                    position: vec3(
                        if corner.x == 0. { 360. } else { 1560. },
                        if corner.y == 0. { top } else { bottom },
                        0.,
                    ),
                    texture_position: vec2(
                        corner.x,
                        if corner.y == 0. { uv_top } else { uv_bottom },
                    ),
                    color: FloatColor4::from_rgba(1., 1., 1., alpha).into_unorm(),
                });
                pass.run(builder.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: self.textures.focused_selection.as_source(),
                        transform: HigurashiTransform::convert_clip_depth(matrix),
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            let glyph = &self.glyphs[index];
            let text = multiply(
                multiply(matrix, Mat4::from_translation(vec3(0., 8., 0.))),
                multiply(
                    Mat4::from_translation(vec3(960., 0., 0.)),
                    multiply(
                        Mat4::from_scale(vec3(1., 1., 0.)),
                        Mat4::from_translation(vec3(
                            -glyph.layout.size.x * 0.5,
                            -(glyph.layout.size.y * 0.),
                            0.,
                        )),
                    ),
                ),
            );
            let focus = row.focus.value();
            let fill = vec4(focus, focus, focus, alpha);
            glyph.render_gradient_at(
                pass,
                text,
                fill,
                fill,
                Vec4::new(0., 0., 0., alpha),
                builder,
            );
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct Shared(pub Arc<Mutex<Screen>>);
impl Shared {
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}
impl Overlay for Shared {
    fn identity(&self) -> usize {
        self.identity()
    }
    fn input_permissions(&self) -> OverlayInput {
        OverlayInput {
            menus: false,
            hide_window: false,
        }
    }
    fn probe(&mut self, _: Vec2) -> bool {
        false
    }
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
    fn advance_with_voice(&mut self, delta: Ticks, voice: &mut dyn VoiceCommands) {
        self.0.lock().model.advance(delta, voice);
    }
    fn fast_forward(&mut self) {
        self.0.lock().model.fast_forward();
    }
    fn take_cues(&mut self) -> Vec<&'static str> {
        self.0.lock().model.take_cues()
    }
    fn priority(&self) -> f32 {
        self.0.lock().properties.values()[5]
    }
    fn render(&self, pass: &mut RenderPass, stencil: u8, kind: PassKind) -> Result<()> {
        self.0.lock().render(pass, stencil, kind)
    }
}

#[cfg(test)]
mod tests;
