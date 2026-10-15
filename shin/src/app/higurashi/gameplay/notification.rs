//! Updated55e10/55fd4/56040 scenario/autosave title notification.
use crate::{
    adv::higurashi::{
        browser_navigation::labels::{Glyphs, Layout},
        ui_state::Indicator,
    },
    asset::font::GpuFontLazy,
    layer::render_params::multiply_transforms as multiply,
};
use anyhow::Result;
use glam::{Mat4, vec3, vec4};
use shin_core::{layout::LayoutParams, time::Ticks};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use std::sync::Arc;

pub(super) struct Notification {
    pub text: String,
    pub glyphs: Option<Glyphs>,
}
impl Default for Notification {
    fn default() -> Self {
        Self {
            text: String::new(),
            glyphs: None,
        }
    }
}
pub(super) fn show(clock: &mut Indicator) {
    clock.transition.fast_forward_to(0.0);
    for (value, time, flags) in [
        (1.0, f32::from_bits(0x3cf5c28f), 0x40),
        (0.0, 240.0, 0x280),
        (0.0, f32::from_bits(0x3cf5c28f), 0x40),
    ] {
        clock
            .transition
            .control(value, Ticks::from_f32(time), flags)
            .expect("Native notification tween");
    }
}
impl Notification {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        text: &str,
        clock: &mut Indicator,
    ) -> Result<()> {
        let layout = Layout::from_script(
            font.clone(),
            text,
            LayoutParams {
                layout_width: 1916.0,
                text_size: 48.0,
                perform_soft_breaks: false,
                ..Default::default()
            },
        )?;
        self.glyphs = Some(Glyphs::from_layout(device, queue, font, layout));
        self.text = text.to_owned();
        show(clock);
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass, clock: &Indicator, enabled: bool) {
        if !enabled || clock.transition.is_idle() {
            return;
        }
        let Some(glyphs) = &self.glyphs else {
            return;
        };
        let alpha = clock.transition.value() * clock.opacity;
        let projection = Mat4::from_cols_array(&[
            2.0 / 1920.0,
            0.0,
            0.0,
            0.0,
            0.0,
            2.0 / -1080.0,
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
        ]);
        let transform = multiply(
            projection,
            multiply(
                Mat4::from_translation(vec3(4.0, 1076.0, 0.0)),
                multiply(
                    Mat4::from_scale(vec3(1.0, 1.0, 0.0)),
                    Mat4::from_translation(vec3(0.0, -glyphs.layout.size.y, 0.0)),
                ),
            ),
        );
        glyphs.render_at(
            pass,
            transform,
            vec4(1.0, 1.0, 1.0, alpha),
            vec4(0.0, 0.0, 0.0, alpha),
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }
}

#[cfg(test)]
mod tests;
