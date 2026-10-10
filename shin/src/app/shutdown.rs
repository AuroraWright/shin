//! Desktop window closure waits for save completion and presents write failures.
use super::{AppAction, SceneHost};
use crate::{
    adv::storage_retry_dialog::StorageRetryDialog, asset::system::AssetServer,
    render::PreRenderContext,
};
use enum_map::EnumMap;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Requested,
    Failure(Box<StorageRetryDialog>),
    Ready,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Frame {
    Continue,
    Consumed,
    Exit,
}
#[derive(Default)]
pub(super) struct Shutdown {
    phase: Phase,
}
impl Shutdown {
    pub fn request(&mut self) {
        // Repeated native close requests cannot replace a notice or accept it.
        if matches!(self.phase, Phase::Idle) {
            self.phase = Phase::Requested;
        }
    }
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
    pub fn pauses_scene(&self) -> bool {
        matches!(self.phase, Phase::Failure(_) | Phase::Ready)
    }
    pub fn update(
        &mut self,
        host: &mut SceneHost,
        server: &AssetServer,
        ctx: &mut PreRenderContext,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        pointer: glam::Vec2,
        extent: (u16, u16),
        elapsed: f64,
    ) -> anyhow::Result<Frame> {
        if matches!(self.phase, Phase::Requested) {
            // Never drain a write behind SaveGame's dialog owner: its next
            // poll would otherwise access a snapshot that shutdown removed.
            if host.has_save_task() {
                return Ok(Frame::Continue);
            }
            match host.flush_shutdown() {
                Ok(()) => {
                    self.phase = Phase::Ready;
                    return Ok(Frame::Exit);
                }
                Err(error) => {
                    tracing::warn!(%error, "Shutdown save failed; keeping window open");
                    // If UI assets cannot load, abort closure and preserve the
                    // live game; the original storage error remains in the log.
                    self.phase = Phase::Idle;
                    let dialog = StorageRetryDialog::load_shutdown_error(
                        server,
                        input[AppAction::PointerClick].is_held,
                        extent,
                    )?;
                    self.phase = Phase::Failure(Box::new(dialog));
                    host.suspend_shutdown_input(input[AppAction::PointerClick].is_held);
                }
            }
        }
        match &mut self.phase {
            Phase::Idle => Ok(Frame::Continue),
            Phase::Ready => Ok(Frame::Exit),
            Phase::Requested => Ok(Frame::Consumed),
            Phase::Failure(dialog) => {
                dialog.input(
                    pointer,
                    input[AppAction::PointerClick].is_held,
                    extent,
                    elapsed,
                    input,
                );
                let done = dialog.update(delta);
                dialog.prepare(ctx);
                // The global audio mixer is suspended while the scene is
                // frozen; discard these sounds rather than replaying them later.
                let _ = dialog.take_sounds();
                if done {
                    self.phase = if dialog.retry_requested() {
                        Phase::Requested
                    } else {
                        Phase::Idle
                    };
                    host.suspend_shutdown_input(input[AppAction::PointerClick].is_held);
                }
                // Consume final acknowledgement input as well as modal frames.
                Ok(Frame::Consumed)
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Phase::Failure(dialog) = &self.phase {
            dialog.render(pass);
        }
    }
    #[cfg(test)]
    pub fn reference_failure_is_open(&self) -> bool {
        matches!(&self.phase, Phase::Failure(dialog) if dialog.reference_is_open())
    }
    #[cfg(test)]
    pub fn reference_button(&self, index: usize) -> glam::Vec2 {
        let Phase::Failure(dialog) = &self.phase else {
            panic!("No shutdown failure notice")
        };
        dialog.reference_button_at(index)
    }
}
