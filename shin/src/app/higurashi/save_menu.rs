//! Concrete updated SaveMenu owner: one mixed scope for private tasks,
//! Questions and SaveGame, followed by native preview/cache clocks.
use super::{
    first_boot::CommonAssets,
    runtime_game_data::RuntimeGameData,
    runtime_saves::{Slots, manual::ManualWrite},
};
use crate::{
    adv::{
        higurashi::{
            browser_controls::Scrollbar,
            input::Buttons,
            mode_stack::{Mode as ScopeMode, ModePush, ModeStack},
            question::{self, Question, view::View as QuestionView},
            save_game::{RecoveryPolicy, SaveGame},
            save_menu::{
                Action, Mode, Model, Services, SlotMetadata, ViewState,
                model::{Kind, Task},
                resources::Loader,
                view::View,
            },
        },
        input_area::{InputArea, PointerFrame},
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{
    format::save::higurashi::{SaveData, SaveDate},
    time::Ticks,
    vm::command::types::Volume,
};
use shin_render::render_pass::RenderPass;
use std::sync::Arc;
pub(super) mod preview;

pub(super) struct Prepared {
    current: Option<RuntimeGameData>,
    model: Model,
    extent: (u16, u16),
    append: bool,
    setting: u8,
    preferences: preview::globals::Snapshot,
}
fn metadata(slots: &Slots) -> [SlotMetadata; 100] {
    std::array::from_fn(|i| SlotMetadata {
        valid: slots.manual[i].valid,
        scenario_id: slots.manual[i].data.progress.active_entry().scenario_id,
    })
}
impl Prepared {
    /// No clock, entropy, commits, source mutation or resource jobs occur here.
    pub fn new(
        source: Option<&RuntimeGameData>,
        settings: &SaveData,
        slots: &Slots,
        append: bool,
        extent: (u16, u16),
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "SaveMenu requires a nonzero physical canvas"
        );
        Ok(Self {
            current: source.cloned(),
            model: Model::new(
                if source.is_some() {
                    Mode::Save
                } else {
                    Mode::Load
                },
                settings.save_menu_position,
                append,
                metadata(slots),
            )?,
            extent,
            append,
            setting: u8::from(settings.settings.extra_flag),
            preferences: preview::globals::Snapshot::new(settings),
        })
    }
    #[cfg(test)]
    pub fn current(&self) -> Option<&RuntimeGameData> {
        self.current.as_ref()
    }
}
pub(super) struct Environment {
    pub common: Arc<CommonAssets>,
    pub server: Arc<AssetServer>,
    pub sounds: Arc<Mutex<SystemSoundPlayer>>,
    pub storage: Option<Arc<dyn super::load_game::Storage>>,
}
pub(super) enum Request {
    ReturnSource(u32),
    Load { data: RuntimeGameData, flags: u32 },
}

struct DialogJob {
    task: Option<shin_tasks::AsyncTask<Result<(Question, QuestionView)>>>,
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
struct Dialog {
    job: Option<DialogJob>,
    ready: Option<(Question, QuestionView)>,
    input_extent: (u16, u16),
}
impl Dialog {
    fn new(server: Arc<AssetServer>, kind: crate::adv::higurashi::save_menu::QuestionKind) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            let labels = kind.labels().split('|').collect::<Vec<_>>();
            QuestionView::load(
                &server,
                kind.prompt(),
                &labels,
                kind.selected(),
                kind.cancellable(),
            )
            .await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            job: Some(DialogJob {
                task: Some(task),
                retirement,
            }),
            ready: None,
            input_extent: (1280, 720),
        }
    }
    fn complete(&self) -> bool {
        self.ready.as_ref().is_some_and(|(q, _)| q.complete())
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        env: &Environment,
        extent: (u16, u16),
    ) -> Result<Vec<question::Callback>> {
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|j| j.task.as_mut())
            .and_then(|t| t.poll_naive())
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
                    Volume(1.0),
                );
            }
            fn callback(&mut self, event: question::Callback) {
                self.events.push(event);
            }
        }
        let mut callbacks = Callbacks {
            env,
            events: Vec::new(),
        };
        if let Some((question, _)) = &mut self.ready {
            // Switch touch services are literal1280x720. Desktop PointerFrame
            // carries window pixels,so only its input adapter uses this extent.
            if self.input_extent != extent {
                question.resize_input(extent, &mut callbacks);
                self.input_extent = extent;
            }
            question.poll(
                delta,
                u32::from(permission),
                buttons.pressed,
                buttons.repeat,
                pointer,
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
    Save {
        task: SaveGame,
        transaction: Option<ManualWrite>,
        selected: usize,
    },
}
struct Runtime<'a, 'p> {
    env: &'a Environment,
    current: &'a mut Option<RuntimeGameData>,
    settings: &'a mut SaveData,
    slots: &'a mut Slots,
    loader: &'a mut Loader,
    area: &'a mut InputArea,
    previews: &'a mut preview::PreviewService,
    date: &'a mut dyn FnMut() -> Result<SaveDate>,
    pointer: &'a PointerFrame<'p>,
    buttons: Buttons,
    extent: (u16, u16),
    requests: &'a mut Vec<Request>,
    spawned: Vec<Owned>,
}
impl Services for Runtime<'_, '_> {
    fn input(&mut self, model: &mut Model, permission: bool) {
        let captured = if permission {
            self.area.update(model, self.pointer)
        } else {
            self.area.disable(model);
            false
        };
        model.poll_controls(permission, captured, self.buttons);
    }
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(name) => self
                .env
                .sounds
                .lock()
                .play(&self.env.server, name, Volume(1.0)),
            Action::DisableInput => {} // input() already performs the one native disable.
            Action::OpenQuestion(kind) => self
                .spawned
                .push(Owned::Question(Dialog::new(self.env.server.clone(), kind))),
            Action::SaveSelected(index) => {
                let storage = self
                    .env
                    .storage
                    .clone()
                    .context("Manual Save requires loaded storage")?;
                let now = (self.date)()?;
                let current = self
                    .current
                    .as_mut()
                    .context("Load-only menu cannot save")?;
                let transaction = self.previews.with_settings(self.settings, |settings| {
                    self.slots.begin_save(settings, index, current, now)
                })?;
                self.previews.invalidate(index);
                let snapshot = transaction.snapshot();
                let recover = storage.recover_write_errors();
                let write = Arc::new(move || storage.write(&snapshot.encode()?));
                self.spawned.push(Owned::Save {
                    task: SaveGame::new(self.env.server.clone(), write, 0, 0)
                        .with_desktop_recovery(recover, RecoveryPolicy::RetryOrReturn),
                    transaction: Some(transaction),
                    selected: index,
                });
            }
            Action::DeleteSelected(index) => {
                let storage = self
                    .env
                    .storage
                    .clone()
                    .context("Manual Delete requires loaded storage")?;
                let transaction = self.previews.with_settings(self.settings, |settings| {
                    self.slots.begin_delete(settings, index)
                })?;
                self.previews.invalidate(index);
                let snapshot = transaction.snapshot();
                let recover = storage.recover_write_errors();
                let write = Arc::new(move || storage.write(&snapshot.encode()?));
                self.spawned.push(Owned::Save {
                    task: SaveGame::new(self.env.server.clone(), write, 0, 0)
                        .with_desktop_recovery(recover, RecoveryPolicy::RetryOrReturn),
                    transaction: Some(transaction),
                    selected: index,
                });
            }
            Action::LoadSelected(index) => {
                *self.current = Some(self.slots.load_manual(self.settings, index)?)
            }
            Action::ReturnSource(stage) => self.requests.push(Request::ReturnSource(stage)),
            Action::QueueAdv { flags } => self.requests.push(Request::Load {
                data: self
                    .current
                    .as_ref()
                    .context("Loaded menu lost its complete GameData")?
                    .clone(),
                flags,
            }),
        }
        Ok(())
    }
    fn retire_first(&mut self) {
        self.loader
            .retire_first()
            .expect("Native First retires after atlas readiness");
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    status: u32,
    runtime: Runtime<'a, 'p>,
}
impl ScopeMode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(match self {
            Self::Private(task) => task.done,
            Self::Question(dialog) => dialog.complete(),
            Self::Save { task, .. } => task.complete(),
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
            frame.runtime.retire_first();
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
                    frame.status,
                    delta,
                    permission,
                    &mut frame.runtime,
                )? {
                    pushes.push(Self::Private(task));
                }
            }
            Self::Question(dialog) => {
                for event in dialog.poll(
                    delta,
                    permission,
                    frame.runtime.buttons,
                    frame.runtime.pointer,
                    frame.runtime.env,
                    frame.runtime.extent,
                )? {
                    frame.model.question_callback(event.stage, event.button);
                    for action in frame.model.take_actions() {
                        frame.runtime.event(action)?;
                    }
                    for task in frame.model.take_pending_tasks() {
                        pushes.push(Self::Private(task));
                    }
                }
            }
            Self::Save {
                task,
                transaction,
                selected,
            } => {
                if let Err(error) = task.poll(
                    delta,
                    permission,
                    frame.runtime.buttons,
                    Some(frame.runtime.pointer),
                ) {
                    if let Some(transaction) = transaction.take() {
                        transaction.complete(frame.runtime.slots, frame.runtime.settings, false);
                        frame.model.write_complete();
                        frame.runtime.previews.invalidate(*selected);
                    }
                    return Err(error);
                }
                if let Some(success) = task.take_result() {
                    transaction
                        .take()
                        .context("Manual Save completed twice")?
                        .complete(frame.runtime.slots, frame.runtime.settings, success);
                    frame.model.write_complete();
                    if !success {
                        frame.runtime.previews.invalidate(*selected);
                    }
                }
            }
        }
        for child in frame.runtime.spawned.drain(..) {
            pushes.push(child);
        }
        Ok(0)
    }
}
pub(super) struct Owner {
    env: Environment,
    current: Option<RuntimeGameData>,
    pub model: Model,
    scrollbar: Scrollbar,
    scope: ModeStack<Owned>,
    loader: Loader,
    view: View,
    area: InputArea,
    extent: (u16, u16),
    previews: preview::PreviewService,
    requests: Vec<Request>,
    date: Box<dyn FnMut() -> Result<SaveDate> + Send>,
    fault: Option<String>,
}
impl Owner {
    pub fn from_prepared(env: Environment, prepared: Prepared) -> Result<Self> {
        let previews = preview::PreviewService::new(
            env.server.clone(),
            env.common.scenario.clone(),
            (u32::from(prepared.extent.0), u32::from(prepared.extent.1)),
            prepared.append,
            prepared.setting,
            Arc::new(preview::os_seed),
        )
        .with_globals(prepared.preferences);
        let mut scope = ModeStack::default();
        scope.push(Owned::Private(Task {
            kind: Kind::First,
            done: false,
        }));
        let loader = Loader::new(env.server.clone());
        let view = View::new(env.common.font.clone());
        let mut area = InputArea::new(prepared.extent.0, prepared.extent.1);
        area.set_long_press_enabled(true);
        Ok(Self {
            env,
            current: prepared.current,
            model: prepared.model,
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            scope,
            loader,
            view,
            area,
            extent: prepared.extent,
            previews,
            requests: Vec::new(),
            date: Box::new(|| Ok(SaveDate::now())),
            fault: None,
        })
    }
    pub fn complete(&self) -> bool {
        self.model.complete
    }
    /// Observe retained input/Questions; never publish, poll or mutate a child.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        if self.fault.is_some()
            || (self.model.mode == Mode::Load && self.model.selected != 0)
            || self.model.complete
            || self.model.closing
            || self.model.loaded
        {
            return None;
        }
        let mut question = None;
        let mut recovery = None;
        let mut input = false;
        self.scope.draw_ref(|child| match child {
            Owned::Save { task, .. } => {
                recovery = match task.diagnostic_recovery() {
                    Some((2, 0, true)) => Some("save_recovery_retry"),
                    Some((2, 1, true)) => Some("save_recovery_return"),
                    Some((2, 0, false)) => Some("save_recovery_retry_only"),
                    _ => None,
                };
            }
            Owned::Question(dialog) if matches!(self.model.request, 1 | 2) => {
                if let Some((model, _)) = &dialog.ready {
                    // request2 also covers MissingAppend's one-button notice.
                    // The actual Load confirmation has two buttons/defaultNo.
                    if model.phase() == 2 && model.buttons().len() == 2 {
                        question = match (self.model.mode, self.model.request, model.selected()) {
                            (Mode::Load, 2, 1) => Some("load_question"),
                            (Mode::Load, 2, 0) => Some("load_question_yes"),
                            (Mode::Save, 1, 1) => Some("save_question"),
                            (Mode::Save, 1, 0) => Some("save_question_yes"),
                            _ => None,
                        };
                    }
                }
            }
            Owned::Private(task) if task.kind == Kind::Input && !task.done => input = true,
            _ => {}
        });
        if recovery.is_some() {
            return recovery;
        }
        if question.is_some() {
            return question;
        }
        (input
            && self.scope.iter().all(|child| {
                matches!(child, Owned::Private(task) if task.done || task.kind == Kind::Input)
            })
            && self.model.progress == 1.0
            && self.model.request == 0
            && self.model.scroll.is_idle())
        .then_some(if self.model.mode == Mode::Save { "save_menu" } else { "load_menu" })
    }
    pub fn retire(mut self, settings: &mut SaveData) {
        self.previews.retire_jobs();
        self.previews
            .synchronize(settings)
            .expect("Retired capture has valid TIPS IDs");
    }
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Invalid SaveMenu physical extent"
        );
        if self.extent != extent {
            self.area.disable(&mut self.model);
            self.area = InputArea::new(extent.0, extent.1);
            self.area.set_long_press_enabled(true);
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
                Owned::Save { task, .. } => task.cancel_input(),
                Owned::Private(_) => {}
            }
        }
    }
    pub fn has_save_task(&self) -> bool {
        self.scope
            .iter()
            .any(|child| matches!(child, Owned::Save { task, .. } if task.blocks_close()))
    }
    #[cfg(test)]
    pub(super) fn reference_save_failure(&self) -> Option<(i32, i32, bool)> {
        self.scope.iter().find_map(|child| match child {
            Owned::Save { task, .. } => task.reference_recovery(),
            _ => None,
        })
    }
    #[cfg(test)]
    pub(super) fn reference_save_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.scope
            .iter()
            .flat_map(|child| match child {
                Owned::Save { task, .. } => task.reference_retirements(),
                _ => Vec::new(),
            })
            .collect()
    }
    /// Join preview producers before a desktop writer freezes global progress.
    /// Acquired TIPS can outlive the last poll; date/VM queues stay untouched.
    pub fn quiesce_progress(&mut self, settings: &mut SaveData) -> Result<()> {
        self.previews.retire_jobs();
        self.previews.synchronize(settings)
    }
    pub fn state(&self) -> ViewState {
        self.model.view_state(self.scrollbar)
    }
    #[cfg(test)]
    pub(super) fn reference_input_ready(&self) -> bool {
        let mut borrowed_input = false;
        self.scope.draw_ref(|mode| {
            if matches!(mode, Owned::Private(task) if task.kind == Kind::Input && !task.done) {
                borrowed_input = true;
            }
        });
        borrowed_input
            && self.scope.iter().all(|mode| {
                matches!(mode, Owned::Private(task) if task.done || task.kind == Kind::Input)
            })
            && self.model.progress == 1.0
            && !self.model.complete
            && !self.model.closing
            && !self.model.loaded
            && self.model.request == 0
            && self.model.scroll.is_idle()
    }
    pub fn poll(
        &mut self,
        settings: &mut SaveData,
        slots: &mut Slots,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        date: Option<&mut dyn FnMut() -> Result<SaveDate>>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid SaveMenu frame delta"
        );
        if let Some(fault) = &self.fault {
            bail!("SaveMenu failed:{fault}");
        }
        let result = (|| {
            self.previews.synchronize(settings)?;
            let ready = self.loader.poll()?;
            self.model.set_slots(metadata(slots));
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model: &mut self.model,
                    status: if ready { 2 } else { 1 },
                    runtime: Runtime {
                        env: &self.env,
                        current: &mut self.current,
                        settings,
                        slots,
                        loader: &mut self.loader,
                        area: &mut self.area,
                        previews: &mut self.previews,
                        date: date.unwrap_or(&mut *self.date),
                        pointer,
                        buttons,
                        extent: self.extent,
                        requests: &mut self.requests,
                        spawned: Vec::new(),
                    },
                },
            )?;
            self.model
                .advance_clocks(delta, self.loader.atlas.is_some(), &mut self.scrollbar);
            self.previews.poll_rows(slots, &self.state())?;
            self.previews.synchronize(settings)?;
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        let state = self.state();
        self.previews.prepare(context, &state)?;
        self.view
            .synchronize(&self.env.server, &state, self.previews.rows())?;
        for mode in self.scope.iter() {
            match mode {
                Owned::Question(dialog) => {
                    if let Some((q, v)) = &dialog.ready {
                        v.prepare(context, q);
                    }
                }
                Owned::Save { task, .. } => task.prepare(context),
                _ => {}
            }
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.view.render(
            &self.state(),
            self.previews.rows(),
            self.loader.atlas.as_deref(),
            &self.env.common.system,
            &self.previews.thumbnail_sources(),
            pass,
        );
        self.scope.draw_ref(|mode| match mode {
            Owned::Question(dialog) => {
                if let Some((q, v)) = &dialog.ready {
                    v.render(pass, q);
                }
            }
            Owned::Save { task, .. } => task.render(pass),
            _ => {}
        });
    }
    #[cfg(test)]
    pub(super) fn reference_current(&self) -> Option<&RuntimeGameData> {
        self.current.as_ref()
    }
    #[cfg(test)]
    pub(super) fn reference_question(&self) -> Option<(i32, i32)> {
        self.scope.iter().find_map(|mode| match mode {
            Owned::Question(dialog) => dialog
                .ready
                .as_ref()
                .map(|(q, _)| (q.phase(), q.selected())),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
