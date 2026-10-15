//! Updated d2d74 saved preview VM and ae778's two independent worker owners.
//! Preview callbacks consume saved responses, never live globals or commit queues.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use anyhow::{Result, bail, ensure};
use shin_core::{
    format::{save::PersistData, scenario::higurashi::HigurashiScenario},
    vm::{
        command::{CommandResult, types::LayerType},
        higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand as Cmd},
    },
};
use shin_render::{
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::texture::{DepthStencilTarget, TextureSource},
};

use super::super::{runtime_game_data::RuntimeGameData, runtime_saves::Slots};
mod animation;
mod effect_capture;
mod effects;
pub(super) mod globals;
mod notification;
mod postprocessing;
mod temporary;
use crate::{
    adv::higurashi::{
        group,
        layers::{LayerBanks, SLOTS, Target},
        save_menu::{
            model::ViewState,
            resources::{self, Labels, Row, Rows},
        },
        variables::HigurashiVariables,
    },
    asset::system::AssetServer,
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
        user::higurashi::HigurashiUserLayer,
    },
    render::PreRenderContext,
};

pub(super) type SeedSource = Arc<dyn Fn() -> u32 + Send + Sync>;
pub(super) fn os_seed() -> u32 {
    getrandom::u32().expect("Higurashi preview layer OS entropy unavailable")
}

/// d3200 packs only occupied physical front rows; roots and saved SAVEINFO
/// labels follow. No message, audio, page, back plane or pending queue is copied.
#[derive(Clone, Debug)]
pub(super) struct Hydrated {
    pub records: [[i32; 79]; SLOTS],
    pub root: [i32; 65],
    pub screen: [i32; 65],
    pub labels: [Arc<str>; 2],
    pub scenario_id: i32,
}

impl Hydrated {
    pub fn replay(scenario: &HigurashiScenario, data: &RuntimeGameData) -> Result<Self> {
        let entry = data.progress.active_entry();
        let mut layers = LayerBanks::default();
        let mut current: [Arc<str>; 2] = std::array::from_fn(|_| Arc::from(""));
        let mut labels = current.clone();
        if entry.save_position != 0 {
            let mut vm = HigurashiScripter::new(scenario, entry.scenario_id, entry.random_seed)?;
            let mut responses = HigurashiVariables::new(PersistData(vec![]), vec![]);
            responses.restore_route(entry.response_data.clone(), 0)?;
            let mut counter = 0u32;
            let mut event = -1i32;
            let mut last_text = false;
            let mut feedback = CommandResult::None;
            let mut done = false;
            let boundary = |counter: &mut u32| {
                *counter = counter.wrapping_add(1);
                *counter >= entry.save_position
            };
            // Native runs until its dedicated preview stop callback. A bounded
            // diagnostic prevents corrupt authored loops from occupying a worker.
            for _ in 0..1_000_000 {
                let command = vm.run(feedback, 100_000)?;
                feedback = CommandResult::None;
                match command {
                    Cmd::EXIT { arg1: 0, .. } => done = true,
                    Cmd::SGET { dest, .. } => {
                        feedback =
                            CommandResult::WriteMemory(dest, responses.response(false, true, 0)?)
                    }
                    Cmd::OpcodeDA { dest, .. } => {
                        feedback =
                            CommandResult::WriteMemory(dest, responses.response(true, true, 0)?)
                    }
                    Cmd::MSGSET { .. } => {
                        if event < 0 {
                            done = boundary(&mut counter);
                        }
                        last_text = true;
                    }
                    Cmd::MSGCLOSE { .. } => last_text = false,
                    Cmd::SELECT { dest, .. } => {
                        if !last_text {
                            done = boundary(&mut counter);
                        }
                        if !done {
                            feedback = CommandResult::WriteMemory(
                                dest,
                                responses.response_choice(true, 0)?,
                            );
                            done = boundary(&mut counter);
                        }
                    }
                    Cmd::SAVEINFO { level, info } => {
                        if let Some(slot) = current.get_mut(usize::try_from(level)?) {
                            *slot = Arc::from(info);
                        } else {
                            bail!("Saved preview SAVEINFO level outside0..2");
                        }
                    }
                    Cmd::AUTOSAVE {} => {
                        done = boundary(&mut counter);
                        labels = current.clone();
                    }
                    Cmd::EVBEGIN { argument } => {
                        done = boundary(&mut counter);
                        event = argument;
                    }
                    Cmd::EVEND {} => {
                        done = boundary(&mut counter);
                        event = -1;
                    }
                    Cmd::WIPE { .. } => {
                        layers.commit_back();
                    }
                    Cmd::LAYERINIT { id } => {
                        layers.initialize(id)?;
                    }
                    Cmd::LAYERLOAD {
                        args: [id, kind, flags],
                        params,
                    } => {
                        layers.load(id, kind, flags as u32, params)?;
                    }
                    Cmd::LAYERUNLOAD { args: [id, _] } => {
                        layers.unload(id)?;
                    }
                    Cmd::LAYERCTRL {
                        args: [id, property],
                        params,
                    } => {
                        if property != 28 && property != 31 {
                            layers.control(id, property, params.0, params.2 as u32)?;
                        }
                    }
                    Cmd::LAYERBACK { argument } => {
                        layers.start_back(argument)?;
                    }
                    Cmd::LAYERSELECT { args: [from, to] } => layers.select_native_u16(from, to),
                    Cmd::LAYERSWAP { args: [from, to] } => layers.swap_native(from, to),
                    Cmd::FEELICON {
                        args: [id, character, x, y, power],
                    } => voice_override(&mut layers, id, [character, x, y, power]),
                    Cmd::TIPSGET { ids } => {
                        done = boundary(&mut counter);
                        if done {
                            temporary(&mut layers, 12, &ids)?;
                        } else {
                            done = boundary(&mut counter);
                        }
                    }
                    Cmd::CHARSELECT { dest, argument, .. } => {
                        done = boundary(&mut counter);
                        if done {
                            temporary(&mut layers, 11, &[argument])?;
                        } else {
                            feedback = CommandResult::WriteMemory(
                                dest,
                                responses.response_choice(true, 0)?,
                            );
                            done = boundary(&mut counter);
                        }
                    }
                    Cmd::SNRSEL { .. } | Cmd::KAKERA {} => done = boundary(&mut counter),
                    Cmd::QUIZ { dest, .. } => {
                        done = boundary(&mut counter);
                        // d59a4 consumes feedback even at the stopping boundary.
                        feedback =
                            CommandResult::WriteMemory(dest, responses.response_choice(true, 0)?);
                    }
                    // d49e0/d5a84 suppress every persistent/byte setter. Audio,
                    // waits, unlocks, movies, notifications and other UI callbacks
                    // are actual no-op virtuals in the dedicated preview VM.
                    _ => {}
                }
                if done {
                    break;
                }
            }
            ensure!(
                done,
                "Saved preview exceeded native replay diagnostic budget"
            );
        }
        let mut records = [[0; 79]; SLOTS];
        for record in &mut records {
            record[0] = -1;
        }
        let mut used = 0;
        for slot in 0..SLOTS {
            let record = layers.record(Target {
                logical: 0,
                slot: slot as u8,
                bank: 0,
            });
            if record.layer_type() >= 0 {
                records[used] = *record.native_words();
                used += 1;
            }
        }
        Ok(Self {
            records,
            root: *layers.root_targets(),
            screen: *layers.front_screen_targets(),
            labels,
            scenario_id: entry.scenario_id,
        })
    }
}

fn voice_override(layers: &mut LayerBanks, id: i32, values: [i32; 4]) {
    let [from, to] = layers.selected_native();
    let ids: Vec<_> = if id == -4 {
        (from..=to).collect()
    } else if (0..1024).contains(&id) {
        vec![id as u16]
    } else {
        vec![]
    };
    for id in ids {
        if let Some(target) = layers.target(id) {
            layers.record_mut(target).native_words_mut()[2..6].copy_from_slice(&values);
        }
    }
}
fn temporary(layers: &mut LayerBanks, kind: i32, ids: &[i32]) -> Result<()> {
    let target = layers.reserve_temporary()?;
    let words = layers.record_mut(target).native_words_mut();
    words[0] = kind;
    words[1] = i32::from(target.logical);
    words[2] = -1;
    words[14..].copy_from_slice(&crate::layer::higurashi_properties::INITIAL_VALUES);
    if kind == 11 {
        words[6] = ids[0];
    } else {
        ensure!(
            ids.len() <= 6,
            "Saved preview TIPSGET exceeds six native parameter lanes"
        );
        words[6] = 0;
        words[7] = ids.len() as i32;
        words[8..8 + ids.len()].copy_from_slice(ids);
    }
    // d5714/d58c8 use the retained back selector for these two flags after
    // writing temporary resource metadata into physical front.
    let selected = layers.target(target.logical).unwrap();
    let words = layers.record_mut(selected).native_words_mut();
    words[14 + 22] = 1;
    words[14 + 5] = 1;
    Ok(())
}

fn targets(values: &[i32; 65], seeds: [u32; 4]) -> HigurashiProperties {
    let mut result = HigurashiProperties::from_seeds(seeds);
    for (i, value) in values.iter().enumerate() {
        result
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to(*value as f32);
    }
    result
}

/// d3710 prepares all factories before cloning them in compact row order.
/// Type7 is explicitly mapped to Null, unlike Umineko's linked Movie picture.
enum Layer {
    Basic(HigurashiUserLayer),
    Effect {
        view: effects::View,
        properties: HigurashiProperties,
        cache: parking_lot::Mutex<Option<effect_capture::Cache>>,
        shared_depth: effect_capture::SharedDepth,
    },
    Animation {
        view: animation::View,
        properties: HigurashiProperties,
    },
    Notification {
        view: notification::View,
        properties: HigurashiProperties,
    },
    Temporary {
        view: temporary::View,
        properties: HigurashiProperties,
        globals: globals::Shared,
    },
}
impl group::Drawable for Layer {
    fn properties(&self) -> &HigurashiProperties {
        match self {
            Self::Basic(layer) => &layer.properties,
            Self::Temporary { properties, .. }
            | Self::Animation { properties, .. }
            | Self::Effect { properties, .. }
            | Self::Notification { properties, .. } => properties,
        }
    }
    fn render(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: shin_render::PassKind,
    ) -> Result<()> {
        match self {
            Self::Basic(layer) => layer.render(pass, parent, stencil, kind),
            Self::Effect {
                view,
                properties,
                cache,
                ..
            } => {
                let cache = cache.lock();
                let cache = cache
                    .as_ref()
                    .context("Saved effect needs lower-prefix GPU preparation")?;
                let sampler = if view.kind() == 6 {
                    shin_render::shaders::types::texture::TextureSampler::HigurashiMosaic
                } else {
                    shin_render::shaders::types::texture::TextureSampler::Linear
                };
                view.render(pass, cache.source(sampler), properties, stencil, kind)
            }
            Self::Animation { view, properties } => {
                view.render(pass, properties, parent, stencil, kind)
            }
            Self::Notification { view, properties } => {
                view.render(pass, properties, parent, stencil, kind)
            }
            Self::Temporary {
                view: temporary::View::Character(view),
                properties,
                globals,
            } => {
                // Native444b4 reads the current route preference at render.
                let mut view = view.retain();
                view.route_navigation = globals.lock().snapshot.route_navigation;
                view.render(pass, properties, parent, stencil, kind)
            }
            Self::Temporary {
                view, properties, ..
            } => view.render(pass, properties, parent, stencil, kind),
        }
    }
}
enum PreparedLayer {
    Basic(HigurashiUserLayer),
    Temporary(temporary::Prepared),
    Animation(animation::Prepared),
    Notification(notification::Prepared),
    Effect(effects::Prepared),
}
pub(super) struct Scene {
    layers: Vec<(u8, i32, Layer)>,
    root: HigurashiProperties,
    screen: HigurashiProperties,
}
impl Scene {
    #[cfg(test)]
    async fn load(
        server: &AssetServer,
        scenario: &HigurashiScenario,
        saved: &Hydrated,
        group_seeds: [[u32; 4]; 2],
        seeds: &SeedSource,
        setting: u8,
    ) -> Result<Self> {
        let globals = globals::State::shared(globals::Snapshot {
            asset_setting: setting,
            route_navigation: true,
            tips: Vec::new(),
        });
        Self::load_with_globals(
            server,
            scenario,
            saved,
            group_seeds,
            seeds,
            globals,
            &AtomicBool::new(false),
        )
        .await?
        .context("Saved preview unexpectedly cancelled")
    }
    async fn load_with_globals(
        server: &AssetServer,
        scenario: &HigurashiScenario,
        saved: &Hydrated,
        group_seeds: [[u32; 4]; 2],
        seeds: &SeedSource,
        globals: globals::Shared,
        cancelled: &AtomicBool,
    ) -> Result<Option<Self>> {
        let (route_navigation, setting) = {
            let state = globals.lock();
            (
                state.snapshot.route_navigation,
                state.snapshot.asset_setting,
            )
        };
        let prepared = futures::future::try_join_all(saved.records.iter().enumerate().map(|(slot, record)| async move {
            if matches!(record[0],5|6|10) {
                return Ok(Some((slot,record[1],PreparedLayer::Effect(effects::Prepared::new(record[0])?))));
            }
            if matches!(record[0],4|8|9) {
                let params:&[i32;8]=record[6..14].try_into().unwrap();
                return Ok(Some((slot,record[1],PreparedLayer::Animation(
                    animation::Prepared::load(server,scenario,record[0],params).await?
                ))));
            }
            if matches!(record[0],13|14) {
                let params:&[i32;8]=record[6..14].try_into().unwrap();
                return Ok(Some((slot,record[1],PreparedLayer::Notification(
                    notification::Prepared::load(server,scenario,record[0],params).await?
                ))));
            }
            if matches!(record[0],11|12) {
                let params:&[i32;8]=record[6..14].try_into().unwrap();
                return Ok(Some((slot,record[1],PreparedLayer::Temporary(
                    temporary::Prepared::load(server,scenario,record[0],params,route_navigation).await?
                ))));
            }
            let kind = match record[0] {
                value if !(0..=14).contains(&value) => return Ok(None),
                0 | 7 => LayerType::Null,
                1 => LayerType::Tile,
                // 20df0 falls back to a real Null factory for absent catalog
                // entries, including negative IDs. Asset read failures still
                // propagate when the corresponding metadata exists.
                2 if usize::try_from(record[6]).ok().and_then(|id| scenario.picture_info.get(id)).is_none() => LayerType::Null,
                3 if usize::try_from(record[6]).ok().and_then(|id| scenario.bustup_info.get(id)).is_none() => LayerType::Null,
                2 => LayerType::Picture,
                3 => LayerType::Bustup,
                value => bail!("Saved preview native layer type{value} needs its original factory/render implementation"),
            };
            let p = &record[6..14];
            let owner = HigurashiUserLayer::load_with_setting(server, scenario, kind, (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]), [0; 4], setting).await?;
            Ok::<_, anyhow::Error>(Some((slot, record[1], PreparedLayer::Basic(owner))))
        })).await?;
        let mut layers = Vec::new();
        let shared_effect_depth = Arc::new(parking_lot::Mutex::new(None));
        for (slot, id, owner) in prepared.into_iter().flatten() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(None);
            }
            // Original25180 can return NULL from a ready Animation factory.
            // It consumes neither base OS nor derived global MT samples and
            // must leave a hole at its physical compact slot.
            if matches!(&owner, PreparedLayer::Animation(p) if !p.clone_present()) {
                continue;
            }
            let values: &[i32; 65] = saved.records[slot][14..].try_into().unwrap();
            let properties = targets(values, std::array::from_fn(|_| seeds()));
            let owner = match owner {
                PreparedLayer::Basic(mut owner) => {
                    owner.fast_forward();
                    owner.properties = properties;
                    Layer::Basic(owner)
                }
                PreparedLayer::Animation(prepared) => {
                    let view = prepared
                        .clone_refresh()?
                        .context("Present saved animation clone returned NULL")?;
                    Layer::Animation { view, properties }
                }
                PreparedLayer::Notification(prepared) => {
                    let view = prepared.clone_refresh()?;
                    Layer::Notification { view, properties }
                }
                PreparedLayer::Effect(prepared) => Layer::Effect {
                    view: prepared.clone_refresh(),
                    properties,
                    cache: parking_lot::Mutex::new(None),
                    shared_depth: shared_effect_depth.clone(),
                },
                PreparedLayer::Temporary(prepared) => {
                    let view = prepared.clone_refresh(&mut *globals.lock())?;
                    Layer::Temporary {
                        view,
                        properties,
                        globals: globals.clone(),
                    }
                }
            };
            // Fresh Higurashi BUPs have mouth0/fixed eye and are never advanced
            // or given voice feedback by this owner (d404c suspend boundary).
            layers.push((slot as u8, id, owner));
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        Ok(Some(Self {
            layers,
            root: targets(&saved.root, group_seeds[0]),
            screen: targets(&saved.screen, group_seeds[1]),
        }))
    }
    #[cfg(test)]
    pub fn render(&self, pass: &mut shin_render::render_pass::RenderPass) -> Result<()> {
        ensure!(
            !self
                .layers
                .iter()
                .any(|(_, _, layer)| matches!(layer, Layer::Effect { .. })),
            "Saved effect lower-prefix preparation requires its complete capture context"
        );
        ensure!(
            postprocessing::Packet::new(&self.root).is_none()
                && postprocessing::Packet::new(&self.screen).is_none(),
            "Saved preview color/shader processing requires its complete capture context"
        );
        self.render_contents(pass)
    }
    fn render_contents(&self, pass: &mut shin_render::render_pass::RenderPass) -> Result<()> {
        let children = self
            .layers
            .iter()
            .map(|(slot, owner_id, layer)| group::DrawableChild {
                owner_id: *owner_id,
                slot: *slot,
                layer,
            })
            .collect();
        group::render_capture_drawables(
            pass,
            children,
            &self.screen,
            &self.root.composed_transform(&HigurashiTransform::default()),
        )
    }
    fn capture(&self, context: &mut PreRenderContext, canvas: (u32, u32)) -> Result<RenderTexture> {
        let (width, height) = resources::thumbnail_size(canvas);
        ensure!(
            width > 0 && height > 0,
            "Saved preview needs a nonzero physical canvas"
        );
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            width, height,
        )));
        let depth = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi saved preview depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let mut ctx = PreRenderContext {
            device: context.device,
            queue: context.queue,
            breakup_texture: context.breakup_texture,
            resize_source: &resize,
            sampler_store: context.sampler_store,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: context.pipeline_storage,
            dynamic_buffer: context.dynamic_buffer,
            encoder: context.encoder,
        };
        let mut texture = ctx.new_render_texture("Higurashi saved game preview".into());
        if self
            .layers
            .iter()
            .any(|(_, _, layer)| matches!(layer, Layer::Effect { .. }))
        {
            {
                let mut pass = ctx.begin_pass(
                    texture.as_texture_target(),
                    Some(ctx.depth_stencil),
                    "Higurashi saved preview initial clear",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.),
                );
            }
            self.prepare_effects(&mut ctx)?;
            let depth = ctx.depth_stencil;
            let mut pass = ctx.begin_pass_preserving_depth_stencil(
                texture.as_texture_target(),
                depth,
                "Higurashi saved preview after lower captures",
            );
            self.render_contents(&mut pass)?;
        } else {
            let mut pass = ctx.begin_pass(
                texture.as_texture_target(),
                Some(ctx.depth_stencil),
                "Higurashi saved game preview",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
            self.render_contents(&mut pass)?;
        }
        postprocessing::apply(&mut ctx, &mut texture, &self.screen);
        postprocessing::apply(&mut ctx, &mut texture, &self.root);
        Ok(texture)
    }

    fn prepare_effects(&self, context: &mut PreRenderContext) -> Result<()> {
        let children = group::ordered_drawables(
            self.layers
                .iter()
                .map(|(slot, owner_id, layer)| group::DrawableChild {
                    owner_id: *owner_id,
                    slot: *slot,
                    layer,
                })
                .collect(),
        );
        let parent = self
            .screen
            .composed_transform(&self.root.composed_transform(&HigurashiTransform::default()));
        let mut lower = Vec::new();
        for child in children {
            let layer = &self
                .layers
                .iter()
                .find(|(slot, _, _)| *slot == child.slot)
                .unwrap()
                .2;
            if let Layer::Effect {
                view,
                properties,
                cache,
                shared_depth,
            } = layer
            {
                let mut cache = cache.lock();
                cache
                    .get_or_insert_with(|| effect_capture::Cache::new(context, view))
                    .prepare(context, view, properties, shared_depth, &lower, &parent)?;
                lower.clear();
            }
            lower.push(child);
        }
        Ok(())
    }
}

struct Job<T> {
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl<T> Job<T> {
    fn new(task: shin_tasks::AsyncTask<Result<T>>) -> Self {
        let retirement = task.retirement_handle().unwrap();
        Self {
            task: Some(task),
            retirement,
        }
    }
    fn poll(&mut self) -> Option<Result<T>> {
        let result = self.task.as_mut()?.poll_naive();
        if result.is_some() {
            self.task = None;
        }
        result
    }
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
struct Hydration {
    index: usize,
    cancelled: bool,
    job: Job<Hydrated>,
}
struct Capture {
    index: Option<usize>,
    job: Job<Option<Scene>>,
    ready: Option<Scene>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(super) struct PreviewService {
    server: Arc<AssetServer>,
    scenario: Arc<HigurashiScenario>,
    canvas: (u32, u32),
    append_present: bool,
    seeds: SeedSource,
    group_seeds: [[u32; 4]; 2],
    cached: [Option<Arc<Hydrated>>; 100],
    rows: Rows,
    dirty: [bool; 100],
    thumbnails: [Option<RenderTexture>; 100],
    hydration: Option<Hydration>,
    capture: Option<Capture>,
    fault: Option<String>,
    globals: globals::Shared,
}
impl PreviewService {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        canvas: (u32, u32),
        append_present: bool,
        setting: u8,
        seeds: SeedSource,
    ) -> Self {
        // b8454 constructs the root and three 6b0c0 groups once. The saved-screen
        // endpoint b90b4 selects the second group at 14c0.
        let all_seeds: [[u32; 4]; 4] = std::array::from_fn(|_| std::array::from_fn(|_| seeds()));
        // 6b150/6b15c replace screen X/Y oscillator seeds with 1/2 after its
        // base constructor consumed four fresh OS values.
        let group_seeds = [all_seeds[0], [1, 2, all_seeds[2][2], all_seeds[2][3]]];
        Self {
            server,
            scenario,
            canvas,
            append_present,
            seeds,
            group_seeds,
            cached: std::array::from_fn(|_| None),
            rows: std::array::from_fn(|_| Row::default()),
            dirty: [true; 100],
            thumbnails: std::array::from_fn(|_| None),
            hydration: None,
            capture: None,
            fault: None,
            globals: globals::State::shared(globals::Snapshot {
                asset_setting: setting,
                route_navigation: true,
                tips: Vec::new(),
            }),
        }
    }
    pub fn with_globals(mut self, snapshot: globals::Snapshot) -> Self {
        self.globals = globals::State::shared(snapshot);
        self
    }
    pub fn synchronize(
        &self,
        saved: &mut shin_core::format::save::higurashi::SaveData,
    ) -> Result<()> {
        self.globals.lock().synchronize(saved)
    }
    /// The bitmap clone stage and frozen disk snapshot share a linearization
    /// point;acquisitions are never gated on thumbnail visibility/publication.
    pub fn with_settings<T>(
        &self,
        saved: &mut shin_core::format::save::higurashi::SaveData,
        operation: impl FnOnce(&mut shin_core::format::save::higurashi::SaveData) -> Result<T>,
    ) -> Result<T> {
        let mut globals = self.globals.lock();
        globals.synchronize(saved)?;
        operation(saved)
    }
    pub fn retire_jobs(&mut self) {
        self.capture.take();
        self.hydration.take();
    }
    /// b2290/b2d58 clear only matching owners/index gates. Capture work lives
    /// until its replacement; do not add a stronger metadata/version guard.
    pub fn invalidate(&mut self, index: usize) {
        assert!(index < 100);
        self.cached[index] = None;
        self.thumbnails[index] = None;
        self.dirty[index] = true;
        if let Some(job) = &mut self.hydration {
            if job.index == index {
                job.cancelled = true;
            }
        }
        if let Some(job) = &mut self.capture {
            if job.index == Some(index) {
                job.index = None;
            }
        }
    }
    pub fn rows(&self) -> &Rows {
        &self.rows
    }
    pub fn thumbnail_sources(&self) -> [Option<TextureSource<'_>>; 100] {
        std::array::from_fn(|i| {
            self.thumbnails[i]
                .as_ref()
                .map(RenderTexture::as_texture_source)
        })
    }
    pub fn poll_rows(&mut self, slots: &Slots, state: &ViewState) -> Result<()> {
        if let Some(fault) = &self.fault {
            bail!("Saved preview failed:{fault}");
        }
        let result = self.poll_rows_inner(slots, state);
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    fn poll_rows_inner(&mut self, slots: &Slots, state: &ViewState) -> Result<()> {
        for (index, slot) in slots.manual.iter().enumerate() {
            if !resources::visible(index, state.scroll) {
                self.thumbnails[index] = None;
                self.rows[index] = Row::default();
                self.dirty[index] = true;
                if let Some(capture) = &mut self.capture {
                    if capture.index == Some(index) {
                        capture.index = None;
                    }
                }
                continue;
            }
            if self.hydration.is_none() && slot.valid && self.cached[index].is_none() {
                let scenario = self.scenario.clone();
                let data = slot.data.clone();
                let task = shin_tasks::async_io::spawn_loader(async move {
                    shin_tasks::compute::spawn(move || Hydrated::replay(&scenario, &data)).await
                });
                self.hydration = Some(Hydration {
                    index,
                    cancelled: false,
                    job: Job::new(task),
                });
            }
            let capture_idle = self
                .capture
                .as_ref()
                .is_none_or(|capture| capture.index.is_none());
            if capture_idle && self.thumbnails[index].is_none() {
                if let Some(saved) = &self.cached[index] {
                    if self.append_present || saved.scenario_id <= 9999 {
                        // d359c retires the old worker before preparing a replacement.
                        self.capture.take();
                        let server = self.server.clone();
                        let scenario = self.scenario.clone();
                        let saved = saved.clone();
                        let seeds = self.seeds.clone();
                        let group_seeds = self.group_seeds;
                        let globals = self.globals.clone();
                        let cancelled = Arc::new(AtomicBool::new(false));
                        let cancellation = cancelled.clone();
                        let task = shin_tasks::async_io::spawn_loader(async move {
                            Scene::load_with_globals(
                                &server,
                                &scenario,
                                &saved,
                                group_seeds,
                                &seeds,
                                globals,
                                &cancellation,
                            )
                            .await
                        });
                        self.capture = Some(Capture {
                            index: Some(index),
                            job: Job::new(task),
                            ready: None,
                            cancelled,
                        });
                    }
                }
            }
            if self.dirty[index] {
                self.rows[index] = Row {
                    date: slot.valid.then_some(slot.data.date),
                    labels: if !slot.valid {
                        Labels::Empty
                    } else if let Some(saved) = &self.cached[index] {
                        Labels::Ready {
                            title: saved.labels[0].clone(),
                            subtitle: saved.labels[1].clone(),
                        }
                    } else {
                        Labels::Loading
                    },
                };
                self.dirty[index] = false;
            }
        }
        // af010 samples completed hydration only AFTER all100 rows. Labels and
        // capture eligibility observe its publication in the following update.
        let hydrated = self.hydration.as_mut().and_then(|job| job.job.poll());
        if let Some(result) = hydrated {
            let job = self.hydration.take().unwrap();
            let saved = result?;
            if !job.cancelled {
                self.cached[job.index] = Some(Arc::new(saved));
                self.dirty[job.index] = true;
            }
        }
        if let Some(capture) = &mut self.capture {
            if capture.index.is_some() {
                if let Some(result) = capture.job.poll() {
                    capture.ready = result?;
                    if capture.ready.is_none() {
                        capture.index = None;
                    }
                }
            }
        }
        Ok(())
    }
    /// Explicit desktop GPU service boundary corresponding to d47a8/b963c.
    /// This publishes an actual rendered scene, never a supplied nodata texture.
    pub fn prepare(&mut self, context: &mut PreRenderContext, state: &ViewState) -> Result<()> {
        let Some(capture) = &mut self.capture else {
            return Ok(());
        };
        let Some(index) = capture.index else {
            return Ok(());
        };
        let Some(scene) = capture.ready.take() else {
            return Ok(());
        };
        let texture = scene.capture(context, self.canvas)?;
        if resources::visible(index, state.scroll) {
            self.thumbnails[index] = Some(texture);
        }
        capture.index = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
