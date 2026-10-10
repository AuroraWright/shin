//! Real-window regression for Retina resizing and native fullscreen/Space events.
//! No audio is produced. Run on the GUI thread with `cargo run -p shin-window
//! --example window_lifecycle -- [resize|fullscreen]`.
use enum_map::{Enum, EnumMap};
use shin_input::{Action, ActionState, RawInputState};
use shin_primitives::color::UnormColor;
use shin_render::render_pass::RenderPass;
use shin_window::{AppContext, RenderContext, ShinApp};
use std::time::{Duration, Instant};

#[derive(Enum)]
enum NoAction {
    None,
}
impl Action for NoAction {
    fn lower(_: &RawInputState) -> EnumMap<Self, bool> {
        EnumMap::default()
    }
}
struct Lifecycle {
    fullscreen: bool,
    start: Instant,
    requested: bool,
    exited_fullscreen: bool,
    large_frames: u32,
    windowed_frames: u32,
}
impl ShinApp for Lifecycle {
    type Parameters = bool;
    type EventType = ();
    type ActionType = NoAction;
    fn init(context: AppContext<Self>, fullscreen: bool) -> anyhow::Result<Self> {
        context
            .winit
            .window
            .set_title("Shin window lifecycle regression");
        context.winit.window.focus_window();
        eprintln!(
            "Supported/requested texture dimension{}",
            context.wgpu.device.limits().max_texture_dimension_2d
        );
        Ok(Self {
            fullscreen,
            start: Instant::now(),
            requested: false,
            exited_fullscreen: false,
            large_frames: 0,
            windowed_frames: 0,
        })
    }
    fn custom_event(&mut self, _: AppContext<Self>, _: ()) {}
    fn update(
        &mut self,
        context: AppContext<Self>,
        _: EnumMap<NoAction, ActionState>,
        _: Duration,
        _: &mut wgpu::CommandEncoder,
    ) {
        let elapsed = self.start.elapsed().as_secs_f32();
        let size = context.winit.window.inner_size();
        // The game uses this canvas depth target in addition to the surface
        // target; exercising it reproduces the reported canvas_ds4112 panic.
        let _ = context.render.canvas_depth_stencil_buffer.get_target_view();
        if size.width > 4096 {
            self.large_frames += 1;
        }
        if self.requested && (!self.fullscreen || self.exited_fullscreen) && size.width > 0 {
            self.windowed_frames += 1;
        }
        if elapsed > 0.5 && !self.requested {
            self.requested = true;
            if self.fullscreen {
                // This enters AppKit's native fullscreen Space, the same
                // delegate callbacks and resize sequence as the green button.
                context
                    .winit
                    .window
                    .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            } else {
                let _ = context
                    .winit
                    .window
                    .request_inner_size(winit::dpi::PhysicalSize::new(4112, 2313));
            }
        }
        if self.fullscreen && elapsed > 3.0 && !self.exited_fullscreen {
            self.exited_fullscreen = true;
            context.winit.window.set_fullscreen(None);
        }
        if elapsed > 6.0 {
            assert!(
                self.windowed_frames > 5,
                "Rendering must continue after resize/fullscreen exit"
            );
            if !self.fullscreen {
                assert!(self.large_frames > 5, "Actual4112px window must render");
            }
            eprintln!(
                "Lifecycle passed: native_fullscreen{} large_frames{} windowed_frames{}",
                self.fullscreen, self.large_frames, self.windowed_frames
            );
            context.event_loop.exit();
        }
    }
    fn render(&mut self, _: RenderContext, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLUE), Some(0), Some(1.0));
    }
}
fn main() {
    shin_window::init_tracing();
    shin_tasks::create_task_pools();
    shin_window::run_window::<Lifecycle>(std::env::args().any(|arg| arg == "fullscreen"));
}
