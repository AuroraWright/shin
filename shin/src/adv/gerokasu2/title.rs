//! Ger Title menu/action owner recovered from DA6A0/DC098/DC198/DDE94.
//!
//! The host supplies the asset, outgoing capture, child scenes and ADV owners.
//! `assets_ready` represents the original First asset readiness boundary. The
//! optional title demo and its platform boot policy remain separate owners.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleAvailability {
    /// Active progress valid byte, global settings + C8 (not "any manual slot").
    pub continue_available: bool,
    /// Native signed16 persisted value zero != 0, global settings + 8.
    pub extra_unlocked: bool,
}

#[cfg(test)]
#[path = "title/tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TitleItem {
    NewGame = 0,
    Continue = 1,
    Load = 2,
    Config = 3,
    Extra = 4,
    Manual = 7,
    CgGallery = 8,
    MusicGallery = 9,
    BustupGallery = 10,
    Back = 11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    NewGame { init_value: i32 },
    Continue,
    Load,
    Config,
    Manual,
    MusicGallery,
    CgGallery,
    BustupGallery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMusicAction {
    Stop(u32),
    Play(u32),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TitleInput {
    pub up: bool,
    pub down: bool,
    pub confirm: bool,
    pub back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlePhase {
    Loading,
    Opening,
    Starting,
    Entering,
    Active,
    Leaving,
    Fading,
    Child,
    Handoff,
}

pub const TITLE_ATLAS_PATH: &str = "/title.txa";
pub const TITLE_TEXTURE_NAMES: [&str; 5] = ["bg", "logo", "copyright", "menu_normal", "menu_focus"];
/// Rectangles are [left, top, width, height] in the native 1920 x 1080 canvas.
pub const TITLE_BACKGROUND_RECT: [f32; 4] = [0.0, 0.0, 1920.0, 1080.0];
pub const TITLE_LOGO_RECT: [f32; 4] = [1168.0, 36.0, 752.0, 360.0];
pub const TITLE_COPYRIGHT_RECT: [f32; 4] = [1504.0, 992.0, 416.0, 88.0];
pub const TITLE_BUTTON_SHEET_SIZE: [f32; 2] = [480.0, 1280.0];

pub const fn button_source_rect(item: TitleItem) -> [f32; 4] {
    [0.0, item as u8 as f32 * 80.0, 480.0, 80.0]
}

pub fn button_rect(index: usize) -> [f32; 4] {
    [1306.0, 424.0 + index as f32 * 84.0, 480.0, 80.0]
}

/// Native +D0's speed-controlled linear child blackout. Controls queue while
/// busy, including an early final child callback before the entry fade settles.
#[derive(Debug, Clone)]
struct ChildBlackout {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    queued: std::collections::VecDeque<f32>,
}
impl ChildBlackout {
    fn new() -> Self {
        Self {
            from: 0.,
            to: 0.,
            elapsed: 1.,
            duration: 1.,
            queued: Default::default(),
        }
    }
    fn value(&self) -> f32 {
        (self.to - self.from).mul_add(self.elapsed / self.duration, self.from)
    }
    fn busy(&self) -> bool {
        self.elapsed < self.duration
    }
    fn start(&mut self, target: f32) {
        self.from = self.value();
        self.to = target;
        self.elapsed = 0.;
        self.duration = ((target - self.from) / 0.1).abs();
        if self.duration == 0. {
            self.elapsed = 1.;
            self.duration = 1.;
        }
    }
    fn control(&mut self, target: f32) {
        if self.busy() {
            self.queued.push_back(target);
        } else {
            self.start(target);
        }
    }
    fn advance(&mut self) {
        let mut left = 1.;
        loop {
            let remaining = self.duration - self.elapsed;
            if left < remaining {
                self.elapsed += left;
                break;
            }
            self.elapsed = self.duration;
            left -= remaining;
            if let Some(target) = self.queued.pop_front() {
                self.start(target);
            } else {
                break;
            }
            if left <= 0. {
                break;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TitleController {
    availability: TitleAvailability,
    rows: Vec<TitleItem>,
    selected: Option<usize>,
    extra: bool,
    phase: TitlePhase,
    elapsed: f32,
    leaving_row: Option<usize>,
    pending: Option<TitleItem>,
    child_blackout: ChildBlackout,
    returning_rows: bool,
    first_entry: bool,
    first_elapsed: f32,
    music_actions: Vec<TitleMusicAction>,
}

impl TitleController {
    pub fn new(availability: TitleAvailability, requested_item: i32) -> Self {
        let mut owner = Self {
            availability,
            rows: Vec::with_capacity(6),
            selected: None,
            extra: false,
            phase: TitlePhase::Loading,
            elapsed: 0.0,
            leaving_row: None,
            pending: None,
            child_blackout: ChildBlackout::new(),
            returning_rows: false,
            first_entry: requested_item < 0,
            first_elapsed: 0.,
            music_actions: Vec::new(),
        };
        owner.build_rows(requested_item);
        // DBA20's first-entry rule selects Continue if it exists; otherwise New.
        if requested_item < 0 {
            owner.selected = Some(usize::from(availability.continue_available));
        }
        owner
    }

    fn build_rows(&mut self, requested_item: i32) {
        use TitleItem::*;
        self.rows.clear();
        self.extra = (7..=11).contains(&requested_item);
        if self.extra {
            self.rows
                .extend([CgGallery, MusicGallery, BustupGallery, Back]);
        } else {
            self.rows.push(NewGame);
            if self.availability.continue_available {
                self.rows.push(Continue);
            }
            self.rows.extend([Load, Config]);
            if self.availability.extra_unlocked {
                self.rows.push(Extra);
            }
            self.rows.push(Manual);
        }
        self.selected = Some(
            self.rows
                .iter()
                .position(|item| *item as i32 == requested_item)
                .unwrap_or(0),
        );
    }

    pub fn rows(&self) -> &[TitleItem] {
        &self.rows
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn phase(&self) -> TitlePhase {
        self.phase
    }

    pub fn extra_page(&self) -> bool {
        self.extra
    }

    pub fn bgm_id(&self) -> i32 {
        if self.availability.extra_unlocked {
            10
        } else {
            11
        }
    }

    pub fn assets_ready(&mut self) {
        if self.phase == TitlePhase::Loading {
            self.transition(TitlePhase::Opening);
            self.music_actions.push(if self.first_entry {
                TitleMusicAction::Stop(60)
            } else {
                TitleMusicAction::Play(30)
            });
        }
    }

    pub fn first_elapsed(&self) -> f32 {
        self.first_elapsed
    }
    pub fn take_music_actions(&mut self) -> Vec<TitleMusicAction> {
        std::mem::take(&mut self.music_actions)
    }

    fn transition(&mut self, phase: TitlePhase) {
        self.phase = phase;
        self.elapsed = 0.0;
    }

    fn entrance_ticks(&self) -> f32 {
        20.0 + 4.0 * self.rows.len().saturating_sub(1) as f32
    }

    fn leave_ticks(&self) -> f32 {
        let tail = 4.0 * self.rows.len().saturating_sub(1) as f32;
        tail + if self.leaving_row.is_some() {
            60.0
        } else {
            20.0
        }
    }

    /// Native tasks poll once per update; surplus time is deliberately not
    /// carried into a newly installed phase, child or scene factory callback.
    pub fn update(&mut self, delta_ticks: f32) -> Option<TitleAction> {
        if !delta_ticks.is_finite() || delta_ticks < 0.0 {
            return None;
        }
        if self.phase == TitlePhase::Opening {
            // DD110 observes the previous capture clock. Negative requests
            // install a separate Start task; positive returns enter directly.
            if self.first_elapsed >= 60. {
                if self.first_entry {
                    self.transition(TitlePhase::Starting);
                } else {
                    self.transition(TitlePhase::Entering);
                    self.returning_rows = true;
                    self.elapsed = delta_ticks;
                }
            } else {
                self.first_elapsed = (self.first_elapsed + delta_ticks).min(60.);
            }
            self.advance_retained_parent_clock();
            return None;
        }
        if self.phase == TitlePhase::Starting {
            // DDB38's resource clock is ready after our awaited asset load.
            self.transition(TitlePhase::Entering);
            self.returning_rows = true;
            self.elapsed = delta_ticks;
            self.music_actions.push(TitleMusicAction::Play(0));
            self.advance_retained_parent_clock();
            return None;
        }
        if self.returning_rows && self.phase == TitlePhase::Entering {
            // DD720 polls the previous parent frame's row animators. Its new
            // Input task cannot run until the following TaskGroup snapshot.
            if self.elapsed >= self.entrance_ticks() {
                self.returning_rows = false;
                self.transition(TitlePhase::Active);
            } else {
                self.elapsed += delta_ticks;
            }
            self.advance_retained_parent_clock();
            return None;
        }
        // DA170 polls TaskGroup first, then advances the blackout animator by
        // literal 1.0. Publication observes the previous parent frame's tween.
        if self.phase == TitlePhase::Fading {
            if self.elapsed >= 120.0 {
                let item = self.pending.take()?;
                self.transition(TitlePhase::Handoff);
                return Self::action(item);
            }
            self.elapsed += 1.0;
            return None;
        }
        self.elapsed += delta_ticks;
        match self.phase {
            TitlePhase::Entering if self.elapsed >= self.entrance_ticks() => {
                self.transition(TitlePhase::Active);
            }
            TitlePhase::Leaving if self.elapsed >= self.leave_ticks() => {
                match self.pending.take() {
                    Some(TitleItem::Extra) => {
                        self.build_rows(8);
                        self.transition(TitlePhase::Entering);
                    }
                    Some(TitleItem::Back) => {
                        self.build_rows(4);
                        self.transition(TitlePhase::Entering);
                    }
                    Some(item @ (TitleItem::NewGame | TitleItem::Continue)) => {
                        self.pending = Some(item);
                        self.transition(TitlePhase::Fading);
                    }
                    Some(item) => {
                        // Only retained Load/Config/Manual children dim Title.
                        // CG/Music/Bustup replace the outer mode instead.
                        if matches!(
                            item,
                            TitleItem::Load | TitleItem::Config | TitleItem::Manual
                        ) {
                            self.child_blackout.control(0.5);
                        }
                        self.transition(TitlePhase::Child);
                        self.advance_retained_parent_clock();
                        return Self::action(item);
                    }
                    None => {}
                }
            }
            _ => {}
        }
        self.advance_retained_parent_clock();
        None
    }

    /// DA170 polls the retained child's TaskGroup before advancing +D0 by one,
    /// independently of row/child delta. Hosts call this after child callbacks
    /// on frames where ordinary Title::update is suspended by child ownership.
    pub fn advance_retained_parent_clock(&mut self) {
        self.child_blackout.advance();
    }

    /// After child callbacks, DA170 advances freshly installed DBA20 row
    /// animators in this same frame, without polling their new completion task.
    pub fn advance_retained_parent_frame(&mut self, delta_ticks: f32) {
        if !delta_ticks.is_finite() || delta_ticks < 0.0 {
            return;
        }
        if self.returning_rows && self.phase == TitlePhase::Entering {
            self.elapsed += delta_ticks;
        }
        self.advance_retained_parent_clock();
    }

    fn action(item: TitleItem) -> Option<TitleAction> {
        Some(match item {
            TitleItem::NewGame => TitleAction::NewGame { init_value: 0 },
            TitleItem::Continue => TitleAction::Continue,
            TitleItem::Load => TitleAction::Load,
            TitleItem::Config => TitleAction::Config,
            TitleItem::Manual => TitleAction::Manual,
            TitleItem::MusicGallery => TitleAction::MusicGallery,
            TitleItem::CgGallery => TitleAction::CgGallery,
            TitleItem::BustupGallery => TitleAction::BustupGallery,
            TitleItem::Extra | TitleItem::Back => return None,
        })
    }

    /// Priority is Confirm, Back, Down, Up, matching DC098.
    pub fn input(&mut self, input: TitleInput) -> bool {
        if self.phase != TitlePhase::Active {
            return false;
        }
        if input.confirm {
            self.confirm_selected();
        } else if input.back {
            if self.extra {
                self.leaving_row = None;
                self.pending = Some(TitleItem::Back);
                self.transition(TitlePhase::Leaving);
            }
        } else if input.down {
            self.selected = Some(self.selected.map_or(0, |v| (v + 1) % self.rows.len()));
        } else if input.up {
            self.selected = Some(self.selected.filter(|v| *v > 0).unwrap_or(self.rows.len()) - 1);
        }
        true
    }

    pub fn select(&mut self, index: usize) -> bool {
        if self.phase != TitlePhase::Active || index >= self.rows.len() {
            return false;
        }
        self.selected = Some(index);
        true
    }

    pub fn confirm_selected(&mut self) -> bool {
        if self.phase != TitlePhase::Active {
            return false;
        }
        let Some(row) = self.selected else {
            return false;
        };
        self.leaving_row = Some(row);
        self.pending = Some(self.rows[row]);
        self.transition(TitlePhase::Leaving);
        true
    }

    /// The native pointer callback uses inclusive left/top and exclusive
    /// right/bottom bounds. NaNs and points outside the rows do not hit.
    pub fn point(&self, x: f32, y: f32) -> Option<usize> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        (0..self.rows.len()).find(|index| {
            let [left, top, width, height] = button_rect(*index);
            x >= left && x < left + width && y >= top && y < top + height
        })
    }

    /// Restores the retained page and selection after an ordinary child closes,
    /// or after the host reports a child preparation error.
    pub fn recover_child(&mut self) {
        if self.phase == TitlePhase::Child {
            self.child_blackout.control(0.0);
            self.transition(TitlePhase::Entering);
            self.returning_rows = true;
        }
    }

    /// Host asset-error recovery: a failed ADV preparation keeps the title
    /// retryable. This is a deliberate host boundary, not native unwind parity.
    pub fn recover_handoff(&mut self) {
        if self.phase == TitlePhase::Handoff {
            self.transition(TitlePhase::Entering);
        }
    }

    /// White additive confirmation flash, native row+C0 animator. Native draw
    /// picks the focused sheet directly and adds this white flash separately.
    pub fn row_flash(&self, index: usize) -> f32 {
        if self.phase != TitlePhase::Leaving || self.leaving_row != Some(index) {
            return 0.0;
        }
        (-0.7f32).mul_add(((self.elapsed - 5.0) / 10.0).clamp(0.0, 1.0), 0.7)
    }

    /// Renderer convenience for a black incoming capture and ADV fade. The
    /// original First can fade an actual outgoing capture instead of black.
    pub fn blackout(&self) -> f32 {
        match self.phase {
            TitlePhase::Loading => 1.0,
            TitlePhase::Opening => (1.0 - self.first_elapsed / 60.0).clamp(0.0, 1.0),
            TitlePhase::Fading => (self.elapsed / 120.0).clamp(0.0, 1.0),
            TitlePhase::Handoff => 1.0,
            _ => self.child_blackout.value(),
        }
    }

    pub fn row_alpha(&self, index: usize) -> f32 {
        if index >= self.rows.len() {
            return 0.0;
        }
        match self.phase {
            TitlePhase::Loading
            | TitlePhase::Opening
            | TitlePhase::Starting
            | TitlePhase::Fading
            | TitlePhase::Child
            | TitlePhase::Handoff => 0.0,
            TitlePhase::Entering => ((self.elapsed - index as f32 * 4.0) / 20.0).clamp(0.0, 1.0),
            TitlePhase::Leaving => {
                let (delay, duration) = if self.leaving_row == Some(index) {
                    (4.0 * self.rows.len().saturating_sub(1) as f32 + 30.0, 30.0)
                } else {
                    let ordinal = index - usize::from(self.leaving_row.is_some_and(|v| v < index));
                    (ordinal as f32 * 4.0, 20.0)
                };
                (1.0 - (self.elapsed - delay) / duration).clamp(0.0, 1.0)
            }
            TitlePhase::Active => 1.0,
        }
    }
}
