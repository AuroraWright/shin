//! Real-window CG journey. The parent supplies read-only
//! production-owner snapshots. No gallery/Viewer is constructed by the probe.
use super::{Snapshot, controller_button, key};
use anyhow::{Context, Result, ensure};
use shin_input::{
    RawInputState,
    inputs::{GamepadAxis, GamepadButton},
};
use std::path::Path;
use winit::keyboard::KeyCode;

/// Read directly from the resident CG Opening/Owner after its normal update.
/// Tokens are process-local retained-resource continuity checks for this one
/// journey; they are not native addresses or global native identity evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) struct State {
    pub atlas: bool,
    pub atlas_identity: u64,
    pub first_complete: bool,
    pub input: bool,
    pub closing: bool,
    /// Parent poll counter, included in render-getter purity comparisons.
    pub frame: u32,
    pub page: usize,
    pub pages: usize,
    pub selected: usize,
    pub pictures: Vec<u16>,
    pub unlocked: Vec<bool>,
    pub viewer: Option<ViewerState>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) struct ViewerState {
    pub variant: i32,
    pub closing: bool,
    pub show_info: bool,
    pub fit: bool,
    /// Actual Owner::reference_input_ready(): live art, notclosing, no capture
    /// wait and only the live Input task in the private scope.
    pub input_ready: bool,
    pub capture_busy: bool,
    /// Actual elapsed/duration float bits for render-getter purity checks.
    pub capture_time: [u32; 2],
    pub picture_identity: Option<u64>,
    pub metadata: Option<([i16; 2], [u16; 2])>,
    pub offset: [u32; 2],
    /// Current native properties0,1,12,13, without advancing their tweens.
    pub properties: Option<[u32; 4]>,
    pub properties_busy: bool,
}
impl ViewerState {
    fn ready(&self) -> bool {
        self.input_ready
            && !self.closing
            && !self.capture_busy
            && !self.properties_busy
            && self.picture_identity.is_some()
            && self.properties.is_some()
    }
}
#[derive(Clone, Copy, Debug)]
enum Pan {
    Analog,
    Dpad,
    Keyboard,
}
impl Pan {
    fn raw(self) -> RawInputState {
        match self {
            Self::Analog => {
                let mut raw = RawInputState::new();
                // RawInputState Y is up-positive; Input::analog negates it
                // before the native down-positive byte conversion.
                raw.gamepads.unified.axes[GamepadAxis::LeftStickY] = 0.25;
                raw
            }
            Self::Dpad => controller_button(GamepadButton::Up),
            Self::Keyboard => key(KeyCode::ArrowUp),
        }
    }
    fn delta(self) -> f32 {
        match self {
            // Raw Y+.25 becomes native(-.25*127)=-31, then signed /10.
            Self::Analog => -3.,
            Self::Dpad | Self::Keyboard => -16.,
        }
    }
    fn next(self) -> Option<Self> {
        match self {
            Self::Analog => Some(Self::Dpad),
            Self::Dpad => Some(Self::Keyboard),
            Self::Keyboard => None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Phase {
    EntryRelease,
    OpenCategory,
    AwaitCategory,
    OpenGrid,
    AwaitGrid,
    GridRelease,
    NavigateApplied(usize),
    NavigateReleased(usize),
    AwaitViewer,
    ViewerRelease,
    PanApplied(Pan),
    PanReleased(Pan),
    InfoApplied(bool),
    InfoReleased(bool),
    AwaitFit(bool),
    FitReleased(bool),
    VariantApplied,
    AwaitVariant,
    VariantReleased,
    ViewerCloseApplied,
    AwaitGridReturn,
    GridReturnReleased,
    GridCloseApplied,
    AwaitCategoryReturn,
    CategoryReturnReleased,
    AwaitTopReturn,
    Done,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Report {
    pub polls: u64,
    pub category_default_cg: bool,
    pub grid_ready: bool,
    pub authored_tall_picture: bool,
    pub decoded_variants: u32,
    pub analog_delta_minus_three: bool,
    pub physical_dpad_delta_minus_sixteen: bool,
    pub keyboard_delta_minus_sixteen: bool,
    pub neutral_pan_samples: u32,
    pub info_toggles: u32,
    pub settled_fit_toggles: u32,
    pub viewer_close_observed: bool,
    pub same_grid_returned: bool,
    pub grid_close_observed: bool,
    pub fresh_title_fifteen: bool,
    pub category_back_top_five: bool,
    pub progress_unchanged: bool,
    pub commands_frozen: bool,
}
impl Report {
    pub(super) fn complete(&self) -> bool {
        self.category_default_cg
            && self.grid_ready
            && self.authored_tall_picture
            && self.decoded_variants == 2
            && self.analog_delta_minus_three
            && self.physical_dpad_delta_minus_sixteen
            && self.keyboard_delta_minus_sixteen
            && self.neutral_pan_samples == 3
            && self.info_toggles == 2
            && self.settled_fit_toggles == 2
            && self.viewer_close_observed
            && self.same_grid_returned
            && self.grid_close_observed
            && self.fresh_title_fifteen
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
    atlas_identity: u64,
    picture_identity: u64,
    offset: [u32; 2],
    report: Report,
}
impl Journey {
    /// Parent sends Top2->5 ArrowDown, verifies the real ready Top5, then
    /// constructs this driver. The first next sample is neutral.
    pub(super) fn new(snapshot: &Snapshot, progress: &Path) -> Result<Self> {
        require_top(snapshot, 5)?;
        let (_, title_mode) = snapshot
            .title_initial
            .context("CG source title initial state")?;
        Ok(Self {
            phase: Phase::EntryRelease,
            disk_before: std::fs::read(progress).context("CG progress baseline")?,
            commands_before: snapshot.commands,
            title_mode,
            atlas_identity: 0,
            picture_identity: 0,
            offset: [0f32.to_bits(); 2],
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
            "CG source VM command count changed at {:?}: {} -> {}",
            self.phase,
            self.commands_before,
            snapshot.commands
        );
        ensure!(
            std::fs::read(progress).context("CG progress comparison")? == self.disk_before,
            "CG journey wrote progress/settings at {:?}",
            self.phase
        );
        if snapshot.cg.is_some() {
            ensure!(
                snapshot.menu.is_none()
                    && snapshot.category.is_none()
                    && snapshot.title_initial.is_none()
                    && snapshot.top_selected.is_none(),
                "CG scene retained an active title source at {:?}",
                self.phase
            );
        }
        let mut raw = RawInputState::new();
        match self.phase {
            Phase::EntryRelease => {
                require_top(snapshot, 5)?;
                self.phase = Phase::OpenCategory;
            }
            Phase::OpenCategory => {
                require_top(snapshot, 5)?;
                raw = controller_button(GamepadButton::A);
                self.phase = Phase::AwaitCategory;
            }
            Phase::AwaitCategory => {
                if category_ready(snapshot) {
                    require_category(snapshot)?;
                    self.report.category_default_cg = true;
                    self.phase = Phase::OpenGrid;
                }
            }
            Phase::OpenGrid => {
                require_category(snapshot)?;
                raw = key(KeyCode::Enter);
                self.phase = Phase::AwaitGrid;
            }
            Phase::AwaitGrid => {
                if let Some(grid) = snapshot.cg.as_ref().filter(|g| grid_ready(g)) {
                    ensure!(
                        grid.page == 0 && grid.selected == 0 && grid.pages == 14,
                        "Native initial CG page/selection differs: {grid:?}"
                    );
                    ensure!(grid.viewer.is_none(), "CG inherited a Viewer");
                    ensure!(
                        grid.atlas_identity != 0,
                        "CG atlas continuity token missing"
                    );
                    self.atlas_identity = grid.atlas_identity;
                    self.report.grid_ready = true;
                    self.phase = Phase::GridRelease;
                }
            }
            Phase::GridRelease => {
                self.grid(snapshot, 0)?;
                raw = key(KeyCode::ArrowRight);
                self.phase = Phase::NavigateApplied(1);
            }
            Phase::NavigateApplied(selected) => {
                self.grid(snapshot, selected)?;
                self.phase = Phase::NavigateReleased(selected);
            }
            Phase::NavigateReleased(selected) => {
                let grid = self.grid(snapshot, selected)?;
                if selected < 3 {
                    raw = key(KeyCode::ArrowRight);
                    self.phase = Phase::NavigateApplied(selected + 1);
                } else {
                    ensure!(
                        grid.pictures == [7, 8] && grid.unlocked == [true, true],
                        "CG authored entry3 must contain unlocked IDs7/8: {grid:?}"
                    );
                    raw = controller_button(GamepadButton::A);
                    self.phase = Phase::AwaitViewer;
                }
            }
            Phase::AwaitViewer => {
                self.grid(snapshot, 3)?;
                if let Some(viewer) = snapshot
                    .cg
                    .as_ref()
                    .and_then(|g| g.viewer.as_ref())
                    .filter(|v| v.ready())
                {
                    require_picture(viewer, 0)?;
                    ensure!(
                        !viewer.fit && viewer.show_info && viewer.offset == [0f32.to_bits(); 2],
                        "CG Viewer initial flags/offset differ: {viewer:?}"
                    );
                    ensure_properties(viewer, [0., 0., 1000., 1000.])?;
                    self.picture_identity = viewer.picture_identity.unwrap();
                    self.offset = viewer.offset;
                    self.report.decoded_variants += 1;
                    self.report.authored_tall_picture = true;
                    self.phase = Phase::ViewerRelease;
                }
            }
            Phase::ViewerRelease => {
                self.original_picture(snapshot)?;
                raw = Pan::Analog.raw();
                self.phase = Phase::PanApplied(Pan::Analog);
            }
            Phase::PanApplied(pan) => {
                let viewer = self.original_picture(snapshot)?;
                let expected = [
                    self.offset[0],
                    (f32::from_bits(self.offset[1]) + pan.delta()).to_bits(),
                ];
                ensure!(
                    viewer.offset == expected,
                    "CG {:?} must pan by{} without clamping: {:?} -> {:?}",
                    pan,
                    pan.delta(),
                    self.offset,
                    viewer.offset
                );
                ensure_properties(viewer, [0., f32::from_bits(expected[1]), 1000., 1000.])?;
                self.offset = expected;
                match pan {
                    Pan::Analog => self.report.analog_delta_minus_three = true,
                    Pan::Dpad => self.report.physical_dpad_delta_minus_sixteen = true,
                    Pan::Keyboard => self.report.keyboard_delta_minus_sixteen = true,
                }
                self.phase = Phase::PanReleased(pan);
            }
            Phase::PanReleased(pan) => {
                let viewer = self.original_picture(snapshot)?;
                ensure!(
                    viewer.offset == self.offset,
                    "Neutral CG input repeated pan"
                );
                self.report.neutral_pan_samples += 1;
                if let Some(next) = pan.next() {
                    raw = next.raw();
                    self.phase = Phase::PanApplied(next);
                } else {
                    raw = key(KeyCode::KeyM);
                    self.phase = Phase::InfoApplied(false);
                }
            }
            Phase::InfoApplied(shown) => {
                let viewer = self.original_picture(snapshot)?;
                ensure!(
                    viewer.show_info == shown && viewer.offset == self.offset,
                    "CG info toggle changed the wrong state: {viewer:?}"
                );
                self.report.info_toggles += 1;
                self.phase = Phase::InfoReleased(shown);
            }
            Phase::InfoReleased(shown) => {
                ensure!(
                    self.original_picture(snapshot)?.show_info == shown,
                    "CG info repeated after release"
                );
                if shown {
                    raw = key(KeyCode::KeyB);
                    self.phase = Phase::AwaitFit(true);
                } else {
                    raw = controller_button(GamepadButton::Y);
                    self.phase = Phase::InfoApplied(true);
                }
            }
            Phase::AwaitFit(fit) => {
                let viewer = self.viewer(snapshot)?;
                require_picture(viewer, 0)?;
                ensure!(
                    viewer.fit == fit
                        && viewer.show_info
                        && viewer.offset == self.offset
                        && viewer.picture_identity == Some(self.picture_identity),
                    "CG fit changed live art, stored pan or badges: {viewer:?}"
                );
                if viewer.ready() {
                    let expected = if fit {
                        [0., 0., 500., 500.]
                    } else {
                        [0., f32::from_bits(self.offset[1]), 1000., 1000.]
                    };
                    ensure_properties(viewer, expected)?;
                    self.report.settled_fit_toggles += 1;
                    self.phase = Phase::FitReleased(fit);
                }
            }
            Phase::FitReleased(fit) => {
                let viewer = self.viewer(snapshot)?;
                ensure!(
                    viewer.ready() && viewer.fit == fit,
                    "CG fit did not remain settled after release"
                );
                if fit {
                    raw = controller_button(GamepadButton::X);
                    self.phase = Phase::AwaitFit(false);
                } else {
                    raw = key(KeyCode::Enter);
                    self.phase = Phase::VariantApplied;
                }
            }
            Phase::VariantApplied => {
                ensure!(
                    self.viewer(snapshot)?.variant == 1,
                    "Fresh A did not request authored CG variant1"
                );
                self.phase = Phase::AwaitVariant;
            }
            Phase::AwaitVariant => {
                let viewer = self.viewer(snapshot)?;
                if viewer.ready() {
                    require_picture(viewer, 1)?;
                    ensure!(
                        !viewer.fit && viewer.show_info && viewer.offset == self.offset,
                        "CG variant lost native fit/info/pan continuity: {viewer:?}"
                    );
                    ensure!(
                        viewer.picture_identity != Some(self.picture_identity),
                        "CG variant reused the previous live Picture owner"
                    );
                    ensure_properties(viewer, [0., f32::from_bits(self.offset[1]), 1000., 1000.])?;
                    self.picture_identity = viewer.picture_identity.unwrap();
                    self.report.decoded_variants += 1;
                    self.phase = Phase::VariantReleased;
                }
            }
            Phase::VariantReleased => {
                let viewer = self.viewer(snapshot)?;
                require_picture(viewer, 1)?;
                ensure!(
                    viewer.ready(),
                    "CG variant readiness lost after neutral sample"
                );
                raw = controller_button(GamepadButton::B);
                self.phase = Phase::ViewerCloseApplied;
            }
            Phase::ViewerCloseApplied => {
                let viewer = self.viewer(snapshot)?;
                ensure!(
                    viewer.closing
                        && !viewer.input_ready
                        && !viewer.show_info
                        && viewer.picture_identity.is_none()
                        && viewer.metadata.is_none()
                        && viewer.properties.is_none(),
                    "CG B skipped native retained close/capture state: {viewer:?}"
                );
                self.report.viewer_close_observed = true;
                self.phase = Phase::AwaitGridReturn;
            }
            Phase::AwaitGridReturn => {
                let grid = self.grid(snapshot, 3)?;
                if grid.viewer.is_none() && grid_ready(grid) {
                    self.report.same_grid_returned = true;
                    self.phase = Phase::GridReturnReleased;
                }
            }
            Phase::GridReturnReleased => {
                let grid = self.grid(snapshot, 3)?;
                ensure!(
                    grid.viewer.is_none() && grid_ready(grid),
                    "CG Viewer still owns input"
                );
                raw = key(KeyCode::Escape);
                self.phase = Phase::GridCloseApplied;
            }
            Phase::GridCloseApplied => {
                let grid = snapshot
                    .cg
                    .as_ref()
                    .context("CG grid retired before native close task")?;
                ensure!(
                    grid.closing && grid.viewer.is_none(),
                    "CG B missed native closing state"
                );
                self.report.grid_close_observed = true;
                self.phase = Phase::AwaitCategoryReturn;
            }
            Phase::AwaitCategoryReturn => {
                if category_ready(snapshot) {
                    require_category(snapshot)?;
                    ensure!(
                        snapshot.title_initial == Some((15, self.title_mode)),
                        "CG did not construct fresh Title15 while preserving selected mode: {:?}",
                        snapshot.title_initial
                    );
                    self.report.fresh_title_fifteen = true;
                    self.phase = Phase::CategoryReturnReleased;
                }
            }
            Phase::CategoryReturnReleased => {
                require_category(snapshot)?;
                raw = controller_button(GamepadButton::B);
                self.phase = Phase::AwaitTopReturn;
            }
            Phase::AwaitTopReturn => {
                if snapshot.cg.is_none()
                    && snapshot.menu == Some(("top", 2))
                    && snapshot.top_selected == Some(5)
                    && snapshot.category.is_none()
                {
                    require_top(snapshot, 5)?;
                    ensure!(
                        snapshot.title_initial == Some((15, self.title_mode)),
                        "Category return replaced the new Title unexpectedly"
                    );
                    self.report.category_back_top_five = true;
                    self.report.progress_unchanged = true;
                    self.report.commands_frozen = true;
                    ensure!(
                        self.report.complete(),
                        "CG window observation report incomplete"
                    );
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {
                require_top(snapshot, 5)?;
            }
        }
        Ok(Step {
            raw,
            done: matches!(self.phase, Phase::Done),
        })
    }
    fn grid<'a>(&self, snapshot: &'a Snapshot, selected: usize) -> Result<&'a State> {
        let grid = snapshot.cg.as_ref().context("CG resident scene missing")?;
        ensure!(
            grid.atlas
                && grid.first_complete
                && !grid.closing
                && grid.page == 0
                && grid.pages == 14
                && grid.selected == selected
                && grid.atlas_identity == self.atlas_identity,
            "CG retained grid changed at {:?}: {grid:?}",
            self.phase
        );
        if selected == 3 {
            ensure!(
                grid.pictures == [7, 8] && grid.unlocked == [true, true],
                "CG retained authored entry3 changed"
            );
        }
        Ok(grid)
    }
    fn viewer<'a>(&self, snapshot: &'a Snapshot) -> Result<&'a ViewerState> {
        self.grid(snapshot, 3)?
            .viewer
            .as_ref()
            .context("Actual retained CG Viewer missing")
    }
    fn original_picture<'a>(&self, snapshot: &'a Snapshot) -> Result<&'a ViewerState> {
        let viewer = self.viewer(snapshot)?;
        require_picture(viewer, 0)?;
        ensure!(
            viewer.ready() && !viewer.fit && viewer.picture_identity == Some(self.picture_identity),
            "CG original live Picture continuity/readiness changed: {viewer:?}"
        );
        Ok(viewer)
    }
}
fn grid_ready(grid: &State) -> bool {
    grid.atlas && grid.first_complete && grid.input && !grid.closing && grid.viewer.is_none()
}
fn require_picture(viewer: &ViewerState, variant: i32) -> Result<()> {
    ensure!(
        viewer.variant == variant
            && viewer
                .picture_identity
                .is_some_and(|identity| identity != 0)
            && viewer.metadata == Some(([960, 2160], [1920, 2160])),
        "CG authored Picture/metadata mismatch: {viewer:?}"
    );
    Ok(())
}
fn ensure_properties(viewer: &ViewerState, expected: [f32; 4]) -> Result<()> {
    ensure!(
        !viewer.properties_busy && viewer.properties == Some(expected.map(f32::to_bits)),
        "CG current transform is not the expected settled native state: {:?}, expected{expected:?}",
        viewer.properties
    );
    Ok(())
}
fn category_ready(snapshot: &Snapshot) -> bool {
    snapshot.cg.is_none()
        && snapshot.menu == Some(("cat", 2))
        && snapshot.category == Some((1, 5, 2))
}
fn require_category(snapshot: &Snapshot) -> Result<()> {
    ensure!(
        category_ready(snapshot),
        "Actual Gallery Category1/defaultCg5 is not ready"
    );
    ensure!(
        snapshot.config.is_none()
            && snapshot.manual.is_none()
            && snapshot.chart.is_none()
            && snapshot.picker.is_none(),
        "CG category has an unrelated modal child"
    );
    Ok(())
}
fn require_top(snapshot: &Snapshot, selected: i32) -> Result<()> {
    ensure!(
        snapshot.cg.is_none()
            && snapshot.category.is_none()
            && snapshot.menu == Some(("top", 2))
            && snapshot.top_selected == Some(selected),
        "CG source has not returned to actual ready Top{selected}"
    );
    ensure!(
        snapshot.config.is_none()
            && snapshot.manual.is_none()
            && snapshot.chart.is_none()
            && snapshot.picker.is_none(),
        "CG source has an unrelated modal child"
    );
    Ok(())
}
// The parent owns compilation and actual-window execution on exact Loopback Audio.
