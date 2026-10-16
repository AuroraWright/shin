//! Actual retained Kakera resources/input/audio/question and deferred ADV request.
//! Complete parent rendering precedes its retained First/question children.
use super::{
    Model,
    motion::{self, Action, GameplayRequest, Motion},
};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            music_commands::MusicCommands,
            question::{self, Question, view::View},
            runner::reconstruction_mode::Overlay,
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::scene_host::Globals,
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use shin_core::{time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::{Arc, mpsc};

struct Job<T> {
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
struct QuestionOwner {
    job: Option<Job<(Question, View)>>,
    ready: Option<(Question, View)>,
}
pub(in crate::app::higurashi) struct Opening {
    pub globals: Globals,
    pub atlas: Option<Arc<CharacterTextures>>,
    job: Option<Job<Arc<MaskTexture>>>,
    received: mpsc::Receiver<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
    overlay: Option<Overlay>,
    pub motion: Motion,
    view: super::view::View,
    area: InputArea,
    extent: (u16, u16),
    question: Option<QuestionOwner>,
    ready: bool,
    transferred: bool,
    fault: Option<String>,
}
struct Services<'a, 'p> {
    area: &'a mut InputArea,
    question: &'a mut Option<QuestionOwner>,
    pointer: &'a PointerFrame<'p>,
    buttons: crate::adv::higurashi::input::Buttons,
    permission: bool,
    globals: &'a mut Globals,
}
struct QuestionServices<'a> {
    state: &'a mut Motion,
    globals: &'a mut Globals,
}
impl question::Services for QuestionServices<'_> {
    fn cue(&mut self, cue: question::Cue) {
        let name = match cue {
            question::Cue::Move => "move",
            question::Cue::Cancel => "cancel",
            question::Cue::Confirm => "decide",
            question::Cue::NoSelection => "error",
        };
        self.globals
            .sounds
            .lock()
            .play(&self.globals.server, name, Volume(1.0));
    }
    fn callback(&mut self, callback: question::Callback) {
        self.state.exit_answer(callback.stage, callback.button);
    }
}
impl motion::Services for Services<'_, '_> {
    fn input(&mut self, state: &mut Motion, permission: bool) {
        let permission = permission && self.permission;
        let captured = if permission {
            self.area.update(state, self.pointer)
        } else {
            self.area.disable(state);
            false
        };
        state.controls(permission, captured, self.buttons);
    }
    fn question(&mut self, state: &mut Motion, permission: bool, delta: Ticks) -> bool {
        let Some(owner) = self.question.as_mut() else {
            return false;
        };
        let Some((model, _)) = owner.ready.as_mut() else {
            return false;
        };
        let mut services = QuestionServices {
            state,
            globals: self.globals,
        };
        model.poll(
            delta,
            u32::from(permission && self.permission),
            self.buttons.pressed,
            self.buttons.repeat,
            self.pointer,
            false,
            &mut services,
        );
        model.complete()
    }
    fn next_seed(&mut self) -> u32 {
        crate::engine_random::next_seed()
    }
}
impl Opening {
    pub fn new(
        globals: Globals,
        selected: i32,
        data: impl Into<crate::app::higurashi::runtime_game_data::RuntimeGameData>,
        capture: Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Self {
        let motion = Motion::new(
            Model::new(data, selected, &globals.data.persist_data),
            capture.is_some(),
        );
        let view = super::view::View::new(globals.common.font.clone());
        let (sender, received) = mpsc::channel();
        let server: Arc<AssetServer> = globals.server.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas = server.load::<CharacterTextures, _>("/kakera.txa").await?;
            let _ = sender.send(atlas);
            server.load::<MaskTexture, _>("/default.msk").await
        });
        let retirement = task.retirement_handle().unwrap();
        let overlay = capture.map(|snapshot| Overlay {
            clock: motion.overlay.clone(),
            snapshot,
            mask: None,
        });
        Self {
            globals,
            atlas: None,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            received,
            mask: None,
            overlay,
            motion,
            view,
            area: InputArea::new(extent.0, extent.1),
            extent,
            question: None,
            ready: false,
            transferred: false,
            fault: None,
        }
    }
    pub fn resize_input(&mut self, extent: (u16, u16)) {
        if self.extent == extent {
            return;
        }
        self.area.disable(&mut self.motion);
        self.area = InputArea::new(extent.0, extent.1);
        self.extent = extent;
        if let Some((model, _)) = self.question.as_mut().and_then(|q| q.ready.as_mut()) {
            model.resize_input(
                extent,
                &mut QuestionServices {
                    state: &mut self.motion,
                    globals: &mut self.globals,
                },
            );
        }
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.area.disable(&mut self.motion);
        if let Some((model, _)) = self.question.as_mut().and_then(|q| q.ready.as_mut()) {
            model.cancel_input(&mut QuestionServices {
                state: &mut self.motion,
                globals: &mut self.globals,
            });
        }
        Ok(())
    }
    pub fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: crate::adv::higurashi::input::Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid Kakera scene delta"
        );
        ensure!(
            !self.transferred && self.motion.request.is_none(),
            "Dispatch the retained Kakera ADV request before updating the old scene"
        );
        if let Some(fault) = &self.fault {
            bail!("Kakera preparation failed:{fault}");
        }
        let result = (|| {
            if self.motion.first_retiring() {
                self.job.take();
                self.mask.take();
                self.overlay.take();
            }
            if self
                .question
                .as_ref()
                .and_then(|q| q.ready.as_ref())
                .is_some_and(|(q, _)| q.complete())
            {
                self.question.take();
            }
            if let Some(job) = &mut self.job {
                while let Ok(atlas) = self.received.try_recv() {
                    self.atlas = Some(atlas);
                }
                if let Some(task) = &mut job.task {
                    if let Some(mask) = task.poll_naive() {
                        self.mask = Some(mask?);
                        job.task = None;
                        self.ready = true;
                    }
                }
            }
            if let Some(owner) = &mut self.question {
                if let Some(job) = &mut owner.job {
                    if let Some(task) = &mut job.task {
                        if let Some(pair) = task.poll_naive() {
                            let (mut model, view) = pair?;
                            model.resize_input(
                                self.extent,
                                &mut QuestionServices {
                                    state: &mut self.motion,
                                    globals: &mut self.globals,
                                },
                            );
                            owner.ready = Some((model, view));
                            job.task = None;
                        }
                    }
                }
            }
            while let Ok(atlas) = self.received.try_recv() { self.atlas = Some(atlas); }
            if let Some(atlas) = &self.atlas { self.motion.model.apply_artwork(atlas); }
            self.motion.advance(
                if self.ready { 2 } else { 1 },
                delta,
                &mut Services {
                    area: &mut self.area,
                    question: &mut self.question,
                    pointer,
                    buttons,
                    permission,
                    globals: &mut self.globals,
                },
            );
            for action in std::mem::take(&mut self.motion.actions) {
                match action {
                    Action::Cue(name) => {
                        self.globals
                            .sounds
                            .lock()
                            .play(&self.globals.server, name, Volume(1.0))
                    }
                    Action::PlayBgm(id) => self.globals.audio.play_bgm(id, 60, 0, 1.0)?,
                    Action::StopBgm => self.globals.audio.stop_bgm(60),
                    Action::ExitQuestion => {
                        ensure!(
                            self.question.is_none(),
                            "Kakera already owns its exit question"
                        );
                        let server = self.globals.server.clone();
                        let task = shin_tasks::async_io::spawn_loader(async move {
                            View::load(&server, motion::EXIT_PROMPT, &["はい", "いいえ"], 1, true)
                                .await
                        });
                        let retirement = task.retirement_handle().unwrap();
                        self.question = Some(QuestionOwner {
                            job: Some(Job {
                                task: Some(task),
                                retirement,
                            }),
                            ready: None,
                        });
                    }
                }
            }
            if let Some(overlay) = &mut self.overlay {
                overlay.clock = self.motion.overlay.clone();
            }
            while let Some(event) = self.globals.audio.pop_music_event() {
                if let crate::adv::higurashi::audio::MusicEvent::Started(info) = event {
                    for id in [info.id, i32::from(info.linked_id)]
                        .into_iter()
                        .filter(|id| *id >= 0)
                    {
                        let words = &mut self.globals.data.save_vectors.unlocked_bgms;
                        let at = id as usize / 32;
                        if at >= words.len() {
                            words.resize((at + 1).next_multiple_of(256), 0);
                        }
                        words[at] |= 1 << (id as usize % 32);
                    }
                }
            }
            self.globals.audio.update(delta, false);
            self.globals.sounds.lock().update();
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn take_gameplay_request(&mut self) -> Option<GameplayRequest> {
        let request = self.motion.request.take()?;
        self.transferred = true;
        Some(request)
    }
    pub fn into_globals(self) -> Globals {
        self.globals
    }
    pub fn prepare_question(&self, context: &mut PreRenderContext) {
        if let Some((model, view)) = self.question.as_ref().and_then(|q| q.ready.as_ref()) {
            view.prepare(context, model);
        }
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        self.view
            .synchronize(&self.globals.server, &self.motion.model)?;
        self.prepare_question(context);
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.view.render(&self.motion, self.atlas.as_deref(), pass);
        self.render_children(pass);
    }
    pub fn render_children(&self, pass: &mut RenderPass) {
        if self.motion.borrowed.contains(&1) {
            if let Some(overlay) = &self.overlay {
                overlay.render(pass);
            }
        }
        let question_visible = self.motion.tasks().any(|(id, kind, _)| {
            kind == motion::Kind::Question && self.motion.borrowed.contains(&id)
        });
        if question_visible {
            if let Some((model, view)) = self.question.as_ref().and_then(|q| q.ready.as_ref()) {
                view.render(pass, model);
            }
        }
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_input_ready(&self) -> bool {
        self.ready
            && self.atlas.is_some()
            && self.question.is_none()
            && self.motion.first_phase().is_none()
            && !self.motion.closing
            && self.motion.request.is_none()
            && self.motion.clocks[9].is_idle()
            && self.motion.clocks[9].value() == 0.
            && self.motion.tasks().any(|(id, kind, done)| {
                kind == motion::Kind::Input && !done && self.motion.borrowed.contains(&id)
            })
            && self
                .motion
                .tasks()
                .all(|(_, kind, done)| done || kind == motion::Kind::Input)
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_question(&self) -> Option<(i32, i32)> {
        self.question
            .as_ref()?
            .ready
            .as_ref()
            .map(|(q, _)| (q.phase(), q.selected()))
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_retirements(
        &self,
    ) -> Vec<shin_tasks::LoaderRetirement> {
        self.retirements()
    }
    #[cfg(test)]
    pub(super) fn retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.job
            .iter()
            .map(|job| job.retirement.clone())
            .chain(
                self.question
                    .iter()
                    .filter_map(|q| q.job.as_ref().map(|job| job.retirement.clone())),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests;
