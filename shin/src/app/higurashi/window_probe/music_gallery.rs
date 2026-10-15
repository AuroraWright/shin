//! Actual-window Music journey. The parent supplies read-only snapshots of the
//! resident production Opening/Owner; this driver constructs no game owners.
use super::{Snapshot, controller_button, key};
use anyhow::{Context, Result, ensure};
use shin_input::{RawInputState, inputs::GamepadButton};
use std::path::Path;
use winit::keyboard::KeyCode;

/// Read after the normal production update. Audio status can change on its own
/// thread, and the worker can publish resources independently of a main poll.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) struct State {
    pub atlas: bool,
    pub mask: bool,
    pub first_complete: bool,
    pub input: bool,
    pub closing: bool,
    pub selected: usize,
    pub playing: i32,
    pub rows: usize,
    pub scroll_bits: u32,
    pub scroll_busy: bool,
    /// The UI elapsed field, not an audio playback position.
    pub elapsed_bits: u32,
    /// Actual catalog words and unlock snapshot for the selected resident row.
    pub bgm: u16,
    pub once: u16,
    pub unlocked: bool,
    /// Actual MusicCommands::bgm_status(), never a synthesized elapsed clock.
    pub bgm_status: Option<u32>,
    /// Successful actual Opening Cue dispatches: move, decide, cancel, error.
    pub cues: [u64; 4],
}
impl State {
    fn ready(&self) -> bool {
        // First can append Input before First is pruned. Its mask only retires
        // at the following prune; do not mistake appended Input for its poll.
        self.atlas
            && self.first_complete
            && self.input
            && !self.mask
            && !self.closing
            && !self.scroll_busy
    }

    /// The parent calls this around its actual render callback. Only the real
    /// pending worker's false->true publications are allowed to race the read.
    /// Audio status is sampled independently and is not render-owned state.
    pub(super) fn assert_render_getter(&self, after: &Self) -> Result<()> {
        let mut before_stable = self.clone();
        let mut after_stable = after.clone();
        before_stable.atlas = false;
        before_stable.mask = false;
        before_stable.bgm_status = None;
        after_stable.atlas = false;
        after_stable.mask = false;
        after_stable.bgm_status = None;
        ensure!(
            before_stable == after_stable,
            "Music render advanced owner/input/clocks/cues: {self:?} -> {after:?}"
        );
        ensure!(
            !self.mask || self.atlas,
            "Music mask was published before its atlas: {self:?}"
        );
        ensure!(
            !after.mask || after.atlas,
            "Music mask was published before its atlas: {after:?}"
        );
        if self.first_complete {
            ensure!(
                self.atlas == after.atlas && self.mask == after.mask,
                "Completed Music resources changed during render: {self:?} -> {after:?}"
            );
        } else {
            ensure!(
                (!self.atlas || after.atlas) && (!self.mask || after.mask),
                "Pending Music resources retired during render: {self:?} -> {after:?}"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Binding {
    Keyboard,
    Controller,
}
impl Binding {
    fn play(self) -> RawInputState {
        match self {
            Self::Keyboard => key(KeyCode::Enter),
            Self::Controller => controller_button(GamepadButton::A),
        }
    }
    fn stop(self) -> RawInputState {
        match self {
            Self::Keyboard => key(KeyCode::KeyM),
            Self::Controller => controller_button(GamepadButton::Y),
        }
    }
    fn close(self) -> RawInputState {
        match self {
            Self::Keyboard => key(KeyCode::Escape),
            Self::Controller => controller_button(GamepadButton::B),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    EntryRelease,
    OpenCategory,
    AwaitCategory,
    SelectMusic,
    MusicSelected,
    OpenMusic,
    AwaitMusic,
    MusicRelease,
    NavigateApplied(usize),
    NavigateSettling(usize),
    NavigateReleased(usize),
    PlayApplied(Binding),
    PlayReleased(Binding),
    StopApplied(Binding),
    StopReleased(Binding),
    CloseApplied(Binding),
    AwaitCategoryReturn,
    CategoryReturnReleased,
    AwaitTopReturn,
    Done,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Report {
    pub polls: u64,
    pub category_default_cg: bool,
    pub category_music_selected: bool,
    pub ready_visits: u32,
    pub first_prunes_observed: u32,
    pub keyboard_arrows: u32,
    pub controller_dpad: u32,
    pub keyboard_shoulders: u32,
    pub controller_shoulders: u32,
    pub neutral_settled_navigation: u32,
    pub movement_cues: u32,
    pub keyboard_play: bool,
    pub controller_play: bool,
    pub keyboard_stop: bool,
    pub controller_stop: bool,
    pub literal_play_status: Vec<u32>,
    pub no_extra_decide: bool,
    pub keyboard_close: bool,
    pub controller_close: bool,
    pub cancellation_cues: u32,
    pub fresh_title_sixteen: u32,
    pub category_back_top_five: bool,
    pub progress_unchanged: bool,
    pub commands_frozen: bool,
}
impl Report {
    pub(super) fn complete(&self) -> bool {
        self.category_default_cg
            && self.category_music_selected
            && self.ready_visits == 2
            && self.first_prunes_observed == 2
            && self.keyboard_arrows == 3
            && self.controller_dpad == 3
            && self.keyboard_shoulders == 2
            && self.controller_shoulders == 2
            && self.neutral_settled_navigation == 10
            && self.movement_cues == 10
            && self.keyboard_play
            && self.controller_play
            && self.keyboard_stop
            && self.controller_stop
            && self.literal_play_status.len() == 2
            && self.literal_play_status.iter().all(|s| s & 2 != 0)
            && self.no_extra_decide
            && self.keyboard_close
            && self.controller_close
            && self.cancellation_cues == 2
            && self.fresh_title_sixteen == 2
            && self.category_back_top_five
            && self.progress_unchanged
            && self.commands_frozen
    }
}

pub(super) struct Step {
    pub raw: RawInputState,
    pub done: bool,
}
pub(super) struct Journey {
    phase: Phase,
    disk_before: Vec<u8>,
    commands_before: u64,
    title_mode: u32,
    expected_cues: [u64; 4],
    last_elapsed: Option<f32>,
    report: Report,
}
impl Journey {
    /// Parent completes the actual CG return to Top5, then constructs this
    /// driver. The first sample releases every key/controller button.
    pub(super) fn new(snapshot: &Snapshot, progress: &Path) -> Result<Self> {
        require_top(snapshot)?;
        let (_, title_mode) = snapshot
            .title_initial
            .context("Music source title initial state")?;
        Ok(Self {
            phase: Phase::EntryRelease,
            disk_before: std::fs::read(progress).context("Music progress baseline")?,
            commands_before: snapshot.commands,
            title_mode,
            expected_cues: [0; 4],
            last_elapsed: None,
            report: Report::default(),
        })
    }

    pub(super) fn report(&self) -> &Report {
        &self.report
    }

    pub(super) fn poll(&mut self, snapshot: &Snapshot, progress: &Path) -> Result<Step> {
        self.report.polls += 1;
        ensure!(
            snapshot.commands == self.commands_before,
            "Music source VM command count changed at {:?}: {} -> {}",
            self.phase,
            self.commands_before,
            snapshot.commands
        );
        ensure!(
            std::fs::read(progress).context("Music progress comparison")? == self.disk_before,
            "Music journey wrote progress/settings at {:?}",
            self.phase
        );
        if let Some(music) = &snapshot.music {
            ensure!(
                snapshot.menu.is_none()
                    && snapshot.category.is_none()
                    && snapshot.title_initial.is_none()
                    && snapshot.top_selected.is_none()
                    && snapshot.cg.is_none(),
                "Music retained an active source scene at {:?}",
                self.phase
            );
            require_no_child(snapshot)?;
            ensure!(
                music.rows == 158 && music.selected < music.rows,
                "Music supplied catalog/selection differs: {music:?}"
            );
            let elapsed = f32::from_bits(music.elapsed_bits);
            let scroll = f32::from_bits(music.scroll_bits);
            ensure!(
                elapsed.is_finite()
                    && elapsed >= 0.
                    && self.last_elapsed.is_none_or(|previous| elapsed >= previous),
                "Music UI elapsed regressed within a resident owner: {:?} -> {elapsed}",
                self.last_elapsed
            );
            ensure!(
                scroll.is_finite() && (0. ..=6450.).contains(&scroll),
                "Music scroll left the authored 7268/818 range: {music:?}"
            );
            self.last_elapsed = Some(elapsed);
        } else {
            self.last_elapsed = None;
        }

        let mut raw = RawInputState::new();
        match self.phase {
            Phase::EntryRelease => {
                require_top(snapshot)?;
                self.phase = Phase::OpenCategory;
            }
            Phase::OpenCategory => {
                require_top(snapshot)?;
                raw = key(KeyCode::Enter);
                self.phase = Phase::AwaitCategory;
            }
            Phase::AwaitCategory => {
                if category_ready(snapshot, 5) {
                    require_category(snapshot, 5)?;
                    self.report.category_default_cg = true;
                    self.phase = Phase::SelectMusic;
                }
            }
            Phase::SelectMusic => {
                require_category(snapshot, 5)?;
                // Category is a vertical list: native repeat6 selects6 from5.
                // Right/Left below apply to Music's two-column rows instead.
                raw = key(KeyCode::ArrowDown);
                self.phase = Phase::MusicSelected;
            }
            Phase::MusicSelected => {
                require_category(snapshot, 6)?;
                self.report.category_music_selected = true;
                self.phase = Phase::OpenMusic;
            }
            Phase::OpenMusic => {
                require_category(snapshot, 6)?;
                raw = controller_button(GamepadButton::A);
                self.phase = Phase::AwaitMusic;
            }
            Phase::AwaitMusic => {
                if let Some(music) = snapshot.music.as_ref().filter(|m| m.ready()) {
                    require_row(music, 0)?;
                    ensure!(
                        music.playing == -1
                            && music.scroll_bits == 0f32.to_bits()
                            && music.cues == [0; 4],
                        "Music fresh Input retained a marker/scroll/cue: {music:?}"
                    );
                    self.expected_cues = music.cues;
                    self.report.ready_visits += 1;
                    self.report.first_prunes_observed += 1;
                    ensure!(
                        self.report.ready_visits <= 2,
                        "Unexpected third Music visit"
                    );
                    self.phase = Phase::MusicRelease;
                }
            }
            Phase::MusicRelease => {
                self.settled(snapshot, 0, 0., -1)?;
                if self.report.ready_visits == 1 {
                    raw = navigation(0).raw;
                    self.phase = Phase::NavigateApplied(0);
                } else {
                    raw = Binding::Controller.close();
                    self.phase = Phase::CloseApplied(Binding::Controller);
                }
            }
            Phase::NavigateApplied(index) => {
                let command = navigation(index);
                let music = self.active(snapshot, command.selected, -1)?;
                let expected = increment(self.expected_cues, 0)?;
                ensure!(
                    music.cues == expected,
                    "Music navigation must emit exactly one move cue: {:?} -> {:?}",
                    self.expected_cues,
                    music.cues
                );
                self.expected_cues = expected;
                self.report.movement_cues += 1;
                match index {
                    0 | 2 | 9 => self.report.keyboard_arrows += 1,
                    1 | 3 | 8 => self.report.controller_dpad += 1,
                    5 | 6 => self.report.keyboard_shoulders += 1,
                    4 | 7 => self.report.controller_shoulders += 1,
                    _ => unreachable!("Bounded Music navigation sequence"),
                }
                self.phase = Phase::NavigateSettling(index);
            }
            Phase::NavigateSettling(index) => {
                let command = navigation(index);
                let music = self.active(snapshot, command.selected, -1)?;
                self.require_cues(music)?;
                if !music.scroll_busy {
                    self.settled(snapshot, command.selected, command.scroll, -1)?;
                    self.phase = Phase::NavigateReleased(index);
                }
            }
            Phase::NavigateReleased(index) => {
                let command = navigation(index);
                self.settled(snapshot, command.selected, command.scroll, -1)?;
                self.report.neutral_settled_navigation += 1;
                if index + 1 < 10 {
                    raw = navigation(index + 1).raw;
                    self.phase = Phase::NavigateApplied(index + 1);
                } else {
                    raw = Binding::Keyboard.play();
                    self.phase = Phase::PlayApplied(Binding::Keyboard);
                }
            }
            Phase::PlayApplied(binding) => {
                let music = self.settled(snapshot, 0, 0., 0)?;
                let status = playing_status(music)?;
                self.report.literal_play_status.push(status);
                match binding {
                    Binding::Keyboard => self.report.keyboard_play = true,
                    Binding::Controller => self.report.controller_play = true,
                }
                self.phase = Phase::PlayReleased(binding);
            }
            Phase::PlayReleased(binding) => {
                playing_status(self.settled(snapshot, 0, 0., 0)?)?;
                raw = binding.stop();
                self.phase = Phase::StopApplied(binding);
            }
            Phase::StopApplied(binding) => {
                self.settled(snapshot, 0, 0., -1)?;
                // StopBgm30 can still have an active actual audio source. Only
                // the synchronous native playing marker is expected to clear.
                match binding {
                    Binding::Keyboard => self.report.keyboard_stop = true,
                    Binding::Controller => self.report.controller_stop = true,
                }
                self.phase = Phase::StopReleased(binding);
            }
            Phase::StopReleased(binding) => {
                self.settled(snapshot, 0, 0., -1)?;
                match binding {
                    Binding::Keyboard => {
                        raw = Binding::Controller.play();
                        self.phase = Phase::PlayApplied(Binding::Controller);
                    }
                    Binding::Controller => {
                        ensure!(
                            self.expected_cues == [10, 0, 0, 0],
                            "Music play/stop introduced an extra cue: {:?}",
                            self.expected_cues
                        );
                        self.report.no_extra_decide = true;
                        raw = Binding::Keyboard.close();
                        self.phase = Phase::CloseApplied(Binding::Keyboard);
                    }
                }
            }
            Phase::CloseApplied(binding) => {
                let music = snapshot
                    .music
                    .as_ref()
                    .context("Music retired before native retained close flash")?;
                require_row(music, 0)?;
                ensure!(
                    music.atlas
                        && music.first_complete
                        && !music.mask
                        && music.closing
                        && !music.input
                        && music.playing == -1,
                    "Music B skipped its native retained closing state: {music:?}"
                );
                let expected = increment(self.expected_cues, 2)?;
                ensure!(
                    music.cues == expected,
                    "Music close must emit exactly one cancel cue: {:?} -> {:?}",
                    self.expected_cues,
                    music.cues
                );
                self.expected_cues = expected;
                self.report.cancellation_cues += 1;
                match binding {
                    Binding::Keyboard => self.report.keyboard_close = true,
                    Binding::Controller => self.report.controller_close = true,
                }
                self.phase = Phase::AwaitCategoryReturn;
            }
            Phase::AwaitCategoryReturn => {
                if let Some(music) = &snapshot.music {
                    ensure!(music.closing, "Music close task lost its closing state");
                    self.require_cues(music)?;
                } else if category_ready(snapshot, 6) {
                    require_category(snapshot, 6)?;
                    ensure!(
                        snapshot.title_initial == Some((16, self.title_mode)),
                        "Music did not construct fresh Title16 with remembered mode: {:?}",
                        snapshot.title_initial
                    );
                    self.report.fresh_title_sixteen += 1;
                    self.phase = Phase::CategoryReturnReleased;
                }
            }
            Phase::CategoryReturnReleased => {
                require_category(snapshot, 6)?;
                ensure!(
                    snapshot.title_initial == Some((16, self.title_mode)),
                    "Music category return replaced the new Title"
                );
                if self.report.ready_visits == 1 {
                    // Second natural visit distinguishes actual Music B from
                    // the separate Category B that returns to Top5.
                    raw = key(KeyCode::Enter);
                    self.phase = Phase::AwaitMusic;
                } else {
                    raw = controller_button(GamepadButton::B);
                    self.phase = Phase::AwaitTopReturn;
                }
            }
            Phase::AwaitTopReturn => {
                if snapshot.music.is_none()
                    && snapshot.cg.is_none()
                    && snapshot.category.is_none()
                    && snapshot.menu == Some(("top", 2))
                    && snapshot.top_selected == Some(5)
                {
                    require_top(snapshot)?;
                    ensure!(
                        snapshot.title_initial == Some((16, self.title_mode)),
                        "Music Category B replaced the fresh Title unexpectedly"
                    );
                    self.report.category_back_top_five = true;
                    self.report.progress_unchanged = true;
                    self.report.commands_frozen = true;
                    ensure!(self.report.complete(), "Music window report incomplete");
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {
                require_top(snapshot)?;
            }
        }
        Ok(Step {
            raw,
            done: matches!(self.phase, Phase::Done),
        })
    }

    fn require_cues(&self, music: &State) -> Result<()> {
        ensure!(
            music.cues == self.expected_cues,
            "Unexpected Music cue at {:?}: {:?}, expected {:?}",
            self.phase,
            music.cues,
            self.expected_cues
        );
        Ok(())
    }

    fn active<'a>(
        &self,
        snapshot: &'a Snapshot,
        selected: usize,
        playing: i32,
    ) -> Result<&'a State> {
        let music = snapshot
            .music
            .as_ref()
            .context("Actual resident Music owner missing")?;
        require_row(music, selected)?;
        ensure!(
            music.atlas
                && music.first_complete
                && music.input
                && !music.mask
                && !music.closing
                && music.playing == playing,
            "Music owner/playing state differs at {:?}: {music:?}",
            self.phase
        );
        Ok(music)
    }

    fn settled<'a>(
        &self,
        snapshot: &'a Snapshot,
        selected: usize,
        scroll: f32,
        playing: i32,
    ) -> Result<&'a State> {
        let music = self.active(snapshot, selected, playing)?;
        self.require_cues(music)?;
        ensure!(
            !music.scroll_busy && music.scroll_bits == scroll.to_bits(),
            "Music scroll is not settled at native target{scroll}: {music:?}"
        );
        Ok(music)
    }
}

struct Navigation {
    raw: RawInputState,
    selected: usize,
    scroll: f32,
}
fn navigation(index: usize) -> Navigation {
    // 30c44: L/R move eight pairs, not eight individual entries.30e70 reveals
    // within the exact818 viewport using authored pair height92 and a4tick tween.
    let (raw, selected, scroll) = match index {
        0 => (key(KeyCode::ArrowRight), 1, 0.),
        1 => (controller_button(GamepadButton::Down), 3, 0.),
        2 => (key(KeyCode::ArrowLeft), 2, 0.),
        3 => (controller_button(GamepadButton::Down), 4, 0.),
        4 => (controller_button(GamepadButton::R), 20, 194.),
        5 => (key(KeyCode::KeyH), 4, 184.),
        6 => (key(KeyCode::KeyJ), 20, 194.),
        7 => (controller_button(GamepadButton::L), 4, 184.),
        8 => (controller_button(GamepadButton::Up), 2, 92.),
        9 => (key(KeyCode::ArrowUp), 0, 0.),
        _ => unreachable!("Bounded Music window navigation sequence"),
    };
    Navigation {
        raw,
        selected,
        scroll,
    }
}
fn increment(mut cues: [u64; 4], index: usize) -> Result<[u64; 4]> {
    cues[index] = cues[index]
        .checked_add(1)
        .context("Music cue counter overflow")?;
    Ok(cues)
}
fn require_row(music: &State, selected: usize) -> Result<()> {
    // All visited supplied records0..4/20 have bgmID==index and once_flag0.
    // These are actual typed catalog words, not a generalized row-ID formula.
    ensure!(
        [0, 1, 2, 3, 4, 20].contains(&selected),
        "Unspecified Music authored fixture row{selected}"
    );
    ensure!(
        music.rows == 158
            && music.selected == selected
            && music.bgm == selected as u16
            && music.once == 0
            && music.unlocked,
        "Music supplied row{selected} metadata/unlock differs: {music:?}"
    );
    Ok(())
}
fn playing_status(music: &State) -> Result<u32> {
    let status = music
        .bgm_status
        .context("Music actual BGM status service missing")?;
    ensure!(
        status & 2 != 0,
        "Music looped catalog0 play lost actual status bit2: {status:#x}"
    );
    Ok(status)
}
fn category_ready(snapshot: &Snapshot, selected: i32) -> bool {
    snapshot.music.is_none()
        && snapshot.cg.is_none()
        && snapshot.menu == Some(("cat", 2))
        && snapshot.category == Some((1, selected, 2))
}
fn require_category(snapshot: &Snapshot, selected: i32) -> Result<()> {
    ensure!(
        category_ready(snapshot, selected),
        "Actual Gallery Category1/selected{selected} is not ready"
    );
    require_no_child(snapshot)
}
fn require_top(snapshot: &Snapshot) -> Result<()> {
    ensure!(
        snapshot.music.is_none()
            && snapshot.cg.is_none()
            && snapshot.category.is_none()
            && snapshot.menu == Some(("top", 2))
            && snapshot.top_selected == Some(5),
        "Music source has not returned to actual ready Top5"
    );
    require_no_child(snapshot)
}
fn require_no_child(snapshot: &Snapshot) -> Result<()> {
    ensure!(
        snapshot.config.is_none()
            && snapshot.manual.is_none()
            && snapshot.chart.is_none()
            && snapshot.picker.is_none()
            && snapshot.system_menu.is_none()
            && !snapshot.tips,
        "Music journey has an unrelated modal/destination child"
    );
    Ok(())
}
// The parent owns registration, compilation and real-window/exactLoopback execution.
