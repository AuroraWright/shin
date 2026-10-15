//! Concrete VOID callbacks for OTSUGET/KAKERAGET; no checkpoint/date services.
use super::*;
use crate::{
    adv::higurashi::{
        layers::{HigurashiLayers, Target},
        notification_command::{Request, Services as NotificationServices},
        temporary_wrapper::Completion,
        touch::Overlay as InputOverlay,
    },
    layer::user::higurashi::HigurashiLayerKind,
};

#[derive(Default)]
pub(super) struct NotificationGet {
    pub job: Option<super::temporary_job::Job>,
}
struct Services<'a> {
    owner: &'a mut NotificationGet,
    saved: &'a mut SaveData,
    overlay: &'a mut Option<Box<dyn InputOverlay>>,
    common: &'a CommonAssets,
    server: &'a Arc<AssetServer>,
    seeds: &'a Seeds,
    setting: u8,
}
impl NotificationServices for Services<'_> {
    fn prepare(
        &mut self,
        request: &Request,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool> {
        let kind = HigurashiLayerKind::from_native(request.kind())
            .context("Invalid notification layer")?;
        let job = self.owner.job.get_or_insert_with(|| {
            super::temporary_job::Job::new(
                self.server.clone(),
                self.common.scenario.clone(),
                kind,
                request.producer_params(),
                self.setting,
            )
        });
        job.publish(
            target,
            layers,
            self.saved,
            self.overlay,
            self.common,
            self.server,
            self.seeds,
        )
    }
    fn poll(&mut self) -> Result<bool> {
        match self
            .owner
            .job
            .as_ref()
            .map_or(Ok(None), |job| job.completion())?
        {
            Some(Completion::Notification) => Ok(true),
            None => Ok(false),
            Some(Completion::Tips(_)) => bail!("VOID notification received a TIPS result"),
        }
    }
}
impl NotificationGet {
    pub fn poll(
        &mut self,
        runner: &mut HigurashiRunner,
        saved: &mut SaveData,
        overlay: &mut Option<Box<dyn InputOverlay>>,
        common: &CommonAssets,
        server: &Arc<AssetServer>,
        seeds: &Seeds,
    ) -> Result<bool> {
        let mut services = Services {
            owner: self,
            saved,
            overlay,
            common,
            server,
            seeds,
            setting: runner.layer_setting(),
        };
        let complete = runner
            .poll_notification_command(&mut services)?
            .unwrap_or(false);
        if complete {
            if let Some(job) = &mut self.job {
                job.retire_ui(runner.scene_layers_mut()?, overlay);
            }
            runner.complete_external(shin_core::vm::command::CommandResult::None)?;
            self.job = None;
        }
        Ok(complete)
    }
}
