//! SaveMenu's private replay/resource channels. Nothing here publishes a VM,
//! message, gallery event, or audible player to the active ADV.
use super::view;
use crate::{
    adv::gerokasu2::{
        root::Gerokasu2Root,
        save_menu::preview::{Completions, Controller, Event, Labels, Request},
        save_preview_replay::{self, Snapshot},
    },
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::Result;
use shin_core::format::{save::gerokasu2::SaveData, scenario::gerokasu2::Gerokasu2Scenario};
use shin_render::render_texture::RenderTexture;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

mod capture;
#[cfg(test)]
mod tests;

struct Job<T> {
    request: Request,
    task: Option<shin_tasks::AsyncTask<Result<T>>>,
    cancelled: Arc<AtomicBool>,
}
impl<T> Job<T> {
    fn poll(&mut self) -> Option<Result<T>> {
        let result = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        Some(result)
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}

struct TextureJob {
    worker: Job<Gerokasu2Root>,
    scene: Option<Result<Gerokasu2Root>>,
    output: Option<Arc<RenderTexture>>,
}

pub(super) struct Previews {
    controller: Controller,
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    font: Arc<GpuGerokasu2Font>,
    snapshots: [Option<Snapshot>; 100],
    pub(super) textures: [Option<Arc<RenderTexture>>; 100],
    pub(super) labels: [Option<view::Labels>; 100],
    replay: Option<Job<Snapshot>>,
    texture: Option<TextureJob>,
    capture: capture::Capture,
    #[cfg(test)]
    replay_starts: [usize; 100],
    #[cfg(test)]
    texture_starts: [usize; 100],
}

impl Previews {
    pub(super) fn new(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        font: Arc<GpuGerokasu2Font>,
    ) -> Self {
        Self {
            controller: Controller::default(),
            capture: capture::Capture::new(&assets.context().wgpu_device),
            assets,
            scenario,
            font,
            snapshots: std::array::from_fn(|_| None),
            textures: std::array::from_fn(|_| None),
            labels: std::array::from_fn(|_| None),
            replay: None,
            texture: None,
            #[cfg(test)]
            replay_starts: [0; 100],
            #[cfg(test)]
            texture_starts: [0; 100],
        }
    }

    pub(super) fn invalidate(&mut self, slot: usize) {
        self.snapshots[slot] = None;
        self.textures[slot] = None;
        self.labels[slot] = None;
        // Replay invalidation is discard-only. Texture abandonment retains the
        // service job until replacement/menu destruction, as native D68B0 does.
        self.controller.invalidate(slot);
    }

    pub(super) fn update(&mut self, scroll: f32, data: &SaveData) {
        let mut replay = None;
        if let Some(job) = self.replay.as_mut()
            && let Some(result) = job.poll()
        {
            let request = job.request;
            match result {
                Ok(snapshot) => replay = Some((request, snapshot)),
                Err(error) => {
                    tracing::error!(%error, slot=request.slot, "Ger saved preview replay failed; retaining fallback until the slot changes");
                    self.controller.replay_failed(request);
                }
            }
            self.replay = None;
        }
        if let Some(job) = self.texture.as_mut()
            && let Some(result) = job.worker.poll()
        {
            job.scene = Some(result);
        }
        let texture = self.texture.as_ref().and_then(|job| {
            job.output
                .as_ref()
                .map(|texture| (job.worker.request, texture.clone()))
        });
        let events = self.controller.frame(
            scroll,
            &std::array::from_fn(|i| data.manual_save_slots[i].is_some()),
            Completions {
                replay: replay
                    .as_ref()
                    .map(|(request, snapshot)| (*request, snapshot.labels[0].is_empty())),
                texture: texture.as_ref().map(|(request, _)| *request),
            },
        );
        for event in events {
            match event {
                Event::StartReplay(request) => self.start_replay(request, data),
                Event::StartTexture(request) => self.start_texture(request),
                Event::DropTexture { slot } => self.textures[slot] = None,
                Event::ClearLabels { slot } => self.labels[slot] = None,
                Event::SetLabels { slot, labels } => self.set_labels(slot, labels),
                Event::ReplayPublished(request) => {
                    let (completed, snapshot) = replay.take().expect("completed replay payload");
                    assert_eq!(completed, request);
                    self.snapshots[request.slot] = Some(snapshot);
                }
                Event::ReplayDiscarded(_) => {
                    replay = None;
                }
                Event::TexturePublished(request) => {
                    let (completed, texture) = texture.as_ref().expect("completed texture payload");
                    assert_eq!(*completed, request);
                    self.textures[request.slot] = Some(texture.clone());
                }
                Event::AbandonTexture(_) => {}
            }
        }
    }

    fn start_replay(&mut self, request: Request, data: &SaveData) {
        let source = data.manual_save_slots[request.slot]
            .as_ref()
            .expect("native replay start requires an occupied slot")
            .clone();
        let scenario = self.scenario.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            shin_tasks::compute::spawn(move || {
                save_preview_replay::collect(scenario, source, || {
                    worker_cancelled.load(Ordering::Acquire)
                })
            })
            .await
        });
        self.replay = Some(Job {
            request,
            task: Some(task),
            cancelled,
        });
        #[cfg(test)]
        {
            self.replay_starts[request.slot] += 1;
        }
    }

    fn start_texture(&mut self, request: Request) {
        // Drop joins registered child work before the next private graph can
        // begin construction. No old service future can publish into this job.
        self.texture = None;
        let layers = self.snapshots[request.slot]
            .as_ref()
            .expect("native texture start requires a published snapshot")
            .layers
            .clone();
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            crate::adv::gerokasu2::save_preview_scene::prepare(
                assets,
                scenario,
                layers,
                worker_cancelled,
            )
            .await
        });
        self.texture = Some(TextureJob {
            worker: Job {
                request,
                task: Some(task),
                cancelled,
            },
            scene: None,
            output: None,
        });
        #[cfg(test)]
        {
            self.texture_starts[request.slot] += 1;
        }
    }

    fn set_labels(&mut self, slot: usize, labels: Labels) {
        if let Labels::Saved { first_is_empty } = labels {
            let saved = &self.snapshots[slot]
                .as_ref()
                .expect("native saved labels require a published snapshot")
                .labels;
            let result = if first_is_empty {
                view::Labels::prepare(&self.assets, &self.font, &saved[1], None)
            } else {
                view::Labels::prepare(&self.assets, &self.font, &saved[0], Some(&saved[1]))
            };
            match result {
                Ok(labels) => self.labels[slot] = Some(labels),
                Err(error) => {
                    tracing::error!(%error, slot, "Ger save labels could not be prepared")
                }
            }
        } else {
            self.labels[slot] = None;
        }
    }

    pub(super) fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        let Some(job) = self.texture.as_mut() else {
            return Ok(());
        };
        if self.controller.texture_request() != Some(job.worker.request) || job.output.is_some() {
            return Ok(());
        }
        let Some(result) = job.scene.take() else {
            return Ok(());
        };
        let mut scene = match result {
            Ok(scene) => Some(scene),
            Err(error) => {
                tracing::error!(%error,slot=job.worker.request.slot,"Ger preview resource worker failed; publishing the native black fallback");
                None
            }
        };
        job.output = Some(Arc::new(self.capture.capture(
            scene.as_mut(),
            &self.assets,
            context,
        )?));
        Ok(())
    }
}
