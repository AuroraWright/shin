//! Native ConfigMenu controller5ccc0 and asynchronous close/reset lifetime.
//! The screen publishes `settings` to the VM after each dispatched input frame.
pub mod dialog;
#[cfg(test)]
mod render_reference;
pub mod screen;
pub mod view;
pub mod widgets;
use super::config::widgets::{CONFIG_ROWS, ConfigEdit, ConfigWidget};
use super::input_area::{InputArea, InputAreaTarget, PointerFrame};
use glam::Vec2;
use shin_core::{
    format::save::Settings,
    time::{Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPhase {
    Opening,
    Active,
    CloseFlash,
    WaitingSave,
    ResetFlash,
    ResetDialog,
    NoticeDialog,
    Closing,
    Closed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRequest {
    Save,
    ResetConfirmation,
    AutoSleepNotice,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigInput {
    pub previous_tab: bool,
    pub next_tab: bool,
    pub up: bool,
    pub down: bool,
    pub reset: bool,
    pub close: bool,
    pub left: bool,
    pub right: bool,
    pub axis: f32,
}
#[derive(Debug)]
pub struct ConfigTab {
    pub widgets: Vec<ConfigWidget>,
    selected: usize,
    saved_scroll: i32,
    selection: Tweener,
    flash: Tweener,
}
impl ConfigTab {
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn selection(&self) -> f32 {
        self.selection.value()
    }
    pub fn flash(&self) -> f32 {
        self.flash.value()
    }
}
#[derive(Debug)]
enum PointerTarget {
    Outside,
    Chrome(i32),
    Tab(usize),
    Row(usize),
    Content,
}
#[derive(Debug)]
pub struct ConfigModel {
    pub settings: Settings,
    initial: Settings,
    tabs: [ConfigTab; 3],
    selected_tab: usize,
    phase: ConfigPhase,
    progress: f32,
    scroll: Tweener,
    close_flash: Tweener,
    reset_flash: Tweener,
    requests: Vec<ConfigRequest>,
    edits: Vec<ConfigEdit>,
    sounds: Vec<&'static str>,
    responses: Vec<i32>,
    pointer: PointerTarget,
    velocity: f32,
}
fn retarget(t: &mut Tweener, value: f32, speed: f32) {
    t.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - t.value()).abs() / speed)),
    );
}
fn flash(t: &mut Tweener) {
    retarget(t, 0.7, 0.2);
    t.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
}
impl ConfigModel {
    pub fn new(settings: Settings) -> Self {
        let mut tabs = std::array::from_fn(|i| ConfigTab {
            widgets: CONFIG_ROWS[i]
                .iter()
                .map(|&spec| ConfigWidget::new(spec, &settings))
                .collect(),
            selected: 0,
            flash: Tweener::new(0.0),
            saved_scroll: 0,
            selection: Tweener::new(if i == 0 { 1.0 } else { 0.0 }),
        });
        tabs[0].widgets[0].focus(true, false);
        Self {
            initial: settings.clone(),
            settings,
            tabs,
            selected_tab: 0,
            phase: ConfigPhase::Opening,
            progress: 0.0,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            reset_flash: Tweener::new(0.0),
            requests: Vec::new(),
            edits: Vec::new(),
            sounds: Vec::new(),
            responses: Vec::new(),
            pointer: PointerTarget::Outside,
            velocity: 0.0,
        }
    }
    pub fn phase(&self) -> ConfigPhase {
        self.phase
    }
    pub(crate) fn save_failed(&mut self) {
        if self.phase == ConfigPhase::WaitingSave {
            self.phase = ConfigPhase::Active;
        }
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn tabs(&self) -> &[ConfigTab; 3] {
        &self.tabs
    }
    pub fn selected_tab(&self) -> usize {
        self.selected_tab
    }
    pub fn selected_row(&self) -> usize {
        self.tabs[self.selected_tab].selected
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn reset_flash(&self) -> f32 {
        self.reset_flash.value()
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn accepts_controller_input(&self) -> bool {
        self.phase == ConfigPhase::Active && self.scroll.is_idle()
    }
    pub fn take_requests(&mut self) -> Vec<ConfigRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_edits(&mut self) -> Vec<ConfigEdit> {
        std::mem::take(&mut self.edits)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn take_responses(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.responses)
    }
    pub(crate) fn take_response(&mut self) -> Option<i32> {
        if self.responses.is_empty() {
            None
        } else {
            Some(self.responses.remove(0))
        }
    }
    pub fn controller_input(&mut self, input: ConfigInput) {
        if !self.accepts_controller_input() {
            return;
        }
        let next = if input.previous_tab {
            (self.selected_tab + 2) % 3
        } else if input.next_tab {
            (self.selected_tab + 1) % 3
        } else {
            self.selected_tab
        };
        if next != self.selected_tab {
            self.change_tab(next);
            self.sounds.push("move");
            return;
        }
        let old = self.selected_row();
        let count = self.tabs[self.selected_tab].widgets.len();
        let row = if input.up {
            old.saturating_sub(1)
        } else if input.down {
            (old + 1).min(count - 1)
        } else {
            old
        };
        if row != old {
            self.select_row(row, true);
            self.sounds.push("move");
            return;
        }
        if input.reset {
            flash(&mut self.reset_flash);
            self.phase = ConfigPhase::ResetFlash;
            self.sounds.push("decide");
            return;
        }
        if input.close {
            flash(&mut self.close_flash);
            self.phase = ConfigPhase::CloseFlash;
            self.sounds.push("cancel");
            return;
        }
        let row = self.selected_row();
        if let Some(edit) = self.tabs[self.selected_tab].widgets[row].controller(
            input.left,
            input.right,
            &mut self.settings,
        ) {
            self.velocity = 0.0;
            self.record_edit(edit);
            return;
        }
        // Native authored pages are padded to666 and never scroll. The nonzero
        // axis path still resets the tween after an unchanged boundary edit.
        if input.axis != 0.0 {
            self.velocity = 0.0;
            self.scroll = Tweener::new(0.0);
        }
    }
    pub fn change_tab(&mut self, tab: usize) {
        assert!(tab < 3);
        if tab == self.selected_tab {
            return;
        }
        let old = &mut self.tabs[self.selected_tab];
        old.saved_scroll = self.scroll.value() as i32;
        old.widgets[old.selected].focus(false, false);
        retarget(&mut old.selection, 0.0, 0.25);
        self.selected_tab = tab;
        self.velocity = 0.0;
        let new = &mut self.tabs[tab];
        retarget(&mut new.selection, 1.0, 0.25);
        self.scroll = Tweener::new(new.saved_scroll as f32);
        new.widgets[new.selected].focus(true, false);
    }
    pub fn select_row(&mut self, row: usize, scroll_into_view: bool) {
        let tab = &mut self.tabs[self.selected_tab];
        assert!(row < tab.widgets.len());
        if row == tab.selected {
            return;
        }
        tab.widgets[tab.selected].focus(false, true);
        tab.selected = row;
        tab.widgets[row].focus(true, true);
        if scroll_into_view {
            self.velocity = 0.0;
            self.scroll
                .enqueue_now(0.0, Tween::linear(Ticks::from_f32(4.0)));
        }
    }
    fn record_edit(&mut self, edit: ConfigEdit) {
        if let Some(cue) = edit.cue {
            self.sounds.push(cue);
        }
        if edit.show_auto_sleep_notice {
            self.phase = ConfigPhase::NoticeDialog;
            self.requests.push(ConfigRequest::AutoSleepNotice);
        }
        self.edits.push(edit);
    }
    fn start_closing(&mut self) {
        self.responses.push(0);
        self.phase = ConfigPhase::Closing;
    }
    /// Native SaveGame completion callback60bf0, after the owner finishes its task.
    pub fn save_finished(&mut self) {
        if self.phase == ConfigPhase::WaitingSave {
            self.start_closing();
        }
    }
    pub fn notice_finished(&mut self) {
        if self.phase == ConfigPhase::NoticeDialog {
            self.phase = ConfigPhase::Active;
        }
    }
    /// Native reset callback60560 applies defaults only for the zero/zero response.
    pub fn reset_response(&mut self, button: i32, cancelled: i32) {
        self.reset_choice(button, cancelled);
        self.reset_finished();
    }
    /// Apply the final0 dialog callback while its exit still owns input.
    pub fn reset_choice(&mut self, button: i32, final_flag: i32) {
        if self.phase != ConfigPhase::ResetDialog {
            return;
        }
        if button == 0 && final_flag == 0 {
            self.settings = Settings::default();
            for tab in &mut self.tabs {
                tab.widgets = tab
                    .widgets
                    .iter()
                    .map(|w| ConfigWidget::new(w.spec, &self.settings))
                    .collect();
            }
            let tab = &mut self.tabs[self.selected_tab];
            tab.widgets[tab.selected].focus(true, false);
            self.edits.push(ConfigEdit {
                cue: None,
                show_auto_sleep_notice: false,
                touch_effects_enabled: Some(self.settings.v17_showscenetitle),
            });
        }
    }
    pub fn reset_finished(&mut self) {
        if self.phase == ConfigPhase::ResetDialog {
            self.phase = ConfigPhase::Active;
        }
    }
    pub fn update(&mut self, ticks: f32) {
        if !ticks.is_finite() || ticks < 0.0 {
            return;
        }
        // Task checks precede this frame's feedback clocks, matching609d0/60250.
        match self.phase {
            ConfigPhase::Opening => {
                self.progress = (self.progress + ticks * 0.1).min(1.0);
                if self.progress == 1.0 {
                    self.phase = ConfigPhase::Active;
                }
            }
            ConfigPhase::CloseFlash if self.close_flash.is_idle() => {
                if self.settings.serialized_eq(&self.initial) {
                    self.start_closing();
                } else {
                    self.phase = ConfigPhase::WaitingSave;
                    self.requests.push(ConfigRequest::Save);
                }
            }
            ConfigPhase::ResetFlash if self.reset_flash.is_idle() => {
                self.phase = ConfigPhase::ResetDialog;
                self.requests.push(ConfigRequest::ResetConfirmation);
            }
            ConfigPhase::Closing => {
                self.progress = (self.progress - ticks * 0.1).max(0.0);
                if self.progress == 0.0 {
                    self.phase = ConfigPhase::Closed;
                    self.responses.push(1);
                }
            }
            _ => {}
        }
        // All original pages have range666 and viewport666. Any page momentum
        // reaches a boundary immediately, exactly as the native clamp/epsilon check.
        if self.velocity != 0.0 {
            self.scroll = Tweener::new(0.0);
            self.velocity = 0.0;
        }
        let delta = Ticks::from_f32(ticks);
        self.scroll.update(delta);
        self.close_flash.update(delta);
        self.reset_flash.update(delta);
        for tab in &mut self.tabs {
            tab.selection.update(delta);
            tab.flash.update(delta);
        }
        for widget in &mut self.tabs[self.selected_tab].widgets {
            widget.update(delta);
        }
    }
}
impl ConfigModel {
    pub fn input_frame(
        &mut self,
        area: &mut InputArea,
        frame: &PointerFrame<'_>,
        input: ConfigInput,
    ) {
        if self.phase != ConfigPhase::Active {
            area.disable(self);
        } else if !area.update(self, frame) {
            self.controller_input(input);
        }
    }
    fn local(&self, p: Vec2) -> Vec2 {
        p - Vec2::new(136.0, 252.0 - self.scroll.value())
    }
    fn pointer_close(&mut self) {
        flash(&mut self.close_flash);
        self.phase = ConfigPhase::CloseFlash;
        self.sounds.push("decide");
    }
    fn pointer_reset_request(&mut self) {
        flash(&mut self.reset_flash);
        self.phase = ConfigPhase::ResetFlash;
        self.sounds.push("decide");
    }
    pub fn pointer_velocity(&self) -> f32 {
        self.velocity
    }
}
impl InputAreaTarget for ConfigModel {
    fn probe(&mut self, p: Vec2) -> bool {
        if self.phase != ConfigPhase::Active {
            return false;
        }
        self.pointer = PointerTarget::Outside;
        if (32.0..116.0).contains(&p.y) {
            if (1764.0..1848.0).contains(&p.x) {
                self.pointer = PointerTarget::Chrome(1);
                return true;
            }
            if (1446.0..1752.0).contains(&p.x) {
                self.pointer = PointerTarget::Chrome(2);
                return true;
            }
        }
        if !(150.0..992.0).contains(&p.y) {
            self.pointer = PointerTarget::Chrome(0);
            return true;
        }
        for tab in 0..3 {
            if tab != self.selected_tab
                && (510.0 + tab as f32 * 300.0..810.0 + tab as f32 * 300.0).contains(&p.x)
                && (144.0..211.5).contains(&p.y)
            {
                self.pointer = PointerTarget::Tab(tab);
                return true;
            }
        }
        if !(136.0..1784.0).contains(&p.x) || !(252.0..918.0).contains(&p.y) {
            return false;
        }
        self.pointer = PointerTarget::Content;
        if self.velocity.abs() >= f32::EPSILON {
            self.velocity = 0.0;
            return true;
        }
        let local = self.local(p);
        let widgets = &mut self.tabs[self.selected_tab].widgets;
        if let Some((row, w)) = widgets
            .iter_mut()
            .enumerate()
            .find(|(_, w)| (f32::from(w.spec.y)..f32::from(w.spec.y) + 88.0).contains(&local.y))
        {
            w.probe(local.x, local.y);
            self.pointer = PointerTarget::Row(row);
        }
        true
    }
    fn reset(&mut self) {
        if let PointerTarget::Row(row) = self.pointer {
            self.tabs[self.selected_tab].widgets[row].cancel();
        }
        self.pointer = PointerTarget::Outside;
    }
    fn press(&mut self, p: Vec2) {
        match self.pointer {
            PointerTarget::Chrome(1) => retarget(&mut self.close_flash, 0.7, 0.2),
            PointerTarget::Chrome(2) => retarget(&mut self.reset_flash, 0.7, 0.2),
            PointerTarget::Tab(tab) => retarget(&mut self.tabs[tab].flash, 0.7, 0.2),
            PointerTarget::Row(row) => {
                self.select_row(row, true);
                let p = self.local(p);
                self.tabs[self.selected_tab].widgets[row].press_captured(p.x, p.y);
            }
            _ => {}
        }
    }
    fn release(&mut self, _: Vec2) {
        match self.pointer {
            PointerTarget::Chrome(1) => self.pointer_close(),
            PointerTarget::Chrome(2) => self.pointer_reset_request(),
            PointerTarget::Tab(tab) => {
                flash(&mut self.tabs[tab].flash);
                self.sounds.push("decide");
                self.change_tab(tab);
            }
            PointerTarget::Row(row) => {
                self.select_row(row, true);
                if let Some(edit) =
                    self.tabs[self.selected_tab].widgets[row].release(&mut self.settings)
                {
                    self.record_edit(edit);
                }
            }
            _ => {}
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        match self.pointer {
            PointerTarget::Chrome(_) => self.cancel(),
            PointerTarget::Tab(tab) => retarget(&mut self.tabs[tab].flash, 0.0, 0.2),
            PointerTarget::Row(row) => {
                let previous = self.local(previous);
                let current = self.local(current);
                let (handled, edit) = self.tabs[self.selected_tab].widgets[row].pointer_drag(
                    previous,
                    current,
                    &mut self.settings,
                );
                if let Some(edit) = edit {
                    self.record_edit(edit);
                }
                if handled {
                    self.select_row(row, true);
                } else {
                    self.tabs[self.selected_tab].widgets[row].cancel();
                    self.pointer = PointerTarget::Content;
                    self.scroll = Tweener::new(0.0);
                }
            }
            PointerTarget::Content => self.scroll = Tweener::new(0.0),
            PointerTarget::Outside => {}
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if matches!(self.pointer, PointerTarget::Content) && velocity.y.is_finite() {
            self.velocity = velocity.y;
        }
    }
    fn cancel(&mut self) {
        match self.pointer {
            PointerTarget::Chrome(_) => {
                retarget(&mut self.close_flash, 0.0, 0.2);
                retarget(&mut self.reset_flash, 0.0, 0.2);
                self.pointer = PointerTarget::Chrome(0);
            }
            PointerTarget::Tab(tab) => retarget(&mut self.tabs[tab].flash, 0.0, 0.2),
            PointerTarget::Row(row) => {
                self.tabs[self.selected_tab].widgets[row].cancel();
                self.pointer = PointerTarget::Content;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
