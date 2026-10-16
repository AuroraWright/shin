//! Updated75b28 history handoff and outward row measurement. The retained
//! browser owns both loader tasks and joins them before dropping GPU resources.
use super::{
    browser_controls::{BrowserControls, RowMetadata, Viewport},
    browser_glyphs::{self, BrowserGlyphCache, PositionedRow},
    browser_history::{BrowserHistory, Item, Row},
    browser_lifecycle::{BrowserLifecycle, Services},
    choice::ChoiceHistory,
    progress::ProgressMemory,
    read_history::MessageReadHistory,
};
use crate::{
    adv::{characters::view::CharacterTextures, input_area::PointerFrame},
    asset::{font::GpuFontLazy, system::AssetServer},
};
use anyhow::{Result, ensure};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    layout::{HigurashiBacklogVoices, font::FontMetrics},
    time::Ticks,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Measurement {
    rows: Arc<[Row]>,
    measured: Vec<Option<RowMetadata>>,
    anchor: usize,
    selected: usize,
    viewport: Viewport,
    iteration: usize,
    ends: u8,
    active: bool,
}
impl Measurement {
    fn new(rows: Arc<[Row]>, anchor: usize) -> Result<Self> {
        ensure!(
            !rows.is_empty() && anchor <= rows.len() && rows.len() <= i32::MAX as usize / 2,
            "Invalid browser measurement anchor"
        );
        Ok(Self {
            selected: anchor.min(rows.len() - 1),
            measured: vec![None; rows.len()],
            rows,
            anchor,
            viewport: Viewport {
                first: anchor,
                end: anchor,
                minimum: 0.0,
                extent: 0.0,
            },
            iteration: 0,
            ends: 0,
            active: true,
        })
    }
    /// One native lock/unlock interval. Empty out-of-range attempts are kept:
    /// both ends must be visited before the worker clears its active byte.
    fn step(&mut self, font: impl FontMetrics + Clone) -> Result<()> {
        let offset =
            ((self.iteration + 1) / 2) as i32 * if self.iteration & 1 == 0 { 1 } else { -1 };
        self.iteration += 1;
        let index = self.anchor as i32 + offset;
        if index < 0 {
            self.ends |= 1;
        } else if index as usize >= self.rows.len() {
            self.ends |= 2;
        } else {
            let index = index as usize;
            let row = &self.rows[index];
            let layout = browser_glyphs::layout(row, font, HigurashiBacklogVoices::Count(0))?;
            let mut height = layout.size.y;
            if matches!(&row.item, Item::Message { argument: Some(n), .. } if *n >= 0)
                && height < 76.0
            {
                height = 76.0;
            }
            height += 36.0;
            let top = if offset < 0 {
                self.viewport.minimum - height
            } else {
                self.viewport.extent
            };
            let positioned = PositionedRow {
                row: row.clone(),
                top,
                height,
                voice_count: layout.voice_count,
            };
            self.measured[index] = Some(RowMetadata::from(&positioned));
            if offset < 0 {
                self.viewport.minimum = top;
                self.viewport.first -= 1;
            } else {
                self.viewport.extent += height;
                self.viewport.end += 1;
            }
        }
        Ok(())
    }
}

pub struct Request {
    pub scenario: Arc<HigurashiScenario>,
    pub source: ProgressMemory,
    pub read_history: MessageReadHistory,
    pub choices: ChoiceHistory,
    pub read_only: bool,
    pub command_limit: usize,
}
pub struct PreparedHistory {
    pub rows: Arc<[Row]>,
    pub markers: Vec<u32>,
    pub current_row: usize,
    pub jump_source: ProgressMemory,
}
#[derive(Default)]
struct Shared {
    history: Option<Arc<PreparedHistory>>,
    measurement: Option<Measurement>,
    font: Option<Arc<GpuFontLazy>>,
}
pub struct Snapshot {
    pub history: Arc<PreparedHistory>,
    pub measured: Vec<Option<RowMetadata>>,
    pub viewport: Viewport,
    pub worker_active: bool,
}
impl Snapshot {
    fn metadata(&self) -> Vec<RowMetadata> {
        self.history
            .rows
            .iter()
            .zip(&self.measured)
            .map(|(row, measured)| {
                measured.unwrap_or(RowMetadata {
                    kind: match row.item {
                        Item::Message { .. } => 0,
                        Item::Choice { .. } => 1,
                        Item::Checkpoint { .. } => 2,
                        Item::CharacterChoice { .. } => 3,
                    },
                    voice_count: 0,
                    top: 0.0,
                    height: 1.0,
                })
            })
            .collect()
    }
    fn positioned(&self, scroll: f32) -> Vec<PositionedRow> {
        self.history
            .rows
            .iter()
            .zip(&self.measured)
            .filter_map(|(row, m)| {
                let m = m.filter(|m| {
                    let relative = m.top - (scroll - 162.0);
                    relative < 1080.0 && relative + m.height > 0.0
                });
                m.map(|m| PositionedRow {
                    row: row.clone(),
                    top: m.top,
                    height: m.height,
                    voice_count: m.voice_count,
                })
            })
            .collect()
    }
}
pub struct Worker {
    assets: Arc<AssetServer>,
    shared: Arc<Mutex<Shared>>,
    cancelled: Arc<AtomicBool>,
    history_task: Option<shin_tasks::AsyncTask<Result<()>>>,
    atlas_task: Option<shin_tasks::AsyncTask<Result<Arc<CharacterTextures>>>>,
    atlas: Option<Arc<CharacterTextures>>,
    fault: Option<String>,
    retirements: Vec<shin_tasks::LoaderRetirement>,
}
impl Worker {
    pub fn start(assets: Arc<AssetServer>, request: Request) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let state = shared.clone();
        let cancel = cancelled.clone();
        let server = assets.clone();
        let history_task = shin_tasks::async_io::spawn_loader(async move {
            let font: Arc<GpuFontLazy> = server.load("/newrodin.fnt").await?;
            shin_tasks::compute::spawn(move || {
                let history = BrowserHistory::collect_cancellable(
                    &request.scenario,
                    &request.source,
                    &request.read_history,
                    &request.choices,
                    request.read_only,
                    request.command_limit,
                    || cancel.load(Ordering::Acquire),
                )?;
                if cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                ensure!(
                    !history.rows.is_empty(),
                    "Browser history has no eligible rows"
                );
                let rows: Arc<[Row]> = history.rows.into();
                let measurement =
                    Measurement::new(rows.clone(), history.measurement_anchor as usize)?;
                let prepared = Arc::new(PreparedHistory {
                    rows,
                    markers: history.markers,
                    current_row: measurement.selected,
                    jump_source: history.jump_source,
                });
                {
                    let mut state = state.lock().unwrap();
                    state.history = Some(prepared);
                    state.font = Some(font.clone());
                    state.measurement = Some(measurement);
                }
                loop {
                    let mut guard = state.lock().unwrap();
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let measured = guard.measurement.as_mut().unwrap();
                    if !measured.active {
                        break;
                    }
                    measured.step(font.clone())?;
                    let finished = measured.ends == 3;
                    drop(guard);
                    if finished {
                        state.lock().unwrap().measurement.as_mut().unwrap().active = false;
                        break;
                    }
                }
                Ok(())
            })
            .await
        });
        let server = assets.clone();
        let atlas_task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load("/logview.txa").await?;
            for name in [
                "chapter",
                "cursor",
                "voice",
                "line",
                "obi",
                "caption",
                "close",
                "help_logview",
                "key",
            ] {
                ensure!(atlas.contains(name), "Browser atlas has no {name}");
            }
            Ok(atlas)
        });
        let retirements = vec![
            history_task.retirement_handle().unwrap(),
            atlas_task.retirement_handle().unwrap(),
        ];
        Self {
            assets,
            shared,
            cancelled,
            history_task: Some(history_task),
            atlas_task: Some(atlas_task),
            atlas: None,
            fault: None,
            retirements,
        }
    }
    pub fn atlas(&self) -> Option<&Arc<CharacterTextures>> {
        self.atlas.as_ref()
    }
    pub fn poll(&mut self) -> Result<Option<Snapshot>> {
        if let Some(result) = self
            .history_task
            .as_mut()
            .and_then(|task| task.poll_naive())
        {
            self.history_task = None;
            if let Err(error) = result {
                self.fault = Some(error.to_string());
            }
        }
        if let Some(result) = self.atlas_task.as_mut().and_then(|task| task.poll_naive()) {
            self.atlas_task = None;
            match result {
                Ok(atlas) => self.atlas = Some(atlas),
                Err(error) => self.fault = Some(error.to_string()),
            }
        }
        ensure!(
            self.fault.is_none(),
            "Browser worker failed: {}",
            self.fault.as_deref().unwrap_or("")
        );
        let shared = self.shared.lock().unwrap();
        Ok(shared.measurement.as_ref().map(|measured| Snapshot {
            history: shared.history.as_ref().unwrap().clone(),
            measured: measured.measured.clone(),
            viewport: measured.viewport,
            worker_active: measured.active,
        }))
    }
    fn retire(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(task) = self.history_task.take() {
            retire(task);
        }
        if let Some(task) = self.atlas_task.take() {
            retire(task);
        }
        #[cfg(not(target_arch = "wasm32"))]
        for handle in self.retirements.drain(..) {
            handle.retire(());
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.retire();
    }
}
fn retire<T>(task: shin_tasks::AsyncTask<T>) {
    #[cfg(not(target_arch = "wasm32"))]
    task.cancel_and_retire();
    #[cfg(target_arch = "wasm32")]
    drop(task);
}

/// Parent owned resources. Worker retirement precedes retained child/view/cache
/// destruction, matching original7612c. No worker holds a pointer to this owner.
pub struct Browser {
    worker: Worker,
    lifecycle: Option<BrowserLifecycle>,
    cache: BrowserGlyphCache,
    snapshot: Option<Snapshot>,
}
impl Browser {
    pub fn start(assets: Arc<AssetServer>, request: Request) -> Self {
        Self {
            worker: Worker::start(assets, request),
            lifecycle: None,
            cache: BrowserGlyphCache::default(),
            snapshot: None,
        }
    }
    pub(crate) fn history_rows(&self) -> Option<Arc<[Row]>> {
        self.snapshot.as_ref().map(|s| s.history.rows.clone())
    }
    #[cfg(test)]
    pub(crate) fn reference_clear_glyph_cache(&mut self) { self.cache = Default::default(); }
    #[cfg(test)]
    pub(crate) fn reference_worker_lifetime(&self) -> (Arc<AtomicBool>, Vec<shin_tasks::LoaderRetirement>) {
        (self.worker.cancelled.clone(), self.worker.retirements.clone())
    }
    pub fn lifecycle(&self) -> Option<&BrowserLifecycle> {
        self.lifecycle.as_ref()
    }
    pub fn lifecycle_mut(&mut self) -> Option<&mut BrowserLifecycle> {
        self.lifecycle.as_mut()
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        duck_music: bool,
        services: &mut impl Services,
    ) -> Result<()> {
        self.poll_with_mouse_release(
            delta, permission, pressed, repeated, frame, duck_music, services, None,
        )
    }
    pub(crate) fn poll_with_mouse_release(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        duck_music: bool,
        services: &mut impl Services,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<()> {
        if let Some(snapshot) = self.worker.poll()? {
            if snapshot.measured[snapshot.history.current_row].is_some() {
                if self.lifecycle.is_none() {
                    let history = &snapshot.history;
                    let controls = BrowserControls::new(
                        snapshot.metadata(),
                        snapshot.viewport,
                        history.current_row,
                        0.0,
                    )?;
                    let mut lifecycle = BrowserLifecycle::new(
                        controls,
                        history.rows.iter().map(|r| r.boundary).collect(),
                        history.jump_source.clone(),
                    )?;
                    lifecycle.configure_navigation_rendering(
                        history.rows.clone(),
                        history.markers.clone(),
                        history.current_row,
                    )?;
                    self.lifecycle = Some(lifecycle);
                } else {
                    self.lifecycle
                        .as_mut()
                        .unwrap()
                        .controls_mut()
                        .publish_measurement(snapshot.metadata(), snapshot.viewport)?;
                }
                self.lifecycle.as_mut().unwrap().publish_readiness(
                    if self.worker.atlas.is_some() { 2 } else { 0 },
                    snapshot.worker_active,
                );
            }
            self.snapshot = Some(snapshot);
        }
        if let Some(lifecycle) = &mut self.lifecycle {
            lifecycle.poll_with_mouse_release(
                delta, permission, pressed, repeated, frame, duck_music, services, mouse_release,
            )?;
        }
        Ok(())
    }
    pub fn prepare(&mut self, outline_mode: u8) -> Result<()> {
        if let (Some(snapshot), Some(lifecycle)) = (&self.snapshot, &mut self.lifecycle) {
            let font = self
                .worker
                .shared
                .lock()
                .unwrap()
                .font
                .as_ref()
                .unwrap()
                .clone();
            let context = self.worker.assets.context();
            self.cache.synchronize(
                &context.wgpu_device,
                &context.wgpu_queue,
                font,
                &snapshot.positioned(lifecycle.controls().scroll()),
                lifecycle.controls().scroll(),
                outline_mode,
            )?;
            lifecycle
                .controls_mut()
                .set_cached_rows(self.cache.indices().map(|i| i as usize));
            lifecycle.prepare_navigation_views(&self.worker.assets)?;
        }
        Ok(())
    }
    /// Question render targets are prepared before the parent render pass.
    pub fn prepare_questions(&mut self, context: &mut crate::render::PreRenderContext) {
        if let Some(lifecycle) = &mut self.lifecycle {
            lifecycle.prepare_questions(context);
        }
    }
    /// Original77254 draws content, input-stack navigation, shell, then the
    /// parent task-stack questions. The original snapshot/retirement rules of
    /// both child stacks remain in their lifecycle owner.
    pub fn render(
        &mut self,
        pass: &mut shin_render::render_pass::RenderPass,
        system: &CharacterTextures,
    ) {
        self.render_content(pass, system);
        if let Some(lifecycle) = &mut self.lifecycle {
            lifecycle.render_navigation_views(pass);
            if let Some(atlas) = self.worker.atlas() {
                super::browser_shell::render(
                    pass,
                    atlas,
                    super::browser_shell::State::from_controls(
                        lifecycle.slide(),
                        lifecycle.controls(),
                    ),
                );
            }
            lifecycle.render_questions(pass);
        }
    }
    /// Retained native row order, text, voice feedback and shared boundaries.
    pub fn render_row_feedback(&self, pass: &mut shin_render::render_pass::RenderPass) {
        if let (Some(snapshot), Some(lifecycle), Some(atlas)) =
            (&self.snapshot, &self.lifecycle, self.worker.atlas())
        {
            let controls = lifecycle.controls();
            super::browser_content::render_rows(
                pass,
                atlas,
                &self.cache,
                &snapshot.positioned(controls.scroll()),
                controls.scroll(),
                lifecycle.slide(),
                controls.selected(),
                |index| {
                    controls
                        .flashes(index as usize)
                        .map_or((0.0, 0.0), |f| (f.jump.value(), f.voice.value()))
                },
            );
        }
    }
    pub fn render_content(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        system: &CharacterTextures,
    ) {
        if let (Some(snapshot), Some(lifecycle), Some(atlas)) =
            (&self.snapshot, &self.lifecycle, self.worker.atlas())
        {
            let controls = lifecycle.controls();
            super::browser_content::render(
                pass,
                atlas,
                system,
                &self.cache,
                &snapshot.positioned(controls.scroll()),
                controls.scroll(),
                lifecycle.slide(),
                controls.selected(),
                |index| {
                    controls
                        .flashes(index as usize)
                        .map_or((0.0, 0.0), |f| (f.jump.value(), f.voice.value()))
                },
                controls.scrollbar(),
            );
        }
    }
    pub fn render_scrollbar(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        system: &CharacterTextures,
    ) {
        if let Some(lifecycle) = &self.lifecycle {
            if self.worker.atlas().is_some() {
                super::browser_decorations::render_scrollbar(
                    pass,
                    system,
                    lifecycle.controls().scrollbar(),
                    lifecycle.slide(),
                );
            }
        }
    }
    pub fn render_text(&self, pass: &mut shin_render::render_pass::RenderPass) {
        if let (Some(snapshot), Some(lifecycle)) = (&self.snapshot, &self.lifecycle) {
            self.cache.render(
                pass,
                &snapshot.positioned(lifecycle.controls().scroll()),
                lifecycle.controls().scroll(),
                lifecycle.slide(),
            );
        }
    }
}

#[cfg(test)]
mod tests;
