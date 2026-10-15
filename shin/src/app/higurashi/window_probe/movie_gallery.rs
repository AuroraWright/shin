//! Source-only Movie window journey. All owners, media and navigation are
//! supplied by the normal application; this driver sends canonical raw input.
use super::{Snapshot, controller_button, key};
use crate::app::higurashi::movie_gallery::owner::{ReferenceScopeState, ReferenceViewerState};
use anyhow::{Context, Result, ensure};
use shin_input::{
    RawInputState,
    inputs::{GamepadAxis, GamepadButton, VirtualGamepadButton},
};
use std::path::Path;
use winit::keyboard::KeyCode;

/// Read directly from the actual resident Opening/Owner. Player status and
/// clock are independent transport observations, never default placeholders.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::app::higurashi) struct State {
    pub atlas: bool,
    pub mask: bool,
    pub first_complete: bool,
    pub input: bool,
    pub closing: bool,
    /// Atlas-gated native parent counter, not decoder FrameId or elapsed time.
    pub frame: u32,
    pub selected: usize,
    pub rows: usize,
    pub scroll_bits: u32,
    pub scroll_busy: bool,
    pub close_bits: u32,
    pub selected_movie: u16,
    pub selected_locked: bool,
    pub scope: ReferenceScopeState,
    pub viewer: Option<ReferenceViewerState>,
    /// Successful actual Opening Cue dispatches: move, decide, cancel, error.
    pub cues: [u64; 4],
}
impl State {
    fn grid_ready(&self) -> bool {
        self.atlas
            && !self.mask
            && self.first_complete
            && self.input
            && !self.closing
            && !self.scroll_busy
            && self.viewer.is_none()
            && self.scope.tasks == 1
            && self.scope.borrowed_tasks == 1
            && self.scope.viewers == 0
            && self.scope.borrowed_viewers == 0
            && self.scope.media_owners == 0
            && self.scope.first_tasks == 0
            && !self.scope.capture_present
    }

    /// Actual Player time/status can change on a worker independently of render.
    /// All model, scope, input, media ownership and retained RGBA observations
    /// must remain exact. Pending atlas/mask publication alone can race a read.
    pub(super) fn assert_render_getter(&self, after: &Self) -> Result<()> {
        let mut before_stable = self.clone();
        let mut after_stable = after.clone();
        for stable in [&mut before_stable, &mut after_stable] {
            stable.atlas = false;
            stable.mask = false;
            if let Some(viewer) = &mut stable.viewer {
                viewer.status = 0;
                viewer.time_micros = None;
            }
        }
        ensure!(
            before_stable == after_stable,
            "Movie render advanced model/input/scope/cues/retained frame: {self:?} -> {after:?}"
        );
        ensure!(
            (!self.mask || self.atlas) && (!after.mask || after.atlas),
            "Movie mask was published before atlas: {self:?} -> {after:?}"
        );
        if self.first_complete {
            ensure!(
                self.atlas == after.atlas && self.mask == after.mask,
                "Completed Movie resources changed in render: {self:?} -> {after:?}"
            );
        } else {
            ensure!(
                (!self.atlas || after.atlas) && (!self.mask || after.mask),
                "Pending Movie resources retired in render: {self:?} -> {after:?}"
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
    fn enter(self) -> RawInputState {
        match self {
            Self::Keyboard => key(KeyCode::Enter),
            Self::Controller => controller_button(GamepadButton::A),
        }
    }
    fn cancel(self) -> RawInputState {
        match self {
            Self::Keyboard => key(KeyCode::Escape),
            Self::Controller => controller_button(GamepadButton::B),
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum ViewerClose {
    ControllerA,
    ControllerB,
    KeyboardEscape,
}
impl ViewerClose {
    fn raw(self) -> RawInputState {
        match self {
            Self::ControllerA => controller_button(GamepadButton::A),
            Self::ControllerB => controller_button(GamepadButton::B),
            Self::KeyboardEscape => key(KeyCode::Escape),
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Command {
    Navigate(usize),
    IgnoredShoulder(usize),
    LockedConfirm,
    Viewer(usize),
    Close(Binding),
}
#[derive(Clone, Copy, Debug)]
enum Phase {
    EntryRelease,
    OpenCategory,
    AwaitCategory,
    SelectMovie,
    MovieSelected,
    OpenMovie,
    AwaitMovie,
    GridRelease,
    CommandApplied(Command),
    CommandSettling(Command),
    CommandReleased,
    AwaitViewer(usize),
    ViewerRelease(usize),
    FocusHold,
    Playback(usize),
    ViewerCloseApplied(usize),
    AwaitGridReturn(usize),
    GridReturnReleased,
    ParentCloseApplied(Binding),
    AwaitCategoryReturn,
    CategoryReturnReleased,
    AwaitTopReturn,
    Done,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Report {
    pub polls: u64,
    pub category_default_cg: bool,
    pub category_movie_selected: bool,
    pub ready_visits: u32,
    pub first_prunes_observed: u32,
    pub keyboard_arrows: u32,
    pub controller_dpad: u32,
    pub first_stick_directions: u32,
    pub neutral_settled_moves: u32,
    pub ignored_keyboard_shoulders: u32,
    pub ignored_controller_shoulders: u32,
    pub movement_cues: u32,
    pub locked_error: bool,
    pub authored_movie_ids: Vec<u16>,
    pub literal_play_status: Vec<u32>,
    pub retained_extents: Vec<(u32, u32)>,
    pub observed_time_pairs: Vec<(u64, u64)>,
    /// Child/parent handshake only; physical focus observations live in the
    /// parent window driver's separate report.
    pub focus_checkpoint_requested: bool,
    pub focus_checkpoint_resumed: bool,
    pub keyboard_open: bool,
    pub controller_opens: u32,
    pub viewer_controller_a_close: bool,
    pub viewer_controller_b_close: bool,
    pub viewer_keyboard_escape_close: bool,
    pub same_selected_grid_returns: u32,
    pub media_prunes_observed: u32,
    pub keyboard_parent_close: bool,
    pub controller_parent_close: bool,
    pub fresh_title_seventeen: u32,
    pub category_back_top_five: bool,
    pub progress_unchanged: bool,
    pub commands_frozen: bool,
}
impl Report {
    pub(super) fn complete(&self) -> bool {
        self.category_default_cg
            && self.category_movie_selected
            && self.ready_visits == 2
            && self.first_prunes_observed == 2
            && self.keyboard_arrows == 4
            && self.controller_dpad == 4
            && self.first_stick_directions == 2
            && self.neutral_settled_moves == 10
            && self.ignored_keyboard_shoulders == 2
            && self.ignored_controller_shoulders == 2
            && self.movement_cues == 10
            && self.locked_error
            && self.authored_movie_ids == [6, 9, 0]
            && self.literal_play_status == [1, 1, 1]
            && self.retained_extents == [(1920, 1080); 3]
            && self.observed_time_pairs.len() == 3
            && self
                .observed_time_pairs
                .iter()
                .all(|&(start, end)| end >= start.saturating_add(100_000))
            && self.focus_checkpoint_requested
            && self.focus_checkpoint_resumed
            && self.keyboard_open
            && self.controller_opens == 2
            && self.viewer_controller_a_close
            && self.viewer_controller_b_close
            && self.viewer_keyboard_escape_close
            && self.same_selected_grid_returns == 3
            && self.media_prunes_observed == 3
            && self.keyboard_parent_close
            && self.controller_parent_close
            && self.fresh_title_seventeen == 2
            && self.category_back_top_five
            && self.progress_unchanged
            && self.commands_frozen
    }
}
pub(super) struct Step {
    pub raw: RawInputState,
    pub done: bool,
    /// One-shot request. Parent owns companion-window focus/suspend/resume and
    /// suppresses child polling until resume_after_focus succeeds.
    pub focus_checkpoint: bool,
}
pub(super) struct Journey {
    phase: Phase,
    disk_before: Vec<u8>,
    commands_before: u64,
    title_mode: u32,
    expected_cues: [u64; 4],
    command_index: usize,
    selected: usize,
    viewer_id: u64,
    playback_start: u64,
    focus_time: u64,
    report: Report,
}
impl Journey {
    /// Called only after Music has returned through its real fresh Title to
    /// ready Top5. The first sample is neutral; no destination is injected.
    pub(super) fn new(snapshot: &Snapshot, progress: &Path) -> Result<Self> {
        require_top(snapshot)?;
        let (_, title_mode) = snapshot
            .title_initial
            .context("Movie source Title initial state")?;
        Ok(Self {
            phase: Phase::EntryRelease,
            disk_before: std::fs::read(progress).context("Movie progress baseline")?,
            commands_before: snapshot.commands,
            title_mode,
            expected_cues: [0; 4],
            command_index: 0,
            selected: 0,
            viewer_id: 0,
            playback_start: 0,
            focus_time: 0,
            report: Report::default(),
        })
    }
    pub(super) fn report(&self) -> &Report {
        &self.report
    }

    /// Parent calls only after real focus restore, a neutral production input
    /// update and independently observed resumed media-time progress. This
    /// method validates retained state; it creates no owner and advances no
    /// model/media clock or input. It does not itself prove OS focus behavior.
    pub(super) fn resume_after_focus(
        &mut self,
        snapshot: &Snapshot,
        progress: &Path,
    ) -> Result<()> {
        ensure!(
            matches!(self.phase, Phase::FocusHold),
            "Movie focus checkpoint is not held"
        );
        ensure!(
            snapshot.commands == self.commands_before
                && std::fs::read(progress).context("Movie held progress comparison")?
                    == self.disk_before,
            "Movie focus hold changed source commands/progress"
        );
        ensure!(
            snapshot.buttons.held == 0
                && snapshot.buttons.pressed == 0
                && snapshot.buttons.repeat == 0
                && snapshot.buttons.rapid_repeat == 0,
            "Movie focus resumed before a neutral production input sample"
        );
        require_no_child(snapshot)?;
        let viewer = self.viewer(snapshot, 0)?;
        ensure!(
            viewer_ready(viewer),
            "Movie focus hold lost its actual ready Viewer: {viewer:?}"
        );
        let now = viewer
            .time_micros
            .context("Actual resumed Movie time missing")?;
        ensure!(
            now > self.focus_time,
            "Movie focus hold resumed without media-time progression"
        );
        self.playback_start = now;
        self.report.focus_checkpoint_resumed = true;
        self.phase = Phase::Playback(0);
        Ok(())
    }
    pub(super) fn poll(&mut self, snapshot: &Snapshot, progress: &Path) -> Result<Step> {
        self.report.polls += 1;
        ensure!(
            snapshot.commands == self.commands_before,
            "Movie journey advanced source VM at {:?}: {} -> {}",
            self.phase,
            self.commands_before,
            snapshot.commands
        );
        ensure!(
            std::fs::read(progress).context("Movie progress comparison")? == self.disk_before,
            "Movie journey wrote progress/settings at {:?}",
            self.phase
        );
        if let Some(movie) = &snapshot.movie {
            ensure!(
                snapshot.menu.is_none()
                    && snapshot.category.is_none()
                    && snapshot.title_initial.is_none()
                    && snapshot.top_selected.is_none()
                    && snapshot.cg.is_none()
                    && snapshot.music.is_none(),
                "Movie retained an active source scene at {:?}",
                self.phase
            );
            require_no_child(snapshot)?;
            require_row(movie, movie.selected)?;
            ensure!(
                movie.scroll_bits == 0f32.to_bits(),
                "Actual twelve-row Movie grid scrolled outside its720-height document: {movie:?}"
            );
            if let Some(viewer) = &movie.viewer {
                ensure!(
                    viewer.opacity.is_finite()
                        && viewer.backdrop.is_finite()
                        && (0. ..=1.).contains(&viewer.opacity)
                        && (0. ..=1.).contains(&viewer.backdrop)
                        && movie.scope.viewers == 1
                        && movie.scope.media_owners == 1,
                    "Movie Viewer observations are invalid: {movie:?}"
                );
            }
        }
        let mut raw = RawInputState::new();
        let mut focus_checkpoint = false;
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
                    self.phase = Phase::SelectMovie;
                }
            }
            Phase::SelectMovie => {
                require_category(snapshot, 5)?;
                raw = key(KeyCode::ArrowDown);
                self.phase = Phase::MovieSelected;
            }
            Phase::MovieSelected => {
                // Gallery Category is vertical: two fresh Down samples5->6->7.
                require_category(snapshot, 6)?;
                self.phase = Phase::OpenMovie;
            }
            Phase::OpenMovie => {
                if category_ready(snapshot, 6) {
                    raw = controller_button(GamepadButton::Down);
                } else {
                    require_category(snapshot, 7)?;
                    self.report.category_movie_selected = true;
                    raw = controller_button(GamepadButton::A);
                    self.phase = Phase::AwaitMovie;
                }
            }
            Phase::AwaitMovie => {
                if let Some(movie) = snapshot.movie.as_ref().filter(|m| m.grid_ready()) {
                    require_row(movie, 0)?;
                    ensure!(
                        movie.cues == [0; 4],
                        "Fresh Movie retained old cues: {movie:?}"
                    );
                    self.expected_cues = movie.cues;
                    self.selected = 0;
                    self.viewer_id = 0;
                    self.command_index = 0;
                    self.report.ready_visits += 1;
                    self.report.first_prunes_observed += 1;
                    ensure!(
                        self.report.ready_visits <= 2,
                        "Unexpected third Movie visit"
                    );
                    self.phase = Phase::GridRelease;
                }
            }
            Phase::GridRelease | Phase::CommandReleased | Phase::GridReturnReleased => {
                self.grid(snapshot, self.selected)?;
                let command = command(self.report.ready_visits, self.command_index);
                match command {
                    Command::Navigate(index) => raw = navigation(index).0,
                    Command::IgnoredShoulder(index) => raw = shoulder(index),
                    Command::LockedConfirm => raw = key(KeyCode::Enter),
                    Command::Viewer(index) => {
                        let plan = viewer_plan(index);
                        ensure!(
                            self.selected == plan.cell,
                            "Movie Viewer plan selected mismatch"
                        );
                        raw = plan.open.enter();
                    }
                    Command::Close(binding) => {
                        raw = binding.cancel();
                        self.phase = Phase::ParentCloseApplied(binding);
                    }
                }
                if !matches!(command, Command::Close(_)) {
                    self.phase = Phase::CommandApplied(command);
                }
            }
            Phase::CommandApplied(command) => {
                match command {
                    Command::Navigate(index) => {
                        let (_, selected) = navigation(index);
                        self.expected_cues = increment(self.expected_cues, 0)?;
                        self.selected = selected;
                        let movie = snapshot
                            .movie
                            .as_ref()
                            .context("Movie navigation owner missing")?;
                        require_row(movie, selected)?;
                        self.require_cues(movie)?;
                        ensure!(
                            movie.viewer.is_none() && !movie.closing,
                            "Movie navigation opened a child"
                        );
                        self.report.movement_cues += 1;
                        match index {
                            0 | 4 | 5 | 8 => self.report.keyboard_arrows += 1,
                            1 | 2 | 7 | 9 => self.report.controller_dpad += 1,
                            3 | 6 => self.report.first_stick_directions += 1,
                            _ => unreachable!("Bounded Movie navigation"),
                        }
                    }
                    Command::IgnoredShoulder(index) => {
                        self.grid(snapshot, self.selected)?;
                        match index {
                            0 | 1 => self.report.ignored_keyboard_shoulders += 1,
                            2 | 3 => self.report.ignored_controller_shoulders += 1,
                            _ => unreachable!("Bounded ignored shoulders"),
                        }
                    }
                    Command::LockedConfirm => {
                        self.expected_cues = increment(self.expected_cues, 3)?;
                        self.grid(snapshot, 3)?;
                        self.report.locked_error = true;
                    }
                    Command::Viewer(index) => {
                        self.expected_cues = increment(self.expected_cues, 1)?;
                        let plan = viewer_plan(index);
                        let movie = snapshot
                            .movie
                            .as_ref()
                            .context("Movie open owner missing")?;
                        require_row(movie, plan.cell)?;
                        self.require_cues(movie)?;
                        let viewer = movie
                            .viewer
                            .context("Actual Movie A did not append its Viewer")?;
                        ensure!(
                            viewer.movie_id == plan.movie_id && !viewer.closing,
                            "Movie A opened a different/closing Viewer: {viewer:?}"
                        );
                        self.viewer_id = viewer.id;
                        match plan.open {
                            Binding::Keyboard => self.report.keyboard_open = true,
                            Binding::Controller => self.report.controller_opens += 1,
                        }
                        self.phase = Phase::AwaitViewer(index);
                    }
                    Command::Close(_) => unreachable!("Separate Movie close phase"),
                }
                if !matches!(command, Command::Viewer(_)) {
                    self.phase = Phase::CommandSettling(command);
                }
            }
            Phase::CommandSettling(command) => {
                let movie = snapshot
                    .movie
                    .as_ref()
                    .context("Movie neutral owner missing")?;
                self.require_cues(movie)?;
                if movie.grid_ready() {
                    self.grid(snapshot, self.selected)?;
                    if matches!(command, Command::Navigate(_)) {
                        self.report.neutral_settled_moves += 1;
                    }
                    self.command_index += 1;
                    self.phase = Phase::CommandReleased;
                }
            }
            Phase::AwaitViewer(index) => {
                let viewer = self.viewer(snapshot, index)?;
                ensure!(
                    !viewer.closing,
                    "Movie ended before actual ready frame: {viewer:?}"
                );
                if viewer_ready(viewer) {
                    self.playback_start =
                        viewer.time_micros.context("Actual Movie time missing")?;
                    self.report.authored_movie_ids.push(viewer.movie_id);
                    self.report.literal_play_status.push(viewer.status);
                    self.report
                        .retained_extents
                        .push(viewer.retained_frame.unwrap());
                    self.phase = Phase::ViewerRelease(index);
                }
            }
            Phase::ViewerRelease(index) => {
                let viewer = self.viewer(snapshot, index)?;
                ensure!(
                    viewer_ready(viewer),
                    "Movie lost ready state during neutral release: {viewer:?}"
                );
                ensure!(
                    snapshot.buttons.held == 0
                        && snapshot.buttons.pressed == 0
                        && snapshot.buttons.repeat == 0
                        && snapshot.buttons.rapid_repeat == 0,
                    "Movie Viewer opening input was not released"
                );
                if index == 0 {
                    ensure!(
                        !self.report.focus_checkpoint_requested,
                        "Repeated Movie focus checkpoint"
                    );
                    self.focus_time = viewer
                        .time_micros
                        .context("Actual focus Movie time missing")?;
                    self.report.focus_checkpoint_requested = true;
                    self.phase = Phase::FocusHold;
                    focus_checkpoint = true;
                } else {
                    self.phase = Phase::Playback(index);
                }
            }
            Phase::FocusHold => {
                // Parent normally suppresses poll while it owns this hold.
                // A neutral callback cannot advance the child to cancellation.
                let viewer = self.viewer(snapshot, 0)?;
                ensure!(
                    viewer_ready(viewer),
                    "Movie focus hold lost ready Viewer: {viewer:?}"
                );
            }
            Phase::Playback(index) => {
                let viewer = self.viewer(snapshot, index)?;
                ensure!(
                    viewer_ready(viewer),
                    "Movie stopped before bounded natural playback: {viewer:?}"
                );
                let now = viewer.time_micros.context("Actual Movie time missing")?;
                ensure!(
                    now >= self.playback_start,
                    "Movie media clock regressed before close"
                );
                if now >= self.playback_start.saturating_add(100_000) {
                    self.report
                        .observed_time_pairs
                        .push((self.playback_start, now));
                    raw = viewer_plan(index).close.raw();
                    self.phase = Phase::ViewerCloseApplied(index);
                }
            }
            Phase::ViewerCloseApplied(index) => {
                self.expected_cues = increment(self.expected_cues, 2)?;
                let viewer = self.viewer(snapshot, index)?;
                ensure!(
                    viewer.closing
                        && !viewer.private_input_ready
                        && viewer.retained_frame == Some((1920, 1080)),
                    "Movie A/B skipped retained Viewer close: {viewer:?}"
                );
                match viewer_plan(index).close {
                    ViewerClose::ControllerA => self.report.viewer_controller_a_close = true,
                    ViewerClose::ControllerB => self.report.viewer_controller_b_close = true,
                    ViewerClose::KeyboardEscape => self.report.viewer_keyboard_escape_close = true,
                }
                self.phase = Phase::AwaitGridReturn(index);
            }
            Phase::AwaitGridReturn(index) => {
                let movie = snapshot
                    .movie
                    .as_ref()
                    .context("Movie source grid retired with Viewer")?;
                require_row(movie, viewer_plan(index).cell)?;
                self.require_cues(movie)?;
                if movie.viewer.is_some() {
                    ensure!(
                        movie.viewer.unwrap().closing,
                        "Movie Viewer close was undone"
                    );
                } else if movie.grid_ready() {
                    self.grid(snapshot, self.selected)?;
                    self.report.same_selected_grid_returns += 1;
                    self.report.media_prunes_observed += 1;
                    self.command_index += 1;
                    self.phase = Phase::GridReturnReleased;
                }
            }
            Phase::ParentCloseApplied(binding) => {
                self.expected_cues = increment(self.expected_cues, 2)?;
                let movie = snapshot
                    .movie
                    .as_ref()
                    .context("Movie parent skipped retained close")?;
                require_row(movie, self.selected)?;
                self.require_cues(movie)?;
                ensure!(
                    movie.closing
                        && !movie.input
                        && movie.atlas
                        && !movie.mask
                        && movie.first_complete
                        && movie.viewer.is_none()
                        && movie.scope.media_owners == 0,
                    "Movie parent B did not enter retained Close: {movie:?}"
                );
                match binding {
                    Binding::Keyboard => self.report.keyboard_parent_close = true,
                    Binding::Controller => self.report.controller_parent_close = true,
                }
                self.phase = Phase::AwaitCategoryReturn;
            }
            Phase::AwaitCategoryReturn => {
                if let Some(movie) = &snapshot.movie {
                    ensure!(movie.closing, "Movie parent lost closing state");
                    self.require_cues(movie)?;
                } else if category_ready(snapshot, 7) {
                    require_category(snapshot, 7)?;
                    ensure!(
                        snapshot.title_initial == Some((17, self.title_mode)),
                        "Movie close did not construct fresh Title17/remembered mode: {:?}",
                        snapshot.title_initial
                    );
                    self.report.fresh_title_seventeen += 1;
                    self.phase = Phase::CategoryReturnReleased;
                }
            }
            Phase::CategoryReturnReleased => {
                require_category(snapshot, 7)?;
                ensure!(
                    snapshot.title_initial == Some((17, self.title_mode)),
                    "Movie return Title changed"
                );
                if self.report.ready_visits == 1 {
                    raw = key(KeyCode::Enter);
                    self.phase = Phase::AwaitMovie;
                } else {
                    raw = controller_button(GamepadButton::B);
                    self.phase = Phase::AwaitTopReturn;
                }
            }
            Phase::AwaitTopReturn => {
                if top_ready(snapshot) {
                    require_top(snapshot)?;
                    ensure!(
                        snapshot.title_initial == Some((17, self.title_mode)),
                        "Movie Category B replaced fresh Title17"
                    );
                    self.report.category_back_top_five = true;
                    self.report.progress_unchanged = true;
                    self.report.commands_frozen = true;
                    ensure!(
                        self.report.complete(),
                        "Movie window report incomplete: {:?}",
                        self.report
                    );
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => require_top(snapshot)?,
        }
        Ok(Step {
            raw,
            done: matches!(self.phase, Phase::Done),
            focus_checkpoint,
        })
    }
    fn require_cues(&self, movie: &State) -> Result<()> {
        ensure!(
            movie.cues == self.expected_cues,
            "Unexpected Movie cue at {:?}: {:?}, expected {:?}",
            self.phase,
            movie.cues,
            self.expected_cues
        );
        Ok(())
    }
    fn grid<'a>(&self, snapshot: &'a Snapshot, selected: usize) -> Result<&'a State> {
        let movie = snapshot
            .movie
            .as_ref()
            .context("Actual Movie owner missing")?;
        require_row(movie, selected)?;
        self.require_cues(movie)?;
        ensure!(
            movie.grid_ready(),
            "Actual Movie grid is not ready at {:?}: {movie:?}",
            self.phase
        );
        Ok(movie)
    }
    fn viewer<'a>(&self, snapshot: &'a Snapshot, index: usize) -> Result<&'a ReferenceViewerState> {
        let movie = snapshot
            .movie
            .as_ref()
            .context("Actual Movie Viewer owner missing")?;
        let plan = viewer_plan(index);
        require_row(movie, plan.cell)?;
        self.require_cues(movie)?;
        let viewer = movie
            .viewer
            .as_ref()
            .context("Actual retained Movie Viewer missing")?;
        ensure!(
            viewer.id == self.viewer_id
                && viewer.movie_id == plan.movie_id
                && movie.atlas
                && !movie.mask
                && movie.first_complete
                && !movie.closing
                && movie.scope.viewers == 1
                && movie.scope.media_owners == 1
                && movie.scope.borrowed_viewers == 1,
            "Movie Viewer identity/source changed: {movie:?}"
        );
        Ok(viewer)
    }
}

fn viewer_ready(viewer: &ReferenceViewerState) -> bool {
    !viewer.closing
        && viewer.private_input_ready
        && viewer.status == 1
        && viewer.playing
        && viewer.opacity == 1.
        && viewer.backdrop == 1.
        && viewer.retained_frame == Some((1920, 1080))
        && viewer.time_micros.is_some()
}
struct ViewerPlan {
    cell: usize,
    movie_id: u16,
    open: Binding,
    close: ViewerClose,
}
fn viewer_plan(index: usize) -> ViewerPlan {
    match index {
        0 => ViewerPlan {
            cell: 1,
            movie_id: 6,
            open: Binding::Controller,
            close: ViewerClose::ControllerA,
        },
        1 => ViewerPlan {
            cell: 0,
            movie_id: 9,
            open: Binding::Keyboard,
            close: ViewerClose::ControllerB,
        },
        2 => ViewerPlan {
            cell: 2,
            movie_id: 0,
            open: Binding::Controller,
            close: ViewerClose::KeyboardEscape,
        },
        _ => unreachable!("Bounded three-Movie journey"),
    }
}
fn command(visit: u32, index: usize) -> Command {
    match (visit, index) {
        (1, 0..=4) => Command::Navigate(index),
        (1, 5..=8) => Command::IgnoredShoulder(index - 5),
        (1, 9) => Command::LockedConfirm,
        (1, 10..=11) => Command::Navigate(index - 5),
        (1, 12) => Command::Viewer(0),
        (1, 13) => Command::Navigate(7),
        (1, 14) => Command::Viewer(1),
        (1, 15) => Command::Close(Binding::Keyboard),
        (2, 0..=1) => Command::Navigate(index + 8),
        (2, 2) => Command::Viewer(2),
        (2, 3) => Command::Close(Binding::Controller),
        _ => unreachable!("Bounded two-visit Movie sequence"),
    }
}
fn navigation(index: usize) -> (RawInputState, usize) {
    match index {
        0 => (key(KeyCode::ArrowRight), 1),
        1 => (controller_button(GamepadButton::Down), 5),
        2 => (controller_button(GamepadButton::Up), 1),
        3 => (analog(1.), 2),
        4 => (key(KeyCode::ArrowRight), 3),
        5 => (key(KeyCode::ArrowLeft), 2),
        6 => (analog(-1.), 1),
        7 => (controller_button(GamepadButton::Left), 0),
        8 => (key(KeyCode::ArrowRight), 1),
        9 => (controller_button(GamepadButton::Right), 2),
        _ => unreachable!("Bounded Movie grid navigation"),
    }
}
fn analog(x: f32) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = x;
    // Probe::raw_input replaces the already produced RawInputState. Supply the
    // corresponding real producer threshold result as well as its axis value;
    // a bare axis here would skip RawInputManager's virtual-key update.
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLRight] = x >= 0.7;
    raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLLeft] = x <= -0.7;
    raw
}
fn shoulder(index: usize) -> RawInputState {
    match index {
        0 => key(KeyCode::KeyH),
        1 => key(KeyCode::KeyJ),
        2 => controller_button(GamepadButton::L),
        3 => controller_button(GamepadButton::R),
        _ => unreachable!("Bounded ignored Movie shoulders"),
    }
}
fn increment(mut cues: [u64; 4], index: usize) -> Result<[u64; 4]> {
    cues[index] = cues[index]
        .checked_add(1)
        .context("Movie cue counter overflow")?;
    Ok(cues)
}
fn require_row(movie: &State, selected: usize) -> Result<()> {
    // Actual fixed roster. Only bits9/6/0 are granted in the temporary fixture,
    // before startup. This driver never changes resident unlocks or a user save.
    const MOVIES: [u16; 12] = [9, 6, 0, 1, 7, 8, 2, 3, 4, 5, 10, 11];
    let expected = *MOVIES
        .get(selected)
        .context("Movie fixture selected outside12 rows")?;
    ensure!(
        movie.rows == MOVIES.len()
            && movie.selected == selected
            && movie.selected_movie == expected
            && movie.selected_locked == (selected >= 3),
        "Actual Movie row{selected} metadata/unlock differs: {movie:?}"
    );
    Ok(())
}
fn category_ready(snapshot: &Snapshot, selected: i32) -> bool {
    snapshot.movie.is_none()
        && snapshot.music.is_none()
        && snapshot.cg.is_none()
        && snapshot.menu == Some(("cat", 2))
        && snapshot.category == Some((1, selected, 2))
}
fn require_category(snapshot: &Snapshot, selected: i32) -> Result<()> {
    ensure!(
        category_ready(snapshot, selected),
        "Actual Gallery Category1/row{selected} is not ready"
    );
    require_no_child(snapshot)
}
fn top_ready(snapshot: &Snapshot) -> bool {
    snapshot.movie.is_none()
        && snapshot.music.is_none()
        && snapshot.cg.is_none()
        && snapshot.category.is_none()
        && snapshot.menu == Some(("top", 2))
        && snapshot.top_selected == Some(5)
}
fn require_top(snapshot: &Snapshot) -> Result<()> {
    ensure!(top_ready(snapshot), "Movie source is not actual ready Top5");
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
        "Movie journey has an unrelated modal/destination child"
    );
    Ok(())
}
