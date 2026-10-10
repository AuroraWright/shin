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

    fn init(context: AppContext<Self>, parameters: Self::Parameters) -> anyhow::Result<Self>;

    fn map_canvas_size(window_size: PhysicalSize<u32>) -> ViewportParams {
        ViewportParams::with_aspect_ratio(window_size, 16.0 / 9.0)
    }

    fn custom_event(&mut self, context: AppContext<Self>, event: Self::EventType);

    /// Raw pointer and analog input in addition to the lowered actions.
    fn raw_input(&mut self, _state: &shin_input::RawInputState) {}

    /// Continue playback without focus; OS lifecycle suspension still pauses.
    fn run_in_background(&self) -> bool {
        false
    }

    /// Cancel captured input when focus leaves, including during background play.
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

pub struct WindowState {
    #[cfg(target_os = "macos")]
    quit_handler: Option<macos_quit::QuitHandler>,
    pub window: Arc<Window>,
    pub resize_source: SurfaceResizeSource,
    suspension: PlatformSuspension,
}

struct PlatformSuspension {
    focused: bool,
    lifecycle_suspended: bool,
    background_allowed: bool,
    last_update: Instant,
}
impl PlatformSuspension {
    fn new(focused: bool, now: Instant) -> Self {
        Self {
            focused,
            lifecycle_suspended: false,
            background_allowed: false,
            last_update: now,
        }
    }
    fn is_suspended(&self) -> bool {
        (!self.focused && !self.background_allowed) || self.lifecycle_suspended
    }
    fn is_background_running(&self) -> bool {
        !self.focused && !self.is_suspended()
    }

    fn change(
        &mut self,
        focused: Option<bool>,
        lifecycle: Option<bool>,
        now: Instant,
    ) -> Option<bool> {
        self.change_policy(focused, lifecycle, None, now)
    }
    fn change_policy(
        &mut self,
        focused: Option<bool>,
        lifecycle: Option<bool>,
        background_allowed: Option<bool>,
        now: Instant,
    ) -> Option<bool> {
        let previous = self.is_suspended();
        if let Some(focused) = focused {
            self.focused = focused;
        }
        if let Some(lifecycle) = lifecycle {
            self.lifecycle_suspended = lifecycle;
        }
        if let Some(background_allowed) = background_allowed {
            self.background_allowed = background_allowed;
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
        (!self.is_suspended()).then_some(elapsed)
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
    fn enabled_background_play_keeps_elapsed_time_across_focus_changes() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(true, initial);
        assert_eq!(state.change_policy(None, None, Some(true), initial), None);
        assert_eq!(
            state.change(Some(false), None, initial + Duration::from_millis(8)),
            None
        );
        assert!(state.is_background_running());
        assert_eq!(
            state.elapsed(initial + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            state.change(Some(true), None, initial + Duration::from_millis(24)),
            None
        );
        assert!(!state.is_background_running());
        assert_eq!(
            state.elapsed(initial + Duration::from_millis(32)),
            Some(Duration::from_millis(16))
        );
    }

    #[test]
    fn background_option_changes_and_lifecycle_do_not_catch_up_paused_time() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(false, initial);
        assert_eq!(
            state.change_policy(None, None, Some(true), initial + Duration::from_secs(30)),
            Some(false)
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(31)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            state.change(None, Some(true), initial + Duration::from_secs(32)),
            Some(true)
        );
        assert!(!state.is_background_running());
        assert_eq!(state.elapsed(initial + Duration::from_secs(90)), None);
        assert_eq!(
            state.change(None, Some(false), initial + Duration::from_secs(120)),
            Some(false)
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(121)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            state.change_policy(None, None, Some(false), initial + Duration::from_secs(122)),
            Some(true)
        );
        assert_eq!(state.elapsed(initial + Duration::from_secs(180)), None);
    }

    #[test]
    fn overlapping_focus_and_lifecycle_suspension_discards_background_elapsed() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(true, initial);
        assert_eq!(
            state.elapsed(initial + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            state.change(Some(false), None, initial + Duration::from_secs(1)),
            Some(true)
        );
        assert_eq!(
            state.change(None, Some(true), initial + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            state.change(Some(true), None, initial + Duration::from_secs(3)),
            None,
            "Focus alone cannot override lifecycle suspension"
        );
        assert_eq!(state.elapsed(initial + Duration::from_secs(60)), None);
        // No redraw occurs during the next sixty seconds. The same transition
        // used by the event handler must discard this interval itself.
        assert_eq!(
            state.change(None, Some(false), initial + Duration::from_secs(120)),
            Some(false)
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(120) + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
    }

    #[test]
    fn initially_unfocused_and_duplicate_events_preserve_the_right_clock() {
        let initial = Instant::now();
        let mut state = PlatformSuspension::new(false, initial);
        assert_eq!(state.elapsed(initial + Duration::from_secs(30)), None);
        assert_eq!(
            state.change(Some(true), None, initial + Duration::from_secs(60)),
            Some(false)
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(60) + Duration::from_millis(16)),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            state.change(
                Some(true),
                Some(false),
                initial + Duration::from_secs(60) + Duration::from_millis(18)
            ),
            None
        );
        assert_eq!(
            state.elapsed(initial + Duration::from_secs(60) + Duration::from_millis(20)),
            Some(Duration::from_millis(4)),
            "Duplicate active events cannot reset elapsed time"
        );
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
        let mut attributes = Window::default_attributes();

        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            use winit::dpi::{LogicalPosition, LogicalSize};
            attributes = attributes
                .with_inner_size(LogicalSize::new(1920, 1080))
                .with_maximized(false)
                .with_position(LogicalPosition::new(1080, 0));
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

        let size = A::map_canvas_size(window.inner_size());

        info!("Created a window with size: {:?}", size);

        let window_resize_source = SurfaceResizeSource::new(size);

        Self {
            #[cfg(target_os = "macos")]
            quit_handler: None,
            suspension: PlatformSuspension::new(window.has_focus(), Instant::now()),
            window,
            resize_source: window_resize_source,
        }
    }

    pub fn toggle_fullscreen(&self) {
        let window = &self.window;

        if window.fullscreen().is_some() {
            info!("Exiting fullscreen mode");
            window.set_fullscreen(None);
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
                let result = shin_render::init::init_wgpu(window, surface_resize_handle, None).await;

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
                None,
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
    winit.suspension.change_policy(
        Some(winit.window.has_focus()),
        None,
        Some(app.run_in_background()),
        Instant::now(),
    );
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
            // Deferred saves must advance even if Quit was requested unfocused.
            self.change_suspension(None, None);
            if let Some(winit) = self.winit() {
                winit.window.request_redraw();
            }
        }
    }

    fn change_suspension(&mut self, focused: Option<bool>, lifecycle: Option<bool>) {
        let background_allowed = match self {
            Self::Operational { app, .. } => app.run_in_background(),
            _ => false,
        };
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
        if let Some(suspended) = winit.suspension.change_policy(
            focused,
            lifecycle,
            Some(background_allowed),
            Instant::now(),
        ) {
            debug!(
                ?focused,
                ?lifecycle,
                background_allowed,
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
        context.winit.suspension.focused,
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
                if size.width == 0
                    || size.height == 0
                    || winit.suspension.is_suspended()
                    || winit.suspension.is_background_running()
                {
                    return;
                }

                // Acquire before advancing story/input. A transient surface
                // failure during a native fullscreen transition skips a frame
                // without dropping updates or panicking in an AppKit callback.
                let (viewport, surface_texture) = match render.surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        warn!("GPU surface ran out of memory");
                        event_loop.exit();
                        return;
                    }
                    Err(error) => {
                        debug!(?error, "Surface frame temporarily unavailable");
                        winit.window.request_redraw();
                        return;
                    }
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
                            view: &surface_texture.view,
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

                info_span!("present").in_scope(|| {
                    winit.window.pre_present_notify();
                    surface_texture.texture.present();
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

        if winit.suspension.is_background_running() {
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
                debug!(?elapsed, "Background playback tick");
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
            // tracy_client::Client::start();

            tracing_subscriber::registry()
                .with(tracing_tracy::TracyLayer::default()
                    .with_filter(tracing_subscriber::filter::filter_fn(|m| m.module_path().is_some_and(|v| v.starts_with("shin")))))
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
