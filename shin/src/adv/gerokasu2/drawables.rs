//! Real basic assets and retained front/back bank owners for LAYERLOAD services.
use super::{
    layer_control::{LayerControlServices, PropertyTarget},
    layer_edit::{LayerEdit, LayerEditServices},
    layer_factory::{BasicDrawable, LayerFactoryRequest, PreparedBasicDrawable},
    layer_load::{LoadCounters, LoadPlan, LoadServices, LoadTarget, LoadTransition},
    layers::Gerokasu2Layers,
    message_owner::SharedMessageOwner,
    root::Gerokasu2Root,
    scene::SceneServices,
};
use crate::{
    asset::system::AssetServer,
    layer::{
        DrawableLayer, Layer, LayerGroup, LayerProperties, ScreenLayer,
        owner::{LayerRead, LayerWrite},
        render_params::TransformParams,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::{AnyWiper, Wiper as _, gerokasu2::TransitionRequest},
};
use anyhow::{Context, Result, bail, ensure};
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario,
    time::Ticks,
    vm::{
        command::types::{LayerbankId, PLANES_COUNT, PlaneId},
        gerokasu2::Gerokasu2RuntimeCommand,
    },
};
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

// BEGIN NATIVE BACK ROOT MODEL
/// ADV+D0 retains the first pre-mutation root until an animation-enabled
/// command poll blocks. Completed commands and host frame boundaries keep it.
pub(crate) struct PreviousRoot<T> {
    value: Option<T>,
}
impl<T> Default for PreviousRoot<T> {
    fn default() -> Self {
        Self { value: None }
    }
}
impl<T> PreviousRoot<T> {
    pub fn is_none(&self) -> bool {
        self.value.is_none()
    }
    pub fn capture(&mut self, current: &T, clone: impl FnOnce(&T) -> T) -> bool {
        if self.value.is_some() {
            return false;
        }
        self.value = Some(clone(current));
        true
    }
    pub fn command_blocked(&mut self, animations_allowed: bool) -> bool {
        if animations_allowed {
            self.value.take().is_some()
        } else {
            false
        }
    }
    pub fn selected<'a>(&'a self, current: &'a T) -> &'a T {
        self.value.as_ref().unwrap_or(current)
    }
    pub fn selected_mut<'a>(&'a mut self, current: &'a mut T) -> &'a mut T {
        self.value.as_mut().unwrap_or(current)
    }
    pub fn update_pair(&mut self, current: &mut T, mut update: impl FnMut(&mut T)) {
        update(current);
        if let Some(previous) = self.value.as_mut() {
            update(previous);
        }
    }
}
// END NATIVE BACK ROOT MODEL

/// Shared root/Screen/Page/bank owners and resource/transition services. The
/// runner attaches its retained message;pending jobs preserve existing owners.
pub struct BasicLoadBackend {
    assets: Arc<AssetServer>,
    movie_audio: Option<Arc<shin_audio::AudioManager>>,
    movie_volume_settings: crate::audio::MovieVolumeSettings,
    scenario: Arc<Gerokasu2Scenario>,
    counters: LoadCounters,
    root: Gerokasu2Root,
    previous_root: PreviousRoot<Gerokasu2Root>,
    task: Option<shin_tasks::AsyncTask<Result<PreparedBasicDrawable>>>,
    prepared: Option<PreparedBasicDrawable>,
    prototype: Option<BasicDrawable>,
    resource_present: bool,
    resource_error: Option<String>,
    transition: LoadTransition,
    transition_task: Option<shin_tasks::AsyncTask<Result<AnyWiper>>>,
    prepared_wiper: Option<AnyWiper>,
    transition_present: bool,
    transition_error: Option<String>,
    load_plane: PlaneId,
    frame_started: Instant,
    skip: bool,
    animations: bool,
    loaded: Vec<(i32, [i32; 8])>,
    mask_loader: super::masks::AssetMaskLoader,
}
impl BasicLoadBackend {
    pub(crate) fn reconstruction_has_jobs(&self) -> bool {
        self.task.is_some() || self.transition_task.is_some() || self.mask_loader.has_job()
    }
    pub(crate) fn reconstruction_assets(&self) -> Arc<AssetServer> {
        self.assets.clone()
    }
    pub(crate) fn reconstruction_scenario(&self) -> Arc<Gerokasu2Scenario> {
        self.scenario.clone()
    }
    pub(crate) fn reconstruction_audio(&self) -> Option<Arc<shin_audio::AudioManager>> {
        self.movie_audio.clone()
    }
    pub(crate) fn reconstruction_root_mut(&mut self) -> &mut Gerokasu2Root {
        &mut self.root
    }
    /// Original13800 uses ADV+D0 when retained, otherwise ADV+C0.
    pub(crate) fn modal_capture_root_mut(&mut self) -> &mut Gerokasu2Root {
        self.selected_root_mut()
    }
    /// Native11190/13800 select the retained pre-mutation root when present.
    pub fn selected_root(&self) -> &Gerokasu2Root {
        self.previous_root.selected(&self.root)
    }
    pub fn selected_root_mut(&mut self) -> &mut Gerokasu2Root {
        self.previous_root.selected_mut(&mut self.root)
    }
    pub fn has_previous_root(&self) -> bool {
        !self.previous_root.is_none()
    }
    /// Called at native105E0/10AA0 only after a command poll returns pending.
    /// It must not run for a paused frame, completed command, or budget yield.
    pub fn command_blocked(&mut self) {
        self.previous_root.command_blocked(self.animations);
    }
    pub(crate) fn reconstruction_set_root(&mut self, root: Gerokasu2Root) {
        self.root = root;
    }
    pub(crate) fn reconstruction_snapshot(
        &self,
        ctx: &mut RenderCloneCtx,
        reuse_cache: bool,
    ) -> (Gerokasu2Root, Vec<Option<BasicDrawable>>) {
        let mut layers = Vec::new();
        for plane in self
            .root
            .screen()
            .active_page_layer()
            .planes()
            .iter()
            .filter(|_| reuse_cache)
        {
            for bank in plane.get_used_layerbank_ids() {
                if let Some(layer) = plane.get_layer(bank) {
                    if layer.layer.interaction().is_none() {
                        layers.push(layer.render_clone(ctx));
                    }
                }
            }
        }
        // Original19E44 excludes InteractiveLayer before cloning cache values.
        let cache = layers.into_iter().map(Some).collect();
        (self.root.render_clone(ctx), cache)
    }
    pub fn root(&self) -> &Gerokasu2Root {
        &self.root
    }
    pub fn attach_movie_audio(&mut self, audio: Arc<shin_audio::AudioManager>) {
        self.movie_audio = Some(audio);
    }
    pub fn movie_volume_settings(&self) -> crate::audio::MovieVolumeSettings {
        self.movie_volume_settings
    }
    pub fn set_movie_volume_settings(&mut self, settings: crate::audio::MovieVolumeSettings) {
        self.movie_volume_settings = settings;
    }
    pub fn screen(&self) -> &ScreenLayer<BasicDrawable> {
        self.root.screen()
    }
    pub fn screen_mut(&mut self) -> &mut ScreenLayer<BasicDrawable> {
        self.root.screen_mut()
    }
    pub fn message_resources(&self) -> Result<super::message_ui::MessageResources> {
        Ok(self
            .root
            .message_owner()
            .context("Ger message owner is not attached")?
            .lock()
            .resources())
    }
    pub fn attach_selection(&mut self, owner: super::selection::SharedSelection) {
        self.root.attach_selection(owner);
    }
    pub fn remove_selection(&mut self) {
        self.root.remove_selection();
    }
    pub fn attach_message(&mut self, owner: SharedMessageOwner) -> Result<()> {
        self.root.attach_message(owner)
    }
    pub fn prepare_message_frame(&mut self, ctx: &AdvUpdateContext) {
        self.previous_root
            .update_pair(&mut self.root, |root| root.prepare_message_frame(ctx));
    }
    pub fn prepare_root_fast_forward(&mut self) {
        self.previous_root
            .update_pair(&mut self.root, |root| root.fast_forward());
    }
    pub fn new(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        counters: LoadCounters,
    ) -> Self {
        Self {
            movie_audio: None,
            movie_volume_settings: Default::default(),
            assets,
            scenario,
            counters,
            root: Gerokasu2Root::new(),
            previous_root: PreviousRoot::default(),
            task: None,
            prepared: None,
            prototype: None,
            resource_present: false,
            resource_error: None,
            transition: Default::default(),
            transition_task: None,
            prepared_wiper: None,
            transition_present: false,
            transition_error: None,
            load_plane: PlaneId::new(0),
            frame_started: Instant::now(),
            skip: false,
            animations: true,
            loaded: Vec::new(),
            mask_loader: Default::default(),
        }
    }
    pub fn set_frame(&mut self, started: Instant, skip: bool) {
        self.frame_started = started;
        self.skip = skip;
    }
    pub fn set_transition(&mut self, transition: LoadTransition) {
        self.transition = transition;
    }
    pub fn animations_allowed(&self) -> bool {
        self.animations
    }
    pub fn has_back(&self) -> bool {
        self.screen().has_pending_page()
    }
    pub fn resource_error(&self) -> Option<&str> {
        self.resource_error.as_deref()
    }
    pub fn transition_error(&self) -> Option<&str> {
        self.transition_error.as_deref()
    }
    pub fn loaded_notifications(&self) -> &[(i32, [i32; 8])] {
        &self.loaded
    }
    pub fn take_loaded_notifications(&mut self) -> Vec<(i32, [i32; 8])> {
        std::mem::take(&mut self.loaded)
    }
    pub(crate) fn reconstruction_source(&self, ctx: &mut RenderCloneCtx) -> Box<Self> {
        let mut source = Box::new(Self::new(
            self.assets.clone(),
            self.scenario.clone(),
            LoadCounters::from_layers(&Gerokasu2Layers::default()),
        ));
        source.root = self.root.render_clone(ctx);
        source.movie_audio = self.movie_audio.clone();
        source.movie_volume_settings = self.movie_volume_settings;
        source
    }
    /// Called on the host, outside executor workers, before the source VM dies.
    pub(crate) fn retire_reconstruction_jobs(&mut self) {
        self.mask_loader.retire();
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        if let Some(task) = self.transition_task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        self.prepared = None;
        self.prototype = None;
        self.prepared_wiper = None;
    }
    pub fn layer(&self, plane: PlaneId, bank: LayerbankId) -> Option<LayerRead<'_, BasicDrawable>> {
        self.planes()[usize::from(plane.raw())].get_layer(bank)
    }
    pub fn layer_mut(
        &mut self,
        plane: PlaneId,
        bank: LayerbankId,
    ) -> Option<LayerWrite<'_, BasicDrawable>> {
        self.planes_mut()[usize::from(plane.raw())].get_layer_mut(bank)
    }
    fn planes(&self) -> &[LayerGroup<BasicDrawable>] {
        self.screen().page_layer().planes()
    }
    fn planes_mut(&mut self) -> &mut [LayerGroup<BasicDrawable>] {
        self.screen_mut().page_layer_mut().planes_mut()
    }
    pub fn update(&mut self, context: &AdvUpdateContext) {
        self.previous_root
            .update_pair(&mut self.root, |root| root.update(context));
    }
    pub fn fast_forward(&mut self) {
        self.previous_root
            .update_pair(&mut self.root, |root| root.fast_forward());
    }
    /// The caller supplies the page/parent boundary; these methods prepare and
    /// render the selected bank plane, not a complete Screen/Page root.
    pub fn pre_render_plane(
        &mut self,
        plane: PlaneId,
        context: &mut PreRenderContext,
        transform: &TransformParams,
    ) {
        self.planes_mut()[usize::from(plane.raw())].pre_render(context, transform);
    }
    pub fn render_plane(
        &self,
        plane: PlaneId,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil: u8,
        kind: PassKind,
    ) {
        self.planes()[usize::from(plane.raw())].render(pass, transform, stencil, kind);
    }
    pub fn pre_render_scene(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
    ) -> Result<()> {
        let root = self.selected_root_mut();
        root.pre_render(context, transform);
        root.check_render()?;
        Ok(())
    }
    pub fn render_scene(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        let root = self.selected_root();
        root.render(pass, transform, stencil, kind);
        root.check_render()
    }
    fn accept_transition(&mut self, result: Result<AnyWiper>) -> Result<()> {
        match result {
            Ok(wiper) => {
                self.prepared_wiper = Some(wiper);
                self.transition_error = None;
                Ok(())
            }
            Err(error) => {
                self.transition_error = Some(format!("{error:#}"));
                Err(error)
            }
        }
    }
    pub fn retry_transition(&mut self, settings: LoadTransition) -> Result<()> {
        self.start_transition(settings)
    }
    fn accept(&mut self, result: Result<PreparedBasicDrawable>) -> Result<()> {
        match result {
            Ok(prepared) => {
                self.prepared = Some(prepared);
                self.resource_error = None;
                Ok(())
            }
            Err(error) => {
                self.resource_error = Some(format!("{error:#}"));
                Err(error)
            }
        }
    }
    /// Retry only the retained resource request, leaving integer counters and
    /// old drawable owners untouched. Native task exception recovery is outside
    /// the CPU oracle; this explicit host retry is exercised separately.
    pub fn retry_resource(&mut self, plan: &LoadPlan) -> Result<()> {
        self.start_resource(plan)
    }
}

impl LayerEditServices for BasicLoadBackend {
    fn edit_layers(&mut self, edit: &LayerEdit) -> Result<()> {
        if let LayerEdit::Swap { plane, targets } = edit {
            for target in targets {
                ensure!(
                    self.layer(*plane, target.layerbank).is_some(),
                    "Missing Gerokasu2 swap bank {:?} in {plane:?}",
                    target.layerbank
                );
            }
        }
        self.capture_previous_scene()?;
        let group = self
            .screen_mut()
            .page_layer_mut()
            .get_plane_mut(edit.plane());
        match edit {
            LayerEdit::Unload { targets, delay, .. } => {
                for target in targets {
                    group.remove_layer(target.layerbank, Ticks::from_f32(*delay as f32));
                }
            }
            LayerEdit::Swap { targets, .. } => {
                for target in targets {
                    group
                        .get_layer_mut(target.layerbank)
                        .unwrap()
                        .properties_mut()
                        .set_layer_id(target.layer);
                }
            }
            LayerEdit::Clear { .. } => {
                group.clear_layers();
                group.clear_mask_texture();
            }
        }
        Ok(())
    }
}

impl LayerControlServices for BasicLoadBackend {
    fn supports_root(&self) -> bool {
        self.root.has_message()
    }
    fn begin_property_change(&mut self) -> Result<()> {
        self.capture_previous_scene()
    }
    fn visit_properties(
        &mut self,
        targets: &[PropertyTarget],
        action: &mut dyn FnMut(&mut LayerProperties),
    ) -> Result<()> {
        // No target may be changed before the complete address set is valid.
        for &target in targets {
            match target {
                PropertyTarget::Root | PropertyTarget::Message => ensure!(
                    self.root.has_message(),
                    "Gerokasu2 outer root owner is not attached"
                ),
                PropertyTarget::Bank { plane, bank } => ensure!(
                    self.layer(plane, bank).is_some(),
                    "Missing Gerokasu2 drawable bank {bank:?} in {plane:?}"
                ),
                _ => {}
            }
        }
        for &target in targets {
            match target {
                PropertyTarget::Root => action(self.root.properties_mut()),
                PropertyTarget::Message => action(self.root.message_properties_mut().unwrap()),
                PropertyTarget::Screen => action(self.screen_mut().properties_mut()),
                PropertyTarget::Page => action(self.screen_mut().page_layer_mut().properties_mut()),
                PropertyTarget::Plane(plane) => action(
                    self.screen_mut()
                        .page_layer_mut()
                        .get_plane_mut(plane)
                        .properties_mut(),
                ),
                PropertyTarget::Bank { plane, bank } => {
                    action(self.layer_mut(plane, bank).unwrap().properties_mut())
                }
            }
        }
        Ok(())
    }
    fn bank_wiping(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<bool> {
        Ok(self
            .screen()
            .page_layer()
            .get_plane(plane)
            .has_wiper_for_layerbank(bank))
    }
}

impl SceneServices for BasicLoadBackend {
    fn observe_screen(&mut self) -> Result<()> {
        Ok(())
    }
    fn capture_previous_scene(&mut self) -> Result<()> {
        if self.previous_root.is_none() {
            let device = self.assets.context().wgpu_device.clone();
            let mut encoder = device.create_command_encoder(&Default::default());
            let mut ctx = RenderCloneCtx {
                device: &device,
                encoder: &mut encoder,
            };
            self.previous_root
                .capture(&self.root, |root| root.render_clone(&mut ctx));
            self.assets.context().wgpu_queue.submit([encoder.finish()]);
        }
        Ok(())
    }
    fn pageback(&mut self, start_anew: bool) -> Result<()> {
        self.capture_previous_scene()?;
        let device = self.assets.context().wgpu_device.clone();
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut ctx = RenderCloneCtx {
            device: &device,
            encoder: &mut encoder,
        };
        self.screen_mut().pageback(&mut ctx, start_anew);
        for plane in self.screen_mut().page_layer_mut().planes_mut() {
            plane.stop_transitions();
        }
        self.assets.context().wgpu_queue.submit([encoder.finish()]);
        Ok(())
    }
    fn promote_pending_page(&mut self, with_transition: bool) -> Result<()> {
        let wiper = if with_transition {
            let device = self.assets.context().wgpu_device.clone();
            let mut encoder = device.create_command_encoder(&Default::default());
            let mut ctx = RenderCloneCtx {
                device: &device,
                encoder: &mut encoder,
            };
            let value = self
                .prepared_wiper
                .as_ref()
                .context("Ger scene has no prepared wiper")?
                .render_clone(&mut ctx);
            self.assets.context().wgpu_queue.submit([encoder.finish()]);
            Some(value)
        } else {
            None
        };
        self.screen_mut().apply_transition(wiper);
        Ok(())
    }
    fn scene_wiping(&mut self) -> Result<bool> {
        Ok(self.screen().is_transition_active())
    }
}
impl super::masks::MaskServices for BasicLoadBackend {
    fn start_mask_resource(&mut self, id: i32) -> Result<bool> {
        self.mask_loader
            .start(self.assets.clone(), &self.scenario, id)
    }
    fn mask_resource_ready(&mut self) -> Result<bool> {
        self.mask_loader.ready()
    }
    fn capture_mask_source(&mut self) -> Result<()> {
        self.capture_previous_scene()
    }
    fn publish_mask(&mut self, plane: PlaneId, flags: i32) -> Result<()> {
        let mask = self.mask_loader.texture()?;
        self.screen_mut()
            .page_layer_mut()
            .get_plane_mut(plane)
            .set_mask_texture(
                mask,
                shin_core::vm::command::types::MaskFlags::from_bits_retain(flags),
            );
        Ok(())
    }
    fn clear_mask(&mut self, plane: PlaneId) -> Result<()> {
        self.screen_mut()
            .page_layer_mut()
            .get_plane_mut(plane)
            .clear_mask_texture();
        Ok(())
    }
    fn set_mask_animations_allowed(&mut self, allowed: bool) {
        self.set_animations_allowed(allowed);
    }
    fn mask_command_finished(&mut self) -> Result<()> {
        self.mask_loader.finish();
        Ok(())
    }
}
impl LoadServices for BasicLoadBackend {
    fn frame_elapsed_us(&self) -> u64 {
        self.frame_started
            .elapsed()
            .as_micros()
            .min(u128::from(u64::MAX)) as u64
    }
    fn prepare_plan(
        &mut self,
        layers: &mut Gerokasu2Layers,
        command: &Gerokasu2RuntimeCommand,
    ) -> Result<LoadPlan> {
        let plan = LoadPlan::prepare(layers, &mut self.counters, command)?;
        self.load_plane = plan.plane;
        self.task = None;
        self.prepared = None;
        self.prototype = None;
        self.resource_present = false;
        self.resource_error = None;
        self.clear_transition();
        Ok(plan)
    }
    fn start_resource(&mut self, plan: &LoadPlan) -> Result<()> {
        self.task = None;
        self.prepared = None;
        self.prototype = None;
        self.resource_error = None;
        let p = plan.params;
        let request = LayerFactoryRequest::resolve(
            &self.scenario,
            plan.kind,
            (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]),
        )
        .context("Gerokasu2 layer type produced no factory")?;
        self.resource_present = true;
        match request {
            LayerFactoryRequest::Null => self.prepared = Some(PreparedBasicDrawable::Null),
            LayerFactoryRequest::Tile { color, rect } => {
                self.prepared = Some(PreparedBasicDrawable::Tile { color, rect })
            }
            LayerFactoryRequest::Effect => {
                self.prepared = Some(PreparedBasicDrawable::Procedural(
                    super::procedural_animation::PreparedProcedural::Effect,
                ))
            }
            LayerFactoryRequest::Movie { .. } => {
                let audio = self
                    .movie_audio
                    .clone()
                    .context("Ger movie audio manager is not attached")?;
                let assets = self.assets.clone();
                let settings = self.movie_volume_settings;
                self.task = Some(shin_tasks::async_io::spawn_loader(async move {
                    Ok(PreparedBasicDrawable::Movie(parking_lot::Mutex::new(
                        super::movie::load(&assets, &audio, &request, settings).await?,
                    )))
                }));
            }
            LayerFactoryRequest::Picture { .. }
            | LayerFactoryRequest::Bustup { .. }
            | LayerFactoryRequest::CardSelect { .. }
            | LayerFactoryRequest::Animation { .. }
            | LayerFactoryRequest::FocusLine { .. }
            | LayerFactoryRequest::Rain { .. } => {
                let assets = self.assets.clone();
                self.task = Some(shin_tasks::async_io::spawn_loader(async move {
                    PreparedBasicDrawable::load(&assets, &request).await
                }));
            }
        }
        Ok(())
    }
    fn resource_present(&self) -> bool {
        self.resource_present
    }
    fn wait_resource(&mut self, micros: u32) -> Result<()> {
        if let Some(task) = &mut self.task {
            if let Some(result) = task.poll_with_timeout(Duration::from_micros(u64::from(micros))) {
                self.task = None;
                self.accept(result)?;
            }
        }
        Ok(())
    }
    fn resource_ready(&mut self) -> Result<bool> {
        if let Some(error) = &self.resource_error {
            bail!("Retained Gerokasu2 resource failed: {error}");
        }
        if let Some(task) = &mut self.task {
            if let Some(result) = task.poll_naive() {
                self.task = None;
                self.accept(result)?;
            }
        }
        Ok(self.prepared.is_some())
    }
    fn set_animations_allowed(&mut self, a: bool) {
        self.animations = a;
    }
    fn animations_allowed(&self) -> bool {
        self.animations
    }
    fn transition(&self) -> LoadTransition {
        self.transition
    }
    fn configure_transition(&mut self, settings: LoadTransition) -> Result<()> {
        self.set_transition(settings);
        Ok(())
    }
    fn start_transition(&mut self, t: LoadTransition) -> Result<()> {
        self.clear_transition();
        let Some(request) =
            TransitionRequest::resolve(&self.scenario, t.kind, t.duration, t.params)
        else {
            return Ok(());
        };
        let assets = self.assets.clone();
        self.transition_present = true;
        self.transition_task = Some(shin_tasks::async_io::spawn_loader(async move {
            request.load(&assets).await
        }));
        Ok(())
    }
    fn transition_present(&self) -> bool {
        self.transition_present
    }
    fn wait_transition(&mut self, micros: u32) -> Result<()> {
        if let Some(task) = &mut self.transition_task {
            if let Some(result) = task.poll_with_timeout(Duration::from_micros(u64::from(micros))) {
                self.transition_task = None;
                self.accept_transition(result)?;
            }
        }
        Ok(())
    }
    fn clear_transition(&mut self) {
        self.transition_task = None;
        self.prepared_wiper = None;
        self.transition_present = false;
        self.transition_error = None;
    }
    fn transition_ready(&mut self) -> Result<bool> {
        if let Some(error) = &self.transition_error {
            bail!("Retained Gerokasu2 transition failed: {error}");
        }
        if let Some(task) = &mut self.transition_task {
            if let Some(result) = task.poll_naive() {
                self.transition_task = None;
                self.accept_transition(result)?;
            }
        }
        Ok(self.prepared_wiper.is_some())
    }
    fn skip_requested(&mut self) -> bool {
        self.skip
    }
    fn publish(&mut self, plan: &LoadPlan, with_transition: bool) -> Result<()> {
        self.capture_previous_scene()?;
        ensure!(
            !with_transition || self.prepared_wiper.is_some(),
            "Gerokasu2 publication has no prepared wiper"
        );
        let device = self.assets.context().wgpu_device.clone();
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut ctx = RenderCloneCtx {
            device: &device,
            encoder: &mut encoder,
        };
        if plan.needs_loader
            && matches!(
                self.prepared,
                Some(
                    PreparedBasicDrawable::Picture(..)
                        | PreparedBasicDrawable::Bustup(..)
                        | PreparedBasicDrawable::CardSelect(..)
                )
            )
            && self.prototype.is_none()
        {
            self.prototype = Some(self.prepared.as_ref().unwrap().construct());
        }
        let banks = self.planes();
        let mut replacements = Vec::with_capacity(plan.targets.len());
        for target in &plan.targets {
            let old = banks[usize::from(plan.plane.raw())].get_layer(target.bank);
            let drawable = if target.already_same {
                old.as_ref()
                    .context("Reused Gerokasu2 bank has no drawable owner")?
                    .render_clone(&mut ctx)
            } else if let Some(prototype) = &self.prototype {
                prototype.render_clone(&mut ctx)
            } else if let Some(movie) = self
                .prepared
                .as_ref()
                .and_then(|p| p.construct_movie(&mut ctx))
            {
                movie
            } else {
                self.prepared
                    .as_ref()
                    .context("Changed Gerokasu2 bank has no prepared resource")?
                    .try_construct()?
            };
            let properties = if target.keep_properties {
                let old = old.context("Kept Gerokasu2 bank has no drawable owner")?;
                let props = old.properties();
                if with_transition && plan.flags & 8 == 0 {
                    props.clone()
                } else {
                    props.share()
                }
            } else {
                let mut p = LayerProperties::with_policy(crate::layer::PropertyPolicy::Gerokasu2);
                p.set_layer_id(target.logical);
                p
            };
            let wiper = with_transition
                .then(|| self.prepared_wiper.as_ref().unwrap().render_clone(&mut ctx));
            replacements.push((target, drawable, properties, wiper));
        }
        self.assets.context().wgpu_queue.submit([encoder.finish()]);
        let banks = self.planes_mut();
        for (target, mut drawable, mut properties, wiper) in replacements {
            properties.set_layerload_counter1(target.load_counter);
            drawable.set_properties(properties);
            banks[usize::from(plan.plane.raw())].add_layer_with_wiper(target.bank, drawable, wiper);
        }
        Ok(())
    }
    fn loaded_layer(&mut self, kind: i32, params: [i32; 8]) -> Result<()> {
        self.loaded.push((kind, params));
        Ok(())
    }
    fn any_target_wiping(&mut self, targets: &[LoadTarget]) -> Result<bool> {
        let plane = &self.planes()[usize::from(self.load_plane.raw())];
        Ok(targets
            .iter()
            .any(|target| plane.has_wiper_for_layerbank(target.bank)))
    }
    fn command_finished(&mut self) -> Result<()> {
        self.task = None;
        self.prepared = None;
        self.prototype = None;
        self.resource_present = false;
        self.clear_transition();
        Ok(())
    }
}

#[cfg(test)]
#[path = "drawables/back_root_tests.rs"]
mod back_root_tests;
#[cfg(test)]
mod tests;

impl super::movie::MovieServices for BasicLoadBackend {
    fn movie_status(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<u32> {
        Ok(self
            .layer(plane, bank)
            .map_or(0, |layer| match &layer.layer {
                super::layer_factory::GerDrawable::Shared(
                    crate::layer::user::UserLayer::Movie(movie),
                ) => movie.gerokasu2_wait_status(),
                _ => 0,
            }))
    }
}
