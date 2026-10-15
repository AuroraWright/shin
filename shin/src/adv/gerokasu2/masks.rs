//! Captured-plane MASKLOAD/MASKUNLOAD owners and actual MSK asset acquisition.
use super::layers::Gerokasu2Layers;
use crate::asset::{mask::MaskTexture, system::AssetServer};
use anyhow::{Context, Result, bail, ensure};
use futures::FutureExt;
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario,
    vm::{
        command::{RuntimeCommand, types::PlaneId},
        gerokasu2::Gerokasu2RuntimeCommand as Cmd,
    },
};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaskOperation {
    Load {
        mask_id: i32,
        flags: i32,
        animation_permission_arg: i32,
    },
    Unload,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskPlan {
    pub plane: PlaneId,
    pub operation: MaskOperation,
}
impl MaskPlan {
    /// Native constructors4244/43C0 capture the selected plane. Commit this
    /// caller-owned integer copy once the retained live owner is installed.
    pub fn prepare(layers: &mut Gerokasu2Layers, command: &Cmd) -> Result<Self> {
        let plane = layers.current_plane();
        let operation = match command {
            Cmd::MaskLoad {
                mask_id,
                flags,
                animation_permission_arg,
            } => {
                layers.set_mask(*mask_id, *flags);
                MaskOperation::Load {
                    mask_id: *mask_id,
                    flags: *flags,
                    animation_permission_arg: *animation_permission_arg,
                }
            }
            Cmd::Shared(RuntimeCommand::MASKUNLOAD(_)) => {
                layers.clear_mask();
                MaskOperation::Unload
            }
            _ => bail!("Expected Gerokasu2 MASKLOAD/MASKUNLOAD"),
        };
        Ok(Self { plane, operation })
    }
}

/// The backend owns the real resource job and retained scene graph. A missing
/// metadata ID is a normal absent resource; I/O/decode failures remain errors.
pub trait MaskServices {
    fn start_mask_resource(&mut self, mask_id: i32) -> Result<bool>;
    fn mask_resource_ready(&mut self) -> Result<bool>;
    fn capture_mask_source(&mut self) -> Result<()>;
    fn publish_mask(&mut self, plane: PlaneId, flags: i32) -> Result<()>;
    /// Preserve stored/render flags while removing only this plane's texture.
    fn clear_mask(&mut self, plane: PlaneId) -> Result<()>;
    fn set_mask_animations_allowed(&mut self, allowed: bool);
    fn mask_command_finished(&mut self) -> Result<()> {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskStage {
    Start,
    Publication,
    Complete,
}
pub struct MaskController {
    plan: MaskPlan,
    stage: MaskStage,
    resource_present: bool,
    captured: bool,
    published: bool,
}
impl MaskController {
    pub fn new(plan: MaskPlan) -> Self {
        Self {
            plan,
            stage: MaskStage::Start,
            resource_present: false,
            captured: false,
            published: false,
        }
    }
    pub fn plan(&self) -> &MaskPlan {
        &self.plan
    }
    pub fn stage(&self) -> MaskStage {
        self.stage
    }
    /// OriginalC7E0/D370: no optional5000us wait and no skip shortcut. Source
    /// capture precedes the ready check, including missing metadata/unload.
    pub fn poll(&mut self, services: &mut dyn MaskServices) -> Result<bool> {
        if self.stage == MaskStage::Complete {
            return Ok(true);
        }
        if self.stage == MaskStage::Start {
            if let MaskOperation::Load {
                mask_id,
                animation_permission_arg,
                ..
            } = self.plan.operation
            {
                self.resource_present = services.start_mask_resource(mask_id)?;
                if self.resource_present && animation_permission_arg & 1 == 0 {
                    services.set_mask_animations_allowed(false);
                }
            }
            self.stage = MaskStage::Publication;
        }
        if !self.captured {
            services.capture_mask_source()?;
            self.captured = true;
        }
        if !self.published {
            match self.plan.operation {
                MaskOperation::Load { flags, .. } if self.resource_present => {
                    match services.mask_resource_ready() {
                        Ok(false) => return Ok(false),
                        Ok(true) => services.publish_mask(self.plan.plane, flags)?,
                        Err(error) => {
                            // Desktop recovery is explicit; never publish a
                            // placeholder for a failed real asset preparation.
                            services.set_mask_animations_allowed(true);
                            return Err(error);
                        }
                    }
                }
                _ => services.clear_mask(self.plan.plane)?,
            }
            self.published = true;
        }
        if matches!(self.plan.operation, MaskOperation::Load { .. }) {
            services.set_mask_animations_allowed(true);
        }
        services.mask_command_finished()?;
        self.stage = MaskStage::Complete;
        Ok(true)
    }
}

/// Concrete backend acquisition: Ger metadata resolves to the shared MSK4
/// loader and a real Arc<MaskTexture>. Keep the prepared value until publish.
#[derive(Default)]
pub struct AssetMaskLoader {
    task: Option<shin_tasks::AsyncTask<Result<Arc<MaskTexture>>>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
    texture: Option<Arc<MaskTexture>>,
    source: Option<(Arc<AssetServer>, String)>,
    error: Option<String>,
}
impl AssetMaskLoader {
    pub fn has_job(&self) -> bool {
        self.task.is_some()
    }
    pub fn path(&self) -> Option<&str> {
        self.source.as_ref().map(|(_, path)| path.as_str())
    }
    pub fn start(
        &mut self,
        assets: Arc<AssetServer>,
        scenario: &Gerokasu2Scenario,
        mask_id: i32,
    ) -> Result<bool> {
        ensure!(
            self.task.is_none(),
            "Ger mask asset job is already retained"
        );
        self.retire();
        self.source = usize::try_from(mask_id)
            .ok()
            .and_then(|id| scenario.mask_info.get(id))
            .map(|info| (assets, format!("/mask/{}.msk", info.name.as_str())));
        if self.source.is_none() {
            return Ok(false);
        }
        self.spawn();
        Ok(true)
    }
    fn spawn(&mut self) {
        let (assets, path) = self.source.as_ref().unwrap().clone();
        self.task = Some(shin_tasks::async_io::spawn_loader(async move {
            std::panic::AssertUnwindSafe(assets.load::<MaskTexture, _>(&path))
                .catch_unwind()
                .await
                .map_err(|panic| {
                    let message = panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("unknown panic");
                    anyhow::anyhow!("Ger mask preparation failed for {path}: {message}")
                })?
                .with_context(|| format!("Loading Ger mask {path}"))
        }));
    }
    pub fn ready(&mut self) -> Result<bool> {
        if let Some(error) = &self.error {
            bail!("Retained Ger mask resource failed: {error}");
        }
        if let Some(task) = &mut self.task {
            if let Some(result) = task.poll_naive() {
                self.retirement = task.retirement_handle();
                self.task = None;
                match result {
                    Ok(texture) => self.texture = Some(texture),
                    Err(error) => {
                        self.error = Some(format!("{error:#}"));
                        return Err(error);
                    }
                }
            }
        }
        Ok(self.texture.is_some())
    }
    pub fn texture(&self) -> Result<Arc<MaskTexture>> {
        self.texture
            .clone()
            .context("Ger mask resource is not ready for publication")
    }
    /// Retry the same retained path after a surfaced I/O/decode failure. Native
    /// platform exception recovery is outside the command control-flow oracle.
    pub fn retry(&mut self) -> Result<()> {
        ensure!(
            self.source.is_some() && self.error.is_some() && self.task.is_none(),
            "Ger mask has no failed retained resource to retry"
        );
        let source = self.source.take().unwrap();
        self.retire();
        self.source = Some(source);
        self.spawn();
        Ok(())
    }
    /// Retire from the scene caller, outside an executor/decoder worker.
    pub fn retire(&mut self) {
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        let texture = self.texture.take();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(retirement) = self.retirement.take() {
            retirement.retire(texture);
        } else {
            drop(texture);
        }
        #[cfg(target_arch = "wasm32")]
        {
            drop(texture);
            self.retirement = None;
        }
        self.source = None;
        self.error = None;
    }
    pub fn finish(&mut self) {
        self.retire();
    }
}
#[cfg(test)]
mod tests;
