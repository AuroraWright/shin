//! Final magic/trick choice (native LastLayer, factory0x6d320).
use crate::{
    asset::{system::AssetServer, texture_archive::TextureArchive},
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Vec2, Vec4, vec4};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, gpu_texture::GpuTexture,
    quad_vertices::QuadVertices, render_pass::RenderPass, shaders::types::RenderClone,
};
use std::sync::{Arc, Mutex};

// Native hit tests0x6f200,0x6f3e0,0x6f5d0 use screen coordinates.
pub const CHOICES: [Vec4; 2] = [
    vec4(272.0, 816.0, 624.0, 176.0),
    vec4(1024.0, 816.0, 624.0, 176.0),
];
pub fn confirmation_rects(choice: usize) -> [Vec4; 3] {
    if choice == 0 {
        [
            vec4(496.0, 96.0, 918.0, 800.0),
            vec4(768.0, 880.0, 436.0, 176.0),
            vec4(1416.0, 816.0, 480.0, 176.0),
        ]
    } else {
        [
            vec4(506.0, 96.0, 918.0, 800.0),
            vec4(720.0, 880.0, 436.0, 176.0),
            vec4(24.0, 816.0, 480.0, 176.0),
        ]
    }
}
fn contains(rect: Vec4, p: Vec2) -> bool {
    p.is_finite()
        && p.x >= rect.x
        && p.x < rect.x + rect.z
        && p.y >= rect.y
        && p.y < rect.y + rect.w
}
// LastLayer uses seven native FloatTweeners. Flags0x40 specify speed,
// so the0.7->0 flash at0.2/tick takes3.5ticks per leg (bb964).
#[derive(Debug, Clone)]
struct Motion {
    value: f32,
    start: f32,
    elapsed: f32,
    queue: std::collections::VecDeque<(f32, f32, bool)>,
}
impl Motion {
    fn new(value: f32) -> Self {
        Self {
            value,
            start: value,
            elapsed: 0.0,
            queue: Default::default(),
        }
    }
    fn busy(&self) -> bool {
        !self.queue.is_empty()
    }
    fn set(&mut self, target: f32, duration: f32, sine_out: bool) {
        self.queue.clear();
        self.start = self.value;
        self.elapsed = 0.0;
        if duration <= 0.0 {
            self.value = target;
            self.start = target;
        } else {
            self.queue.push_back((target, duration, sine_out));
        }
    }
    fn speed_to(&mut self, target: f32) {
        self.set(target, (target - self.value).abs() / 0.2, false);
    }
    fn flash(&mut self) {
        self.set(0.7, (0.7 - self.value).abs() / 0.2, false);
        self.queue.push_back((0.0, 3.5, false));
    }
    fn update(&mut self, mut delta: f32) {
        if delta <= 0.0 {
            return;
        }
        while let Some(&(target, duration, sine_out)) = self.queue.front() {
            let consumed = delta.min((duration - self.elapsed).max(0.0));
            self.elapsed += consumed;
            delta -= consumed;
            if self.elapsed >= duration {
                self.value = target;
                self.start = target;
                self.elapsed = 0.0;
                self.queue.pop_front();
            } else {
                let progress = self.elapsed / duration;
                let progress = if sine_out {
                    (progress * std::f32::consts::FRAC_PI_2).sin()
                } else {
                    progress
                };
                self.value = self.start + (target - self.start) * progress;
                break;
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct LastModel {
    pub choice: Option<usize>,
    pub focus: Option<usize>,
    pub result: Option<i32>,
    last_pointer: Option<Vec2>,
    slide: Motion,
    front: Motion,
    door: Motion,
    choice_flashes: [Motion; 2],
    back_flash: Motion,
    confirm_flash: Motion,
    confirming: bool,
    phase: f32,
    sounds: Vec<&'static str>,
    pointer_area: Option<crate::adv::input_area::InputArea>,
    pointer_extent: (u16, u16),
    pointer_row: Option<usize>,
    pointer_consumed: bool,
}
impl Default for LastModel {
    fn default() -> Self {
        Self {
            choice: None,
            focus: None,
            result: None,
            last_pointer: None,
            slide: Motion::new(0.0),
            front: Motion::new(1.0),
            door: Motion::new(0.0),
            choice_flashes: [Motion::new(0.0), Motion::new(0.0)],
            back_flash: Motion::new(0.0),
            confirm_flash: Motion::new(0.0),
            confirming: false,
            phase: 0.0,
            sounds: Vec::new(),
            pointer_area: Some(crate::adv::input_area::InputArea::canvas()),
            pointer_extent: (1920, 1080),
            pointer_row: None,
            pointer_consumed: false,
        }
    }
}
impl LastModel {
    pub fn restore_completed_answer(&mut self, answer: i32) {
        // Native6f930: answer0 slides right; every other answer slides left.
        // All other tween values/queues reset to zero. Choice294 is retained,
        // while state298=-1 disables input and marks the getter completed.
        self.slide
            .set(if answer == 0 { 960.0 } else { -960.0 }, 0.0, false);
        self.front.set(0.0, 0.0, false);
        self.door.set(0.0, 0.0, false);
        for flash in &mut self.choice_flashes {
            flash.set(0.0, 0.0, false);
        }
        self.back_flash.set(0.0, 0.0, false);
        self.confirm_flash.set(0.0, 0.0, false);
        self.confirming = false;
        self.result = Some(self.choice.map_or(-1, |choice| choice as i32));
    }
    pub fn update(&mut self, delta: f32) {
        if !delta.is_finite() || delta < 0.0 {
            return;
        }
        // Native6e598 tests these flashes before advancing the base tweens.
        if !self.choice_flashes.iter().any(Motion::busy) && !self.back_flash.busy() {
            self.slide.update(delta);
            self.front.update(delta);
            self.door.update(delta);
        }
        for flash in &mut self.choice_flashes {
            flash.update(delta);
        }
        self.back_flash.update(delta);
        self.confirm_flash.update(delta);
        self.phase = (self.phase + delta * 0.01).fract();
        if self.confirming && !self.confirm_flash.busy() {
            self.result = self.choice.map(|choice| choice as i32);
            self.confirming = false;
        }
    }
    fn accepts_input(&self) -> bool {
        self.result.is_none() && !self.confirming && !self.slide.busy() && !self.front.busy()
    }
    fn controller_input(&mut self, left: bool, right: bool, confirm: bool) {
        if self.pointer_consumed || !self.accepts_input() {
            return;
        }
        // Native70000: Enter wins on an already selected door. In the initial
        // view Enter is ignored and repeated Left takes priority over Right.
        match self.choice {
            Some(_) if confirm => self.confirm(),
            Some(0) if right => self.back(),
            Some(1) if left => self.back(),
            None if left => self.select(0),
            None if right => self.select(1),
            _ => {}
        }
    }
    fn action_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        use crate::app::AppAction;
        self.controller_input(
            state[AppAction::Left].is_clicked_or_repeated,
            state[AppAction::Right].is_clicked_or_repeated,
            state[AppAction::Enter].is_clicked,
        );
    }
    fn pointer_hit(&self, position: Vec2) -> Option<usize> {
        match self.choice {
            None => CHOICES.iter().position(|&rect| contains(rect, position)),
            Some(choice) => {
                let [door, plate, back] = confirmation_rects(choice);
                if contains(door, position) || contains(plate, position) {
                    Some(0)
                } else if contains(back, position) {
                    Some(1)
                } else {
                    None
                }
            }
        }
    }
    fn pointer_frame(
        &mut self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        let mut area = self
            .pointer_area
            .take()
            .expect("Last pointer dispatcher reentered");
        if extent != self.pointer_extent && extent.0 > 0 && extent.1 > 0 {
            area.disable(self);
            area = crate::adv::input_area::InputArea::new(extent.0, extent.1);
            self.pointer_extent = extent;
        }
        if !self.accepts_input() {
            area.disable(self);
            self.pointer_consumed = true;
        } else {
            self.pointer_consumed = area.update(self, frame);
        }
        self.pointer_area = Some(area);
    }
    fn disable_pointer(&mut self) {
        let mut area = self.pointer_area.take().unwrap();
        area.disable(self);
        self.pointer_area = Some(area);
        self.pointer_consumed = false;
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    fn select(&mut self, choice: usize) {
        self.sounds.push("move");
        self.choice = Some(choice);
        self.focus = None;
        self.choice_flashes[choice].flash();
        self.slide
            .set(if choice == 0 { 960.0 } else { -960.0 }, 20.0, true);
        self.front.set(0.0, 20.0, false);
        self.door.set(1.0, 20.0, false);
        self.phase = 0.0;
    }
    pub fn navigate(&mut self, delta: i32) {
        if !self.accepts_input() || delta == 0 {
            return;
        }
        match self.choice {
            None => self.select(usize::from(delta > 0)),
            Some(0) if delta > 0 => self.back(),
            Some(1) if delta < 0 => self.back(),
            _ => {}
        }
    }
    pub fn confirm(&mut self) {
        if !self.accepts_input() {
            return;
        }
        if self.choice.is_some() {
            self.sounds.push("decide");
            self.door.set(0.0, 5.0, false);
            self.confirm_flash.flash();
            self.confirming = true;
        } else if let Some(focus) = self.focus {
            self.select(focus);
        }
    }
    pub fn back(&mut self) {
        if !self.accepts_input() || self.choice.is_none() {
            return;
        }
        self.sounds.push("move");
        self.choice = None;
        self.focus = None;
        self.back_flash.flash();
        self.slide.set(0.0, 20.0, true);
        self.front.set(1.0, 20.0, false);
        self.door.set(0.0, 20.0, false);
    }
    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        if !self.accepts_input() || (!clicked && self.last_pointer == Some(position)) {
            return;
        }
        self.last_pointer = Some(position);
        let old_focus = self.focus;
        self.focus = match self.choice {
            None => CHOICES.iter().position(|&r| contains(r, position)),
            Some(choice) => {
                let [door, plate, back] = confirmation_rects(choice);
                if contains(door, position) || contains(plate, position) {
                    Some(0)
                } else if contains(back, position) {
                    Some(1)
                } else {
                    None
                }
            }
        };
        if self.focus != old_focus {
            if self.choice.is_none() {
                for (index, flash) in self.choice_flashes.iter_mut().enumerate() {
                    flash.speed_to(if self.focus == Some(index) { 0.7 } else { 0.0 });
                }
            } else {
                self.confirm_flash
                    .speed_to(if self.focus == Some(0) { 0.7 } else { 0.0 });
                self.back_flash
                    .speed_to(if self.focus == Some(1) { 0.7 } else { 0.0 });
            }
        }
        if clicked && self.focus.is_some() {
            if self.choice.is_some() && self.focus == Some(1) {
                self.back();
            } else {
                self.confirm();
            }
        }
    }
}
impl crate::adv::input_area::InputAreaTarget for LastModel {
    fn probe(&mut self, position: Vec2) -> bool {
        if !self.accepts_input() {
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
            match self.choice {
                None => self.choice_flashes[row].speed_to(0.7),
                Some(_) if row == 0 => self.confirm_flash.speed_to(0.7),
                Some(_) => self.back_flash.speed_to(0.7),
            }
        }
    }
    fn release(&mut self, _position: Vec2) {
        if let Some(row) = self.pointer_row {
            match self.choice {
                None => self.select(row),
                Some(_) if row == 0 => self.confirm(),
                Some(_) => self.back(),
            }
            // Native704a0 retains the captured row; InputArea resets it after Up.
        }
    }
    fn cancel(&mut self) {
        if self.pointer_row.take().is_some() {
            if self.choice.is_none() {
                for flash in &mut self.choice_flashes {
                    flash.speed_to(0.0);
                }
            } else {
                self.confirm_flash.speed_to(0.0);
                self.back_flash.speed_to(0.0);
            }
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
#[derive(TextureArchive)]
pub(crate) struct LastTextures {
    #[txa(name = "bg")]
    bg: GpuTexture,
    #[txa(name = "front")]
    front: GpuTexture,
    #[txa(name = "back")]
    back: GpuTexture,
    #[txa(name = "plate")]
    plate: GpuTexture,
    #[txa(name = "shadow")]
    shadow: GpuTexture,
    #[txa(name = "select")]
    select: GpuTexture,
    #[txa(name = "obi")]
    _obi: GpuTexture,
    #[txa(name = "flash_door")]
    flash_door: GpuTexture,
}
pub struct LastLayerImpl {
    textures: Arc<LastTextures>,
    model: Arc<Mutex<LastModel>>,
}
#[cfg(test)]
impl LastLayerImpl {
    pub(crate) fn reference_texture(&self, name: &str) -> &GpuTexture {
        match name {
            "bg" => &self.textures.bg,
            "front" => &self.textures.front,
            "select" => &self.textures.select,
            "back" => &self.textures.back,
            "plate" => &self.textures.plate,
            "shadow" => &self.textures.shadow,
            "flash_door" => &self.textures.flash_door,
            "obi" => &self.textures._obi,
            _ => panic!("Unknown captured native Last texture {name}"),
        }
    }
}
impl Clone for LastLayerImpl {
    fn clone(&self) -> Self {
        Self {
            textures: self.textures.clone(),
            model: Arc::new(Mutex::new(self.model.lock().unwrap().clone())),
        }
    }
}
impl RenderClone for LastLayerImpl {
    fn render_clone(&self, _: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        self.clone()
    }
}
impl std::fmt::Debug for LastLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LastLayer")
            .field("model", &self.model)
            .finish()
    }
}
pub type LastLayer = NewDrawableLayerWrapper<LastLayerImpl>;
impl LastLayer {
    pub async fn load(server: &AssetServer) -> Self {
        Self::new(
            server
                .load("/last.txa")
                .await
                .expect("Failed to load final choice textures"),
        )
    }
    pub(crate) fn new(textures: Arc<LastTextures>) -> Self {
        Self::from_plain_layer_constructor(|| LastLayerImpl {
            textures,
            model: Arc::new(Mutex::new(LastModel::default())),
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        let inner = self.inner_ref();
        (
            Arc::as_ptr(&inner.textures) as usize,
            Arc::as_ptr(&inner.model) as usize,
            inner.model.lock().unwrap().choice.map_or(-1, |x| x as i32),
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_draw_commands(&self, commands: &[&str]) {
        use crate::adv::input_area::InputAreaTarget;
        let inner = self.inner_ref();
        let mut model = inner.model.lock().unwrap();
        for command in commands {
            let mut parts = command.split_whitespace();
            let op = parts.next().unwrap();
            let args: Vec<f32> = parts.map(|p| p.parse().unwrap()).collect();
            match op {
                "N" => *model = LastModel::default(),
                "C" => model.controller_input(args[0] != 0.0, args[1] != 0.0, args[2] != 0.0),
                "P" => {
                    assert!(model.probe(Vec2::new(args[0], args[1])));
                }
                "H" => model.press(Vec2::new(args[0], args[1])),
                "R" => model.release(Vec2::new(args[0], args[1])),
                "L" => InputAreaTarget::reset(&mut *model),
                "X" => model.cancel(),
                "T" => model.update(args[0]),
                _ => panic!("Unsupported Last draw fixture command: {command}"),
            }
        }
    }
    pub fn navigate(&self, delta: i32) {
        self.inner_ref().model.lock().unwrap().navigate(delta);
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        self.inner_ref().model.lock().unwrap().action_input(state);
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer_frame(frame, extent);
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.inner_ref().model.lock().unwrap().disable_pointer();
    }
    #[cfg(test)]
    pub(crate) fn reference_accepts_input(&self) -> bool {
        self.inner_ref().model.lock().unwrap().accepts_input()
    }
    #[cfg(test)]
    pub(crate) fn reference_pointer_feedback(&self) -> (Option<usize>, bool, [f32; 7]) {
        let model = self.inner_ref().model.lock().unwrap();
        (
            model.pointer_row,
            model.confirming,
            [
                model.slide.value,
                model.front.value,
                model.door.value,
                model.choice_flashes[0].value,
                model.choice_flashes[1].value,
                model.back_flash.value,
                model.confirm_flash.value,
            ],
        )
    }
    pub fn confirm(&self) {
        self.inner_ref().model.lock().unwrap().confirm();
    }
    pub fn back(&self) {
        self.inner_ref().model.lock().unwrap().back();
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer(position, clicked);
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        self.inner_ref().model.lock().unwrap().take_sounds()
    }
    pub fn advance_animation(&self, ticks: f32) {
        self.inner_ref().model.lock().unwrap().update(ticks);
    }
    pub fn result(&self) -> Option<i32> {
        self.inner_ref().model.lock().unwrap().result
    }
    pub fn reset(&self) {
        *self.inner_ref().model.lock().unwrap() = LastModel::default();
    }
    pub fn restore_completed_answer(&self, answer: i32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .restore_completed_answer(answer);
    }
    #[cfg(test)]
    pub(crate) fn reference_restored_tweens(&self) -> [f32; 7] {
        let model = self.inner_ref().model.lock().unwrap();
        [
            model.slide.value,
            model.front.value,
            model.door.value,
            model.choice_flashes[0].value,
            model.choice_flashes[1].value,
            model.back_flash.value,
            model.confirm_flash.value,
        ]
    }
}
impl AdvUpdatable for LastLayerImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.model
            .lock()
            .unwrap()
            .update(context.delta_ticks.as_f32());
    }
}
impl NewDrawableLayerFastForward for LastLayerImpl {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayerNeedsSeparatePass for LastLayerImpl {}
impl NewDrawableLayer for LastLayerImpl {
    fn property_policy(&self) -> crate::layer::new_drawable_layer::DrawablePropertyPolicy {
        // Last's vtable uses6e580 for update (no base property update),
        //6e570/710d0 are no-op fast-forward/pre-render. Render6e670 reads
        //754d0/754a0 transforms/flags and sets fixed white colors/blend modes.
        crate::layer::new_drawable_layer::DrawablePropertyPolicy::TransformOnly
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
        let model = self.model.lock().unwrap();
        let matrix = transform.compute_final_transform();
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, pass_kind == PassKind::Transparent, false)
            .color_blend_type(match pass_kind {
                PassKind::Opaque => ColorBlendType::Opaque,
                PassKind::Transparent => ColorBlendType::Layer1,
            });
        let draw = |pass: &mut RenderPass,
                    texture: &GpuTexture,
                    r: Vec4,
                    uv: Vec4,
                    alpha: f32,
                    flash: Option<f32>| {
            let color = shin_core::primitives::color::FloatColor4::WHITE.with_alpha(alpha);
            let vertices = QuadVertices::new()
                .with_box(
                    r.x - 960.0,
                    r.y - 540.0,
                    r.x + r.z - 960.0,
                    r.y + r.w - 540.0,
                )
                .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                .with_color(color.into_unorm());
            if let Some(flash) = flash {
                vertices.render_button(
                    pass,
                    builder,
                    texture.as_source(),
                    matrix,
                    Vec4::ZERO,
                    vec4(1.0, 1.0, 1.0, flash),
                );
            } else {
                let mut source = texture.as_source();
                if std::ptr::eq(texture, &self.textures.bg) {
                    source.sampler =
                        shin_render::shaders::types::texture::TextureSampler::LinearMirrorU;
                }
                vertices.render_sprite(pass, builder, source, matrix);
            }
        };
        let full = vec4(0.0, 0.0, 1.0, 1.0);
        let offset = model.slide.value;
        if pass_kind == PassKind::Opaque {
            // Native6e73c..6e840: opaque background writes strict stencil coverage.
            // Native6e7c4: one quad, U0..2 with mirrored-repeat sampler mode2.
            draw(
                pass,
                &self.textures.bg,
                vec4(-960.0 + offset, 0.0, 3840.0, 1080.0),
                vec4(0.0, 0.0, 2.0, 1.0),
                1.0,
                None,
            );
            return;
        }
        if offset != 0.0 {
            let choice = usize::from(offset < 0.0);
            let (plate_x, back_x, uv, back_uv, flash_x, flash_uv) = if choice == 0 {
                (
                    -288.0,
                    456.0,
                    vec4(0.0, 0.0, 0.5, 1.0),
                    vec4(0.5, 0.0, 1.0, 1.0),
                    -464.0,
                    full,
                )
            } else {
                (
                    1584.0,
                    984.0,
                    vec4(0.5, 0.0, 1.0, 1.0),
                    vec4(0.0, 0.0, 0.5, 1.0),
                    1466.0,
                    vec4(1.0, 0.0, 0.0, 1.0),
                )
            };
            draw(
                pass,
                &self.textures.back,
                vec4(back_x + offset, 816.0, 480.0, 176.0),
                back_uv,
                1.0,
                Some(model.back_flash.value),
            );

            draw(
                pass,
                &self.textures.shadow,
                vec4(plate_x + offset, 880.0, 624.0, 176.0),
                full,
                1.0,
                None,
            );
            draw(
                pass,
                &self.textures.plate,
                vec4(plate_x + offset, 880.0, 624.0, 176.0),
                uv,
                1.0,
                Some(model.confirm_flash.value),
            );

            let pulse = 0.2 + 0.2 * (model.phase * f32::from_bits(0x40c90fd0)).sin();
            draw(
                pass,
                &self.textures.flash_door,
                vec4(flash_x + offset, 48.0, 918.0, 844.0),
                flash_uv,
                pulse * model.door.value,
                None,
            );
            draw(
                pass,
                &self.textures.flash_door,
                vec4(flash_x + offset, 48.0, 918.0, 844.0),
                flash_uv,
                model.confirm_flash.value,
                None,
            );
        }
        if model.front.value > 0.0 {
            draw(
                pass,
                &self.textures.front,
                vec4(offset, 0.0, 1920.0, 1080.0),
                full,
                model.front.value,
                None,
            );
            for (index, rect) in CHOICES.into_iter().enumerate() {
                let rect = vec4(rect.x + offset, rect.y, rect.z, rect.w);
                let uv = vec4(index as f32 * 0.5, 0.0, (index + 1) as f32 * 0.5, 1.0);
                draw(
                    pass,
                    &self.textures.select,
                    rect,
                    uv,
                    model.front.value,
                    Some(model.choice_flashes[index].value),
                );
            }
            for x in [-960.0, 1920.0] {
                draw(
                    pass,
                    &self.textures._obi,
                    vec4(x + offset, 0.0, 960.0, 1080.0),
                    vec4(0.5, 0.0, 1.0, 1.0),
                    model.front.value,
                    None,
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_priorities_match_native_door_state_and_ignore_cancel() {
        use crate::app::AppAction;
        for choice in [None, Some(0), Some(1)] {
            for bits in 0..8 {
                let mut model = LastModel::default();
                if let Some(choice) = choice {
                    model.navigate(if choice == 0 { -1 } else { 1 });
                    model.update(7.0);
                    model.update(20.0);
                    model.take_sounds();
                }
                let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                input[AppAction::Left].is_clicked_or_repeated = bits & 1 != 0;
                input[AppAction::Right].is_clicked_or_repeated = bits & 2 != 0;
                input[AppAction::Enter].is_clicked = bits & 4 != 0;
                input[AppAction::Cancel].is_clicked = true;
                model.action_input(&input);
                let expected = match choice {
                    None if bits & 1 != 0 => Some(0),
                    None if bits & 2 != 0 => Some(1),
                    Some(_) if bits & 4 != 0 => choice,
                    Some(0) if bits & 2 != 0 => None,
                    Some(1) if bits & 1 != 0 => None,
                    _ => choice,
                };
                assert_eq!(model.choice, expected);
                assert_eq!(model.confirming, choice.is_some() && bits & 4 != 0);
                let cues = model.take_sounds();
                assert_eq!(
                    cues,
                    if model.confirming {
                        vec!["decide"]
                    } else if expected != choice {
                        vec!["move"]
                    } else {
                        vec![]
                    }
                );
                if model.confirming {
                    model.update(7.0);
                    assert_eq!(model.result, choice.map(|c| c as i32));
                }
            }
        }
        // Hover is a desktop pointer convenience, not a native controller choice.
        let mut model = LastModel::default();
        model.pointer(Vec2::new(400.0, 900.0), false);
        model.controller_input(false, false, true);
        assert_eq!(model.choice, None);
    }
    fn captured_sample(
        model: &mut LastModel,
        sampler: &mut crate::adv::input_area::PointerSampler,
        extent: (u16, u16),
        point: Vec2,
        held: bool,
        seconds: f64,
    ) {
        sampler.sample(
            point * Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
            held,
            seconds,
        );
        model.pointer_frame(&sampler.frame(), extent);
    }
    #[test]
    fn captured_last_taps_consume_controls_hold_drag_resize_and_retained_up_position() {
        for extent in [(960, 540), (1920, 1080), (3840, 2160)] {
            for choice in 0..2 {
                let mut model = LastModel::default();
                let mut sampler = crate::adv::input_area::PointerSampler::default();
                let rect = CHOICES[choice];
                let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
                captured_sample(&mut model, &mut sampler, extent, point, true, 0.0);
                model.controller_input(true, true, true);
                assert_eq!(model.choice, None);
                assert_eq!(model.pointer_row, Some(choice));
                captured_sample(&mut model, &mut sampler, extent, point, true, 0.149);
                assert_eq!(model.choice_flashes[choice].value, 0.0);
                captured_sample(&mut model, &mut sampler, extent, point, true, 0.002);
                model.update(1.0);
                assert!((model.choice_flashes[choice].value - 0.2).abs() < 1e-6);
                captured_sample(
                    &mut model,
                    &mut sampler,
                    extent,
                    point + Vec2::new(20.0, 0.0),
                    true,
                    0.01,
                );
                model.update(1.0);
                assert_eq!(model.pointer_row, None);
                assert_eq!(model.choice_flashes[choice].value, 0.0);
                assert!(model.take_sounds().is_empty());
                captured_sample(&mut model, &mut sampler, extent, point, false, 0.01);
                captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
                captured_sample(
                    &mut model,
                    &mut sampler,
                    extent,
                    Vec2::new(1900.0, 1000.0),
                    false,
                    0.1,
                );
                assert_eq!(model.choice, Some(choice));
                assert_eq!(model.pointer_row, None);
                assert_eq!(model.take_sounds(), ["move"]);
                model.update(7.0);
                model.update(20.0);
                let rect = confirmation_rects(choice)[1];
                let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
                captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
                model.controller_input(choice == 1, choice == 0, true);
                assert!(!model.confirming);
                assert_eq!(model.choice, Some(choice));
                captured_sample(&mut model, &mut sampler, extent, point, false, 0.1);
                assert!(model.confirming);
                assert_eq!(model.take_sounds(), ["decide"]);
                model.update(7.0);
                assert_eq!(model.result, Some(choice as i32));
            }
            let mut model = LastModel::default();
            let mut sampler = crate::adv::input_area::PointerSampler::default();
            let point = Vec2::new(400.0, 900.0);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.0);
            model.disable_pointer();
            assert_eq!(model.pointer_row, None);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.2);
            assert_eq!(
                model.pointer_row, None,
                "Already held pointer cannot synthesize a fresh Down"
            );
            captured_sample(&mut model, &mut sampler, extent, point, false, 0.01);
            assert_eq!(model.choice, None);
            captured_sample(&mut model, &mut sampler, extent, point, true, 0.01);
            captured_sample(&mut model, &mut sampler, (1280, 720), point, true, 0.2);
            if extent != (1280, 720) {
                assert_eq!(model.pointer_row, None, "Resize cancels old capture");
            }
            assert_eq!(model.choice, None);
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes original/1.03 Last controller/capture and seven native tween clocks"]
    fn reference_native_last_input_and_pending_clocks_match_production_model() {
        use crate::adv::input_area::InputAreaTarget;
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-last-input-oracle-{}", std::process::id()));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-last-input.c"))
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
        let start = |commands: &mut Vec<String>, choice: Option<usize>| {
            commands.push("N".to_owned());
            if let Some(choice) = choice {
                commands.extend([
                    format!(
                        "C {} {} 0",
                        usize::from(choice == 0),
                        usize::from(choice == 1)
                    ),
                    "T 7".to_owned(),
                    "T 20".to_owned(),
                ]);
            }
        };
        for choice in [None, Some(0), Some(1)] {
            for bits in 0..8 {
                for tick in [
                    0.0, 0.125, 0.25, 0.75, 1.0, 2.5, 3.5, 6.99, 7.0, 20.0, 1000.0,
                ] {
                    start(&mut commands, choice);
                    commands.push(format!(
                        "C {} {} {}",
                        bits & 1,
                        (bits >> 1) & 1,
                        (bits >> 2) & 1
                    ));
                    commands.push(format!("T {tick}"));
                    commands.extend(
                        [
                            "C 1 1 1", "T 0", "T 0.25", "T 3.25", "T 3.5", "T 20", "C 1 1 1",
                            "T 7", "T 20", "C 1 1 1", "T 7", "T 0",
                        ]
                        .map(str::to_owned),
                    );
                }
            }
            let rects = match choice {
                None => CHOICES.to_vec(),
                Some(c) => confirmation_rects(c).to_vec(),
            };
            for rect in &rects {
                start(&mut commands, choice);
                for p in [
                    Vec2::new(rect.x, rect.y),
                    Vec2::new(rect.x + rect.z, rect.y),
                    Vec2::new(rect.x, rect.y + rect.w),
                    Vec2::new(rect.x - 0.001, rect.y),
                    Vec2::new(rect.x, rect.y - 0.001),
                    Vec2::new(rect.x + rect.z - 0.001, rect.y + rect.w - 0.001),
                    Vec2::new(1900.0, 1079.0),
                ] {
                    commands.push(format!("P {} {}", p.x, p.y));
                }
            }
            for rect in rects {
                let point = Vec2::new(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
                for held in [0.0, 0.25, 1.0, 3.49, 3.5, 7.0, 20.0] {
                    for callback in ["R 1900 1079", "X", "B", "D", "L"] {
                        start(&mut commands, choice);
                        commands.extend([
                            format!("P {} {}", point.x, point.y),
                            "H 1900 1079".to_owned(),
                            format!("T {held}"),
                            callback.to_owned(),
                            "L".to_owned(),
                            "G".to_owned(),
                        ]);
                        commands.extend(
                            [
                                "T 0", "T 0.25", "T 1", "T 2.5", "T 3.5", "T 7", "T 20", "C 1 1 1",
                                "T 7", "T 20",
                            ]
                            .map(str::to_owned),
                        );
                    }
                }
            }
        }
        // Native delayed base clocks freeze whenever a choice/back flash is busy,
        // even if a single oversized update completes that flash.
        for choice in 0..2 {
            commands.push("N".to_owned());
            commands.push(format!(
                "C {} {} 0",
                usize::from(choice == 0),
                usize::from(choice == 1)
            ));
            commands.extend((0..116).map(|_| "T 0.25".to_owned()));
            commands.push("C 1 1 0".to_owned());
            commands.extend((0..116).map(|_| "T 0.25".to_owned()));
        }
        let payload = commands.join("\n") + "\n";
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
            let input = payload.clone();
            let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            assert!(
                output.status.success(),
                "{name}:{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(), commands.len());
            let mut model = LastModel::default();
            let (mut decide, mut moves) = (0, 0);
            for (step, (command, line)) in commands.iter().zip(output.lines()).enumerate() {
                let mut parts = command.split_whitespace();
                let op = parts.next().unwrap();
                let args: Vec<f32> = parts.map(|s| s.parse().unwrap()).collect();
                let mut consumed = -1;
                match op {
                    "N" => {
                        model = LastModel::default();
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
                    "L" => InputAreaTarget::reset(&mut model),
                    "G" => model.gesture(Vec2::ZERO),
                    "T" => model.update(args[0]),
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
                assert_eq!(fields.len(), 24);
                let integer = |i: usize| fields[i].parse::<i32>().unwrap();
                let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
                let choice = model.choice.map_or(-1, |c| c as i32);
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
                        choice,
                        if model.result.is_some() {
                            -1
                        } else if model.confirming {
                            1
                        } else {
                            0
                        },
                        model.pointer_row.map_or(-1, |r| r as i32),
                        model.result.is_some() as i32,
                        choice,
                        consumed,
                        decide,
                        moves
                    ],
                    "{name} step{step} {command}"
                );
                assert!(
                    (scalar(9) - model.phase).abs() < 2e-6,
                    "{name} step{step} phase {} vs {}",
                    scalar(9),
                    model.phase
                );
                let motions = [
                    &model.slide,
                    &model.front,
                    &model.door,
                    &model.choice_flashes[0],
                    &model.choice_flashes[1],
                    &model.back_flash,
                    &model.confirm_flash,
                ];
                for (field, motion) in motions.into_iter().enumerate() {
                    let actual = scalar(10 + field);
                    let tolerance = if field == 0 { 0.0002 } else { 2e-6 };
                    assert!(
                        (actual - motion.value).abs() < tolerance,
                        "{name} step{step} {command} field{field}:native{actual} port{}",
                        motion.value
                    );
                    assert_eq!(
                        integer(17 + field),
                        motion.busy() as i32,
                        "{name} step{step} {command} busy{field}"
                    );
                }
            }
            eprintln!(
                "{name}:{} Last native/production command steps match choice/phase/capture/getter/cues/seven values andbusy flags",
                commands.len()
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    fn completed_choice_restore_preserves_visuals_without_replaying_input() {
        use super::super::cake_layer::CakeModel;
        for answer in 0..15 {
            let mut cake = CakeModel::default();
            cake.restore_completed_answer(answer);
            cake.navigate(1);
            cake.confirm();
            cake.pointer(Vec2::new(960.0, 700.0), true);
            assert_eq!(cake.selected, None);
            assert_eq!(cake.visual_slice(), Some(answer as usize));
            assert_eq!(cake.result, Some(-1));
            cake.reset();
            assert_eq!(cake.visual_slice(), None);
            assert_eq!(cake.result, None);
        }
        for answer in [0, 1] {
            let mut model = LastModel::default();
            model.restore_completed_answer(answer);
            model.navigate(1);
            model.confirm();
            model.back();
            model.pointer(Vec2::new(960.0, 300.0), true);
            model.update(120.0);
            assert_eq!(model.choice, None);
            assert_eq!(model.result, Some(-1));
            assert_eq!(model.slide.value, if answer == 0 { 960.0 } else { -960.0 });
            assert_eq!(model.door.value, 0.0);
            assert!(!model.accepts_input());
            assert!(model.take_sounds().is_empty());
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "executes guarded original/1.03 Cake and Last completed-save callbacks"]
    fn reference_native_completed_cake_and_last_restoration() {
        use super::super::cake_layer::CakeModel;
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-choice-restore-oracle-{}", std::process::id()));
        let compile = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-completed-choice-restoration.c"))
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
        for prior in -1..15 {
            for answer in (-1..=15).chain([99]) {
                requests += &format!("1 {answer} {prior}\n");
            }
        }
        for prior in -1..2 {
            for answer in [-1, 0, 1, 2, 99] {
                requests += &format!("3 {answer} {prior}\n");
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
            assert_eq!(output.lines().count(), 303);
            for line in output.lines() {
                let values: Vec<f32> = line
                    .split_whitespace()
                    .map(|v| v.parse().unwrap())
                    .collect();
                let answer = values[1] as i32;
                let selected = (values[2] >= 0.0).then_some(values[2] as usize);
                if values[0] == 1.0 {
                    let mut model = CakeModel::default();
                    model.selected = selected;
                    model.restore_completed_answer(answer);
                    assert_eq!(model.selected, selected);
                    assert_eq!(model.result, Some(values[3] as i32));
                    assert_eq!(
                        model.reference_tween_values().as_slice(),
                        &values[4..],
                        "{name}: {line}"
                    );
                } else {
                    let mut model = LastModel::default();
                    model.choice = selected;
                    // Exercise native reset of pre-existing idle values as well.
                    model.slide.value = 0.25;
                    model.front.value = 0.25;
                    model.door.value = 0.25;
                    model.restore_completed_answer(answer);
                    assert_eq!(model.choice, selected);
                    assert_eq!(model.result, Some(values[3] as i32));
                    let actual = [
                        model.slide.value,
                        model.front.value,
                        model.door.value,
                        model.choice_flashes[0].value,
                        model.choice_flashes[1].value,
                        model.back_flash.value,
                        model.confirm_flash.value,
                    ];
                    assert_eq!(&actual, &values[4..], "{name}: {line}");
                    assert!(
                        model.slide.queue.is_empty()
                            && model.front.queue.is_empty()
                            && model.door.queue.is_empty()
                    );
                    assert!(
                        model
                            .choice_flashes
                            .iter()
                            .all(|flash| flash.queue.is_empty())
                    );
                    assert!(
                        model.back_flash.queue.is_empty() && model.confirm_flash.queue.is_empty()
                    );
                }
            }
            let bytes = std::fs::read(&exe).unwrap();
            let delta = if name.ends_with("update") { 0xa0 } else { 0 };
            for (address, words) in [
                (
                    0x44e98,
                    &[
                        0x91012260u32,
                        0x9401d981,
                        0x4ea01c08,
                        0x1e202008,
                        0x5400088d,
                    ][..],
                ),
                (0x44448, &[0x12800008u32, 0xf9005268, 0xb900aa68][..]),
                (0x6daa4, &[0xb2607fe8u32, 0xf9014a68, 0xf9014e68][..]),
            ] {
                for (offset, &word) in words.iter().enumerate() {
                    let address = address - delta + offset * 4 + 256;
                    assert_eq!(
                        u32::from_le_bytes(bytes[address..address + 4].try_into().unwrap()),
                        word,
                        "{name}: native message/pointer draw gate or constructor retained-selection sentinel changed"
                    );
                }
            }
            for address in [0x45330, 0x6f930, 0xbb2e0] {
                let address = if name.ends_with("update") {
                    address - 0xa0
                } else {
                    address
                };
                let mut bytes = std::fs::read(&exe).unwrap();
                bytes[address + 256] ^= 1;
                let altered = std::env::temp_dir()
                    .join(format!("shin-choice-altered-{}", std::process::id()));
                std::fs::write(&altered, bytes).unwrap();
                let rejected = Command::new(&binary).arg(&altered).output().unwrap();
                std::fs::remove_file(altered).unwrap();
                assert_eq!(rejected.status.code(), Some(2));
                assert!(String::from_utf8_lossy(&rejected.stderr).contains("Unreviewed"));
            }
            eprintln!(
                "{name}:303 native Cake/Last completed restore cases, retained selection/result, 31 Cake and seven Last tween values; altered callbacks/reset rejected before execution"
            );
        }
        std::fs::remove_file(binary).unwrap();
    }

    #[test]
    fn two_stage_native_hits_confirm_original_response_and_back_without_answer() {
        for choice in 0..2 {
            let mut model = LastModel::default();
            model.confirm();
            assert_eq!(model.choice, None);
            let rect = CHOICES[choice];
            model.pointer(Vec2::new(rect.x + 100.0, 900.0), true);
            assert_eq!(model.choice, Some(choice));
            assert_eq!(model.result, None);
            model.update(7.0);
            model.update(20.0);
            let back = confirmation_rects(choice)[2];
            model.pointer(Vec2::new(back.x + 100.0, 900.0), true);
            assert_eq!(model.choice, None);
            assert_eq!(model.result, None);
            model.update(7.0);
            model.update(20.0);
            model.pointer(Vec2::new(rect.x + 100.0, 900.0), true);
            model.update(7.0);
            model.update(20.0);
            model.pointer(Vec2::new(960.0, 300.0), true);
            assert_eq!(model.result, None);
            model.update(7.0);
            assert_eq!(model.result, Some(choice as i32));
            model.back();
            model.navigate(1);
            model.confirm();
            assert_eq!(model.result, Some(choice as i32));
        }
    }
    #[test]
    fn invalid_hits_and_keyboard_require_selection_before_confirming() {
        let mut model = LastModel::default();
        for point in [
            Vec2::ZERO,
            Vec2::new(f32::NAN, 900.0),
            Vec2::new(896.0, 900.0),
        ] {
            model.pointer(point, true);
            assert_eq!(model.choice, None);
        }
        model.navigate(-1);
        assert_eq!(model.choice, Some(0));
        assert_eq!(model.result, None);
        model.update(7.0);
        model.update(20.0);
        model.confirm();
        assert_eq!(model.result, None);
        model.update(7.0);
        assert_eq!(model.result, Some(0));
    }
    #[test]
    fn slide_waits_for_flash_locks_input_and_confirmation_finishes_after_flash() {
        let mut model = LastModel::default();
        model.navigate(-1);
        model.confirm();
        model.back();
        model.navigate(1);
        assert_eq!(model.choice, Some(0));
        assert_eq!(model.result, None);
        model.update(3.5);
        assert_eq!(model.slide.value, 0.0);
        assert!((model.choice_flashes[0].value - 0.7).abs() < 1e-5);
        model.update(3.5);
        assert_eq!(model.slide.value, 0.0);
        model.update(10.0);
        assert!((model.slide.value - 960.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert_eq!(model.front.value, 0.5);
        assert_eq!(model.door.value, 0.5);
        model.pointer(Vec2::new(960.0, 300.0), true);
        assert!(!model.confirming);
        model.update(10.0);
        model.confirm();
        assert!(model.confirming);
        model.update(5.0);
        assert_eq!(model.door.value, 0.0);
        assert_eq!(model.result, None);
        model.pointer(Vec2::new(1700.0, 900.0), true);
        model.back();
        assert_eq!(model.choice, Some(0));
        model.update(1.99);
        assert_eq!(model.result, None);
        model.update(0.01);
        assert_eq!(model.result, Some(0));
    }
    #[test]
    fn animation_rejects_nonfinite_time_and_back_reverses_slide() {
        let mut model = LastModel::default();
        model.navigate(1);
        model.update(f32::NAN);
        model.update(-1.0);
        model.update(f32::INFINITY);
        assert_eq!(model.slide.value, 0.0);
        model.update(7.0);
        model.update(20.0);
        assert_eq!(model.slide.value, -960.0);
        model.back();
        model.update(7.0);
        assert_eq!(model.slide.value, -960.0);
        model.update(10.0);
        assert_eq!(model.front.value, 0.5);
        model.update(10.0);
        assert_eq!(model.slide.value, 0.0);
        assert_eq!(model.front.value, 1.0);
        assert_eq!(model.door.value, 0.0);
        assert_eq!(model.result, None);
        model.navigate(-1);
        assert_eq!(model.choice, Some(0));
    }
    #[test]
    fn hover_uses_native_speed_and_click_finishes_from_current_brightness() {
        let mut model = LastModel::default();
        model.pointer(Vec2::new(400.0, 900.0), false);
        model.update(1.0);
        assert!((model.choice_flashes[0].value - 0.2).abs() < 1e-5);
        model.update(2.5);
        assert_eq!(model.choice_flashes[0].value, 0.7);
        model.pointer(Vec2::new(400.0, 900.0), true);
        model.update(3.5);
        assert_eq!(model.slide.value, 0.0);
        model.update(20.0);
        model.pointer(Vec2::new(960.0, 300.0), false);
        model.update(3.5);
        assert_eq!(model.confirm_flash.value, 0.7);
        model.pointer(Vec2::new(960.0, 300.0), true);
        model.update(3.49);
        assert_eq!(model.result, None);
        model.update(0.01);
        assert_eq!(model.result, Some(0));
    }
    #[test]
    fn native_sound_events_follow_accepted_actions_and_ignore_hover_and_locked_input() {
        let mut model = LastModel::default();
        model.pointer(Vec2::new(400.0, 900.0), false);
        assert!(model.take_sounds().is_empty());
        model.pointer(Vec2::new(400.0, 900.0), true);
        assert_eq!(model.take_sounds(), ["move"]);
        model.confirm();
        model.back();
        model.navigate(1);
        assert!(model.take_sounds().is_empty());
        model.update(7.0);
        model.update(20.0);
        model.back();
        assert_eq!(model.take_sounds(), ["move"]);
        model.update(7.0);
        model.update(20.0);
        model.navigate(1);
        model.update(7.0);
        model.update(20.0);
        model.confirm();
        assert_eq!(model.take_sounds(), ["move", "decide"]);
        model.update(7.0);
        model.confirm();
        model.back();
        assert!(model.take_sounds().is_empty());
    }
}

#[cfg(test)]
impl LastLayer {
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
            model.choice = Some(1);
        }
        let LastModel {
            slide,
            front,
            door,
            choice_flashes: [a, b],
            back_flash,
            confirm_flash,
            ..
        } = &mut *model;
        [slide, front, door, a, b, back_flash, confirm_flash]
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                if command == "populate" || (command == "mutate" && i == 0) {
                    let value = if command == "mutate" {
                        999.0
                    } else {
                        100.0 + i as f32
                    };
                    *t = Motion::new(value);
                    if command == "populate" {
                        for j in 0..queued {
                            t.queue
                                .push_back((value + (j + 1) as f32 * 0.25, 10.0, false));
                        }
                    }
                } else if command == "advance" {
                    t.update(delta);
                }
                (t.value.to_bits(), t.busy())
            })
            .collect()
    }
}
