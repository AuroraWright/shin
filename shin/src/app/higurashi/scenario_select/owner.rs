//! Updated b3904/b4d68 ScenarioSelect owner. The worker publishes its atlas
//! before executor completion; First owns a separate mask and plain capture fade.
use super::{Action, Kind, Model, SceneRequest, Services, Task, view::View};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            music_commands::MusicCommands,
            question::{self, Question, view::View as QuestionView},
            runner::reconstruction_mode::Overlay,
        },
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, save_menu::Environment},
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use glam::Vec2;
use parking_lot::Mutex;
use shin_core::{format::save::higurashi::SaveData, time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::{collections::VecDeque, sync::Arc};

pub(in crate::app::higurashi) type Request = SceneRequest;

struct Job<T> {
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    retirement: shin_tasks::LoaderRetirement,
    retired: bool,
}
impl<T> Job<T> {
    fn new(task: shin_tasks::AsyncTask<Result<T>>) -> Self {
        Self {
            retirement: task.retirement_handle().unwrap(),
            task: Some(task),
            retired: false,
        }
    }
    fn poll(&mut self) -> Option<Result<T>> {
        let result = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        Some(result)
    }
    fn retire(&mut self) {
        if !self.retired {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
            self.retired = true;
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.retire();
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) enum ReferencePublication {
    Atlas,
    Mask,
}
#[cfg(test)]
pub(in crate::app::higurashi) struct ReferenceResources {
    pub atlas: std::sync::Weak<CharacterTextures>,
    pub mask: Option<std::sync::Weak<MaskTexture>>,
}
#[cfg(test)]
pub(in crate::app::higurashi) type ReferenceGate =
    Box<dyn FnOnce(ReferenceResources) -> futures::future::BoxFuture<'static, ()> + Send>;
enum Gate {
    None,
    #[cfg(test)]
    Hold {
        at: ReferencePublication,
        callback: Option<ReferenceGate>,
    },
}
impl Gate {
    #[cfg(test)]
    async fn observe(
        &mut self,
        at: ReferencePublication,
        atlas: std::sync::Weak<CharacterTextures>,
        mask: Option<std::sync::Weak<MaskTexture>>,
    ) {
        if let Self::Hold {
            at: wanted,
            callback,
        } = self
        {
            if *wanted == at {
                if let Some(callback) = callback.take() {
                    callback(ReferenceResources { atlas, mask }).await;
                }
            }
        }
    }
}

struct First {
    job: Option<Job<()>>,
    mask: Arc<Mutex<Option<Arc<MaskTexture>>>>,
    overlay: Option<Overlay>,
    completed: bool,
}
impl First {
    fn new(
        server: Arc<AssetServer>,
        published: Arc<Mutex<Option<Arc<CharacterTextures>>>>,
        snapshot: Option<RenderTexture>,
        mut gate: Gate,
    ) -> Self {
        // b67d8..b6860 captures/retains the previous whole scene BEFORE27520.
        let overlay = snapshot.map(|snapshot| Overlay {
            clock: crate::adv::higurashi::reconstruction_mode::OverlayClock::captured(true),
            snapshot,
            mask: None,
        });
        let mask = Arc::new(Mutex::new(None));
        let destination = mask.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load("/snrsel.txa").await?;
            // Desktop malformed-archive guard; native missing-name lookup fails.
            for name in [
                "ks_bg",
                "sce_menu",
                "cursor",
                "obi",
                "caption",
                "close",
                "help_snrsel",
                "key",
            ] {
                ensure!(
                    atlas.contains(name),
                    "ScenarioSelect archive is missing {name}"
                );
            }
            // b7d2c..b7d34: parent+b8 publication precedes even the mask lookup.
            #[cfg(test)]
            let atlas_weak = Arc::downgrade(&atlas);
            // No extra worker Arc may keep the atlas alive while the next load
            // waits: native releases its temporary atlas before mask lookup.
            *published.lock() = Some(atlas);
            #[cfg(test)]
            gate.observe(ReferencePublication::Atlas, atlas_weak.clone(), None)
                .await;
            let loaded: Arc<MaskTexture> = server.load("/default.msk").await?;
            #[cfg(test)]
            let mask_weak = Arc::downgrade(&loaded);
            *destination.lock() = Some(loaded);
            #[cfg(test)]
            gate.observe(ReferencePublication::Mask, atlas_weak, Some(mask_weak))
                .await;
            // Keep the cfg-only publication gate out of production behavior.
            let _ = &mut gate;
            Ok(())
        });
        Self {
            job: Some(Job::new(task)),
            mask,
            overlay,
            completed: false,
        }
    }
    fn status(&mut self) -> Result<u32> {
        if let Some(output) = self.job.as_mut().and_then(Job::poll) {
            output?;
            ensure!(
                self.mask.lock().is_some(),
                "Completed ScenarioSelect worker has no mask"
            );
            self.completed = true;
        }
        Ok(if self.completed { 2 } else { 1 })
    }
    fn synchronize(&mut self, task: &Task) {
        if let Some(overlay) = &mut self.overlay {
            overlay.clock = task.capture.as_ref().expect("First metadata clock").clone();
        }
    }
}
impl Drop for First {
    fn drop(&mut self) {
        // b6a5c cancels/joins, then5531c drops capture, then mask+20, job+18.
        if let Some(job) = &mut self.job {
            job.retire();
        }
        self.overlay.take();
        self.mask.lock().take();
        self.job.take();
    }
}

struct Contact<'a> {
    model: &'a mut Model,
    seed: &'a mut dyn FnMut() -> u32,
}
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, position: Vec2) -> bool {
        self.model.probe_contact(position)
    }
    fn reset(&mut self) {
        self.model.reset_contact();
    }
    fn press(&mut self, _: Vec2) {
        self.model.press_contact();
    }
    fn release(&mut self, _: Vec2) {
        self.model.release_contact(self.seed);
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.model.drag_contact();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {
        self.model.cancel_contact();
    }
}

struct Dialog {
    job: Option<Job<(Question, QuestionView)>>,
    ready: Option<(Question, QuestionView)>,
    extent: (u16, u16),
}
impl Dialog {
    fn new(server: Arc<AssetServer>) -> Self {
        let job = shin_tasks::async_io::spawn_loader(async move {
            QuestionView::load(&server, super::EXIT_PROMPT, &super::EXIT_LABELS, 1, true).await
        });
        Self {
            job: Some(Job::new(job)),
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
        services: &FrameServices<'_, '_>,
    ) -> Result<Vec<question::Callback>> {
        if let Some(output) = self.job.as_mut().and_then(Job::poll) {
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
            env: services.env,
            events: Vec::new(),
        };
        if let Some((q, _)) = &mut self.ready {
            if self.extent != services.extent {
                q.resize_input(services.extent, &mut callbacks);
                self.extent = services.extent;
            }
            q.poll(
                delta,
                u32::from(permission),
                services.buttons.pressed,
                services.buttons.repeat,
                services.pointer,
                false,
                &mut callbacks,
            );
        }
        Ok(callbacks.events)
    }
}

enum Owned {
    Private { task: Task, first: Option<First> },
    Question(Dialog),
}
struct FrameServices<'a, 'p> {
    env: &'a Environment,
    area: &'a mut InputArea,
    extent: (u16, u16),
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    music: &'a mut dyn MusicCommands,
    requests: &'a mut VecDeque<Request>,
    next_seed: &'a Arc<dyn Fn() -> u32 + Send + Sync>,
    spawned: Vec<Owned>,
}
impl Services for FrameServices<'_, '_> {
    fn next_seed(&mut self) -> u32 {
        (self.next_seed)()
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let mut seed = || (self.next_seed)();
        let captured = {
            let mut contact = Contact {
                model: &mut *model,
                seed: &mut seed,
            };
            if permission {
                self.area.update(&mut contact, self.pointer)
            } else {
                self.area.disable(&mut contact);
                false
            }
        };
        model.controls(permission, captured, self.buttons, &mut seed);
        Ok(())
    }
    fn event(&mut self, event: Action) -> Result<()> {
        match event {
            Action::Cue(name) => self
                .env
                .sounds
                .lock()
                .play(&self.env.server, name, Volume(1.)),
            Action::DisableInput => {}
            Action::PlayBgm { id, fade, loops } => {
                self.music.play_bgm(id, fade as i32, loops, 1.)?
            }
            Action::StopBgm(fade) => self.music.stop_bgm(fade as i32),
            Action::ExitQuestion => self
                .spawned
                .push(Owned::Question(Dialog::new(self.env.server.clone()))),
            Action::Request(request) => self.requests.push_back(request),
        }
        Ok(())
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    services: FrameServices<'a, 'p>,
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(match self {
            Self::Private { task, .. } => task.done,
            Self::Question(d) => d.complete(),
        })
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Private { task, first } => {
                let status = if matches!(task.kind, Kind::First(0)) {
                    first.as_mut().expect("First resource owner").status()?
                } else {
                    2
                };
                for task in
                    task.poll_native(frame.model, status, delta, permission, &mut frame.services)?
                {
                    pushes.push(Self::Private { task, first: None });
                }
                if let Some(first) = first {
                    first.synchronize(task);
                }
            }
            Self::Question(dialog) => {
                for event in dialog.poll(delta, permission, &frame.services)? {
                    // b77a0 acts only on stage0 + Yes0; stage1 is deliberately inert.
                    frame.model.question_callback(event.stage, event.button);
                }
                for task in frame.model.take_pending_tasks() {
                    pushes.push(Self::Private { task, first: None });
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

pub(in crate::app::higurashi) struct Owner {
    model: Option<Model>,
    view: View,
    scope: ModeStack<Owned>,
    published: Arc<Mutex<Option<Arc<CharacterTextures>>>>,
    area: InputArea,
    extent: (u16, u16),
    requests: VecDeque<Request>,
    fault: Option<String>,
    next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    env: Environment,
}
impl Drop for Owner {
    fn drop(&mut self) {
        // b4b04 drops clocks/full GameData first, b4ce0 clears atlas BEFORE
        // b4cfc d0f40. First then cancels/joins, releases capture/mask/job.
        self.model.take();
        self.published.lock().take();
        drop(std::mem::take(&mut self.scope));
        // A worker already inside publication may publish while joining. The
        // shared desktop cell stays alive through join and releases any such
        // late Arc when its last owner dies; no visibility/cancel gate is added.
    }
}
impl Owner {
    pub fn new_retained(
        env: Environment,
        kind: u32,
        selected: i32,
        append: bool,
        data: &RuntimeGameData,
        settings: &SaveData,
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
        capture: &mut Option<RenderTexture>,
    ) -> Result<Self> {
        Self::construct(
            env,
            kind,
            selected,
            append,
            data,
            settings,
            extent,
            next_seed,
            capture,
            Gate::None,
        )
    }
    fn construct(
        env: Environment,
        kind: u32,
        selected: i32,
        append: bool,
        data: &RuntimeGameData,
        settings: &SaveData,
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
        capture: &mut Option<RenderTexture>,
        gate: Gate,
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "ScenarioSelect requires a nonzero input extent"
        );
        ensure!(
            kind != 5
                || env.common.scenario.variant
                    == shin_core::format::scenario::higurashi::HigurashiVariant::Updated,
            "Origin ScenarioSelect requires updated Higurashi"
        );
        let origin_bgm = env
            .common
            .scenario
            .bgm_info
            .iter()
            .position(|row| row.name.as_str() == "org/2/it_moves")
            .map(|index| index as i32);
        // All fallible input/schema checks precede ownership transfer or jobs.
        // Missing Origin BGM is the native -1/stop branch, never a fallback ID.
        let append =
            super::super::main_title::append_story_available(append, env.common.scenario.variant);
        let mut model = Model::new(
            kind,
            selected,
            append,
            data,
            settings,
            origin_bgm,
            capture.is_some(),
        )?;
        let tasks = model.take_pending_tasks();
        ensure!(
            tasks.len() == 1 && matches!(tasks[0].kind, Kind::First(0)),
            "Invalid ScenarioSelect First schema"
        );
        let mut task = tasks.into_iter().next().unwrap();
        let published = Arc::new(Mutex::new(None));
        let mut first = First::new(env.server.clone(), published.clone(), capture.take(), gate);
        first.synchronize(&task);
        task.done = false;
        let mut scope = ModeStack::default();
        scope.push(Owned::Private {
            task,
            first: Some(first),
        });
        Ok(Self {
            model: Some(model),
            view: View::new(),
            scope,
            published,
            area: InputArea::new(extent.0, extent.1),
            extent,
            requests: VecDeque::new(),
            fault: None,
            next_seed,
            env,
        })
    }
    #[cfg(test)]
    pub fn data(&self) -> &RuntimeGameData {
        &self.model.as_ref().unwrap().data
    }
    #[cfg(test)]
    pub fn pending_request(&self) -> Option<&Request> {
        self.requests.front()
    }
    pub fn take_request(&mut self) -> Option<Request> {
        self.requests.pop_front()
    }
    pub fn has_save_task(&self) -> bool {
        false
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Invalid ScenarioSelect input extent"
        );
        if extent != self.extent {
            self.cancel_input();
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        let mut seed = || (self.next_seed)();
        self.area.disable(&mut Contact {
            model: self.model.as_mut().unwrap(),
            seed: &mut seed,
        });
        for child in self.scope.iter_mut() {
            if let Owned::Question(dialog) = child {
                dialog.cancel_input();
            }
        }
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        music: &mut dyn MusicCommands,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid ScenarioSelect frame delta"
        );
        if let Some(fault) = &self.fault {
            bail!("ScenarioSelect failed: {fault}");
        }
        let result = (|| {
            let model = self.model.as_mut().unwrap();
            if let Some(atlas) = self.published.lock().as_ref() {
                let size = atlas.get("sce_menu").size();
                model.constrain_artwork((size.width, size.height));
                model.publish_resources();
            }
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model: &mut *model,
                    services: FrameServices {
                        env: &self.env,
                        area: &mut self.area,
                        extent: self.extent,
                        buttons,
                        pointer,
                        music,
                        requests: &mut self.requests,
                        next_seed: &self.next_seed,
                        spawned: Vec::new(),
                    },
                },
            )?;
            // b4d68 polls the entire mixed scope before blackout/rows/close clocks.
            model.advance_clocks(delta);
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn prepare(&self, context: &mut PreRenderContext) {
        for child in self.scope.iter() {
            if let Owned::Question(dialog) = child {
                if let Some((q, view)) = &dialog.ready {
                    view.prepare(context, q);
                }
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let atlas = self.published.lock().clone();
        let mut state = self.model.as_ref().unwrap().view_state();
        // b4e04 reads parent+b8 directly, even when executor status is not2.
        state.ready = atlas.is_some();
        self.view.render(&state, atlas.as_deref(), false, pass);
        self.scope.draw_ref(|child| match child {
            Owned::Private {
                first: Some(first), ..
            } => {
                if let Some(overlay) = &first.overlay {
                    overlay.render(pass);
                }
            }
            Owned::Question(dialog) => {
                if let Some((q, view)) = &dialog.ready {
                    view.render(pass, q);
                }
            }
            _ => {}
        });
        Ok(())
    }
    #[cfg(test)]
    pub fn new_with_reference_gate(
        env: Environment,
        kind: u32,
        selected: i32,
        append: bool,
        data: &RuntimeGameData,
        settings: &SaveData,
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
        capture: &mut Option<RenderTexture>,
        at: ReferencePublication,
        gate: ReferenceGate,
    ) -> Result<Self> {
        Self::construct(
            env,
            kind,
            selected,
            append,
            data,
            settings,
            extent,
            next_seed,
            capture,
            Gate::Hold {
                at,
                callback: Some(gate),
            },
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_state(&self) -> (bool, bool, bool, Option<i32>, i32) {
        let mut completed = false;
        let mut phase = None;
        let mut input = false;
        for child in self.scope.iter() {
            if let Owned::Private { task, first } = child {
                match task.kind {
                    Kind::First(value) => {
                        phase = Some(value);
                        completed = first.as_ref().is_some_and(|first| first.completed);
                    }
                    Kind::Input => input |= !task.done,
                    _ => {}
                }
            }
        }
        (
            self.published.lock().is_some(),
            completed,
            input,
            phase,
            self.model.as_ref().unwrap().selected,
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_model(&self) -> &Model {
        self.model.as_ref().unwrap()
    }
    #[cfg(test)]
    pub fn reference_question(&self) -> Option<(i32, i32)> {
        self.scope.iter().find_map(|child| {
            if let Owned::Question(dialog) = child {
                dialog
                    .ready
                    .as_ref()
                    .map(|(q, _)| (q.phase(), q.selected()))
            } else {
                None
            }
        })
    }
}
