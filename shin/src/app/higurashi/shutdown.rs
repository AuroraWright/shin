//! Desktop close is deferred until owned writes finish. Failed writes keep the
//! scene and the same immutable snapshot alive for retry or return to play.
use super::persistence::FrozenProgress;
use crate::{
    adv::{
        higurashi::{
            input::Buttons,
            question::{self, Question, view::View},
        },
        input_area::PointerFrame,
    },
    asset::system::AssetServer,
    render::PreRenderContext,
};
use anyhow::{Context, Result};
use shin_core::time::Ticks;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

struct Writer {
    snapshot: FrozenProgress,
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Writer {
    fn new(snapshot: FrozenProgress) -> Self {
        let frozen = snapshot.clone();
        let task = shin_tasks::async_io::spawn_loader(async move { frozen.write() });
        Self {
            retirement: task.retirement_handle().unwrap(),
            task: Some(task),
            snapshot,
        }
    }
    fn poll(&mut self) -> Option<Result<()>> {
        let output = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        Some(output)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct Failure {
    snapshot: FrozenProgress,
    error: String,
    loading: Option<shin_tasks::AsyncTask<Result<(Question, View)>>>,
    retirement: shin_tasks::LoaderRetirement,
    ready: Option<(Question, View)>,
}
impl Failure {
    fn new(snapshot: FrozenProgress, error: String, server: Arc<AssetServer>) -> Self {
        let task =
            shin_tasks::async_io::spawn_loader(async move {
                View::load(&server,
                "セーブデータを保存できませんでした。@r保存先をご確認のうえ、再試行してください。",
                &["再試行", "戻る"], 0, true).await
            });
        Self {
            snapshot,
            error,
            retirement: task.retirement_handle().unwrap(),
            loading: Some(task),
            ready: None,
        }
    }
    fn poll(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Option<bool>> {
        if let Some(output) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            self.ready = Some(output.context("Loading Higurashi shutdown retry notice")?);
        }
        struct Callbacks(Option<bool>);
        impl question::Services for Callbacks {
            fn cue(&mut self, _: question::Cue) {}
            fn callback(&mut self, event: question::Callback) {
                if event.stage == 1 {
                    self.0 = Some(event.button == 0);
                }
            }
        }
        let mut callbacks = Callbacks(None);
        if let Some((model, _)) = &mut self.ready {
            model.poll(
                elapsed,
                u32::from(permission),
                buttons.pressed,
                buttons.repeat,
                pointer,
                false,
                &mut callbacks,
            );
        }
        Ok(callbacks.0)
    }
}
impl Drop for Failure {
    fn drop(&mut self) {
        if let Some(task) = self.loading.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(self.ready.take());
    }
}
#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Requested,
    Writing(Writer),
    Failure(Failure),
    Ready,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Frame {
    Continue,
    Consumed,
    Exit,
}
#[derive(Default)]
pub(super) struct Shutdown {
    phase: Phase,
    flush_input: bool,
}
impl Shutdown {
    pub fn request(&mut self) {
        if matches!(self.phase, Phase::Idle) {
            self.phase = Phase::Requested;
            self.flush_input = true;
        }
    }
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
    pub fn freezes_scene(&self) -> bool {
        matches!(
            self.phase,
            Phase::Writing(_) | Phase::Failure(_) | Phase::Ready
        )
    }
    pub fn requested(&self) -> bool {
        matches!(self.phase, Phase::Requested)
    }
    /// Observes the retained notice; never polls its loader or question.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        match &self.phase {
            Phase::Idle => None,
            Phase::Requested => Some("close_requested"),
            Phase::Writing(_) => Some("close_writing"),
            Phase::Failure(failure) => match &failure.ready {
                Some((model, _)) if model.phase() == 2 => match model.selected() {
                    0 => Some("close_recovery_retry"),
                    1 => Some("close_recovery_return"),
                    _ => None,
                },
                _ => Some("close_transition"),
            },
            Phase::Ready => Some("close_ready"),
        }
    }
    /// This call happens only after the retained scene's writer scopes retire.
    /// None means startup has not published progress; do not replace its file.
    pub fn begin(&mut self, snapshot: Option<FrozenProgress>) {
        assert!(self.requested());
        self.phase = snapshot.map_or(Phase::Ready, |snapshot| {
            Phase::Writing(Writer::new(snapshot))
        });
    }
    pub fn take_input_flush(&mut self) -> bool {
        std::mem::take(&mut self.flush_input)
    }
    #[cfg(test)]
    pub fn update(
        &mut self,
        server: &Arc<AssetServer>,
        elapsed: Ticks,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Frame> {
        self.update_with_permission(server, elapsed, true, buttons, pointer)
    }
    pub fn update_with_permission(
        &mut self,
        server: &Arc<AssetServer>,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<Frame> {
        if let Phase::Writing(writer) = &mut self.phase {
            if let Some(output) = writer.poll() {
                let next = match output {
                    Ok(()) => Phase::Ready,
                    Err(error) => {
                        let error = format!("{error:#}");
                        tracing::warn!(%error, "Higurashi shutdown save failed; keeping live scene");
                        Phase::Failure(Failure::new(writer.snapshot.clone(), error, server.clone()))
                    }
                };
                self.phase = next;
                self.flush_input = true;
            }
        }
        if let Phase::Failure(failure) = &mut self.phase {
            match failure.poll(elapsed, permission, buttons, pointer) {
                Ok(Some(retry)) => {
                    self.phase = if retry {
                        Phase::Writing(Writer::new(failure.snapshot.clone()))
                    } else {
                        Phase::Idle
                    };
                    self.flush_input = true;
                    return Ok(Frame::Consumed);
                }
                Ok(None) => {}
                Err(error) => {
                    // A broken notice resource cannot discard the live scene
                    // or pretend its failed snapshot reached storage.
                    self.phase = Phase::Idle;
                    self.flush_input = true;
                    return Err(error);
                }
            }
        }
        Ok(match self.phase {
            Phase::Idle | Phase::Requested => Frame::Continue,
            Phase::Ready => Frame::Exit,
            Phase::Writing(_) | Phase::Failure(_) => Frame::Consumed,
        })
    }
    pub fn prepare(&self, context: &mut PreRenderContext) {
        if let Phase::Failure(failure) = &self.phase {
            if let Some((model, view)) = &failure.ready {
                view.prepare(context, model);
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Phase::Failure(failure) = &self.phase {
            if let Some((model, view)) = &failure.ready {
                view.render(pass, model);
            }
        }
    }
    pub fn failure(&self) -> Option<&str> {
        if let Phase::Failure(failure) = &self.phase {
            Some(&failure.error)
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(super) fn writer_retirement(&self) -> Option<shin_tasks::LoaderRetirement> {
        if let Phase::Writing(writer) = &self.phase {
            Some(writer.retirement.clone())
        } else {
            None
        }
    }
    #[cfg(test)]
    pub(super) fn failure_phase(&self) -> Option<i32> {
        if let Phase::Failure(failure) = &self.phase {
            failure.ready.as_ref().map(|(model, _)| model.phase())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
