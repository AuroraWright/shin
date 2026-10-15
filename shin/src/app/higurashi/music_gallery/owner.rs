//! Concrete updated BGMMode owner candidate. Source-only/unregistered handoff:
//! root owns module registration, compilation, hardware and outer Globals.
use super::{Action, Model, Task, view::View};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            reconstruction_mode::{OverlayClock, overlay::DrawPacket},
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::save_menu::Environment,
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use glam::Vec4;
use parking_lot::Mutex;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::HigurashiScenario},
    primitives::color::FloatColor4,
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder, render_pass::RenderPass,
    render_texture::RenderTexture, shaders::types::buffer::VertexSource,
};
use std::{collections::VecDeque, sync::Arc};
#[cfg(test)]
mod tests;

/// All audio/global/platform effects remain at the parent boundary. Music's
/// absent GameData constructor parameter does not authorize projecting Globals.
pub(in crate::app::higurashi) trait Services {
    /// Apply one literal action or return an error. An Err leaves it unacknowledged
    /// for retry; a parent that can partially apply an action must own that retry
    /// token/idempotence itself. Successfully acknowledged actions are not replayed.
    fn event(&mut self, action: Action) -> Result<()>;
    /// Exact32d94 result. Called only at the native parent-tail point when the
    /// playing marker is nonnegative, after preceding successful play/stop actions.
    fn bgm_status(&self) -> u32;
}

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
    retired: bool,
}
impl Job {
    fn new(task: shin_tasks::AsyncTask<Result<()>>) -> Self {
        Self {
            retirement: task.retirement_handle().unwrap(),
            task: Some(task),
            retired: false,
        }
    }
    fn poll(&mut self) -> Option<Result<()>> {
        let result = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        Some(result)
    }
    fn retire(&mut self) {
        if self.retired {
            return;
        }
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        // Completed outputs may have left AsyncTask while decoded child owners
        // still carry this scope. Drain it before capture/mask/job retirement too.
        self.retirement.clone().retire(());
        self.retired = true;
    }
}
impl Drop for Job {
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
#[derive(Default)]
struct Published {
    // These are two independent native atomic owners, not a single completed
    // worker output. Moving to the slots releases each worker temporary first.
    atlas: Option<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
}
struct Loader {
    job: Option<Job>,
    published: Arc<Mutex<Published>>,
    snapshot: Option<RenderTexture>,
    completed: bool,
    fault: Option<String>,
}
impl Loader {
    fn new(server: Arc<AssetServer>, snapshot: Option<RenderTexture>, mut gate: Gate) -> Self {
        let published = Arc::new(Mutex::new(Published::default()));
        let destination = published.clone();
        //318a8 retained the capture before27520. No task receives a live source
        //scene or an implicit redraw/capture closure.
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load(super::ATLAS_PATH).await?;
            View::validate_atlas(&atlas)?;
            #[cfg(test)]
            let atlas_weak = Arc::downgrade(&atlas);
            //321d4/32238 publishes BEFORE the next mask lookup and before status2.
            //Move (rather than clone) so no worker Arc prolongs parent art lifetime.
            destination.lock().atlas = Some(atlas);
            #[cfg(test)]
            gate.observe(ReferencePublication::Atlas, atlas_weak.clone(), None)
                .await;
            let mask: Arc<MaskTexture> = server.load(super::MASK_PATH).await?;
            #[cfg(test)]
            let mask_weak = Arc::downgrade(&mask);
            destination.lock().mask = Some(mask);
            #[cfg(test)]
            gate.observe(ReferencePublication::Mask, atlas_weak, Some(mask_weak))
                .await;
            let _ = &mut gate;
            Ok(())
        });
        Self {
            job: Some(Job::new(task)),
            published,
            snapshot,
            completed: false,
            fault: None,
        }
    }
    fn status(&mut self) -> Result<u32> {
        if let Some(fault) = &self.fault {
            bail!("Music worker failed:{fault}");
        }
        if let Some(result) = self.job.as_mut().and_then(Job::poll) {
            if let Err(error) = result {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            let output = self.published.lock();
            ensure!(
                output.atlas.is_some() && output.mask.is_some(),
                "Completed Music worker has incomplete publications"
            );
            self.completed = true;
        }
        // Desktop scheduler boundary: pending may be0/1 natively. Both must
        // stay pending; only exactly2 is admitted by the original First model.
        Ok(if self.completed { 2 } else { 1 })
    }
    fn atlas(&self) -> Option<Arc<CharacterTextures>> {
        self.published.lock().atlas.clone()
    }
    fn mask(&self) -> Option<Arc<MaskTexture>> {
        self.published.lock().mask.clone()
    }
    fn retire_first(&mut self) {
        //31b70 actual284f8 cancellation/join. Never lock a publication mutex
        //during the join: a running worker may still be publishing either owner.
        if let Some(job) = &mut self.job {
            job.retire();
        }
        //31b78 capture,31b80..31b98 mask,31b9c..31bac job. Atlas is PARENT-owned.
        self.snapshot.take();
        self.published.lock().mask.take();
        self.job.take();
    }
    fn release_parent_atlas(&mut self) {
        //The worker has already joined through scoped First retirement. This
        //safe shared slot replaces a native raw parent pointer without gating
        //or suppressing any in-flight publication before that join.
        self.published.lock().atlas.take();
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.retire_first();
        self.release_parent_atlas();
    }
}

struct Frame<'a, 'p> {
    model: &'a mut Model,
    loader: &'a mut Loader,
    area: &'a mut InputArea,
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    parent: &'a mut dyn Services,
    next_id: &'a mut u64,
    failed: &'a mut Option<(u64, bool)>,
}
enum Resume {
    FirstStop(Ticks),
    Input {
        keep: bool,
        events: VecDeque<Action>,
        pushes: Vec<Task>,
    },
    CloseStop,
    CloseTitle,
}
struct Owned {
    id: u64,
    task: Task,
    resume: Option<Resume>,
}
impl Owned {
    fn new(id: u64, task: Task) -> Self {
        Self {
            id,
            task,
            resume: None,
        }
    }
    #[cfg(test)]
    fn pending_event(&self) -> Option<&Action> {
        match self.resume.as_ref()? {
            Resume::FirstStop(_) => Some(&Action::StopBgm(60)),
            Resume::Input { events, .. } => events.front(),
            Resume::CloseStop => Some(&Action::StopBgm(30)),
            Resume::CloseTitle => Some(&Action::ReturnTitle {
                selected: 16,
                mode: -1,
            }),
        }
    }
    /// Fallible bridge of31c1c/30c44/32078. The frozen pure Task's infallible
    /// service hook cannot suspend First at Stop60 before its clock advancement.
    /// Keep only the actual pending suffix here; do not unwind through a panic.
    fn poll_native(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
    ) -> Result<Vec<Task>> {
        if self.resume.is_none() {
            match &mut self.task {
                Task::First { phase, overlay } => {
                    if *phase == 0 && frame.loader.status()? == 2 {
                        overlay.masked(
                            30,
                            f32::from_bits(0x3e99999a),
                            0,
                            frame.loader.published.lock().mask.is_some(),
                        );
                        self.resume = Some(Resume::FirstStop(delta));
                    } else {
                        let mut pushes = vec![];
                        if *phase == 1 && !overlay.busy() {
                            frame.model.closing = false;
                            pushes.push(Task::Input { done: false });
                            *phase = -1;
                        }
                        overlay.advance(delta);
                        return Ok(pushes);
                    }
                }
                Task::Input { .. } => {
                    let captured = if permission {
                        frame.area.update(frame.model, frame.pointer)
                    } else {
                        frame.area.disable(frame.model);
                        false
                    };
                    let keep = frame.model.controller(permission, captured, frame.buttons);
                    self.resume = Some(Resume::Input {
                        keep,
                        //The actual5eb0c/InputArea disable already ran above.
                        //This pure-model marker is private, not a parent event.
                        events: frame
                            .model
                            .take_actions()
                            .into_iter()
                            .filter(|event| !matches!(event, Action::DisableInput))
                            .collect(),
                        pushes: frame.model.take_pending_tasks(),
                    });
                }
                Task::Close { .. } => {
                    if !frame.model.close_flash.is_idle() {
                        return Ok(vec![]);
                    }
                    self.resume = Some(Resume::CloseStop);
                }
            }
        }
        loop {
            match self.resume.as_mut().unwrap() {
                Resume::FirstStop(delta) => {
                    frame.parent.event(Action::StopBgm(60))?;
                    if let Task::First { phase, overlay } = &mut self.task {
                        *phase = 1;
                        overlay.advance(*delta);
                    }
                    self.resume = None;
                    return Ok(vec![]);
                }
                Resume::Input {
                    keep,
                    events,
                    pushes,
                } => {
                    while let Some(action) = events.front() {
                        frame.parent.event(action.clone())?;
                        events.pop_front();
                    }
                    if let Task::Input { done } = &mut self.task {
                        *done = !*keep;
                    }
                    let pushes = std::mem::take(pushes);
                    self.resume = None;
                    return Ok(pushes);
                }
                Resume::CloseStop => {
                    frame.parent.event(Action::StopBgm(30))?;
                    self.resume = Some(Resume::CloseTitle);
                }
                Resume::CloseTitle => {
                    frame.parent.event(Action::ReturnTitle {
                        selected: 16,
                        mode: -1,
                    })?;
                    if let Task::Close { done } = &mut self.task {
                        *done = true;
                    }
                    self.resume = None;
                    return Ok(vec![]);
                }
            }
        }
    }
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(self.task.complete())
    }
    fn retire(&mut self, frame: &mut Frame<'_, '_>) {
        if matches!(self.task, Task::First { .. }) {
            frame.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        let tasks = match self.poll_native(delta, permission, frame) {
            Ok(tasks) => tasks,
            Err(error) => {
                *frame.failed = Some((self.id, self.resume.is_some()));
                return Err(error);
            }
        };
        for task in tasks {
            let id = *frame.next_id;
            *frame.next_id += 1;
            pushes.push(Owned::new(id, task));
        }
        //30c44's return is the callable keep-alive, not permission propagation.
        Ok(0)
    }
}
struct Retire<'a> {
    loader: &'a mut Loader,
}
impl Mode<Retire<'_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(self.task.complete())
    }
    fn retire(&mut self, retire: &mut Retire<'_>) {
        if matches!(self.task, Task::First { .. }) {
            retire.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        _: Ticks,
        _: bool,
        _: &mut Retire<'_>,
        _: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        unreachable!("Music destruction never polls private callbacks")
    }
}

fn draw_overlay(
    clock: &OverlayClock,
    snapshot: Option<&RenderTexture>,
    mask: Option<&MaskTexture>,
    pass: &mut RenderPass,
) -> Result<()> {
    let mask = mask.filter(|_| clock.mask_present);
    ensure!(
        !clock.mask_present || mask.is_some(),
        "Music First lost its retained mask"
    );
    let dimensions = mask.map(|mask| {
        let s = mask.texture.size();
        (s.width, s.height)
    });
    let Some(packet) = clock.draw_packet(dimensions) else {
        return Ok(());
    };
    let snapshot = snapshot.ok_or_else(|| {
        anyhow::anyhow!("Music First has visible capture without its source bitmap")
    })?;
    let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1);
    let transform = crate::render::normalized_projection_matrix();
    match packet {
        DrawPacket::Plain { vertices, opacity } => pass.run(builder.build(
            RenderProgramWithArguments::Layer {
                output_kind: LayerShaderOutputKind::Layer,
                fragment_shader: LayerFragmentShader::Default,
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: snapshot.as_texture_source(),
                transform,
                color_multiplier: FloatColor4::from_rgba(opacity, opacity, opacity, opacity),
                fragment_shader_param: Vec4::ZERO,
            },
            DrawPrimitive::TrianglesStrip,
        )),
        DrawPacket::Masked { vertices, minmax } => pass.run(builder.build(
            RenderProgramWithArguments::HigurashiOverlay {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: snapshot.as_texture_source(),
                mask: mask.unwrap().texture.as_source(),
                transform,
                minmax,
            },
            DrawPrimitive::TrianglesStrip,
        )),
    }
    Ok(())
}

enum Stage {
    Scope(VecDeque<u64>),
    BeforeTail,
    TailClocks,
    Finalize,
}
struct PendingTick {
    delta: Ticks,
    stage: Stage,
}
pub(in crate::app::higurashi) struct Owner {
    model: Option<Model>,
    view: Option<View>,
    scope: ModeStack<Owned>,
    loader: Loader,
    area: Option<InputArea>,
    extent: (u16, u16),
    events: VecDeque<Action>,
    pending: Option<PendingTick>,
    next_id: u64,
    fault: Option<String>,
    env: Environment,
}
impl Drop for Owner {
    fn drop(&mut self) {
        //Actual2fd44..2fd9c, before scope/atlas/InputArea. Rust combines several
        //native pure storages; no byte-identical allocator layout is claimed.
        self.view.take();
        if let Some(mut model) = self.model.take() {
            drop(std::mem::replace(
                &mut model.close_flash,
                crate::layer::higurashi_properties::HigurashiTweener::new(0.),
            ));
            //Native scrollbar between close and scroll is pure value storage.
            drop(std::mem::replace(
                &mut model.scroll,
                crate::layer::higurashi_properties::HigurashiTweener::new(0.),
            ));
            while let Some(row) = model.rows.pop() {
                drop(row);
            }
            drop(model);
        }
        //2fda0 scope BEFORE2fda8 atomic parent atlas. CG/other owners differ.
        self.scope.clear_with(&mut Retire {
            loader: &mut self.loader,
        });
        self.loader.retire_first(); //Exception/early-admission fallback; idempotent.
        self.loader.release_parent_atlas();
        self.area.take();
    }
}
impl Owner {
    /// Preflight all fallible schema/font inputs before consuming the cached
    /// whole-scene capture or submitting First. No live scene redraw is supplied.
    pub fn new_retained(
        env: Environment,
        scenario: &HigurashiScenario,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
    ) -> Result<Self> {
        Self::construct(
            env,
            scenario,
            settings,
            capture,
            extent,
            services,
            Gate::None,
        )
    }
    fn construct(
        env: Environment,
        scenario: &HigurashiScenario,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
        gate: Gate,
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Music requires a nonzero physical input extent"
        );
        let area = InputArea::new(extent.0, extent.1);
        let mut scope = ModeStack::default();
        let mut model = Model::new(
            &scenario.music_box_info,
            &settings.save_vectors.unlocked_bgms,
            env.server.contains("/append.txt"),
        )?;
        //The pure model fixture bounds c0524. Its concrete native constructor
        //stores interval0/1,first0/end1 before any ready parent metrics sample.
        //Keep that valid range for art published before the first main poll.
        model.scrollbar = crate::adv::higurashi::browser_controls::Scrollbar {
            position: 0,
            visible: 1,
            first: 0,
            end: 1,
        };
        //Pure metrics/layout preflight before a fallible parent side effect.
        //The subsequent retained glyph preparation uses these immutable inputs.
        super::view::completion_layout(env.common.font.clone(), &model.completion_text)?;
        //Native completion/achievement precedes First capture/submission. There
        //is no success or capture.take on a parent callback failure.
        for action in model.take_actions() {
            services.event(action)?;
        }
        let mut view = View::new(env.common.font.clone());
        view.synchronize(&env.server, &model)?;
        let first = Task::first(capture.is_some());
        let loader = Loader::new(env.server.clone(), capture.take(), gate);
        scope.push(Owned::new(0, first));
        Ok(Self {
            model: Some(model),
            view: Some(view),
            scope,
            loader,
            area: Some(area),
            extent,
            events: VecDeque::new(),
            pending: None,
            next_id: 1,
            fault: None,
            env,
        })
    }
    fn model(&self) -> &Model {
        self.model.as_ref().unwrap()
    }
    fn flush_events(&mut self, services: &mut dyn Services) -> Result<()> {
        while let Some(action) = self.events.front() {
            services.event(action.clone())?;
            self.events.pop_front();
        }
        Ok(())
    }
    ///2fe58..2ffb4, ending at the queued ccb10 boundary. The pure Model fixture
    ///accepts an already sampled status value; this concrete owner must obtain
    ///it AFTER scroll advancement, rather than evaluate it as a call argument
    ///before the whole tail. Keep these service splits explicit.
    fn begin_tail(&mut self, delta: Ticks, services: &dyn Services) {
        let model = self.model.as_mut().unwrap();
        if model.velocity != 0. {
            model.move_scroll(model.scroll.value() - model.velocity, true);
            model.velocity *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if model.velocity.abs() < f32::from_bits(0x3dcccccd) {
                model.velocity = 0.;
            }
        }
        if self.loader.atlas().is_some() && model.document_height >= 818 {
            model.scrollbar = crate::adv::higurashi::browser_controls::Scrollbar {
                position: model.scroll.value() as i32,
                visible: 818,
                first: 0,
                end: model.document_height,
            };
        }
        model.scroll.update(delta);
        if model.playing >= 0 {
            if services.bgm_status() & 2 == 0 {
                model.playing = -1;
            }
            self.events.push_back(Action::PlatformByte);
        }
    }
    ///2ffb8..30000 follows a successfully acknowledged ccb10 request. Its delta
    ///is the original admitted callback delta, including a retry after an error.
    fn finish_tail(&mut self, delta: Ticks) {
        let model = self.model.as_mut().unwrap();
        model.close_flash.update(delta);
        for row in &mut model.rows {
            row.flash.update(delta);
        }
        model.elapsed += delta.as_f32() * f32::from_bits(0x3c888889);
    }
    /// Normal original2fe2c invokes scope permission1. A false permission here
    /// is the desktop focus/component InputArea-disable adapter; native full-parent
    /// corpus uses true. The app must retain approved Dpad/first-stick aliases.
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        services: &mut dyn Services,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid Music frame delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Music failed:{fault}");
        }
        let mut pending = if let Some(pending) = self.pending.take() {
            pending
        } else {
            let mut failed = None;
            let result = self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model: self.model.as_mut().unwrap(),
                    loader: &mut self.loader,
                    area: self.area.as_mut().unwrap(),
                    buttons,
                    pointer,
                    parent: services,
                    next_id: &mut self.next_id,
                    failed: &mut failed,
                },
            );
            if let Err(error) = result {
                if let Some((id, true)) = failed {
                    //Keep the exact borrowed draw/poll snapshot. Do not call
                    //ModeStack.poll again on retry: that would prune/repoll.
                    let mut ids = vec![];
                    self.scope.draw_ref(|task| ids.push(task.id));
                    ids.reverse();
                    let first = ids.iter().position(|candidate| *candidate == id).unwrap();
                    self.pending = Some(PendingTick {
                        delta,
                        stage: Stage::Scope(ids[first..].iter().copied().collect()),
                    });
                } else {
                    //File/decode failure is terminal for this actual factory.
                    self.fault = Some(format!("{error:#}"));
                }
                return Err(error);
            }
            PendingTick {
                delta,
                stage: Stage::BeforeTail,
            }
        };
        loop {
            match &mut pending.stage {
                Stage::Scope(ids) => {
                    while let Some(&id) = ids.front() {
                        let mut ignored = None;
                        let task = self.scope.iter_mut().find(|task| task.id == id).unwrap();
                        let tasks = match task.poll_native(
                            pending.delta,
                            false,
                            &mut Frame {
                                model: self.model.as_mut().unwrap(),
                                loader: &mut self.loader,
                                area: self.area.as_mut().unwrap(),
                                buttons,
                                pointer,
                                parent: services,
                                next_id: &mut self.next_id,
                                failed: &mut ignored,
                            },
                        ) {
                            Ok(tasks) => tasks,
                            Err(error) => {
                                if task.resume.is_none() {
                                    self.fault = Some(format!("{error:#}"));
                                }
                                self.pending = Some(pending);
                                return Err(error);
                            }
                        };
                        ids.pop_front();
                        for task in tasks {
                            let id = self.next_id;
                            self.next_id += 1;
                            self.scope.push(Owned::new(id, task));
                        }
                    }
                    pending.stage = Stage::BeforeTail;
                }
                Stage::BeforeTail => {
                    self.begin_tail(pending.delta, services);
                    pending.stage = Stage::TailClocks;
                }
                Stage::TailClocks => {
                    if let Err(error) = self.flush_events(services) {
                        self.pending = Some(pending);
                        return Err(error);
                    }
                    self.finish_tail(pending.delta);
                    pending.stage = Stage::Finalize;
                }
                Stage::Finalize => {
                    if let Err(error) = self
                        .view
                        .as_mut()
                        .unwrap()
                        .synchronize(&self.env.server, self.model.as_ref().unwrap())
                    {
                        self.pending = Some(pending);
                        return Err(error);
                    }
                    return Ok(());
                }
            }
        }
    }
    /// All art/font/mask GPU uploads belong to actual asset/glyph owners. This
    /// method never polls jobs, advances clocks, captures a source or redraws ADV.
    pub fn prepare(&self, _: &mut PreRenderContext) {}
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let atlas = self.loader.atlas();
        self.view.as_ref().unwrap().render(
            self.model(),
            atlas.as_deref(),
            &self.env.common.system,
            false,
            pass,
        )?;
        let mask = self.loader.mask();
        let mut result = Ok(());
        self.scope.draw_ref(|task| {
            if result.is_err() {
                return;
            }
            if let Task::First { overlay, .. } = &task.task {
                result = draw_overlay(
                    overlay,
                    self.loader.snapshot.as_ref(),
                    mask.as_deref(),
                    pass,
                );
            }
        });
        result
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Music input extent");
        if extent != self.extent {
            self.cancel_input();
            self.area = Some(InputArea::new(extent.0, extent.1));
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        self.area
            .as_mut()
            .unwrap()
            .disable(self.model.as_mut().unwrap());
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Retained native owner lifecycle inspection hook.")]
    pub fn closing(&self) -> bool {
        self.model().closing
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Retained native owner lifecycle inspection hook.")]
    pub fn has_save_task(&self) -> bool {
        false
    }
    /// A failed parent event remains owned here; the next poll resumes the same
    /// frozen callback before admitting any new delta/buttons/pointer frame.
    #[cfg(test)]
    pub fn pending_event(&self) -> Option<&Action> {
        self.scope
            .iter()
            .find_map(Owned::pending_event)
            .or_else(|| self.events.front())
    }
    #[cfg(test)]
    pub fn pending_frame(&self) -> bool {
        self.pending.is_some()
    }
    #[cfg(test)]
    pub fn retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.loader
            .job
            .iter()
            .map(|job| job.retirement.clone())
            .collect()
    }
    #[cfg(test)]
    pub fn new_with_reference_gate(
        env: Environment,
        scenario: &HigurashiScenario,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
        at: ReferencePublication,
        gate: ReferenceGate,
    ) -> Result<Self> {
        Self::construct(
            env,
            scenario,
            settings,
            capture,
            extent,
            services,
            Gate::Hold {
                at,
                callback: Some(gate),
            },
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_model(&self) -> &Model {
        self.model()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_state(&self) -> (bool, bool, bool, bool, bool, bool) {
        let published = self.loader.published.lock();
        (
            published.atlas.is_some(),
            published.mask.is_some(),
            self.loader.completed,
            self.scope
                .iter()
                .any(|task| matches!(task.task, Task::Input { done: false })),
            self.loader.snapshot.is_some(),
            self.pending.is_some(),
        )
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_resources(&self) -> ReferenceResources {
        let published = self.loader.published.lock();
        ReferenceResources {
            atlas: published
                .atlas
                .as_ref()
                .map(Arc::downgrade)
                .unwrap_or_default(),
            mask: published.mask.as_ref().map(Arc::downgrade),
        }
    }
}
