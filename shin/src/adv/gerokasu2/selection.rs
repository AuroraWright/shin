//! SELECT overlay owner and four-bit result history;kept in the Ger dialect.
pub(crate) mod draw;
use super::generic_text::Text;
use super::{
    message_ui::MessageResources,
    save_commands::{SaveCapture, SaveServices},
};
use crate::{adv::choice::ChoiceModel, asset::system::AssetServer};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec3};
use shin_core::{
    format::save::index_set::IndexSet, layout::gerokasu2::choice_layouter,
    primitives::color::FloatColor4, time::Ticks,
};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, quad_vertices::QuadVertices,
    render_pass::RenderPass,
};
use std::sync::Arc;
#[derive(Clone)]
pub struct SelectionRequest {
    pub base: i32,
    pub index: i32,
    pub mask: i32,
    pub variants: Vec<String>,
}
#[derive(Clone)]
pub struct SelectionHistory {
    pub seen: IndexSet,
    pub last: Vec<u8>,
}
impl Default for SelectionHistory {
    fn default() -> Self {
        Self {
            seen: IndexSet::new(),
            last: Vec::new(),
        }
    }
}
impl SelectionHistory {
    pub fn record(&mut self, request: &SelectionRequest, result: i32) -> Result<()> {
        ensure!(
            (0..12).contains(&result),
            "SELECT returned an invalid variant {result}"
        );
        if request.base >= 0 {
            self.seen
                .add((request.base as u32).wrapping_add(result as u32) as usize);
        }
        if request.index >= 0 {
            let index = request.index as usize;
            ensure!(
                index <= 65535,
                "SELECT history index exceeds native u16 input"
            );
            // Native byte table grows32 entries ahead.
            if self.last.len() <= index {
                self.last.resize((index + 1).next_multiple_of(32), 0);
            }
            self.last[index] = (result + 1) as u8;
        }
        Ok(())
    }
}
struct Label {
    text: Text,
    /// OriginalD4980..D49A8 adds both sides of the three-pixel border.
    layout_size: Vec2,
}
pub type SharedSelection = Arc<parking_lot::Mutex<SelectionOwner>>;
pub struct SelectionOwner {
    pub model: ChoiceModel,
    labels: Vec<Label>,
    resources: MessageResources,
    pub hidden: bool,
    pub opacity: f32,
}
impl SelectionOwner {
    pub fn prepare(
        assets: &AssetServer,
        resources: MessageResources,
        request: &SelectionRequest,
        _history: &SelectionHistory,
    ) -> Result<Self> {
        ensure!(
            request.variants.len() <= 12,
            "Ger SELECT exceeds twelve variants"
        );
        let seed = (request.mask as u32 & 0x1000 != 0).then(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u32
        });
        let mut model = ChoiceModel::gerokasu2(request.variants.len(), request.mask, seed);
        let mut labels = Vec::new();
        for &index in &model.indices {
            let (commands, lines, size) =
                choice_layouter(resources.medium.metrics()).parse(&request.variants[index]);
            // D484C supplies caller text_size48 for gradient interpolation.
            let text = Text::prepare(
                &assets.context().wgpu_device,
                &assets.context().wgpu_queue,
                &resources.medium,
                commands,
                lines,
                48.0,
            )?;
            labels.push(Label {
                text,
                layout_size: size + Vec2::splat(6.0),
            });
        }
        model.heights = labels
            .iter()
            .map(|l| l.layout_size.y + 16.0)
            .collect();
        Ok(Self {
            model,
            labels,
            resources,
            hidden: false,
            opacity: 1.0,
        })
    }
    pub fn update(&mut self, ticks: Ticks) {
        self.opacity = ticks
            .as_f32()
            .mul_add(if self.hidden { -0.1 } else { 0.1 }, self.opacity)
            .clamp(0.0, 1.0);
        if self.opacity >= 1.0 {
            self.model.update(ticks);
        }
    }
    pub fn ready(&self) -> bool {
        self.model.phase == crate::adv::choice::model::Phase::Waiting
            && !self.hidden
            && self.opacity >= 1.0
    }
    pub fn blocks_playback(&self) -> bool {
        self.model.phase != crate::adv::choice::model::Phase::Waiting || self.hidden
    }
    pub fn input(
        &mut self,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        vertical: bool,
        confirm: bool,
    ) {
        if self.ready() {
            self.model.input(up, down, left, right, vertical, confirm);
        }
    }
    pub fn pointer(&mut self, point: Vec2, held: bool) {
        if self.ready() {
            self.model.pointer_state(point, held);
        }
    }
    pub fn cancel_pointer(&mut self) {
        self.model.cancel_pointer();
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        parent: Mat4,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if kind != PassKind::Transparent || self.opacity <= 0.0 {
            return Ok(());
        }
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(stencil, true, false);
        let mut y = self.model.top();
        for ((label, row), &height) in self
            .labels
            .iter()
            .zip(&self.model.rows)
            .zip(&self.model.heights)
        {
            let transform =
                parent * Mat4::from_translation(vec3(row.x.value() - 960.0, y - 540.0, 0.0));
            let alpha = self.opacity * row.alpha.value();
            let background = |pass: &mut RenderPass, alpha: f32| {
                if alpha <= 0.0 {
                    return;
                }
                for (top, bottom, v0, v1) in [
                    (-8.0, 0.0, 0.0, 0.1),
                    (0.0, height, 0.1, 0.9),
                    (height, height + 8.0, 0.9, 1.0),
                ] {
                    QuadVertices::new()
                        .with_box(360.0, top, 1560.0, bottom)
                        .with_tex_box(0.0, v0, 1.0, v1)
                        .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm())
                        .render_sprite(
                            pass,
                            builder,
                            self.resources.textures.selection.as_source(),
                            transform,
                        );
                }
            };
            background(pass, alpha);
            label.text.render(
                pass,
                builder,
                transform
                    * Mat4::from_translation(vec3(
                        960.0 - label.layout_size.x * 0.5,
                        8.0,
                        0.0,
                    )),
                Vec4::new(1.0, 1.0, 1.0, alpha),
            )?;
            background(pass, alpha * (row.focus.value() * 0.5));
            background(pass, alpha * row.flash.value());
            y += height + 32.0;
        }
        Ok(())
    }
}
pub trait SelectionServices {
    fn begin_selection(&mut self, request: &SelectionRequest) -> Result<()>;
    fn selection_result(&mut self) -> Option<i32>;
    fn record_selection_history(&mut self, request: &SelectionRequest, result: i32) -> Result<()>;
    fn finish_selection(&mut self);
}
pub struct SelectionCommand {
    pub request: SelectionRequest,
    bound: bool,
    before: SaveCapture,
    after: SaveCapture,
    result: Option<i32>,
    recorded: bool,
    history_recorded: bool,
}
impl SelectionCommand {
    pub fn new(request: SelectionRequest) -> Self {
        Self {
            request,
            bound: false,
            before: Default::default(),
            after: Default::default(),
            result: None,
            recorded: false,
            history_recorded: false,
        }
    }
    pub fn bind(&mut self, services: &mut dyn SelectionServices) -> Result<()> {
        if !self.bound {
            services.begin_selection(&self.request)?;
            self.bound = true;
        }
        Ok(())
    }
    pub fn capture_before(&mut self, services: &mut dyn SaveServices) -> Result<()> {
        self.before.poll(services)
    }
    pub fn poll(&mut self, services: &mut dyn SelectionServices) -> Option<i32> {
        if self.result.is_none() {
            self.result = services.selection_result();
        }
        self.result
    }
    pub fn record_history(
        &mut self,
        services: &mut dyn SelectionServices,
        result: i32,
    ) -> Result<()> {
        if !self.history_recorded {
            services.record_selection_history(&self.request, result)?;
            self.history_recorded = true;
        }
        Ok(())
    }
    pub fn capture_after(&mut self, result: i32, save: &mut dyn SaveServices) -> Result<()> {
        if !self.recorded {
            save.save_session().record_selection(result);
            self.recorded = true;
        }
        self.after.poll(save)
    }
}
#[cfg(test)]
mod tests;
