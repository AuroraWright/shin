//! Updated ab700/abafc SaveGame task and predicate-driven notification.
use super::{
    input::Buttons,
    question::{self, Question, view::View},
};
use crate::{adv::input_area::PointerFrame, asset::system::AssetServer, render::PreRenderContext};
use anyhow::{Result, bail};
use shin_core::time::Ticks;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

mod recovery;
#[cfg(test)]
mod recovery_tests;

/// Only the desktop storage boundary marks an IO failure as recoverable.
/// Encoding, resource and VM errors retain their original fatal behavior.
#[derive(Debug)]
pub(crate) struct WriteIoFailure(anyhow::Error);
impl WriteIoFailure {
    pub(crate) fn new(error: anyhow::Error) -> Self {
        Self(error)
    }
}
impl std::fmt::Display for WriteIoFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for WriteIoFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RecoveryPolicy {
    RetryOnly,
    RetryOrReturn,
}

pub const PROMPT: &str =
    "セーブデータを更新中です。@r電源を切ったり、ソフトを終了したりしないでください。";
pub type WriteSnapshot = Arc<dyn Fn() -> Result<()> + Send + Sync>;

#[derive(Debug)]
pub struct Model {
    phase: i32,
    pub flag: bool,
}
pub trait Services {
    fn notification(&mut self) -> Result<()>;
    fn completed(&mut self) -> Result<()>;
    fn children(&mut self) -> Result<()>;
}
impl Model {
    pub fn new(flag: u32) -> Self {
        Self {
            phase: 0,
            flag: flag & 1 != 0,
        }
    }
    pub fn phase(&self) -> i32 {
        self.phase
    }
    pub fn complete(&self, children_empty: bool) -> bool {
        children_empty && self.phase < 0
    }
    /// ababc/abafc:callback precedes this frame's child pruning/polling.
    pub fn poll(&mut self, services: &mut impl Services) -> Result<()> {
        if self.phase == 0 {
            services.notification()?;
            self.phase = i32::MAX;
        }
        if self.phase == 2 {
            services.completed()?;
            self.phase = -1;
        }
        services.children()
    }
    /// ad5a4 ignores stage0;stage1 releases the future before setting2.
    pub fn callback(&mut self, stage: u32) {
        if stage == 1 {
            self.phase = 2;
        }
    }
}
#[derive(Default)]
pub struct Predicate {
    started: bool,
}
pub trait FutureServices {
    fn start(&mut self) -> Result<()>;
    fn status(&mut self) -> Result<u32>;
}
impl Predicate {
    pub fn poll(&mut self, services: &mut impl FutureServices) -> Result<bool> {
        if !self.started {
            services.start()?;
            self.started = true;
            return Ok(false);
        }
        Ok(services.status()? == 2)
    }
}
struct DialogJob {
    task: Option<shin_tasks::AsyncTask<Result<(Question, View)>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for DialogJob {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct WriteJob {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
    output: Option<Result<()>>,
}
impl Drop for WriteJob {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
pub struct SaveGame {
    model: Model,
    server: Arc<AssetServer>,
    write: WriteSnapshot,
    dialog_job: Option<DialogJob>,
    dialog: Option<(Question, View)>,
    writer: Option<WriteJob>,
    notified: bool,
    pub id: u64,
    predicate: Predicate,
    recovery_policy: Option<RecoveryPolicy>,
    recovery: Option<recovery::Recovery>,
    cancelled: bool,
    returned: bool,
}
impl SaveGame {
    pub fn new(server: Arc<AssetServer>, write: WriteSnapshot, flag: u32, id: u64) -> Self {
        Self {
            model: Model::new(flag),
            server,
            write,
            dialog_job: None,
            dialog: None,
            writer: None,
            notified: false,
            id,
            predicate: Default::default(),
            recovery_policy: None,
            recovery: None,
            cancelled: false,
            returned: false,
        }
    }
    pub(crate) fn with_desktop_recovery(mut self, enabled: bool, policy: RecoveryPolicy) -> Self {
        self.recovery_policy = enabled.then_some(policy);
        self
    }
    pub fn complete(&self) -> bool {
        self.cancelled
            || self
                .model
                .complete(self.dialog.is_none() && self.dialog_job.is_none())
    }
    pub fn phase(&self) -> i32 {
        self.model.phase()
    }
    /// A returned IO error is terminal for this worker, even while its native
    /// notification/source remains retained. It is never a success callback.
    pub fn write_failed(&self) -> bool {
        self.writer
            .as_ref()
            .is_some_and(|writer| matches!(writer.output, Some(Err(_))))
    }
    /// A recoverable manual transaction must settle before desktop close can
    /// freeze rows. Original terminal/native failures keep their prior boundary.
    pub(crate) fn blocks_close(&self) -> bool {
        !self.complete() && (!self.write_failed() || self.recovery.is_some())
    }
    pub fn take_notification(&mut self) -> bool {
        std::mem::take(&mut self.notified)
    }
    /// Manual Save/Delete own an explicit rollback transaction. Other callers
    /// retain the success-only notification and use RetryOnly.
    pub(crate) fn take_result(&mut self) -> Option<bool> {
        if std::mem::take(&mut self.returned) {
            Some(false)
        } else if self.take_notification() {
            Some(true)
        } else {
            None
        }
    }
    pub fn cancel_input(&mut self) {
        if let Some((question, _)) = &mut self.dialog {
            question.cancel_input(&mut super::question::CancelOnly);
        }
        if let Some(recovery) = &mut self.recovery {
            recovery.cancel_input();
        }
    }
    fn poll_writer(&mut self) -> Result<bool> {
        struct Worker<'a> {
            writer: &'a mut Option<WriteJob>,
            write: &'a WriteSnapshot,
        }
        impl FutureServices for Worker<'_> {
            fn start(&mut self) -> Result<()> {
                let write = self.write.clone();
                let task = shin_tasks::async_io::spawn_loader(async move {
                    let began = Instant::now();
                    write()?;
                    if let Some(left) = Duration::from_secs(1).checked_sub(began.elapsed()) {
                        std::thread::sleep(left);
                    }
                    Ok(())
                });
                *self.writer = Some(WriteJob {
                    retirement: task.retirement_handle().unwrap(),
                    task: Some(task),
                    output: None,
                });
                Ok(())
            }
            fn status(&mut self) -> Result<u32> {
                let writer = self.writer.as_mut().unwrap();
                if writer.output.is_none() {
                    writer.output = writer.task.as_mut().unwrap().poll_naive();
                }
                match writer.output.as_ref() {
                    Some(Ok(())) => Ok(2),
                    Some(Err(error)) => bail!("Saving Higurashi progress failed:{error:#}"),
                    None => Ok(1),
                }
            }
        }
        self.predicate.poll(&mut Worker {
            writer: &mut self.writer,
            write: &self.write,
        })
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: Option<&PointerFrame<'_>>,
    ) -> Result<u32> {
        if self.cancelled {
            return Ok(0);
        }
        if let Some(recovery) = &mut self.recovery {
            let Some(decision) = recovery.poll(delta, permission, buttons, pointer)? else {
                return Ok(0);
            };
            self.recovery.take();
            self.writer.take();
            self.predicate = Default::default();
            if decision == recovery::Decision::Return {
                // Retire the notice without invoking the native success path.
                self.dialog_job.take();
                self.dialog.take();
                self.cancelled = true;
                self.returned = true;
            }
            // Retry starts on the following callback with the same immutable
            // write closure and existing native notice/model.
            return Ok(0);
        }
        struct Frame<'a, 'b> {
            owner: &'a mut SaveGame,
            delta: Ticks,
            permission: bool,
            buttons: Buttons,
            pointer: Option<&'b PointerFrame<'b>>,
        }
        impl Services for Frame<'_, '_> {
            fn notification(&mut self) -> Result<()> {
                let server = self.owner.server.clone();
                let task = shin_tasks::async_io::spawn_loader(async move {
                    View::load(&server, PROMPT, &[], 0, true).await
                });
                self.owner.dialog_job = Some(DialogJob {
                    retirement: task.retirement_handle().unwrap(),
                    task: Some(task),
                });
                Ok(())
            }
            fn completed(&mut self) -> Result<()> {
                self.owner.notified = true;
                Ok(())
            }
            fn children(&mut self) -> Result<()> {
                let owner = &mut self.owner;
                // d11c8 removes a completed notification on the next poll.
                if owner.dialog.as_ref().is_some_and(|(q, _)| q.complete()) {
                    owner.dialog.take();
                }
                if let Some(output) = owner
                    .dialog_job
                    .as_mut()
                    .and_then(|j| j.task.as_mut())
                    .and_then(|t| t.poll_naive())
                {
                    owner.dialog_job.as_mut().unwrap().task = None;
                    owner.dialog = Some(output?);
                    owner.dialog_job.take();
                }
                let Some((question, _)) = owner.dialog.as_ref() else {
                    return Ok(());
                };
                let dismiss = if question.phase() == 2 {
                    owner.poll_writer()?
                } else {
                    false
                };
                struct Callbacks {
                    values: Vec<question::Callback>,
                }
                impl question::Services for Callbacks {
                    fn cue(&mut self, _: question::Cue) {
                        unreachable!("A no-answer save notification has no cue");
                    }
                    fn callback(&mut self, value: question::Callback) {
                        self.values.push(value);
                    }
                }
                let mut callbacks = Callbacks { values: Vec::new() };
                let empty = PointerFrame {
                    number: 0,
                    time_seconds: 0.0,
                    events: &[],
                };
                owner.dialog.as_mut().unwrap().0.poll(
                    self.delta,
                    u32::from(self.permission),
                    self.buttons.pressed,
                    self.buttons.repeat,
                    self.pointer.unwrap_or(&empty),
                    dismiss,
                    &mut callbacks,
                );
                for event in callbacks.values {
                    if event.stage == 1 {
                        owner.writer.take();
                        owner.predicate = Default::default();
                    }
                    owner.model.callback(event.stage);
                }
                Ok(())
            }
        }
        // Split the task phase from its child owner;callbacks can change it.
        let flag = u32::from(self.model.flag);
        let mut model = std::mem::replace(&mut self.model, Model::new(flag));
        let result = model.poll(&mut Frame {
            owner: self,
            delta,
            permission,
            buttons,
            pointer,
        });
        if self.model.phase == 2 {
            model.callback(1);
        }
        self.model = model;
        if let Err(error) = result {
            let io_failure = self.writer.as_ref().is_some_and(|writer| {
                writer.output.as_ref().is_some_and(|output| {
                    output
                        .as_ref()
                        .is_err_and(|error| error.is::<WriteIoFailure>())
                })
            });
            if let Some(policy) = self.recovery_policy.filter(|_| io_failure) {
                let recovery = recovery::Recovery::new(
                    self.server.clone(),
                    format!("{error:#}"),
                    matches!(policy, RecoveryPolicy::RetryOrReturn),
                );
                tracing::warn!(
                    error = recovery.error(),
                    "Higurashi save failed; retaining snapshot for recovery"
                );
                self.recovery = Some(recovery);
            } else {
                return Err(error);
            }
        }
        Ok(0)
    }
    pub fn prepare(&self, context: &mut PreRenderContext) {
        if let Some((q, v)) = &self.dialog {
            v.prepare(context, q);
        }
        if let Some(recovery) = &self.recovery {
            recovery.prepare(context);
        }
    }
    pub fn render(&self, pass: &mut shin_render::render_pass::RenderPass) {
        if let Some((q, v)) = &self.dialog {
            v.render(pass, q);
        }
        if let Some(recovery) = &self.recovery {
            recovery.render(pass);
        }
    }
    pub(crate) fn diagnostic_recovery(&self) -> Option<(i32, i32, bool)> {
        self.recovery.as_ref()?.diagnostic_state()
    }
    #[cfg(test)]
    pub(crate) fn reference_recovery(&self) -> Option<(i32, i32, bool)> {
        self.diagnostic_recovery()
    }
    #[cfg(test)]
    pub(crate) fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.writer
            .iter()
            .map(|job| job.retirement.clone())
            .chain(self.dialog_job.iter().map(|job| job.retirement.clone()))
            .chain(
                self.recovery
                    .iter()
                    .map(|recovery| recovery.reference_retirement()),
            )
            .collect()
    }
}
