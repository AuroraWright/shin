//! Retained desktop write-error choice. The Question resources, animation and
//! input semantics are native-backed; the disk-error policy is a desktop owner.
use super::{Buttons, DialogJob, Question, View, question};
use crate::{adv::input_area::PointerFrame, asset::system::AssetServer, render::PreRenderContext};
use anyhow::{Context, Result, bail, ensure};
use shin_core::time::Ticks;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

const PROMPT: &str =
    "セーブデータを保存できませんでした。@r保存先をご確認のうえ、再試行してください。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision {
    Retry,
    Return,
}

pub(super) struct Recovery {
    error: String,
    allow_return: bool,
    job: Option<DialogJob>,
    retirement: shin_tasks::LoaderRetirement,
    ready: Option<(Question, View)>,
    fault: Option<String>,
}

impl Recovery {
    pub(super) fn new(server: Arc<AssetServer>, error: String, allow_return: bool) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            let labels: &[&str] = if allow_return {
                &["再試行", "戻る"]
            } else {
                &["再試行"]
            };
            View::load(&server, PROMPT, labels, 0, allow_return).await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            error,
            allow_return,
            job: Some(DialogJob {
                task: Some(task),
                retirement: retirement.clone(),
            }),
            retirement,
            ready: None,
            fault: None,
        }
    }

    pub(super) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: Option<&PointerFrame<'_>>,
    ) -> Result<Option<Decision>> {
        if let Some(error) = &self.fault {
            bail!("Higurashi save recovery failed:{error}");
        }
        let result = self.poll_inner(delta, permission, buttons, pointer);
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }

    fn poll_inner(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: Option<&PointerFrame<'_>>,
    ) -> Result<Option<Decision>> {
        // Loader publication and accepted closing animations continue when
        // input permission is denied. Only Question owns that input gate.
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|job| job.task.as_mut())
            .and_then(|task| task.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            self.ready = Some(output.context("Loading Higurashi save recovery question")?);
        }
        struct Callbacks(Option<i32>);
        impl question::Services for Callbacks {
            // As with desktop shutdown failure, this helper owns no audio
            // service. It does not emit native successful-save callbacks.
            fn cue(&mut self, _: question::Cue) {}
            fn callback(&mut self, event: question::Callback) {
                if event.stage == 1 {
                    self.0 = Some(event.button);
                }
            }
        }
        let mut callbacks = Callbacks(None);
        let empty = PointerFrame {
            number: 0,
            time_seconds: 0.0,
            events: &[],
        };
        if let Some((model, _)) = &mut self.ready {
            model.poll(
                delta,
                u32::from(permission),
                buttons.pressed,
                buttons.repeat,
                pointer.unwrap_or(&empty),
                false,
                &mut callbacks,
            );
        }
        callbacks
            .0
            .map(|button| {
                if button == 0 {
                    Ok(Decision::Retry)
                } else {
                    ensure!(
                        self.allow_return,
                        "Retry-only recovery returned another choice"
                    );
                    Ok(Decision::Return)
                }
            })
            .transpose()
    }

    pub(super) fn prepare(&self, context: &mut PreRenderContext) {
        if let Some((model, view)) = &self.ready {
            view.prepare(context, model);
        }
    }
    pub(super) fn render(&self, pass: &mut RenderPass) {
        if let Some((model, view)) = &self.ready {
            view.render(pass, model);
        }
    }
    pub(super) fn cancel_input(&mut self) {
        if let Some((model, _)) = &mut self.ready {
            model.cancel_input(&mut question::CancelOnly);
        }
    }
    pub(super) fn error(&self) -> &str {
        &self.error
    }
    pub(super) fn diagnostic_state(&self) -> Option<(i32, i32, bool)> {
        self.ready
            .as_ref()
            .map(|(model, _)| (model.phase(), model.selected(), self.allow_return))
    }
    #[cfg(test)]
    pub(super) fn reference_state(&self) -> Option<(i32, i32, bool)> {
        self.diagnostic_state()
    }
    #[cfg(test)]
    pub(super) fn reference_retirement(&self) -> shin_tasks::LoaderRetirement {
        self.retirement.clone()
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        // First stop/join the top future. Its completed-but-unobserved output
        // and any published View must drop inside the same loader scope before
        // waiting for descendant work. DialogJob then retires an empty task.
        let output = self
            .job
            .as_mut()
            .and_then(|job| job.task.take())
            .and_then(|task| task.cancel_and_wait());
        self.retirement.clone().retire((output, self.ready.take()));
        self.job.take();
    }
}
