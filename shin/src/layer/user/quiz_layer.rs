use std::sync::{Arc, Mutex};

use crate::{
    asset::{system::AssetServer, texture_archive::TextureArchive},
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Vec2, vec4};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, gpu_texture::GpuTexture,
    quad_vertices::QuadVertices, render_pass::RenderPass, shaders::types::RenderClone,
};

#[derive(TextureArchive)]
pub struct QuizTextures {
    #[txa(name = "button")]
    pub button: GpuTexture,
    #[txa(name = "consult")]
    pub consult: GpuTexture,
}

/// Original QuizLayer constructor 0xc7740 filters seven buttons by mask, but
/// returns the unfiltered bit index (0xc8670), including consultation at bit 6.
#[derive(Debug, Clone)]
pub struct QuizModel {
    pub answers: Vec<usize>,
    pub selected: usize,
    pub result: Option<i32>,
    last_pointer: Option<Vec2>,
    focus: Vec<shin_core::time::Tweener>,
    consult_phase: f32,
    flash: Vec<shin_core::time::Tweener>,
    confirming: Option<usize>,
    sounds: Vec<&'static str>,
    pointer_area: Option<crate::adv::input_area::InputArea>,
    pointer_extent: (u16, u16),
    pointer_row: Option<usize>,
    pointer_consumed: bool,
}

impl QuizModel {
    pub fn new(mask: i32) -> Self {
        let answers: Vec<_> = (0..7).filter(|&i| mask as u32 & (1 << i) != 0).collect();
        let focus = (0..answers.len())
            .map(|row| shin_core::time::Tweener::new(if row == 0 { 1.0 } else { 0.0 }))
            .collect();
        Self {
            flash: vec![shin_core::time::Tweener::new(0.0); answers.len()],
            confirming: None,
            answers,
            focus,
            consult_phase: 0.0,
            selected: 0,
            result: None,
            last_pointer: None,
            sounds: Vec::new(),
            pointer_area: Some(crate::adv::input_area::InputArea::canvas()),
            pointer_extent: (1920, 1080),
            pointer_row: None,
            pointer_consumed: false,
        }
    }

    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub(crate) fn restore_completed_answer(&mut self, answer: i32) {
        // Native c86a0 restores the original answer index, not the packed row.
        // It sets the previous row's focus to .5 and the answered row to 1.
        self.confirming = None;
        if let Some(row) = self.answers.iter().position(|&id| id as i32 == answer) {
            self.focus[self.selected].fast_forward_to(0.5);
            self.selected = row;
            self.focus[row].fast_forward_to(1.0);
        }
        self.result = self.answers.get(self.selected).map(|&id| id as i32);
    }
    pub fn move_selection(&mut self, delta: i32) {
        if self.result.is_none() && self.confirming.is_none() && !self.answers.is_empty() {
            let selected =
                (self.selected as i32 + delta).rem_euclid(self.answers.len() as i32) as usize;
            if selected != self.selected {
                self.sounds.push("move");
            }
            self.select_row(selected);
        }
    }

    fn controller_input(&mut self, up: bool, down: bool, confirm: bool) {
        // Native c89b0 owns the complete input frame, including closing frames.
        // Confirm precedes both directions; repeated Up takes priority over Down.
        if self.controller_blocked() {
            return;
        }
        if confirm {
            self.confirm();
        } else if up {
            self.move_selection(-1);
        } else if down {
            self.move_selection(1);
        }
    }

    fn action_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        use crate::app::AppAction;
        self.controller_input(
            state[AppAction::Up].is_clicked_or_repeated,
            state[AppAction::AnyDown].is_clicked_or_repeated,
            state[AppAction::Enter].is_clicked,
        );
    }

    fn select_row(&mut self, row: usize) {
        if row == self.selected {
            return;
        }
        use shin_core::time::{Ticks, Tween};
        // Native c8b14/c8b40: outgoing focus speed .05, incoming .25.
        let old = &mut self.focus[self.selected];
        old.enqueue_now(0.0, Tween::linear(Ticks::from_f32(old.value() / 0.05)));
        let new = &mut self.focus[row];
        new.enqueue_now(
            1.0,
            Tween::linear(Ticks::from_f32((1.0 - new.value()) / 0.25)),
        );
        self.selected = row;
    }
    pub fn update(&mut self, delta: shin_core::time::Ticks) {
        // Native c813c..c815c checks the selected flash before advancing clocks.
        if let Some(row) = self.confirming {
            if self.flash[row].is_idle() {
                self.result = Some(self.answers[row] as i32);
                self.confirming = None;
            }
        }
        for (focus, flash) in self.focus.iter_mut().zip(&mut self.flash) {
            focus.update(delta);
            flash.update(delta);
        }
        // Native c8194..c81ac: fractional part of phase + delta/60.
        self.consult_phase = (self.consult_phase + delta.as_f32() / 60.0).fract();
    }

    pub fn confirm(&mut self) {
        if self.result.is_none() && self.confirming.is_none() && self.selected < self.answers.len()
        {
            use shin_core::time::{Ticks, Tween};
            let flash = &mut self.flash[self.selected];
            // Native c8a60..c8a94 clears the queue, then schedules .7 and 0
            // using the constant-rate flag0x40 and speed.2 for both legs.
            flash.enqueue_now(
                0.7,
                Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
            );
            flash.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
            self.confirming = Some(self.selected);
            self.sounds.push("decide");
        }
    }

    pub fn rect(&self, row: usize) -> glam::Vec4 {
        let answer = self.answers[row];
        // Native UV table 0xc9870 and packed visible-button layout 0xc7968.
        let width = if answer == 6 { 368.0 } else { 256.0 };
        let height = if answer == 6 { 176.0 } else { 64.0 };
        vec4(
            240.0 - width * 0.5,
            292.0 + row as f32 * 72.0,
            width,
            height,
        )
    }

    fn pointer_hit(&self, position: Vec2) -> Option<usize> {
        (0..self.answers.len()).find(|&row| {
            let r = self.rect(row);
            position.x >= r.x
                && position.x < r.x + r.z
                && position.y >= r.y
                && position.y < r.y + r.w
        })
    }
    fn controller_blocked(&self) -> bool {
        self.pointer_consumed || self.result.is_some() || self.confirming.is_some()
    }
    fn pointer_frame(
        &mut self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
        canvas_position: Vec2,
    ) {
        let mut area = self
            .pointer_area
            .take()
            .expect("Quiz pointer dispatcher reentered");
        if extent != self.pointer_extent && extent.0 > 0 && extent.1 > 0 {
            // Match other native InputArea owners: resize cancels the old contact.
            area.disable(self);
            area = crate::adv::input_area::InputArea::new(extent.0, extent.1);
            self.pointer_extent = extent;
        }
        if self.result.is_some() || self.confirming.is_some() {
            area.disable(self);
            self.pointer_consumed = true;
        } else {
            self.pointer_consumed = area.update(self, frame);
            if !self.pointer_consumed {
                self.pointer(canvas_position, false);
            }
        }
        self.pointer_area = Some(area);
    }
    fn disable_pointer(&mut self) {
        let mut area = self.pointer_area.take().unwrap();
        area.disable(self);
        self.pointer_area = Some(area);
        self.pointer_consumed = false;
    }
    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        if self.result.is_some() || self.confirming.is_some() {
            return;
        }
        let row = self.pointer_hit(position);
        if self.last_pointer != Some(position) || clicked {
            if let Some(row) = row {
                self.select_row(row);
                if clicked {
                    self.confirm();
                }
            }
        }
        self.last_pointer = Some(position);
    }
}

impl crate::adv::input_area::InputAreaTarget for QuizModel {
    fn probe(&mut self, position: Vec2) -> bool {
        if self.result.is_some() || self.confirming.is_some() {
            return false;
        }
        self.pointer_row = self.pointer_hit(position);
        self.pointer_row.is_some()
    }
    fn reset(&mut self) {
        self.pointer_row = None;
    }
    fn press(&mut self, _position: Vec2) {
        if let Some(row) = self.pointer_row {
            use shin_core::time::{Ticks, Tween};
            let flash = &mut self.flash[row];
            flash.enqueue_now(
                0.7,
                Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
            );
            self.select_row(row);
        }
    }
    fn release(&mut self, _position: Vec2) {
        if let Some(row) = self.pointer_row.take() {
            self.select_row(row);
            self.confirm();
        }
    }
    fn cancel(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            use shin_core::time::{Ticks, Tween};
            let flash = &mut self.flash[row];
            flash.enqueue_now(0.0, Tween::linear(Ticks::from_f32(flash.value() / 0.2)));
        }
    }
    fn long_press(&mut self, _position: Vec2) {
        self.cancel();
    }
    fn drag(&mut self, _previous: Vec2, _current: Vec2) {
        self.cancel();
    }
    fn gesture(&mut self, _velocity: Vec2) {}
}

pub struct QuizLayerImpl {
    textures: Arc<QuizTextures>,
    model: Arc<Mutex<QuizModel>>,
}
impl Clone for QuizLayerImpl {
    fn clone(&self) -> Self {
        Self {
            textures: self.textures.clone(),
            model: Arc::new(Mutex::new(self.model.lock().unwrap().clone())),
        }
    }
}
impl RenderClone for QuizLayerImpl {
    fn render_clone(&self, _: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        self.clone()
    }
}

impl std::fmt::Debug for QuizLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuizLayer")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

pub type QuizLayer = NewDrawableLayerWrapper<QuizLayerImpl>;
impl QuizLayer {
    pub async fn load(server: &AssetServer, mask: i32) -> Self {
        Self::new(
            server
                .load("/quiz.txa")
                .await
                .expect("Failed to load quiz textures"),
            mask,
        )
    }
    pub(crate) fn new(textures: Arc<QuizTextures>, mask: i32) -> Self {
        Self::from_plain_layer_constructor(|| QuizLayerImpl {
            textures,
            model: Arc::new(Mutex::new(QuizModel::new(mask))),
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        let inner = self.inner_ref();
        (
            Arc::as_ptr(&inner.textures) as usize,
            Arc::as_ptr(&inner.model) as usize,
            inner.model.lock().unwrap().selected as i32,
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_button_rect(&self, row: usize) -> glam::Vec4 {
        self.inner_ref().model.lock().unwrap().rect(row)
    }
    #[cfg(test)]
    pub(crate) fn reference_pointer_feedback(
        &self,
        row: usize,
    ) -> (Option<usize>, f32, Option<usize>) {
        let model = self.inner_ref().model.lock().unwrap();
        (
            model.pointer_row,
            model.flash[row].value(),
            model.confirming,
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_focus_values(&self) -> Vec<f32> {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .focus
            .iter()
            .map(|v| v.value())
            .collect()
    }
    pub(crate) fn restore_completed_answer(&self, answer: i32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .restore_completed_answer(answer);
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer(position, clicked);
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
        canvas_position: Vec2,
    ) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer_frame(frame, extent, canvas_position);
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.inner_ref().model.lock().unwrap().disable_pointer();
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        self.inner_ref().model.lock().unwrap().action_input(state);
    }
    pub fn navigate(&self, delta: i32) {
        self.inner_ref().model.lock().unwrap().move_selection(delta);
    }
    pub fn confirm(&self) {
        self.inner_ref().model.lock().unwrap().confirm();
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        self.inner_ref().model.lock().unwrap().take_sounds()
    }
    pub fn result(&self) -> Option<i32> {
        self.inner_ref().model.lock().unwrap().result
    }
}

impl NewDrawableLayerNeedsSeparatePass for QuizLayerImpl {}
impl NewDrawableLayer for QuizLayerImpl {
    fn property_policy(&self) -> crate::layer::new_drawable_layer::DrawablePropertyPolicy {
        // Native c8110 updates only the model; c8100/710d0 are RET.
        // c81d0 reads transforms/flags and uses fixed white/regular blending.
        crate::layer::new_drawable_layer::DrawablePropertyPolicy::TransformOnly
    }
    fn renders_pass(&self, pass_kind: PassKind) -> bool {
        pass_kind == PassKind::Transparent
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        _params: &DrawableParams,
        _clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent {
            return;
        }
        let model = self.model.lock().unwrap();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, false)
            .color_blend_type(ColorBlendType::Layer1);
        for (row, &answer) in model.answers.iter().enumerate() {
            let rect = model.rect(row);
            let (texture, uv) = if answer == 6 {
                let left = if model.consult_phase >= 0.5 {
                    368.0
                } else {
                    0.0
                };
                (
                    &self.textures.consult,
                    vec4(left / 768.0, 0.0, (left + 368.0) / 768.0, 1.0),
                )
            } else {
                (
                    &self.textures.button,
                    vec4(0.0, answer as f32 / 6.0, 1.0, (answer + 1) as f32 / 6.0),
                )
            };
            let color = shin_core::primitives::color::FloatColor4::WHITE;
            QuadVertices::new()
                .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(color.into_unorm())
                .render_button(
                    pass,
                    builder,
                    texture.as_source(),
                    transform.compute_top_left_transform(),
                    vec4(1.0, 1.0, 1.0, model.focus[row].value() * 0.5),
                    vec4(1.0, 1.0, 1.0, model.flash[row].value()),
                );
        }
    }
}
impl NewDrawableLayerFastForward for QuizLayerImpl {
    fn fast_forward(&mut self) {}
}
impl AdvUpdatable for QuizLayerImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.model.lock().unwrap().update(context.delta_ticks);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
impl QuizLayer {
    pub(crate) fn reference_plain_owner_probe(&self) -> (usize, Box<dyn Fn() -> usize>) {
        let model = &self.inner_ref().model;
        let id = Arc::as_ptr(model) as usize;
        let weak = Arc::downgrade(model);
        (id, Box::new(move || weak.strong_count()))
    }
    pub(crate) fn reference_plain_owner_clocks(
        &self,
        command: &str,
        queued: usize,
        delta: f32,
    ) -> Vec<(u32, bool)> {
        let mut model = self.inner_ref().model.lock().unwrap();
        if command == "selection" {
            model.selected = model.answers.len() - 1;
        }
        let QuizModel { focus, flash, .. } = &mut *model;
        let clocks = focus
            .iter_mut()
            .zip(flash)
            .flat_map(|(a, b)| [a, b])
            .collect::<Vec<_>>();
        clocks
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                super::plain_owner_tests::clock_command(t, command, i, queued, delta);
                (t.value().to_bits(), !t.is_idle())
            })
            .collect()
    }
}

// Owned interactive views dispatch through the same actual layer.
impl super::InteractiveView<'_, QuizLayer> {
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        self.with(|layer| layer.reference_controller_state())
    }
    #[cfg(test)]
    pub(crate) fn reference_button_rect(&self, row: usize) -> glam::Vec4 {
        self.with(|layer| layer.reference_button_rect(row))
    }
    #[cfg(test)]
    pub(crate) fn reference_pointer_feedback(
        &self,
        row: usize,
    ) -> (Option<usize>, f32, Option<usize>) {
        self.with(|layer| layer.reference_pointer_feedback(row))
    }
    #[cfg(test)]
    pub(crate) fn reference_focus_values(&self) -> Vec<f32> {
        self.with(|layer| layer.reference_focus_values())
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.with(|layer| layer.pointer(position, clicked))
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
        canvas_position: Vec2,
    ) {
        self.with(|layer| layer.pointer_frame(frame, extent, canvas_position))
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.with(|layer| layer.ignore_pointer_state())
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        self.with(|layer| layer.handle_controller_input(state))
    }
    pub fn navigate(&self, delta: i32) {
        self.with(|layer| layer.navigate(delta))
    }
    pub fn confirm(&self) {
        self.with(|layer| layer.confirm())
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        self.with(|layer| layer.take_sounds())
    }
    pub fn result(&self) -> Option<i32> {
        self.with(|layer| layer.result())
    }
}
