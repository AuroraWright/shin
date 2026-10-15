//! Updated b378c publishes only /saveload.txa. NewRodin and the scrollbar's
//! /systex.txa are shared engine resources, supplied by the owning scene.
use crate::{adv::character_view::CharacterTextures, asset::system::AssetServer};
use anyhow::{Result, ensure};
use shin_core::format::save::higurashi::SaveDate;
use std::sync::Arc;

/// The hydrated VM retains its two saved SAVEINFO strings. Those are already
/// Unicode in the Rust VM; native c4990 decodes its legacy Shift-JIS pointers
/// before invoking the same SimpleText script parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Labels {
    Empty,
    Loading,
    Ready { title: Arc<str>, subtitle: Arc<str> },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Some also represents a valid slot whose packed calendar is all zeroes.
    /// Native SaveMenu displays the calendar components without validation.
    pub date: Option<SaveDate>,
    pub labels: Labels,
}
impl Default for Row {
    fn default() -> Self {
        Self {
            date: None,
            labels: Labels::Empty,
        }
    }
}
pub type Rows = [Row; 100];

/// Original visibility is inclusive at the lower edge (y + 192 == 0).
/// Header/footer artwork covers rows outside the inner list viewport.
pub fn visible(index: usize, scroll: f32) -> bool {
    let y = (12 + index * 206) as f32 + 4.0 - (scroll - 162.0);
    y < 1080.0 && y + 192.0 >= 0.0
}

/// aeba4 drops offscreen thumbnails and SimpleText caches, while keeping the
/// hydrated saved VM. The owning preview service keeps VM ownership separately
/// and calls this after sampling the model's current scroll value.
pub fn retire_invisible_thumbnails<T>(thumbnails: &mut [Option<T>; 100], scroll: f32) {
    for (index, thumbnail) in thumbnails.iter_mut().enumerate() {
        if !visible(index, scroll) {
            thumbnail.take();
        }
    }
}

/// adb3c multiplies these native logical dimensions by the physical canvas
/// scale and rounds up. Thumbnail geometry remains 342 x 192 logical pixels.
pub fn thumbnail_size(canvas: (u32, u32)) -> (u32, u32) {
    (
        (canvas.0 as f32 / 1920.0 * 342.0).ceil() as u32,
        (canvas.1 as f32 / 1080.0 * 192.0).ceil() as u32,
    )
}

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Arc<CharacterTextures>>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
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
pub struct Loader {
    pub atlas: Option<Arc<CharacterTextures>>,
    job: Option<Job>,
    fault: Option<String>,
}
impl Loader {
    pub fn new(server: Arc<AssetServer>) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            server.load::<CharacterTextures, _>("/saveload.txa").await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            atlas: None,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            fault: None,
        }
    }
    pub fn poll(&mut self) -> Result<bool> {
        if let Some(fault) = &self.fault {
            anyhow::bail!("Higurashi Save/Load artwork failed: {fault}");
        }
        if let Some(job) = &mut self.job {
            if let Some(task) = &mut job.task {
                if let Some(result) = task.poll_naive() {
                    job.task = None;
                    match result {
                        Ok(atlas) => self.atlas = Some(atlas),
                        Err(error) => {
                            self.fault = Some(format!("{error:#}"));
                            return Err(error);
                        }
                    }
                }
            }
        }
        Ok(self.atlas.is_some())
    }
    pub fn retire_first(&mut self) -> Result<()> {
        ensure!(
            self.atlas.is_some(),
            "Save/Load first task retired before artwork publication"
        );
        self.job.take();
        Ok(())
    }
}
