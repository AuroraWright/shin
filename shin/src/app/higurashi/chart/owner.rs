//! Concrete updated4c200/4687c Chart. Atlas publication precedes First status2;
//! one mixed scope owns private tasks and the captured-index Question child.
use super::{
    catalog::Catalog,
    model::{self, Action, Kind, Model, Services, Task},
    view::View,
};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            mode_stack::{Mode, ModePush, ModeStack},
            question::{self, Question, view::View as QuestionView},
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, save_menu::Environment},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{time::Ticks, vm::command::types::Volume};
use shin_render::render_pass::RenderPass;
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Debug)]
pub(in crate::app::higurashi) enum Request {
    ReturnSource(u32),
    StartAdv { data: RuntimeGameData, flags: u32 },
}
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
pub(in crate::app::higurashi) type ReferenceGate = Box<
    dyn FnOnce(std::sync::Weak<CharacterTextures>) -> futures::future::BoxFuture<'static, ()>
        + Send,
>;
enum Gate {
    None,
    #[cfg(test)]
    Hold(ReferenceGate),
}
struct Loader {
    job: Option<Job<()>>,
    published: Arc<Mutex<Option<Arc<CharacterTextures>>>>,
    completed: bool,
}
impl Loader {
    fn new(server: Arc<crate::asset::system::AssetServer>, gate: Gate) -> Self {
        let published = Arc::new(Mutex::new(None));
        let destination = published.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load("/chart.txa").await?;
            // Desktop malformed-archive guard. The native lookup crashes on a missing
            // artwork name; no fallback atlas or empty replacement is published here.
            for name in [
                "parts",
                "window",
                "obi",
                "caption",
                "books",
                "arrow",
                "close",
                "help_chart",
                "key",
                "cursor",
            ] {
                ensure!(atlas.contains(name), "Chart archive is missing {name}");
            }
            //4cab0..4cad8 publishes/retains/releases BEFORE the worker returns1.
            // Rendering reads this owner independently of First's executor status.
            *destination.lock() = Some(atlas.clone());
            match gate {
                Gate::None => {}
                #[cfg(test)]
                Gate::Hold(callback) => callback(Arc::downgrade(&atlas)).await,
            }
            Ok(())
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            published,
            completed: false,
        }
    }
    fn poll_status(&mut self) -> Result<u32> {
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|j| j.task.as_mut())
            .and_then(|t| t.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            output?;
            ensure!(
                self.published.lock().is_some(),
                "Completed Chart worker has no published atlas"
            );
            self.completed = true;
        }
        Ok(if self.completed { 2 } else { 1 })
    }
    fn atlas(&self) -> Option<Arc<CharacterTextures>> {
        self.published.lock().clone()
    }
    fn retire_first(&mut self) {
        self.job.take();
    }
}
struct Dialog {
    index: i32,
    job: Option<Job<(Question, QuestionView)>>,
    ready: Option<(Question, QuestionView)>,
    extent: (u16, u16),
}
impl Dialog {
    fn new(server: Arc<crate::asset::system::AssetServer>, index: i32, prompt: String) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            QuestionView::load(&server, &prompt, &["はい", "いいえ"], 1, true).await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            index,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            ready: None,
            extent: (1280, 720),
        }
    }
    fn complete(&self) -> bool {
        self.ready.as_ref().is_some_and(|(q, _)| q.complete())
    }
    fn cancel_input(&mut self) {
        if let Some((q, _)) = &mut self.ready {
            q.cancel_input(&mut question::CancelOnly);
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &FrameServices<'_, '_>,
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
                    Volume(1.),
                );
            }
            fn callback(&mut self, event: question::Callback) {
                self.events.push(event);
            }
        }
        let mut callbacks = Callbacks {
            env: frame.env,
            events: Vec::new(),
        };
        if let Some((q, _)) = &mut self.ready {
            if self.extent != frame.extent {
                q.resize_input(frame.extent, &mut callbacks);
                self.extent = frame.extent;
            }
            q.poll(
                delta,
                u32::from(permission),
                frame.input.buttons.pressed,
                frame.input.buttons.repeat,
                frame.pointer,
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
}
impl Owned {
    fn complete(&self) -> bool {
        match self {
            Self::Private(t) => t.done,
            Self::Question(d) => d.complete(),
        }
    }
}
struct FrameServices<'a, 'p> {
    env: &'a Environment,
    area: &'a mut InputArea,
    loader: &'a mut Loader,
    extent: (u16, u16),
    input: model::Input,
    pointer: &'a PointerFrame<'p>,
    requests: &'a mut VecDeque<Request>,
    next_seed: &'a Arc<dyn Fn() -> u32 + Send + Sync>,
    spawned: Vec<Owned>,
}
impl Services for FrameServices<'_, '_> {
    fn input(&mut self, m: &mut Model, permission: bool) {
        let captured = if permission {
            self.area.update(m, self.pointer)
        } else {
            self.area.disable(m);
            false
        };
        m.poll_controls(permission, captured, self.input);
    }
    fn event(&mut self, event: Action) -> Result<()> {
        match event {
            Action::Cue(name) => self
                .env
                .sounds
                .lock()
                .play(&self.env.server, name, Volume(1.)),
            Action::DisableInput => {}
            Action::Question { index, prompt } => self.spawned.push(Owned::Question(Dialog::new(
                self.env.server.clone(),
                index,
                prompt,
            ))),
            Action::ReturnSource(stage) => self.requests.push_back(Request::ReturnSource(stage)),
            Action::StartAdv { data, flags } => {
                self.requests.push_back(Request::StartAdv { data, flags })
            }
        }
        Ok(())
    }
    fn next_seed(&mut self) -> u32 {
        (self.next_seed)()
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    status: u32,
    services: FrameServices<'a, 'p>,
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(self.complete())
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
                    frame.status,
                    delta,
                    permission,
                    &mut frame.services,
                )? {
                    pushes.push(Self::Private(task));
                }
            }
            Self::Question(dialog) => {
                for event in dialog.poll(delta, permission, &frame.services)? {
                    frame
                        .model
                        .question_response(dialog.index, event.stage, event.button);
                }
                for task in frame.model.take_pending_tasks() {
                    pushes.push(Self::Private(task));
                }
                for event in frame.model.take_actions() {
                    frame.services.event(event)?;
                }
            }
        }
        for child in frame.services.spawned.drain(..) {
            pushes.push(child);
        }
        Ok(0)
    }
}
struct Retire<'a> {
    loader: &'a mut Loader,
}
impl Mode<Retire<'_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(self.complete())
    }
    fn retire(&mut self, s: &mut Retire<'_>) {
        if matches!(
            self,
            Self::Private(Task {
                kind: Kind::First,
                ..
            })
        ) {
            s.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        _: Ticks,
        _: bool,
        _: &mut Retire<'_>,
        _: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        unreachable!("Chart destruction never polls callbacks")
    }
}
pub(in crate::app::higurashi) struct Owner {
    model: Option<Model>,
    view: Option<View>,
    scrollbar: Scrollbar,
    scope: ModeStack<Owned>,
    loader: Loader,
    area: InputArea,
    extent: (u16, u16),
    requests: VecDeque<Request>,
    fault: Option<String>,
    next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    env: Environment,
}
impl Drop for Owner {
    fn drop(&mut self) {
        //484ec..485a0 destroys group clocks/text/chrome/graph storage before d0f40.
        //Desktop Model combines the trivial available-list storage with graph state;
        //no byte-for-byte native allocation ordering is claimed for those pure Vecs.
        self.model.take();
        self.view.take();
        self.scope.clear_with(&mut Retire {
            loader: &mut self.loader,
        });
        self.loader.retire_first(); // Covers an early fault before scoped admission.
        //485bc..485d0 clears/releases the published atlas AFTER every child worker.
        self.loader.published.lock().take();
    }
}
impl Owner {
    pub fn new(
        env: Environment,
        catalog: Arc<Catalog>,
        group: i32,
        mask: i32,
        unlocks: &[u32],
        route: &[u32],
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> Result<Self> {
        Self::construct(
            env,
            catalog,
            group,
            mask,
            unlocks,
            route,
            extent,
            next_seed,
            Gate::None,
        )
    }
    fn construct(
        env: Environment,
        catalog: Arc<Catalog>,
        group: i32,
        mask: i32,
        unlocks: &[u32],
        route: &[u32],
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
        gate: Gate,
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Chart requires a nonzero input extent"
        );
        let model = Model::new(catalog, group, mask, unlocks, route)?;
        let mut view = View::new(env.common.font.clone());
        view.synchronize(&env.server, &model)?;
        let mut scope = ModeStack::default();
        scope.push(Owned::Private(Task {
            kind: Kind::First,
            done: false,
        }));
        let loader = Loader::new(env.server.clone(), gate);
        Ok(Self {
            model: Some(model),
            view: Some(view),
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            scope,
            loader,
            area: InputArea::new(extent.0, extent.1),
            extent,
            requests: VecDeque::new(),
            fault: None,
            next_seed,
            env,
        })
    }
    pub(super) fn model(&self) -> &Model {
        self.model.as_ref().unwrap()
    }
    pub fn complete(&self) -> bool {
        self.model().complete
    }
    /// The caller must retain a complete queued request before deleting this
    /// owner/source or attempting a fallible actual scene factory.
    #[cfg(test)]
    pub fn pending_request(&self) -> Option<&Request> {
        self.requests.front()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.requests.pop_front()
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Chart input extent");
        if extent != self.extent {
            self.area.disable(self.model.as_mut().unwrap());
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
            for child in self.scope.iter_mut() {
                if let Owned::Question(dialog) = child {
                    dialog.cancel_input();
                }
            }
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(self.model.as_mut().unwrap());
        for child in self.scope.iter_mut() {
            if let Owned::Question(d) = child {
                d.cancel_input();
            }
        }
    }
    #[cfg(test)]
    pub fn has_save_task(&self) -> bool {
        false
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        input: model::Input,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid Chart frame delta"
        );
        if let Some(f) = &self.fault {
            bail!("Chart failed:{f}");
        }
        let result = (|| {
            let status = self.loader.poll_status()?;
            let model = self.model.as_mut().unwrap();
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model,
                    status,
                    services: FrameServices {
                        env: &self.env,
                        area: &mut self.area,
                        loader: &mut self.loader,
                        extent: self.extent,
                        input,
                        pointer,
                        requests: &mut self.requests,
                        next_seed: &self.next_seed,
                        spawned: Vec::new(),
                    },
                },
            )?;
            let ready = self.loader.atlas().is_some();
            model.advance_clocks(delta, ready, &mut self.scrollbar);
            self.view
                .as_mut()
                .unwrap()
                .synchronize(&self.env.server, model)?;
            Ok(())
        })();
        if let Err(e) = &result {
            self.fault = Some(format!("{e:#}"));
        }
        result
    }
    pub fn prepare(&self, context: &mut PreRenderContext) {
        for child in self.scope.iter() {
            if let Owned::Question(dialog) = child {
                if let Some((q, v)) = &dialog.ready {
                    v.prepare(context, q);
                }
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let atlas = self.loader.atlas();
        self.view.as_ref().unwrap().render(
            self.model(),
            self.scrollbar,
            atlas.as_deref(),
            &self.env.common.system,
            false,
            pass,
        )?;
        self.scope.draw_ref(|child| {
            if let Owned::Question(d) = child {
                if let Some((q, v)) = &d.ready {
                    v.render(pass, q);
                }
            }
        });
        Ok(())
    }
    #[cfg(test)]
    pub fn new_with_reference_gate(
        env: Environment,
        catalog: Arc<Catalog>,
        group: i32,
        mask: i32,
        unlocks: &[u32],
        route: &[u32],
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
        gate: ReferenceGate,
    ) -> Result<Self> {
        Self::construct(
            env,
            catalog,
            group,
            mask,
            unlocks,
            route,
            extent,
            next_seed,
            Gate::Hold(gate),
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_state(&self) -> (bool, bool, bool, u32, i32, i32) {
        let m = self.model();
        (
            self.loader.atlas().is_some(),
            self.loader.completed,
            !m.closing
                && self.scope.iter().any(|v| {
                    matches!(
                        v,
                        Owned::Private(Task {
                            kind: Kind::Input,
                            done: false
                        })
                    )
                }),
            m.progress.to_bits(),
            m.current_group,
            m.groups[m.current_group as usize].selected,
        )
    }
    #[cfg(test)]
    pub fn reference_selected_entry(&self) -> Option<(u32, i32)> {
        let model = self.model();
        let selected = model.groups[model.current_group as usize].selected;
        let node = model.nodes.get(usize::try_from(selected).ok()?)?;
        Some((
            node.source_id,
            i32::from(model.catalog.records[node.record].value as i16),
        ))
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_cursor(&self) -> (u32, u32, u32) {
        let m = self.model();
        let g = &m.groups[m.current_group as usize];
        (
            g.x.value().to_bits(),
            g.y.value().to_bits(),
            g.scroll.value().to_bits(),
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_question(&self) -> Option<(i32, i32, i32)> {
        self.scope.iter().find_map(|v| {
            if let Owned::Question(d) = v {
                d.ready
                    .as_ref()
                    .map(|(q, _)| (d.index, q.phase(), q.selected()))
            } else {
                None
            }
        })
    }
    #[cfg(test)]
    pub fn reference_first_job(&self) -> Option<(bool, bool)> {
        self.loader
            .job
            .as_ref()
            .map(|job| (job.task.is_some(), job.retirement.is_finished()))
    }
}
