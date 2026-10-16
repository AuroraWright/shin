//! Concrete4dd80 Config ownership: atlas-only First, one native mixed scope,
//! real reset Question and SaveGame, and caller-owned source return callbacks.
#[cfg(test)]
use super::ViewState;
use super::{Action, Kind, Model, Services, Task, view::View};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            question::{self, Question, view::View as QuestionView},
            save_game::SaveGame,
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::{persistence::FrozenProgress, runtime_saves::Slots, save_menu::Environment},
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{format::save::higurashi::SaveData, time::Ticks, vm::command::types::Volume};
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

#[cfg(test)]
mod publication_tests;

const RESET_PROMPT: &str = "設定を初期値に戻します。よろしいですか？";
struct Job<T> {
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationPoint {
    Before,
    After,
}
#[cfg(test)]
type PublicationGate = Box<
    dyn FnOnce(std::sync::Weak<CharacterTextures>) -> futures::future::BoxFuture<'static, ()>
        + Send,
>;
enum Gate {
    None,
    #[cfg(test)]
    Hold {
        at: PublicationPoint,
        callback: Option<PublicationGate>,
    },
}
impl Gate {
    #[cfg(test)]
    async fn observe(
        &mut self,
        point: PublicationPoint,
        atlas: std::sync::Weak<CharacterTextures>,
    ) {
        if let Self::Hold { at, callback } = self {
            if *at == point {
                if let Some(callback) = callback.take() {
                    callback(atlas).await;
                }
            }
        }
    }
}
struct Loader {
    job: Option<Job<()>>,
    published: Arc<Mutex<Option<Arc<CharacterTextures>>>>,
    completed: bool,
    fault: Option<String>,
}
impl Loader {
    fn with_gate(server: Arc<crate::asset::system::AssetServer>, mut gate: Gate) -> Self {
        let published = Arc::new(Mutex::new(None));
        let destination = published.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load("/config.txa").await?;
            #[cfg(test)]
            let weak = Arc::downgrade(&atlas);
            #[cfg(test)]
            gate.observe(PublicationPoint::Before, weak.clone()).await;
            //539b4:53a18..20 publishes parent+c8 BEFORE temporary/file cleanup
            //and53a84 return. This is independently visible to draw/clock reads.
            *destination.lock() = Some(atlas);
            #[cfg(test)]
            gate.observe(PublicationPoint::After, weak).await;
            let _ = &mut gate;
            Ok(())
        });
        Self {
            job: Some(Job {
                retirement: task.retirement_handle().unwrap(),
                task: Some(task),
            }),
            published,
            completed: false,
            fault: None,
        }
    }
    fn poll(&mut self) -> Result<bool> {
        if let Some(fault) = &self.fault {
            bail!("Config First worker failed:{fault}");
        }
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|job| job.task.as_mut())
            .and_then(|task| task.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            if let Err(error) = output {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            ensure!(
                self.atlas().is_some(),
                "Completed Config First lost its atlas publication"
            );
            self.completed = true;
        }
        //52688/52698 admits Open only on executor status2. Publication alone
        //is never completion; pending native0/1 share the same consumer branch.
        Ok(self.completed)
    }
    fn atlas(&self) -> Option<Arc<CharacterTextures>> {
        self.published.lock().clone()
    }
    fn retire_first(&mut self) {
        //525a8 joins First's job before releasing it. Ordinary pruning retains
        //the parent+c8 atlas. Pending destruction uses retire_job directly.
        assert!(
            self.completed && self.atlas().is_some(),
            "Config First pruned before completion/publication"
        );
        self.retire_job();
    }
    fn retire_job(&mut self) {
        self.job.take();
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        //50994..509b0: private jobs retire BEFORE clearing the parent archive.
        //Never hold its mutex while draining; workers may still be publishing.
        self.retire_job();
        self.published.lock().take();
    }
}
struct Dialog {
    job: Option<Job<(Question, QuestionView)>>,
    ready: Option<(Question, QuestionView)>,
    extent: (u16, u16),
}
impl Dialog {
    fn new(server: Arc<crate::asset::system::AssetServer>) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            QuestionView::load(&server, RESET_PROMPT, &["はい", "いいえ"], 1, true).await
        });
        Self {
            job: Some(Job {
                retirement: task.retirement_handle().unwrap(),
                task: Some(task),
            }),
            ready: None,
            extent: (1280, 720),
        }
    }
    fn complete(&self) -> bool {
        self.ready
            .as_ref()
            .is_some_and(|(question, _)| question.complete())
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        runtime: &FrameServices<'_, '_>,
    ) -> Result<Vec<question::Callback>> {
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|job| job.task.as_mut())
            .and_then(|task| task.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            self.ready = Some(output?);
            self.job.take();
        }
        struct Callbacks<'a> {
            env: &'a Environment,
            events: Vec<question::Callback>,
        }
        impl question::Services for Callbacks<'_> {
            fn cue(&mut self, cue: question::Cue) {
                self.env.sounds.lock().play(
                    &self.env.server,
                    match cue {
                        question::Cue::Move => "move",
                        question::Cue::Confirm => "decide",
                        question::Cue::Cancel => "cancel",
                        question::Cue::NoSelection => "error",
                    },
                    Volume(1.),
                );
            }
            fn callback(&mut self, value: question::Callback) {
                self.events.push(value);
            }
        }
        let mut callbacks = Callbacks {
            env: runtime.env,
            events: Vec::new(),
        };
        if let Some((question, _)) = &mut self.ready {
            if self.extent != runtime.extent {
                question.resize_input(runtime.extent, &mut callbacks);
                self.extent = runtime.extent;
            }
            question.poll(
                delta,
                u32::from(permission),
                runtime.buttons.pressed,
                runtime.buttons.repeat,
                runtime.pointer,
                false,
                &mut callbacks,
            );
        }
        Ok(callbacks.events)
    }
}
enum Owned {
    Private(Task),
    Question(Dialog),
    Save(SaveGame),
}
struct FrameServices<'a, 'p> {
    env: &'a Environment,
    settings: &'a mut SaveData,
    slots: Option<&'a Slots>,
    platform_flag: &'a mut bool,
    area: &'a mut InputArea,
    loader: &'a mut Loader,
    extent: (u16, u16),
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    requests: &'a mut Vec<u32>,
    spawned: Vec<Owned>,
}
impl FrameServices<'_, '_> {
    fn publish(&mut self, model: &Model) {
        self.settings.settings = model.settings.clone();
        self.env
            .sounds
            .lock()
            .set_settings_volume(model.settings.common.v3_sysvol);
    }
}
impl Services for FrameServices<'_, '_> {
    fn input(&mut self, model: &mut Model, permission: bool) {
        let captured = if permission {
            self.area.update(model, self.pointer)
        } else {
            self.area.disable(model);
            false
        };
        model.poll_controls(permission, captured, self.buttons);
        // Native widgets write shared settings before their following cue/callback.
        self.publish(model);
    }
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(cue) => self
                .env
                .sounds
                .lock()
                .play(&self.env.server, cue, Volume(1.)),
            Action::DisableInput => {}
            Action::ResetQuestion => self
                .spawned
                .push(Owned::Question(Dialog::new(self.env.server.clone()))),
            Action::Save { flag } => {
                let snapshot = FrozenProgress {
                    data: self.settings.clone(),
                    slots: self.slots.cloned(),
                    storage: self
                        .env
                        .storage
                        .clone()
                        .context("Config save requires loaded progress storage")?,
                };
                let recover = snapshot.storage.recover_write_errors();
                let write = Arc::new(move || snapshot.write());
                self.spawned.push(Owned::Save(
                    SaveGame::new(self.env.server.clone(), write, flag, 0).with_desktop_recovery(
                        recover,
                        crate::adv::higurashi::save_game::RecoveryPolicy::RetryOnly,
                    ),
                ));
            }
            Action::ReturnSource(stage) => self.requests.push(stage),
            //27540/e7aa4 writes the existing platform owner+70. No desktop
            //background-play, audio suspension or sleep policy is inferred.
            Action::SetPlatformFlag(value) => *self.platform_flag = value,
        }
        Ok(())
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    first_completed: bool,
    services: FrameServices<'a, 'p>,
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(match self {
            Self::Private(task) => task.done,
            Self::Question(dialog) => dialog.complete(),
            Self::Save(task) => task.complete(),
        })
    }
    fn retire(&mut self, frame: &mut Frame<'_, '_>) {
        if matches!(
            self,
            Self::Private(Task {
                kind: Kind::First,
                ..
            })
        ) {
            frame.services.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Private(task) => {
                for task in task.poll_native(
                    frame.model,
                    if frame.first_completed { 2 } else { 1 },
                    delta,
                    permission,
                    &mut frame.services,
                )? {
                    pushes.push(Self::Private(task));
                }
            }
            Self::Question(dialog) => {
                for event in dialog.poll(delta, permission, &frame.services)? {
                    frame.model.reset_answer(event.stage, event.button);
                }
                frame.services.publish(frame.model);
                for action in frame.model.take_actions() {
                    frame.services.event(action)?;
                }
                for task in frame.model.take_pending_tasks() {
                    pushes.push(Self::Private(task));
                }
            }
            Self::Save(task) => {
                task.poll(
                    delta,
                    permission,
                    frame.services.buttons,
                    Some(frame.services.pointer),
                )?;
                if task.take_notification() {
                    frame.model.save_complete();
                    for action in frame.model.take_actions() {
                        frame.services.event(action)?;
                    }
                    for task in frame.model.take_pending_tasks() {
                        pushes.push(Self::Private(task));
                    }
                }
            }
        }
        for child in frame.services.spawned.drain(..) {
            pushes.push(child);
        }
        Ok(0)
    }
}
pub(in crate::app::higurashi) struct Owner {
    // Native destruction retires widget/model storage, private scope/jobs and
    // atlas before releasing callback/input/environment resource owners.
    pub model: Model,
    scrollbar: Scrollbar,
    scope: ModeStack<Owned>,
    loader: Loader,
    view: View,
    area: InputArea,
    extent: (u16, u16),
    requests: Vec<u32>,
    fault: Option<String>,
    #[cfg(test)]
    after_scope: Option<Box<dyn FnOnce() + Send + Sync>>,
    env: Environment,
}
impl Owner {
    pub fn new(
        env: Environment,
        settings: &SaveData,
        flags: u32,
        append: bool,
        extent: (u16, u16),
    ) -> Result<Self> {
        Self::with_gate(env, settings, flags, append, extent, Gate::None)
    }
    fn with_gate(
        env: Environment,
        settings: &SaveData,
        flags: u32,
        append: bool,
        extent: (u16, u16),
        gate: Gate,
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Config requires a nonzero input extent"
        );
        let model = Model::new(settings.settings.clone(), flags, append)?;
        let mut scope = ModeStack::default();
        scope.push(Owned::Private(Task {
            kind: Kind::First,
            done: false,
        }));
        let loader = Loader::with_gate(env.server.clone(), gate);
        Ok(Self {
            model,
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            scope,
            loader,
            view: View::new(),
            area: InputArea::new(extent.0, extent.1),
            extent,
            requests: Vec::new(),
            fault: None,
            #[cfg(test)]
            after_scope: None,
            env,
        })
    }
    pub fn complete(&self) -> bool {
        self.model.complete
    }
    pub fn take_requests(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.requests)
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Config input extent");
        if extent != self.extent {
            self.area.disable(&mut self.model);
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.model);
        for child in self.scope.iter_mut() {
            match child {
                Owned::Question(dialog) => {
                    if let Some((question, _)) = &mut dialog.ready {
                        question.cancel_input(&mut question::CancelOnly);
                    }
                }
                Owned::Save(task) => task.cancel_input(),
                _ => {}
            }
        }
    }
    pub fn has_save_task(&self) -> bool {
        self.scope
            .iter()
            .any(|child| matches!(child, Owned::Save(task) if task.blocks_close()))
    }
    pub fn publish_progress(&self, settings: &mut SaveData) {
        settings.settings = self.model.settings.clone();
    }
    pub fn poll(
        &mut self,
        settings: &mut SaveData,
        slots: Option<&Slots>,
        platform_flag: &mut bool,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid Config frame delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Config failed:{fault}");
        }
        let result = (|| {
            let first_completed = self.loader.poll()?;
            if let Some(atlas) = self.loader.atlas() {
                self.model.set_extra_artwork(
                    atlas.contains("slider") && atlas.get("slider").size_vec().y >= 240.,
                );
            }
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model: &mut self.model,
                    first_completed,
                    services: FrameServices {
                        env: &self.env,
                        settings,
                        slots,
                        platform_flag,
                        area: &mut self.area,
                        loader: &mut self.loader,
                        extent: self.extent,
                        buttons,
                        pointer,
                        requests: &mut self.requests,
                        spawned: Vec::new(),
                    },
                },
            )?;
            #[cfg(test)]
            if let Some(after_scope) = self.after_scope.take() {
                after_scope();
            }
            //50bcc acquire occurs AFTER private-scope callbacks. A publication
            //between First status and this tail must enable its1-tick scroll.
            self.model
                .advance_clocks(delta, self.loader.atlas().is_some(), &mut self.scrollbar);
            settings.settings = self.model.settings.clone();
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    #[cfg(test)]
    pub fn state(&self) -> ViewState {
        self.model
            .view_state(self.loader.atlas().is_some(), self.scrollbar)
    }
    pub fn prepare(&self, context: &mut PreRenderContext) {
        for child in self.scope.iter() {
            match child {
                Owned::Question(dialog) => {
                    if let Some((question, view)) = &dialog.ready {
                        view.prepare(context, question);
                    }
                }
                Owned::Save(task) => task.prepare(context),
                _ => {}
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        //50c88 loads the currently published atlas, without polling the job or
        //opening/ticking the menu. Keep one retained read for this parent draw.
        let atlas = self.loader.atlas();
        self.view.render(
            &self.model.view_state(atlas.is_some(), self.scrollbar),
            atlas.as_deref(),
            &self.env.common.system,
            false,
            pass,
        )?;
        self.scope.draw_ref(|child| match child {
            Owned::Question(dialog) => {
                if let Some((question, view)) = &dialog.ready {
                    view.render(pass, question);
                }
            }
            Owned::Save(task) => task.render(pass),
            _ => {}
        });
        Ok(())
    }
    #[cfg(test)]
    pub fn reference_input_ready(&self) -> bool {
        let mut borrowed_input = false;
        self.scope.draw_ref(|child| {
            if matches!(child, Owned::Private(task) if task.kind == Kind::Input && !task.done) {
                borrowed_input = true;
            }
        });
        borrowed_input
            && !self.model.closing
            && self.model.scroll.is_idle()
            && self.scope.iter().all(|child| {
                matches!(child, Owned::Private(task) if task.done || task.kind == Kind::Input)
            })
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_state(&self) -> (u32, usize, bool, u8) {
        (
            self.model.flags,
            self.model.selected,
            !self.model.closing
                && self.scope.iter().any(|child| {
                    matches!(
                        child,
                        Owned::Private(Task {
                            kind: Kind::Input,
                            done: false
                        })
                    )
                }),
            self.model.settings.common.v3_sysvol,
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_question(&self) -> Option<(i32, i32)> {
        self.scope.iter().find_map(|child| match child {
            Owned::Question(dialog) => dialog
                .ready
                .as_ref()
                .map(|(question, _)| (question.phase(), question.selected())),
            _ => None,
        })
    }
}
