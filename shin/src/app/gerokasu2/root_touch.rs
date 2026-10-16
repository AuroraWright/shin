//! ADV contact callbacks `156D0..168B0` (probe, reset, press, release, long
//! press, drag, gesture, cancel) on the shared InputArea, the navi toolbar
//! actions and HiddenWindow's tap (`1BC90`).
use super::*;
use crate::adv::{
    character_view::CharacterTextures,
    gerokasu2::root_ui::{self, Availability, Indicators, RootUi},
    input_area::{InputArea, InputAreaTarget, TapInput, desktop_gesture_vector},
};

/// App-owned openings requested by contacts; dispatched with the menu gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// `12F2C` (right swipe with the toolbar hidden).
    SystemMenu,
    /// `1346C` (down swipe).
    HideWindow,
    /// `13010` (toolbar button 2, after its flash).
    Backlog,
}
pub(super) struct Touch {
    area: InputArea,
    /// `+950`: -1 none, 0 swallowed, >= 1 button ID + 1.
    selected: i32,
    /// `+954`: the interactive overlay accepted the probe.
    overlay: bool,
    /// The current contact started on the toolbar; overlays ignore it.
    pub toolbar_contact: bool,
    /// `19178`: the backlog opens once button 2's flash finishes.
    pending_backlog: bool,
    hidden: TapInput,
    hidden_pointer: PointerSampler,
}
fn contact_area() -> InputArea {
    // The event hint's touch row reads "long tap to skip" (16060).
    let mut area = InputArea::canvas();
    area.set_long_press_enabled(true);
    area
}
impl Default for Touch {
    fn default() -> Self {
        Self {
            area: contact_area(),
            selected: -1,
            overlay: false,
            toolbar_contact: false,
            pending_backlog: false,
            hidden: TapInput::default(),
            hidden_pointer: PointerSampler::default(),
        }
    }
}
#[derive(Clone, Copy)]
struct Context {
    /// The ADV gate (`15130` unblocked).
    gate: bool,
    /// ADV `+4D0`.
    inhibited: bool,
    system_controls: bool,
    event: i32,
    /// `12CD0`/`13210`.
    menu_allowed: bool,
    overlay_available: bool,
}
enum Action {
    Cue(&'static str),
    Button(usize),
    Message,
    EventSkip,
    Request(Request),
}
struct Contact<'a> {
    ui: &'a mut RootUi,
    touch: &'a mut Touch,
    context: Context,
    actions: &'a mut Vec<Action>,
}
impl Contact<'_> {
    fn button(&self) -> Option<usize> {
        let id = usize::try_from(self.touch.selected - 1).ok()?;
        (self.ui.navi_ready() && self.ui.buttons[id].enabled).then_some(id)
    }
    /// `15130`: a failed gate retires the overlay and swallows the contact.
    fn blocked(&mut self) -> bool {
        if self.context.gate {
            return false;
        }
        self.touch.overlay = false;
        self.touch.selected = 0;
        true
    }
}
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        self.touch.toolbar_contact = false;
        if !p.is_finite() || p.min_element() < 0. || p.x >= 1920. || p.y >= 1080. {
            return false;
        }
        self.touch.overlay = false;
        self.touch.selected = self.ui.hit(p);
        self.touch.toolbar_contact = self.touch.selected >= 0;
        if self.touch.selected < 0 && self.context.gate {
            self.touch.overlay = self.context.overlay_available;
        }
        true
    }
    fn reset(&mut self) {
        self.touch.selected = -1;
        self.touch.overlay = false;
    }
    fn press(&mut self, _: Vec2) {
        if let Some(id) = self.button() {
            self.ui.press(id);
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.touch.selected >= 1 {
            if let Some(id) = self.button() {
                self.ui.release(id);
                self.actions.push(Action::Button(id));
            }
            self.touch.selected = 0;
            return;
        }
        if self.touch.selected == 0 || self.blocked() {
            return;
        }
        let c = self.context;
        if c.system_controls && c.event >= 0 && !self.ui.hint.busy() {
            self.ui.show_hint(true);
        } else if !self.touch.overlay {
            // Overlay releases stay with the selection/card pointer owners.
            self.actions.push(Action::Message);
        }
    }
    /// `16060`: during an event a long press skips it, like B (`12C4C`).
    fn long_press(&mut self, _: Vec2) {
        if self.touch.selected >= 0 {
            self.drag(Vec2::ZERO, Vec2::ZERO);
            return;
        }
        if self.blocked() {
            return;
        }
        if self.context.system_controls && self.context.event >= 0 {
            self.actions.push(Action::EventSkip);
            self.touch.selected = 0;
        }
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {
        if self.touch.selected >= 1 {
            self.ui.cancel((self.touch.selected - 1) as usize);
            self.touch.selected = 0;
        } else if self.touch.selected < 0 {
            self.blocked();
        }
    }
    fn gesture(&mut self, v: Vec2) {
        if self.touch.selected >= 0 || v.length() <= 20. {
            return;
        }
        let c = self.context;
        let angle = v.y.atan2(v.x) / f32::from_bits(0x4049_0fd8) * 180.;
        if angle > -30. && angle < 30. {
            if self.ui.navi_ready() {
                self.actions.push(Action::Cue("page"));
                self.ui.hide_navi();
            } else if c.gate && c.system_controls && c.menu_allowed {
                self.actions.push(Action::Request(Request::SystemMenu));
            }
        } else if angle > 150. || angle < -150. {
            if !c.system_controls {
                return;
            }
            self.actions.push(Action::Cue("page"));
            if !c.inhibited && !self.ui.navi_shown {
                self.ui.show_navi();
            }
        } else if angle > 60. && angle < 120. && c.gate && c.system_controls && c.menu_allowed {
            self.actions.push(Action::Request(Request::HideWindow));
        }
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        self.gesture(desktop_gesture_vector(displacement, velocity));
    }
    fn cancel(&mut self) {
        if self.touch.selected >= 1 {
            if let Some(id) = self.button() {
                self.ui.cancel(id);
            }
            self.touch.selected = 0;
        }
    }
}
impl GerHost {
    pub(super) async fn load_root_ui(assets: &AssetServer) -> Result<Arc<CharacterTextures>> {
        let atlas: Arc<CharacterTextures> = assets.load(root_ui::ATLAS_PATH).await?;
        atlas.require_names(root_ui::ATLAS_PATH, &root_ui::TEXTURE_NAMES)?;
        Ok(atlas)
    }
    fn adv_gate(&self) -> bool {
        !self.suspended
            && !self.hidden_window
            && self.chapter.is_none()
            && self.scene.reconstruction_phase().is_none()
            && !self.scene.is_exiting()
    }
    /// `10760..10A14`, at the start of each ADV update.
    pub(super) fn update_root_availability(&mut self) {
        let playback = self.scene.playback();
        let availability = Availability {
            gate: self.adv_gate(),
            system_controls: playback.system_controls,
            modes_allowed: !self.scene.playback_blocked(),
            event: self.scene.save_session().timeline.event,
        };
        self.root.update_enabled(availability);
    }
    pub(super) fn cancel_touch(&mut self) {
        let mut actions = vec![];
        let context = self.touch_context();
        let touch = &mut self.touch;
        let mut area = std::mem::replace(&mut touch.area, contact_area());
        area.disable(&mut Contact {
            ui: &mut self.root,
            touch,
            context,
            actions: &mut actions,
        });
        self.touch.area = area;
        self.touch.toolbar_contact = false;
    }
    fn touch_context(&self) -> Context {
        let choosing = self.scene.selection().is_some();
        Context {
            gate: self.adv_gate(),
            inhibited: self.scene.reconstruction_phase().is_some() || self.scene.is_exiting(),
            system_controls: self.scene.playback().system_controls,
            event: self.scene.save_session().timeline.event,
            menu_allowed: self.scene.system_menu_allowed(),
            overlay_available: (choosing && self.scene.selection_ready())
                || self.scene.card_ready(),
        }
    }
    /// Dispatch this frame's contact through the ADV callbacks. Returns
    /// whether a contact was consumed.
    pub(super) fn update_touch(&mut self, held: bool) -> Result<bool> {
        let mut actions = vec![];
        let context = self.touch_context();
        let frame = self.pointer.frame();
        let release = self.pointer.mouse_release();
        let touch = &mut self.touch;
        let mut area = std::mem::replace(&mut touch.area, contact_area());
        let consumed = area.update_with_mouse_release(
            &mut Contact {
                ui: &mut self.root,
                touch,
                context,
                actions: &mut actions,
            },
            &frame,
            release,
        );
        self.touch.area = area;
        for action in actions {
            match action {
                Action::Cue(cue) => self.scene.play_system_cue(cue),
                Action::Request(request) => self.request = Some(request),
                Action::Message => {
                    if self.scene.selection().is_none() && !self.scene.card_ready() {
                        self.scene.playback_mut().input(false, false, true);
                        self.scene.runner_mut().interrupt_wait();
                        self.scene.click_message(false)?;
                    }
                }
                Action::Button(id) => self.toolbar(id, held),
                Action::EventSkip => self.event_skip()?,
            }
        }
        Ok(consumed)
    }
    /// `15AC0`'s jump table at `FC131`.
    fn toolbar(&mut self, id: usize, held: bool) {
        match id {
            0 | 1 => {
                let modes = &mut self.scene.playback_mut().modes;
                let on = if id == 0 {
                    modes.toggle_autoplay();
                    modes.autoplay
                } else {
                    modes.toggle_skip();
                    modes.skip
                };
                self.scene.play_system_cue(if on { "son" } else { "cancel" });
            }
            2 => {
                self.scene.play_system_cue("decide");
                self.touch.pending_backlog = true;
            }
            3 => {
                self.chapter_request(false, held);
            }
            _ => {
                self.chapter_request(true, held);
            }
        }
    }
    /// The blocking child `184D8`/`19178` waits for button 2's flash.
    pub(super) fn poll_pending_backlog(&mut self) {
        if !self.touch.pending_backlog || self.root.buttons[2].press.busy() {
            return;
        }
        self.touch.pending_backlog = false;
        if self.root.navi_shown && self.root.buttons[2].enabled {
            self.request = Some(Request::Backlog);
        }
    }
    pub(super) fn backlog_pending(&self) -> bool {
        self.touch.pending_backlog
    }
    /// HiddenWindow `1BC90`: the tap that the ripple recognizes restores it.
    pub(super) fn hidden_window_tapped(
        &mut self,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> bool {
        let touch = &mut self.touch;
        touch.hidden_pointer.sample(
            viewport.map(point).unwrap_or(Vec2::splat(-1.0)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        touch.hidden.update(&touch.hidden_pointer, (1920, 1080))
    }
    /// A held mouse that hid the window cannot restore it on release.
    pub(super) fn arm_hidden_window(&mut self) {
        self.touch.hidden.cancel();
        self.touch.hidden_pointer.cancel_contact(true);
    }
    pub(super) fn render_root_ui(
        &self,
        pass: &mut RenderPass,
        settings: &shin_core::format::save::Settings,
    ) {
        let modes = &self.scene.playback().modes;
        self.root.render(
            &self.root_atlas,
            pass,
            Indicators {
                auto: modes.autoplay,
                skip: modes.skip,
                auto_active: self.scene.autoplay_active(),
                skip_active: self.scene.skip_active(),
                disallow_skip_unread: settings.v11_disallowskipunread,
            },
        );
    }
}
