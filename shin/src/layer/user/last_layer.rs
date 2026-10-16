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
    #[cfg(test)]
    pub fn advance_animation(&self, ticks: f32) {
        self.inner_ref().model.lock().unwrap().update(ticks);
    }
    pub fn result(&self) -> Option<i32> {
        self.inner_ref().model.lock().unwrap().result
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
mod tests;

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

// Owned interactive views dispatch through the same actual layer.
impl super::InteractiveView<'_, LastLayer> {
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        self.with(|layer| layer.reference_controller_state())
    }
    pub fn navigate(&self, delta: i32) {
        self.with(|layer| layer.navigate(delta))
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        self.with(|layer| layer.handle_controller_input(state))
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        self.with(|layer| layer.pointer_frame(frame, extent))
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.with(|layer| layer.ignore_pointer_state())
    }
    #[cfg(test)]
    pub(crate) fn reference_accepts_input(&self) -> bool {
        self.with(|layer| layer.reference_accepts_input())
    }
    #[cfg(test)]
    pub(crate) fn reference_pointer_feedback(&self) -> (Option<usize>, bool, [f32; 7]) {
        self.with(|layer| layer.reference_pointer_feedback())
    }
    pub fn confirm(&self) {
        self.with(|layer| layer.confirm())
    }
    pub fn back(&self) {
        self.with(|layer| layer.back())
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.with(|layer| layer.pointer(position, clicked))
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        self.with(|layer| layer.take_sounds())
    }
    pub fn result(&self) -> Option<i32> {
        self.with(|layer| layer.result())
    }
}
