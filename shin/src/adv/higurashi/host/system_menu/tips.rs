//! Actual1c804 item6 question and20844 staged return-to-TIPS callback.
use super::{OwnedMenu, ReturnOnly};
use crate::adv::higurashi::{
    host::ModeServices,
    question::{self, Question, view::View},
};
use anyhow::{Result, bail};
use parking_lot::Mutex;
use shin_core::{time::Ticks, vm::command::types::Volume};
use std::sync::Arc;

pub const PROMPT: &str = "TIPSメニューに戻ります。@rよろしいですか？";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Reopen,
    Return,
}
pub(super) fn action(callback: question::Callback) -> Action {
    if callback.stage == 1 && callback.button == 0 {
        Action::Return
    } else if callback.stage == 0 && callback.button != 0 {
        Action::Reopen
    } else {
        Action::None
    }
}
pub(in crate::adv::higurashi::host) struct QuestionMode {
    parent: Arc<Mutex<OwnedMenu>>,
    assets: Arc<crate::asset::system::AssetServer>,
    worker: Option<shin_tasks::AsyncTask<Result<(Question, View)>>>,
    retirement: shin_tasks::LoaderRetirement,
    loaded: Option<(Question, View)>,
    fault: Option<String>,
}
impl Drop for QuestionMode {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel_and_retire();
        }
        self.retirement.clone().retire(self.loaded.take());
    }
}
impl QuestionMode {
    pub(in crate::adv::higurashi::host) fn cancel_input(&mut self) {
        if let Some((model, _)) = &mut self.loaded {
            model.cancel_input(&mut question::CancelOnly);
        }
    }
    pub(super) fn new(
        parent: Arc<Mutex<OwnedMenu>>,
        assets: Arc<crate::asset::system::AssetServer>,
    ) -> Self {
        let source = assets.clone();
        let worker = shin_tasks::async_io::spawn_loader(async move {
            View::load(
                &source,
                PROMPT,
                &crate::adv::higurashi::browser_lifecycle::JUMP_BUTTONS,
                1,
                true,
            )
            .await
        });
        let retirement = worker.retirement_handle().unwrap();
        Self {
            parent,
            assets,
            worker: Some(worker),
            retirement,
            loaded: None,
            fault: None,
        }
    }
    pub(in crate::adv::higurashi::host) fn completion(&self) -> u32 {
        u32::from(
            self.loaded
                .as_ref()
                .is_some_and(|(model, _)| model.complete()),
        )
    }
    pub(in crate::adv::higurashi::host) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut ModeServices<'_>,
    ) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!("Higurashi TIPS return question failed:{fault}");
        }
        if let Some(result) = self.worker.as_mut().and_then(|worker| worker.poll_naive()) {
            self.worker = None;
            match result {
                Ok(loaded) => self.loaded = Some(loaded),
                Err(error) => {
                    self.fault = Some(format!("{error:#}"));
                    return Err(error);
                }
            }
        }
        if let Some((model, _)) = &mut self.loaded {
            struct Callbacks<'a> {
                assets: &'a crate::asset::system::AssetServer,
                sounds: &'a super::GlobalSounds,
                callbacks: Vec<question::Callback>,
            }
            impl question::Services for Callbacks<'_> {
                fn cue(&mut self, cue: question::Cue) {
                    let name = match cue {
                        question::Cue::Move => "move",
                        question::Cue::Cancel => "cancel",
                        question::Cue::Confirm => "decide",
                        question::Cue::NoSelection => "error",
                    };
                    let mut sounds = self.sounds.lock();
                    sounds.update();
                    sounds.play(self.assets, name, Volume(1.0));
                }
                fn callback(&mut self, callback: question::Callback) {
                    self.callbacks.push(callback);
                }
            }
            let empty = crate::adv::input_area::PointerFrame {
                number: 0,
                time_seconds: 0.0,
                events: &[],
            };
            let mut callbacks = Callbacks {
                assets: &self.assets,
                sounds: services
                    .system_sounds
                    .expect("SystemMenu owns global TIPS cues"),
                callbacks: Vec::new(),
            };
            model.poll(
                delta,
                u32::from(permission),
                services.buttons.pressed,
                services.buttons.repeat,
                services.pointer.unwrap_or(&empty),
                false,
                &mut callbacks,
            );
            for callback in callbacks.callbacks {
                match action(callback) {
                    Action::Reopen => self.parent.lock().model.child_returned(&mut ReturnOnly),
                    Action::Return => *services.menu_tips_return = true,
                    Action::None => {}
                }
            }
        }
        Ok(0)
    }
    pub(in crate::adv::higurashi::host) fn prepare(
        &mut self,
        context: &mut crate::render::PreRenderContext,
    ) {
        if let Some((model, view)) = &self.loaded {
            view.prepare(context, model);
        }
    }
    pub(in crate::adv::higurashi::host) fn render(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
    ) {
        if let Some((model, view)) = &self.loaded {
            view.render(pass, model);
        }
    }
    #[cfg(test)]
    pub(in crate::adv::higurashi::host) fn state(&self) -> Option<(i32, i32)> {
        self.loaded
            .as_ref()
            .map(|(model, _)| (model.phase(), model.selected()))
    }
}

#[cfg(test)]
mod tests;
