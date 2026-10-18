//! Native ADV15130..163fc: captured navi buttons and full-screen flicks.
use super::input_area::{InputArea, InputAreaTarget, MouseRelease, PointerFrame};
use glam::{Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

pub(super) const POSITIONS: [Vec2; 5] = [
    vec2(1332.0, 160.0),
    vec2(1524.0, 160.0),
    vec2(1716.0, 160.0),
    vec2(1428.0, 292.0),
    vec2(1620.0, 292.0),
];
const SIZE: Vec2 = vec2(192.0, 120.0);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Request {
    Advance,
    EventHint,
    EventSkip,
    Auto,
    Skip,
    Backlog,
    Previous,
    Next,
    Menu,
    Hide,
    Sound(&'static str),
}
#[derive(Default, Clone, Copy)]
pub(super) struct Eligibility {
    pub input: bool,
    pub system: bool,
    pub event: bool,
    pub modes: bool,
    pub hide: bool,
    pub advance: bool,
}
struct Contact {
    open: bool,
    slide: Tweener,
    enabled: [bool; 5],
    brightness: [Tweener; 5],
    flashes: [Tweener; 5],
    capture: i32,
    toolbar_handled: bool,
    eligibility: Eligibility,
    requests: Vec<Request>,
    backlog_pending: bool,
    idle_ticks: f32,
}
impl Contact {
    fn new() -> Self {
        Self {
            open: false,
            slide: Tweener::new(0.0),
            enabled: [false; 5],
            brightness: std::array::from_fn(|_| Tweener::new(0.0)),
            flashes: std::array::from_fn(|_| Tweener::new(0.0)),
            capture: -1,
            toolbar_handled: false,
            eligibility: Eligibility::default(),
            requests: Vec::new(),
            backlog_pending: false,
            idle_ticks: 0.0,
        }
    }
    fn fully_open(&self) -> bool {
        self.open && (self.slide.value() - 1.0).abs() < f32::EPSILON
    }
    fn hit(&self, position: Vec2) -> i32 {
        if !self.fully_open() {
            return -1;
        }
        for (index, origin) in POSITIONS.iter().enumerate() {
            let p = position - *origin;
            if p.cmpge(Vec2::ZERO).all() && p.cmplt(SIZE).all() {
                return if self.enabled[index] {
                    index as i32 + 1
                } else {
                    0
                };
            }
        }
        -1
    }
    fn move_to(clock: &mut Tweener, target: f32, speed: f32) {
        let current = clock.value();
        clock.fast_forward_to(current);
        clock.enqueue(
            target,
            Tween::linear(Ticks::from_f32((target - current).abs() / speed)),
        );
    }
    fn close(&mut self) {
        if self.open {
            Self::move_to(&mut self.slide, 0.0, 0.1);
            self.open = false;
        }
    }
    fn flash(&mut self, index: usize, release: bool) {
        Self::move_to(&mut self.flashes[index], 0.8, 0.2);
        if release {
            self.flashes[index].enqueue(0.0, Tween::linear(Ticks::from_f32(4.0)));
        }
    }
    fn cancel_button(&mut self) {
        if self.capture > 0 {
            self.toolbar_handled = true;
            Self::move_to(&mut self.flashes[self.capture as usize - 1], 0.0, 0.2);
        }
        self.capture = 0;
    }
    fn synchronize(&mut self, eligibility: Eligibility) {
        self.eligibility = eligibility;
        let sys = eligibility.input && eligibility.system;
        let desired = [
            sys && eligibility.modes,
            sys && eligibility.modes,
            sys && !eligibility.event,
            sys,
            sys,
        ];
        for (index, desired) in desired.into_iter().enumerate() {
            if self.enabled[index] != desired {
                self.enabled[index] = desired;
                Self::move_to(
                    &mut self.brightness[index],
                    f32::from(u8::from(desired)),
                    if desired { 0.25 } else { 0.1 },
                );
                if !desired {
                    Self::move_to(&mut self.flashes[index], 0.0, 0.2);
                }
            }
        }
        if !eligibility.system {
            self.close();
        }
    }
    fn gesture_direction(&mut self, direction: Vec2) {
        // Use the same f32 divisor as native1594bc, not a rounded angle.
        let degrees = direction.y.atan2(direction.x) / 3.141592025756836_f32 * 180.0;
        if degrees > -30.0 && degrees < 30.0 {
            if self.fully_open() {
                self.requests.push(Request::Sound("page"));
                self.close();
            } else if self.eligibility.system && !self.eligibility.event {
                self.requests.push(Request::Menu);
            }
        } else if degrees < -150.0 || degrees > 150.0 {
            if self.eligibility.system {
                self.requests.push(Request::Sound("page"));
                if !self.open {
                    Self::move_to(&mut self.slide, 1.0, 0.1);
                    self.open = true;
                }
            }
        } else if degrees > 60.0
            && degrees < 120.0
            && self.eligibility.system
            && !self.eligibility.event
            && self.eligibility.hide
        {
            self.requests.push(Request::Hide);
        }
    }

    fn update(&mut self, ticks: Ticks) {
        // Native19260 checks before advancing the button clock, like other
        // FunctionTasks. A revoked/closed button cannot dispatch afterward.
        if self.backlog_pending && self.flashes[2].is_idle() {
            self.backlog_pending = false;
            if self.fully_open() && self.enabled[2] {
                self.requests.push(Request::Backlog);
            }
        }
        self.slide.update(ticks);
        for clock in self.brightness.iter_mut().chain(self.flashes.iter_mut()) {
            clock.update(ticks);
        }
        // fb770 measures microseconds and caps each clock step at100ms.
        self.idle_ticks += ticks.as_f32().clamp(0.0, 6.0);
        if self.idle_ticks >= 3600.0 {
            self.close();
        }
    }
}
impl InputAreaTarget for Contact {
    fn probe(&mut self, position: Vec2) -> bool {
        self.capture = self.hit(position);
        self.toolbar_handled = self.capture >= 0;
        true
    }
    fn reset(&mut self) {
        self.capture = -1;
    }
    fn press(&mut self, _: Vec2) {
        if self.capture > 0 && self.fully_open() && self.enabled[self.capture as usize - 1] {
            self.flash(self.capture as usize - 1, false);
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.capture < 0 {
            if self.eligibility.system && self.eligibility.event {
                self.requests.push(Request::EventHint);
            } else if self.eligibility.advance {
                self.requests.push(Request::Advance);
            }
        } else if self.capture > 0 && self.fully_open() && self.enabled[self.capture as usize - 1] {
            let index = self.capture as usize - 1;
            self.flash(index, true);
            match index {
                0 => self.requests.push(Request::Auto),
                1 => self.requests.push(Request::Skip),
                2 => {
                    self.requests.push(Request::Sound("decide"));
                    self.backlog_pending = true;
                }
                3 => self.requests.push(Request::Previous),
                4 => self.requests.push(Request::Next),
                _ => unreachable!(),
            }
        }
    }
    fn long_press(&mut self, _: Vec2) {
        if self.capture < 0 && self.eligibility.system && self.eligibility.event {
            self.requests.push(Request::EventSkip);
        }
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {
        if self.capture >= 0 {
            self.cancel_button();
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.capture >= 0 || velocity.length() <= 20.0 {
            return;
        }
        self.gesture_direction(velocity);
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        tracing::debug!(
            ?displacement,
            ?velocity,
            capture = self.capture,
            system = self.eligibility.system,
            event = self.eligibility.event,
            "Desktop story drag released"
        );
        if self.capture >= 0 {
            return;
        }
        // A deliberate desktop drag must work without a touch-flick release
        // speed (over1200 canvas px/s) or a precisely timed mouse-button Up.
        // Four times the native drag threshold separates this from tap jitter.
        self.gesture(crate::adv::input_area::desktop_gesture_vector(
            displacement,
            velocity,
        ));
    }

    fn cancel(&mut self) {
        self.cancel_button();
    }
}

pub(super) struct StoryTouch {
    area: InputArea,
    contact: Contact,
    extent: (u16, u16),
    last_frame: Option<u32>,
}
impl StoryTouch {
    pub fn new() -> Self {
        let mut area = InputArea::canvas();
        area.set_long_press_enabled(true);
        Self {
            area,
            contact: Contact::new(),
            extent: (1920, 1080),
            last_frame: None,
        }
    }
    pub fn update(&mut self, ticks: Ticks) {
        self.contact.update(ticks);
    }
    pub fn configure(&mut self, eligibility: Eligibility) {
        self.contact.synchronize(eligibility);
    }
    pub fn close(&mut self) {
        self.contact.close();
    }
    pub fn blocks_input(&self) -> bool {
        self.contact.backlog_pending
    }
    #[cfg(test)]
    pub(super) fn reference_toolbar_open(&self) -> bool {
        self.contact.open
    }
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.contact.requests)
    }
    pub fn disable(&mut self) {
        self.area.disable(&mut self.contact);
        self.contact.requests.clear();
    }
    /// Return consumed input and whether a toolbar contact owns this frame.
    /// Outside contacts still forward to interactive layer/choice mouse owners.
    #[cfg(test)]
    pub fn pointer(
        &mut self,
        frame: &PointerFrame<'_>,
        extent: (u16, u16),
        eligibility: Eligibility,
    ) -> (bool, bool) {
        self.pointer_with_mouse_release(frame, extent, eligibility, None)
    }
    fn pointer_with_mouse_release(
        &mut self,
        frame: &PointerFrame<'_>,
        extent: (u16, u16),
        eligibility: Eligibility,
        mouse_release: Option<MouseRelease>,
    ) -> (bool, bool) {
        self.contact.synchronize(eligibility);
        if self.last_frame != Some(frame.number) {
            self.contact.toolbar_handled = self.contact.capture >= 0;
            self.last_frame = Some(frame.number);
        }
        if extent.0 == 0 || extent.1 == 0 || !eligibility.input {
            self.disable();
            return (false, false);
        }
        if self.blocks_input() {
            // Keep this frame's release sound until the ADV drains it. The
            // deferred Backlog task owns input, including interactive children.
            self.area.disable(&mut self.contact);
            return (false, true);
        }
        if extent != self.extent {
            self.disable();
            self.area = InputArea::new(extent.0, extent.1);
            self.area.set_long_press_enabled(true);
            self.extent = extent;
        }
        let consumed = self
            .area
            .update_with_mouse_release(&mut self.contact, frame, mouse_release);
        if consumed {
            self.contact.idle_ticks = 0.0;
        }
        (consumed, self.contact.toolbar_handled)
    }
    pub fn render(&self, texture: &GpuTexture, pass: &mut RenderPass) {
        let slide = self.contact.slide.value();
        if slide <= 0.0 {
            return;
        }
        for (index, origin) in POSITIONS.iter().enumerate() {
            let origin = *origin + vec2((1.0 - slide) * 588.0, 0.0);
            let tint = self.contact.brightness[index].value() * 0.5 + 0.5;
            let color = FloatColor4::from_rgba(tint, tint, tint, 1.0).into_unorm();
            let row = index as f32 * 0.2;
            let vertices =
                shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
                    position: vec3(
                        origin.x + SIZE.x * corner.x,
                        origin.y + SIZE.y * corner.y,
                        0.0,
                    ),
                    texture_position: vec2(corner.x, row + 0.2 * corner.y),
                    color,
                });
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .build(
                        RenderProgramWithArguments::Button {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: texture.as_source(),
                            transform: crate::render::top_left_projection_matrix(),
                            flash1: Vec4::ZERO,
                            flash2: vec4(1.0, 1.0, 1.0, self.contact.flashes[index].value()),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        }
    }
}

impl super::Adv {
    pub(super) fn story_touch_eligibility(&self) -> Eligibility {
        let overlay = self.active_interactive_layer();
        let input = !self.finished
            && !self.is_loading_saved_scene()
            && self.pending_save.is_none()
            && self.progress_error.is_none()
            && self.restore_error_dialog.is_none()
            && self.chapter_dialog.is_none()
            && self.system_menu.is_none()
            && self.adv_state.modal_backdrop.is_none()
            && self.adv_state.backlog.is_none()
            && !self.adv_state.hidden_ui_active
            && self.adv_state.character_screen.is_none()
            && self.adv_state.ui_hide_count == 0
            && self.adv_state.audio_pause_count == 0;
        Eligibility {
            input,
            system: self.vm_state.system_controls_allowed,
            event: self.vm_state.current_event.is_some(),
            modes: overlay
                .as_ref()
                .is_none_or(|layer| layer.allows_playback_mode_changes()),
            hide: self
                .adv_state
                .choice_menu
                .as_ref()
                .is_some_and(super::choice::ChoiceMenu::can_hide)
                || self.adv_state.message_layer().is_interested_in_input(),
            advance: self.vm_state.current_event.is_none()
                && overlay.is_none()
                && self.adv_state.choice_menu.is_none(),
        }
    }
    pub(super) fn process_story_pointer(&mut self) -> (bool, bool) {
        let eligibility = self.story_touch_eligibility();
        self.story_touch.pointer_with_mouse_release(
            &self.modal_pointer.frame(),
            self.modal_pointer_extent,
            eligibility,
            self.modal_pointer.mouse_release(),
        )
    }
    pub(super) fn apply_story_touch_requests(&mut self) {
        for request in self.story_touch.take_requests() {
            tracing::debug!(?request, "Story pointer action");
            match request {
                Request::Advance => {
                    if !self
                        .current_command
                        .as_mut()
                        .is_some_and(super::UpdatableCommand::advance)
                    {
                        self.adv_state.message_layer_mut().try_advance();
                    }
                }
                Request::EventHint => self.event_skip.show_hint(true),
                Request::EventSkip => self.pending_event_skip = true,
                Request::Auto => {
                    self.adv_state.playback_modes.toggle_autoplay();
                    self.pending_playback_sound = Some(if self.adv_state.playback_modes.autoplay {
                        "son"
                    } else {
                        "cancel"
                    });
                }
                Request::Skip => {
                    self.adv_state.playback_modes.toggle_skip();
                    self.pending_playback_sound = Some(if self.adv_state.playback_modes.skip {
                        "son"
                    } else {
                        "cancel"
                    });
                }
                Request::Backlog => self.pending_backlog = true,
                Request::Previous => {
                    self.pending_chapter = Some(super::chapter_navigation::Direction::Previous)
                }
                Request::Next => {
                    self.pending_chapter = Some(super::chapter_navigation::Direction::Next)
                }
                Request::Menu => self.pending_system_menu = true,
                Request::Hide => {
                    self.story_touch.close();
                    self.adv_state.playback_modes.reset();
                    self.adv_state.hidden_ui_active = true;
                    self.adv_state.acquire_ui_hide();
                }
                Request::Sound(sound) => self.pending_playback_sound = Some(sound),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::input_area::PointerSampler;
    use super::*;
    fn eligibility(event: bool) -> Eligibility {
        Eligibility {
            input: true,
            system: true,
            event,
            modes: true,
            hide: true,
            advance: !event,
        }
    }
    fn open(touch: &mut StoryTouch) {
        touch.contact.synchronize(eligibility(false));
        touch.contact.gesture(vec2(-30.0, 0.0));
        touch.update(Ticks::from_f32(10.0));
        touch.take_requests();
        assert!(touch.contact.fully_open());
    }
    fn desktop_sample(
        touch: &mut StoryTouch,
        sampler: &mut PointerSampler,
        extent: (u16, u16),
        position: Vec2,
        held: bool,
        elapsed: f64,
        desktop: bool,
    ) {
        sampler.sample(position, held, elapsed);
        touch.pointer_with_mouse_release(
            &sampler.frame(),
            extent,
            eligibility(false),
            if desktop {
                sampler.mouse_release()
            } else {
                None
            },
        );
    }
    #[test]
    fn desktop_flick_retains_recent_motion_across_stationary_release_frames() {
        for width in [960, 1920, 3840] {
            let extent = (width, width * 9 / 16);
            let scale = width as f32 / 1920.0;
            for fps in [60.0, 120.0] {
                for direction in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0)] {
                    for desktop in [false, true] {
                        let mut touch = StoryTouch::new();
                        let mut sampler = PointerSampler::default();
                        let start = vec2(800.0, 500.0) * scale;
                        desktop_sample(
                            &mut touch,
                            &mut sampler,
                            extent,
                            start,
                            true,
                            1.0 / fps,
                            desktop,
                        );
                        let end = start + direction * 240.0 * scale;
                        desktop_sample(
                            &mut touch,
                            &mut sampler,
                            extent,
                            end,
                            true,
                            1.0 / fps,
                            desktop,
                        );
                        desktop_sample(
                            &mut touch,
                            &mut sampler,
                            extent,
                            end,
                            true,
                            1.0 / fps,
                            desktop,
                        );
                        desktop_sample(
                            &mut touch,
                            &mut sampler,
                            extent,
                            end,
                            false,
                            1.0 / fps,
                            desktop,
                        );
                        // Both pointer routing and action handling read this frame.
                        touch.pointer_with_mouse_release(
                            &sampler.frame(),
                            extent,
                            eligibility(false),
                            if desktop {
                                sampler.mouse_release()
                            } else {
                                None
                            },
                        );
                        let expected = if !desktop {
                            vec![]
                        } else if direction.x < 0.0 {
                            vec![Request::Sound("page")]
                        } else if direction.x > 0.0 {
                            vec![Request::Menu]
                        } else {
                            vec![Request::Hide]
                        };
                        assert_eq!(
                            touch.take_requests(),
                            expected,
                            "width{width} fps{fps} direction{direction} desktop{desktop}"
                        );
                        assert_eq!(touch.contact.open, desktop && direction.x < 0.0);
                    }
                }
            }
        }
    }
    #[test]
    fn desktop_flick_includes_movement_on_release_and_closes_an_open_toolbar() {
        let mut touch = StoryTouch::new();
        let mut sampler = PointerSampler::default();
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            vec2(800.0, 500.0),
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            vec2(500.0, 500.0),
            false,
            0.016,
            true,
        );
        assert_eq!(touch.take_requests(), [Request::Sound("page")]);
        touch.update(Ticks::from_f32(10.0));
        assert!(touch.contact.fully_open());
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            vec2(500.0, 500.0),
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            vec2(800.0, 500.0),
            false,
            0.016,
            true,
        );
        assert_eq!(touch.take_requests(), [Request::Sound("page")]);
        assert!(!touch.contact.open);
    }
    #[test]
    fn desktop_drags_keep_taps_cancel_and_toolbar_button_capture_distinct() {
        let mut touch = StoryTouch::new();
        let mut sampler = PointerSampler::default();
        let start = vec2(800.0, 500.0);
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            start,
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            start,
            false,
            0.016,
            true,
        );
        assert_eq!(touch.take_requests(), [Request::Advance]);
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            start,
            true,
            0.016,
            true,
        );
        let end = start - vec2(300.0, 0.0);
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            end,
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            end,
            true,
            0.15,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            end,
            false,
            0.016,
            true,
        );
        assert_eq!(
            touch.take_requests(),
            [Request::Sound("page")],
            "A deliberate desktop drag survives a pause before release"
        );
        assert!(touch.contact.open);
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            start,
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            end,
            true,
            0.016,
            true,
        );
        sampler.cancel_contact(true);
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            end,
            false,
            0.016,
            true,
        );
        assert!(
            touch.take_requests().is_empty(),
            "Cancellation must not emit a flick"
        );
        open(&mut touch);
        let button = POSITIONS[4] + SIZE * 0.5;
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            button,
            true,
            0.016,
            true,
        );
        desktop_sample(
            &mut touch,
            &mut sampler,
            (1920, 1080),
            button - vec2(300.0, 0.0),
            false,
            0.016,
            true,
        );
        assert!(
            touch.take_requests().is_empty(),
            "Dragging a captured toolbar button must not activate another action"
        );
    }
    #[test]
    fn desktop_directional_drag_threshold_preserves_native_flicks_and_tap_jitter() {
        for displacement in [vec2(-63.0, 0.0), vec2(63.0, 0.0), vec2(0.0, 63.0)] {
            let mut contact = Contact::new();
            contact.synchronize(eligibility(false));
            contact.mouse_gesture(displacement, Vec2::ZERO);
            assert!(contact.requests.is_empty());
        }
        for (displacement, expected) in [
            (vec2(-64.0, 0.0), Request::Sound("page")),
            (vec2(64.0, 0.0), Request::Menu),
            (vec2(0.0, 64.0), Request::Hide),
        ] {
            let mut contact = Contact::new();
            contact.synchronize(eligibility(false));
            contact.mouse_gesture(displacement, Vec2::ZERO);
            assert_eq!(contact.requests, [expected]);
        }
        let mut contact = Contact::new();
        contact.synchronize(eligibility(false));
        contact.gesture(vec2(-10.0, 0.0));
        assert!(
            contact.requests.is_empty(),
            "Native touch speed threshold retained"
        );
        contact.mouse_gesture(vec2(-32.0, 0.0), vec2(-30.0, 0.0));
        assert_eq!(
            contact.requests,
            [Request::Sound("page")],
            "Short fast mouse flick retained"
        );
    }
    #[test]
    fn deferred_backlog_keeps_release_feedback_and_input_loss_discards_actions() {
        let mut touch = StoryTouch::new();
        let mut sampler = PointerSampler::default();
        open(&mut touch);
        let point = POSITIONS[2] + SIZE * 0.5;
        sampler.sample(point, true, 0.01);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        sampler.sample(point, false, 0.2);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        // ADV pointer forwarding and keyboard handling share this frame.
        assert_eq!(
            touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false)),
            (false, true)
        );
        assert_eq!(touch.take_requests(), vec![Request::Sound("decide")]);
        assert!(touch.blocks_input());
        // A release queued before focus/input loss cannot leak into a later frame.
        touch.contact.requests.push(Request::Next);
        touch.disable();
        assert!(touch.take_requests().is_empty());
    }
    #[test]
    fn captured_chapter_button_ignores_drag_retargeting_disabled_rows_and_resize() {
        let mut touch = StoryTouch::new();
        open(&mut touch);
        let mut sampler = PointerSampler::default();
        let point = POSITIONS[4] + SIZE * 0.5;
        sampler.sample(point, true, 0.0);
        assert_eq!(
            touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false)),
            (true, true)
        );
        assert!(touch.take_requests().is_empty());
        sampler.sample(POSITIONS[3] + SIZE * 0.5, true, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        sampler.sample(point, false, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        assert!(touch.take_requests().is_empty());
        sampler.sample(point, true, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        sampler.sample(point, false, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        assert_eq!(touch.take_requests(), vec![Request::Next]);
        assert!(touch.take_requests().is_empty());
        sampler.sample(point, true, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(false));
        sampler.sample(point, true, 0.1);
        touch.pointer(&sampler.frame(), (1280, 720), eligibility(false));
        sampler.sample(point, false, 0.1);
        touch.pointer(&sampler.frame(), (1280, 720), eligibility(false));
        assert!(touch.take_requests().is_empty());
        // Event mode leaves chapters enabled but disables the Log row.
        let mut sampler = PointerSampler::default();
        let disabled = POSITIONS[2] + SIZE * 0.5;
        sampler.sample(disabled, true, 0.1);
        assert!(
            touch
                .pointer(&sampler.frame(), (1920, 1080), eligibility(true))
                .1
        );
        sampler.sample(disabled, false, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        assert!(touch.take_requests().is_empty());
    }
    #[test]
    fn event_tap_long_press_and_toolbar_flick_share_one_capture() {
        let mut touch = StoryTouch::new();
        let mut sampler = PointerSampler::default();
        let point = vec2(500.0, 500.0);
        sampler.sample(point, true, 0.0);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        sampler.sample(point, false, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        assert_eq!(touch.take_requests(), vec![Request::EventHint]);
        sampler.sample(point, true, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        sampler.sample(point, true, 1.151);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        assert_eq!(touch.take_requests(), vec![Request::EventSkip]);
        sampler.sample(point, false, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        assert!(touch.take_requests().is_empty());
        // A fast left drag cancels a potential event tap/hold and opens navi.
        sampler.sample(point, true, 0.1);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        sampler.sample(point - vec2(100.0, 0.0), true, 0.01);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        sampler.sample(point, false, 0.01);
        touch.pointer(&sampler.frame(), (1920, 1080), eligibility(true));
        assert_eq!(touch.take_requests(), vec![Request::Sound("page")]);
        assert!(touch.contact.open);
    }
    #[test]
    fn backlog_waits_for_flash_and_rechecks_eligibility() {
        for cancel in [false, true] {
            let mut touch = StoryTouch::new();
            open(&mut touch);
            touch.contact.capture = 3;
            touch.contact.release(Vec2::ZERO);
            assert_eq!(touch.take_requests(), vec![Request::Sound("decide")]);
            assert!(touch.blocks_input());
            if cancel {
                touch.configure(eligibility(true));
            }
            touch.update(Ticks::from_f32(4.0));
            assert!(touch.take_requests().is_empty());
            touch.update(Ticks::from_f32(4.0));
            assert!(touch.take_requests().is_empty());
            touch.update(Ticks::ZERO);
            assert_eq!(
                touch.take_requests(),
                if cancel {
                    vec![]
                } else {
                    vec![Request::Backlog]
                }
            );
            assert!(!touch.blocks_input());
        }
    }
    #[test]
    #[ignore = "requires supplied original/1.03 executables; CPU only"]
    fn reference_native_toolbar_geometry_hit_gesture_and_chapter_callbacks() {
        use std::{
            io::Write,
            path::Path,
            process::{Command, Stdio},
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let out = Path::new("/private/tmp").join(format!("shin-toolbar-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let helper = out.join("native");
        assert!(
            Command::new("clang")
                .args(["-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
                .arg(root.join("reverse-engineering/native-adv-toolbar.c"))
                .arg("-o")
                .arg(&helper)
                .status()
                .unwrap()
                .success()
        );
        let mut queries = Vec::new();
        for (open, slide) in [(false, 0.0), (true, 0.5), (true, 1.0)] {
            for mask in [0, 31, 15, 23, 27, 29, 30] {
                for origin in POSITIONS {
                    for offset in [
                        vec2(-0.01, 0.0),
                        Vec2::ZERO,
                        vec2(191.99, 119.99),
                        vec2(192.0, 0.0),
                        vec2(0.0, 120.0),
                        SIZE * 0.5,
                    ] {
                        queries.push((0, open, slide, mask, true, origin + offset));
                    }
                }
            }
        }
        for (open, slide) in [(false, 0.0), (true, 0.5), (true, 1.0)] {
            for sys in [false, true] {
                for degrees in [
                    -180.0_f32, -151.0, -150.0, -149.0, -90.0, -31.0, -30.0, -29.0, 0.0, 29.0,
                    30.0, 31.0, 59.0, 60.0, 61.0, 90.0, 119.0, 120.0, 121.0, 149.0, 150.0, 151.0,
                    180.0,
                ] {
                    for speed in [20.0, 30.0] {
                        let rad = degrees.to_radians();
                        queries.push((1, open, slide, 31, sys, vec2(rad.cos(), rad.sin()) * speed));
                    }
                }
            }
        }
        for index in [0, 1, 3, 4] {
            for mask in [31, 31 ^ (1 << index)] {
                queries.push((2, true, 1.0, mask, true, POSITIONS[index] + SIZE * 0.5));
            }
        }
        let input = queries
            .iter()
            .map(|(mode, open, slide, mask, sys, p)| {
                format!(
                    "{mode} {} {slide:.9} {mask} {} {:.9} {:.9}\n",
                    u8::from(*open),
                    u8::from(*sys),
                    p.x,
                    p.y
                )
            })
            .collect::<String>();
        std::fs::write(out.join("input.txt"), &input).unwrap();
        for filename in ["uncompressed_executable", "uncompressed_executable_update"] {
            let mut child = Command::new(&helper)
                .arg(root.join("game-data/umineko").join(filename))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let native = child.wait_with_output().unwrap();
            assert!(
                native.status.success(),
                "{}",
                String::from_utf8_lossy(&native.stderr)
            );
            std::fs::write(out.join(format!("{filename}.txt")), &native.stdout).unwrap();
            let text = String::from_utf8(native.stdout).unwrap();
            let rows: Vec<_> = text.lines().collect();
            assert_eq!(rows.len(), queries.len() + 5);
            for (index, row) in rows[..5].iter().enumerate() {
                assert_eq!(
                    *row,
                    format!("G {index} {} {}", POSITIONS[index].x, POSITIONS[index].y)
                );
            }
            for (case, (row, (mode, open, slide, mask, sys, p))) in
                rows[5..].iter().zip(&queries).enumerate()
            {
                let mut contact = Contact::new();
                contact.open = *open;
                contact.slide = Tweener::new(*slide);
                contact.enabled = std::array::from_fn(|i| mask & (1 << i) != 0);
                contact.eligibility = eligibility(false);
                contact.eligibility.system = *sys;
                let mut result = -2;
                match mode {
                    0 => result = contact.hit(*p),
                    1 => contact.gesture(*p),
                    2 => {
                        contact.probe(*p);
                        if contact.capture > 0 {
                            contact.release(*p);
                        }
                    }
                    _ => unreachable!(),
                }
                let mut destination = 0;
                let mut sounds = 0;
                let mut modes = 0;
                for request in &contact.requests {
                    match request {
                        Request::Menu => destination = 1,
                        Request::Hide => destination = 2,
                        Request::Previous => destination = 3,
                        Request::Next => destination = 4,
                        Request::Sound(_) => sounds += 1,
                        Request::Auto => {
                            modes = 1;
                            sounds += 1;
                        }
                        Request::Skip => {
                            modes = 2;
                            sounds += 1;
                        }
                        _ => panic!("Unexpected {request:?}"),
                    }
                }
                let values: Vec<_> = row.split_whitespace().skip(1).collect();
                assert_eq!(
                    values[..5]
                        .iter()
                        .map(|v| v.parse::<i32>().unwrap())
                        .collect::<Vec<_>>(),
                    vec![result, destination, sounds, modes, i32::from(contact.open)],
                    "{filename} case{case} {p:?}"
                );
                assert!((values[5].parse::<f32>().unwrap() - contact.slide.value()).abs() < 1e-6);
            }
            println!("{filename}: {} native toolbar cases passed", queries.len());
        }
        println!("Evidence: {}", out.display());
    }
}
