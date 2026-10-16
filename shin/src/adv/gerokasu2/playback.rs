//! Native11950/11b00 playback gates and15090 completion read tracking.
use crate::adv::playback::PlaybackModes;
use shin_core::format::save::index_set::IndexSet;

pub struct Playback {
    pub modes: PlaybackModes,
    pub system_controls: bool,
    pub hidden_ui: bool,
    pub disallow_skip_unread: bool,
    /// Port's temporary Ctrl shortcut; separate from the native mode word.
    pub held_skip: bool,
    /// Snapshot from MSGSET's constructor; completion never changes it.
    pub was_read: bool,
    pub seen_messages: IndexSet,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            modes: Default::default(),
            system_controls: true,
            hidden_ui: false,
            disallow_skip_unread: true,
            held_skip: false,
            was_read: true,
            seen_messages: IndexSet::new(),
        }
    }
}
impl Playback {
    pub fn enter_message(&mut self, id: u32) {
        self.was_read = (id as i32) < 0 || self.seen_messages.contains(id as usize);
    }
    pub fn complete_message(&mut self, id: u32) {
        if (id as i32) >= 0 {
            self.seen_messages.add(id as usize);
        }
    }
    /// Native SYSCALL0 changes permission and clears both mode bits.
    pub fn system_call(&mut self, id: i32, argument: i32) {
        if id == 0 {
            self.system_controls = argument != 0;
            self.modes.reset();
        }
    }
    /// Native L/autoplay wins simultaneous L/R clicks. `cancel` is the
    /// 11650 mask 0x200000C0 (X bit6, Y bit7, B alias bit29; never A); it clears the mode word only when
    /// neither toggle was dispatched. Returns the native 258F0 cue when the
    /// word changed: 1167C/11714 select `son` for a bit turned on and
    /// `cancel` for one turned off; 1166C clears with `cancel`. The caller
    /// consumes that press, as 11744 returns.
    pub fn input(&mut self, auto: bool, skip: bool, cancel: bool) -> Option<&'static str> {
        if self.hidden_ui {
            return None;
        }
        if self.system_controls && auto {
            self.modes.toggle_autoplay();
            Some(if self.modes.autoplay { "son" } else { "cancel" })
        } else if self.system_controls && skip {
            self.modes.toggle_skip();
            Some(if self.modes.skip { "son" } else { "cancel" })
        } else if cancel && (self.modes.autoplay || self.modes.skip) {
            self.modes.reset();
            Some("cancel")
        } else {
            None
        }
    }
    pub fn autoplay(&self, event: i32, interactive_overlay: bool) -> bool {
        self.system_controls
            && !self.hidden_ui
            && event < 0
            && !interactive_overlay
            && self.modes.autoplay
    }
    pub fn skip(&self, event: i32, interactive_overlay: bool) -> bool {
        self.system_controls
            && !self.hidden_ui
            && event < 0
            && !interactive_overlay
            && (self.modes.skip || self.held_skip)
            && (!self.disallow_skip_unread || self.was_read)
    }
}
#[cfg(test)]
mod tests;
