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
    pub fn reset(&self) {
        let mut model = self.inner_ref().model.lock().unwrap();
        model.disable_pointer();
        model.result = None;
        model.confirming = None;
        for flash in &mut model.flash {
            flash.fast_forward_to(0.0);
        }
        model.last_pointer = None;
        model.sounds.clear();
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
mod tests {
    include!("last_render_tests.rs");
    include!("plain_render_tests.rs");
    use super::super::murderer_model::MurdererMenu;
    use super::*;
    #[test]
    fn controller_confirm_and_up_priority_keep_filtered_answer_and_single_cue() {
        for mask in 1..128 {
            let count = QuizModel::new(mask).answers.len();
            for selected in 0..count {
                for bits in 0..8 {
                    let mut model = QuizModel::new(mask);
                    model.move_selection(selected as i32);
                    model.take_sounds();
                    let (up, down, confirm) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
                    model.controller_input(up, down, confirm);
                    let row = if confirm {
                        selected
                    } else if up {
                        (selected + count - 1) % count
                    } else if down {
                        (selected + 1) % count
                    } else {
                        selected
                    };
                    assert_eq!(model.selected, row);
                    assert_eq!(model.confirming, confirm.then_some(selected));
                    assert_eq!(
                        model.take_sounds(),
                        if confirm {
                            vec!["decide"]
                        } else if row != selected {
                            vec!["move"]
                        } else {
                            vec![]
                        }
                    );
                    if confirm {
                        model.controller_input(true, true, true);
                        assert!(model.take_sounds().is_empty());
                        model.update(shin_core::time::Ticks::from_f32(7.0));
                        assert_eq!(model.result, None);
                        model.update(shin_core::time::Ticks::ZERO);
                        assert_eq!(model.result, Some(model.answers[selected] as i32));
                        model.controller_input(true, true, true);
                        assert_eq!(model.selected, selected);
                        assert!(model.take_sounds().is_empty());
                    }
                }
            }
        }
        // No native equivalence claim for an empty mask; preserve safe port handling.
        let mut empty = QuizModel::new(0);
        empty.controller_input(true, true, true);
        assert_eq!(empty.result, None);
        assert!(empty.take_sounds().is_empty());
    }

    #[test]
    fn raw_quiz_dpad_stick_and_keyboard_use_combined_repeated_menu_actions() {
        use crate::app::AppAction;
        use shin_input::{
            Action, RawInputState,
            inputs::{GamepadButton, VirtualGamepadButton},
        };
        let mut raw = RawInputState::new();
        let state = |raw: &RawInputState| {
            let lowered = AppAction::lower(raw);
            enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                is_clicked: lowered[action],
                is_clicked_or_repeated: lowered[action],
                ..Default::default()
            })
        };
        let mut model = QuizModel::new(0x45);
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLDown] = true;
        model.action_input(&state(&raw));
        assert_eq!(model.selected, 1);
        raw.gamepads.unified.buttons[GamepadButton::Up] = true;
        model.action_input(&state(&raw));
        assert_eq!(
            model.selected, 0,
            "Real Up outranks simultaneous virtual Down"
        );
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLDown] = false;
        raw.gamepads.unified.buttons[GamepadButton::Up] = false;
        raw.keyboard.insert(winit::keyboard::KeyCode::ArrowDown);
        raw.keyboard.insert(winit::keyboard::KeyCode::Enter);
        model.action_input(&state(&raw));
        assert_eq!(model.selected, 0, "Enter confirms before keyboard movement");
        assert_eq!(model.confirming, Some(0));
        assert_eq!(model.take_sounds(), ["move", "move", "decide"]);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes guarded original/1.03 Quiz input and tween code on prepared rows"]
    fn reference_native_quiz_input_clocks_and_capture_match_production_model() {
        use crate::adv::input_area::InputAreaTarget;
        use shin_core::time::Ticks;
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-quiz-input-oracle-{}", std::process::id()));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-quiz-input.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let mut commands = Vec::<String>::new();
        for mask in 1..128 {
            let model = QuizModel::new(mask);
            for row in 0..model.answers.len() {
                for bits in 0..8 {
                    commands.push(format!("N {mask}"));
                    commands.extend((0..row).map(|_| "C 0 1 0".to_owned()));
                    commands.push("T 20".to_owned());
                    commands.push(format!(
                        "C {} {} {}",
                        bits & 1,
                        (bits >> 1) & 1,
                        (bits >> 2) & 1
                    ));
                    commands.extend(
                        [
                            "T 1", "C 1 1 1", "T 2.5", "T 1.75", "T 1.75", "C 1 1 1", "T 0",
                            "C 1 1 1", "T 60", "C 1 1 1", "T 0",
                        ]
                        .map(str::to_owned),
                    );
                }
                let rect = model.rect(row);
                let point = Vec2::new(rect.x + rect.z / 2.0, rect.y + rect.w / 2.0);
                for held in [0.0, 0.25, 1.0, 2.5, 3.5, 4.0, 20.0] {
                    for release in ["R 240 320", "X", "B", "D", "L"] {
                        commands.extend([
                            format!("N {mask}"),
                            format!("P {} {}", point.x, point.y),
                            "H 1900 1000".to_owned(),
                            format!("T {held}"),
                            release.to_owned(),
                        ]);
                        commands.extend(
                            [
                                "G",
                                "R 1900 1000",
                                "T 0",
                                "T 0.25",
                                "T 1",
                                "T 2.5",
                                "T 3.5",
                                "T 7",
                                "T 0",
                                "C 1 1 1",
                                "T 7",
                                "T 0",
                            ]
                            .map(str::to_owned),
                        );
                    }
                }
                // Exact half-open boundaries, including the larger consultation button.
                commands.push(format!("N {mask}"));
                for point in [
                    Vec2::new(rect.x, rect.y),
                    Vec2::new(rect.x + rect.z, rect.y),
                    Vec2::new(rect.x, rect.y + rect.w),
                    Vec2::new(rect.x - 0.001, rect.y),
                    Vec2::new(rect.x, rect.y - 0.001),
                    Vec2::new(rect.x + rect.z - 0.001, rect.y + rect.w - 0.001),
                    Vec2::new(240.0, rect.y + rect.w + 4.0),
                    Vec2::new(1900.0, 1000.0),
                ] {
                    commands.push(format!("P {} {}", point.x, point.y));
                }
            }
            // Retarget focus while both outgoing and incoming queues are running.
            commands.push(format!("N {mask}"));
            for i in 0..40 {
                commands.push(if i % 3 == 0 { "C 1 1 0" } else { "C 0 1 0" }.to_owned());
                commands.push("T 0.25".to_owned());
            }
        }
        commands.push("N 64".to_owned());
        commands.extend((0..121).map(|_| "T 1".to_owned()));
        commands.extend(["T 60", "T 1000", "C 1 1 0"].map(str::to_owned));
        let input = commands.join("\n") + "\n";
        let folder = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .parent()
            .unwrap()
            .to_owned();
        for name in ["uncompressed_executable", "uncompressed_executable_update"] {
            let mut child = Command::new(&binary)
                .arg(folder.join(name))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let payload = input.clone();
            let writer = std::thread::spawn(move || stdin.write_all(payload.as_bytes()).unwrap());
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(), commands.len());
            let mut model = QuizModel::new(127);
            let (mut decide, mut moves) = (0, 0);
            for (step, (command, line)) in commands.iter().zip(output.lines()).enumerate() {
                let mut parts = command.split_whitespace();
                let op = parts.next().unwrap();
                let args: Vec<f32> = parts.map(|s| s.parse().unwrap()).collect();
                let mut consumed = -1;
                match op {
                    "N" => {
                        model = QuizModel::new(args[0] as i32);
                        decide = 0;
                        moves = 0;
                    }
                    "C" => {
                        model.controller_input(args[0] != 0.0, args[1] != 0.0, args[2] != 0.0);
                        consumed = 1;
                    }
                    "P" => consumed = model.probe(Vec2::new(args[0], args[1])) as i32,
                    "H" => model.press(Vec2::new(args[0], args[1])),
                    "R" => model.release(Vec2::new(args[0], args[1])),
                    "X" => model.cancel(),
                    "B" => model.long_press(Vec2::ZERO),
                    "D" => model.drag(Vec2::ZERO, Vec2::ZERO),
                    "G" => model.gesture(Vec2::ZERO),
                    "L" => InputAreaTarget::reset(&mut model),
                    "T" => model.update(Ticks::from_f32(args[0])),
                    _ => panic!("{command}"),
                }
                for cue in model.take_sounds() {
                    match cue {
                        "decide" => decide += 1,
                        "move" => moves += 1,
                        _ => panic!("{cue}"),
                    }
                }
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.len(), 10 + model.answers.len() * 2);
                let integer = |i: usize| fields[i].parse::<i32>().unwrap();
                let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
                assert_eq!(
                    [
                        integer(1),
                        integer(2),
                        integer(3),
                        integer(4),
                        integer(5),
                        integer(6),
                        integer(7),
                        integer(8)
                    ],
                    [
                        model.selected as i32,
                        if model.result.is_some() {
                            -1
                        } else if model.confirming.is_some() {
                            1
                        } else {
                            0
                        },
                        model.pointer_row.map_or(-1, |row| row as i32),
                        model.result.is_some() as i32,
                        model.answers[model.selected] as i32,
                        consumed,
                        decide,
                        moves
                    ],
                    "{name} step{step} {command}"
                );
                let mut expected = vec![model.consult_phase];
                for row in 0..model.answers.len() {
                    expected.extend([model.focus[row].value(), model.flash[row].value()]);
                }
                for (field, expected) in expected.into_iter().enumerate() {
                    let actual = scalar(9 + field);
                    assert!(
                        (actual - expected).abs() < 2e-6,
                        "{name} step{step} {command} field{field}: {actual} vs {expected}"
                    );
                }
            }
            // Reject changes to every executed body, geometry setup and constant.
            let original = std::fs::read(folder.join(name)).unwrap();
            let shift = if name.ends_with("update") { 0xa0 } else { 0 };
            let constant_shift = if shift != 0 { 0xfe0 } else { 0 };
            let bodies = [
                0xc8110, 0xc8670, 0xc89b0, 0xc8b70, 0xc8bc0, 0xc8bd0, 0xc8ca0, 0xc8dd0, 0xc8e30,
                0xc8e90, 0xc8ea0, 0xc8f40, 0xc97b0, 0xc78d4, 0xbb1c0, 0xbb420, 0xbb4a0, 0xbb6c0,
                0xbb9f0, 0xbba00, 0xbba50, 0xcd810, 0xcd820, 0xcd830, 0xcd840,
            ];
            let constants = [
                0x159a78, 0x159660, 0x159798, 0x1595a4, 0x159974, 0x159724, 0x159344, 0x159548,
                0x1594a8, 0x159ac4, 0x159900, 0x1597b8, 0x159a1c, 0x159444, 0x159828,
            ];
            let altered = std::env::temp_dir()
                .join(format!("shin-quiz-input-altered-{}", std::process::id()));
            for address in bodies
                .into_iter()
                .map(|a| a - shift)
                .chain(constants.into_iter().map(|a| a + constant_shift))
            {
                let mut bytes = original.clone();
                bytes[address + 256] ^= 1;
                std::fs::write(&altered, bytes).unwrap();
                let rejected = Command::new(&binary).arg(&altered).output().unwrap();
                assert_eq!(
                    rejected.status.code(),
                    Some(2),
                    "{name}:altered {address:x}"
                );
            }
            std::fs::remove_file(&altered).unwrap();
            eprintln!(
                "{name}:{} native/production Quiz command rows:127nonempty masks,448visible answer rows,all8controller combinations,captured callbacks,focus/flash/phase/result/cues;40mutants rejected",
                commands.len()
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    fn completed_quiz_restores_original_answer_focus_without_input_or_cues() {
        use shin_core::time::Ticks;
        for mask in 1..128 {
            let answers = QuizModel::new(mask).answers;
            for (row, &answer) in answers.iter().enumerate() {
                let mut model = QuizModel::new(mask);
                model.restore_completed_answer(answer as i32);
                assert_eq!(model.selected, row);
                assert_eq!(model.result, Some(answer as i32));
                assert_eq!(model.confirming, None);
                for index in 0..answers.len() {
                    let expected = if index == row {
                        1.0
                    } else if index == 0 {
                        0.5
                    } else {
                        0.0
                    };
                    assert_eq!(model.focus[index].value(), expected);
                    assert!(model.focus[index].is_idle());
                    assert_eq!(model.flash[index].value(), 0.0);
                }
                model.move_selection(1);
                model.confirm();
                model.pointer(Vec2::new(240.0, 320.0), true);
                model.update(Ticks::from_f32(120.0));
                assert_eq!(model.selected, row);
                assert_eq!(model.result, Some(answer as i32));
                assert!(model.take_sounds().is_empty());
            }
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes guarded original/1.03 ARM64 Quiz restoration on fresh rows"]
    fn reference_native_quiz_restoration_executed_answers_and_focus() {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-quiz-restore-oracle-{}", std::process::id()));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-quiz-restoration.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        let mut requests = String::new();
        for mask in 1..128 {
            for answer in (-1..=7).chain([99]) {
                requests += &format!("{mask} {answer}\n");
            }
        }
        for name in ["uncompressed_executable", "uncompressed_executable_update"] {
            let exe = folder.join(name);
            let mut child = Command::new(&binary)
                .arg(&exe)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(requests.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(), 1270);
            for line in output.lines() {
                let values: Vec<f32> = line
                    .split_whitespace()
                    .map(|v| v.parse().unwrap())
                    .collect();
                let mut model = QuizModel::new(values[0] as i32);
                model.restore_completed_answer(values[1] as i32);
                assert_eq!(model.selected, values[2] as usize, "{name}: {line}");
                assert_eq!(model.result, Some(values[3] as i32), "{name}: {line}");
                for (row, &expected) in values[4..].iter().enumerate() {
                    assert_eq!(model.focus[row].value(), expected, "{name}: {line}");
                }
            }
            // The caller dispatches only completed layerbank metadata.
            let mut bytes = std::fs::read(&exe).unwrap();
            for (a, word) in [
                (0x1a758, 0x5287f089u32),
                (0x1a75c, 0x38696908),
                (0x1a760, 0x34000168),
                (0x1a76c, 0xb97f8901),
                (0x1a774, 0xf9404508),
            ] {
                assert_eq!(
                    u32::from_le_bytes(bytes[a + 256..a + 260].try_into().unwrap()),
                    word,
                    "{name} caller {a:x}"
                );
            }
            let address = if name.ends_with("update") {
                0xc8600
            } else {
                0xc86a0
            };
            bytes[address + 256] ^= 1;
            let altered =
                std::env::temp_dir().join(format!("shin-quiz-altered-{}", std::process::id()));
            std::fs::write(&altered, bytes).unwrap();
            let rejected = Command::new(&binary).arg(&altered).output().unwrap();
            std::fs::remove_file(altered).unwrap();
            assert_eq!(rejected.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&rejected.stderr).contains("Unreviewed"));
            eprintln!(
                "{name}:1270 native restoration cases (448 valid answer rows), focus/result/selected equality; altered callback rejects before execution"
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    fn confirmation_flash_preserves_answer_and_publishes_after_idle_poll() {
        use shin_core::time::Ticks;
        for mask in 1..128 {
            for row in 0..(mask as u32).count_ones() as usize {
                let mut model = QuizModel::new(mask);
                model.move_selection(row as i32);
                let answer = model.answers[row] as i32;
                model.confirm();
                model.move_selection(1);
                model.pointer(Vec2::new(240.0, 300.0), true);
                model.confirm();
                assert_eq!(model.selected, row);
                assert_eq!(model.result, None);
                assert_eq!(
                    model
                        .take_sounds()
                        .iter()
                        .filter(|&&sound| sound == "decide")
                        .count(),
                    1
                );
                for (ticks, flash) in [(1.0, 0.2), (2.5, 0.7), (1.75, 0.35), (1.75, 0.0)] {
                    model.update(Ticks::from_f32(ticks));
                    assert!((model.flash[row].value() - flash).abs() < 1e-6);
                    assert_eq!(model.result, None);
                }
                model.update(Ticks::ZERO);
                assert_eq!(model.result, Some(answer));
            }
        }
        let mut model = QuizModel::new(1);
        model.confirm();
        model.update(Ticks::from_f32(1000.0));
        assert_eq!(
            model.result, None,
            "Publication checks idle before clocks even for large frames"
        );
        let mut clone = model.clone();
        clone.update(Ticks::ZERO);
        assert_eq!(clone.result, Some(0));
        assert_eq!(
            model.result, None,
            "Cloned render snapshots own their pending animation"
        );
    }

    #[test]
    #[ignore = "requires supplied original and updated executable"]
    fn reference_native_quiz_confirmation_flash_queue_and_preclock_result_gate() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        for (name, offset, float_offset) in [
            ("uncompressed_executable", 0usize, 0usize),
            ("uncompressed_executable_update", 0xa0, 0xfe0),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
            for (a, expected) in [
                (0xc8134, 0x7100051f),
                (0xc8138, 0x54000141),
                (0xc814c, 0x9101c100),
                (0xc8150, 0x97ffce28),
                (0xc8154, 0x37000060),
                (0xc8158, 0x12800008),
                (0xc815c, 0xb9004268),
                (0xc817c, 0x9101c280),
                (0xc8180, 0x97ffce34),
                (0xc8a78, 0x321a03e1),
                (0xc8a80, 0x97ffcbe0),
                (0xc8a84, 0x1e2703e0),
                (0xc8a8c, 0x321a03e1),
                (0xc8a94, 0x97ffcbdb),
                (0xc8a98, 0x320003e8),
                (0xc8a9c, 0xb9004268),
                (0xc89b8, 0x34000068),
                (0xc8688, 0xd35ffd08),
                (0xbb94c, 0x373000d4),
                (0xbb978, 0x1e203820),
                (0xbb97c, 0x1e221800),
                (0xbb980, 0x1e20c000),
            ] {
                assert_eq!(word(a - offset), expected, "{name} at{a:x}");
            }
            for (a, value) in [(0x1595a4, 0.7f32), (0x159974, 0.2)] {
                assert_eq!(
                    word(a + float_offset),
                    value.to_bits(),
                    "{name} constant{a:x}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires supplied original and updated executable"]
    fn reference_native_quiz_capture_press_release_and_cancel_callbacks() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        for (name, offset) in [
            ("uncompressed_executable", 0usize),
            ("uncompressed_executable_update", 0xa0),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
            for (a, expected) in [
                (0xc8b74, 0xb9404108),
                (0xc8b9c, 0xb9004900),
                (0xc8bc8, 0xb9004909),
                (0xc8c14, 0x321a03e1),
                (0xc8c18, 0x2a1f03e2),
                (0xc8c58, 0x321a03e1),
                (0xc8c7c, 0x1e2a1001),
                (0xc8d3c, 0x321a03e1),
                (0xc8d78, 0x321a03e1),
                (0xc8d8c, 0x321a03e1),
                (0xc8d98, 0x320003e8),
                (0xc8d9c, 0xb9004288),
                (0xc8da8, 0xb9004909),
                (0xc8e08, 0x1e2703e0),
                (0xc8e0c, 0x321a03e1),
                (0xc8e20, 0xb9004909),
                (0xc8e68, 0x1e2703e0),
                (0xc8e70, 0x2a1f03e2),
                (0xc8e80, 0xb9004909),
                (0xc8ed8, 0x1e2703e0),
                (0xc8edc, 0x321a03e1),
                (0xc8ef0, 0xb9004909),
                (0xc897c, 0x1a9f17e0),
                (0xc89b4, 0xb9404108),
                (0xc89bc, 0x320003e0),
            ] {
                assert_eq!(word(a - offset), expected, "{name} callback {a:x}");
            }
        }
    }

    #[test]
    fn consultation_hover_animates_white_focus_without_answering() {
        use shin_core::time::Ticks;
        let mut model = QuizModel::new(0b1000101);
        model.pointer(Vec2::new(240.0, 480.0), false);
        model.update(Ticks::from_f32(2.0));
        assert_eq!(model.selected, 2);
        assert_eq!(model.result, None);
        assert!((model.focus[2].value() - 0.5).abs() < 1e-6);
        assert!((model.focus[0].value() - 0.9).abs() < 1e-6);
        model.pointer(Vec2::new(240.0, 480.0), false);
        model.update(Ticks::from_f32(2.0));
        assert_eq!(model.focus[2].value(), 1.0);
        model.update(Ticks::from_f32(26.0));
        assert_eq!(model.consult_phase, 0.5);
        model.update(Ticks::from_f32(30.0));
        assert_eq!(model.consult_phase, 0.0);
        model.pointer(Vec2::new(240.0, 380.0), false);
        model.update(Ticks::from_f32(4.0));
        assert!((model.focus[2].value() - 0.8).abs() < 1e-6);
        assert_eq!(model.result, None);
    }

    fn capture_sample(
        model: &mut QuizModel,
        sampler: &mut crate::adv::input_area::PointerSampler,
        extent: (u16, u16),
        point: Vec2,
        held: bool,
        seconds: f64,
    ) {
        let physical = point * Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
        sampler.sample(physical, held, seconds);
        model.pointer_frame(&sampler.frame(), extent, point);
    }
    #[test]
    fn quiz_captured_taps_keep_original_answer_for_every_mask_and_viewport() {
        use shin_core::time::Ticks;
        for extent in [(960, 540), (1920, 1080), (3840, 2160)] {
            for mask in 1..128 {
                for row in 0..QuizModel::new(mask).answers.len() {
                    let mut model = QuizModel::new(mask);
                    let answer = model.answers[row];
                    let r = model.rect(row);
                    let point = Vec2::new(r.x + r.z / 2.0, r.y + r.w / 2.0);
                    let mut sampler = crate::adv::input_area::PointerSampler::default();
                    capture_sample(&mut model, &mut sampler, extent, point, true, 0.0);
                    assert_eq!(model.result, None);
                    assert_eq!(model.confirming, None);
                    assert_eq!(model.pointer_row, Some(row));
                    assert!(model.controller_blocked());
                    assert!(model.take_sounds().is_empty());
                    // Native Up coordinates are ignored; last captured position wins.
                    capture_sample(
                        &mut model,
                        &mut sampler,
                        extent,
                        Vec2::new(1900.0, 1000.0),
                        false,
                        0.1,
                    );
                    assert_eq!(model.selected, row);
                    assert_eq!(model.confirming, Some(row));
                    assert_eq!(model.result, None);
                    assert!(model.controller_blocked());
                    assert_eq!(model.take_sounds(), ["decide"]);
                    model.pointer_frame(&sampler.frame(), extent, point); // Same frame cannot confirm twice.
                    assert!(model.take_sounds().is_empty());
                    model.update(Ticks::from_f32(7.0));
                    assert_eq!(model.result, None);
                    model.update(Ticks::ZERO);
                    assert_eq!(model.result, Some(answer as i32));
                }
            }
        }
    }
    #[test]
    fn quiz_hold_press_and_drag_cancel_preserve_native_flash_queues() {
        use shin_core::time::Ticks;
        for extent in [(960, 540), (1920, 1080), (3840, 2160)] {
            let mut model = QuizModel::new(0x41);
            let mut sampler = crate::adv::input_area::PointerSampler::default();
            let point = Vec2::new(240.0, 400.0);
            capture_sample(&mut model, &mut sampler, extent, point, true, 0.0);
            capture_sample(&mut model, &mut sampler, extent, point, true, 0.149);
            assert_eq!(model.selected, 0);
            assert_eq!(model.flash[1].value(), 0.0);
            capture_sample(&mut model, &mut sampler, extent, point, true, 0.002);
            assert_eq!(model.selected, 1);
            assert!(model.take_sounds().is_empty());
            model.update(Ticks::from_f32(3.5));
            assert!((model.flash[1].value() - 0.7).abs() < 1e-6);
            assert_eq!(model.result, None);
            assert_eq!(model.confirming, None);
            // At exactly16canvas pixels the native contact remains a tap.
            capture_sample(
                &mut model,
                &mut sampler,
                extent,
                point + Vec2::new(16.0, 0.0),
                true,
                0.01,
            );
            assert_eq!(model.pointer_row, Some(1));
            capture_sample(
                &mut model,
                &mut sampler,
                extent,
                point + Vec2::new(20.0, 0.0),
                true,
                0.01,
            );
            assert_eq!(model.pointer_row, None);
            assert_eq!(model.confirming, None);
            model.update(Ticks::from_f32(1.0));
            assert!((model.flash[1].value() - 0.5).abs() < 1e-6);
            capture_sample(&mut model, &mut sampler, extent, point, false, 0.01);
            assert!(model.take_sounds().is_empty());
            assert_eq!(model.result, None);
            model.update(Ticks::from_f32(2.5));
            assert_eq!(model.flash[1].value(), 0.0);
            // A held press at peak needs only the native3.5tick fade after release.
            capture_sample(&mut model, &mut sampler, extent, point, true, 0.0);
            capture_sample(&mut model, &mut sampler, extent, point, true, 0.16);
            model.update(Ticks::from_f32(3.5));
            capture_sample(&mut model, &mut sampler, extent, point, false, 0.0);
            assert_eq!(model.take_sounds(), ["decide"]);
            model.update(Ticks::from_f32(3.5));
            assert_eq!(model.result, None);
            model.update(Ticks::ZERO);
            assert_eq!(model.result, Some(6));
        }
    }
    #[test]
    fn quiz_cancel_resize_and_opening_under_held_contact_never_answer() {
        use shin_core::time::Ticks;
        let point = Vec2::new(240.0, 400.0);
        let mut sampler = crate::adv::input_area::PointerSampler::default();
        sampler.sample(point, true, 0.0); // Down happened before the Quiz was installed.
        let mut model = QuizModel::new(0x41);
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, true, 1.0);
        assert_eq!(model.pointer_row, None);
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, false, 0.0);
        assert_eq!(model.confirming, None);
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, true, 0.0);
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, true, 0.16);
        model.update(Ticks::from_f32(3.5));
        model.disable_pointer();
        model.update(Ticks::from_f32(3.5));
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, false, 0.0);
        assert_eq!(model.confirming, None);
        assert_eq!(model.flash[1].value(), 0.0);
        capture_sample(&mut model, &mut sampler, (1920, 1080), point, true, 0.0);
        capture_sample(&mut model, &mut sampler, (3840, 2160), point, true, 0.16);
        assert_eq!(model.pointer_row, None);
        capture_sample(&mut model, &mut sampler, (3840, 2160), point, false, 0.0);
        assert_eq!(model.confirming, None);
        assert!(model.take_sounds().is_empty());
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_consultation_blink_and_white_focus() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom).parent().unwrap();
        for (name, offset, floats) in [
            ("uncompressed_executable", 0usize, 0usize),
            ("uncompressed_executable_update", 0xa0, 0xfe0),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
            for (a, expected) in [
                (0xc83b0, 0xbd404e64),
                (0xc83b4, 0x1e2b2080),
                (0xc83b8, 0x1e222804),
                (0xc83bc, 0x1e24bc00),
                (0xc8418, 0x1e2b0943),
                (0xc8198, 0x1e211901),
                (0xc819c, 0xbd404e60),
                (0xc81a0, 0x1e202820),
            ] {
                assert_eq!(word(a - offset), expected, "{name} instruction {a:x}");
            }
            for (a, expected) in [
                (0x159798, 60.0f32),
                (0x159724, 0.05),
                (0x159828, 368.0),
                (0x1594a8, 768.0),
                (0x159ac4, 176.0),
            ] {
                assert_eq!(word(a + floats), expected.to_bits(), "{name} float {a:x}");
            }
        }
    }

    #[test]
    #[ignore = "requires supplied quiz atlas and GPU"]
    fn reference_consultation_hover_white_highlight_and_independent_blink_gpu() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/quiz.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let tex = &archive.textures[archive.name_to_index["consult"]];
        assert_eq!(tex.dimensions(), (768, 176));
        for (initial, samples, atlas_left, focus) in [
            (0.0, vec![], 0.0, 0.0),
            (0.0, vec![(Vec2::new(240.0, 480.0), 0.0)], 0.0, 0.0),
            (0.0, vec![(Vec2::new(240.0, 480.0), 2.0)], 0.0, 0.5),
            (0.0, vec![(Vec2::new(240.0, 480.0), 4.0)], 0.0, 1.0),
            (30.0, vec![], 368.0, 0.0),
            (30.0, vec![(Vec2::new(240.0, 480.0), 2.0)], 368.0, 0.5),
            (
                0.0,
                vec![
                    (Vec2::new(240.0, 480.0), 4.0),
                    (Vec2::new(240.0, 380.0), 4.0),
                ],
                0.0,
                0.8,
            ),
        ] {
            let pixels = render_reference_frame_pixels(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::QuizHover(initial, &samples)),
            );
            std::fs::write("/tmp/shin-consult-hover-debug.rgba", &pixels).unwrap();
            let mut worst = 0.0f32;
            let mut worst_sample = None;
            for y in 436..612usize {
                for x in 56..424usize {
                    let t = sample_reference_rgba(
                        tex.dimensions(),
                        tex.as_raw(),
                        (atlas_left + x as f32 + 0.5 - 56.0) / 768.0,
                        (y as f32 + 0.5 - 436.0) / 176.0,
                    );
                    let pixel = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                    for c in 0..3 {
                        let expected = (t[c] + (255.0 - t[c]) * focus * 0.5) * t[3] / 255.0;
                        let error = (pixel[c] as f32 - expected).abs();
                        if error > worst {
                            worst = error;
                            worst_sample = Some((x, y, c, pixel[c], expected));
                        }
                    }
                }
            }
            assert!(
                worst <= 2.0,
                "consult hover initial{initial} focus{focus} atlas{atlas_left}: RGB error{worst}, sample{worst_sample:?}"
            );
            println!("Consult hover focus{focus} atlas{atlas_left}:64768 pixels RGB error{worst}");
        }
    }

    #[test]
    #[ignore = "requires supplied Murderer atlas, Metal GPU and Loopback Audio"]
    fn reference_completed_murderer_restoration_atlas_gpu() {
        crate::init_test_task_pools();
        let _audio = shin_audio::AudioManager::new_for_tests();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/murderer.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let tex = |name: &str| archive.get_texture(name).unwrap();
        for (answer, name) in [
            "thumb_1", "thumb_2", "thumb_4", "thumb_56", "thumb_7", "thumb_8",
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::Replay),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::MurdererRestored(answer as i32)),
            );
            let image = tex(name);
            let mut worst = 0.0f32;
            let mut count = 0;
            for y in 0..1080usize {
                for x in 0..1920usize {
                    let t = sample_reference_rgba(
                        image.dimensions(),
                        image.as_raw(),
                        (x as f32 + 0.5) / 1920.0,
                        (y as f32 + 0.5) / 1080.0,
                    );
                    assert!(t[3] > 254.99, "Full-screen native replay atlas is opaque");
                    for c in 0..3 {
                        worst = worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - t[c]).abs());
                    }
                    count += 1;
                }
            }
            assert!(worst < 1.1, "Restored replay{answer} RGB error{worst}");
            eprintln!(
                "Restored replay{answer}: {count} independent full-screen atlas pixels, maxRGBerror{worst}"
            );
        }
        for (answer, index, scroll) in [(6, 0, 0.0), (19, 13, 810.0), (26, 20, 1170.0)] {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::Hints),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::MurdererRestored(answer)),
            );
            let top = 180.0 + index as f32 * 90.0 - scroll;
            let mut worst = 0.0f32;
            let mut count = 0;
            for y in top as usize..top as usize + 88 {
                for x in 320..1600usize {
                    let t = sample_reference_rgba(
                        tex("hint_menu").dimensions(),
                        tex("hint_menu").as_raw(),
                        (x as f32 + 0.5 - 320.0) / 1280.0,
                        (y as f32 + 0.5 - top) / 88.0,
                    );
                    let blend = |dst: [f32; 3], src: [f32; 4]| {
                        std::array::from_fn::<_, 3, _>(|c| {
                            (src[c] * src[3] / 255.0 + dst[c] * (1.0 - src[3] / 255.0)).round()
                        })
                    };
                    let bg = sample_reference_rgba(
                        tex("different_space_2d").dimensions(),
                        tex("different_space_2d").as_raw(),
                        (x as f32 + 0.5) / 1920.0,
                        (y as f32 + 0.5) / 1080.0,
                    );
                    let mut expected = blend([0.0; 3], bg);
                    // Native left nine-slice positions/UVs; the translucent hint
                    // plate must be compared over this actual panel/background.
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let u = if px < 600.0 {
                        (px - 264.0) / 336.0 * 0.875
                    } else {
                        0.875 + (px - 600.0) / 1040.0 * (11.0 / 12.0 - 0.875)
                    };
                    let v = if py < 564.0 {
                        (py - 164.0) / 400.0 * (25.0 / 28.0)
                    } else {
                        25.0 / 28.0 + (py - 564.0) / 336.0 * (13.0 / 14.0 - 25.0 / 28.0)
                    };
                    let panel =
                        sample_reference_rgba(tex("left").dimensions(), tex("left").as_raw(), u, v);
                    expected = blend(expected, panel);
                    let white_mix = |value: f32| value + (255.0 - value) * 0.5;
                    expected = blend(
                        expected,
                        [white_mix(t[0]), white_mix(t[1]), white_mix(t[2]), t[3]],
                    );
                    if (352..1568).contains(&x)
                        && y as f32 + 0.5 >= top + 13.0
                        && y as f32 + 0.5 < top + 77.0
                    {
                        let row = if index == 20 { 0 } else { index + 1 };
                        let glyph = sample_reference_rgba(
                            tex("hint_text").dimensions(),
                            tex("hint_text").as_raw(),
                            (x as f32 + 0.5 - 352.0) / 1216.0,
                            (row as f32 * 64.0 + y as f32 + 0.5 - top - 13.0) / 1408.0,
                        );
                        expected = blend(
                            expected,
                            [
                                white_mix(glyph[0]),
                                white_mix(glyph[1]),
                                white_mix(glyph[2]),
                                glyph[3],
                            ],
                        );
                    }
                    for c in 0..3 {
                        worst =
                            worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - expected[c]).abs());
                    }
                    count += 1;
                }
            }
            assert_eq!(count, 112_640);
            assert!(
                worst < 1.1,
                "Restored hint{index} scroll{scroll} RGBerror{worst}"
            );
            eprintln!(
                "Restored hint{index} scroll{scroll}: {count} independent atlas focus/text pixels, maxRGBerror{worst}"
            );
        }
        let background = tex("different_space_1a");
        for (answer, face) in [(27, "But_FutekiB2"), (48, "But_KomaruB1")] {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::Accusation),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::MurdererRestored(answer)),
            );
            let mut worst = 0.0f32;
            let mut count = 0;
            for (name, rect, area) in [
                (face, vec4(1182.0, 0.0, 384.0, 384.0), (1200..1550, 20..170)),
                (
                    "But_BodyB",
                    vec4(928.0, 384.0, 812.0, 696.0),
                    (1200..1700, 400..850),
                ),
            ] {
                let image = tex(name);
                for y in area.1 {
                    for x in area.0.clone() {
                        let bg = sample_reference_rgba(
                            background.dimensions(),
                            background.as_raw(),
                            (x as f32 + 0.5) / 1920.0,
                            (y as f32 + 0.5) / 1080.0,
                        );
                        let t = sample_reference_rgba(
                            image.dimensions(),
                            image.as_raw(),
                            (x as f32 + 0.5 - rect.x) / rect.z,
                            (y as f32 + 0.5 - rect.y) / rect.w,
                        );
                        for c in 0..3 {
                            let expected = t[c] * t[3] / 255.0 + bg[c] * (1.0 - t[3] / 255.0);
                            worst =
                                worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - expected).abs());
                        }
                        count += 1;
                    }
                }
            }
            assert!(
                worst < 1.1,
                "Restored accusation{answer} {face} RGBerror{worst}"
            );
            eprintln!(
                "Restored accusation{answer} {face}: {count} independent body/face pixels, maxRGBerror{worst}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied Cake atlas, Metal GPU and Loopback Audio"]
    fn reference_completed_cake_restoration_atlas_gpu() {
        crate::init_test_task_pools();
        let _audio = shin_audio::AudioManager::new_for_tests();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/cake.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let base = archive.get_texture("base").unwrap();
        // Independent native46d3c placements, including both extremes and top.
        for (answer, rect) in [
            (0, Some((744, 336, 256, 592))),
            (7, Some((856, 16, 208, 368))),
            (14, Some((936, 336, 240, 576))),
            (-1, None),
        ] {
            let pixels = render_reference_frame_pixels(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::CakeRestored(answer)),
            );
            let slice =
                (answer >= 0).then(|| archive.get_texture(&format!("{answer:02}")).unwrap());
            let mut worst = 0.0f32;
            let mut changed = 0;
            for y in 0..1080usize {
                for x in 0..1920usize {
                    let mut expected = [0.0f32; 3];
                    if (360..1560).contains(&x) {
                        let t = sample_reference_rgba(
                            base.dimensions(),
                            base.as_raw(),
                            (x as f32 + 0.5 - 360.0) / 1200.0,
                            (y as f32 + 0.5) / 1080.0,
                        );
                        for c in 0..3 {
                            expected[c] = (t[c] * t[3] / 255.0).round();
                        }
                    }
                    if let Some((left, top, width, height)) = rect {
                        if (left..left + width).contains(&x) && (top..top + height).contains(&y) {
                            let image = slice.unwrap();
                            let t = sample_reference_rgba(
                                image.dimensions(),
                                image.as_raw(),
                                (x as f32 + 0.5 - left as f32) / width as f32,
                                (y as f32 + 0.5 - top as f32) / height as f32,
                            );
                            if t[3] > 128.0 {
                                changed += 1;
                            }
                            for c in 0..3 {
                                expected[c] =
                                    t[c] * t[3] / 255.0 + expected[c] * (1.0 - t[3] / 255.0);
                            }
                        }
                    }
                    for c in 0..3 {
                        worst =
                            worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - expected[c]).abs());
                    }
                }
            }
            if answer >= 0 {
                assert!(changed > 1000);
            }
            assert!(
                worst < 2.0,
                "Cake restored answer{answer}: independent atlas RGB error{worst}"
            );
            eprintln!(
                "Cake restored answer{answer}:2073600 independent pixels, {changed} opaque slice pixels, native hidden message/pointer, maxRGBerror{worst}"
            );
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "requires supplied Cake atlas, native executable, Metal and Loopback Audio"]
    fn reference_cake_input_feedback_atlas_gpu() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{
            io::{Read, Write},
            process::{Command, Stdio},
        };
        crate::init_test_task_pools();
        let _audio = shin_audio::AudioManager::new_for_tests();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-cake-gpu-native-{}", std::process::id()));
        let output = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-cake-input.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
        let executable = root.join("umineko_switch_files").join(if updated {
            "uncompressed_executable_update"
        } else {
            "uncompressed_executable"
        });
        let paths = std::env::var_os("SHIN_REFERENCE_PATCH_ROM")
            .into_iter()
            .chain(std::iter::once(
                std::env::var_os("SHIN_REFERENCE_ROM").unwrap(),
            ));
        let mut bytes = Vec::new();
        for path in paths {
            let mut rom =
                RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                    .unwrap();
            if let Ok(file) = rom.find_file("/cake.txa") {
                rom.open_file(file)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                break;
            }
        }
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let a = Vec2::new(850.0, 700.0);
        let b = Vec2::new(960.0, 700.0);
        let cases: Vec<(&str, Vec<(Vec2, bool, f32)>)> = vec![
            ("initial", vec![]),
            ("incoming-focus", vec![(a, false, 1.0)]),
            (
                "outgoing-and-incoming",
                vec![(a, false, 4.0), (b, false, 1.0)],
            ),
            ("flash-first-tick", vec![(a, false, 4.0), (a, true, 1.0)]),
            ("flash-peak", vec![(a, false, 4.0), (a, true, 3.5)]),
            (
                "flash-finished-still-pending",
                vec![(a, false, 4.0), (a, true, 7.0)],
            ),
            (
                "answer-published-still-fading",
                vec![(a, false, 4.0), (a, true, 7.0), (a, false, 1.0)],
            ),
            (
                "instruction-and-finger-hidden",
                vec![(a, false, 4.0), (a, true, 7.0), (a, false, 3.0)],
            ),
        ];
        // Independent native initializer geometry, in ascending original row order.
        let rects = [
            (744, 336, 256, 592),
            (600, 336, 384, 544),
            (520, 320, 464, 464),
            (504, 272, 480, 384),
            (536, 176, 448, 208),
            (616, 80, 368, 304),
            (728, 32, 256, 336),
            (856, 16, 208, 368),
            (936, 32, 256, 352),
            (936, 96, 368, 288),
            (952, 176, 432, 208),
            (936, 288, 480, 368),
            (936, 320, 464, 464),
            (936, 336, 384, 544),
            (936, 336, 240, 576),
        ];
        let mut all: Vec<(String, Vec<u8>, [f32; 15], [f32; 15], f32, Vec2)> = Vec::new();
        for (name, samples) in cases {
            let mut request = "N\n".to_string();
            for &(point, clicked, ticks) in &samples {
                if clicked {
                    request +=
                        &format!("P {} {}\nR {} {}\nL\n", point.x, point.y, point.x, point.y);
                } else if !request.contains("R ") {
                    request += &format!("M {} {}\nC 0 0 0 0\n", point.x, point.y);
                }
                request += &format!("T {ticks}\n");
            }
            let mut child = Command::new(&binary)
                .arg(&executable)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(request.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            let fields: Vec<&str> = output.lines().last().unwrap().split_whitespace().collect();
            let value = |i: usize| fields[i].parse::<f32>().unwrap();
            all.push((
                name.to_string(),
                render_reference_frame_pixels(
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(MurdererFeedback::CakeTimed(&samples)),
                ),
                std::array::from_fn(|row| value(11 + row * 2)),
                std::array::from_fn(|row| value(12 + row * 2)),
                value(41),
                Vec2::new(value(6), value(7)),
            ));
        }
        for answer in [0, 7, 14, -1] {
            all.push((
                format!("restored-{answer}"),
                render_reference_frame_pixels(
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(MurdererFeedback::CakeRestored(answer)),
                ),
                std::array::from_fn(|row| if row as i32 == answer { 1.0 } else { 0.0 }),
                [0.0; 15],
                0.0,
                Vec2::new(1440.0, 900.0),
            ));
        }
        let base = archive.get_texture("base").unwrap();
        let msg = archive.get_texture("msg").unwrap();
        let finger = archive.get_texture("finger").unwrap();
        for (name, pixels, focus, flash, opacity, point) in all {
            let mut expected = vec![[0.0f32; 3]; 1920 * 1080];
            let mut draw = |dimensions: (u32, u32),
                            data: &[u8],
                            (left, top, width, height): (i32, i32, i32, i32),
                            alpha: f32,
                            white: f32| {
                let alpha = (alpha * 255.0) as u8 as f32 / 255.0;
                for y in top.max(0)..(top + height).min(1080) {
                    for x in left.max(0)..(left + width).min(1920) {
                        let sampled = sample_reference_rgba(
                            dimensions,
                            data,
                            (x as f32 + 0.5 - left as f32) / width as f32,
                            (y as f32 + 0.5 - top as f32) / height as f32,
                        );
                        let coverage = sampled[3] / 255.0 * alpha;
                        let previous = &mut expected[y as usize * 1920 + x as usize];
                        for c in 0..3 {
                            let source = (255.0 - sampled[c]).mul_add(white, sampled[c]);
                            previous[c] =
                                (source * coverage + previous[c] * (1.0 - coverage)).round();
                        }
                    }
                }
            };
            draw(
                base.dimensions(),
                base.as_raw(),
                (360, 0, 1200, 1080),
                1.0,
                0.0,
            );
            for row in 0..15 {
                if focus[row] > 0.0 {
                    let image = archive.get_texture(&format!("{row:02}")).unwrap();
                    draw(
                        image.dimensions(),
                        image.as_raw(),
                        rects[row],
                        focus[row],
                        flash[row],
                    );
                }
            }
            if opacity > 0.0 {
                draw(
                    msg.dimensions(),
                    msg.as_raw(),
                    (616, 920, 688, 96),
                    opacity,
                    0.0,
                );
                draw(
                    finger.dimensions(),
                    finger.as_raw(),
                    ((point.x - 16.0) as i32, (point.y - 16.0) as i32, 96, 96),
                    opacity,
                    0.0,
                );
            }
            let mut worst = 0.0f32;
            for (i, expected) in expected.iter().enumerate() {
                for c in 0..3 {
                    worst = worst.max((pixels[i * 4 + c] as f32 - expected[c]).abs());
                }
            }
            assert!(
                worst <= 2.0,
                "Cake{name}: independent native fields/atlas full-frame RGB error{worst}"
            );
            eprintln!(
                "Cake{name}:2073600 independent native-field/atlas pixels, focus0={:.3}/14={:.3},flash0={:.3},instruction={opacity:.3},maxRGBerror{worst}",
                focus[0], focus[14], flash[0]
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    #[ignore = "requires supplied original/1.03 executable, quiz atlas, Metal and Loopback Audio"]
    fn reference_quiz_controller_feedback_native_fields_and_atlas_gpu() {
        crate::init_test_task_pools();
        let _audio = shin_audio::AudioManager::new_for_tests();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{
            io::{Read, Write},
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-quiz-gpu-oracle-{}", std::process::id()));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-quiz-input.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
        let executable = root.join("umineko_switch_files").join(if updated {
            "uncompressed_executable_update"
        } else {
            "uncompressed_executable"
        });
        let mut atlas = None;
        for path in std::env::var_os("SHIN_REFERENCE_PATCH_ROM")
            .into_iter()
            .chain(std::env::var_os("SHIN_REFERENCE_ROM"))
        {
            let mut rom =
                RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                    .unwrap();
            if let Ok(file) = rom.find_file("/quiz.txa") {
                let mut bytes = Vec::new();
                rom.open_file(file)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                atlas =
                    Some(shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap());
                break;
            }
        }
        let atlas = atlas.unwrap();
        let cases: [(&str, &[(u8, f32)]); 11] = [
            ("initial", &[(0, 0.0)]),
            ("down-focus", &[(2, 1.0)]),
            ("up-wins", &[(3, 1.0)]),
            ("retarget", &[(2, 0.25), (2, 0.25), (3, 0.25)]),
            ("confirm-before-down", &[(6, 1.0)]),
            ("consult-flash-rise", &[(3, 4.0), (7, 1.0)]),
            ("consult-flash-peak", &[(3, 4.0), (7, 3.5)]),
            ("consult-flash-end", &[(3, 4.0), (7, 7.0)]),
            ("consult-published", &[(3, 4.0), (7, 7.0), (7, 0.0)]),
            ("consult-blink-second-half", &[(3, 30.0)]),
            ("consult-blink-wrap", &[(3, 60.0)]),
        ];
        for (name, samples) in cases {
            let mut request = "N 69\n".to_owned();
            for &(bits, ticks) in samples {
                request += &format!(
                    "C {} {} {}\nT {ticks}\n",
                    bits & 1,
                    (bits >> 1) & 1,
                    (bits >> 2) & 1
                );
            }
            let mut child = Command::new(&binary)
                .arg(&executable)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(request.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let native = String::from_utf8(output.stdout).unwrap();
            let fields: Vec<&str> = native.lines().last().unwrap().split_whitespace().collect();
            let value = |i: usize| fields[i].parse::<f32>().unwrap();
            let phase = value(9);
            let pixels = render_reference_frame_pixels(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::QuizController(samples)),
            );
            let mut expected = vec![[0.0f32; 3]; 1920 * 1080];
            for (row, id, left, top, width, height) in [
                (0, 0, 112usize, 292usize, 256usize, 64usize),
                (1, 2, 112, 364, 256, 64),
                (2, 6, 56, 436, 368, 176),
            ] {
                let texture = &atlas.textures
                    [atlas.name_to_index[if id == 6 { "consult" } else { "button" }]];
                let focus = value(10 + row * 2) * 0.5;
                let flash = value(11 + row * 2);
                for y in top..top + height {
                    for x in left..left + width {
                        let u =
                            (x as f32 + 0.5 - left as f32) / if id == 6 { 768.0 } else { 256.0 };
                        let v = (y as f32 + 0.5 - top as f32) / height as f32;
                        let uv = if id == 6 {
                            (u + if phase >= 0.5 { 368.0 / 768.0 } else { 0.0 }, v)
                        } else {
                            (u, (id as f32 + v) / 6.0)
                        };
                        let texel = sample_reference_rgba(
                            texture.dimensions(),
                            texture.as_raw(),
                            uv.0,
                            uv.1,
                        );
                        for c in 0..3 {
                            let focused = texel[c] + (255.0 - texel[c]) * focus;
                            expected[y * 1920 + x][c] =
                                (focused + (255.0 - focused) * flash) * texel[3] / 255.0;
                        }
                    }
                }
            }
            let mut worst = 0.0f32;
            for (i, expected) in expected.iter().enumerate() {
                for c in 0..3 {
                    worst = worst.max((pixels[i * 4 + c] as f32 - expected[c]).abs());
                }
            }
            assert!(
                worst <= 1.1,
                "Quiz {name}: independent native-field/atlas RGB error {worst}"
            );
            eprintln!(
                "Quiz {name}:2073600 native-field/atlas pixels,phase{phase},maxRGBerror{worst}"
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    #[ignore = "requires supplied quiz atlas and Metal GPU"]
    fn reference_completed_quiz_restoration_atlas_focus_gpu() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/quiz.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        for answer in [0, 2, 6] {
            let pixels = render_reference_frame_pixels(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::QuizRestored(answer)),
            );
            let mut worst = 0.0f32;
            let mut count = 0;
            // Bounds/UVs and white-mix weights are derived from native rows.
            for (id, left, top, width, height) in [
                (0, 112, 292, 256, 64),
                (2, 112, 364, 256, 64),
                (6, 56, 436, 368, 176),
            ] {
                let texture = &archive.textures
                    [archive.name_to_index[if id == 6 { "consult" } else { "button" }]];
                let focus = if id == answer {
                    1.0
                } else if id == 0 {
                    0.5
                } else {
                    0.0
                };
                for y in top..top + height {
                    for x in left..left + width {
                        let uv = if id == 6 {
                            (
                                (x as f32 + 0.5 - left as f32) / 768.0,
                                (y as f32 + 0.5 - top as f32) / 176.0,
                            )
                        } else {
                            (
                                (x as f32 + 0.5 - left as f32) / 256.0,
                                (id as f32 + (y as f32 + 0.5 - top as f32) / 64.0) / 6.0,
                            )
                        };
                        let t = sample_reference_rgba(
                            texture.dimensions(),
                            texture.as_raw(),
                            uv.0,
                            uv.1,
                        );
                        for c in 0..3 {
                            let expected = (t[c] + (255.0 - t[c]) * focus * 0.5) * t[3] / 255.0;
                            worst =
                                worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - expected).abs());
                        }
                        count += 1;
                    }
                }
            }
            assert!(worst < 1.1, "Restored answer {answer} RGB error {worst}");
            eprintln!(
                "Restored answer {answer}: {count} independent atlas pixels, max RGB error {worst}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied quiz atlas and GPU"]
    fn reference_quiz_confirmation_white_flash_gpu() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/quiz.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let tex = &archive.textures[archive.name_to_index["consult"]];
        for (samples, flash) in [
            (vec![0.0], 0.0),
            (vec![1.0], 0.2),
            (vec![3.5], 0.7),
            (vec![5.25], 0.35),
            (vec![7.0], 0.0),
            (vec![7.0, 0.0], 0.0),
        ] {
            let pixels = render_reference_frame_pixels(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::QuizConfirmation(&samples)),
            );
            let mut worst = 0.0f32;
            for y in 436..612usize {
                for x in 56..424usize {
                    let texel = sample_reference_rgba(
                        tex.dimensions(),
                        tex.as_raw(),
                        (x as f32 + 0.5 - 56.0) / 768.0,
                        (y as f32 + 0.5 - 436.0) / 176.0,
                    );
                    for c in 0..3 {
                        let focus = texel[c] + (255.0 - texel[c]) * 0.5;
                        let expected = (focus + (255.0 - focus) * flash) * texel[3] / 255.0;
                        worst = worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - expected).abs());
                    }
                }
            }
            assert!(
                worst <= 2.0,
                "Quiz confirmation samples{samples:?} flash{flash}: RGB error{worst}"
            );
            eprintln!("Quiz confirmation flash{flash},64768 pixels, maxRGBerror{worst}");
        }
    }

    #[test]
    fn native_quiz_sounds_require_selection_change_or_valid_confirmation() {
        let mut model = QuizModel::new(1 << 6);
        model.move_selection(1);
        assert!(model.take_sounds().is_empty());
        model.confirm();
        model.confirm();
        assert_eq!(model.take_sounds(), ["decide"]);
        let mut model = QuizModel::new(5);
        model.move_selection(1);
        assert_eq!(model.take_sounds(), ["move"]);
        let r = model.rect(0);
        model.pointer(Vec2::new(r.x + 10.0, r.y + 10.0), false);
        assert!(model.take_sounds().is_empty());
        model.pointer(Vec2::new(r.x + 10.0, r.y + 10.0), true);
        assert_eq!(model.take_sounds(), ["decide"]);
        let mut empty = QuizModel::new(0);
        empty.confirm();
        assert!(empty.take_sounds().is_empty());
    }
    #[test]
    fn masked_answers_keep_script_index_and_consultation_is_six() {
        let mut quiz = QuizModel::new(0b1010010);
        assert_eq!(quiz.answers, [1, 4, 6]);
        quiz.move_selection(-1);
        quiz.confirm();
        quiz.update(shin_core::time::Ticks::from_f32(7.0));
        quiz.update(shin_core::time::Ticks::ZERO);
        assert_eq!(quiz.result, Some(6));
        quiz.move_selection(1);
        quiz.confirm();
        assert_eq!(quiz.result, Some(6));
        let mut empty = QuizModel::new(0);
        empty.confirm();
        assert_eq!(empty.result, None);
    }
    #[test]
    fn pointer_uses_packed_visible_rows_and_ignores_outside_clicks() {
        let mut quiz = QuizModel::new(0b10100);
        quiz.pointer(Vec2::new(240.0, 370.0), true);
        assert_eq!(quiz.result, None);
        quiz.update(shin_core::time::Ticks::from_f32(7.0));
        quiz.update(shin_core::time::Ticks::ZERO);
        assert_eq!(quiz.result, Some(4));
        let mut quiz = QuizModel::new(0b10100);
        quiz.pointer(Vec2::new(900.0, 370.0), true);
        assert_eq!(quiz.result, None);
        assert_eq!(quiz.rect(1), vec4(112.0, 364.0, 256.0, 64.0));
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_renders_with_supplied_assets() {
        render_reference_interaction(None, None, None);
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_murderer_accusation_renders_with_supplied_assets() {
        render_reference_interaction(Some(MurdererMenu::Accusation), None, None);
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_murderer_menus_render_with_supplied_assets() {
        for menu in [
            MurdererMenu::Top,
            MurdererMenu::Replay,
            MurdererMenu::Rules,
            MurdererMenu::Hints,
            MurdererMenu::PurpleByPerson,
            MurdererMenu::PurpleByChapter,
        ] {
            render_reference_interaction(Some(menu), None, None);
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_final_choice_renders_with_supplied_assets() {
        for choice in [None, Some(0), Some(1)] {
            render_reference_interaction(None, Some(choice), None);
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_final_slide_renders_with_supplied_assets() {
        render_reference_interaction(None, Some(Some(0)), Some(17.0));
        render_reference_interaction(None, Some(Some(1)), Some(17.0));
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_accusation_white_flash_renders_with_supplied_assets() {
        render_reference_interaction(Some(MurdererMenu::Accusation), None, Some(15.0));
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_replay_expansion_renders_with_supplied_assets() {
        for ticks in [0.0, 30.0, 60.0] {
            render_reference_interaction(Some(MurdererMenu::Replay), None, Some(ticks));
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_replay_feedback_renders_with_supplied_assets() {
        for ticks in [0.0, 3.5, 7.0] {
            render_reference_frame(Some(MurdererMenu::Replay), None, None, Some(ticks));
        }
    }
    #[test]
    #[ignore = "requires supplied ROM, executable and Metal GPU"]
    fn reference_murderer_browser_panels_silhouette_and_scrollbar_gpu() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let rom_path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&rom_path).parent().unwrap();
        for (name, delta, rodata) in [
            ("uncompressed_executable", 0usize, 0usize),
            ("uncompressed_executable_update", 0xa0, 0xfe0),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
            for (a, v) in [
                (0x9528c, 0x1e2d900f),
                (0x95e28, 0x1e2a100a),
                (0x96158, 0x1e2a1003),
                (0x94768, 0x34000b08),
                (0x98b28, 0xb981a268),
                (0x9878c, 0xb901a268),
                (0x9884c, 0xb901a268),
            ] {
                assert_eq!(word(a - delta), v, "{name} instruction{a:x}");
            }
            for (a, v) in [
                (0x1597b0, 130.0f32),
                (0x159460, 690.0),
                (0x159848, 1762.0),
                (0x159600, 654.0),
                (0x1593ac, 1088.0),
                (0x15952c, 2048.0),
                (0x159ab4, 72.0),
                (0x1594e4, 1152.0),
                (0x1594ac, 1160.0),
                (0x159af4, 272.0),
                (0x1595b8, 0.58),
                (0x159604, 0.36),
            ] {
                assert_eq!(word(a + rodata), v.to_bits(), "{name} constant{a:x}");
            }
        }
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(&rom_path).unwrap()).unwrap())
                .unwrap();
        let mut archive = |path: &str| {
            let mut b = Vec::new();
            rom.open_file(rom.find_file(path).unwrap())
                .unwrap()
                .read_to_end(&mut b)
                .unwrap();
            shin_core::format::texture_archive::read_texture_archive(&b).unwrap()
        };
        let murderer = archive("/murderer.txa");
        let character = archive("/chars/m_kla.txa");
        let systex = archive("/systex.txa");
        let right = &murderer.textures[murderer.name_to_index["right2"]];
        let left = &murderer.textures[murderer.name_to_index["left"]];
        let silhouette = &character.textures[0];
        let scroll = &systex.textures[systex.name_to_index["scroll"]];
        let battler = render_reference_frame_pixels(
            Some(MurdererMenu::Accusation),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let bg = &murderer.textures[murderer.name_to_index["different_space_1a"]];
        for (name, rect, area) in [
            (
                "But_Majime3",
                vec4(1182.0, 0.0, 384.0, 384.0),
                (1200..1550, 20..170),
            ),
            (
                "But_BodyA",
                vec4(928.0, 384.0, 812.0, 696.0),
                (1200..1700, 400..850),
            ),
        ] {
            let tex = &murderer.textures[murderer.name_to_index[name]];
            let mut worst = 0.0f32;
            for y in area.1 {
                for x in area.0.clone() {
                    let background = sample_reference_rgba(
                        bg.dimensions(),
                        bg.as_raw(),
                        (x as f32 + 0.5) / 1920.0,
                        (y as f32 + 0.5) / 1080.0,
                    );
                    let t = sample_reference_rgba(
                        tex.dimensions(),
                        tex.as_raw(),
                        (x as f32 + 0.5 - rect.x) / rect.z,
                        (y as f32 + 0.5 - rect.y) / rect.w,
                    );
                    for c in 0..3 {
                        worst = worst.max(
                            (battler[(y * 1920 + x) * 4 + c] as f32
                                - (t[c] * t[3] / 255.0 + background[c] * (1.0 - t[3] / 255.0)))
                                .abs(),
                        );
                    }
                }
            }
            assert!(worst < 2.0, "Battler{name} RGB error{worst}");
        }
        for menu in [MurdererMenu::PurpleByPerson, MurdererMenu::PurpleByChapter] {
            let pixels = render_reference_frame_pixels(
                Some(menu),
                None,
                None,
                None,
                None,
                None,
                (menu == MurdererMenu::PurpleByChapter)
                    .then_some(MurdererFeedback::PurpleHeadingScroll(0.0)),
            );
            let mut checked = 0;
            let mut worst = 0.0f32;
            for y in 200..880 {
                for x in 134..160 {
                    let v = if y < 564 {
                        (y as f32 + 0.5 - 164.0) / 400.0 * 25.0 / 28.0
                    } else {
                        25.0 / 28.0 + (y as f32 + 0.5 - 564.0) / 336.0 / 28.0
                    };
                    let t = sample_reference_rgba(
                        left.dimensions(),
                        left.as_raw(),
                        (x as f32 + 0.5 - 130.0) / 336.0 * 0.875,
                        v,
                    );
                    if t[3] > 254.99 {
                        for c in 0..3 {
                            worst = worst.max((pixels[(y * 1920 + x) * 4 + c] as f32 - t[c]).abs());
                        }
                        checked += 1;
                    }
                }
            }
            assert!(
                checked > 10000 && worst < 2.0,
                "Left panel{menu:?}: {checked} pixels error{worst}"
            );
            let scene = &murderer.textures[murderer.name_to_index["different_space_2d"]];
            let decoration = if menu == MurdererMenu::PurpleByPerson {
                silhouette
            } else {
                &murderer.textures[murderer.name_to_index["hana"]]
            };
            let (rect, tint) = if menu == MurdererMenu::PurpleByPerson {
                (vec4(1088.0, 72.0, 960.0, 1080.0), [255u8, 255, 255])
            } else {
                (vec4(1160.0, 272.0, 760.0, 880.0), [147u8, 91, 0])
            };
            let mut outside_worst = 0.0f32;
            for y in 940..1080 {
                for x in 1200..1740 {
                    let bg = sample_reference_rgba(
                        scene.dimensions(),
                        scene.as_raw(),
                        (x as f32 + 0.5) / 1920.0,
                        (y as f32 + 0.5) / 1080.0,
                    );
                    let t = sample_reference_rgba(
                        decoration.dimensions(),
                        decoration.as_raw(),
                        (x as f32 + 0.5 - rect.x) / rect.z,
                        (y as f32 + 0.5 - rect.y) / rect.w,
                    );
                    let a = t[3] / 255.0 * 63.0 / 255.0;
                    for c in 0..3 {
                        outside_worst = outside_worst.max(
                            (pixels[(y * 1920 + x) * 4 + c] as f32
                                - (t[c] * tint[c] as f32 / 255.0 * a + bg[c] * (1.0 - a)))
                                .abs(),
                        );
                    }
                }
            }
            assert!(
                outside_worst < 1.1,
                "{menu:?} image must extend below panel:75600pixels RGB error{outside_worst}"
            );
            eprintln!(
                "Native{menu:?} full image extent:75600pixels below panel, maxRGB{outside_worst}"
            );
            if menu == MurdererMenu::PurpleByPerson {
                let mut worst = 0.0f32;
                for y in 450..880 {
                    for x in 1200..1740 {
                        let bg = sample_reference_rgba(
                            right.dimensions(),
                            right.as_raw(),
                            0.25 + (x as f32 + 0.5 - 706.0) / 1068.0 * 0.25,
                            0.25 + (y as f32 + 0.5 - 180.0) / 720.0 * 0.25,
                        );
                        let t = sample_reference_rgba(
                            silhouette.dimensions(),
                            silhouette.as_raw(),
                            (x as f32 + 0.5 - 1088.0) / 960.0,
                            (y as f32 + 0.5 - 72.0) / 1080.0,
                        );
                        let alpha = t[3] / 255.0 * 63.0 / 255.0;
                        for c in 0..3 {
                            worst = worst.max(
                                (pixels[(y * 1920 + x) * 4 + c] as f32
                                    - (t[c] * alpha + bg[c] * (1.0 - alpha)))
                                    .abs(),
                            );
                        }
                    }
                }
                assert!(
                    worst < 2.0,
                    "Native25% silhouette:232200 pixels RGB error{worst}"
                );
            } else {
                // Opaque decoded thumb texels at the top of the chapter list.
                let end = 188.0 + 704.0 * 720.0 / 2610.0;
                let mut checked = 0;
                for y in 190..end as usize {
                    for x in 654..670 {
                        let t = sample_reference_rgba(
                            scroll.dimensions(),
                            scroll.as_raw(),
                            0.5 + (x as f32 + 0.5 - 654.0) / 16.0 * 0.5,
                            0.25 + (y as f32 + 0.5 - 188.0) / (end - 188.0) * 0.25,
                        );
                        if t[3] > 254.99 {
                            for c in 0..3 {
                                assert!((pixels[(y * 1920 + x) * 4 + c] as f32 - t[c]).abs() < 2.0);
                            }
                            checked += 1;
                        }
                    }
                }
                assert!(checked > 500, "Chapter scrollbar is missing");
            }
        }
    }
    fn sample_reference_rgba((width, height): (u32, u32), data: &[u8], u: f32, v: f32) -> [f32; 4] {
        let x = (u * width as f32 - 0.5).clamp(0.0, width as f32 - 1.0);
        let y = (v * height as f32 - 0.5).clamp(0.0, height as f32 - 1.0);
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = (x0 + 1).min(width - 1);
        let y1 = (y0 + 1).min(height - 1);
        std::array::from_fn::<_, 4, _>(|channel| {
            let a = data[((y0 * width + x0) * 4) as usize + channel] as f32
                * (1.0 - (x - x0 as f32))
                + data[((y0 * width + x1) * 4) as usize + channel] as f32 * (x - x0 as f32);
            let b = data[((y1 * width + x0) * 4) as usize + channel] as f32
                * (1.0 - (x - x0 as f32))
                + data[((y1 * width + x1) * 4) as usize + channel] as f32 * (x - x0 as f32);
            a * (1.0 - (y - y0 as f32)) + b * (y - y0 as f32)
        })
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_purple_sticky_headings() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/murderer.txa").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let texture = &archive.textures[archive.name_to_index["chapter"]];
        let render = |scroll| {
            render_reference_frame_pixels(
                Some(MurdererMenu::PurpleByChapter),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::PurpleHeadingScroll(scroll)),
            )
        };
        let baseline = render(0.0);
        // Hand-reviewed outgoing/incoming header y positions at three first
        // boundary phases and two second-boundary phases, independent of model.
        for (scroll, headers) in [
            (450.0, vec![(0, 180.0f32), (1, 270.0)]),
            (495.0, vec![(0, 135.0), (1, 225.0)]),
            (540.0, vec![(1, 180.0)]),
            (1125.0, vec![(1, 180.0), (2, 315.0)]),
            (1215.0, vec![(1, 135.0), (2, 225.0)]),
        ] {
            let actual = render(scroll);
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            for (heading, y0) in headers {
                for y in y0.max(180.0) as usize..(y0 + 88.0).min(900.0) as usize {
                    for x in 166..646 {
                        let u = (x as f32 + 0.5 - 166.0) / 480.0;
                        let v = (heading as f32 * 88.0 + y as f32 + 0.5 - y0) / 528.0;
                        let texel =
                            sample_reference_rgba(texture.dimensions(), texture.as_raw(), u, v);
                        if texel[3] < 254.99 {
                            continue;
                        }
                        let pixel = &actual[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                        for channel in 0..3 {
                            worst = worst.max((pixel[channel] as f32 - texel[channel]).abs());
                        }
                        assert_eq!(pixel[3], 255);
                        checked += 1;
                    }
                }
            }
            assert!(checked > 1000);
            assert!(
                worst <= 1.1,
                "Sticky headings at{scroll}: {checked} decoded texels, RGB error{worst}"
            );
            for (index, (pixel, old)) in actual
                .chunks_exact(4)
                .zip(baseline.chunks_exact(4))
                .enumerate()
            {
                let x = index % 1920;
                let y = index / 1920;
                if !(166..646).contains(&x) || !(180..900).contains(&y) {
                    assert_eq!(pixel, old, "scroll changed outside chapter viewport");
                }
            }
            eprintln!(
                "Sticky headings at{scroll}: {checked} decoded texels, RGB error{worst}, outside viewport unchanged"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_purple_chapter_pointer_feedback() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/murderer.txa").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let tex = |name: &str| &archive.textures[archive.name_to_index[name]];
        let row = Vec2::new(200.0, 390.0);
        for (case, (samples, focus1, focus2, flash2)) in [
            (vec![(row, true, 2.0)], 0.8f32, 0.5f32, 0.4f32),
            (vec![(row, true, 10.0), (row, false, 1.75)], 0.0, 1.0, 0.35),
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::PurpleByChapter),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::PointerAt(&samples)),
            );
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            for (original, focus, flash) in [(1, focus1, 0.0), (2, focus2, flash2)] {
                let y0 = 180.0 + original as f32 * 90.0;
                for y in y0 as usize..(y0 + 88.0) as usize {
                    for x in 166..646 {
                        let u = (x as f32 + 0.5 - 166.0) / 480.0;
                        let plate = tex("chap_menu");
                        let mut expected = sample_reference_rgba(
                            plate.dimensions(),
                            plate.as_raw(),
                            u,
                            (y as f32 + 0.5 - y0) / 88.0,
                        );
                        if expected[3] < 254.99 {
                            continue;
                        }
                        let mix = |value: f32| {
                            let value = value + (255.0 - value) * (focus * 0.5);
                            value + (255.0 - value) * flash
                        };
                        for c in 0..3 {
                            expected[c] = mix(expected[c]).round();
                        }
                        if y as f32 + 0.5 >= y0 + 12.0 && y as f32 + 0.5 < y0 + 76.0 {
                            let text = tex("chap_text");
                            let glyph = sample_reference_rgba(
                                text.dimensions(),
                                text.as_raw(),
                                u,
                                ((original - 1) as f32 * 64.0 + y as f32 + 0.5 - y0 - 12.0)
                                    / 1472.0,
                            );
                            for c in 0..3 {
                                expected[c] = (expected[c] * (1.0 - glyph[3] / 255.0)
                                    + mix(glyph[c]) * glyph[3] / 255.0)
                                    .round();
                            }
                        }
                        let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                        for c in 0..3 {
                            worst = worst.max((actual[c] as f32 - expected[c]).abs());
                        }
                        assert_eq!(actual[3], 255);
                        checked += 1;
                    }
                }
            }
            assert!(checked > 1000);
            assert!(
                worst <= 2.1,
                "Chapter rows{case}: {checked} decoded samples error{worst}"
            );
            eprintln!("Chapter rows{case}: {checked} decoded plate/text samples error{worst}");
        }
        let point = Vec2::new(200.0, 200.0);
        for (case, (samples, y0, flash)) in [
            (vec![(point, true, 2.0)], 135.0f32, 0.4f32),
            (vec![(point, true, 10.0), (point, false, 1.75)], 180.0, 0.35),
            (
                vec![(point, true, 2.0), (point + Vec2::Y * 40.0, true, 1.0)],
                175.0,
                0.2,
            ),
            (
                vec![
                    (point, true, 2.0),
                    (point + Vec2::Y * 40.0, true, 1.0),
                    (point + Vec2::Y * 40.0, false, 4.0),
                ],
                175.0,
                0.0,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::PurpleByChapter),
                None,
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::PurpleHeadingPointer(495.0, &samples)),
            );
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            let header = tex("chapter");
            for y in y0.max(180.0) as usize..(y0 + 88.0) as usize {
                for x in 166..646 {
                    let sample = sample_reference_rgba(
                        header.dimensions(),
                        header.as_raw(),
                        (x as f32 + 0.5 - 166.0) / 480.0,
                        (y as f32 + 0.5 - y0) / 528.0,
                    );
                    if sample[3] < 254.99 {
                        continue;
                    }
                    let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                    for c in 0..3 {
                        worst = worst.max(
                            (actual[c] as f32 - (sample[c] + (255.0 - sample[c]) * flash)).abs(),
                        );
                    }
                    assert_eq!(actual[3], 255);
                    checked += 1;
                }
            }
            assert!(checked > 1000);
            assert!(
                worst <= 1.1,
                "Heading{case}: {checked} decoded samples error{worst}"
            );
            eprintln!("Heading{case}: {checked} decoded texture samples error{worst}");
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_purple_person_focus_and_pointer_feedback() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/murderer.txa").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let texture = |name: &str| &archive.textures[archive.name_to_index[name]];
        let names = [
            "kla0", "nat0", "jes0", "eva0", "hid0", "geo0", "rud0", "kir0", "but0", "ros0", "mar1",
            "nan0", "gen0", "sha0", "kan0", "goh0", "kum0",
        ];
        let point = Vec2::new(440.0, 210.0);
        for (case, (feedback, focus0, focus1, flash1)) in [
            (
                MurdererFeedback::PurplePersonFocus(2.0),
                0.8f32,
                0.5f32,
                0.0f32,
            ),
            (
                MurdererFeedback::PointerAt(&[(point, true, 2.0)]),
                0.8,
                0.5,
                0.4,
            ),
            (
                MurdererFeedback::PointerAt(&[(point, true, 10.0), (point, false, 1.75)]),
                0.0,
                1.0,
                0.35,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::PurpleByPerson),
                None,
                None,
                None,
                None,
                None,
                Some(feedback),
            );
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            for index in 0..17 {
                let rect = super::super::murderer_model::purple_person_rect(index).unwrap();
                let (focus, flash) = match index {
                    0 => (focus0, 0.0),
                    1 => (focus1, flash1),
                    _ => (0.0, 0.0),
                };
                for y in rect.y.ceil() as usize..(rect.y + rect.w).floor() as usize {
                    for x in rect.x.ceil() as usize..(rect.x + rect.z).floor() as usize {
                        let uv = (
                            (x as f32 + 0.5 - rect.x) / rect.z,
                            (y as f32 + 0.5 - rect.y) / rect.w,
                        );
                        let base = texture("non");
                        let mut expected =
                            sample_reference_rgba(base.dimensions(), base.as_raw(), uv.0, uv.1);
                        if expected[3] < 254.99 {
                            continue;
                        }
                        let portrait = texture(names[index]);
                        let texel = sample_reference_rgba(
                            portrait.dimensions(),
                            portrait.as_raw(),
                            uv.0,
                            uv.1,
                        );
                        for channel in 0..3 {
                            expected[channel] = (expected[channel].round()
                                * (1.0 - texel[3] / 255.0)
                                + texel[channel]
                                    * if [3, 4, 6, 7, 9, 12].contains(&index) && channel != 0 {
                                        15.0 / 255.0
                                    } else {
                                        1.0
                                    }
                                    * texel[3]
                                    / 255.0)
                                .round();
                        }
                        for amount in [focus * 0.5, flash] {
                            let alpha = (amount * 255.0) as u8 as f32 / 255.0;
                            for channel in 0..3 {
                                expected[channel] = (expected[channel]
                                    + (255.0 - expected[channel]) * alpha)
                                    .round();
                            }
                        }
                        let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                        for channel in 0..3 {
                            worst = worst.max((actual[channel] as f32 - expected[channel]).abs());
                        }
                        assert_eq!(actual[3], 255);
                        checked += 1;
                    }
                }
            }
            assert!(checked > 20000);
            assert!(
                worst <= 2.1,
                "Purple person{case}: {checked} independently decoded samples, RGB error{worst}"
            );
            eprintln!("Purple person{case}: {checked} decoded portrait samples, RGB error{worst}");
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_accusation_focus_and_pointer_feedback() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/murderer.txa").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let texture = |name: &str| &archive.textures[archive.name_to_index[name]];
        let names = [
            "kla0", "nat0", "jes0", "eva0", "hid0", "geo0", "rud0", "kir0", "but0", "ros0", "mar1",
            "nan0", "gen0", "sha0", "kan0", "goh0", "kum0",
        ];
        let person = Vec2::new(580.0, 240.0);
        let submit = Vec2::new(500.0, 850.0);
        for (case, (feedback, focus28, focus29, flash29, selected29, focus45, flash45)) in [
            (
                MurdererFeedback::AccusationFocus(2.0),
                0.8f32,
                0.5f32,
                0.0f32,
                false,
                0.0f32,
                0.0f32,
            ),
            (
                MurdererFeedback::PointerAt(&[(person, true, 2.0)]),
                0.8,
                0.5,
                0.4,
                false,
                0.0,
                0.0,
            ),
            (
                MurdererFeedback::PointerAt(&[(person, true, 10.0)]),
                0.0,
                1.0,
                0.7,
                false,
                0.0,
                0.0,
            ),
            (
                MurdererFeedback::PointerAt(&[(person, true, 10.0), (person, false, 1.75)]),
                0.0,
                1.0,
                0.35,
                true,
                0.0,
                0.0,
            ),
            (
                MurdererFeedback::PointerAt(&[(submit, true, 10.0)]),
                0.0,
                0.0,
                0.0,
                false,
                1.0,
                0.7,
            ),
            (
                MurdererFeedback::PointerAt(&[(submit, true, 10.0), (submit, false, 1.75)]),
                0.0,
                0.0,
                0.0,
                false,
                1.0,
                0.35,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::Accusation),
                None,
                None,
                None,
                None,
                None,
                Some(feedback),
            );
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            for entry in 28..=45 {
                let rect = super::super::murderer_model::accusation_rect(entry).unwrap();
                let (focus, flash) = match entry {
                    28 => (focus28, 0.0),
                    29 => (focus29, flash29),
                    45 => (focus45, flash45),
                    _ => (0.0, 0.0),
                };
                for y in rect.y.ceil() as usize..(rect.y + rect.w).floor() as usize {
                    for x in rect.x.ceil() as usize..(rect.x + rect.z).floor() as usize {
                        let uv = (
                            (x as f32 + 0.5 - rect.x) / rect.z,
                            (y as f32 + 0.5 - rect.y) / rect.w,
                        );
                        let base = texture(if entry == 45 { "tokutei" } else { "non" });
                        let mut expected =
                            sample_reference_rgba(base.dimensions(), base.as_raw(), uv.0, uv.1);
                        if expected[3] < 254.99 {
                            continue;
                        }
                        if entry != 45 {
                            let portrait = texture(names[entry - 28]);
                            let mut overlays = vec![portrait];
                            if entry == 29 && selected29 {
                                overlays.push(texture("murderer"));
                            }
                            for overlay in overlays {
                                let texel = sample_reference_rgba(
                                    overlay.dimensions(),
                                    overlay.as_raw(),
                                    uv.0,
                                    uv.1,
                                );
                                for channel in 0..3 {
                                    expected[channel] = (expected[channel].round()
                                        * (1.0 - texel[3] / 255.0)
                                        + texel[channel] * texel[3] / 255.0)
                                        .round();
                                }
                            }
                        }
                        for amount in [focus * 0.5, flash] {
                            let alpha = if entry == 45 {
                                amount
                            } else {
                                (amount * 255.0) as u8 as f32 / 255.0
                            };
                            for channel in 0..3 {
                                expected[channel] =
                                    expected[channel] + (255.0 - expected[channel]) * alpha;
                                if entry != 45 {
                                    expected[channel] = expected[channel].round();
                                }
                            }
                        }
                        let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                        for channel in 0..3 {
                            worst = worst.max((actual[channel] as f32 - expected[channel]).abs());
                        }
                        assert_eq!(actual[3], 255);
                        checked += 1;
                    }
                }
            }
            assert!(checked > 20000);
            assert!(
                worst <= 2.1,
                "Accusation feedback{case}: {checked} independent decoded samples, RGB error{worst}"
            );
            eprintln!(
                "Accusation feedback{case}: {checked} decoded portrait/Submit samples, RGB error{worst}"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_hint_focus_and_pointer_feedback() {
        crate::init_test_task_pools();
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/murderer.txa").unwrap();
        let mut data = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let plate = &archive.textures[archive.name_to_index["hint_menu"]];
        let text = &archive.textures[archive.name_to_index["hint_text"]];
        let point = Vec2::new(330.0, 820.0);
        let origin = Vec2::new(300.0, 400.0);
        let dragged = origin + Vec2::Y * 40.0;
        for (feedback, focus19, focus20, flash20, scroll_override, focus18) in [
            (
                MurdererFeedback::HintFocus(2.0),
                0.5,
                0.8,
                0.0f32,
                None,
                0.0,
            ),
            (MurdererFeedback::HintScroll(2.0), 0.0, 0.0, 0.0, None, 0.0),
            (
                MurdererFeedback::PointerAt(&[(point, true, 2.0)]),
                0.0,
                1.0,
                0.4,
                None,
                0.0,
            ),
            (
                MurdererFeedback::PointerAt(&[(point, true, 10.0), (point, false, 1.75)]),
                0.0,
                1.0,
                0.35,
                None,
                0.0,
            ),
            (
                MurdererFeedback::HintAxis(-0.5),
                0.0,
                1.0,
                0.0,
                Some(1154.0),
                0.0,
            ),
            (
                MurdererFeedback::HintTimed(&[
                    (origin, true, 0.0),
                    (dragged, true, 0.0),
                    (dragged, false, 2.5),
                ]),
                0.625,
                0.75,
                0.0,
                Some(1030.0),
                0.0,
            ),
            (
                MurdererFeedback::HintTimed(&[
                    (origin, true, 0.0),
                    (dragged, true, 0.0),
                    (dragged, false, 2.5),
                    (Vec2::new(400.0, 200.0), true, 1.0),
                    (Vec2::new(400.0, 200.0), false, 0.0),
                ]),
                0.525,
                0.65,
                0.0,
                Some(1030.0),
                0.25,
            ),
        ] {
            let pixels = render_reference_frame_pixels(
                Some(MurdererMenu::Hints),
                None,
                None,
                None,
                Some(&[7.0, 0.0, 30.0]),
                None,
                Some(feedback),
            );
            let mut checked = 0;
            let mut glyphs = 0;
            let mut worst = 0.0f32;
            let scrolling = matches!(feedback, MurdererFeedback::HintScroll(_));
            let scroll = scroll_override.unwrap_or(if scrolling { 1125.0 } else { 1170.0 });
            for row in (scroll / 90.0) as usize..21 {
                let top = 180.0 + row as f32 * 90.0 - scroll;
                let focus = if scrolling {
                    match row {
                        12 => 0.5,
                        13 => 0.8,
                        14 => 0.4,
                        _ => 0.0,
                    }
                } else if row == 18 {
                    focus18
                } else if row == 19 {
                    focus19
                } else if row == 20 {
                    focus20
                } else {
                    0.0
                };
                let flash = if row == 20 { flash20 } else { 0.0 };
                for y in
                    (top.max(180.0) as usize + 1..(top + 88.0).min(900.0) as usize - 1).step_by(2)
                {
                    for x in (321usize..1599).step_by(2) {
                        let yf = y as f32 + 0.5;
                        let xf = x as f32 + 0.5;
                        let base = sample_reference_rgba(
                            plate.dimensions(),
                            plate.as_raw(),
                            (xf - 320.0) / 1280.0,
                            (yf - top) / 88.0,
                        );
                        if base[3] < 254.99 {
                            continue;
                        }
                        let label =
                            if xf >= 352.0 && xf < 1568.0 && yf >= top + 13.0 && yf < top + 77.0 {
                                let atlas_row = if row == 20 { 0 } else { row + 1 };
                                sample_reference_rgba(
                                    text.dimensions(),
                                    text.as_raw(),
                                    (xf - 352.0) / 1216.0,
                                    (atlas_row as f32 * 64.0 + yf - top - 13.0) / 1408.0,
                                )
                            } else {
                                [0.0; 4]
                            };
                        let white_mix = |value: f32| {
                            let first = value + (255.0 - value) * focus * 0.5;
                            first + (255.0 - first) * flash
                        };
                        let actual = &pixels[(y * 1920 + x) * 4..][..4];
                        for channel in 0..3 {
                            let expected = white_mix(base[channel]) * (1.0 - label[3] / 255.0)
                                + white_mix(label[channel]) * label[3] / 255.0;
                            worst = worst.max((actual[channel] as f32 - expected).abs());
                        }
                        glyphs += usize::from(label[3] > 1.0);
                        checked += 1;
                        assert_eq!(actual[3], 255);
                    }
                }
            }
            assert!(checked > 100000 && glyphs > 1000);
            assert!(worst <= 2.0, "Hint{feedback:?}: RGB error{worst}");
            eprintln!(
                "Hint{feedback:?}: {checked} decoded plate/text samples including{glyphs} glyph pixels, RGB error{worst}"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_replay_and_back_pointer_flash() {
        crate::init_test_task_pools();
        let render = |samples: &[(Vec2, bool, f32)]| {
            render_reference_frame_pixels(
                Some(MurdererMenu::Replay),
                None,
                None,
                None,
                Some(&[7.0, 0.0, 30.0]),
                None,
                Some(MurdererFeedback::PointerAt(samples)),
            )
        };
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/murderer.txa").unwrap();
        let mut data = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
        let source = render(&[]);
        for point in [Vec2::new(130.0, 200.0), Vec2::new(1700.0, 60.0)] {
            let rect = if point.x > 1600.0 {
                super::super::murderer_model::BACK_RECT
            } else {
                super::super::murderer_model::replay_rect(0).unwrap()
            };
            let held = render(&[(point, true, 10.0)]);
            let released = render(&[(point, true, 10.0), (point, false, 1.75)]);
            let texture = &archive.textures[archive.name_to_index[if point.x > 1600.0 {
                "back"
            } else {
                "thumb_1_s"
            }]];
            let additive = if point.x > 1600.0 { 0.0 } else { 128.0 };
            let mut checked = 0;
            let mut changing = 0;
            let mut worst = 0.0f32;
            for (index, ((actual, old), peak)) in released
                .chunks_exact(4)
                .zip(source.chunks_exact(4))
                .zip(held.chunks_exact(4))
                .enumerate()
            {
                let x = (index % 1920) as f32 + 0.5;
                let y = (index / 1920) as f32 + 0.5;
                let in_rect =
                    x >= rect.x && x < rect.x + rect.z && y >= rect.y && y < rect.y + rect.w;
                if in_rect {
                    let texel = sample_reference_rgba(
                        texture.dimensions(),
                        texture.as_raw(),
                        (x - rect.x) / rect.z,
                        (y - rect.y) / rect.w,
                    );
                    if texel[3] >= 254.99 {
                        for channel in 0..3 {
                            for (pixel, brightness) in [(peak, 0.7f32), (actual, 0.35)] {
                                let expected = (texel[channel]
                                    + (255.0 - texel[channel]) * brightness
                                    + additive)
                                    .min(255.0);
                                worst = worst.max((pixel[channel] as f32 - expected).abs());
                            }
                        }
                        checked += 1;
                    }
                } else {
                    assert_eq!(actual, old, "release changed outside pointer rectangle");
                    assert_eq!(peak, old, "hold changed outside pointer rectangle");
                }
                changing += usize::from(old[..3] != peak[..3]);
                assert_eq!(actual[3], 255);
            }
            assert!(changing > 1000);
            assert!(checked > 10000);
            assert!(
                worst <= 2.0,
                "Replay/Back pointer{point:?}: RGB error{worst}"
            );
            eprintln!(
                "Replay/Back pointer{point:?}: {changing} changing pixels, {checked} decoded opaque texels at peak/release, RGB error{worst}"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_top_pointer_hold_and_release_flash() {
        let render = |samples: &[(bool, f32)]| {
            render_reference_frame_pixels(
                Some(MurdererMenu::Top),
                None,
                None,
                None,
                Some(&[]),
                None,
                Some(MurdererFeedback::Pointer(samples)),
            )
        };
        let source = render(&[]);
        for (samples, brightness) in [
            (vec![(true, 2.0)], 0.4f32),
            (vec![(true, 10.0)], 0.7),
            (vec![(true, 2.0), (false, 2.0)], 0.6),
            (vec![(true, 10.0), (false, 1.75)], 0.35),
        ] {
            let actual = render(&samples);
            let rect = super::super::murderer_model::top_menu_rect(0).unwrap();
            // The white quad uses a quantized alpha after plate/text draws.
            let alpha = (brightness * 255.0) as u8 as f32 / 255.0;
            let mut worst = 0.0f32;
            for (index, (actual, old)) in actual
                .chunks_exact(4)
                .zip(source.chunks_exact(4))
                .enumerate()
            {
                let point = Vec2::new((index % 1920) as f32 + 0.5, (index / 1920) as f32 + 0.5);
                let amount = if point.x >= rect.x
                    && point.x < rect.x + rect.z
                    && point.y >= rect.y
                    && point.y < rect.y + rect.w
                {
                    alpha
                } else {
                    0.0
                };
                for channel in 0..3 {
                    let expected = old[channel] as f32 * (1.0 - amount) + 255.0 * amount;
                    worst = worst.max((actual[channel] as f32 - expected).abs());
                }
                assert_eq!(actual[3], 255);
            }
            assert!(worst <= 1.5, "Top pointer{samples:?}: RGB error{worst}");
            eprintln!("Top pointer{samples:?}:2073600 pixels, RGB error{worst}");
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_top_focus_interpolates_supplied_plates() {
        let render = |ticks| {
            render_reference_frame_pixels(
                Some(MurdererMenu::Top),
                None,
                None,
                None,
                Some(&[]),
                None,
                Some(MurdererFeedback::Focus(ticks)),
            )
        };
        let start = render(0.0);
        let end = render(10.0);
        for ticks in [2.0f32, 4.0, 7.0] {
            let actual = render(ticks);
            let mut worst = 0.0f32;
            let mut changing = 0;
            for (index, ((actual, start), end)) in actual
                .chunks_exact(4)
                .zip(start.chunks_exact(4))
                .zip(end.chunks_exact(4))
                .enumerate()
            {
                let point = Vec2::new((index % 1920) as f32 + 0.5, (index / 1920) as f32 + 0.5);
                let row = (0..2).find(|&row| {
                    let rect = super::super::murderer_model::top_menu_rect(row).unwrap();
                    point.x >= rect.x
                        && point.x < rect.x + rect.z
                        && point.y >= rect.y
                        && point.y < rect.y + rect.w
                });
                let amount = match row {
                    Some(0) => (ticks * 0.1).min(1.0),
                    Some(1) => (ticks * 0.25).min(1.0),
                    _ => 0.0,
                };
                for channel in 0..3 {
                    let expected = start[channel] as f32
                        + (end[channel] as f32 - start[channel] as f32) * amount;
                    worst = worst.max((actual[channel] as f32 - expected).abs());
                }
                changing += usize::from(start[..3] != end[..3]);
                assert_eq!(actual[3], 255);
            }
            assert!(changing > 100_000);
            // Palette bytes are truncated before texture sampling/blending.
            // Comparing independently quantized endpoint renders introduces
            // up to two byte units; also check plain plate texels directly.
            assert!(worst <= 2.0, "Top focus{ticks}: worst RGB error{worst}");
            for row in 0..2 {
                let focus = if row == 0 {
                    (1.0 - ticks * 0.1).max(0.0)
                } else {
                    (ticks * 0.25).min(1.0)
                };
                let expected = [
                    (255.0 * (0.2157 + (1.0 - 0.2157) * focus)) as u8,
                    (255.0 * (0.1059 + (0.1608 - 0.1059) * focus)) as u8,
                    (255.0 * (0.1059 + (0.1608 - 0.1059) * focus)) as u8,
                ];
                let offset = ((450 + 96 * row) * 1920 + 1010) * 4;
                assert_eq!(
                    &actual[offset..offset + 3],
                    &expected,
                    "plain Top plate row{row}/tick{ticks}"
                );
            }
            eprintln!("Top focus{ticks}: {changing} changing pixels, worst RGB error{worst}");
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_top_opening_composites_frozen_source_with_live_child() {
        let render = |menu, ticks: &[f32]| {
            render_reference_frame_pixels(Some(menu), None, None, None, Some(ticks), None, None)
        };
        let source = render(MurdererMenu::Top, &[]);
        let flash = render(MurdererMenu::Replay, &[3.5]);
        let flash_rect = super::super::murderer_model::top_menu_rect(0).unwrap();
        let mut worst_flash = 0.0f32;
        for (index, (actual, old)) in flash
            .chunks_exact(4)
            .zip(source.chunks_exact(4))
            .enumerate()
        {
            let x = (index % 1920) as f32;
            let y = (index / 1920) as f32;
            let amount = if x >= flash_rect.x
                && x < flash_rect.x + flash_rect.z
                && y >= flash_rect.y
                && y < flash_rect.y + flash_rect.w
            {
                0.7
            } else {
                0.0
            };
            for channel in 0..3 {
                let expected = old[channel] as f32 * (1.0 - amount) + 255.0 * amount;
                worst_flash = worst_flash.max((actual[channel] as f32 - expected).abs());
            }
            assert_eq!(actual[3], 255);
        }
        assert!(
            worst_flash <= 1.5,
            "Top white flash: worst RGB error{worst_flash}"
        );
        println!(
            "Top confirmation peak white rectangle:2073600 pixels; worst RGB error{worst_flash}"
        );
        let target = render(MurdererMenu::Replay, &[7.0, 0.0, 30.0]);
        for ticks in [0.0, 7.5, 15.0, 22.5] {
            let frame = render(MurdererMenu::Replay, &[7.0, 0.0, ticks]);
            let alpha = ticks / 30.0;
            let mut worst = 0.0f32;
            for ((actual, old), new) in frame
                .chunks_exact(4)
                .zip(source.chunks_exact(4))
                .zip(target.chunks_exact(4))
            {
                for channel in 0..3 {
                    let expected =
                        old[channel] as f32 * (1.0 - alpha) + new[channel] as f32 * alpha;
                    worst = worst.max((actual[channel] as f32 - expected).abs());
                }
                assert_eq!(actual[3], 255);
            }
            assert!(worst <= 1.5, "Top wipe{ticks}: worst RGB error{worst}");
            println!("Top default opening at{ticks}ticks:2073600 pixels; worst RGB error{worst}");
        }
        let face_old = render(MurdererMenu::Accusation, &[7.0]);
        let face_new = render(MurdererMenu::Accusation, &[7.0, 0.0, 10.0]);
        let face_mid = render(MurdererMenu::Accusation, &[7.0, 0.0, 5.0]);
        assert_ne!(face_old, face_new);
        let mut worst_face = 0.0f32;
        for ((actual, old), new) in face_mid
            .chunks_exact(4)
            .zip(face_old.chunks_exact(4))
            .zip(face_new.chunks_exact(4))
        {
            for channel in 0..3 {
                let expected = (old[channel] as f32 + new[channel] as f32) * 0.5;
                worst_face = worst_face.max((actual[channel] as f32 - expected).abs());
            }
            assert_eq!(actual[3], 255);
        }
        assert!(
            worst_face <= 1.5,
            "Preliminary face wipe: worst RGB error{worst_face}"
        );
        println!(
            "Top accusation preliminary face wipe:2073600 pixels; worst RGB error{worst_face}"
        );
        // Glass uses the already independently verified native prism mesh and
        // shader. Exercise it here with actual frozen Top/live accusation art.
        let stage = render(MurdererMenu::Accusation, &[7.0, 0.0, 30.0, 0.0]);
        let middle = render(MurdererMenu::Accusation, &[7.0, 0.0, 30.0, 0.0, 30.0]);
        let done = render(MurdererMenu::Accusation, &[7.0, 0.0, 30.0, 0.0, 60.0]);
        assert_ne!(stage, middle);
        assert_ne!(middle, done);
        assert!(
            middle
                .chunks_exact(4)
                .filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
                .count()
                > 1_000_000
        );
        assert!(middle.chunks_exact(4).all(|p| p[3] == 255));
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_quiz_layer_murderer_back_flash_and_frozen_return_to_top() {
        let render = |menu, ticks: &[f32]| {
            render_reference_frame_pixels(
                Some(menu),
                None,
                None,
                None,
                Some(&[7.0, 0.0, 30.0, 0.0, 60.0]),
                Some(ticks),
                None,
            )
        };
        for menu in [
            MurdererMenu::Replay,
            MurdererMenu::Rules,
            MurdererMenu::PurpleByPerson,
            MurdererMenu::PurpleByChapter,
            MurdererMenu::Hints,
            MurdererMenu::Accusation,
        ] {
            let source = render(menu, &[]);
            let target = render(menu, &[7.0, 0.0, 30.0]);
            let middle = render(menu, &[7.0, 0.0, 15.0]);
            let mut worst = 0.0f32;
            for ((actual, old), new) in middle
                .chunks_exact(4)
                .zip(source.chunks_exact(4))
                .zip(target.chunks_exact(4))
            {
                for channel in 0..3 {
                    let expected = (old[channel] as f32 + new[channel] as f32) * 0.5;
                    worst = worst.max((actual[channel] as f32 - expected).abs());
                }
                assert_eq!(actual[3], 255);
            }
            assert!(worst <= 1.5, "Back wipe{menu:?}: worst RGB error{worst}");
            println!("Back{menu:?}:2073600 independently mixed pixels; worst RGB error{worst}");
            if menu == MurdererMenu::Replay {
                let peak = render(menu, &[3.5]);
                use shin_core::{
                    format::rom::RomReader, primitives::stateless_reader::StatelessFile,
                };
                use std::io::Read;
                let mut rom = RomReader::new(
                    StatelessFile::new(
                        std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();
                let entry = rom.find_file("/murderer.txa").unwrap();
                let mut data = Vec::new();
                rom.open_file(entry)
                    .unwrap()
                    .read_to_end(&mut data)
                    .unwrap();
                let archive =
                    shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
                let texture = &archive.textures[archive.name_to_index["back"]];
                assert_eq!(texture.dimensions(), (160, 84));
                let mut checked = 0;
                let mut worst = 0.0f32;
                for y in 0..84 {
                    for x in 0..160 {
                        let texel = texture.get_pixel(x, y).0;
                        if texel[3] != 255 {
                            continue;
                        }
                        let pixel =
                            &peak[((32 + y as usize) * 1920 + 1640 + x as usize) * 4..][..4];
                        for channel in 0..3 {
                            let expected = texel[channel] as f32 * 0.3 + 255.0 * 0.7;
                            worst = worst.max((pixel[channel] as f32 - expected).abs());
                        }
                        assert_eq!(pixel[3], 255);
                        checked += 1;
                    }
                }
                assert!(checked > 1000);
                assert!(
                    worst <= 1.5,
                    "Back peak texture flash: worst RGB error{worst}"
                );
                println!("Back peak flash:{checked} decoded opaque texels; worst RGB error{worst}");
            }
        }
    }
    fn render_reference_interaction(
        menu: Option<MurdererMenu>,
        final_choice: Option<Option<usize>>,
        animation_ticks: Option<f32>,
    ) {
        render_reference_frame(menu, final_choice, animation_ticks, None);
    }
    fn render_reference_frame(
        menu: Option<MurdererMenu>,
        final_choice: Option<Option<usize>>,
        animation_ticks: Option<f32>,
        replay_flash_ticks: Option<f32>,
    ) {
        render_reference_frame_pixels(
            menu,
            final_choice,
            animation_ticks,
            replay_flash_ticks,
            None,
            None,
            None,
        );
    }
    #[derive(Debug, Clone, Copy)]
    enum MurdererFeedback<'a> {
        Focus(f32),
        HintFocus(f32),
        HintScroll(f32),
        HintAxis(f32),
        HintTimed(&'a [(Vec2, bool, f32)]),
        AccusationFocus(f32),
        PurplePersonFocus(f32),
        PurpleHeadingScroll(f32),
        PurpleHeadingPointer(f32, &'a [(Vec2, bool, f32)]),
        Pointer(&'a [(bool, f32)]),
        PointerAt(&'a [(Vec2, bool, f32)]),
        QuizHover(f32, &'a [(Vec2, f32)]),
        QuizConfirmation(&'a [f32]),
        QuizRestored(i32),
        QuizController(&'a [(u8, f32)]),
        CakeRestored(i32),
        CakeTimed(&'a [(Vec2, bool, f32)]),
        MurdererRestored(i32),
        LastCommands(&'a [&'a str], Option<LastPassTest>, Option<&'a str>),
    }
    fn render_reference_frame_pixels(
        menu: Option<MurdererMenu>,
        final_choice: Option<Option<usize>>,
        animation_ticks: Option<f32>,
        replay_flash_ticks: Option<f32>,
        opening_ticks: Option<&[f32]>,
        back_ticks: Option<&[f32]>,
        top_feedback: Option<MurdererFeedback<'_>>,
    ) -> Vec<u8> {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use crate::render::PreRenderContext;
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
                required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
                ..Default::default()
            },
            None,
        ))
        .unwrap();
        let mut io = LayeredAssetIo::new();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let textures = server.load_sync::<QuizTextures>("/quiz.txa").unwrap();
        let cake = shin_tasks::block_on(super::super::cake_layer::CakeLayer::load(&server));
        // Keep the ordinary screenshot's Cake pending with settled focus.
        cake.pointer(Vec2::new(850.0, 700.0), false);
        cake.reference_update(4.0);
        assert_eq!(cake.result(), None);
        if let Some(MurdererFeedback::CakeRestored(answer)) = top_feedback {
            cake.reset();
            cake.restore_completed_answer(answer);
            assert_eq!(cake.result(), Some(-1));
        } else if let Some(MurdererFeedback::CakeTimed(samples)) = top_feedback {
            cake.reset();
            for &(position, clicked, ticks) in samples {
                cake.pointer(position, clicked);
                cake.reference_update(ticks);
            }
        }
        assert!(super::super::InteractiveLayerRef::Cake(&cake).allows_playback_mode_changes());
        let mut accusation = if let Some(menu) = menu {
            let view = shin_tasks::block_on(
                super::super::murderer_accusation::MurdererAccusationView::load_menu(&server, 20),
            );
            let index = match menu {
                MurdererMenu::Top => None,
                MurdererMenu::Replay => Some(0),
                MurdererMenu::Rules => Some(1),
                MurdererMenu::Hints => Some(4),
                MurdererMenu::Accusation => Some(5),
                MurdererMenu::PurpleByPerson => Some(2),
                MurdererMenu::PurpleByChapter => Some(3),
            };
            if let Some(index) = index {
                view.navigate(index);
                // Opening/return fixtures start from a settled Top selection.
                view.advance_animation(10.0);
                view.confirm();
                for &delta in opening_ticks.unwrap_or(&[7.0, 0.0, 30.0, 0.0, 60.0]) {
                    view.advance_animation(delta);
                }
            }
            if let Some(feedback) = top_feedback {
                match feedback {
                    MurdererFeedback::MurdererRestored(answer) => {
                        view.take_sounds();
                        view.restore_completed_answer(answer);
                        assert!(view.take_sounds().is_empty());
                    }
                    MurdererFeedback::QuizHover(..)
                    | MurdererFeedback::QuizConfirmation(..)
                    | MurdererFeedback::QuizRestored(..)
                    | MurdererFeedback::QuizController(..)
                    | MurdererFeedback::CakeRestored(..)
                    | MurdererFeedback::CakeTimed(..)
                    | MurdererFeedback::LastCommands(..) => {
                        panic!("Quiz hover fixture has no Murderer menu")
                    }
                    MurdererFeedback::Focus(ticks) => {
                        assert_eq!(menu, MurdererMenu::Top);
                        view.navigate(1);
                        view.advance_animation(ticks);
                    }
                    MurdererFeedback::PurpleHeadingPointer(scroll, samples) => {
                        assert_eq!(menu, MurdererMenu::PurpleByChapter);
                        view.reference_set_purple_list_scroll(scroll);
                        for &(point, held, ticks) in samples {
                            view.pointer_state(point, held);
                            view.advance_animation(ticks);
                        }
                    }
                    MurdererFeedback::PurpleHeadingScroll(scroll) => {
                        assert_eq!(menu, MurdererMenu::PurpleByChapter);
                        view.reference_set_purple_list_scroll(scroll);
                    }
                    MurdererFeedback::PurplePersonFocus(ticks) => {
                        assert_eq!(menu, MurdererMenu::PurpleByPerson);
                        let mut state = enum_map::EnumMap::<
                            crate::app::AppAction,
                            shin_input::ActionState,
                        >::default();
                        state[crate::app::AppAction::Right].is_clicked_or_repeated = true;
                        view.handle_controller_input(&state);
                        view.advance_animation(ticks);
                    }
                    MurdererFeedback::AccusationFocus(ticks) => {
                        assert_eq!(menu, MurdererMenu::Accusation);
                        let mut state = enum_map::EnumMap::<
                            crate::app::AppAction,
                            shin_input::ActionState,
                        >::default();
                        state[crate::app::AppAction::Right].is_clicked_or_repeated = true;
                        view.handle_controller_input(&state);
                        view.advance_animation(ticks);
                    }
                    MurdererFeedback::HintAxis(axis) => {
                        assert_eq!(menu, MurdererMenu::Hints);
                        view.handle_controller_input_with_scroll(&Default::default(), axis);
                    }
                    MurdererFeedback::HintTimed(samples) => {
                        assert_eq!(menu, MurdererMenu::Hints);
                        for &(point, held, ticks) in samples {
                            view.pointer_state_with_time(point, held, 0.016683333333333335);
                            view.advance_animation(ticks);
                        }
                    }
                    MurdererFeedback::HintFocus(ticks) => {
                        assert_eq!(menu, MurdererMenu::Hints);
                        view.navigate(-1);
                        view.advance_animation(ticks);
                    }
                    MurdererFeedback::HintScroll(ticks) => {
                        assert_eq!(menu, MurdererMenu::Hints);
                        let mut state = enum_map::EnumMap::<
                            crate::app::AppAction,
                            shin_input::ActionState,
                        >::default();
                        state[crate::app::AppAction::Up].is_clicked_or_repeated = true;
                        for _ in 0..7 {
                            view.handle_controller_input(&state);
                            view.advance_animation(4.0);
                        }
                        view.handle_controller_input(&state);
                        view.advance_animation(ticks);
                    }
                    MurdererFeedback::PointerAt(samples) => {
                        for &(point, held, ticks) in samples {
                            view.pointer_state(point, held);
                            view.advance_animation(ticks);
                        }
                    }
                    MurdererFeedback::Pointer(samples) => {
                        assert_eq!(menu, MurdererMenu::Top);
                        for &(held, ticks) in samples {
                            view.pointer_state(Vec2::new(1010.0, 450.0), held);
                            view.advance_animation(ticks);
                        }
                    }
                }
            }
            if opening_ticks.is_none()
                && menu == MurdererMenu::PurpleByChapter
                && top_feedback.is_none()
            {
                view.navigate(4); // Original chapter record5 is native red truth.
            }
            if opening_ticks.is_none() && menu == MurdererMenu::Accusation && top_feedback.is_none()
            {
                for x in [490.0, 600.0, 710.0] {
                    view.pointer(Vec2::new(x, 480.0), true);
                }
                view.advance_animation(10.0); // Settle native focus and row flashes.
            }
            if let Some(ticks) = back_ticks {
                view.back();
                for &delta in ticks {
                    view.advance_animation(delta);
                }
            }
            assert_eq!(view.result(), None);
            assert_eq!(
                super::super::InteractiveLayerRef::Murderer(&view).allows_playback_mode_changes(),
                matches!(
                    top_feedback,
                    Some(MurdererFeedback::MurdererRestored(0..=5))
                )
            );
            if menu == MurdererMenu::Accusation {
                if let Some(ticks) = animation_ticks {
                    view.pointer(Vec2::new(500.0, 850.0), true);
                    assert!(!view.allows_playback_mode_changes());
                    view.advance_animation(7.0);
                    view.advance_animation(0.0);
                    assert!(view.allows_playback_mode_changes());
                    view.advance_animation(ticks);
                    assert_eq!(view.result(), None);
                }
            }
            if menu == MurdererMenu::Replay {
                if let Some(ticks) = replay_flash_ticks {
                    view.confirm();
                    view.advance_animation(ticks);
                    assert!(!view.allows_playback_mode_changes());
                    assert!(view.result().is_none());
                }
                if let Some(ticks) = animation_ticks {
                    view.confirm();
                    assert!(!view.allows_playback_mode_changes());
                    view.advance_animation(7.0);
                    view.advance_animation(0.0);
                    view.advance_animation(30.0);
                    view.advance_animation(ticks);
                    assert!(view.allows_playback_mode_changes());
                    assert!(view.result().is_none());
                }
            }
            Some(view)
        } else {
            None
        };
        let mut final_view = final_choice.map(|choice| {
            let mut view = shin_tasks::block_on(super::super::last_layer::LastLayer::load(&server));
            if let Some(MurdererFeedback::LastCommands(commands, config, _)) = top_feedback {
                if let Some(properties) =
                    config.and_then(|c| c.transform).and_then(|t| t.properties)
                {
                    use crate::{layer::DrawableLayer, update::AdvUpdatable};
                    view.properties_mut().reference_install_native_state(
                        &properties.values,
                        &properties.wobblers,
                        properties.advance,
                    );
                    if let Some((index, target, duration)) = properties.queued {
                        use num_traits::FromPrimitive;
                        view.properties_mut()
                            .property_tweener_mut(
                                shin_core::vm::command::types::LayerProperty::from_usize(index)
                                    .unwrap(),
                            )
                            .enqueue(
                                target,
                                shin_core::time::Tween::linear(shin_core::time::Ticks::from_f32(
                                    duration,
                                )),
                            );
                    }
                    let mut frame_id = shin_core::primitives::update::FrameId::default();
                    for command in commands {
                        if let Some(delta) = command.strip_prefix("T ") {
                            frame_id.advance();
                            view.update(&crate::update::AdvUpdateContext {
                                frame_id,
                                delta_ticks: shin_core::time::Ticks::from_f32(
                                    delta.parse().unwrap(),
                                ),
                                asset_server: &server,
                                device: &device,
                                queue: &queue,
                                are_animations_allowed: true,
                                voice_callback: None,
                                movie_volume_settings: Default::default(),
                            });
                        } else {
                            view.reference_draw_commands(&[command]);
                        }
                    }
                    if properties.fast_forward {
                        crate::layer::Layer::fast_forward(&mut view);
                    }
                } else {
                    view.reference_draw_commands(commands);
                }
            } else if let Some(choice) = choice {
                view.navigate(if choice == 0 { -1 } else { 1 });
                view.advance_animation(7.0);
                view.advance_animation(animation_ticks.unwrap_or(27.0) - 7.0);
            }
            if !matches!(top_feedback, Some(MurdererFeedback::LastCommands(..))) {
                assert_eq!(view.result(), None);
            }
            view
        });
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test depth"),
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
            label: Some("Choice test target"),
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
        if opening_ticks.is_some() {
            if let Some(view) = &mut accusation {
                crate::layer::Layer::pre_render(view, &mut context, &TransformParams::default());
            }
        }
        if let (Some(view), Some(MurdererFeedback::LastCommands(_, Some(config), _))) =
            (&mut final_view, top_feedback)
        {
            if let Some(t) = config.transform.filter(|t| t.properties.is_some()) {
                crate::layer::Layer::pre_render(view, &mut context, &t.parent);
            }
        }
        drop(context);
        let quiz = QuizLayerImpl {
            textures,
            model: Arc::new(Mutex::new(QuizModel::new(0b1000101))),
        };
        if let Some(MurdererFeedback::QuizHover(initial_ticks, samples)) = top_feedback {
            let mut model = quiz.model.lock().unwrap();
            model.update(shin_core::time::Ticks::from_f32(initial_ticks));
            for &(point, ticks) in samples {
                model.pointer(point, false);
                model.update(shin_core::time::Ticks::from_f32(ticks));
            }
            assert_eq!(
                model.result, None,
                "Hover never answers the authored question"
            );
        } else if let Some(MurdererFeedback::QuizController(samples)) = top_feedback {
            use crate::app::AppAction;
            let mut model = quiz.model.lock().unwrap();
            for &(bits, ticks) in samples {
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                input[AppAction::Up].is_clicked_or_repeated = bits & 1 != 0;
                input[AppAction::AnyDown].is_clicked_or_repeated = bits & 2 != 0;
                input[AppAction::Enter].is_clicked = bits & 4 != 0;
                model.action_input(&input);
                model.update(shin_core::time::Ticks::from_f32(ticks));
            }
        } else if let Some(MurdererFeedback::QuizRestored(answer)) = top_feedback {
            quiz.model.lock().unwrap().restore_completed_answer(answer);
        } else if let Some(MurdererFeedback::QuizConfirmation(samples)) = top_feedback {
            let mut model = quiz.model.lock().unwrap();
            model.pointer(Vec2::new(240.0, 480.0), false);
            model.update(shin_core::time::Ticks::from_f32(4.0));
            model.confirm();
            for &ticks in samples {
                model.update(shin_core::time::Ticks::from_f32(ticks));
            }
        } else {
            quiz.model
                .lock()
                .unwrap()
                .pointer(Vec2::new(240.0, 480.0), true);
            let mut model = quiz.model.lock().unwrap();
            model.update(shin_core::time::Ticks::from_f32(7.0));
            model.update(shin_core::time::Ticks::ZERO);
            assert_eq!(model.result, Some(6));
        }
        {
            let mut pass = RenderPass::new(
                &mut pipelines,
                &mut dynamic,
                &samplers,
                &device,
                &mut encoder,
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(DepthStencilTarget { view: &depth_view }),
                None,
                "Choice menu",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            if !matches!(
                top_feedback,
                Some(
                    MurdererFeedback::CakeRestored(..)
                        | MurdererFeedback::CakeTimed(..)
                        | MurdererFeedback::LastCommands(..)
                )
            ) {
                quiz.render_drawable_direct(
                    &mut pass,
                    &TransformParams::default(),
                    &DrawableParams {
                        color_multiplier: shin_core::primitives::color::FloatColor4::WHITE,
                        blend_type: shin_render::LayerBlendType::Type1,
                        fragment_shader: shin_render::LayerFragmentShader::Default,
                        shader_param: glam::Vec4::ZERO,
                    },
                    &DrawableClipParams {
                        mode: crate::layer::render_params::DrawableClipMode::None,
                        area: glam::Vec4::ZERO,
                    },
                    1,
                    PassKind::Transparent,
                );
            }
            if !matches!(
                top_feedback,
                Some(
                    MurdererFeedback::QuizHover(..)
                        | MurdererFeedback::QuizConfirmation(..)
                        | MurdererFeedback::QuizRestored(..)
                        | MurdererFeedback::QuizController(..)
                        | MurdererFeedback::LastCommands(..)
                )
            ) {
                cake.inner_ref().render_drawable_direct(
                    &mut pass,
                    &TransformParams::default(),
                    &DrawableParams {
                        color_multiplier: shin_core::primitives::color::FloatColor4::WHITE,
                        blend_type: shin_render::LayerBlendType::Type1,
                        fragment_shader: shin_render::LayerFragmentShader::Default,
                        shader_param: glam::Vec4::ZERO,
                    },
                    &DrawableClipParams {
                        mode: crate::layer::render_params::DrawableClipMode::None,
                        area: glam::Vec4::ZERO,
                    },
                    2,
                    PassKind::Transparent,
                );
            }
            if let Some(view) = &accusation {
                if opening_ticks.is_some() {
                    for kind in [PassKind::Opaque, PassKind::Transparent] {
                        crate::layer::Layer::render(
                            view,
                            &mut pass,
                            &TransformParams::default(),
                            3,
                            kind,
                        );
                    }
                } else {
                    view.inner_ref().render_drawable_direct(
                        &mut pass,
                        &TransformParams::default(),
                        &DrawableParams {
                            color_multiplier: shin_core::primitives::color::FloatColor4::WHITE,
                            blend_type: shin_render::LayerBlendType::Type1,
                            fragment_shader: shin_render::LayerFragmentShader::Default,
                            shader_param: glam::Vec4::ZERO,
                        },
                        &DrawableClipParams {
                            mode: crate::layer::render_params::DrawableClipMode::None,
                            area: glam::Vec4::ZERO,
                        },
                        3,
                        PassKind::Transparent,
                    );
                }
            }
            if let Some(view) = &final_view {
                let pass_case = match top_feedback {
                    Some(MurdererFeedback::LastCommands(_, config, _)) => config,
                    _ => None,
                };
                let captured = match top_feedback {
                    Some(MurdererFeedback::LastCommands(_, _, captured)) => captured,
                    _ => None,
                };
                let mut final_transform = pass_case
                    .and_then(|c| c.transform)
                    .map(|t| t.local)
                    .unwrap_or_default();
                if let Some(t) = pass_case.and_then(|c| c.transform) {
                    final_transform.compose_with(&t.parent, t.flags);
                }
                if let Some(config) = pass_case {
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(config.initial_stencil),
                        None,
                    );
                }
                let draw_overlap = |pass: &mut RenderPass,
                                    rect: glam::Vec4,
                                    color: shin_core::primitives::color::FloatColor4,
                                    reference: u8| {
                    use shin_render::shaders::types::{buffer::VertexSource, vertices::PosVertex};
                    use shin_render::{
                        DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
                    };
                    let position = |x: f32, y: f32| PosVertex {
                        position: glam::vec3(x / 960.0 - 1.0, 1.0 - y / 540.0, 0.0),
                    };
                    let vertices = [
                        position(rect.x, rect.y),
                        position(rect.x + rect.z, rect.y),
                        position(rect.x, rect.y + rect.w),
                        position(rect.x + rect.z, rect.y + rect.w),
                    ];
                    pass.run(
                        RenderRequestBuilder::new()
                            .depth_stencil_shorthand(reference, false, false)
                            .color_blend_type(shin_render::ColorBlendType::Opaque)
                            .build(
                                RenderProgramWithArguments::Clear {
                                    vertices: VertexSource::VertexData {
                                        vertices: &vertices,
                                    },
                                    color,
                                },
                                DrawPrimitive::TrianglesStrip,
                            ),
                    );
                };
                if pass_case.is_some_and(|c| c.overlap) {
                    draw_overlap(
                        &mut pass,
                        glam::vec4(1320.0, 720.0, 480.0, 320.0),
                        shin_core::primitives::color::FloatColor4::from_rgba(0.0, 1.0, 1.0, 1.0),
                        6,
                    );
                }
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    if kind == PassKind::Transparent && pass_case.is_some_and(|c| c.overlap) {
                        draw_overlap(
                            &mut pass,
                            glam::vec4(0.0, 0.0, 1920.0, 1080.0),
                            shin_core::primitives::color::FloatColor4::from_rgba(
                                1.0, 0.0, 1.0, 1.0,
                            ),
                            3,
                        );
                    }
                    let bit = if kind == PassKind::Opaque { 1 } else { 2 };
                    if pass_case.is_some_and(|c| c.passes & bit == 0) {
                        continue;
                    }
                    if let Some(captured) = captured {
                        render_captured_last_draws(view.inner_ref(), &mut pass, captured, kind);
                        continue;
                    }
                    if let Some(t) = pass_case
                        .and_then(|c| c.transform)
                        .filter(|t| t.properties.is_some())
                    {
                        crate::layer::Layer::render(view, &mut pass, &t.parent, 4, kind);
                        continue;
                    }
                    view.inner_ref().render_drawable_direct(
                        &mut pass,
                        &final_transform,
                        &DrawableParams {
                            color_multiplier: shin_core::primitives::color::FloatColor4::WHITE,
                            blend_type: shin_render::LayerBlendType::Type1,
                            fragment_shader: shin_render::LayerFragmentShader::Default,
                            shader_param: glam::Vec4::ZERO,
                        },
                        &DrawableClipParams {
                            mode: crate::layer::render_params::DrawableClipMode::None,
                            area: glam::Vec4::ZERO,
                        },
                        4,
                        kind,
                    );
                }
            }
        }
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1920 * 1080 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1920 * 4),
                    rows_per_image: Some(1080),
                },
            },
            target.size(),
        );
        let mut uploads = device.create_command_encoder(&Default::default());
        dynamic.finish(&mut uploads);
        queue.submit([uploads.finish(), encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device.poll(wgpu::Maintain::Wait);
        receive.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range();
        if matches!(
            top_feedback,
            Some(
                MurdererFeedback::QuizHover(..)
                    | MurdererFeedback::QuizConfirmation(..)
                    | MurdererFeedback::QuizRestored(..)
                    | MurdererFeedback::QuizController(..)
                    | MurdererFeedback::CakeRestored(..)
                    | MurdererFeedback::CakeTimed(..)
                    | MurdererFeedback::MurdererRestored(..)
                    | MurdererFeedback::LastCommands(..)
            )
        ) {
            return pixels.to_vec();
        }
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in pixels.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        if let Some(ticks) = opening_ticks {
            std::fs::write(
                format!(
                    "/tmp/shin-murderer-transition-{:?}-{:?}-{:?}-{:?}.ppm",
                    menu.unwrap(),
                    ticks,
                    back_ticks,
                    top_feedback
                ),
                ppm,
            )
            .unwrap();
            return pixels.to_vec();
        }
        if let Some(view) = &final_view {
            let choice = final_choice.unwrap();
            let name = match choice {
                None => "select",
                Some(0) => "magic",
                Some(_) => "trick",
            };
            let suffix = animation_ticks
                .map(|ticks| format!("-{ticks}"))
                .unwrap_or_default();
            std::fs::write(format!("/tmp/shin-last-{name}{suffix}.ppm"), ppm).unwrap();
            let rects: Vec<_> = match choice {
                None => super::super::last_layer::CHOICES.to_vec(),
                Some(choice) => super::super::last_layer::confirmation_rects(choice).to_vec(),
            };
            for rect in rects {
                let distinct: std::collections::HashSet<_> = (rect.y as usize
                    ..(rect.y + rect.w) as usize)
                    .flat_map(|y| {
                        pixels[(y * 1920 + rect.x as usize) * 4
                            ..(y * 1920 + (rect.x + rect.z) as usize) * 4]
                            .chunks_exact(4)
                    })
                    .map(|p| [p[0], p[1], p[2]])
                    .collect();
                assert!(
                    distinct.len() > 100,
                    "Final choice {name} rectangle is blank"
                );
            }
            if let Some(choice) = choice {
                if animation_ticks.is_some() {
                    view.confirm();
                    assert_eq!(view.result(), None);
                    view.advance_animation(20.0);
                    assert_eq!(view.result(), None);
                }
                let back = super::super::last_layer::confirmation_rects(choice)[2];
                view.pointer(Vec2::new(back.x + 100.0, 900.0), true);
                assert_eq!(view.result(), None);
                view.advance_animation(7.0);
                view.advance_animation(20.0);
                view.navigate(if choice == 0 { -1 } else { 1 });
                view.advance_animation(7.0);
                view.advance_animation(20.0);
                view.pointer(Vec2::new(960.0, 300.0), true);
                assert_eq!(view.result(), None);
                view.advance_animation(7.0);
                assert_eq!(view.result(), Some(choice as i32));
            } else {
                view.pointer(Vec2::new(1200.0, 900.0), true);
                assert_eq!(view.result(), None);
                view.advance_animation(7.0);
                view.advance_animation(20.0);
                view.confirm();
                assert_eq!(view.result(), None);
                view.advance_animation(7.0);
                assert_eq!(view.result(), Some(1));
            }
            return pixels.to_vec();
        }
        if matches!(
            menu,
            Some(
                MurdererMenu::Accusation
                    | MurdererMenu::PurpleByPerson
                    | MurdererMenu::PurpleByChapter
                    | MurdererMenu::Hints
            )
        ) && top_feedback.is_some()
        {
            // Custom feedback/scroll snapshots have independent pixel checks;
            // they do not run the stable menu selection/completion fixture.
            return pixels.to_vec();
        }
        if let Some(view) = &accusation {
            let menu = menu.unwrap();
            if menu == MurdererMenu::Replay {
                if let Some(ticks) = animation_ticks.or(replay_flash_ticks) {
                    use shin_core::{
                        format::rom::RomReader, primitives::stateless_reader::StatelessFile,
                    };
                    use std::io::Read;
                    let mut rom = RomReader::new(
                        StatelessFile::new(
                            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap())
                                .unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    let entry = rom.find_file("/murderer.txa").unwrap();
                    let mut data = Vec::new();
                    rom.open_file(entry)
                        .unwrap()
                        .read_to_end(&mut data)
                        .unwrap();
                    let archive =
                        shin_core::format::texture_archive::read_texture_archive(&data).unwrap();
                    let small = &archive.textures[archive.name_to_index["thumb_1_s"]];
                    let large = &archive.textures[archive.name_to_index["thumb_1"]];
                    let amount = if replay_flash_ticks.is_some() {
                        0.0
                    } else {
                        (ticks / 60.0 * std::f32::consts::FRAC_PI_2).sin()
                    };
                    let flash = replay_flash_ticks.map_or(0.0, |ticks| {
                        if ticks <= 3.5 {
                            ticks * 0.2
                        } else {
                            (7.0 - ticks) * 0.2
                        }
                    });
                    // Independently recover native center/size lerps from the
                    // executable constants, rather than reading model geometry.
                    let width = 544.0 + 1376.0 * amount;
                    let height = 306.0 + 774.0 * amount;
                    let left = 396.0 + 564.0 * amount - width * 0.5;
                    let top = 346.0 + 194.0 * amount - height * 0.5;
                    let sample = |(width, height): (u32, u32), data: &[u8], u: f32, v: f32| {
                        let x = (u * width as f32 - 0.5).clamp(0.0, width as f32 - 1.0);
                        let y = (v * height as f32 - 0.5).clamp(0.0, height as f32 - 1.0);
                        let x0 = x.floor() as u32;
                        let y0 = y.floor() as u32;
                        let x1 = (x0 + 1).min(width - 1);
                        let y1 = (y0 + 1).min(height - 1);
                        std::array::from_fn::<_, 4, _>(|channel| {
                            let a = data[((y0 * width + x0) * 4) as usize + channel] as f32
                                * (1.0 - (x - x0 as f32))
                                + data[((y0 * width + x1) * 4) as usize + channel] as f32
                                    * (x - x0 as f32);
                            let b = data[((y1 * width + x0) * 4) as usize + channel] as f32
                                * (1.0 - (x - x0 as f32))
                                + data[((y1 * width + x1) * 4) as usize + channel] as f32
                                    * (x - x0 as f32);
                            a * (1.0 - (y - y0 as f32)) + b * (y - y0 as f32)
                        })
                    };
                    let mut checked = 0;
                    let mut worst = 0.0f32;
                    for y in ((top.ceil() as usize + 2)..((top + height).floor() as usize - 2))
                        .step_by(2)
                    {
                        for x in ((left.ceil() as usize + 2)..((left + width).floor() as usize - 2))
                            .step_by(2)
                        {
                            let u = (x as f32 + 0.5 - left) / width;
                            let v = (y as f32 + 0.5 - top) / height;
                            let a = sample(small.dimensions(), small.as_raw(), u, v);
                            let b = sample(large.dimensions(), large.as_raw(), u, v);
                            if a[3] < 254.99 || b[3] < 254.99 {
                                continue;
                            }
                            let pixel = &pixels[(y * 1920 + x) * 4..][..4];
                            for channel in 0..3 {
                                let mut expected = a[channel] + (b[channel] - a[channel]) * amount;
                                if replay_flash_ticks.is_some() {
                                    expected =
                                        (expected + (255.0 - expected) * flash + 128.0).min(255.0);
                                }
                                worst = worst.max((pixel[channel] as f32 - expected).abs());
                            }
                            assert_eq!(pixel[3], 255);
                            checked += 1;
                        }
                    }
                    assert!(checked > 30000);
                    assert!(worst <= 3.0, "Replay blend{amount}: worst error{worst}");
                    println!(
                        "Native replay expansion{ticks}: {checked} independent bilinear/texture-mix pixels, worst error{worst}"
                    );
                }
            }
            let name = if menu == MurdererMenu::Accusation {
                "accusation".to_string()
            } else {
                format!("{menu:?}").to_lowercase()
            };
            let suffix = animation_ticks
                .map(|ticks| format!("-flash-{ticks}"))
                .unwrap_or_default();
            std::fs::write(format!("/tmp/shin-murderer-{name}{suffix}.ppm"), ppm).unwrap();
            let rects: Vec<_> = match menu {
                MurdererMenu::Accusation => (28..=45)
                    .map(|entry| super::super::murderer_model::accusation_rect(entry).unwrap())
                    .collect(),
                MurdererMenu::Top => (0..6)
                    .map(|i| super::super::murderer_model::top_menu_rect(i).unwrap())
                    .collect(),
                MurdererMenu::Replay => (0..6)
                    .map(|i| super::super::murderer_model::replay_rect(i).unwrap())
                    .collect(),
                MurdererMenu::Hints => (0..8)
                    .map(|i| vec4(320.0, 180.0 + 90.0 * i as f32, 1280.0, 88.0))
                    .collect(),
                MurdererMenu::Rules => vec![vec4(300.0, 180.0, 1120.0, 720.0)],
                MurdererMenu::PurpleByPerson => (0..17)
                    .map(|i| super::super::murderer_model::purple_person_rect(i).unwrap())
                    .chain([vec4(726.0, 180.0, 1024.0, 720.0)])
                    .collect(),
                MurdererMenu::PurpleByChapter => vec![
                    vec4(166.0, 180.0, 480.0, 720.0),
                    vec4(726.0, 180.0, 1024.0, 720.0),
                ],
            };
            for (index, rect) in rects.into_iter().enumerate() {
                if replay_flash_ticks == Some(3.5) && index == 0 {
                    // White flash0.7 plus additive focus0.5 saturates the
                    // selected card. Its decoded-texture pixel check above
                    // verifies that expected solid white frame.
                    continue;
                }
                let distinct: std::collections::HashSet<_> = (rect.y as usize
                    ..(rect.y + rect.w) as usize)
                    .flat_map(|y| {
                        pixels[(y * 1920 + rect.x as usize) * 4
                            ..(y * 1920 + (rect.x + rect.z) as usize) * 4]
                            .chunks_exact(4)
                    })
                    .map(|p| [p[0], p[1], p[2]])
                    .collect();
                assert!(
                    distinct.len() > if animation_ticks.is_some() { 25 } else { 100 },
                    "Native {menu:?} rectangle {index} is blank"
                );
            }
            if animation_ticks == Some(15.0) && menu == MurdererMenu::Accusation {
                assert!(
                    pixels
                        .chunks_exact(4)
                        .all(|p| p[0] >= 127 && p[1] >= 127 && p[2] >= 127),
                    "Half-whiteflashmustcoverentiretarget"
                );
            }
            if menu == MurdererMenu::Top {
                // A plain white pixel in the plate is tinted by the native
                // selected palette (red255/green41/blue41). This also guards
                // the shared sprite shader against swapping RGBA channels.
                let offset = (450 * 1920 + 1010) * 4;
                assert!(pixels[offset] > 200);
                assert!(
                    pixels[offset + 1] < 80 && pixels[offset + 2] < 80,
                    "Selected plate lost its native red tint: {:?}",
                    &pixels[offset..offset + 4]
                );
            }
            if matches!(
                menu,
                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter
            ) {
                let colored = (180..900)
                    .flat_map(|y| {
                        pixels[(y * 1920 + 726) * 4..(y * 1920 + 1750) * 4].chunks_exact(4)
                    })
                    .filter(|p| {
                        if menu == MurdererMenu::PurpleByChapter {
                            p[0] > 120 && p[1] < 80 && p[2] < 80
                        } else {
                            p[2] > 120 && p[2] > p[0] && p[0] > p[1]
                        }
                    })
                    .count();
                assert!(colored > 100, "Native transcript color missing in {menu:?}");
            }
            match menu {
                MurdererMenu::Accusation => {
                    view.pointer(Vec2::new(500.0, 850.0), true);
                    assert_eq!(view.result(), None);
                    for _ in 0..68 {
                        view.advance_animation(1.0);
                    }
                    assert_eq!(view.result(), Some(27));
                }
                MurdererMenu::Top => {
                    view.navigate(5);
                    view.confirm();
                    for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
                        view.advance_animation(delta);
                    }
                    for x in [490.0, 600.0, 710.0] {
                        view.pointer(Vec2::new(x, 480.0), true);
                    }
                    view.pointer(Vec2::new(500.0, 850.0), true);
                    assert_eq!(view.result(), None);
                    for _ in 0..68 {
                        view.advance_animation(1.0);
                    }
                    assert_eq!(view.result(), Some(27));
                }
                MurdererMenu::Replay => {
                    view.pointer(Vec2::new(130.0, 200.0), true);
                    assert_eq!(view.result(), None);
                    for _ in 0..128 {
                        view.advance_animation(1.0);
                    }
                    assert_eq!(view.result(), Some(0));
                }
                MurdererMenu::Hints => {
                    view.pointer(Vec2::new(500.0, 200.0), true);
                    assert_eq!(view.result(), None);
                    for _ in 0..64 {
                        view.advance_animation(1.0);
                    }
                    assert_eq!(view.result(), Some(19));
                }
                MurdererMenu::Rules => {
                    view.pointer(Vec2::new(1700.0, 60.0), true);
                    view.confirm();
                    assert_eq!(view.result(), None);
                }
                MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter => {
                    view.scroll_text(180.0);
                    view.navigate(1);
                    view.confirm();
                    assert_eq!(view.result(), None);
                    view.back();
                    assert_eq!(view.result(), None);
                }
            }
            return pixels.to_vec();
        }
        std::fs::write("/tmp/shin-quiz-layer.ppm", ppm).unwrap();
        let center = (700 * 1920 + 850) * 4;
        assert!(
            pixels[center] > 10 || pixels[center + 1] > 10,
            "Cake did not render"
        );
        let model = quiz.model.lock().unwrap();
        for row in 0..model.answers.len() {
            let r = model.rect(row);
            let nonblack = (r.y as usize..(r.y + r.w) as usize)
                .flat_map(|y| {
                    pixels[(y * 1920 + r.x as usize) * 4..(y * 1920 + (r.x + r.z) as usize) * 4]
                        .chunks_exact(4)
                })
                .filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
                .count();
            assert!(nonblack > 100, "Quiz button {row} did not render");
        }
        pixels.to_vec()
    }
}

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
