//! Native ConfigMenu controller5ccc0 and asynchronous close/reset lifetime.
//! The screen publishes `settings` to the VM after each dispatched input frame.
use super::config_widgets::{CONFIG_ROWS, ConfigEdit, ConfigWidget};
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
mod tests {
    use super::*;
    fn active() -> ConfigModel {
        let mut m = ConfigModel::new(Settings::default());
        m.update(10.0);
        m
    }
    #[test]
    fn tab_and_row_priority_memory_and_native_four_tick_input_lock() {
        let mut m = active();
        m.controller_input(ConfigInput {
            previous_tab: true,
            next_tab: true,
            down: true,
            close: true,
            ..Default::default()
        });
        assert_eq!(m.selected_tab(), 2);
        assert_eq!(m.phase(), ConfigPhase::Active);
        m.controller_input(ConfigInput {
            down: true,
            reset: true,
            close: true,
            ..Default::default()
        });
        assert_eq!(m.selected_row(), 1);
        assert!(!m.accepts_controller_input());
        m.controller_input(ConfigInput {
            close: true,
            ..Default::default()
        });
        assert_eq!(m.phase(), ConfigPhase::Active);
        m.update(3.9);
        assert!(!m.accepts_controller_input());
        m.update(0.1);
        assert!(m.accepts_controller_input());
        m.controller_input(ConfigInput {
            previous_tab: true,
            ..Default::default()
        });
        assert_eq!(m.selected_tab(), 1);
        assert_eq!(m.selected_row(), 0);
        m.controller_input(ConfigInput {
            next_tab: true,
            ..Default::default()
        });
        assert_eq!(m.selected_tab(), 2);
        assert_eq!(m.selected_row(), 1);
        assert!(m.tabs()[2].widgets[1].focused());
        assert!(!m.tabs()[1].widgets[0].focused());
        m.controller_input(ConfigInput {
            up: true,
            down: true,
            left: true,
            ..Default::default()
        });
        assert_eq!(m.selected_row(), 0);
        assert_eq!(m.settings.v10_skipspeed, 50);
        assert_eq!(m.take_sounds(), ["move", "move", "move", "move", "move"]);
    }
    #[test]
    fn unchanged_close_avoids_save_changed_close_waits_for_explicit_completion() {
        let mut m = active();
        m.controller_input(ConfigInput {
            close: true,
            ..Default::default()
        });
        m.update(7.0);
        assert_eq!(m.phase(), ConfigPhase::CloseFlash);
        m.update(0.0);
        assert_eq!(m.take_responses(), [0]);
        assert!(m.take_requests().is_empty());
        m.update(10.0);
        assert_eq!(m.take_responses(), [1]);
        assert_eq!(m.phase(), ConfigPhase::Closed);
        m.update(100.0);
        assert!(m.take_responses().is_empty());
        let mut m = active();
        m.controller_input(ConfigInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(m.settings.v3_sysvol, 60);
        m.controller_input(ConfigInput {
            close: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        assert_eq!(m.phase(), ConfigPhase::WaitingSave);
        assert_eq!(m.take_requests(), [ConfigRequest::Save]);
        assert!(m.take_responses().is_empty());
        m.update(1000.0);
        assert_eq!(m.progress(), 1.0);
        assert!(m.take_requests().is_empty());
        m.save_finished();
        m.save_finished();
        assert_eq!(m.take_responses(), [0]);
        m.update(10.0);
        assert_eq!(m.take_responses(), [1]);
        // Edits that return to opening values do not create a save task.
        let mut m = active();
        m.controller_input(ConfigInput {
            right: true,
            ..Default::default()
        });
        m.controller_input(ConfigInput {
            left: true,
            ..Default::default()
        });
        m.controller_input(ConfigInput {
            close: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        assert!(m.take_requests().is_empty());
        assert_eq!(m.phase(), ConfigPhase::Closing);
    }
    #[test]
    fn reset_confirm_cancel_and_notice_preserve_live_setting_semantics() {
        let mut m = active();
        m.settings.v3_sysvol = 90;
        m.controller_input(ConfigInput {
            reset: true,
            close: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        assert_eq!(m.take_requests(), [ConfigRequest::ResetConfirmation]);
        assert_eq!(m.phase(), ConfigPhase::ResetDialog);
        m.reset_response(0, 1);
        assert_eq!(m.settings.v3_sysvol, 90);
        m.controller_input(ConfigInput {
            reset: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        m.reset_response(0, 0);
        assert_eq!(m.settings.v3_sysvol, 55);
        assert_eq!(m.take_edits()[0].touch_effects_enabled, Some(true));
        m.change_tab(2);
        m.select_row(4, false);
        m.controller_input(ConfigInput {
            right: true,
            ..Default::default()
        });
        assert!(m.settings.v18_showsongtitle);
        assert_eq!(m.phase(), ConfigPhase::NoticeDialog);
        assert!(m.take_requests().contains(&ConfigRequest::AutoSleepNotice));
        m.controller_input(ConfigInput {
            left: true,
            ..Default::default()
        });
        assert!(m.settings.v18_showsongtitle);
        m.notice_finished();
        m.controller_input(ConfigInput {
            left: true,
            ..Default::default()
        });
        assert!(!m.settings.v18_showsongtitle);
    }
    fn pointer_frame(
        m: &mut ConfigModel,
        area: &mut InputArea,
        number: u32,
        time: f64,
        events: &[super::super::input_area::PointerEvent],
        input: ConfigInput,
    ) {
        m.input_frame(
            area,
            &PointerFrame {
                number,
                time_seconds: time,
                events,
            },
            input,
        );
    }
    #[test]
    fn physical_pointer_tabs_captured_toggle_and_controller_priority() {
        use super::super::input_area::{PointerEvent, PointerKind};
        use glam::ivec2;
        let mut m = active();
        let mut area = InputArea::new(1920, 1080);
        // Selected tab is not a tab hit. A fast tap on the next tab releases
        // its original target even though the Up coordinate is over Close.
        assert!(!m.probe(Vec2::new(600.0, 180.0)));
        pointer_frame(
            &mut m,
            &mut area,
            1,
            0.0,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: ivec2(900, 180),
            }],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert_eq!(m.selected_tab(), 0);
        pointer_frame(
            &mut m,
            &mut area,
            2,
            0.1,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 0,
                position: ivec2(1800, 40),
            }],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert_eq!(m.selected_tab(), 1);
        assert_eq!(m.phase(), ConfigPhase::Active);
        assert_eq!(m.take_sounds(), ["decide"]);
        m.change_tab(0);
        pointer_frame(
            &mut m,
            &mut area,
            3,
            0.2,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: ivec2(1550, 700),
            }],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert_eq!(m.selected_row(), 0);
        assert!(!m.settings.v4_voicefocus);
        pointer_frame(
            &mut m,
            &mut area,
            4,
            0.4,
            &[],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert_eq!(m.selected_row(), 4);
        assert!(!m.settings.v4_voicefocus);
        m.update(2.0);
        assert!(m.tabs()[0].widgets[4].flash(1) > 0.0);
        pointer_frame(
            &mut m,
            &mut area,
            5,
            0.5,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 0,
                position: ivec2(0, 0),
            }],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert!(m.settings.v4_voicefocus);
        assert_eq!(m.take_sounds(), ["decide"]);
        assert_eq!(m.phase(), ConfigPhase::Active);
        m.update(4.0);
        pointer_frame(
            &mut m,
            &mut area,
            6,
            0.6,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: ivec2(1300, 700),
            }],
            ConfigInput::default(),
        );
        pointer_frame(&mut m, &mut area, 7, 0.8, &[], ConfigInput::default());
        area.disable(&mut m);
        assert!(m.settings.v4_voicefocus);
        assert!(m.take_sounds().is_empty());
    }
    #[test]
    fn resized_pointer_slider_drag_vertical_fallback_and_inertia_stop() {
        use super::super::input_area::{PointerEvent, PointerKind};
        use glam::ivec2;
        let mut m = active();
        let mut area = InputArea::new(960, 540);
        pointer_frame(
            &mut m,
            &mut area,
            1,
            0.0,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: ivec2(733, 140),
            }],
            ConfigInput::default(),
        );
        pointer_frame(
            &mut m,
            &mut area,
            2,
            0.1,
            &[PointerEvent {
                kind: PointerKind::Move,
                id: 0,
                position: ivec2(748, 140),
            }],
            ConfigInput {
                close: true,
                ..Default::default()
            },
        );
        assert_eq!(m.settings.v3_sysvol, 60);
        assert!(m.take_sounds().is_empty());
        pointer_frame(
            &mut m,
            &mut area,
            3,
            0.2,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 0,
                position: ivec2(900, 20),
            }],
            ConfigInput::default(),
        );
        assert_eq!(m.phase(), ConfigPhase::Active);
        assert_eq!(m.pointer_velocity(), 0.0);
        pointer_frame(
            &mut m,
            &mut area,
            4,
            0.3,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 0,
                position: ivec2(748, 140),
            }],
            ConfigInput::default(),
        );
        pointer_frame(
            &mut m,
            &mut area,
            5,
            0.4,
            &[PointerEvent {
                kind: PointerKind::Move,
                id: 0,
                position: ivec2(748, 160),
            }],
            ConfigInput::default(),
        );
        assert_eq!(m.settings.v3_sysvol, 60);
        pointer_frame(
            &mut m,
            &mut area,
            6,
            0.41,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 0,
                position: ivec2(0, 0),
            }],
            ConfigInput::default(),
        );
        assert!(m.pointer_velocity() > 0.0);
        assert_eq!(m.scroll(), 0.0);
        m.update(1.0);
        assert_eq!(m.pointer_velocity(), 0.0);
    }
    #[test]
    fn chrome_drag_cancels_and_gap_contact_consumes_without_editing() {
        let mut m = active();
        assert!(m.probe(Vec2::new(1800.0, 40.0)));
        m.press(Vec2::new(1800.0, 40.0));
        m.update(2.0);
        assert!(m.close_flash() > 0.0);
        m.drag(Vec2::new(1800.0, 40.0), Vec2::new(1700.0, 200.0));
        m.release(Vec2::ZERO);
        assert_eq!(m.phase(), ConfigPhase::Active);
        m.reset();
        assert!(m.probe(Vec2::new(1500.0, 40.0)));
        m.release(Vec2::ZERO);
        assert_eq!(m.phase(), ConfigPhase::ResetFlash);
        let mut m = active();
        assert!(m.probe(Vec2::new(1500.0, 346.0)));
        m.press(Vec2::ZERO);
        m.release(Vec2::ZERO);
        assert!(m.take_sounds().is_empty());
        assert_eq!(m.selected_row(), 0);
        assert!(!m.probe(Vec2::new(1784.0, 400.0)));
        assert!(!m.probe(Vec2::new(130.0, 400.0)));
    }
    #[test]
    #[ignore = "requires supplied executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_config_pointer_capture_slider_cancel_and_angle_gate() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x5d56c, 0xf9000109),
            (0x5d584, 0x1e202020),
            (0x5d598, 0x540002e5),
            (0x5d5c0, 0xb9042a68),
            (0x5d5ec, 0xb9042a68),
            (0x5d618, 0x13000109),
            (0x5d620, 0xb9042a69),
            (0x5d624, 0x540000e1),
            (0x5d634, 0x97ffff3b),
            (0x5d638, 0xb9041e60),
            (0x5d640, 0x320003e0),
            (0x5d664, 0x2a1f03e0),
            (0x5d678, 0x54fffe6b),
            (0x5d688, 0x54fffde5),
            (0x5d698, 0x54fffd6b),
            (0x5d700, 0x1e20a520),
            (0x5d704, 0x54000424),
            (0x5d728, 0xb904227f),
            (0x5d740, 0x54000064),
            (0x5d744, 0xf902127f),
            (0x5d768, 0x1e202900),
            (0x5d778, 0xf9401d08),
            (0x5d77c, 0x4ea91d21),
            (0x5d78c, 0xb9042268),
            (0x5d7c4, 0xb9042814),
            (0x5d7c8, 0xb9041c14),
            (0x5d7f4, 0xf9402108),
            (0x5d7fc, 0xb9042274),
            (0x5d888, 0x37f800e8),
            (0x5d8b4, 0x91058100),
            (0x5d8d4, 0x321a03e1),
            (0x5d908, 0x320003e2),
            (0x5d914, 0x97fffe2b),
            (0x5d95c, 0xf9402501),
            (0x5d99c, 0x7100091f),
            (0x5d9d0, 0x17fffc14),
            (0x5da1c, 0x940177f9),
            (0x5da48, 0x17fffd5a),
            (0x5da70, 0x17fffb4c),
            (0x5dae0, 0xf9402901),
            (0x5db10, 0xd65f03c0),
            (0x5db54, 0x37f80288),
            (0x5db9c, 0xb9042a7f),
            (0x5dbd0, 0x9401778c),
            (0x5dc44, 0xf9402d08),
            (0x5dc4c, 0x36000100),
            (0x5dc58, 0x320003e2),
            (0x5dc88, 0xf9402108),
            (0x5dc90, 0xb904227f),
            (0x5dd94, 0x4ea81d00),
            (0x5dd9c, 0x94017551),
            (0x5ddd4, 0x36f800a8),
            (0x5dddc, 0x36f80068),
            (0x5dde4, 0x34000048),
            (0x5ddec, 0xbd042401),
            (0x5de90, 0xb9042a7f),
            (0x5ded0, 0x940176cc),
            (0x5df08, 0xf9402108),
            (0x5df10, 0xb904227f),
            (0x570f0, 0xb9007408),
            (0x57160, 0xb9807408),
            (0x57184, 0xd0000808),
            (0x572b8, 0xb9007668),
            (0x57328, 0xb9007668),
            (0x57334, 0x2a1f03e0),
            (0x58160, 0x3901801f),
            (0x58170, 0xd65f03c0),
            (0x58180, 0xbd401002),
            (0x581ec, 0xd65f03c0),
            (0x581f0, 0xd65f03c0),
            (0x5821c, 0x34000388),
            (0x58240, 0x1e210800),
            (0x58244, 0x1e38000a),
            (0x58284, 0x320003e0),
            (0x582e8, 0x540002a8),
            (0x582fc, 0x94031225),
            (0x58328, 0x54000164),
            (0x58338, 0x540000ec),
            (0x58360, 0x39018269),
            (0x58368, 0x290cfe68),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159694, 32.0_f32),
            (0x159658, 116.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x159738, 1446.0),
            (0x1596e8, 1752.0),
            (0x159a20, 150.0),
            (0x159944, 992.0),
            (0x159340, 136.0),
            (0x1595dc, 1784.0),
            (0x159920, 252.0),
            (0x1594c8, 918.0),
            (0x159490, 510.0),
            (0x159a7c, 144.0),
            (0x159730, 300.0),
            (0x15941c, 67.5),
            (0x159734, -252.0),
            (0x159890, -136.0),
            (0x15997c, 40.0),
            (0x159834, 140.0),
            (0x1594bc, 3.141592),
            (0x159814, 180.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        let mut model = active();
        assert!(model.probe(Vec2::new(1764.0, 32.0)));
        model.release(Vec2::ZERO);
        assert_eq!(model.phase(), ConfigPhase::CloseFlash);
        let mut model = active();
        assert!(!model.probe(Vec2::new(500.0, 230.0)));
        assert!(model.probe(Vec2::new(1783.0, 917.0)));
        assert!(matches!(model.pointer, PointerTarget::Content));
        assert!(!model.probe(Vec2::new(1784.0, 917.0)));
        assert!(model.probe(Vec2::new(900.0, 150.0)));
        assert!(matches!(model.pointer, PointerTarget::Tab(1)));
        assert!(!model.probe(Vec2::new(900.0, 211.5)));
    }
    #[test]
    #[ignore = "requires supplied executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_config_controller_tab_memory_and_save_lifetime() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x5ccf4, 0x37000320),
            (0x5cd04, 0x370002a0),
            (0x5cd18, 0x37500369),
            (0x5cd1c, 0x365803e9),
            (0x5cd84, 0x7100050a),
            (0x5cd8c, 0x1a8ab175),
            (0x5cd50, 0x14000002),
            (0x5cda0, 0x37c001cb),
            (0x5cda4, 0x36c8022b),
            (0x5cdcc, 0x6b0a02bf),
            (0x5cddc, 0x0aab7d75),
            (0x5cde8, 0x37380be9),
            (0x5cdec, 0x37e80ce9),
            (0x5ce04, 0xf9401908),
            (0x5ce10, 0x37000900),
            (0x5cf58, 0xb904267f),
            (0x5cf80, 0x97fffe08),
            (0x5cfa0, 0x97fffea0),
            (0x5d014, 0xb980fa69),
            (0x5d01c, 0xb900f928),
            (0x5d028, 0xb9441a68),
            (0x5d02c, 0xb9010128),
            (0x5d04c, 0x2a1f03e1),
            (0x5d060, 0xb904267f),
            (0x5d084, 0xbd40f900),
            (0x5d088, 0x5e21d800),
            (0x5d090, 0x94017894),
            (0x5d170, 0xb9820109),
            (0x5d174, 0xb9041a69),
            (0x5d188, 0x2a1f03e1),
            (0x5d1dc, 0x6b01011f),
            (0x5d208, 0x2a0103f5),
            (0x5d234, 0x320003e1),
            (0x5d2ec, 0x1e221001),
            (0x5d300, 0x940179c0),
            (0x606b0, 0x1e210800),
            (0x606b8, 0x1e212800),
            (0x606c0, 0x1e212000),
            (0x606d8, 0xb900ba88),
            (0x60cd0, 0xf81e0ff3),
            (0x60cec, 0x1e210800),
            (0x60cfc, 0x320003e0),
            (0x60d0c, 0xb81fc3a0),
            (0x60d24, 0xd63f0100),
            (0x60d30, 0x3901e268),
            (0x609e8, 0x94016c02),
            (0x609ec, 0x360000c0),
            (0x60a0c, 0x9114d261),
            (0x60a14, 0x9401c28b),
            (0x60a18, 0x36000080),
            (0x60a20, 0x97fff058),
            (0x60a4c, 0x320003e2),
            (0x60a50, 0x9401d248),
            (0x60bf4, 0x17ffefe3),
            (0x5cb94, 0xaa0003f3),
            (0x5cbb0, 0xd63f0100),
            (0x6026c, 0x94016de1),
            (0x60270, 0x36000060),
            (0x60560, 0xb9400028),
            (0x60568, 0x2a080128),
            (0x6056c, 0x34000048),
            (0x6058c, 0x9401c39d),
            (0x605ac, 0x97ffe261),
            (0xd14b4, 0xeb0b015f),
            (0xd14cc, 0x3940016c),
            (0xd14d4, 0x6b0d019f),
            (0xd14d8, 0x54000101),
            (0xd14ec, 0x320003f3),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159570, 0.1_f32),
            (0x15999c, -0.1),
            (0x1598cc, 666.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        let settings = Settings::default();
        let mut changed = settings.clone();
        assert!(settings.serialized_eq(&changed));
        changed.v19 ^= 1;
        assert!(!settings.serialized_eq(&changed));
        changed = settings.clone();
        changed.v8 = 1;
        assert!(!settings.serialized_eq(&changed));
        changed = settings.clone();
        changed.v18_showsongtitle = true;
        assert!(!settings.serialized_eq(&changed));
        // Reset changes hidden voice settings too, so close still requires SaveGame.
        let mut initial = settings.clone();
        initial.v19 = 0;
        let mut m = ConfigModel::new(initial);
        m.update(10.0);
        m.controller_input(ConfigInput {
            reset: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        m.reset_response(0, 0);
        m.take_requests();
        assert_eq!(m.settings.v19, u32::MAX);
        m.controller_input(ConfigInput {
            close: true,
            ..Default::default()
        });
        m.update(7.0);
        m.update(0.0);
        assert_eq!(m.take_requests(), [ConfigRequest::Save]);
    }
}
