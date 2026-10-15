//! Unregistered concrete MovieMode/Viewer owner candidate. Native component
//! evidence is frozen separately; root owns registration and actual acceptance.
use super::{Action, Model, Task, Viewer, view::View};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            reconstruction_mode::{OverlayClock, overlay::DrawPacket},
        },
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    app::higurashi::{movie_player::Player, save_menu::Environment},
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Vec2, Vec4};
use parking_lot::Mutex;
use shin_audio::AudioManager;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::HigurashiScenario},
    primitives::{color::FloatColor4, update::FrameId},
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder, render_pass::RenderPass,
    render_texture::RenderTexture, shaders::types::buffer::VertexSource,
};
use std::{collections::BTreeMap, sync::Arc};
#[cfg(test)]
mod tests;

/// Only literal native global effects leave the concrete owner. No default
/// success, save projection, gallery acquisition or inferred media bus.
pub(in crate::app::higurashi) trait Services {
    fn event(&mut self, action: Action) -> Result<()>;
}
#[cfg(any(test, feature = "higurashi-window-probe"))]
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code, reason = "Native viewer snapshot shared by tests and optional window probes.")]
pub(in crate::app::higurashi) struct ReferenceViewerState {
    pub id: u64,
    pub movie_id: u16,
    pub opacity: f32,
    pub backdrop: f32,
    pub closing: bool,
    pub private_input_ready: bool,
    pub status: u32,
    pub playing: bool,
    pub retained_frame: Option<(u32, u32)>,
    pub time_micros: Option<u64>,
}
#[cfg(any(test, feature = "higurashi-window-probe"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "Native viewer snapshot shared by tests and optional window probes.")]
pub(in crate::app::higurashi) struct ReferenceScopeState {
    pub tasks: usize,
    pub borrowed_tasks: usize,
    pub viewers: usize,
    pub borrowed_viewers: usize,
    pub media_owners: usize,
    pub first_tasks: usize,
    pub capture_present: bool,
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
        //9534c retained the capture before27520. No task receives a live source
        //scene or an implicit redraw/capture closure.
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load(super::ATLAS_PATH).await?;
            View::validate_atlas(&atlas)?;
            #[cfg(test)]
            let atlas_weak = Arc::downgrade(&atlas);
            //965bc/9661c publishes BEFORE the next mask lookup and before status2.
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
            bail!("Movie worker failed:{fault}");
        }
        if let Some(result) = self.job.as_mut().and_then(Job::poll) {
            if let Err(error) = result {
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
            let output = self.published.lock();
            ensure!(
                output.atlas.is_some() && output.mask.is_some(),
                "Completed Movie worker has incomplete publications"
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
        //955e8 actual284f8 cancellation/join. Never lock a publication mutex
        //during the join: a running worker may still be publishing either owner.
        if let Some(job) = &mut self.job {
            job.retire();
        }
        //95618 capture,9561c..95644 mask,95648..95664 job. Atlas is PARENT-owned.
        self.snapshot.take();
        self.published.lock().mask.take();
        self.job.take();
    }
    fn release_parent_atlas(&mut self) {
        //Movie93c80..94 releases the current atomic BEFORE its private scope.
        //Keep the slot alive; do not suppress or fake a running publication.
        self.published.lock().atlas.take();
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.retire_first();
        self.release_parent_atlas();
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
        "Movie First lost its retained mask"
    );
    let dimensions = mask.map(|mask| {
        let s = mask.texture.size();
        (s.width, s.height)
    });
    let Some(packet) = clock.draw_packet(dimensions) else {
        return Ok(());
    };
    let snapshot = snapshot.ok_or_else(|| {
        anyhow::anyhow!("Movie First has visible capture without its source bitmap")
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

struct Media {
    player: Option<Player>,
    area: Option<InputArea>,
}
// Actual95ed0 Viewer backdrop projection. Keep the raw OpenGL matrix here;
// the checked Packet submission applies the single WGPU depth bridge.
fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        -1.,
        1.,
        -0.,
        1.,
    ])
}
struct Bridge<'a, 'p> {
    loader: &'a mut Loader,
    grid: &'a mut Option<InputArea>,
    media: &'a mut BTreeMap<u64, Media>,
    assets: &'a Arc<AssetServer>,
    audio: &'a Arc<AudioManager>,
    scenario: &'a HigurashiScenario,
    extent: (u16, u16),
    frame: FrameId,
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    parent: &'a mut dyn Services,
    status_override: Option<(u64, u32)>,
}
impl Bridge<'_, '_> {
    fn player(&mut self, id: u64) -> Result<&mut Player> {
        self.media
            .get_mut(&id)
            .and_then(|m| m.player.as_mut())
            .context("Movie private Player identity is missing")
    }
}
struct GridProxy<'a, 'b, 'p> {
    model: &'a mut Model,
    services: &'a mut Bridge<'b, 'p>,
    fault: Option<anyhow::Error>,
}
impl InputAreaTarget for GridProxy<'_, '_, '_> {
    fn probe(&mut self, p: Vec2) -> bool {
        self.fault.is_none() && self.model.probe(p)
    }
    fn reset(&mut self) {
        if self.fault.is_none() {
            self.model.reset();
        }
    }
    fn press(&mut self, _: Vec2) {
        if self.fault.is_none() {
            self.model.press();
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.fault.is_none() {
            if let Err(e) = self.model.release(self.services) {
                self.fault = Some(e);
            }
        }
    }
    fn long_press(&mut self, _: Vec2) {
        if self.fault.is_none() {
            self.model.long_press();
        }
    }
    fn drag(&mut self, a: Vec2, b: Vec2) {
        if self.fault.is_none() {
            self.model.drag(a, b);
        }
    }
    fn gesture(&mut self, v: Vec2) {
        if self.fault.is_none() {
            self.model.gesture(v);
        }
    }
    fn cancel(&mut self) {
        if self.fault.is_none() {
            self.model.cancel();
        }
    }
}
struct ViewerProxy<'a, 'b, 'p> {
    viewer: &'a mut Viewer,
    services: &'a mut Bridge<'b, 'p>,
    fault: Option<anyhow::Error>,
}
impl InputAreaTarget for ViewerProxy<'_, '_, '_> {
    fn probe(&mut self, _: Vec2) -> bool {
        self.fault.is_none()
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        if self.fault.is_none() {
            if let Err(e) = self.viewer.pointer_release(self.services) {
                self.fault = Some(e);
            }
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}
impl super::Services for Bridge<'_, '_> {
    fn first_status(&mut self) -> Result<u32> {
        self.loader.status()
    }
    fn mask_published(&self) -> bool {
        self.loader.published.lock().mask.is_some()
    }
    fn player_status(&mut self, id: u64) -> Result<u32> {
        if let Some((wanted, status)) = self.status_override.take() {
            ensure!(wanted == id, "Movie status snapshot owner mismatch");
            return Ok(status);
        }
        Ok(self.player(id)?.status())
    }
    fn input_grid(&mut self, model: &mut Model, permission: bool) -> Result<bool> {
        let mut area = self
            .grid
            .take()
            .context("Movie grid InputArea is already borrowed")?;
        let pointer = self.pointer;
        let buttons = self.buttons;
        let mut proxy = GridProxy {
            model,
            services: self,
            fault: None,
        };
        let captured = if permission {
            area.update(&mut proxy, pointer)
        } else {
            area.disable(&mut proxy);
            false
        };
        let fault = proxy.fault.take();
        drop(proxy);
        *self.grid = Some(area);
        if let Some(e) = fault {
            return Err(e);
        }
        model.controller(permission, captured, buttons, self)
    }
    fn input_viewer(&mut self, viewer: &mut Viewer, permission: bool) -> Result<bool> {
        //96034 queries status BEFORE permission/capture/Area callbacks. Cache
        //that one snapshot rather than racing a second preparation-state read.
        let status = self.player(viewer.id)?.status();
        let buttons = self.buttons;
        if status == 2 {
            self.status_override = Some((viewer.id, status));
            return viewer.controller(permission, false, buttons.pressed, self);
        }
        let mut area = self
            .media
            .get_mut(&viewer.id)
            .and_then(|m| m.area.take())
            .context("Movie Viewer InputArea already borrowed")?;
        let pointer = self.pointer;
        let mut proxy = ViewerProxy {
            viewer,
            services: self,
            fault: None,
        };
        let captured = if permission {
            area.update(&mut proxy, pointer)
        } else {
            area.disable(&mut proxy);
            false
        };
        let fault = proxy.fault.take();
        drop(proxy);
        self.media
            .get_mut(&viewer.id)
            .context("Movie Viewer disappeared in pointer callback")?
            .area = Some(area);
        if let Some(e) = fault {
            return Err(e);
        }
        self.status_override = Some((viewer.id, status));
        viewer.controller(permission, captured, buttons.pressed, self)
    }
    fn action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(_)
            | Action::Achievement(_)
            | Action::StopBgm(_)
            | Action::ReturnTitle { .. }
            | Action::PlatformByte => self.parent.event(action),
            //Actual disable ran before the controller service marker. Native
            //EOF status2 never enters that branch or clears pointer capture.
            Action::DisableInput(_) => Ok(()),
            Action::OpenPlayer {
                viewer,
                movie_id,
                source_category,
            } => {
                ensure!(
                    source_category == 1 && !self.media.contains_key(&viewer),
                    "Movie source/Viewer identity"
                );
                let info = self
                    .scenario
                    .movie_info
                    .get(usize::from(movie_id))
                    .context("Movie metadata is missing")?;
                let name = info.name.to_string();
                let player = Player::open(self.assets.clone(), self.audio.clone(), &name)?;
                self.media.insert(
                    viewer,
                    Media {
                        player: Some(player),
                        area: Some(InputArea::new(self.extent.0, self.extent.1)),
                    },
                );
                Ok(())
            }
            Action::StartPlayer(id) => {
                self.player(id)?.start();
                Ok(())
            }
            Action::PausePlayer(id) => {
                self.player(id)?.pause();
                Ok(())
            }
            Action::UpdatePlayer(id) => {
                let frame = self.frame;
                self.player(id)?.update(frame)
            }
            Action::StopPlayer(id) => {
                //Native+20 is full shutdown, distinct from +30 pause. The
                //reviewed adapter keeps facade/RGBA through later Release.
                self.player(id)?.shutdown();
                Ok(())
            }
            Action::ReleasePlayer(id) => {
                let mut player = self
                    .media
                    .get_mut(&id)
                    .and_then(|m| m.player.take())
                    .context("Movie release without retained Player")?;
                player.retire();
                drop(player);
                Ok(()) //InputArea survives private-task destruction below.
            }
            Action::PlayerGain { viewer, value } => self.player(viewer)?.set_gain(value),
            Action::PlayerColor { viewer, value } => {
                self.player(viewer)?.set_color(Vec4::from_array(value))
            }
            Action::JoinFirst => {
                self.loader.retire_first();
                Ok(())
            }
            Action::ReleaseAtlas => {
                self.loader.release_parent_atlas();
                Ok(())
            }
        }
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    bridge: Bridge<'a, 'p>,
    fault: Option<anyhow::Error>,
}
struct Owned {
    task: Task,
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        u32::from(self.task.complete())
    }
    fn retire(&mut self, f: &mut Frame<'_, '_>) {
        let viewer = if let Task::Viewer(v) = &self.task {
            Some(v.id)
        } else {
            None
        };
        let result: Result<()> = if let Task::Viewer(v) = &mut self.task {
            use super::Services as _;
            (|| {
                //95d48 joins Player work,95d50 destroys the backdrop queue,
                //95d58 opacity queue,95d74 releases Player,95d7c private scope.
                //The discarded zero-value shells have no animation/resource.
                f.bridge.action(Action::StopPlayer(v.id))?;
                drop(std::mem::replace(
                    &mut v.backdrop,
                    crate::layer::higurashi_properties::HigurashiTweener::new(0.),
                ));
                drop(std::mem::replace(
                    &mut v.opacity,
                    crate::layer::higurashi_properties::HigurashiTweener::new(0.),
                ));
                f.bridge.action(Action::ReleasePlayer(v.id))?;
                v.scope = ModeStack::default();
                Ok(())
            })()
        } else {
            self.task.retire_native(&mut f.bridge)
        };
        if let Err(e) = result {
            if f.fault.is_none() {
                f.fault = Some(e);
            }
        }
        //Original95d7c scope precedes InputArea destruction; dispose that final
        //area only after the successful native retirement prefix above.
        if let Some(id) = viewer {
            f.bridge.media.remove(&id);
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        f: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        use super::Services as _;
        if let Some(e) = f.fault.take() {
            return Err(e);
        }
        match &mut self.task {
            Task::First { phase, overlay } => {
                if *phase == 0 && f.bridge.first_status()? == 2 {
                    overlay.masked(30, f32::from_bits(0x3e99999a), 0, f.bridge.mask_published());
                    f.bridge.action(Action::StopBgm(60))?;
                    *phase = 1;
                } else if *phase == 1 && !overlay.busy() {
                    f.model.closing = false;
                    pushes.push(Owned {
                        task: Task::Input { done: false },
                    });
                    *phase = -1;
                }
                overlay.advance(delta);
            }
            Task::Input { done } => *done = !f.bridge.input_grid(f.model, permission)?,
            Task::Viewer(v) => v.advance(delta, permission, &mut f.bridge)?,
            Task::Close { done } => {
                if f.model.close_flash.is_idle() {
                    f.bridge.action(Action::StopBgm(30))?;
                    f.bridge.action(Action::ReturnTitle {
                        selected: 17,
                        mode: -1,
                    })?;
                    *done = true;
                }
            }
        }
        for task in f.model.take_pending_tasks() {
            pushes.push(Owned { task });
        }
        Ok(0)
    }
}
struct Initial<'a>(&'a mut dyn Services);
impl super::Services for Initial<'_> {
    fn first_status(&mut self) -> Result<u32> {
        bail!("First status queried before submission")
    }
    fn mask_published(&self) -> bool {
        false
    }
    fn input_grid(&mut self, _: &mut Model, _: bool) -> Result<bool> {
        bail!("Input before First")
    }
    fn input_viewer(&mut self, _: &mut Viewer, _: bool) -> Result<bool> {
        bail!("Viewer before First")
    }
    fn player_status(&mut self, _: u64) -> Result<u32> {
        bail!("Player before OpenPlayer")
    }
    fn action(&mut self, a: Action) -> Result<()> {
        ensure!(
            matches!(a, Action::Achievement(_)),
            "Unexpected Movie construction action"
        );
        self.0.event(a)
    }
}
struct NoExternal;
impl Services for NoExternal {
    fn event(&mut self, _: Action) -> Result<()> {
        bail!("External Movie service requested during final resource retirement")
    }
}
struct DrawServices<'a> {
    id: u64,
    player: &'a Player,
}
impl super::Services for DrawServices<'_> {
    fn first_status(&mut self) -> Result<u32> {
        bail!("First status queried during render")
    }
    fn mask_published(&self) -> bool {
        false
    }
    fn input_grid(&mut self, _: &mut Model, _: bool) -> Result<bool> {
        bail!("Input during render")
    }
    fn input_viewer(&mut self, _: &mut Viewer, _: bool) -> Result<bool> {
        bail!("Viewer input during render")
    }
    fn player_status(&mut self, _: u64) -> Result<u32> {
        bail!("Player status queried during render")
    }
    fn action(&mut self, a: Action) -> Result<()> {
        match a {
            Action::PlayerGain { viewer, value } => {
                ensure!(viewer == self.id, "Movie render gain identity");
                self.player.set_gain(value)
            }
            Action::PlayerColor { viewer, value } => {
                ensure!(viewer == self.id, "Movie render color identity");
                self.player.set_color(Vec4::from_array(value))
            }
            _ => bail!("Unexpected Movie render service action"),
        }
    }
}

pub(in crate::app::higurashi) struct Owner {
    model: Model,
    view: Option<View>,
    scope: ModeStack<Owned>,
    loader: Loader,
    area: Option<InputArea>,
    media: BTreeMap<u64, Media>,
    extent: (u16, u16),
    fault: Option<String>,
    env: Environment,
    audio: Arc<AudioManager>,
    scenario: Arc<HigurashiScenario>,
}
impl Owner {
    pub fn new_retained(
        env: Environment,
        audio: Arc<AudioManager>,
        scenario: Arc<HigurashiScenario>,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
    ) -> Result<Self> {
        Self::construct(
            env,
            audio,
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
        audio: Arc<AudioManager>,
        scenario: Arc<HigurashiScenario>,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
        gate: Gate,
    ) -> Result<Self> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Movie input extent is empty");
        let mut model = Model::new(
            &scenario.movie_info,
            &settings.save_vectors.unlocked_movies,
            env.server.contains("/append.txt"),
        )?;
        ensure!(
            !model.rows.is_empty(),
            "Movie concrete owner has no selectable rows"
        );
        super::view::completion_layout(env.common.font.clone(), &model.completion)?;
        model.publish_initial(&mut Initial(services))?;
        let mut view = View::new(env.common.font.clone());
        view.synchronize(&env.server, &model.view_state(false))?;
        let mut scope = ModeStack::default();
        scope.push(Owned {
            task: Task::first(capture.is_some()),
        });
        let loader = Loader::new(env.server.clone(), capture.take(), gate);
        Ok(Self {
            model,
            view: Some(view),
            scope,
            loader,
            area: Some(InputArea::new(extent.0, extent.1)),
            media: BTreeMap::new(),
            extent,
            fault: None,
            env,
            audio,
            scenario,
        })
    }
    ///One admitted MovieMode callback. FrameId comes from the parent permitted
    ///visual tick; no game-delta media clock or render-driven update is invented.
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        frame: FrameId,
        services: &mut dyn Services,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Movie delta is invalid"
        );
        if let Some(fault) = &self.fault {
            bail!("Movie owner failed:{fault}");
        }
        let result: Result<()> = (|| {
            let mut f = Frame {
                model: &mut self.model,
                bridge: Bridge {
                    loader: &mut self.loader,
                    grid: &mut self.area,
                    media: &mut self.media,
                    assets: &self.env.server,
                    audio: &self.audio,
                    scenario: &self.scenario,
                    extent: self.extent,
                    frame,
                    buttons,
                    pointer,
                    parent: services,
                    status_override: None,
                },
                fault: None,
            };
            self.scope.poll(delta, u32::from(permission), &mut f)?;
            if let Some(e) = f.fault {
                return Err(e);
            }
            let atlas = f.bridge.loader.atlas().is_some();
            f.model.advance_clocks(delta, atlas);
            self.view
                .as_mut()
                .unwrap()
                .synchronize(&self.env.server, &self.model.view_state(atlas))?;
            Ok(())
        })();
        if let Err(e) = &result {
            self.fault = Some(format!("{e:#}"));
        }
        result
    }
    pub fn prepare(&self, _: &mut PreRenderContext) {}
    pub fn render(&self, settings: &SaveData, pass: &mut RenderPass) -> Result<()> {
        let atlas = self.loader.atlas();
        let state = self.model.view_state(atlas.is_some());
        self.view.as_ref().unwrap().render(
            &state,
            atlas.as_deref(),
            &self.env.common.system,
            false,
            pass,
        )?;
        let mask = self.loader.mask();
        let mut result = Ok(());
        self.scope.draw_ref(|owned| {
            if result.is_err() {
                return;
            }
            result = match &owned.task {
                Task::First { overlay, .. } => draw_overlay(
                    overlay,
                    self.loader.snapshot.as_ref(),
                    mask.as_deref(),
                    pass,
                ),
                Task::Viewer(v) => (|| {
                    let player = self
                        .media
                        .get(&v.id)
                        .and_then(|m| m.player.as_ref())
                        .context("Movie borrowed Viewer has no Player")?;
                    //Actual95e90 always draws backdrop, then95f84 gates the
                    //ENTIRE gain/color/standalone sequence, including alpha0.
                    let alpha = v.backdrop.value();
                    let a = ((alpha.max(0.) * 255.) as u32).min(255) << 24;
                    crate::app::higurashi::kakera_menu::view::Packet {
                        texture: String::new(),
                        matrix: projection(),
                        rect: [0., 0., 1920., 1080.],
                        uv: [0., 0., 1., 1.],
                        color: a,
                        opaque: alpha >= 1.,
                        flash: 0.,
                    }
                    .render(pass, None);
                    if player.is_playing() {
                        v.prepare_player_draw(
                            true,
                            settings.settings.common.v0_bgmvol,
                            &mut DrawServices { id: v.id, player },
                        )?;
                        player.draw(pass)?;
                    }
                    Ok(())
                })(),
                _ => Ok(()),
            };
        });
        result
    }
    pub fn sync_suspension(&self) {
        for m in self.media.values() {
            if let Some(p) = &m.player {
                p.sync_suspension();
            }
        }
    }
    pub fn cancel_input(&mut self) {
        struct Reset<'a>(&'a mut Model);
        impl InputAreaTarget for Reset<'_> {
            fn probe(&mut self, _: Vec2) -> bool {
                false
            }
            fn reset(&mut self) {
                self.0.reset();
            }
            fn cancel(&mut self) {
                self.0.cancel();
            }
            fn press(&mut self, _: Vec2) {}
            fn release(&mut self, _: Vec2) {}
            fn long_press(&mut self, _: Vec2) {}
            fn drag(&mut self, _: Vec2, _: Vec2) {}
            fn gesture(&mut self, _: Vec2) {}
        }
        if let Some(a) = &mut self.area {
            a.disable(&mut Reset(&mut self.model));
        }
        struct ResetViewer;
        impl InputAreaTarget for ResetViewer {
            fn probe(&mut self, _: Vec2) -> bool {
                true
            }
            fn reset(&mut self) {}
            fn cancel(&mut self) {}
            fn press(&mut self, _: Vec2) {}
            fn release(&mut self, _: Vec2) {}
            fn long_press(&mut self, _: Vec2) {}
            fn drag(&mut self, _: Vec2, _: Vec2) {}
            fn gesture(&mut self, _: Vec2) {}
        }
        for m in self.media.values_mut() {
            if let Some(a) = &mut m.area {
                a.disable(&mut ResetViewer);
            }
        }
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Movie resize extent is empty");
        if extent != self.extent {
            self.cancel_input();
            self.area = Some(InputArea::new(extent.0, extent.1));
            for m in self.media.values_mut() {
                m.area = Some(InputArea::new(extent.0, extent.1));
            }
            self.extent = extent;
        }
        Ok(())
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Retained native owner lifecycle inspection hook.")]
    pub fn closing(&self) -> bool {
        self.model.closing
    }
    #[cfg(test)]
    #[allow(dead_code, reason = "Retained native owner lifecycle inspection hook.")]
    pub fn has_save_task(&self) -> bool {
        false
    }
    #[cfg(test)]
    pub fn retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.loader
            .job
            .iter()
            .map(|j| j.retirement.clone())
            .collect()
    }
    #[cfg(test)]
    fn new_with_reference_gate(
        env: Environment,
        audio: Arc<AudioManager>,
        scenario: Arc<HigurashiScenario>,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
        services: &mut dyn Services,
        at: ReferencePublication,
        gate: ReferenceGate,
    ) -> Result<Self> {
        Self::construct(
            env,
            audio,
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
        &self.model
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_state(&self) -> (bool, bool, bool, bool, bool, usize) {
        let published = self.loader.published.lock();
        (
            published.atlas.is_some(),
            published.mask.is_some(),
            self.loader.completed,
            self.scope
                .iter()
                .any(|t| matches!(t.task, Task::Input { done: false })),
            self.loader.snapshot.is_some(),
            self.media.len(),
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_viewer_state(&self) -> Option<ReferenceViewerState> {
        self.scope
            .iter()
            .filter_map(|owned| {
                let Task::Viewer(viewer) = &owned.task else {
                    return None;
                };
                let player = self
                    .media
                    .get(&viewer.id)
                    .and_then(|media| media.player.as_ref())
                    .expect("A retained Movie Viewer requires its actual Player");
                let mut private_input_ready = false;
                viewer.scope.draw_ref(|task| {
                    private_input_ready |= matches!(task, super::ViewerTask::Input { done: false });
                });
                Some(ReferenceViewerState {
                    id: viewer.id,
                    movie_id: viewer.movie_id,
                    opacity: viewer.opacity.value(),
                    backdrop: viewer.backdrop.value(),
                    closing: viewer.closing,
                    private_input_ready: !viewer.closing && private_input_ready,
                    status: player.status(),
                    playing: player.is_playing(),
                    retained_frame: player.reference_frame_extent(),
                    time_micros: player.reference_time_micros(),
                })
            })
            .last()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_scope_state(&self) -> ReferenceScopeState {
        let mut state = ReferenceScopeState {
            tasks: self.scope.len(),
            borrowed_tasks: 0,
            viewers: 0,
            borrowed_viewers: 0,
            media_owners: self.media.len(),
            first_tasks: 0,
            capture_present: self.loader.snapshot.is_some(),
        };
        for owned in self.scope.iter() {
            state.viewers += usize::from(matches!(owned.task, Task::Viewer(_)));
            state.first_tasks += usize::from(matches!(owned.task, Task::First { .. }));
        }
        self.scope.draw_ref(|owned| {
            state.borrowed_tasks += 1;
            state.borrowed_viewers += usize::from(matches!(owned.task, Task::Viewer(_)));
        });
        state
    }
    #[cfg(test)]
    fn reference_resources(&self) -> ReferenceResources {
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
impl Drop for Owner {
    fn drop(&mut self) {
        self.view.take();
        self.model.rows.clear();
        self.loader.release_parent_atlas();
        let pointer = PointerFrame {
            number: 0,
            time_seconds: 0.,
            events: &[],
        };
        let mut external = NoExternal;
        let mut f = Frame {
            model: &mut self.model,
            bridge: Bridge {
                loader: &mut self.loader,
                grid: &mut self.area,
                media: &mut self.media,
                assets: &self.env.server,
                audio: &self.audio,
                scenario: &self.scenario,
                extent: self.extent,
                frame: FrameId::default(),
                buttons: Buttons::default(),
                pointer: &pointer,
                parent: &mut external,
                status_override: None,
            },
            fault: None,
        };
        self.scope.clear_with(&mut f);
        drop(f);
        //Fault/construction fallback always drains the actual job before its
        //capture/mask/job, then any atlas publication completed during join.
        self.loader.retire_first();
        self.loader.release_parent_atlas();
        for (_, mut m) in std::mem::take(&mut self.media) {
            if let Some(mut p) = m.player.take() {
                p.shutdown();
                p.retire();
            }
            m.area.take();
        }
        self.area.take();
    }
}
