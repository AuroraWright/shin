//! Ger manual Save/Load controller (B2970/B56A8/B5BE0/B6990/B7348/B7860).
//! Full slot payloads, previews, dialogs and disk publication belong to the host.
pub mod preview;
pub const SLOT_COUNT: usize = 100;
pub const ROW_STRIDE: f32 = 201.0;
pub const ROW_HEIGHT: f32 = 197.0;
pub const VIEW_HEIGHT: f32 = 800.0;
pub const MAX_SCROLL: f32 = 19296.0;
pub const ATLAS: &str = "/saveload.txa";
pub const TEXTURES: [&str; 13] = [
    "bg",
    "button_shadow",
    "caption_load",
    "caption_save",
    "close",
    "help_saveload",
    "key",
    "nodata",
    "num",
    "top",
    "vline",
    "window_focus",
    "window_normal",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Save = 0,
    Load = 1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    Save { overwrite: bool },
    Load,
    Delete,
}
impl QuestionKind {
    pub fn message(self) -> &'static str {
        match self {
            Self::Save { overwrite: true } => {
                "セーブを行います。@r内容は上書きされますが、よろしいですか？"
            }
            Self::Save { overwrite: false } => "セーブを行います。よろしいですか？",
            Self::Load => "ロードします。よろしいですか？",
            Self::Delete => "このセーブデータを削除します。よろしいですか？",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionRequest {
    pub slot: u8,
    pub kind: QuestionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Opening,
    Active,
    RowFlash,
    Confirmation,
    Writing,
    CloseFlash,
    Closing,
    Handoff,
    Closed,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub previous_page: bool,
    pub next_page: bool,
    pub confirm: bool,
    pub delete: bool,
    pub close: bool,
    pub axis: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Load { slot: u8 },
    Close,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRequest {
    Remember(u8),
    /// Publish the frozen menu-entry record with a fresh date and remembered
    /// slot in one write. The host retains the old record/remembered byte for
    /// rollback when write_finished(false) is reported.
    Save {
        slot: u8,
    },
    /// Clear only the valid byte; retain the slot payload for failed-write
    /// rollback. Delete never changes the remembered slot.
    Delete {
        slot: u8,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Outside,
    Close,
    Content(Option<usize>),
}
#[derive(Debug, Clone, Copy)]
struct Motion {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
}
impl Motion {
    fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            elapsed: 1.0,
            duration: 1.0,
        }
    }
    fn value(&self) -> f32 {
        (self.to - self.from).mul_add((self.elapsed / self.duration).min(1.0), self.from)
    }
    fn busy(&self) -> bool {
        self.elapsed < self.duration
    }
    fn set(&mut self, value: f32) {
        *self = Self::new(value)
    }
    fn to(&mut self, value: f32, duration: f32) {
        *self = Self {
            from: self.value(),
            to: value,
            elapsed: 0.0,
            duration: duration.max(f32::MIN_POSITIVE),
        }
    }
    fn update(&mut self, delta: f32) {
        self.elapsed = (self.elapsed + delta).min(self.duration)
    }
}
/// Availability is an immutable snapshot of native valid bytes; viewing an empty
/// slot does not manufacture data, and navigation does not persist selection.
pub struct Controller {
    mode: Mode,
    available: [bool; SLOT_COUNT],
    selected: usize,
    scroll: Motion,
    phase: Phase,
    progress: f32,
    flash: Motion,
    flash_return: bool,
    close_flash: Motion,
    close_flash_return: bool,
    confirmed: Option<usize>,
    confirmation_requested: bool,
    question_kind: Option<QuestionKind>,
    write_backup: Option<bool>,
    write_done: bool,
    dialog_finished: bool,
    velocity: f32,
    sounds: Vec<&'static str>,
    storage: Vec<StorageRequest>,
}
impl Controller {
    /// Native stores a7-bit value on disk but indexes100 rows unchecked. Reject
    /// values100..127 before any host indexing instead of silently clamping.
    pub fn new(remembered: u8, available: [bool; SLOT_COUNT]) -> Result<Self, &'static str> {
        Self::new_with_mode(Mode::Load, remembered, available)
    }
    pub fn new_with_mode(
        mode: Mode,
        remembered: u8,
        available: [bool; SLOT_COUNT],
    ) -> Result<Self, &'static str> {
        if usize::from(remembered) >= SLOT_COUNT {
            return Err("Ger manual save selection exceeds100 slots");
        }
        Ok(Self {
            mode,
            available,
            selected: remembered as usize,
            scroll: Motion::new((f32::from(remembered) * 201.0 - 302.0).clamp(0.0, MAX_SCROLL)),
            phase: Phase::Loading,
            progress: 0.0,
            flash: Motion::new(0.0),
            flash_return: false,
            close_flash: Motion::new(0.0),
            close_flash_return: false,
            confirmed: None,
            confirmation_requested: false,
            question_kind: None,
            write_backup: None,
            write_done: false,
            dialog_finished: false,
            velocity: 0.0,
            sounds: vec![],
            storage: vec![],
        })
    }
    pub fn assets_ready(&mut self) {
        if self.phase == Phase::Loading {
            self.phase = Phase::Opening
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn available(&self, slot: usize) -> bool {
        self.available[slot]
    }
    pub fn row_flash(&self, slot: usize) -> f32 {
        if slot == self.selected {
            self.flash.value()
        } else {
            0.0
        }
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == Phase::Active && !self.scroll.busy()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn take_storage_requests(&mut self) -> Vec<StorageRequest> {
        std::mem::take(&mut self.storage)
    }
    pub fn take_confirmation(&mut self) -> Option<usize> {
        if std::mem::take(&mut self.confirmation_requested) {
            Some(self.selected)
        } else {
            None
        }
    }
    pub fn take_question(&mut self) -> Option<QuestionRequest> {
        if !std::mem::take(&mut self.confirmation_requested) {
            return None;
        }
        Some(QuestionRequest {
            slot: self.selected as u8,
            kind: self.question_kind.expect("pending SaveMenu question kind"),
        })
    }
    pub fn visible_slots(&self) -> impl Iterator<Item = usize> + '_ {
        (0..SLOT_COUNT).filter(|&i| {
            let y = 176.0 + i as f32 * 201.0 - self.scroll();
            y < 1080.0 && y + 197.0 >= 0.0
        })
    }
    fn reveal(&mut self) {
        let top = self.selected as f32 * 201.0;
        let current = self.scroll();
        let target = if top < current {
            top.max(0.0)
        } else if top - current + 197.0 > 800.0 {
            (top + 197.0 - 800.0).min(MAX_SCROLL)
        } else {
            current
        };
        self.scroll.to(target, 4.0)
    }
    pub fn input(&mut self, input: Input) {
        if !self.accepts_input() {
            return;
        }
        let delta = if input.up {
            -1
        } else if input.down {
            1
        } else if input.left {
            -4
        } else if input.right {
            4
        } else if input.previous_page {
            -10
        } else if input.next_page {
            10
        } else {
            0
        };
        let next = (self.selected as i32 + delta).clamp(0, 99) as usize;
        if next != self.selected {
            self.selected = next;
            self.velocity = 0.0;
            self.reveal();
            self.sounds.push("move")
        } else if input.confirm {
            self.confirm()
        } else if input.delete {
            self.delete()
        } else if input.close {
            self.close()
        } else if input.axis.is_finite() && input.axis != 0.0 {
            self.velocity = 0.0;
            self.drag_scroll(input.axis.mul_add(32.0, self.scroll()))
        }
    }
    pub fn select(&mut self, slot: usize) -> bool {
        if !self.accepts_input() || slot >= SLOT_COUNT {
            return false;
        }
        self.selected = slot;
        true
    }
    pub fn confirm(&mut self) {
        if !self.accepts_input() {
            return;
        }
        self.velocity = 0.0;
        self.flash.to(0.7, (0.7 - self.flash.value()).abs() / 0.2);
        self.flash_return = true;
        if self.mode == Mode::Save || self.available[self.selected] {
            self.question_kind = Some(match self.mode {
                Mode::Save => QuestionKind::Save {
                    overwrite: self.available[self.selected],
                },
                Mode::Load => QuestionKind::Load,
            });
            self.phase = Phase::RowFlash;
            self.sounds.push("decide")
        } else {
            self.sounds.push("error")
        }
    }
    pub fn delete(&mut self) {
        // B58F4 consumes Delete in Load mode without dispatch or Back fallback.
        if self.mode != Mode::Save || !self.accepts_input() {
            return;
        }
        self.velocity = 0.0;
        self.flash.to(0.7, (0.7 - self.flash.value()).abs() / 0.2);
        self.flash_return = true;
        if self.available[self.selected] {
            self.question_kind = Some(QuestionKind::Delete);
            self.phase = Phase::RowFlash;
            self.sounds.push("decide");
        } else {
            self.sounds.push("error");
        }
    }
    pub fn close(&mut self) {
        if !self.accepts_input() {
            return;
        }
        self.velocity = 0.0;
        self.close_flash
            .to(0.7, (0.7 - self.close_flash.value()).abs() / 0.2);
        self.close_flash_return = true;
        self.phase = Phase::CloseFlash;
        self.sounds.push("cancel")
    }
    /// Native Load's intermediate stage never publishes; only final Yes copies
    /// progress and remembers selection. No/Cancel return to the same row.
    pub fn dialog_response(&mut self, final_stage: bool, button: i32) {
        if self.phase == Phase::Writing {
            if final_stage {
                self.dialog_finished = true;
                if self.write_done {
                    self.phase = Phase::Active;
                }
            }
            return;
        }
        if self.phase != Phase::Confirmation {
            return;
        }
        if self.question_kind != Some(QuestionKind::Load) {
            if final_stage {
                self.phase = Phase::Active;
            } else if button == 0 {
                self.write_backup = Some(self.available[self.selected]);
                self.write_done = false;
                self.dialog_finished = false;
                let slot = self.selected as u8;
                match self.question_kind.expect("SaveMenu write question") {
                    QuestionKind::Save { .. } => {
                        self.available[self.selected] = true;
                        self.storage.push(StorageRequest::Save { slot });
                    }
                    QuestionKind::Delete => {
                        self.available[self.selected] = false;
                        self.storage.push(StorageRequest::Delete { slot });
                    }
                    QuestionKind::Load => unreachable!(),
                }
                self.phase = Phase::Writing;
            }
            return;
        }
        if !final_stage {
            return;
        }
        if button == 0 {
            self.confirmed = Some(self.selected);
            self.storage
                .push(StorageRequest::Remember(self.selected as u8));
            self.phase = Phase::Closing
        } else {
            self.phase = Phase::Active
        }
    }
    /// Native save completion receives true for success. On failure Save
    /// restores the whole prior record/remembered byte, Delete restores valid1;
    /// the host rolls back payloads while this model restores availability.
    /// Input remains blocked until both the write and Question child finish.
    pub fn write_finished(&mut self, success: bool) {
        if self.phase != Phase::Writing || self.write_done {
            return;
        }
        if let Some(previous) = self.write_backup.take() {
            if !success {
                self.available[self.selected] = previous;
            }
        }
        self.write_done = true;
        if self.dialog_finished {
            self.phase = Phase::Active;
        }
    }
    /// Reversible host recovery if ADV replay or resource hydration fails.
    pub fn load_failed(&mut self) {
        if self.phase == Phase::Handoff {
            self.confirmed = None;
            self.phase = Phase::Opening;
            self.progress = 0.0
        }
    }
    pub fn update(&mut self, delta: f32) -> Option<Action> {
        if !delta.is_finite() || delta < 0.0 {
            return None;
        }
        // Original task polling precedes scroll/flash updates. A completed flash
        // is observed by its task on the following parent frame.
        let action = match self.phase {
            Phase::Opening => {
                self.progress = delta.mul_add(0.1, self.progress).min(1.0);
                if self.progress >= 1.0 {
                    self.phase = Phase::Active
                }
                None
            }
            Phase::RowFlash if !self.flash.busy() && !self.flash_return => {
                self.phase = Phase::Confirmation;
                self.confirmation_requested = true;
                None
            }
            Phase::CloseFlash if !self.close_flash.busy() && !self.close_flash_return => {
                self.phase = Phase::Closing;
                None
            }
            Phase::Closing => {
                self.progress = delta.mul_add(-0.1, self.progress).max(0.0);
                if self.progress <= 0.0 {
                    if let Some(slot) = self.confirmed {
                        self.phase = Phase::Handoff;
                        Some(Action::Load { slot: slot as u8 })
                    } else {
                        self.phase = Phase::Closed;
                        Some(Action::Close)
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        if self.velocity != 0.0 {
            let old = self.scroll();
            self.drag_scroll((-self.velocity).mul_add(delta, old));
            if (old - self.scroll()).abs() < f32::from_bits(0x34000000) {
                self.velocity = 0.0
            } else {
                self.velocity *= 0.95f32.powf(delta);
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0
                }
            }
        }
        // B33EC advances the scroll by literal1.0, independently of row delta.
        self.scroll.update(1.0);
        update_flash(&mut self.flash, &mut self.flash_return, delta);
        update_flash(&mut self.close_flash, &mut self.close_flash_return, delta);
        action
    }
    pub fn pointer(&self, x: f32, y: f32) -> Pointer {
        if !x.is_finite() || !y.is_finite() {
            return Pointer::Outside;
        }
        if (1766.0..1874.0).contains(&x) && (46.0..154.0).contains(&y) {
            return Pointer::Close;
        }
        if !(160.0..992.0).contains(&y) {
            return Pointer::Outside;
        }
        if !(160.0..1760.0).contains(&x) || !(176.0..976.0).contains(&y) {
            return Pointer::Content(None);
        }
        let value = (self.scroll() - 176.0 + y) as i32;
        let index = value / 201;
        let rem = value % 201;
        if rem >= 197 || index < 0 || index >= 100 {
            Pointer::Content(None)
        } else {
            Pointer::Content(Some(index as usize))
        }
    }
    pub fn drag_scroll(&mut self, value: f32) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, MAX_SCROLL);
        while value > self.selected as f32 * 201.0 {
            self.selected += 1
        }
        while value + 800.0 < self.selected as f32 * 201.0 + 197.0 {
            self.selected = self.selected.saturating_sub(1)
        }
        self.selected = self.selected.min(99);
        self.scroll.set(value)
    }
    pub fn pointer_press(&mut self, target: Pointer) {
        if self.phase != Phase::Active {
            return;
        }
        match target {
            Pointer::Close => {
                self.close_flash
                    .to(0.7, (0.7 - self.close_flash.value()).abs() / 0.2);
                self.close_flash_return = false
            }
            Pointer::Content(Some(slot)) if slot < SLOT_COUNT => {
                self.selected = slot;
                self.flash.to(0.7, (0.7 - self.flash.value()).abs() / 0.2);
                self.flash_return = false;
                self.reveal()
            }
            _ => {}
        }
    }
    pub fn pointer_release(&mut self, target: Pointer) {
        if self.phase != Phase::Active {
            return;
        }
        // Pointer release captures the row from Down; scroll tween state does
        // not block the callback, unlike keyboard input.
        let scroll = self.scroll;
        self.scroll.set(self.scroll());
        match target {
            Pointer::Close => self.close(),
            Pointer::Content(Some(slot)) if slot < SLOT_COUNT => {
                self.selected = slot;
                self.confirm()
            }
            _ => {}
        }
        self.scroll = scroll;
    }
    pub fn pointer_cancel(&mut self) {
        self.flash.to(0.0, self.flash.value().abs() / 0.2);
        self.flash_return = false;
        self.close_flash
            .to(0.0, self.close_flash.value().abs() / 0.2);
        self.close_flash_return = false
    }
    pub fn release_scroll(&mut self, velocity: f32) {
        if self.phase == Phase::Active && velocity.is_finite() {
            self.velocity = velocity
        }
    }
}
fn update_flash(m: &mut Motion, release: &mut bool, delta: f32) {
    let remaining = (m.duration - m.elapsed).max(0.0);
    m.update(delta);
    if !m.busy() && *release {
        *release = false;
        m.to(0.0, 3.5);
        m.update((delta - remaining).max(0.0))
    }
}
#[cfg(test)]
#[path = "save_menu/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "save_menu/write_tests.rs"]
mod write_tests;
