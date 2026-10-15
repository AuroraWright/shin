//! Retained original chapter question. It consumes input without modal pause.
use super::{HigurashiHost, HostMode, ModeServices};
use crate::adv::higurashi::{
    navigation::{self, Chapter},
    question::{self, Question, view::View},
};
use anyhow::{Result, bail, ensure};
use shin_core::{time::Ticks, vm::command::types::Volume};

pub(super) struct QuestionMode {
    kind: Chapter,
    assets: std::sync::Arc<crate::asset::system::AssetServer>,
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
    pub(super) fn cancel_input(&mut self) {
        if let Some((model, _)) = &mut self.loaded {
            model.cancel_input(&mut question::CancelOnly);
        }
    }
    pub(super) fn completion(&self) -> u32 {
        u32::from(
            self.loaded
                .as_ref()
                .is_some_and(|(model, _)| model.complete()),
        )
    }
    pub(super) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut ModeServices<'_>,
    ) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!("Higurashi chapter question failed:{fault}");
        }
        if let Some(result) = self.worker.as_mut().and_then(|w| w.poll_naive()) {
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
                sounds: &'a super::system_menu::GlobalSounds,
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
                events: &[],
                number: 0,
                time_seconds: 0.0,
            };
            let mut callbacks = Callbacks {
                assets: &self.assets,
                sounds: services
                    .system_sounds
                    .expect("Chapter question has a global cue owner"),
                callbacks: Vec::new(),
            };
            model.poll(
                delta,
                u32::from(permission),
                services.buttons.pressed,
                services.buttons.repeat,
                services.pointer.unwrap_or(&empty),
                services.navigation_disabled,
                &mut callbacks,
            );
            for callback in callbacks.callbacks {
                if navigation::accepts_chapter(services.navigation_disabled, callback) {
                    services.chapter_requests.push(self.kind);
                }
            }
        }
        Ok(0)
    }
    pub(super) fn prepare(&mut self, context: &mut crate::render::PreRenderContext) {
        if let Some((model, view)) = &self.loaded {
            view.prepare(context, model);
        }
    }
    pub(super) fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        if let Some((model, view)) = &self.loaded {
            view.render(pass, model);
        }
    }
}
impl HigurashiHost {
    pub fn set_navigation_disabled(&mut self, disabled: bool) {
        self.navigation_disabled = disabled;
    }
    pub fn open_chapter_question(
        &mut self,
        kind: Chapter,
        sounds: super::system_menu::GlobalSounds,
    ) -> Result<()> {
        ensure!(
            !self.has_modes(),
            "Chapter question requires an idle mode stack"
        );
        ensure!(
            self.runner.reconstruction_phase().is_none(),
            "Chapter question cannot open during reconstruction"
        );
        if let Some(previous) = &self.system_sounds {
            ensure!(
                std::sync::Arc::ptr_eq(previous, &sounds),
                "Chapter question must retain global cues"
            );
        }
        let assets = self.runner.assets().clone();
        let loaded_assets = assets.clone();
        let worker = shin_tasks::async_io::spawn_loader(async move {
            View::load(
                &loaded_assets,
                kind.prompt(),
                &crate::adv::higurashi::browser_lifecycle::JUMP_BUTTONS,
                1,
                true,
            )
            .await
        });
        let retirement = worker.retirement_handle().unwrap();
        self.system_sounds = Some(sounds);
        self.modes.push(HostMode::ChapterQuestion(QuestionMode {
            kind,
            assets,
            worker: Some(worker),
            retirement,
            loaded: None,
            fault: None,
        }));
        self.mode_input_permission = false;
        Ok(())
    }
    pub fn take_chapter_requests(&mut self) -> Vec<Chapter> {
        std::mem::take(&mut self.chapter_requests)
    }
    /// Native14d30 captures ADV98 before18b78 freezes the source. Keep the
    /// prepared replay owned on fallible capture/preflight errors.
    pub(crate) fn begin_scene_reconstruction(
        &mut self,
        replay: &mut Option<crate::adv::higurashi::replay::ReplaySession>,
        mode: u32,
        context: &mut crate::render::PreRenderContext,
        screens: &mut crate::adv::higurashi::scene_textures::HigurashiSceneTextures,
        history: crate::adv::higurashi::read_history::MessageReadHistory,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<()> {
        self.runner.check_reconstruction_start(
            replay
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing prepared scene replay"))?,
        )?;
        let snapshot = screens.capture_frame_with_overlay(context, &self.runner, self.touch_overlay.as_deref())?;
        self.runner.begin_reconstruction_mode(
            replay.take().unwrap(),
            mode,
            snapshot,
            history,
            seeds,
        )?;
        self.modal.animations_enabled = false;
        self.mode_input_permission = false;
        self.modes
            .push(HostMode::Reconstruction(super::ReconstructionStackMode {
                phase: crate::adv::higurashi::reconstruction_mode::Phase::Preparing,
                completed_owner: None,
            }));
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reference_chapter_question(&self) -> Option<(i32, i32)> {
        self.modes.iter().find_map(|mode| match mode {
            HostMode::ChapterQuestion(question) => question
                .loaded
                .as_ref()
                .map(|(model, _)| (model.phase(), model.selected())),
            _ => None,
        })
    }
}
