//! Concrete3f3d0 Viewer. Unregistered until the parent capture/input adapter is
//! supplied. Worker ownership and art are real; the frozen model owns clocks.
use super::{Cue, Input, Metadata, Model, Picture, Services, Task, view::View};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::mode_stack::{Mode, ModePush, ModeStack},
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    asset::{
        picture::Picture as PictureAsset,
        system::{Asset, AssetDataAccessor, AssetLoadContext, AssetServer},
    },
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
        user::higurashi::HigurashiUserLayer,
    },
};
use anyhow::{Context, Result, bail, ensure};
use glam::Vec2;
use shin_core::{format::scenario::higurashi::HigurashiScenario, time::Ticks};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

/// Must sample four OS-owner seeds, never the VM clock or global MT. Called on
/// the scoped loader after AssetServer opened the file, before its first read.
#[derive(Clone)]
pub(in crate::app::higurashi) struct Seeds(
    Arc<parking_lot::Mutex<Box<dyn FnMut() -> [u32; 4] + Send>>>,
);
impl Seeds {
    pub fn new(sample: impl FnMut() -> [u32; 4] + Send + 'static) -> Self {
        Self(Arc::new(parking_lot::Mutex::new(Box::new(sample))))
    }
    fn sample(&self) -> [u32; 4] {
        (self.0.lock())()
    }
}
impl fmt::Debug for Seeds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CG Viewer OS seed service")
    }
}
#[derive(Clone, Debug)]
struct LoadArgs {
    identity: u64,
    seeds: Seeds,
}
// Asset's Args contract requires Eq/Hash. This owner uses the explicit
// uncached entry point: only Picture's CRC block cache is shared, never a base.
impl PartialEq for LoadArgs {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}
impl Eq for LoadArgs {}
impl Hash for LoadArgs {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}

struct PreparedPicture {
    identity: u64,
    art: Arc<PictureAsset>,
    properties: HigurashiProperties,
}
impl Asset for PreparedPicture {
    type Args = LoadArgs;
    async fn load(
        context: &Arc<AssetLoadContext>,
        args: LoadArgs,
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        ensure!(
            context.game == crate::cli::Game::Higurashi,
            "CG Viewer requires Higurashi picture decoding"
        );
        // ROM accessors already retain their member. Directory accessors defer
        // the OS open until cursor(); acquire one now before sampling. The
        // shared Picture decoder obtains its own cursor later, an explicit
        // extra directory-open transport boundary (no duplicate decoding).
        let opened = data
            .cursor()
            .context("Opening CG Viewer picture before base seeds")?;
        let properties = HigurashiProperties::from_seeds(args.seeds.sample());
        let art = Arc::new(PictureAsset::load(context, (), name, data).await?);
        drop(opened);
        Ok(Self {
            identity: args.identity,
            art,
            properties,
        })
    }
}
struct Job {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Job {
    fn start(server: Arc<AssetServer>, path: String, seeds: Seeds, slot: PreparedSlot) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(identity, 0, "CG Viewer picture identity exhausted");
        let task = shin_tasks::async_io::spawn_loader(async move {
            let result = async {
                server
                    .load_uncached_with_args::<PreparedPicture, _>(
                        path,
                        LoadArgs { identity, seeds },
                    )
                    .await
            }
            .await;
            //41c80 publishes e0 before executor status becomes2. Keeping this
            // outside the task result preserves destructor order even when a
            // finished worker has not yet been observed by the main poll.
            *slot.lock() = Some(result);
            Ok(())
        });
        Self {
            retirement: task.retirement_handle().unwrap(),
            task: Some(task),
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct Art {
    identity: u64,
    picture: Arc<PictureAsset>,
}
type PreparedSlot = Arc<parking_lot::Mutex<Option<Result<PreparedPicture>>>>;
struct Backend {
    job: Option<Job>,
    prepared: PreparedSlot,
    live: Option<Art>,
    snapshot: Option<RenderTexture>,
    staged_snapshot: Option<RenderTexture>,
    fault: Option<String>,
}

/// Exact main-poll capture input. In particular show_info is already false,
/// and the old live picture/capture are still owned here. A platform cache may
/// be copied only when it represents this composition. No clocks may advance.
pub(in crate::app::higurashi) struct Composition<'a> {
    model: &'a Model,
    atlas: &'a CharacterTextures,
    live: Option<&'a Art>,
    snapshot: Option<&'a RenderTexture>,
}
impl Composition<'_> {
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        View::new().render(
            self.model,
            self.atlas,
            self.snapshot.map(RenderTexture::as_texture_source),
            pass,
            |picture, kind, stencil, pass| {
                let live = self
                    .live
                    .ok_or_else(|| anyhow::anyhow!("CG Viewer has model Picture without art"))?;
                ensure!(
                    live.identity == picture.identity,
                    "CG Viewer art/properties identity mismatch"
                );
                // Parent-owned narrow shared helper; no new drawable/base/seeds or
                // cloned advancing properties owner is constructed by rendering.
                HigurashiUserLayer::render_picture_with_properties(
                    &live.picture,
                    &picture.properties,
                    pass,
                    &HigurashiTransform::default(),
                    stencil,
                    kind,
                )
            },
        )
    }
    pub fn badges_hidden(&self) -> bool {
        !self.model.show_info
    }
    #[cfg(test)]
    pub fn old_picture_identity(&self) -> Option<u64> {
        self.model.picture.as_ref().map(|p| p.identity)
    }
}
pub(in crate::app::higurashi::cg_gallery) trait Capture {
    fn capture_parent(&mut self, viewer: &Composition<'_>) -> Result<RenderTexture>;
    fn cue(&mut self, cue: Cue) -> Result<()>;
}

struct Core<'a> {
    backend: &'a mut Backend,
    server: &'a Arc<AssetServer>,
    scenario: &'a HigurashiScenario,
    seeds: &'a Seeds,
    atlas: &'a CharacterTextures,
    capture: Option<&'a mut dyn Capture>,
}
impl Services for Core<'_> {
    fn prepare_picture(&mut self, id: u16) -> Result<()> {
        let info = self
            .scenario
            .picture_info
            .get(id as usize)
            .ok_or_else(|| anyhow::anyhow!("CG Viewer picture metadata ID{id} is missing"))?;
        // Nativebc8a4(kind0): replace '?' by pc, retain actual case in native
        // evidence; mounted ROM lookup is case-insensitive/lowercase here.
        let name = crate::layer::user::higurashi::selected_asset_name(info.name.as_str(), 0);
        let path = format!("/picture/{name}.pic");
        //3f800 replaces the job only after its copied filename is complete.
        self.backend.job.take();
        self.backend.prepared.lock().take();
        self.backend.job = Some(Job::start(
            self.server.clone(),
            path,
            self.seeds.clone(),
            self.backend.prepared.clone(),
        ));
        Ok(())
    }
    fn worker_status(&mut self) -> u32 {
        // Error propagation uses take_prepared's Result, not a fabricated
        // native success or EOF. A failed worker remains status2 until read.
        if let Some(job) = &mut self.backend.job {
            if let Some(result) = job.task.as_mut().and_then(|task| task.poll_naive()) {
                job.task = None;
                match result {
                    Ok(()) => {}
                    Err(error) => self.backend_error(error),
                }
            }
        }
        if self
            .backend
            .job
            .as_ref()
            .is_some_and(|job| job.task.is_none())
            || self.failed()
        {
            2
        } else {
            1
        }
    }
    fn take_prepared(&mut self) -> Result<Picture> {
        self.check_error()?;
        let prepared =
            self.backend.prepared.lock().take().ok_or_else(|| {
                anyhow::anyhow!("CG Viewer ready worker has no prepared Picture")
            })??;
        let metadata = Metadata {
            origin: [prepared.art.origin_x as i16, prepared.art.origin_y as i16],
            extent: [
                prepared.art.effective_width.try_into()?,
                prepared.art.effective_height.try_into()?,
            ],
        };
        let picture = Picture {
            identity: prepared.identity,
            metadata,
            properties: prepared.properties,
        };
        self.backend.live = Some(Art {
            identity: prepared.identity,
            picture: prepared.art,
        });
        Ok(picture)
    }
    fn capture_parent(&mut self, model: &Model) -> Result<()> {
        self.check_error()?;
        ensure!(
            self.backend.staged_snapshot.is_none(),
            "CG Viewer capture already staged"
        );
        let composition = Composition {
            model,
            atlas: self.atlas,
            live: self.backend.live.as_ref(),
            snapshot: self.backend.snapshot.as_ref(),
        };
        ensure!(
            composition.badges_hidden(),
            "CG Viewer capture requires temporarily hidden badges"
        );
        let snapshot = self
            .capture
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CG Viewer capture service missing"))?
            .capture_parent(&composition)?;
        self.backend.staged_snapshot = Some(snapshot);
        Ok(())
    }
    fn publish_capture(&mut self, model: &Model) -> Result<()> {
        let snapshot = self
            .backend
            .staged_snapshot
            .take()
            .ok_or_else(|| anyhow::anyhow!("CG Viewer has no staged capture"))?;
        if model.closing {
            self.backend.live.take();
        }
        // Native41ffc/40738 installs only after old live retirement. Ready has
        // already applied fit/bounds, while close has cleared its model Picture.
        self.backend.snapshot = Some(snapshot);
        Ok(())
    }
    fn input(&mut self, _: &mut Model, _: bool) -> Result<()> {
        bail!("CG Viewer nested input service")
    }
    fn cue(&mut self, cue: Cue) -> Result<()> {
        self.capture
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CG Viewer cue service missing"))?
            .cue(cue)
    }
    fn disable_input(&mut self) {} // The real InputArea is disabled with its target below.
}
impl Core<'_> {
    fn backend_error(&mut self, error: anyhow::Error) {
        self.backend.fault = Some(format!("{error:#}"));
    }
    fn failed(&self) -> bool {
        self.backend.fault.is_some()
    }
    fn check_error(&self) -> Result<()> {
        if let Some(error) = &self.backend.fault {
            bail!("CG Viewer picture worker failed:{error}");
        }
        if let Some(Err(error)) = self.backend.prepared.lock().as_ref() {
            bail!("CG Viewer picture worker failed:{error:#}");
        }
        Ok(())
    }
}
struct Target<'a> {
    model: &'a mut Model,
    core: &'a mut dyn Services,
    fault: Option<anyhow::Error>,
}
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        self.model.probe_contact(p)
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        if self.fault.is_none() {
            self.fault = self.model.release_contact(self.core).err();
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, a: Vec2, b: Vec2) {
        self.model.drag_contact(a, b);
    }
    fn gesture(&mut self, v: Vec2) {
        self.model.gesture_contact(v);
    }
    fn cancel(&mut self) {}
}
fn dispatch_pointer(
    area: &mut InputArea,
    model: &mut Model,
    permission: bool,
    pointer: &PointerFrame<'_>,
    services: &mut dyn Services,
) -> Result<bool> {
    let mut target = Target {
        model,
        core: services,
        fault: None,
    };
    let captured = if permission {
        area.update(&mut target, pointer)
    } else {
        area.disable(&mut target);
        false
    };
    if let Some(error) = target.fault {
        return Err(error);
    }
    Ok(captured)
}
struct Frame<'a, 'p> {
    core: Core<'a>,
    area: &'a mut InputArea,
    input: Input,
    pointer: &'a PointerFrame<'p>,
    model: &'a mut Model,
}
struct InputServices<'a, 'b, 'p> {
    core: &'a mut Core<'b>,
    area: &'a mut InputArea,
    input: Input,
    pointer: &'a PointerFrame<'p>,
}
impl Services for InputServices<'_, '_, '_> {
    fn prepare_picture(&mut self, id: u16) -> Result<()> {
        self.core.prepare_picture(id)
    }
    fn worker_status(&mut self) -> u32 {
        self.core.worker_status()
    }
    fn take_prepared(&mut self) -> Result<Picture> {
        self.core.take_prepared()
    }
    fn capture_parent(&mut self, m: &Model) -> Result<()> {
        self.core.capture_parent(m)
    }
    fn publish_capture(&mut self, m: &Model) -> Result<()> {
        self.core.publish_capture(m)
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let captured = dispatch_pointer(self.area, model, permission, self.pointer, self.core)?;
        model.controls(permission, captured, self.input, self.core)?;
        Ok(())
    }
    fn cue(&mut self, cue: Cue) -> Result<()> {
        self.core.cue(cue)
    }
    fn disable_input(&mut self) {}
}
impl Mode<Frame<'_, '_>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.done)
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        let mut services = InputServices {
            core: &mut frame.core,
            area: frame.area,
            input: frame.input,
            pointer: frame.pointer,
        };
        for task in self.poll_native(frame.model, delta, permission, &mut services)? {
            pushes.push(task);
        }
        Ok(0)
    }
}

pub(in crate::app::higurashi) struct Owner {
    // Native3fd10 joins its worker BEFORE live art, capture, prepared, scope,
    // InputArea. Explicit Drop below enforces this, independent of field order.
    job: Option<Backend>,
    model: Option<Model>,
    scope: ModeStack<Task>,
    area: InputArea,
    server: Arc<AssetServer>,
    scenario: Arc<HigurashiScenario>,
    seeds: Seeds,
    atlas: Weak<CharacterTextures>,
    extent: (u16, u16),
    fault: Option<String>,
}
impl Owner {
    pub fn new(
        entry: super::super::Entry,
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        seeds: Seeds,
        atlas: Arc<CharacterTextures>,
        extent: (u16, u16),
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "CG Viewer requires a nonzero pointer extent"
        );
        let mut backend = Backend {
            job: None,
            prepared: Arc::new(parking_lot::Mutex::new(None)),
            live: None,
            snapshot: None,
            staged_snapshot: None,
            fault: None,
        };
        let mut model = Model::new(
            entry,
            &mut Core {
                backend: &mut backend,
                server: &server,
                scenario: &scenario,
                seeds: &seeds,
                atlas: &atlas,
                capture: None,
            },
        )?;
        let mut scope = ModeStack::default();
        for task in model.take_pending_tasks() {
            scope.push(task);
        }
        Ok(Self {
            job: Some(backend),
            model: Some(model),
            scope,
            area: InputArea::new(extent.0, extent.1),
            server,
            scenario,
            seeds,
            atlas: Arc::downgrade(&atlas),
            extent,
            fault: None,
        })
    }
    pub fn complete(&self) -> bool {
        self.scope.is_empty()
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Invalid CG Viewer pointer extent"
        );
        if self.extent != extent {
            self.cancel_input();
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        // All native reset/cancel target slots are RET; disabling still retires
        // the real InputArea's captured contact and inherited release.
        struct Cancel;
        impl InputAreaTarget for Cancel {
            fn probe(&mut self, _: Vec2) -> bool {
                true
            }
            fn reset(&mut self) {}
            fn press(&mut self, _: Vec2) {}
            fn release(&mut self, _: Vec2) {}
            fn long_press(&mut self, _: Vec2) {}
            fn drag(&mut self, _: Vec2, _: Vec2) {}
            fn gesture(&mut self, _: Vec2) {}
            fn cancel(&mut self) {}
        }
        self.area.disable(&mut Cancel);
    }
    pub(in crate::app::higurashi::cg_gallery) fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        input: Input,
        pointer: &PointerFrame<'_>,
        capture: &mut dyn Capture,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid CG Viewer delta"
        );
        if let Some(fault) = &self.fault {
            bail!("CG Viewer failed:{fault}");
        }
        let atlas = self
            .atlas
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("CG Viewer parent atlas retired before poll"))?;
        let model = self.model.as_mut().unwrap();
        let backend = self.job.as_mut().unwrap();
        let result = self.scope.poll(
            delta,
            u32::from(permission),
            &mut Frame {
                core: Core {
                    backend,
                    server: &self.server,
                    scenario: &self.scenario,
                    seeds: &self.seeds,
                    atlas: &atlas,
                    capture: Some(capture),
                },
                area: &mut self.area,
                input,
                pointer,
                model,
            },
        );
        if let Err(error) = result {
            self.fault = Some(format!("{error:#}"));
            return Err(error);
        }
        model.advance_clocks(delta);
        Ok(())
    }
    pub fn composition<'a>(&'a self, atlas: &'a CharacterTextures) -> Composition<'a> {
        let backend = self.job.as_ref().unwrap();
        Composition {
            model: self.model.as_ref().unwrap(),
            atlas,
            live: backend.live.as_ref(),
            snapshot: backend.snapshot.as_ref(),
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let atlas = self
            .atlas
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("CG Viewer parent atlas retired before render"))?;
        self.composition(&atlas).render(pass)
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_state(&self) -> (i32, bool, bool, bool, Option<u64>, bool, usize) {
        let model = self.model.as_ref().unwrap();
        (
            model.variant,
            model.closing,
            model.show_info,
            model.fit,
            model.picture.as_ref().map(|p| p.identity),
            self.job.as_ref().unwrap().prepared.lock().is_some(),
            self.scope.len(),
        )
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub(in crate::app::higurashi::cg_gallery) fn reference_model(&self) -> &Model {
        self.model.as_ref().unwrap()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_input_ready(&self) -> bool {
        let model = self.model.as_ref().unwrap();
        model.picture.is_some()
            && !model.closing
            && !model.capture.busy()
            && self.scope.len() == 1
            && self
                .scope
                .iter()
                .any(|t| t.kind == super::Kind::Input && !t.done)
    }
    #[cfg(feature = "higurashi-window-probe")]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_window_state(
        &self,
    ) -> crate::app::higurashi::window_probe::cg_gallery::ViewerState {
        use crate::app::higurashi::window_probe::cg_gallery::ViewerState;
        let model = self.model.as_ref().unwrap();
        ViewerState {
            variant: model.variant,
            closing: model.closing,
            show_info: model.show_info,
            fit: model.fit,
            input_ready: self.reference_input_ready(),
            capture_busy: model.capture.busy(),
            capture_time: [
                model.capture.elapsed.to_bits(),
                model.capture.duration.to_bits(),
            ],
            picture_identity: model.picture.as_ref().map(|p| p.identity),
            metadata: model
                .picture
                .as_ref()
                .map(|p| (p.metadata.origin, p.metadata.extent)),
            offset: [model.offset.x.to_bits(), model.offset.y.to_bits()],
            properties: model.picture.as_ref().map(|p| {
                [0, 1, 12, 13].map(|i| p.properties.tweener(i).unwrap().value().to_bits())
            }),
            properties_busy: model.picture.as_ref().is_some_and(|p| {
                [0, 1, 12, 13]
                    .iter()
                    .any(|i| !p.properties.tweener(*i).unwrap().is_idle())
            }),
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(mut backend) = self.job.take() {
            // Drains nested block decode/upload work on this caller before any
            // live/captured/prepared resource or callback state is destroyed.
            if let Some(job) = &mut backend.job {
                if let Some(task) = job.task.take() {
                    task.cancel_and_retire();
                }
                job.retirement.clone().retire(());
            }
            backend.live.take();
            self.model.as_mut().unwrap().picture.take();
            backend.snapshot.take();
            backend.staged_snapshot.take();
            backend.prepared.lock().take();
            backend.job.take();
        }
        // Task has no retire callback; ModeStack's Drop is native reverse order.
        drop(std::mem::take(&mut self.scope));
        self.model.take();
    }
}

#[cfg(test)]
mod hardware_tests;
#[cfg(test)]
mod tests;
