//! Updated97020/9742c BGM name and bouncing note, owned by ADV+278.
use crate::{
    adv::higurashi::{
        audio::{HigurashiAudio, MusicEvent},
        browser_navigation::labels::{Glyphs, Layout},
        ui_state::MenuIndicator,
    },
    asset::font::GpuFontLazy,
    layer::render_params::multiply_transforms as multiply,
};
use anyhow::Result;
use glam::{Mat4, Vec2, vec2, vec3, vec4};
use shin_core::{format::save::higurashi::SaveData, layout::LayoutParams, time::Ticks};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct MusicTitle {
    title: Option<Glyphs>,
    note: Option<Glyphs>,
}
fn layout(font: impl shin_core::layout::font::FontMetrics + Clone, text: &str) -> Result<Layout> {
    Layout::from_script(
        font,
        text,
        LayoutParams {
            layout_width: 1920.0,
            text_size: 36.0,
            perform_soft_breaks: false,
            ..Default::default()
        },
    )
}
fn start(clock: &mut MenuIndicator, note_height: f32) {
    clock.indicator.transition.fast_forward_to(0.0);
    for (value, time, flags) in [
        (note_height + 4.0, 2.0, 0x40),
        (0.0, 180.0, 0x280),
        (0.0, 2.0, 0x40),
    ] {
        clock
            .indicator
            .transition
            .control(value, Ticks::from_f32(time), flags)
            .expect("Native music title tween");
    }
    clock.elapsed = 0.0;
}
fn note_pose(clock: &MenuIndicator) -> (Vec2, f32) {
    let sine = (clock.elapsed / 40.0 * f32::from_bits(0x40490fd0)).sin();
    if sine < 0.0 {
        (
            vec2(1.0 + sine * -0.4, 1.0 + sine * 0.3),
            clock.indicator.transition.value(),
        )
    } else {
        (Vec2::ONE, clock.indicator.transition.value() - 8.0 * sine)
    }
}
fn transform(size: Vec2, position: Vec2, origin: Vec2, scale: Vec2) -> Mat4 {
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
    multiply(
        projection,
        multiply(
            Mat4::from_translation(position.extend(0.0)),
            multiply(
                Mat4::from_scale(scale.extend(0.0)),
                Mat4::from_translation(vec3(-size.x * origin.x, -size.y * origin.y, 0.0)),
            ),
        ),
    )
}
impl MusicTitle {
    pub fn consume(
        &mut self,
        audio: &mut HigurashiAudio,
        clock: &mut MenuIndicator,
        saved: &mut SaveData,
        font: &Arc<GpuFontLazy>,
        context: &crate::asset::system::AssetLoadContext,
    ) -> Result<()> {
        while let Some(event) = audio.pop_music_event() {
            match event {
                MusicEvent::Started(info) => {
                    // Preserve the global gallery publication when consuming
                    // the same metadata that starts the ADV-local label.
                    for id in [info.id, i32::from(info.linked_id)]
                        .into_iter()
                        .filter(|id| *id >= 0)
                    {
                        let words = &mut saved.save_vectors.unlocked_bgms;
                        let index = id as usize / 32;
                        if index >= words.len() {
                            words.resize((index + 1).next_multiple_of(256), 0);
                        }
                        words[index] |= 1 << (id as usize % 32);
                    }
                    self.show(
                        &context.wgpu_device,
                        &context.wgpu_queue,
                        font.clone(),
                        &info.display_name,
                        clock,
                    )?;
                }
                MusicEvent::Stopped => clock.stop(),
            }
        }
        Ok(())
    }
    fn show(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font: Arc<GpuFontLazy>,
        text: &str,
        clock: &mut MenuIndicator,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let title = layout(font.clone(), text)?;
        let note = layout(font.clone(), "♪")?;
        let height = note.size.y;
        self.title = Some(Glyphs::from_layout(device, queue, font.clone(), title));
        self.note = Some(Glyphs::from_layout(device, queue, font, note));
        start(clock, height);
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass, clock: &MenuIndicator, enabled: bool) {
        if !enabled || clock.indicator.opacity <= 0.0 || clock.indicator.transition.is_idle() {
            return;
        }
        let (Some(title), Some(note)) = (&self.title, &self.note) else {
            return;
        };
        let slide = clock.indicator.transition.value();
        let (scale, y) = note_pose(clock);
        for (glyphs, position, origin, scale) in [
            (
                title,
                vec2(note.layout.size.x + 4.0, slide),
                vec2(0.0, 1.0),
                Vec2::ONE,
            ),
            (
                note,
                vec2(note.layout.size.x * 0.5 + 4.0, y),
                vec2(0.5, 1.0),
                scale,
            ),
        ] {
            glyphs.render_at(
                pass,
                transform(glyphs.layout.size, position, origin, scale),
                vec4(0.75, 1.0, 1.0, clock.indicator.opacity),
                vec4(0.0, 0.0, 0.0, clock.indicator.opacity),
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            );
        }
    }
}

#[cfg(test)]
mod tests;
