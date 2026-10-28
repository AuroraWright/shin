mod debug_capture;
#[cfg(target_os = "macos")]
mod macos_quit;

use std::{
    fmt::Debug,
    sync::Arc,
    time::{Duration, Instant},
};

use cfg_if::cfg_if;
use derive_where::derive_where;
use enum_map::EnumMap;
use shin_input::{Action, ActionState, ActionsState, RawInputAccumulator};
use shin_render::{
    init::{RenderResources, WgpuInitResult, WgpuResources},
    render_pass::RenderPass,
    resize::{CanvasSize, ResizeHandle, SurfaceResizeSource, SurfaceSize, ViewportParams},
    shaders::types::texture::{DepthStencilTarget, TextureTarget, TextureTargetKind},
};
use shin_tasks::AsyncTask;
use tracing::{debug, info, info_span, warn};
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopClosed, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

pub struct AppContext<'a, A: ShinApp> {
    pub event_loop: &'a ActiveEventLoop,
    pub event_loop_proxy: &'a ShinEventLoopProxy<A::EventType>,
    pub winit: &'a mut WindowState,
    pub wgpu: &'a WgpuResources,
    pub render: &'a mut RenderResources,
}

pub struct RenderContext<'a> {
    pub winit: &'a WindowState,
    pub wgpu: &'a WgpuResources,
}

pub struct AppContextOwned<A: ShinApp> {
    pub event_loop_proxy: ShinEventLoopProxy<A::EventType>,
    pub winit: WindowState,
    pub wgpu: WgpuResources,
    pub render: RenderResources,
}

pub trait ShinApp: Sized {
    type Parameters;
    type EventType: Send + Debug + 'static;
    type ActionType: Action;

    /// Handle the shortcut at key-down, including when a native transition
    /// prevents the previous key-up from reaching the frame input state.
    const FULLSCREEN_SHORTCUT: Option<KeyCode> = None;

    /// The window title: the game's own title.
    const WINDOW_TITLE: &'static str = "shin";
    /// Runtime window title, for apps whose title depends on the selected game.
    fn window_title() -> &'static str {
        Self::WINDOW_TITLE
    }

    /// Whether the new window starts in fullscreen.
    fn start_fullscreen(_parameters: &Self::Parameters) -> bool {
        false
    }

    fn init(context: AppContext<Self>, parameters: Self::Parameters) -> anyhow::Result<Self>;

    fn map_canvas_size(window_size: PhysicalSize<u32>) -> ViewportParams {
        ViewportParams::with_aspect_ratio(window_size, 16.0 / 9.0)
    }

    fn custom_event(&mut self, context: AppContext<Self>, event: Self::EventType);

    /// Raw pointer and analog input in addition to the lowered actions.
    fn raw_input(&mut self, _state: &shin_input::RawInputState) {}

    /// Cancel captured input when focus leaves; playback continues.
    fn focus_lost(&mut self) {}

    /// Freeze platform-owned playback while the effective policy is suspended.
    fn set_suspended(&mut self, _suspended: bool) {}

    /// Defer window closure while application-owned saves and notices finish.
    /// A deferred application exits through AppContext's event loop when ready.
    fn close_requested(&mut self) -> bool {
        true
    }

    fn update(
        &mut self,
        context: AppContext<Self>,
        input: EnumMap<Self::ActionType, ActionState>,
        elapsed_time: Duration,
        command_encoder: &mut wgpu::CommandEncoder,
    );
    // can't pass context here because `RenderPass` borrows a bunch of stuff from there
    // let's hope it won't be an issue ;)
    fn render(&mut self, context: RenderContext, pass: &mut RenderPass);
}

#[derive_where(Clone)]
pub struct ShinEventLoopProxy<E: Send + Debug + 'static> {
    proxy: EventLoopProxy<ShinAppEventImpl<E>>,
}

impl<E: Send + Debug + 'static> ShinEventLoopProxy<E> {
    /// Request the same deferred application close used by the native window
    /// and AppKit Quit handlers. Application-owned IO finishes before exit.
    pub fn request_close(&self) -> Result<(), EventLoopClosed<()>> {
        self.proxy
            .send_event(ShinAppEventImpl::CloseRequested)
            .map_err(|_| EventLoopClosed(()))
    }

    pub fn send_event(&self, event: E) -> Result<(), EventLoopClosed<E>> {
        self.proxy
            .send_event(ShinAppEventImpl::Custom(event))
            .map_err(|e| {
                let EventLoopClosed(ShinAppEventImpl::Custom(e)) = e else {
                    unreachable!()
                };
                EventLoopClosed(e)
            })
    }
}

/// A 1920x1080 window, shrunk to fit the monitor if needed while keeping
/// 16:9, centered on the monitor.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn centered_window(
    monitor_size: winit::dpi::PhysicalSize<u32>,
    monitor_position: winit::dpi::PhysicalPosition<i32>,
    scale_factor: f64,
) -> (
    winit::dpi::PhysicalSize<u32>,
    winit::dpi::PhysicalPosition<i32>,
) {
    // Leave room for the title bar, menu bar and dock or taskbar.
    const MAX_FRACTION: f64 = 0.9;
    let (monitor_width, monitor_height) = (monitor_size.width as f64, monitor_size.height as f64);
    let fit = (monitor_width * MAX_FRACTION / (1920.0 * scale_factor))
        .min(monitor_height * MAX_FRACTION / (1080.0 * scale_factor))
        .min(1.0);
    let width = (1920.0 * scale_factor * fit).round();
    let height = (1080.0 * scale_factor * fit).round();
    (
        winit::dpi::PhysicalSize::new(width as u32, height as u32),
        winit::dpi::PhysicalPosition::new(
            monitor_position.x + ((monitor_width - width) / 2.0) as i32,
            monitor_position.y + ((monitor_height - height) / 2.0) as i32,
        ),
    )
}

/// The monitor's work area: the part not covered by the taskbar or docked
/// app bars, in physical pixels.
#[cfg(target_os = "windows")]
fn work_area(
    monitor: &winit::monitor::MonitorHandle,
) -> Option<(
    winit::dpi::PhysicalSize<u32>,
    winit::dpi::PhysicalPosition<i32>,
)> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use winit::platform::windows::MonitorHandleExtWindows;
    // SAFETY: MONITORINFO is plain data; all zeroes is a valid value.
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    // SAFETY: winit's handle is a live HMONITOR and cbSize matches `info`.
    if unsafe { GetMonitorInfoW(monitor.hmonitor(), &mut info) } == 0 {
        return None;
    }
    let work = info.rcWork;
    Some((
        winit::dpi::PhysicalSize::new(
            (work.right - work.left).max(0) as u32,
            (work.bottom - work.top).max(0) as u32,
        ),
        winit::dpi::PhysicalPosition::new(work.left, work.top),
    ))
}

pub struct WindowState {
    #[cfg(target_os = "macos")]
    quit_handler: Option<macos_quit::QuitHandler>,
    pub window: Arc<Window>,
    pub resize_source: SurfaceResizeSource,
    suspension: PlatformSuspension,
    surface_availability: SurfaceAvailability,
}

/// Surface availability does not change focus, audio or lifecycle policy.
/// A separate probe permit lets a scheduled redraw retry once even though
/// scheduling has already moved its next deadline into the future.
#[derive(Default)]
struct SurfaceAvailability {
    next_probe: Option<Instant>,
    probe_pending: bool,
}
impl SurfaceAvailability {
    const PROBE_PERIOD: Duration = Duration::from_millis(250);

    fn is_occluded(&self) -> bool {
        self.next_probe.is_some()
    }
    fn occluded(&mut self, now: Instant) {
        self.next_probe = Some(now + Self::PROBE_PERIOD);
        self.probe_pending = false;
    }
    fn visible(&mut self) {
        *self = Self::default();
    }
    fn request_probe(&mut self, now: Instant) -> bool {
        if self.next_probe.is_some_and(|deadline| now >= deadline) {
            self.next_probe = Some(now + Self::PROBE_PERIOD);
            self.probe_pending = true;
            true
        } else {
            false
        }
    }
    fn acquire_allowed(&mut self) -> bool {
        !self.is_occluded() || std::mem::take(&mut self.probe_pending)
    }
}

/// Playback runs whether or not the window has focus; only OS lifecycle
/// suspension pauses it. Focus decides whether input reaches the game.
struct PlatformSuspension {
    focused: bool,
    /// macOS may never activate an app launched from a terminal, leaving its
    /// window unfocused until the user switches away and back. Until the
    /// window has had focus once, take input as if it had it.
    ever_focused: bool,
    lifecycle_suspended: bool,
    last_update: Instant,
}
impl PlatformSuspension {
    /// A stalled frame (the machine waking, a blocked present) counts at most
    /// this much, so fixed-step clocks do not run a long burst to catch up.
    const MAX_FRAME_TIME: Duration = Duration::from_millis(100);

    fn new(focused: bool, now: Instant) -> Self {
        Self {
            focused,
            ever_focused: true,
            lifecycle_suspended: false,
            last_update: now,
        }
    }
    /// A newly created window, which takes input until it first gains focus.
    fn launching(focused: bool, now: Instant) -> Self {
        Self {
            ever_focused: focused,
            ..Self::new(focused, now)
        }
    }
    /// Focus for input: the window is treated as focused until it first
    /// gains focus.
    fn plays_focused(&self) -> bool {
        self.focused || !self.ever_focused
    }
    fn is_suspended(&self) -> bool {
        self.lifecycle_suspended
    }
    /// A fully covered or minimized window gets no redraws: frames run on a
    /// timer and nothing is presented.
    fn updates_without_presentation(&self, surface: &SurfaceAvailability) -> bool {
        !self.is_suspended() && surface.is_occluded()
    }
    fn change(
        &mut self,
        focused: Option<bool>,
        lifecycle: Option<bool>,
        now: Instant,
    ) -> Option<bool> {
        let previous = self.is_suspended();
        if let Some(focused) = focused {
            self.focused = focused;
            self.ever_focused |= focused;
        }
        if let Some(lifecycle) = lifecycle {
            self.lifecycle_suspended = lifecycle;
        }
        let suspended = self.is_suspended();
        if previous != suspended {
            self.last_update = now;
            Some(suspended)
        } else {
            None
        }
    }
    fn elapsed(&mut self, now: Instant) -> Option<Duration> {
        let elapsed = now.duration_since(self.last_update);
        self.last_update = now;
        (!self.is_suspended()).then_some(elapsed.min(Self::MAX_FRAME_TIME))
    }
}

#[cfg(test)]
mod suspension_tests {
    use super::*;

    #[derive(enum_map::Enum)]
    enum TestAction {
        Confirm,
    }
    impl Action for TestAction {
        fn lower(raw: &shin_input::RawInputState) -> EnumMap<Self, bool> {
            use shin_input::inputs::{GamepadButton, MouseButton};
            EnumMap::from_fn(|_| {
                raw.keyboard.contains(&winit::keyboard::KeyCode::Space)
                    || raw.mouse.buttons[MouseButton::Left]
                    || raw.gamepads.is_held(GamepadButton::A)
            })
        }
    }

    #[test]
    fn background_frames_suppress_controls_and_clear_repeat_state() {
        use shin_input::inputs::{GamepadAxis, GamepadButton, MouseButton};
        let mut raw = shin_input::RawInputState::new();
        raw.keyboard.insert(winit::keyboard::KeyCode::Space);
        raw.mouse.buttons[MouseButton::Left] = true;
        raw.gamepads.unified.buttons[GamepadButton::A] = true;
        raw.gamepads.unified.axes[GamepadAxis::LeftStickX] = 1.0;
        let mut actions = ActionsState::<TestAction>::new();
        let (_, foreground) =
            frame_input(raw.clone(), &mut actions, true, Duration::from_millis(16));
        assert!(foreground[TestAction::Confirm].is_clicked);
        let (background_raw, background) =
            frame_input(raw.clone(), &mut actions, false, Duration::from_secs(1));
        assert!(!background[TestAction::Confirm].is_held);
        assert!(background_raw.keyboard.is_empty());
        assert!(!background_raw.mouse.buttons[MouseButton::Left]);
        assert!(!background_raw.gamepads.is_held(GamepadButton::A));
        assert_eq!(
            background_raw.gamepads.unified.axes[GamepadAxis::LeftStickX],
            0.0
        );
        let (_, refocused) = frame_input(raw, &mut actions, true, Duration::from_millis(16));
        assert!(refocused[TestAction::Confirm].is_clicked);
    }

    #[test]
    fn focus_changes_never_pause_playback() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(true, initial);
        assert_eq!(
            state.change(Some(false), None, initial + Duration::from_millis(8)),
            None
        );
        assert!(!state.is_suspended());
        assert!(!state.plays_focused(), "Input stops");
        assert_eq!(
            state.elapsed(initial + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            state.change(Some(true), None, initial + Duration::from_millis(24)),
            None
        );
        assert!(state.plays_focused());
        assert_eq!(
            state.elapsed(initial + Duration::from_millis(32)),
            Some(Duration::from_millis(16))
        );
    }

    #[test]
    fn lifecycle_suspension_discards_paused_time() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(false, initial);
        assert_eq!(
            state.change(None, Some(true), initial + Duration::from_secs(1)),
            Some(true)
        );
        assert_eq!(
            state.change(Some(true), None, initial + Duration::from_secs(2)),
            None,
            "Focus cannot override lifecycle suspension"
        );
        assert_eq!(state.elapsed(initial + Duration::from_secs(60)), None);
        // No redraw occurs meanwhile; the transition itself restarts the clock.
        assert_eq!(
            state.change(None, Some(false), initial + Duration::from_secs(120)),
            Some(false)
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(120) + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            state.change(None, Some(false), initial + Duration::from_secs(121)),
            None,
            "Duplicate active events cannot reset elapsed time"
        );
    }

    #[test]
    fn stalled_frames_count_at_most_100_ms() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(true, initial);
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(5)),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(5) + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
    }
}

#[cfg(test)]
mod surface_availability_tests {
    use super::*;

    #[test]
    fn occlusion_probes_are_bounded_and_survive_delayed_redraw_delivery() {
        let initial = Instant::now();
        let mut surface = SurfaceAvailability::default();
        assert!(surface.acquire_allowed());
        surface.occluded(initial);
        for ms in 0..250 {
            assert!(!surface.request_probe(initial + Duration::from_millis(ms)));
            assert!(!surface.acquire_allowed());
        }
        assert!(surface.request_probe(initial + Duration::from_millis(250)));
        // A delayed/missing redraw cannot turn the deadline into a hot loop.
        assert!(!surface.request_probe(initial + Duration::from_millis(499)));
        assert!(surface.request_probe(initial + Duration::from_millis(500)));
        assert!(
            surface.acquire_allowed(),
            "The scheduled probe gets one permit"
        );
        assert!(
            !surface.acquire_allowed(),
            "Unsolicited redraws remain blocked"
        );
        surface.occluded(initial + Duration::from_millis(510));
        assert!(!surface.request_probe(initial + Duration::from_millis(759)));
        assert!(surface.request_probe(initial + Duration::from_millis(760)));
        assert!(surface.acquire_allowed());
        // Successful acquisition and actual visibility events use this same
        // clear operation, so missing/reordered events cannot stick playback.
        surface.visible();
        assert!(!surface.is_occluded());
        assert!(surface.acquire_allowed());
        assert!(!surface.request_probe(initial + Duration::from_secs(10)));
    }

    #[test]
    fn window_is_centered_and_fits_the_monitor() {
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        // 4K at 2x: 1920x1080 points does not fit, so shrink to 90% height.
        let (size, position) = centered_window(
            PhysicalSize::new(3840, 2160),
            PhysicalPosition::new(0, 0),
            2.0,
        );
        assert_eq!(size, PhysicalSize::new(3456, 1944));
        assert_eq!(position, PhysicalPosition::new(192, 108));
        // A large 1x monitor keeps the full size, offset by the monitor origin.
        let (size, position) = centered_window(
            PhysicalSize::new(2560, 1440),
            PhysicalPosition::new(-2560, 0),
            1.0,
        );
        assert_eq!(size, PhysicalSize::new(1920, 1080));
        assert_eq!(position, PhysicalPosition::new(-2240, 180));
    }

    #[test]
    fn unactivated_launch_takes_input_until_first_focus() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::launching(false, initial);
        assert!(!state.is_suspended());
        assert!(state.plays_focused(), "Accepts input");
        state.change(Some(false), None, initial + Duration::from_millis(8));
        assert!(state.plays_focused());
        state.change(Some(true), None, initial + Duration::from_secs(1));
        state.change(Some(false), None, initial + Duration::from_secs(2));
        assert!(!state.plays_focused());
        assert!(!state.is_suspended(), "Playback continues");
    }

    #[test]
    fn occluded_windows_update_without_presenting_unless_suspended() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(true, initial);
        let mut surface = SurfaceAvailability::default();
        assert!(!state.updates_without_presentation(&surface));
        surface.occluded(initial);
        assert!(state.updates_without_presentation(&surface));
        state.change(Some(false), None, initial + Duration::from_secs(1));
        assert!(
            state.updates_without_presentation(&surface),
            "Focus does not matter"
        );
        state.change(None, Some(true), initial + Duration::from_secs(2));
        assert!(
            !state.updates_without_presentation(&surface),
            "Lifecycle suspension wins"
        );
        state.change(None, Some(false), initial + Duration::from_secs(3));
        surface.visible();
        assert!(!state.updates_without_presentation(&surface));
    }
}

fn fullscreen_shortcut_pressed(
    shortcut: Option<KeyCode>,
    physical_key: PhysicalKey,
    state: ElementState,
    repeat: bool,
    synthetic: bool,
) -> bool {
    !synthetic
        && !repeat
        && state == ElementState::Pressed
        && shortcut.is_some_and(|key| physical_key == PhysicalKey::Code(key))
}

#[cfg(test)]
mod fullscreen_shortcut_tests {
    use super::*;

    #[test]
    fn missing_release_during_transition_does_not_block_the_next_press() {
        let f = PhysicalKey::Code(KeyCode::KeyF);
        // AppKit can omit the release during the first native transition. A
        // fresh nonrepeat down must still toggle, while held repeats must not.
        let events = [
            (f, ElementState::Pressed, false, false),
            (f, ElementState::Pressed, true, false),
            (f, ElementState::Pressed, false, false),
            (f, ElementState::Released, false, false),
            (f, ElementState::Pressed, false, true),
            (
                PhysicalKey::Code(KeyCode::F11),
                ElementState::Pressed,
                false,
                false,
            ),
        ];
        let toggles: Vec<_> = events
            .into_iter()
            .enumerate()
            .filter_map(|(index, (key, state, repeat, synthetic))| {
                fullscreen_shortcut_pressed(Some(KeyCode::KeyF), key, state, repeat, synthetic)
                    .then_some(index)
            })
            .collect();
        assert_eq!(toggles, [0, 2]);
        assert!(!fullscreen_shortcut_pressed(
            None,
            f,
            ElementState::Pressed,
            false,
            false
        ));
    }
}

impl WindowState {
    pub fn new<A: ShinApp>(event_loop: &ActiveEventLoop) -> Self {
        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes().with_title(A::window_title());
        #[cfg(target_os = "windows")]
        let mut centering_area = None;

        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            use winit::dpi::LogicalSize;
            attributes = attributes
                .with_inner_size(LogicalSize::new(1920, 1080))
                .with_maximized(false);
            let monitor = event_loop
                .primary_monitor()
                .or_else(|| event_loop.available_monitors().next());
            if let Some(monitor) = monitor {
                #[cfg(target_os = "windows")]
                let area = work_area(&monitor);
                #[cfg(not(target_os = "windows"))]
                let area = None;
                let (area_size, area_position) =
                    area.unwrap_or_else(|| (monitor.size(), monitor.position()));
                let (size, position) =
                    centered_window(area_size, area_position, monitor.scale_factor());
                attributes = attributes.with_inner_size(size).with_position(position);
                #[cfg(target_os = "windows")]
                {
                    // Shown once the frame is centered below.
                    attributes = attributes.with_visible(false);
                    centering_area = Some((area_size, area_position));
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            use anyhow::Context;
            use web_sys::wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes = web_sys::window()
                .context("Couldn't get window")
                .and_then(|win| win.document().context("Couldn't get document"))
                .and_then(|doc| {
                    let canvas = doc
                        .get_element_by_id("canvas")
                        .context("Couldn't find the canvas element element")?
                        .dyn_into::<web_sys::HtmlCanvasElement>()
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Couldn't cast the element to a canvas element: {:?}",
                                e
                            )
                        })
                        .context("Couldn't cast the element to a canvas element")?;

                    Ok(attributes.with_canvas(Some(canvas)))
                })
                .expect("Couldn't attach canvas to winit window");
        }

        let window = Arc::new(event_loop.create_window(attributes).unwrap());
        // The position places the frame's corner, so center the whole frame
        // (title bar and borders) now that its size is known.
        #[cfg(target_os = "windows")]
        if let Some((area_size, area_position)) = centering_area {
            let outer = window.outer_size();
            let offset = |area: u32, outer: u32| ((area as i32 - outer as i32) / 2).max(0);
            window.set_outer_position(winit::dpi::PhysicalPosition::new(
                area_position.x + offset(area_size.width, outer.width),
                area_position.y + offset(area_size.height, outer.height),
            ));
            window.set_visible(true);
        }

        let size = A::map_canvas_size(window.inner_size());

        info!("Created a window with size: {:?}", size);
        // A launch from a terminal does not always activate the app.
        #[cfg(target_os = "macos")]
        window.focus_window();
        if debug_capture::keep_rendering() {
            debug_capture::disable_app_nap();
        }

        let window_resize_source = SurfaceResizeSource::new(size);

        Self {
            #[cfg(target_os = "macos")]
            quit_handler: None,
            suspension: PlatformSuspension::launching(
                window.has_focus(),
                Instant::now(),
            ),
            surface_availability: SurfaceAvailability::default(),
            window,
            resize_source: window_resize_source,
        }
    }

    pub fn toggle_fullscreen(&self) {
        let window = &self.window;

        if window.fullscreen().is_some() {
            info!("Exiting fullscreen mode");
            window.set_fullscreen(None);
        } else if cfg!(target_os = "macos") {
            // Match the green window button: AppKit owns the fullscreen Space
            // and restores its window frame without changing the display mode.
            info!("Attempting to enter native macOS fullscreen mode");
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        } else if let Some(monitor) = window.current_monitor() {
            if let Some(video_mode) = monitor.video_modes().next() {
                info!(
                    "Attempting to enter exclusive fullscreen mode {}",
                    video_mode
                );
                window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
            }
            if window.fullscreen().is_none() {
                info!(
                    "Attempting to enter non-exclusive fullscreen mode on {}",
                    monitor
                        .name()
                        .unwrap_or_else(|| "unknown monitor".to_string())
                );
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(monitor))));
            }
        }
    }
}

#[derive(Debug)]
enum ShinAppEventImpl<E> {
    #[allow(dead_code)]
    WgpuInitDone(anyhow::Result<WgpuInitResult<'static>>),
    CloseRequested,
    Custom(E),
}

#[inline]
fn start_wgpu_init<A: ShinApp>(
    #[allow(unused)] event_loop: &ActiveEventLoop,
    proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
    raw_input_state: RawInputAccumulator,
    params: A::Parameters,
    winit: WindowState,
    // window: Arc<Window>,
    // surface_resize_handle: ResizeHandle<SurfaceSize>,
) -> WinitAppState<A> {
    let window = winit.window.clone();
    let surface_resize_handle = winit.resize_source.handle();

    cfg_if! {
        if #[cfg(not(any(windows, target_os = "macos")))] {
            // async init
            // this is not necessarily required anywhere besides wasm (we can't do sync init there)
            // however if we have to choose async init is a bit nicer because we can continue handling events
            let proxy_clone = proxy.clone();
            let task = shin_tasks::async_io::spawn(async move {
                let result = shin_render::init::init_wgpu(window, surface_resize_handle).await;

                proxy_clone
                    .send_event(ShinAppEventImpl::WgpuInitDone(result))
                    .unwrap();
            });

            WinitAppState::WaitingForWgpuInit {
                proxy,
                raw_input_state,
                params,
                winit,
                task,
            }
        } else {
            // sync init
            // Windows and macOS require window-handle access on the main
            // thread. Winit's AppKit handle returns Unavailable from a worker,
            // so surface creation must happen in this event-loop callback.
            let result = shin_tasks::block_on(shin_render::init::init_wgpu(
                window,
                surface_resize_handle,
            ));
            finish_wgpu_init(event_loop, proxy, raw_input_state, params, winit, result)
        }
    }
}

#[inline]
fn finish_wgpu_init<A: ShinApp>(
    event_loop: &ActiveEventLoop,
    proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
    raw_input_state: RawInputAccumulator,
    params: A::Parameters,
    mut winit: WindowState,
    result: anyhow::Result<WgpuInitResult<'static>>,
) -> WinitAppState<A> {
    let wgpu = result.expect("wgpu init failed");
    info!("GPU initialization completed; initializing application resources");

    // finish render init
    // the depth stencil should be the same size as the surface, even though it's a bit wasteful
    let (wgpu, mut render) = wgpu.into_resources(
        winit.resize_source.handle::<SurfaceSize>(),
        winit.resize_source.handle::<CanvasSize>(),
    );

    let shin_proxy = ShinEventLoopProxy {
        proxy: proxy.clone(),
    };

    let context = AppContext {
        event_loop,
        event_loop_proxy: &shin_proxy,
        winit: &mut winit,
        wgpu: &wgpu,
        render: &mut render,
    };

    let mut app = A::init(context, params)
        // TODO: report this error to the user better than just panicking
        .expect("App initialization failed");
    // AppKit may make the window key during synchronous GPU/asset setup.
    // The construction-time snapshot must not outlive initialization.
    winit
        .suspension
        .change(Some(winit.window.has_focus()), None, Instant::now());
    info!(
        focused = winit.window.has_focus(),
        suspended = winit.suspension.is_suspended(),
        "Window playback state after initialization"
    );
    app.set_suspended(winit.suspension.is_suspended());
    winit.suspension.last_update = Instant::now();

    let context = AppContextOwned {
        event_loop_proxy: shin_proxy,
        winit,
        wgpu,
        render,
    };

    WinitAppState::Operational {
        proxy,
        raw_input_state,
        input_state: ActionsState::new(),
        context,
        app,
    }
}

#[derive(Default)]
enum WinitAppState<A: ShinApp> {
    WaitingForInitialResume {
        // unfortunately we have to weave in the proxy from outside of the loop until winit 0.31: https://github.com/rust-windowing/winit/pull/3764
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
    },
    /// On web platform the initial winit canvas size is zero, so we have to wait for a non-zero size
    WaitingForNonzeroSize {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
        winit: WindowState,
        surface_resize_handle: ResizeHandle<SurfaceSize>,
    },
    #[allow(dead_code)]
    WaitingForWgpuInit {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
        winit: WindowState,
        #[allow(unused)] // this is just to keep the task alive
        task: AsyncTask<()>,
    },
    Operational {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        input_state: ActionsState<A::ActionType>,
        context: AppContextOwned<A>,
        app: A,
    },
    #[default]
    Poison,
}

impl<A: ShinApp> WinitAppState<A> {
    fn new(proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>, params: A::Parameters) -> Self {
        Self::WaitingForInitialResume {
            proxy,
            params,
            raw_input_state: RawInputAccumulator::new(),
        }
    }

    pub fn input_mut(&mut self) -> &mut RawInputAccumulator {
        match self {
            WinitAppState::WaitingForInitialResume {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::WaitingForNonzeroSize {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::WaitingForWgpuInit {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::Operational {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::Poison => {
                unreachable!()
            }
        }
    }

    pub fn winit(&self) -> Option<&WindowState> {
        match self {
            WinitAppState::WaitingForInitialResume { .. } => None,
            WinitAppState::WaitingForNonzeroSize { winit, .. } => Some(winit),
            WinitAppState::WaitingForWgpuInit { winit, .. } => Some(winit),
            WinitAppState::Operational { context, .. } => Some(&context.winit),
            WinitAppState::Poison => {
                unreachable!()
            }
        }
    }
}

impl<A: ShinApp> WinitAppState<A> {
    fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        let close = match self {
            Self::Operational { app, .. } => app.close_requested(),
            _ => true,
        };
        if close {
            event_loop.exit();
        } else {
            if let Some(winit) = self.winit() {
                winit.window.request_redraw();
            }
        }
    }

    fn change_suspension(&mut self, focused: Option<bool>, lifecycle: Option<bool>) {
        if let Self::Operational {
            context,
            app,
            input_state,
            ..
        } = self
        {
            if focused == Some(false) && context.winit.suspension.focused {
                app.focus_lost();
                *input_state = ActionsState::new();
            }
        }
        let winit = match self {
            Self::WaitingForInitialResume { .. } | Self::Poison => return,
            Self::WaitingForNonzeroSize { winit, .. } | Self::WaitingForWgpuInit { winit, .. } => {
                winit
            }
            Self::Operational { context, .. } => &mut context.winit,
        };
        if let Some(suspended) = winit.suspension.change(focused, lifecycle, Instant::now()) {
            debug!(
                ?focused,
                ?lifecycle,
                suspended,
                "Platform playback suspension changed"
            );
            if !suspended {
                winit.window.request_redraw();
            }
            if let Self::Operational { app, .. } = self {
                app.set_suspended(suspended);
            }
        }
    }
}

fn advance_application<A: ShinApp>(
    app: &mut A,
    context: AppContext<A>,
    raw_input: &mut RawInputAccumulator,
    actions: &mut ActionsState<A::ActionType>,
    elapsed: Duration,
) -> wgpu::CommandEncoder {
    let (raw, input) = frame_input::<A::ActionType>(
        raw_input.start_frame(),
        actions,
        context.winit.suspension.plays_focused(),
        elapsed,
    );
    app.raw_input(&raw);
    let mut encoder = context
        .wgpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Update"),
        });
    info_span!("update").in_scope(|| app.update(context, input, elapsed, &mut encoder));
    raw_input.finish_frame();
    encoder
}

fn frame_input<T: Action>(
    raw: shin_input::RawInputState,
    actions: &mut ActionsState<T>,
    focused: bool,
    elapsed: Duration,
) -> (shin_input::RawInputState, EnumMap<T, ActionState>) {
    if focused {
        let input = actions.update(T::lower(&raw), elapsed);
        (raw, input)
    } else {
        *actions = ActionsState::new();
        (shin_input::RawInputState::new(), EnumMap::default())
    }
}

impl<A: ShinApp> ApplicationHandler<ShinAppEventImpl<A::EventType>> for WinitAppState<A> {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.change_suspension(None, Some(true));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.change_suspension(None, Some(false));
        match std::mem::take(self) {
            WinitAppState::WaitingForInitialResume {
                proxy,
                raw_input_state: input_state,
                params,
            } => {
                #[allow(unused_mut)]
                let mut winit = WindowState::new::<A>(event_loop);
                if A::start_fullscreen(&params) {
                    winit.toggle_fullscreen();
                }
                #[cfg(target_os = "macos")]
                {
                    let quit_proxy = proxy.clone();
                    winit.quit_handler = Some(
                        macos_quit::install(move || {
                            quit_proxy
                                .send_event(ShinAppEventImpl::CloseRequested)
                                .is_ok()
                        })
                        .expect("Failed to register deferred AppKit Quit"),
                    );
                }

                let mut surface_resize_handle = winit.resize_source.surface_handle();
                let current_size = surface_resize_handle.get();

                if current_size != SurfaceSize::default() {
                    *self = start_wgpu_init::<A>(event_loop, proxy, input_state, params, winit);
                } else {
                    *self = WinitAppState::WaitingForNonzeroSize {
                        proxy,
                        raw_input_state: input_state,
                        params,
                        winit,
                        surface_resize_handle,
                    };
                }
            }
            WinitAppState::WaitingForNonzeroSize {
                proxy,
                raw_input_state,
                params,
                winit,
                mut surface_resize_handle,
            } => {
                // Some platforms restore the extent on resume before delivering
                // a resize event. Reuse the window and refresh its current size.
                winit
                    .resize_source
                    .resize(A::map_canvas_size(winit.window.inner_size()));
                if surface_resize_handle.get() != SurfaceSize::default() {
                    *self = start_wgpu_init::<A>(event_loop, proxy, raw_input_state, params, winit);
                } else {
                    *self = WinitAppState::WaitingForNonzeroSize {
                        proxy,
                        raw_input_state,
                        params,
                        winit,
                        surface_resize_handle,
                    };
                }
            }
            waiting @ WinitAppState::WaitingForWgpuInit { .. } => {
                // Preserve the already running task; WgpuInitDone publishes it.
                *self = waiting;
            }
            WinitAppState::Operational {
                proxy,
                raw_input_state,
                input_state,
                mut context,
                app,
            } => {
                context.render.surface = shin_render::init::surface_reinit(
                    &context.wgpu.instance,
                    context.wgpu.device.clone(),
                    context.winit.window.clone(),
                    context.winit.resize_source.handle::<SurfaceSize>(),
                    context.render.surface_texture_format,
                )
                .expect("surface reinit failed");

                *self = WinitAppState::Operational {
                    proxy,
                    raw_input_state,
                    input_state,
                    context,
                    app,
                };
            }
            WinitAppState::Poison => unreachable!(),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ShinAppEventImpl<A::EventType>) {
        match event {
            ShinAppEventImpl::CloseRequested => self.request_close(event_loop),
            ShinAppEventImpl::WgpuInitDone(result) => {
                let WinitAppState::WaitingForWgpuInit {
                    proxy,
                    raw_input_state,
                    params,
                    winit,
                    task: _,
                } = std::mem::take(self)
                else {
                    unreachable!()
                };
                *self = finish_wgpu_init::<A>(
                    event_loop,
                    proxy,
                    raw_input_state,
                    params,
                    winit,
                    result,
                );
            }
            ShinAppEventImpl::Custom(e) => {
                let WinitAppState::Operational {
                    proxy: _,
                    raw_input_state: _,
                    input_state: _,
                    context:
                        AppContextOwned {
                            event_loop_proxy,
                            winit,
                            wgpu,
                            render,
                        },
                    app,
                } = self
                else {
                    warn!("Received custom event before app was initialized");
                    return;
                };

                app.custom_event(
                    AppContext {
                        event_loop,
                        event_loop_proxy,
                        wgpu,
                        winit,
                        render,
                    },
                    e,
                );
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let scale_factor = self
            .winit()
            .map_or(1.0, |winit| winit.window.scale_factor());
        self.input_mut().set_mouse_scale_factor(scale_factor);
        self.input_mut().on_winit_event(&event);

        let Some(winit) = self.winit() else {
            return;
        };

        if window_id != winit.window.id() {
            return;
        }

        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if matches!(self, Self::Operational { .. })
                && winit.window.has_focus()
                && fullscreen_shortcut_pressed(
                    A::FULLSCREEN_SHORTCUT,
                    event.physical_key,
                    event.state,
                    event.repeat,
                    is_synthetic,
                ) =>
            {
                winit.toggle_fullscreen();
            }
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Focused(focused) => self.change_suspension(Some(focused), None),
            WindowEvent::Occluded(occluded) => {
                let winit = match self {
                    Self::WaitingForInitialResume { .. } | Self::Poison => return,
                    Self::WaitingForNonzeroSize { winit, .. }
                    | Self::WaitingForWgpuInit { winit, .. } => winit,
                    Self::Operational { context, .. } => &mut context.winit,
                };
                if occluded {
                    if !winit.surface_availability.is_occluded() {
                        winit.surface_availability.occluded(Instant::now());
                    }
                } else {
                    winit.surface_availability.visible();
                    winit.window.request_redraw();
                }
            }
            WindowEvent::Resized(physical_size) => {
                // Retain a valid offscreen canvas when a minimized window
                // reports zero extent; background updates can still use it.
                if physical_size.width > 0 && physical_size.height > 0 {
                    winit
                        .resize_source
                        .resize(A::map_canvas_size(physical_size));
                }
                winit.window.request_redraw();

                // this is a mouthful...
                match std::mem::take(self) {
                    WinitAppState::WaitingForNonzeroSize {
                        proxy,
                        raw_input_state: input_state,
                        params,
                        mut surface_resize_handle,
                        winit,
                    } => {
                        if surface_resize_handle.get() != SurfaceSize::default() {
                            *self =
                                start_wgpu_init::<A>(event_loop, proxy, input_state, params, winit);
                        } else {
                            *self = WinitAppState::WaitingForNonzeroSize {
                                proxy,
                                raw_input_state: input_state,
                                params,
                                winit,
                                surface_resize_handle,
                            };
                        }
                    }
                    state => *self = state,
                }
            }
            WindowEvent::RedrawRequested => {
                let _span = info_span!("redraw").entered();

                // AppKit's key-window query is authoritative even if the
                // initial Focused event was queued before Operational state.
                #[cfg(target_os = "macos")]
                self.change_suspension(self.winit().map(|winit| winit.window.has_focus()), None);

                let WinitAppState::Operational {
                    proxy: _,
                    raw_input_state,
                    input_state,
                    context:
                        AppContextOwned {
                            event_loop_proxy,
                            winit,
                            wgpu,
                            render,
                        },
                    app,
                } = self
                else {
                    return;
                };

                let size = winit.window.inner_size();
                if size.width == 0 || size.height == 0 || winit.suspension.is_suspended() {
                    return;
                }
                let keep_rendering = debug_capture::keep_rendering();
                let may_acquire = winit.surface_availability.acquire_allowed();
                if !may_acquire && !keep_rendering {
                    return;
                }

                // Acquire before advancing story/input. A transient surface
                // failure during a native fullscreen transition skips a frame
                // without dropping updates or panicking in an AppKit callback.
                let acquired = if !may_acquire {
                    None
                } else {
                    match render.surface.get_current_texture() {
                        Ok(frame) => {
                            winit.surface_availability.visible();
                            Some(frame)
                        }
                        Err(shin_render::init::SurfaceError::Occluded) => {
                            winit.surface_availability.occluded(Instant::now());
                            debug!("Surface occluded; waiting for a bounded presentation probe");
                            None
                        }
                        Err(error) => {
                            debug!(?error, "Surface frame temporarily unavailable");
                            if winit.surface_availability.is_occluded() {
                                winit.surface_availability.occluded(Instant::now());
                            } else if !keep_rendering {
                                winit.window.request_redraw();
                            }
                            None
                        }
                    }
                };
                // Debug playtests render frames that cannot be presented
                // into an offscreen texture instead of skipping them.
                let (viewport, surface_texture, texture, view) = match acquired {
                    Some((viewport, frame)) => {
                        let texture = frame.texture.texture.clone();
                        let view = frame.view.clone();
                        (viewport, Some(frame.texture), texture, view)
                    }
                    None if keep_rendering => {
                        if !winit.surface_availability.is_occluded() {
                            winit.surface_availability.occluded(Instant::now());
                        }
                        let Some((viewport, texture, view)) = render.surface.offscreen_frame()
                        else {
                            return;
                        };
                        (viewport, None, texture, view)
                    }
                    None => return,
                };

                let Some(elapsed) = winit.suspension.elapsed(Instant::now()) else {
                    return;
                };

                let update_encoder = advance_application(
                    app,
                    AppContext {
                        event_loop,
                        event_loop_proxy,
                        winit,
                        wgpu,
                        render,
                    },
                    raw_input_state,
                    input_state,
                    elapsed,
                );

                let mut render_encoder =
                    wgpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Render"),
                        });

                info_span!("render").in_scope(|| {
                    let mut pass = RenderPass::new(
                        &mut render.pipelines,
                        &mut render.dynamic_buffer,
                        &render.sampler_store,
                        &wgpu.device,
                        &mut render_encoder,
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(DepthStencilTarget {
                            view: render.surface_depth_stencil_buffer.resize_and_get_view(),
                        }),
                        Some(viewport),
                        "app",
                    );

                    let context = RenderContext { winit, wgpu };

                    app.render(context, &mut pass);
                });

                let capture = debug_capture::due()
                    .then(|| debug_capture::copy(&wgpu.device, &mut render_encoder, &texture));

                info_span!("submit").in_scope(|| {
                    let mut dynamic_buffer_encoder =
                        wgpu.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("DynamicBuffer"),
                            });
                    render.dynamic_buffer.finish(&mut dynamic_buffer_encoder);

                    wgpu.queue.submit([
                        dynamic_buffer_encoder.finish(),
                        update_encoder.finish(),
                        render_encoder.finish(),
                    ]);
                    render.dynamic_buffer.recall();
                });

                if let Some(capture) = capture {
                    debug_capture::write(&wgpu.device, capture);
                }

                let Some(surface_texture) = surface_texture else {
                    // Offscreen frames are paced by about_to_wait.
                    tracy_client::frame_mark();
                    return;
                };
                info_span!("present").in_scope(|| {
                    winit.window.pre_present_notify();
                    wgpu.queue.present(surface_texture);
                });

                tracy_client::frame_mark();

                winit.window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        _event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        #[cfg(target_os = "macos")]
        let focused = self.winit().map(|winit| winit.window.has_focus());
        #[cfg(not(target_os = "macos"))]
        let focused = None;
        self.change_suspension(focused, None);

        let Self::Operational {
            context:
                AppContextOwned {
                    event_loop_proxy,
                    winit,
                    wgpu,
                    render,
                },
            app,
            input_state,
            raw_input_state,
            ..
        } = self
        else {
            return;
        };

        if debug_capture::keep_rendering()
            && !winit.suspension.is_suspended()
            && winit.surface_availability.is_occluded()
        {
            // Occluded playtest frames render offscreen at 60 Hz.
            const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 60);
            let now = Instant::now();
            winit.surface_availability.request_probe(now);
            if now.duration_since(winit.suspension.last_update) >= PERIOD {
                winit.window.request_redraw();
            }
            _event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                winit.suspension.last_update + PERIOD,
            ));
        } else if winit
            .suspension
            .updates_without_presentation(&winit.surface_availability)
        {
            // Hidden/occluded windows may receive no redraw events. Advance
            // playback without acquiring or presenting the window surface.
            const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 60);
            let now = Instant::now();
            if now.duration_since(winit.suspension.last_update) >= PERIOD {
                let elapsed = winit.suspension.elapsed(now).unwrap();
                let update = advance_application(
                    app,
                    AppContext {
                        event_loop: _event_loop,
                        event_loop_proxy,
                        winit,
                        wgpu,
                        render,
                    },
                    raw_input_state,
                    input_state,
                    elapsed,
                );
                let mut dynamic =
                    wgpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("BackgroundDynamicBuffer"),
                        });
                render.dynamic_buffer.finish(&mut dynamic);
                wgpu.queue.submit([dynamic.finish(), update.finish()]);
                render.dynamic_buffer.recall();
                debug!(?elapsed, "Occluded playback tick");
            }
            if winit.surface_availability.request_probe(Instant::now()) {
                winit.window.request_redraw();
            }
            _event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                winit.suspension.last_update + PERIOD,
            ));
        } else if !winit.suspension.is_suspended() {
            winit.window.request_redraw();
        }
    }
}

pub fn init_tracing() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            console_error_panic_hook::set_once();
            tracing_wasm::set_as_global_default();
        } else {
            // Without the `tracy` feature the layer would still create every
            // shin span, per-draw ones included, for a profiler that is absent.
            #[cfg(feature = "tracy")]
            let tracy = Some(tracing_tracy::TracyLayer::default()
                .with_filter(tracing_subscriber::filter::filter_fn(|m| m.module_path().is_some_and(|v| v.starts_with("shin")))));
            #[cfg(not(feature = "tracy"))]
            let tracy: Option<tracing_subscriber::layer::Identity> = None;

            tracing_subscriber::registry()
                .with(tracy)
                .with(tracing_subscriber::fmt::layer()
                    .with_filter(tracing_subscriber::EnvFilter::builder().from_env_lossy())
                )
            .init();
        }
    }
}

pub fn run_window<A: ShinApp>(
    parameters: A::Parameters,
    #[cfg(target_os = "android")] android_app: winit::platform::android::activity::AndroidApp,
) {
    #[allow(unused_mut)]
    let mut event_loop_builder = EventLoop::<ShinAppEventImpl<A::EventType>>::with_user_event();

    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        event_loop_builder.with_activate_ignoring_other_apps(true);
    }

    #[cfg(target_os = "android")]
    {
        event_loop_builder = event_loop_builder.android_app(android_app);
    }

    let event_loop = event_loop_builder.build().unwrap();
    let proxy = event_loop.create_proxy();

    event_loop
        .run_app(&mut WinitAppState::<A>::new(proxy, parameters))
        .unwrap();
}
