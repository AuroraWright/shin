//! Chart journeys for the real-window probe.
//!
//! The parent supplies feature-only read-only snapshots and normal ShinApp
//! callbacks. This driver never constructs a Chart, edits a live owner, writes
//! progress, injects logical Buttons, or bypasses raw-input lowering.
use super::{Snapshot, controller_button, key};
use anyhow::{Context, Result, ensure};
use shin_input::{
    RawInputState,
    inputs::{GamepadAxis, GamepadButton},
};
use std::path::Path;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Title,
    Game,
}
impl Source {
    fn native(self) -> u32 {
        match self {
            Self::Title => 0,
            Self::Game => 1,
        }
    }
    fn entry(self) -> i32 {
        match self {
            Self::Title => 2,
            Self::Game => 3,
        }
    }
    fn destination(self) -> i32 {
        match self {
            Self::Title => 3,
            Self::Game => 4,
        }
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
                raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = 0.25;
                raw
            }
            Self::Dpad => controller_button(GamepadButton::Right),
            Self::Keyboard => key(KeyCode::ArrowRight),
        }
    }
    fn delta(self) -> f32 {
        match self {
            // Primary995c0 quantizes .25*127 to31; Chart4a4bc..4a4d0
            // divides the nonzero signed byte by10, truncating toward zero.
            Self::Analog => 3.,
            Self::Dpad | Self::Keyboard => 10.,
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
    Navigate,
    NavigationApplied,
    SelectionReleased,
    OpenApplied,
    AwaitReady,
    PanApplied(Pan),
    PanReleased(Pan),
    ShoulderApplied(usize),
    ShoulderReleased(usize),
    CloseApplied,
    AwaitReturn,
    Done,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Report {
    pub polls: u64,
    pub ready_observed: bool,
    pub analog_delta_three: bool,
    pub physical_dpad_delta_ten: bool,
    pub keyboard_arrow_delta_ten: bool,
    pub neutral_samples_stable: u32,
    pub shoulder_checks: u32,
    pub source_returned: bool,
    pub progress_unchanged: bool,
    pub commands_frozen: bool,
}
pub(super) struct Step {
    /// Next raw sample consumed by the parent's next production update.
    pub raw: RawInputState,
    /// True only after Chart is absent and its real source row is ready.
    pub done: bool,
}
pub(super) struct Journey {
    source: Source,
    phase: Phase,
    disk_before: Vec<u8>,
    commands_before: u64,
    group: i32,
    cursor: (u32, u32, u32),
    report: Report,
}
impl Journey {
    /// Start at ready Top2 after title Config, or ready SystemMenu Item3
    /// after in-game Manual. The first emitted sample is always neutral.
    pub(super) fn new(source: Source, snapshot: &Snapshot, progress: &Path) -> Result<Self> {
        ensure!(snapshot.chart.is_none(), "Chart journey inherited a child");
        require_source(source, snapshot, source.entry())?;
        Ok(Self {
            source,
            phase: Phase::EntryRelease,
            disk_before: std::fs::read(progress).context("Chart progress baseline")?,
            commands_before: snapshot.commands,
            group: -1,
            cursor: (0, 0, 0),
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
            "{:?} Chart/source menu advanced VM commands at {:?}: {} -> {}",
            self.source,
            self.phase,
            self.commands_before,
            snapshot.commands
        );
        ensure!(
            std::fs::read(progress).context("Chart progress comparison")? == self.disk_before,
            "{:?} Chart wrote progress/settings at {:?}",
            self.source,
            self.phase
        );
        if let Some((source, ..)) = snapshot.chart {
            ensure!(source == self.source.native(), "Chart source owner changed");
        }
        let mut raw = RawInputState::new();
        match self.phase {
            Phase::EntryRelease => {
                require_source(self.source, snapshot, self.source.entry())?;
                self.phase = Phase::Navigate;
            }
            Phase::Navigate => {
                require_source(self.source, snapshot, self.source.entry())?;
                raw = key(match self.source {
                    Source::Title => KeyCode::ArrowRight,
                    // Snapshot.system_menu contains an ITEM ID, not its row
                    // index. Ordinary list[0,1,2,4,3,5] puts4 above3.
                    Source::Game => KeyCode::ArrowUp,
                });
                self.phase = Phase::NavigationApplied;
            }
            Phase::NavigationApplied => {
                require_source(self.source, snapshot, self.source.destination())?;
                self.phase = Phase::SelectionReleased;
            }
            Phase::SelectionReleased => {
                require_source(self.source, snapshot, self.source.destination())?;
                raw = match self.source {
                    Source::Title => controller_button(GamepadButton::A),
                    Source::Game => key(KeyCode::Enter),
                };
                self.phase = Phase::OpenApplied;
            }
            Phase::OpenApplied => self.phase = Phase::AwaitReady,
            Phase::AwaitReady => {
                if let Some((source, true, true, true, progress, group, _)) = snapshot.chart {
                    ensure!(source == self.source.native(), "Wrong ready Chart source");
                    ensure!(progress == 1f32.to_bits(), "Chart input before fully open");
                    if self.source == Source::Title {
                        ensure!(group == 0, "Native title Chart did not start at group0");
                    }
                    self.group = group;
                    self.cursor = cursor(snapshot)?;
                    ensure_finite_cursor(self.cursor)?;
                    self.report.ready_observed = true;
                    raw = Pan::Analog.raw();
                    self.phase = Phase::PanApplied(Pan::Analog);
                }
            }
            Phase::PanApplied(pan) => {
                require_chart(self.source, snapshot, self.group)?;
                let expected = (
                    (f32::from_bits(self.cursor.0) + pan.delta()).to_bits(),
                    self.cursor.1,
                    self.cursor.2,
                );
                ensure!(
                    cursor(snapshot)? == expected,
                    "{:?} Chart {:?} cursor delta must be{} per update, independent of frame delta: {:?} -> {:?}",
                    self.source,
                    pan,
                    pan.delta(),
                    self.cursor,
                    cursor(snapshot)?
                );
                self.cursor = expected;
                match pan {
                    Pan::Analog => self.report.analog_delta_three = true,
                    Pan::Dpad => self.report.physical_dpad_delta_ten = true,
                    Pan::Keyboard => self.report.keyboard_arrow_delta_ten = true,
                }
                self.phase = Phase::PanReleased(pan);
            }
            Phase::PanReleased(pan) => {
                require_chart(self.source, snapshot, self.group)?;
                ensure!(
                    cursor(snapshot)? == self.cursor,
                    "Neutral sample repeated Chart pan"
                );
                self.report.neutral_samples_stable += 1;
                if let Some(next) = pan.next() {
                    raw = next.raw();
                    self.phase = Phase::PanApplied(next);
                } else {
                    raw = shoulder(0);
                    self.phase = Phase::ShoulderApplied(0);
                }
            }
            Phase::ShoulderApplied(index) => {
                let expected = self.shoulder_group(index);
                require_chart(self.source, snapshot, expected)?;
                if self.source == Source::Game || index % 2 == 1 {
                    ensure!(
                        cursor(snapshot)? == self.cursor,
                        "Chart lost retained group cursor"
                    );
                }
                self.report.shoulder_checks += 1;
                self.phase = Phase::ShoulderReleased(index);
            }
            Phase::ShoulderReleased(index) => {
                require_chart(self.source, snapshot, self.shoulder_group(index))?;
                if index < 3 {
                    raw = shoulder(index + 1);
                    self.phase = Phase::ShoulderApplied(index + 1);
                } else {
                    raw = match self.source {
                        Source::Title => controller_button(GamepadButton::B),
                        Source::Game => key(KeyCode::Escape),
                    };
                    self.phase = Phase::CloseApplied;
                }
            }
            Phase::CloseApplied => {
                // Native B appends CloseWait; it waits for the close flash
                // before Exit sets closing. Input can still be present here.
                self.phase = Phase::AwaitReturn;
            }
            Phase::AwaitReturn => {
                if snapshot.chart.is_none()
                    && source_ready(self.source, snapshot, self.source.destination())
                {
                    self.report.source_returned = true;
                    self.report.progress_unchanged = true;
                    self.report.commands_frozen = true;
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {}
        }
        Ok(Step {
            raw,
            done: matches!(self.phase, Phase::Done),
        })
    }
    fn shoulder_group(&self, index: usize) -> i32 {
        if self.source == Source::Title && index % 2 == 0 {
            4 // Native mask31 means L wraps0->4; next R returns4->0.
        } else {
            self.group // Non-title Chart ignores both group-changing buttons.
        }
    }
}
fn shoulder(index: usize) -> RawInputState {
    match index {
        0 => key(KeyCode::KeyH),
        1 => key(KeyCode::KeyJ),
        2 => controller_button(GamepadButton::L),
        3 => controller_button(GamepadButton::R),
        _ => unreachable!("Four literal keyboard/controller shoulder checks"),
    }
}
fn source_ready(source: Source, s: &Snapshot, selection: i32) -> bool {
    match source {
        Source::Title => s.menu == Some(("top", 2)) && s.top_selected == Some(selection),
        Source::Game => s.system_menu == Some((selection as u32, true)),
    }
}
fn require_source(source: Source, s: &Snapshot, selection: i32) -> Result<()> {
    ensure!(s.chart.is_none(), "Chart still owns the source input scope");
    ensure!(
        source_ready(source, s, selection),
        "{:?} source row{} not ready",
        source,
        selection
    );
    Ok(())
}
fn require_chart(source: Source, s: &Snapshot, group: i32) -> Result<()> {
    ensure!(
        s.chart.is_some_and(
            |(owner, atlas, first_done, input, progress, actual_group, _)| {
                owner == source.native()
                    && atlas
                    && first_done
                    && input
                    && progress == 1f32.to_bits()
                    && actual_group == group
            }
        ),
        "{:?} Chart not actually ready in group{}: {:?}",
        source,
        group,
        s.chart
    );
    Ok(())
}
fn cursor(s: &Snapshot) -> Result<(u32, u32, u32)> {
    s.chart_cursor
        .context("Live Chart lost its read-only cursor snapshot")
}
fn ensure_finite_cursor(cursor: (u32, u32, u32)) -> Result<()> {
    ensure!(
        [cursor.0, cursor.1, cursor.2]
            .into_iter()
            .all(|b| f32::from_bits(b).is_finite()),
        "Chart cursor contains a nonfinite native value"
    );
    Ok(())
}
