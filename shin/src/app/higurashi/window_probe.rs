//! Main-thread, real-window acceptance with synthetic canonical raw samples.
//! All game callbacks delegate to the production HigurashiApp. Only input
//! samples, a temporary companion focus window, and progress paths are owned
//! by this driver; no game destination or loader is substituted.
use super::{
    load_game::{Paths, Storage},
    window_app::HigurashiApp,
};
use crate::{adv::higurashi::input::Buttons, cli::Cli};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use enum_map::EnumMap;
use glam::Vec2;
use parking_lot::Mutex;
use shin_core::format::save::higurashi::SaveData;
use shin_input::{
    ActionState, DummyAction, RawInputState,
    inputs::{GamepadButton, MouseButton},
};
use shin_render::render_pass::RenderPass;
use shin_window::{AppContext, RenderContext, ShinApp};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use winit::{dpi::PhysicalSize, keyboard::KeyCode, window::Window};

pub(super) mod cg_gallery;
mod chart;
pub(super) mod movie_gallery;
pub(super) mod music_gallery;

#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    pub frames: u64,
    pub clock: Option<f64>,
    pub notice: Option<(i32, Vec2)>,
    pub demo_ready: bool,
    pub menu: Option<(&'static str, i32)>,
    pub top_selected: Option<i32>,
    pub config: Option<(u32, usize, bool, u8)>,
    pub config_question: Option<(i32, i32)>,
    pub manual: Option<(usize, i32, bool, u32, i32)>,
    pub chart: Option<(u32, bool, bool, bool, u32, i32, i32)>,
    pub chart_cursor: Option<(u32, u32, u32)>,
    pub cg: Option<cg_gallery::State>,
    pub music: Option<music_gallery::State>,
    pub movie: Option<movie_gallery::State>,
    pub category: Option<(u32, i32, i32)>,
    pub title_initial: Option<(i32, u32)>,
    pub picker: Option<(u32, i32, bool, i32)>,
    pub picker_close_idle: Option<bool>,
    pub tips: bool,
    pub system_menu: Option<(u32, bool)>,
    pub input_diagnostics: Option<String>,
    pub commands: u64,
    pub message: bool,
    pub buttons: Buttons,
    pub closing: bool,
    pub frozen: bool,
    pub exited: bool,
}
#[derive(Clone, Copy, Debug)]
enum Stage {
    AwaitNotice,
    PressSmall,
    ResizeHeld,
    ReleaseResized,
    FreshPress,
    FreshRelease,
    AwaitDemo,
    DemoPressed,
    AwaitTop,
    TopConfigDown,
    TopConfigReleased,
    AwaitTitleConfig,
    TitleConfigEdited,
    TitleResetNoPressed,
    AwaitResetNo,
    ResetNoAnswered,
    AwaitResetNoReturn,
    TitleResetYesPressed,
    AwaitResetYes,
    ResetYesSelected,
    ResetYesAnswered,
    AwaitResetYesReturn,
    TitleConfigAdjusted,
    TitleConfigRelease,
    AwaitTitleConfigReturn,
    Chart(chart::Source),
    ChartTitleLeft,
    ChartTitleReleased,
    AwaitCgTopFive,
    Cg,
    Music,
    Movie,
    MovieFocusWait,
    MovieFocusRestoring,
    MovieFocusResuming,
    TitleManualDown,
    TitleManualReleased,
    AwaitManualOpen(ManualSource),
    ManualForwardPressed(ManualSource),
    AwaitManualForward(ManualSource),
    ManualBackwardPressed(ManualSource),
    AwaitManualBackward(ManualSource),
    ManualDragPressed(ManualSource),
    ManualDragMoved(ManualSource),
    AwaitManualSwipe(ManualSource),
    ManualCloseHeld(ManualSource),
    ManualResizeHeld(ManualSource),
    ManualResizedRelease(ManualSource),
    ManualFreshPress(ManualSource),
    AwaitManualReturn(ManualSource),
    OriginTopSelected,
    OriginTopReleased,
    AwaitOrigin,
    OriginDownPressed,
    OriginDownReleased,
    OriginUpPressed,
    OriginUpReleased,
    OriginTipsDownPressed,
    OriginTipsDownReleased,
    OriginTipsPressed,
    AwaitOriginTips,
    OriginTipsBackPressed,
    AwaitOriginTipsReturn(u8),
    OriginCloseHeld,
    OriginResizeHeld,
    OriginResizeReleased,
    OriginCancelPressed,
    AwaitOriginTitleReturn,
    TitleStoryUp,
    TitleStoryReleased,
    TopPressed,
    AwaitCat,
    CatPressed,
    AwaitMessage,
    MessageSettled(u8),
    SystemMenuPressed,
    AwaitSystemMenu,
    SystemConfigDown,
    SystemConfigReleased,
    SystemConfigPressed,
    AwaitGameConfig,
    GameConfigEdited,
    GameConfigRelease,
    AwaitGameConfigReturn,
    GameManualSelect,
    GameManualSelectRelease,
    GameManualPressed,
    SystemMenuClosed,
    AwaitGameplay,
    FocusHeld,
    FocusWait,
    FocusRestoring,
    FocusRelease,
    CloseQueued,
    Closing,
    Done,
}
#[derive(Clone, Copy, Debug)]
enum ManualSource {
    Title,
    Game,
}
#[derive(Default, Debug)]
struct Report {
    updates: u64,
    raw_samples: u64,
    renders: u64,
    focus_losses: u64,
    suspend_calls: u64,
    resume_calls: u64,
    close_calls: u64,
    max_commands: u64,
    initial_focus: bool,
    resize_cancelled: bool,
    fresh_pointer_answered: bool,
    reset_no_preserved: bool,
    reset_yes_applied: bool,
    title_config_saved: bool,
    game_config_saved: bool,
    config_source_commands_frozen: bool,
    title_manual_returned: bool,
    game_manual_returned: bool,
    title_chart: Option<chart::Report>,
    game_chart: Option<chart::Report>,
    cg: Option<cg_gallery::Report>,
    music: Option<music_gallery::Report>,
    movie: Option<movie_gallery::Report>,
    movie_focus_callbacks: bool,
    movie_focus_clock_frozen: bool,
    movie_focus_media_frozen: bool,
    movie_focus_playback_resumed: bool,
    movie_focus_sample_span_micros: u64,
    movie_focus_stable_span_micros: u64,
    manual_no_save_returns: u32,
    manual_resize_cancelled: u32,
    manual_swipes: u32,
    origin_input_equivalent: bool,
    origin_tips_returned: bool,
    origin_resize_cancelled: bool,
    origin_title_returned: bool,
    origin_no_save_return: bool,
    focus_clock_frozen: bool,
    inherited_enter_blocked: bool,
    close_clock_frozen: bool,
    complete: bool,
    error: Option<String>,
}
struct Parameters {
    cli: Cli,
    progress: PathBuf,
    report: Arc<Mutex<Report>>,
    stop: Arc<AtomicBool>,
    watchdog: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}
struct MovieFocus {
    checkpoint: Snapshot,
    suspended: Option<Snapshot>,
    checks: u8,
    first_sample: Option<Instant>,
    last_sample: Option<Instant>,
    settled_sample: Option<Instant>,
    losses_before: u64,
    suspends_before: u64,
    frozen_time: Option<u64>,
    restore_started: Option<Instant>,
}
struct Probe {
    inner: HigurashiApp,
    report: Arc<Mutex<Report>>,
    stage: Stage,
    next_raw: RawInputState,
    logical_notice: Vec2,
    companion: Option<Window>,
    focus_clock: Option<f64>,
    focus_checks: u8,
    focus_losses_before: u64,
    focus_suspends_before: u64,
    close_clock: Option<f64>,
    deadline: Instant,
    initial_focus_deadline: Instant,
    progress: PathBuf,
    config_source_commands: u64,
    menu_attempts: u32,
    manual_disk_before: Vec<u8>,
    chart_journey: Option<chart::Journey>,
    cg_journey: Option<cg_gallery::Journey>,
    music_journey: Option<music_gallery::Journey>,
    movie_journey: Option<movie_gallery::Journey>,
    movie_focus: Option<MovieFocus>,
}
fn inner_context<'a>(context: &'a mut AppContext<'_, Probe>) -> AppContext<'a, HigurashiApp> {
    AppContext {
        event_loop: context.event_loop,
        event_loop_proxy: context.event_loop_proxy,
        winit: &mut *context.winit,
        wgpu: context.wgpu,
        render: &mut *context.render,
    }
}
fn key(key: KeyCode) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.keyboard.insert(key);
    raw
}
fn controller_enter() -> RawInputState {
    controller_button(GamepadButton::A)
}
fn controller_button(button: GamepadButton) -> RawInputState {
    let mut raw = RawInputState::new();
    raw.gamepads.unified.buttons[button] = true;
    raw
}
fn pointer(window: &Window, logical: Vec2, held: bool) -> RawInputState {
    let viewport = HigurashiApp::map_canvas_size(window.inner_size());
    let mut raw = RawInputState::new();
    raw.mouse.position = Vec2::new(
        viewport.canvas_offset.x as f32,
        viewport.canvas_offset.y as f32,
    ) + logical / Vec2::new(1920., 1080.)
        * Vec2::new(
            viewport.canvas_size.width as f32,
            viewport.canvas_size.height as f32,
        );
    raw.mouse.buttons[MouseButton::Left] = held;
    raw
}
impl Probe {
    fn fail(&mut self, context: &AppContext<Self>, error: anyhow::Error) {
        let error = format!("{error:#}; stage={:?}", self.stage);
        eprintln!("WINDOW PROBE FAIL: {error}");
        self.report.lock().error = Some(error);
        context.event_loop.exit();
    }
    fn advance(&mut self, context: &mut AppContext<Self>) -> Result<()> {
        ensure!(
            Instant::now() < self.deadline,
            "window journey exceeded deadline"
        );
        let snapshot = self.inner.probe_snapshot()?;
        {
            let mut report = self.report.lock();
            report.max_commands = report.max_commands.max(snapshot.commands);
        }
        let window = &context.winit.window;
        let size = window.inner_size();
        match self.stage {
            Stage::AwaitNotice => {
                if size != PhysicalSize::new(1280, 720) {
                    return Ok(());
                }
                if let Some((2, logical)) = snapshot.notice {
                    self.logical_notice = logical;
                    self.next_raw = pointer(window, logical, true);
                    self.stage = Stage::PressSmall;
                    eprintln!("WINDOW PROBE: real notice ready at720p");
                }
            }
            Stage::PressSmall => {
                window.request_inner_size(PhysicalSize::new(1920, 1080));
                self.stage = Stage::ResizeHeld;
            }
            Stage::ResizeHeld => {
                self.next_raw = pointer(window, self.logical_notice, true);
                if size == PhysicalSize::new(1920, 1080) {
                    self.next_raw = pointer(window, self.logical_notice, false);
                    self.stage = Stage::ReleaseResized;
                }
            }
            Stage::ReleaseResized => {
                ensure!(
                    snapshot.notice.is_some_and(|(phase, _)| phase == 2),
                    "resize release answered an inherited notice contact"
                );
                self.report.lock().resize_cancelled = true;
                self.next_raw = pointer(window, self.logical_notice, true);
                self.stage = Stage::FreshPress;
            }
            Stage::FreshPress => {
                self.next_raw = pointer(window, self.logical_notice, false);
                self.stage = Stage::FreshRelease;
            }
            Stage::FreshRelease => {
                ensure!(
                    snapshot.notice.is_some_and(|(phase, _)| phase == 3),
                    "fresh1080p contact did not answer notice"
                );
                self.report.lock().fresh_pointer_answered = true;
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitDemo;
                eprintln!("WINDOW PROBE: resize cancelled hold; fresh1080p pointer answered");
            }
            Stage::AwaitDemo if snapshot.demo_ready => {
                self.next_raw = key(KeyCode::Enter);
                self.stage = Stage::DemoPressed;
            }
            Stage::DemoPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitTop;
            }
            Stage::AwaitTop if snapshot.menu == Some(("top", 2)) => {
                self.next_raw = key(KeyCode::ArrowDown);
                self.stage = Stage::TopConfigDown;
            }
            Stage::TopConfigDown => {
                ensure!(
                    snapshot.top_selected == Some(2),
                    "Down did not select title Config"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::TopConfigReleased;
            }
            Stage::TopConfigReleased => {
                self.next_raw = controller_enter();
                self.stage = Stage::AwaitTitleConfig;
            }
            Stage::AwaitTitleConfig
                if snapshot.config.is_some_and(|(flags, row, ready, volume)| {
                    flags == 0 && row == 0 && ready && volume == 37
                }) =>
            {
                self.next_raw = key(KeyCode::ArrowRight);
                self.stage = Stage::TitleConfigEdited;
            }
            Stage::TitleConfigEdited => {
                ensure!(
                    snapshot
                        .config
                        .is_some_and(|(_, _, _, volume)| volume == 42),
                    "title Config slider did not update37→42"
                );
                self.next_raw = key(KeyCode::KeyM);
                self.stage = Stage::TitleResetNoPressed;
            }
            Stage::TitleResetNoPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitResetNo;
            }
            Stage::AwaitResetNo if snapshot.config_question == Some((2, 1)) => {
                self.next_raw = controller_enter();
                self.stage = Stage::ResetNoAnswered;
            }
            Stage::ResetNoAnswered => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitResetNoReturn;
            }
            Stage::AwaitResetNoReturn if snapshot.config_question.is_none() => {
                ensure!(
                    snapshot
                        .config
                        .is_some_and(|(_, _, ready, volume)| ready && volume == 42),
                    "No reset modified settings"
                );
                self.report.lock().reset_no_preserved = true;
                self.next_raw = key(KeyCode::KeyM);
                self.stage = Stage::TitleResetYesPressed;
            }
            Stage::TitleResetYesPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitResetYes;
            }
            Stage::AwaitResetYes if snapshot.config_question == Some((2, 1)) => {
                self.next_raw = key(KeyCode::ArrowLeft);
                self.stage = Stage::ResetYesSelected;
            }
            Stage::ResetYesSelected => {
                ensure!(
                    snapshot.config_question == Some((2, 0)),
                    "Left did not select reset Yes"
                );
                self.next_raw = controller_enter();
                self.stage = Stage::ResetYesAnswered;
            }
            Stage::ResetYesAnswered => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitResetYesReturn;
            }
            Stage::AwaitResetYesReturn if snapshot.config_question.is_none() => {
                ensure!(
                    snapshot
                        .config
                        .is_some_and(|(_, row, ready, volume)| row == 0 && ready && volume == 40),
                    "Yes reset did not publish native defaults"
                );
                self.report.lock().reset_yes_applied = true;
                self.next_raw = key(KeyCode::ArrowLeft);
                self.stage = Stage::TitleConfigAdjusted;
            }
            Stage::TitleConfigAdjusted => {
                ensure!(
                    snapshot
                        .config
                        .is_some_and(|(_, _, _, volume)| volume == 35),
                    "reset slider did not update40→35"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::TitleConfigRelease;
            }
            Stage::TitleConfigRelease => {
                self.next_raw = key(KeyCode::Escape);
                self.stage = Stage::AwaitTitleConfigReturn;
            }
            Stage::AwaitTitleConfigReturn => {
                self.next_raw = RawInputState::new();
                if snapshot.config.is_none() && snapshot.menu == Some(("top", 2)) {
                    ensure!(
                        snapshot.top_selected == Some(2),
                        "title Config final callback returned to wrong Top row"
                    );
                    ensure!(
                        SaveData::decode(&std::fs::read(&self.progress)?)?
                            .settings
                            .common
                            .v3_sysvol
                            == 35,
                        "title Config SaveGame did not persist35"
                    );
                    self.report.lock().title_config_saved = true;
                    self.chart_journey = Some(chart::Journey::new(
                        chart::Source::Title,
                        &snapshot,
                        &self.progress,
                    )?);
                    self.stage = Stage::Chart(chart::Source::Title);
                    eprintln!(
                        "WINDOW PROBE: title Config0 reset No/Yes,slider35,real SaveGame and Top2 return PASS"
                    );
                }
            }
            Stage::Chart(source) => {
                let journey = self
                    .chart_journey
                    .as_mut()
                    .context("missing Chart journey")?;
                let step = journey.poll(&snapshot, &self.progress)?;
                self.next_raw = step.raw;
                if step.done {
                    let report = journey.report().clone();
                    self.chart_journey = None;
                    match source {
                        chart::Source::Title => {
                            self.report.lock().title_chart = Some(report);
                            self.next_raw = key(KeyCode::ArrowLeft);
                            self.stage = Stage::ChartTitleLeft;
                        }
                        chart::Source::Game => {
                            self.report.lock().game_chart = Some(report);
                            self.next_raw = key(KeyCode::Escape);
                            self.stage = Stage::SystemMenuClosed;
                        }
                    }
                    eprintln!(
                        "WINDOW PROBE: {source:?} retained Chart/raw signed analog,D-pad,keyboard,shoulders,source return/no save PASS"
                    );
                }
            }
            Stage::ChartTitleLeft => {
                ensure!(
                    snapshot.menu == Some(("top", 2)) && snapshot.top_selected == Some(2),
                    "Chart return Left did not reach ready Top2"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::ChartTitleReleased;
            }
            Stage::ChartTitleReleased => {
                self.next_raw = key(KeyCode::ArrowDown);
                self.stage = Stage::AwaitCgTopFive;
            }
            Stage::AwaitCgTopFive => {
                self.next_raw = RawInputState::new();
                self.cg_journey = Some(cg_gallery::Journey::new(&snapshot, &self.progress)?);
                self.stage = Stage::Cg;
            }
            Stage::Cg => {
                let journey = self
                    .cg_journey
                    .as_mut()
                    .context("missing CG window journey")?;
                let step = journey.poll(&snapshot, &self.progress)?;
                self.next_raw = step.raw;
                if step.done {
                    self.report.lock().cg = Some(journey.report().clone());
                    self.cg_journey = None;
                    self.music_journey =
                        Some(music_gallery::Journey::new(&snapshot, &self.progress)?);
                    self.stage = Stage::Music;
                    eprintln!(
                        "WINDOW PROBE: actual Category/CG/two decoded tall PICs,raw analog/D-pad/keyboard,info/fit,retained-grid/freshTitle15/Top5,no save PASS"
                    );
                }
            }
            Stage::Music => {
                let journey = self
                    .music_journey
                    .as_mut()
                    .context("missing Music window journey")?;
                let step = journey.poll(&snapshot, &self.progress)?;
                self.next_raw = step.raw;
                if step.done {
                    ensure!(journey.report().complete(), "incomplete Music journey");
                    self.report.lock().music = Some(journey.report().clone());
                    self.music_journey = None;
                    self.movie_journey =
                        Some(movie_gallery::Journey::new(&snapshot, &self.progress)?);
                    self.stage = Stage::Movie;
                    eprintln!(
                        "WINDOW PROBE: actual Music/two visits,keyboard/controller arrows/shoulders,real play/Y-stop,no extra Decide,both B closes/freshTitle16/Top5,no save PASS"
                    );
                }
            }
            Stage::Movie => {
                let journey = self
                    .movie_journey
                    .as_mut()
                    .context("missing Movie journey")?;
                let step = journey.poll(&snapshot, &self.progress)?;
                self.next_raw = step.raw;
                if step.focus_checkpoint {
                    ensure!(
                        self.movie_focus.is_none(),
                        "Movie focus checkpoint repeated"
                    );
                    let report = self.report.lock();
                    self.movie_focus = Some(MovieFocus {
                        checkpoint: snapshot.clone(),
                        suspended: None,
                        checks: 0,
                        first_sample: None,
                        last_sample: None,
                        settled_sample: None,
                        losses_before: report.focus_losses,
                        suspends_before: report.suspend_calls,
                        frozen_time: None,
                        restore_started: None,
                    });
                    drop(report);
                    self.stage = Stage::MovieFocusWait;
                    let companion = context.event_loop.create_window(
                        Window::default_attributes()
                            .with_title("Higurashi Movie focus probe")
                            .with_inner_size(PhysicalSize::new(160, 90)),
                    )?;
                    companion.focus_window();
                    self.companion = Some(companion);
                } else if step.done {
                    ensure!(journey.report().complete(), "incomplete Movie journey");
                    self.report.lock().movie = Some(journey.report().clone());
                    self.movie_journey = None;
                    self.stage = Stage::TitleManualReleased;
                    eprintln!(
                        "WINDOW PROBE: Movie two visits/three authored players,canonical keyboard/controller/firststick,locked error,Viewer A/B cancels,retained grids,freshTitle17,no save PASS"
                    );
                }
            }
            Stage::MovieFocusWait => {}
            Stage::MovieFocusRestoring => {
                ensure!(
                    window.has_focus() && !self.inner.probe_suspended(),
                    "Movie resume requires authoritative window focus/audio resume"
                );
                self.next_raw = RawInputState::new();
                self.companion.take();
                self.movie_focus.as_mut().unwrap().restore_started = Some(Instant::now());
                self.stage = Stage::MovieFocusResuming;
            }
            Stage::MovieFocusResuming => {
                self.next_raw = RawInputState::new();
                let focus = self
                    .movie_focus
                    .as_ref()
                    .context("missing Movie focus state")?;
                ensure!(
                    focus.restore_started.unwrap().elapsed() < Duration::from_secs(5),
                    "Movie media did not resume after real focus restoration"
                );
                let current = snapshot
                    .movie
                    .as_ref()
                    .context("Movie source lost on focus resume")?;
                let before = focus.checkpoint.movie.as_ref().unwrap();
                let viewer = current
                    .viewer
                    .as_ref()
                    .context("Movie Viewer lost on focus resume")?;
                let original = before.viewer.as_ref().unwrap();
                ensure!(
                    viewer.id == original.id
                        && viewer.movie_id == original.movie_id
                        && viewer.status == 1
                        && viewer.playing
                        && !viewer.closing
                        && viewer.private_input_ready
                        && viewer.retained_frame == original.retained_frame
                        && current.cues == before.cues
                        && current.selected == before.selected,
                    "Movie focus restore changed Viewer/input/cues"
                );
                if viewer
                    .time_micros
                    .context("Movie media clock missing after resume")?
                    >= focus.frozen_time.unwrap().saturating_add(100_000)
                {
                    self.movie_journey
                        .as_mut()
                        .unwrap()
                        .resume_after_focus(&snapshot, &self.progress)?;
                    self.report.lock().movie_focus_playback_resumed = true;
                    self.stage = Stage::Movie;
                    eprintln!(
                        "WINDOW PROBE: actual playing Movie focus froze scene/media and resumed retained Viewer without cancel PASS"
                    );
                }
            }
            Stage::TitleManualDown => {
                ensure!(
                    matches!(snapshot.top_selected, Some(5 | 7 | 4)),
                    "Manual navigation left the native two-column rows: {:?}",
                    snapshot.top_selected
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::TitleManualReleased;
            }
            Stage::TitleManualReleased => match snapshot.top_selected {
                Some(5) => {
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::TitleManualDown;
                }
                Some(7) => {
                    self.next_raw = key(KeyCode::ArrowRight);
                    self.stage = Stage::TitleManualDown;
                }
                Some(4) => {
                    self.next_raw = controller_enter();
                    self.stage = Stage::AwaitManualOpen(ManualSource::Title);
                }
                value => anyhow::bail!("Unexpected title Manual navigation row {value:?}"),
            },
            Stage::AwaitManualOpen(source)
                if snapshot.manual.is_some_and(|(count, page, ready, _, _)| {
                    count == 18 && page == 0 && ready
                }) =>
            {
                self.manual_disk_before = std::fs::read(&self.progress)?;
                self.next_raw = match source {
                    ManualSource::Title => key(KeyCode::KeyJ),
                    ManualSource::Game => controller_button(GamepadButton::R),
                };
                self.stage = Stage::ManualForwardPressed(source);
            }
            Stage::ManualForwardPressed(source) => {
                ensure!(
                    snapshot.manual.is_some_and(|(_, page, _, _, _)| page == 1),
                    "Manual R/J did not advance page"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitManualForward(source);
            }
            Stage::AwaitManualForward(source)
                if snapshot.manual.is_some_and(|(_, page, ready, scroll, _)| {
                    page == 1 && ready && scroll == 1500f32.to_bits()
                }) =>
            {
                self.next_raw = match source {
                    ManualSource::Title => controller_button(GamepadButton::L),
                    ManualSource::Game => key(KeyCode::KeyH),
                };
                self.stage = Stage::ManualBackwardPressed(source);
            }
            Stage::ManualBackwardPressed(source) => {
                ensure!(
                    snapshot.manual.is_some_and(|(_, page, _, _, _)| page == 0),
                    "Manual L/H did not restore page"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitManualBackward(source);
            }
            Stage::AwaitManualBackward(source)
                if snapshot.manual.is_some_and(|(_, page, ready, scroll, _)| {
                    page == 0 && ready && scroll == 0f32.to_bits()
                }) =>
            {
                self.next_raw = pointer(window, Vec2::new(1100., 570.), true);
                self.stage = Stage::ManualDragPressed(source);
            }
            Stage::ManualDragPressed(source) => {
                self.next_raw = pointer(window, Vec2::new(650., 570.), true);
                self.stage = Stage::ManualDragMoved(source);
            }
            Stage::ManualDragMoved(source) => {
                ensure!(
                    snapshot
                        .manual
                        .is_some_and(|(_, _, _, scroll, _)| f32::from_bits(scroll) > 400.),
                    "Manual real pointer drag did not scroll pages"
                );
                self.next_raw = pointer(window, Vec2::new(650., 570.), false);
                self.stage = Stage::AwaitManualSwipe(source);
            }
            Stage::AwaitManualSwipe(source)
                if snapshot.manual.is_some_and(|(_, page, ready, scroll, _)| {
                    page == 1 && ready && scroll == 1500f32.to_bits()
                }) =>
            {
                self.report.lock().manual_swipes += 1;
                self.next_raw = pointer(window, Vec2::new(1806., 72.), true);
                self.stage = Stage::ManualCloseHeld(source);
            }
            Stage::ManualCloseHeld(source) => {
                let target = match source {
                    ManualSource::Title => PhysicalSize::new(1280, 720),
                    // The Origin journey leaves the window at1080p, so this
                    // must change the extent to cancel the held contact.
                    ManualSource::Game => PhysicalSize::new(1280, 720),
                };
                window.request_inner_size(target);
                self.stage = Stage::ManualResizeHeld(source);
            }
            Stage::ManualResizeHeld(source) => {
                self.next_raw = pointer(window, Vec2::new(1806., 72.), true);
                let target = match source {
                    ManualSource::Title => PhysicalSize::new(1280, 720),
                    ManualSource::Game => PhysicalSize::new(1280, 720),
                };
                if size == target {
                    self.next_raw = pointer(window, Vec2::new(1806., 72.), false);
                    self.stage = Stage::ManualResizedRelease(source);
                }
            }
            Stage::ManualResizedRelease(source) => {
                ensure!(
                    snapshot
                        .manual
                        .is_some_and(|(_, page, ready, _, contact)| page == 1
                            && ready
                            && contact == -1),
                    "Manual resize release answered an inherited close contact"
                );
                self.report.lock().manual_resize_cancelled += 1;
                self.next_raw = pointer(window, Vec2::new(1806., 72.), true);
                self.stage = Stage::ManualFreshPress(source);
            }
            Stage::ManualFreshPress(source) => {
                self.next_raw = pointer(window, Vec2::new(1806., 72.), false);
                self.stage = Stage::AwaitManualReturn(source);
            }
            Stage::AwaitManualReturn(source) => {
                self.next_raw = RawInputState::new();
                let returned = snapshot.manual.is_none()
                    && match source {
                        ManualSource::Title => snapshot.menu == Some(("top", 2)),
                        ManualSource::Game => snapshot.system_menu.is_some_and(|(_, ready)| ready),
                    };
                if returned {
                    ensure!(
                        std::fs::read(&self.progress)? == self.manual_disk_before,
                        "Manual unexpectedly wrote progress/settings"
                    );
                    self.report.lock().manual_no_save_returns += 1;
                    match source {
                        ManualSource::Title => {
                            ensure!(
                                snapshot.top_selected == Some(4),
                                "Manual callback did not return Top4"
                            );
                            self.report.lock().title_manual_returned = true;
                            self.next_raw = key(KeyCode::ArrowDown);
                            self.stage = Stage::OriginTopSelected;
                        }
                        ManualSource::Game => {
                            ensure!(
                                snapshot.commands == self.config_source_commands,
                                "Manual/SystemMenu advanced underlying script commands"
                            );
                            self.report.lock().game_manual_returned = true;
                            self.chart_journey = Some(chart::Journey::new(
                                chart::Source::Game,
                                &snapshot,
                                &self.progress,
                            )?);
                            self.stage = Stage::Chart(chart::Source::Game);
                        }
                    }
                    eprintln!(
                        "WINDOW PROBE: {source:?} Manual18 real pages,J/R and H/L,drag/swipe,resize-held close cancellation,fresh close,source return and no save PASS"
                    );
                }
            }
            Stage::OriginTopSelected => {
                ensure!(
                    snapshot.top_selected == Some(8),
                    "Down from Top4 did not select Origin8"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::OriginTopReleased;
            }
            Stage::OriginTopReleased => {
                self.next_raw = controller_enter();
                self.stage = Stage::AwaitOrigin;
            }
            Stage::AwaitOrigin => {
                self.next_raw = RawInputState::new();
                if snapshot.picker.is_some_and(|(kind, selected, ready, _)| {
                    (kind, selected, ready) == (5, 0, true)
                }) {
                    self.manual_disk_before = std::fs::read(&self.progress)?;
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::OriginDownPressed;
                }
            }
            Stage::OriginDownPressed => {
                ensure!(
                    snapshot
                        .picker
                        .is_some_and(|(_, selected, _, _)| selected == 2),
                    "Origin keyboard Down did not move two rows"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::OriginDownReleased;
            }
            Stage::OriginDownReleased => {
                self.next_raw = controller_button(GamepadButton::Up);
                self.stage = Stage::OriginUpPressed;
            }
            Stage::OriginUpPressed => {
                ensure!(
                    snapshot
                        .picker
                        .is_some_and(|(_, selected, _, _)| selected == 0),
                    "Origin controller Up did not restore row0"
                );
                self.report.lock().origin_input_equivalent = true;
                self.next_raw = RawInputState::new();
                self.stage = Stage::OriginUpReleased;
            }
            Stage::OriginUpReleased => {
                self.next_raw = key(KeyCode::ArrowDown);
                self.stage = Stage::OriginTipsDownPressed;
            }
            Stage::OriginTipsDownPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::OriginTipsDownReleased;
            }
            Stage::OriginTipsDownReleased => {
                if snapshot
                    .picker
                    .is_some_and(|(_, selected, _, _)| selected == 9)
                {
                    self.next_raw = controller_enter();
                    self.stage = Stage::OriginTipsPressed;
                } else {
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::OriginTipsDownPressed;
                }
            }
            Stage::OriginTipsPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitOriginTips;
            }
            Stage::AwaitOriginTips => {
                if snapshot.tips {
                    self.next_raw = key(KeyCode::Escape);
                    self.stage = Stage::OriginTipsBackPressed;
                }
            }
            Stage::OriginTipsBackPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitOriginTipsReturn(20);
            }
            Stage::AwaitOriginTipsReturn(count) => {
                self.next_raw = RawInputState::new();
                if snapshot.picker.is_some_and(|(kind, selected, ready, _)| {
                    (kind, selected, ready) == (5, 9, true)
                }) {
                    ensure!(
                        std::fs::read(&self.progress)? == self.manual_disk_before,
                        "Origin/TIPS unexpectedly wrote progress"
                    );
                    self.report.lock().origin_tips_returned = true;
                    self.next_raw = pointer(window, Vec2::new(1806., 72.), true);
                    self.stage = Stage::OriginCloseHeld;
                } else if snapshot.tips && count == 0 {
                    self.next_raw = key(KeyCode::Escape);
                    self.stage = Stage::OriginTipsBackPressed;
                } else {
                    self.stage = Stage::AwaitOriginTipsReturn(count.saturating_sub(1));
                }
            }
            Stage::OriginCloseHeld => {
                ensure!(
                    snapshot.picker == Some((5, 9, true, 1)),
                    "Origin close pointer did not acquire the intended contact"
                );
                window.request_inner_size(PhysicalSize::new(1920, 1080));
                self.stage = Stage::OriginResizeHeld;
            }
            Stage::OriginResizeHeld => {
                self.next_raw = pointer(window, Vec2::new(1806., 72.), true);
                if size == PhysicalSize::new(1920, 1080) {
                    self.next_raw = pointer(window, Vec2::new(1806., 72.), false);
                    self.stage = Stage::OriginResizeReleased;
                }
            }
            Stage::OriginResizeReleased => {
                ensure!(
                    snapshot
                        .picker
                        .is_some_and(|(kind, selected, ready, contact)| (
                            kind, selected, ready, contact
                        ) == (5, 9, true, -1))
                        && snapshot.picker_close_idle == Some(true),
                    "Origin resize inherited a held close"
                );
                self.report.lock().origin_resize_cancelled = true;
                self.next_raw = controller_button(GamepadButton::B);
                self.stage = Stage::OriginCancelPressed;
            }
            Stage::OriginCancelPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitOriginTitleReturn;
            }
            Stage::AwaitOriginTitleReturn => {
                if snapshot.picker.is_none() && snapshot.menu == Some(("top", 2)) {
                    ensure!(
                        snapshot.top_selected == Some(8),
                        "Origin B did not return Top8"
                    );
                    ensure!(
                        std::fs::read(&self.progress)? == self.manual_disk_before,
                        "Origin return unexpectedly saved progress"
                    );
                    self.report.lock().origin_title_returned = true;
                    self.report.lock().origin_no_save_return = true;
                    self.next_raw = key(KeyCode::ArrowUp);
                    self.stage = Stage::TitleStoryUp;
                    eprintln!(
                        "WINDOW PROBE: actual Top8/Origin0/alternate TIPS/Origin9/Top8;keyboard/controller arrows,A/B,held-close resize cancellation,no save PASS"
                    );
                }
            }
            Stage::TitleStoryUp => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::TitleStoryReleased;
            }
            Stage::TitleStoryReleased => {
                if snapshot.top_selected == Some(0) {
                    self.next_raw = key(KeyCode::Enter);
                    self.stage = Stage::TopPressed;
                } else {
                    self.next_raw = key(KeyCode::ArrowUp);
                    self.stage = Stage::TitleStoryUp;
                }
            }
            Stage::TopPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitCat;
            }
            Stage::AwaitCat if snapshot.menu == Some(("cat", 2)) => {
                self.next_raw = controller_enter();
                self.stage = Stage::CatPressed;
            }
            Stage::CatPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitMessage;
            }
            Stage::AwaitMessage if snapshot.message && snapshot.commands >= 10 => {
                self.stage = Stage::MessageSettled(8);
            }
            Stage::MessageSettled(0) => {
                self.config_source_commands = snapshot.commands;
                self.next_raw = key(KeyCode::KeyM);
                self.stage = Stage::SystemMenuPressed;
                eprintln!(
                    "WINDOW PROBE: authored story message,{} completed commands",
                    snapshot.commands
                );
            }
            Stage::MessageSettled(left) => self.stage = Stage::MessageSettled(left - 1),
            Stage::SystemMenuPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitSystemMenu;
            }
            Stage::AwaitSystemMenu if snapshot.system_menu.is_some_and(|(_, ready)| ready) => {
                if snapshot.system_menu == Some((2, true)) {
                    self.next_raw = controller_enter();
                    self.stage = Stage::SystemConfigPressed;
                } else {
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::SystemConfigDown;
                }
            }
            Stage::SystemConfigDown => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::SystemConfigReleased;
            }
            Stage::AwaitSystemMenu => {
                self.menu_attempts += 1;
                self.next_raw = if self.menu_attempts % 30 == 0 {
                    key(KeyCode::KeyM)
                } else {
                    RawInputState::new()
                };
                if self.menu_attempts % 600 == 1 {
                    eprintln!(
                        "WINDOW PROBE: menu eligibility {:?}",
                        snapshot.input_diagnostics
                    );
                }
            }
            Stage::SystemConfigReleased => {
                self.stage = Stage::AwaitSystemMenu;
            }
            Stage::SystemConfigPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitGameConfig;
            }
            Stage::AwaitGameConfig
                if snapshot.config.is_some_and(|(flags, row, ready, volume)| {
                    flags == 1 && row == 0 && ready && volume == 35
                }) =>
            {
                self.next_raw = key(KeyCode::ArrowLeft);
                self.stage = Stage::GameConfigEdited;
            }
            Stage::GameConfigEdited => {
                ensure!(
                    snapshot
                        .config
                        .is_some_and(|(_, _, _, volume)| volume == 30),
                    "ADV Config slider did not update35→30"
                );
                self.next_raw = RawInputState::new();
                self.stage = Stage::GameConfigRelease;
            }
            Stage::GameConfigRelease => {
                self.next_raw = key(KeyCode::Escape);
                self.stage = Stage::AwaitGameConfigReturn;
            }
            Stage::AwaitGameConfigReturn => {
                self.next_raw = RawInputState::new();
                if snapshot.config.is_none() && snapshot.system_menu.is_some_and(|(_, ready)| ready)
                {
                    ensure!(
                        snapshot.commands == self.config_source_commands,
                        "Config/SystemMenu advanced underlying story commands"
                    );
                    ensure!(
                        SaveData::decode(&std::fs::read(&self.progress)?)?
                            .settings
                            .common
                            .v3_sysvol
                            == 30,
                        "ADV Config SaveGame did not persist30"
                    );
                    {
                        let mut report = self.report.lock();
                        report.game_config_saved = true;
                        report.config_source_commands_frozen = true;
                    }
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::GameManualSelectRelease;
                    eprintln!(
                        "WINDOW PROBE: ADV Config1 slider30,real SaveGame,SystemMenu return and frozen source commands PASS"
                    );
                }
            }
            Stage::GameManualSelect => {
                if snapshot.system_menu == Some((3, true)) {
                    self.next_raw = controller_enter();
                    self.stage = Stage::GameManualPressed;
                } else if snapshot.system_menu.is_some_and(|(_, ready)| ready) {
                    self.next_raw = key(KeyCode::ArrowDown);
                    self.stage = Stage::GameManualSelectRelease;
                }
            }
            Stage::GameManualSelectRelease => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::GameManualSelect;
            }
            Stage::GameManualPressed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitManualOpen(ManualSource::Game);
            }
            Stage::SystemMenuClosed => {
                self.next_raw = RawInputState::new();
                self.stage = Stage::AwaitGameplay;
            }
            Stage::AwaitGameplay if snapshot.system_menu.is_none() => {
                self.next_raw = key(KeyCode::Enter);
                self.stage = Stage::FocusHeld;
            }
            Stage::FocusHeld => {
                self.focus_clock = snapshot.clock;
                self.focus_losses_before = self.report.lock().focus_losses;
                self.focus_suspends_before = self.report.lock().suspend_calls;
                self.stage = Stage::FocusWait;
                let companion = context.event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Higurashi automated focus probe")
                        .with_inner_size(PhysicalSize::new(160, 90)),
                )?;
                companion.focus_window();
                self.companion = Some(companion);
            }
            Stage::FocusWait => {}
            Stage::FocusRestoring => {
                ensure!(
                    window.has_focus(),
                    "resumed callback without authoritative window focus"
                );
                ensure!(
                    snapshot.buttons.pressed & ((1 << 13) | (1 << 24)) == 0,
                    "inherited Enter became a fresh press after focus restore"
                );
                self.report.lock().inherited_enter_blocked = true;
                self.next_raw = RawInputState::new();
                self.companion.take();
                self.stage = Stage::FocusRelease;
            }
            Stage::FocusRelease => {
                ensure!(
                    snapshot.buttons.held & ((1 << 13) | (1 << 24)) == 0,
                    "released Enter remains held"
                );
                self.close_clock = snapshot.clock;
                self.stage = Stage::CloseQueued;
                context
                    .event_loop_proxy
                    .request_close()
                    .context("enqueue deferred close")?;
            }
            Stage::CloseQueued | Stage::Closing => {
                if snapshot.frozen && !snapshot.exited {
                    ensure!(
                        snapshot.clock == self.close_clock,
                        "close writer advanced authored scene clock"
                    );
                    self.report.lock().close_clock_frozen = true;
                }
                if snapshot.closing {
                    self.stage = Stage::Closing;
                }
                if snapshot.exited {
                    self.report.lock().complete = true;
                    self.stage = Stage::Done;
                    eprintln!(
                        "WINDOW PROBE: native window driver close completed and retired scenes"
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn check_custom(&mut self, context: &mut AppContext<Self>) -> Result<()> {
        ensure!(
            Instant::now() < self.deadline,
            "window journey exceeded deadline"
        );
        // Synchronous asset setup can outlast macOS's initial activation.
        // Acquire actual focus only before the first production update; later
        // focus changes belong exclusively to the companion-window journeys.
        if self.report.lock().updates == 0 {
            ensure!(
                Instant::now() < self.initial_focus_deadline,
                "initial window never became focused and resumed"
            );
            if !context.winit.window.has_focus() {
                context.winit.window.focus_window();
            }
            context.winit.window.request_redraw();
            return Ok(());
        }
        if matches!(self.stage, Stage::MovieFocusWait) && !context.winit.window.has_focus() {
            let focus = self
                .movie_focus
                .as_mut()
                .context("missing Movie focus checkpoint")?;
            let report = self.report.lock();
            ensure!(
                report.focus_losses > focus.losses_before
                    && report.suspend_calls > focus.suspends_before,
                "Movie focus loss did not reach actual callbacks"
            );
            drop(report);
            ensure!(
                self.inner.probe_suspended(),
                "Movie focus did not suspend actual audio manager"
            );
            let snapshot = self.inner.probe_snapshot()?;
            let current = snapshot
                .movie
                .as_ref()
                .context("Movie source lost while unfocused")?;
            let viewer = current
                .viewer
                .as_ref()
                .context("Movie Viewer lost while unfocused")?;
            let checkpoint = focus.checkpoint.movie.as_ref().unwrap();
            let old_viewer = checkpoint.viewer.as_ref().unwrap();
            ensure!(
                viewer.id == old_viewer.id
                    && viewer.movie_id == old_viewer.movie_id
                    && viewer.status == 1
                    && viewer.playing
                    && !viewer.closing
                    && viewer.retained_frame == old_viewer.retained_frame
                    && current.cues == checkpoint.cues,
                "Unfocused Movie lost retained/playing Viewer or changed cues"
            );
            let media_time = viewer
                .time_micros
                .context("Unfocused Movie clock missing")?;
            if let Some(baseline) = &focus.suspended {
                ensure!(
                    snapshot.frames == baseline.frames
                        && snapshot.clock == baseline.clock
                        && snapshot.commands == baseline.commands,
                    "Unfocused Movie advanced runtime/scene clock/VM"
                );
                let mut stable = current.clone();
                let mut expected = baseline.movie.as_ref().unwrap().clone();
                stable.viewer.as_mut().unwrap().time_micros = None;
                expected.viewer.as_mut().unwrap().time_micros = None;
                ensure!(
                    stable == expected,
                    "Unfocused Movie advanced model/tweens/input/scope/retained frame"
                );
            } else {
                focus.suspended = Some(snapshot.clone());
            }
            // Queued watchdog events can arrive together. Accept media samples
            // only after100ms of real elapsed time, independently of callbacks.
            let sampled_at = Instant::now();
            if focus
                .last_sample
                .is_some_and(|last| sampled_at.duration_since(last) < Duration::from_millis(100))
            {
                return Ok(());
            }
            focus.first_sample.get_or_insert(sampled_at);
            focus.last_sample = Some(sampled_at);
            // Two separated observations settle any in-flight AAC callback;
            // THREE further observations preserve media position within1ms.
            focus.checks += 1;
            if focus.checks <= 2 {
                focus.frozen_time = Some(media_time);
                focus.settled_sample = Some(sampled_at);
            } else {
                ensure!(
                    media_time.abs_diff(focus.frozen_time.unwrap()) <= 1_000,
                    "Unfocused Movie media time advanced after suspension settled"
                );
            }
            if focus.checks >= 5 {
                let sample_span = sampled_at.duration_since(focus.first_sample.unwrap());
                let stable_span = sampled_at.duration_since(focus.settled_sample.unwrap());
                ensure!(
                    sample_span >= Duration::from_millis(400)
                        && stable_span >= Duration::from_millis(300),
                    "Movie focus samples did not span the required real duration"
                );
                let mut report = self.report.lock();
                report.movie_focus_callbacks = true;
                report.movie_focus_clock_frozen = true;
                report.movie_focus_media_frozen = true;
                report.movie_focus_sample_span_micros = sample_span.as_micros() as u64;
                report.movie_focus_stable_span_micros = stable_span.as_micros() as u64;
                drop(report);
                focus.restore_started = Some(Instant::now());
                self.stage = Stage::MovieFocusRestoring;
                context.winit.window.focus_window();
            }
        }
        if matches!(self.stage, Stage::FocusWait) && !context.winit.window.has_focus() {
            let fresh_callbacks = {
                let report = self.report.lock();
                report.focus_losses > self.focus_losses_before
                    && report.suspend_calls > self.focus_suspends_before
            };
            ensure!(fresh_callbacks, "actual focus loss did not reach ShinApp");
            ensure!(
                self.inner.probe_suspended(),
                "focus suspension did not freeze audio"
            );
            let snapshot = self.inner.probe_snapshot()?;
            ensure!(
                snapshot.clock == self.focus_clock,
                "unfocused window advanced scene clock"
            );
            self.focus_checks += 1;
            if self.focus_checks >= 3 {
                self.report.lock().focus_clock_frozen = true;
                self.stage = Stage::FocusRestoring;
                context.winit.window.focus_window();
                eprintln!("WINDOW PROBE: actual focus loss froze scene/audio; restoring window");
            }
        }
        Ok(())
    }
}
impl ShinApp for Probe {
    type Parameters = Parameters;
    type EventType = ();
    type ActionType = DummyAction;
    fn init(mut context: AppContext<Self>, parameters: Parameters) -> Result<Self> {
        context
            .winit
            .window
            .set_title("Higurashi internal automated window probe");
        context
            .winit
            .window
            .request_inner_size(PhysicalSize::new(1280, 720));
        let inner = HigurashiApp::init(inner_context(&mut context), parameters.cli)?;
        parameters.report.lock().initial_focus = context.winit.window.has_focus();
        context.winit.window.focus_window();
        context.winit.window.request_redraw();
        let proxy = context.event_loop_proxy.clone();
        let stop = parameters.stop.clone();
        *parameters.watchdog.lock() = Some(std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
                if proxy.send_event(()).is_err() {
                    break;
                }
            }
        }));
        Ok(Self {
            inner,
            report: parameters.report,
            stage: Stage::AwaitNotice,
            next_raw: RawInputState::new(),
            logical_notice: Vec2::ZERO,
            companion: None,
            focus_clock: None,
            focus_checks: 0,
            focus_losses_before: 0,
            focus_suspends_before: 0,
            close_clock: None,
            deadline: Instant::now() + Duration::from_secs(180),
            initial_focus_deadline: Instant::now() + Duration::from_secs(10),
            progress: parameters.progress,
            config_source_commands: 0,
            menu_attempts: 0,
            manual_disk_before: Vec::new(),
            chart_journey: None,
            cg_journey: None,
            music_journey: None,
            movie_journey: None,
            movie_focus: None,
        })
    }
    fn custom_event(&mut self, mut context: AppContext<Self>, _: ()) {
        if let Err(error) = self.check_custom(&mut context) {
            self.fail(&context, error);
        }
    }
    fn raw_input(&mut self, _: &RawInputState) {
        self.report.lock().raw_samples += 1;
        self.inner.raw_input(&self.next_raw);
    }
    fn run_in_background(&self) -> bool {
        self.inner.run_in_background()
    }
    fn focus_lost(&mut self) {
        self.report.lock().focus_losses += 1;
        self.inner.focus_lost();
    }
    fn set_suspended(&mut self, suspended: bool) {
        if suspended {
            self.report.lock().suspend_calls += 1;
        } else {
            self.report.lock().resume_calls += 1;
        }
        self.inner.set_suspended(suspended);
    }
    fn close_requested(&mut self) -> bool {
        self.report.lock().close_calls += 1;
        self.inner.close_requested()
    }
    fn update(
        &mut self,
        mut context: AppContext<Self>,
        input: EnumMap<DummyAction, ActionState>,
        elapsed: Duration,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.report.lock().updates += 1;
        self.inner
            .update(inner_context(&mut context), input, elapsed, encoder);
        if let Err(error) = self.advance(&mut context) {
            self.fail(&context, error);
        }
    }
    fn render(&mut self, context: RenderContext, pass: &mut RenderPass) {
        let before = self.inner.probe_snapshot().unwrap();
        self.inner.render(context, pass);
        let after = self.inner.probe_snapshot().unwrap();
        match (&before.music, &after.music) {
            (Some(before), Some(after)) => before.assert_render_getter(after).unwrap(),
            (None, None) => {}
            _ => panic!("Music scene presence changed during render"),
        }
        match (&before.movie, &after.movie) {
            (Some(before), Some(after)) => before.assert_render_getter(after).unwrap(),
            (None, None) => {}
            _ => panic!("Movie scene presence changed during render"),
        }
        assert_eq!(
            (
                before.frames,
                before.clock,
                before.commands,
                before.manual,
                before.chart,
                before.chart_cursor,
                before.cg,
                before.category,
                before.title_initial,
                before.picker,
                before.picker_close_idle,
                before.tips
            ),
            (
                after.frames,
                after.clock,
                after.commands,
                after.manual,
                after.chart,
                after.chart_cursor,
                after.cg,
                after.category,
                after.title_initial,
                after.picker,
                after.picker_close_idle,
                after.tips
            ),
            "render must remain a getter"
        );
        self.report.lock().renders += 1;
    }
}

pub(crate) fn run() -> Result<()> {
    let assets =
        std::env::var("SHIN_HIGURASHI_ASSETS").context("SHIN_HIGURASHI_ASSETS is required")?;
    let directory = std::env::temp_dir().join(format!(
        "higurashi-window-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let path = directory.join("progress.dat");
    let storage = Paths {
        current: Some(path.clone()),
        trial: None,
    };
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    // Controlled unlock fixture in this unique temporary save only.
    saved.persist_data.set(0, 31);
    saved.save_vectors.extra_words_1 = vec![u32::MAX; 15];
    saved.save_vectors.unlocked_cgs = vec![u32::MAX; 42];
    saved.save_vectors.unlocked_bgms = vec![u32::MAX; 8];
    // Native roster cells0/1/2 are playable, while cell3(ID1) stays locked.
    saved.save_vectors.unlocked_movies = vec![(1 << 9) | (1 << 6) | 1];
    storage.write(&saved.encode()?)?;
    let cli = Cli::try_parse_from([
        "higurashi-window-probe",
        "--game",
        "higurashi",
        "--loopback-audio",
        "--assets-dir",
        &assets,
        "--save-file",
        path.to_str().context("non-UTF8 temporary path")?,
    ])?;
    let report = Arc::new(Mutex::new(Report::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let watchdog = Arc::new(Mutex::new(None));
    shin_window::run_window::<Probe>(Parameters {
        cli,
        progress: path.clone(),
        report: report.clone(),
        stop: stop.clone(),
        watchdog: watchdog.clone(),
    });
    stop.store(true, Ordering::Release);
    if let Some(thread) = watchdog.lock().take() {
        thread.join().unwrap();
    }
    let report = report.lock();
    eprintln!("WINDOW PROBE REPORT: {report:#?}");
    ensure!(
        report.error.is_none(),
        "window probe failed: {:?}",
        report.error
    );
    ensure!(
        report.complete
            && report.renders > 0
            && report.updates == report.raw_samples
            && report.close_calls == 1,
        "window journey did not complete all production callbacks"
    );
    ensure!(
        report.resize_cancelled
            && report.fresh_pointer_answered
            && report.focus_clock_frozen
            && report.inherited_enter_blocked
            && report.close_clock_frozen,
        "window lifecycle assertions incomplete"
    );
    ensure!(
        report.reset_no_preserved
            && report.reset_yes_applied
            && report.title_config_saved
            && report.game_config_saved
            && report.config_source_commands_frozen,
        "Config journeys incomplete"
    );
    ensure!(
        report.title_manual_returned
            && report.game_manual_returned
            && report.manual_swipes == 2
            && report.manual_resize_cancelled == 2
            && report.manual_no_save_returns == 2,
        "Manual journeys incomplete"
    );
    ensure!(
        report.origin_input_equivalent
            && report.origin_tips_returned
            && report.origin_resize_cancelled
            && report.origin_title_returned
            && report.origin_no_save_return,
        "Origin/TIPS window journey incomplete"
    );
    for (source, chart) in [("Title", &report.title_chart), ("Game", &report.game_chart)] {
        let chart = chart.as_ref().context("missing Chart window report")?;
        ensure!(
            chart.ready_observed
                && chart.analog_delta_three
                && chart.physical_dpad_delta_ten
                && chart.keyboard_arrow_delta_ten
                && chart.neutral_samples_stable == 3
                && chart.shoulder_checks == 4
                && chart.source_returned
                && chart.progress_unchanged
                && chart.commands_frozen,
            "{source} Chart window journey incomplete"
        );
    }
    ensure!(
        report.cg.as_ref().is_some_and(cg_gallery::Report::complete),
        "CG window journey incomplete"
    );
    ensure!(
        report
            .music
            .as_ref()
            .is_some_and(music_gallery::Report::complete),
        "Music window journey incomplete"
    );
    ensure!(
        report
            .movie
            .as_ref()
            .is_some_and(movie_gallery::Report::complete)
            && report.movie_focus_callbacks
            && report.movie_focus_clock_frozen
            && report.movie_focus_media_frozen
            && report.movie_focus_playback_resumed
            && report.movie_focus_sample_span_micros >= 400_000
            && report.movie_focus_stable_span_micros >= 300_000,
        "Movie window/focus journey incomplete"
    );
    let bytes = std::fs::read(&path)?;
    let written = SaveData::decode(&bytes)?;
    ensure!(
        written.play_seconds >= 17 && written.settings.common.v3_sysvol == 30,
        "production close did not persist loaded progress"
    );
    eprintln!(
        "WINDOW PROBE PASS: real window/surface,720p→1080p pointer,boot/movie/title/authored message,actual focus/audio pause,held Enter quarantine,pure render,deferred close,temp save; progress={}",
        PathBuf::from(path).display()
    );
    Ok(())
}
