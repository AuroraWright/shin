//! Updated1caac/20790 Title and1cb74/208f8 Kakera confirmation owners.
use super::{OwnedMenu, ReturnOnly};
use crate::adv::higurashi::{
    host::ModeServices,
    question::{self, Question, view::View},
};
use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{time::Ticks, vm::command::types::Volume};
use std::sync::Arc;

#[cfg(test)]
mod title_tests;
#[cfg(test)]
mod fragment_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    Title,
    Kakera,
}
impl Destination {
    pub(super) fn prompt(self) -> &'static str {
        match self {
            Self::Title => "ゲームを終了して、タイトル画面に戻ります。@rよろしいですか？",
            Self::Kakera => "カケラ紡ぎのメニューに戻ります。@rよろしいですか？",
        }
    }
}
///15e54 normalizes the active signed scenario ID before the656c0 factory.
pub(crate) fn fragment_selected(scenario: i32) -> i32 {
    let normalized = if scenario > 9999 {
        scenario.wrapping_sub(10000)
    } else {
        scenario
    };
    normalized.wrapping_sub(2000)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    Reopen,
    Return,
}
fn action(callback: question::Callback) -> Action {
    if callback.stage == 1 && callback.button == 0 {
        Action::Return
    } else if callback.stage == 0 && callback.button != 0 {
        Action::Reopen
    } else {
        Action::None
    }
}
pub(in crate::adv::higurashi::host) struct QuestionMode {
    destination: Destination,
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
    pub(super) fn new(
        destination: Destination,
        parent: Arc<Mutex<OwnedMenu>>,
        assets: Arc<crate::asset::system::AssetServer>,
    ) -> Self {
        let source = assets.clone();
        let worker = shin_tasks::async_io::spawn_loader(async move {
            View::load(
                &source,
                destination.prompt(),
                &crate::adv::higurashi::browser_lifecycle::JUMP_BUTTONS,
                1,
                true,
            )
            .await
        });
        let retirement = worker.retirement_handle().unwrap();
        Self {
            destination,
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
    pub(in crate::adv::higurashi::host) fn cancel_input(&mut self) {
        if let Some((model, _)) = &mut self.loaded {
            model.cancel_input(&mut question::CancelOnly);
        }
    }
    pub(in crate::adv::higurashi::host) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut ModeServices<'_>,
    ) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!(
                "Higurashi {:?} return question failed:{fault}",
                self.destination
            );
        }
        if let Some(result) = self.worker.as_mut().and_then(|task| task.poll_naive()) {
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
                sounds: services.system_sounds.expect("SystemMenu owns return cues"),
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
                    Action::Return => {
                        ensure!(
                            services.menu_return.is_none(),
                            "SystemMenu return queued twice"
                        );
                        *services.menu_return = Some(self.destination);
                    }
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
    pub(in crate::adv::higurashi::host) fn state(&self) -> Option<(Destination, i32, i32)> {
        self.loaded
            .as_ref()
            .map(|(model, _)| (self.destination, model.phase(), model.selected()))
    }
}
