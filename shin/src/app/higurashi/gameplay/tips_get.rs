//! Concrete c5ac TIPSGET adapter using full live settings and queued scene data.
use super::super::{runtime_game_data::RuntimeGameData, tips_menu::factory::Request};
use super::*;
use crate::{
    adv::higurashi::{
        autosave::Execution,
        layers::{HigurashiLayers, Target},
        temporary_wrapper::Completion,
        tips_get::{TipsGetRequest, TipsGetServices, TipsSceneRequest},
        touch::Overlay as InputOverlay,
    },
    layer::user::higurashi::HigurashiLayerKind,
};

#[derive(Default)]
pub(super) struct TipsGet {
    pub job: Option<super::temporary_job::Job>,
}
struct Services<'a> {
    owner: &'a mut TipsGet,
    saved: &'a mut SaveData,
    overlay: &'a mut Option<Box<dyn InputOverlay>>,
    common: &'a CommonAssets,
    server: &'a Arc<AssetServer>,
    seeds: &'a Seeds,
    setting: u8,
    date: shin_core::format::save::higurashi::SaveDate,
    scene_request: &'a mut Option<Request>,
    clock: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    copied: bool,
}
impl TipsGetServices for Services<'_> {
    fn prepare(
        &mut self,
        request: &TipsGetRequest,
        alternate: bool,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool> {
        let job = self.owner.job.get_or_insert_with(|| {
            super::temporary_job::Job::new(
                self.server.clone(),
                self.common.scenario.clone(),
                HigurashiLayerKind::Tips,
                request.producer_params(alternate),
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
    fn poll(&mut self) -> Result<Option<i32>> {
        match self
            .owner
            .job
            .as_ref()
            .map_or(Ok(None), |job| job.completion())?
        {
            Some(Completion::Tips(result)) => Ok(Some(result)),
            None => Ok(None),
            Some(Completion::Notification) => {
                bail!("TIPSGET received a void notification callback")
            }
        }
    }
    fn packed_time(&mut self) -> Result<u64> {
        let date = (self.clock)()?;
        self.copied = true;
        Ok(bindings::packed_date(date))
    }
    fn opening_copied(
        &mut self,
        source: &crate::adv::higurashi::progress::ProgressMemory,
    ) -> Result<()> {
        if source.progress().active_entry().scenario_id <= 9999 {
            for (index, tip) in self.common.scenario.tips_info.iter().enumerate() {
                let scenario = tip
                    .updated_field
                    .context("Updated TIPS descriptor has no scenario ID")?;
                if scenario >> 4 > 624 {
                    continue;
                }
                let bit = index * 2;
                if !self
                    .saved
                    .save_vectors
                    .extra_words_0
                    .get(bit / 32)
                    .is_some_and(|word| word & (1 << (bit % 32)) != 0)
                {
                    return Ok(());
                }
            }
            // Inspected Switch e8eb0(38) only locks/unlocks. Its desktop
            // main-thread boundary adds no unlock, toast, sound or save effect.
        }
        Ok(())
    }
    fn queue_scene(&mut self, request: &TipsSceneRequest) -> Result<()> {
        let request = Request {
            data: Some(RuntimeGameData::from_progress(
                request.progress.clone(),
                self.date,
                request.trailing_values.clone(),
                request.byte_variables.clone(),
            )),
            selected: request.selected,
            alternate: 0,
        };
        request.preflight(&self.common.scenario, &self.saved.save_vectors)?;
        *self.scene_request = Some(request);
        Ok(())
    }
}
impl TipsGet {
    pub fn poll(
        &mut self,
        runner: &mut HigurashiRunner,
        bindings: &mut Bindings,
        saved: &mut SaveData,
        overlay: &mut Option<Box<dyn InputOverlay>>,
        common: &CommonAssets,
        server: &Arc<AssetServer>,
        seeds: &Seeds,
        scene_request: &mut Option<Request>,
        clock: &mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    ) -> Result<bool> {
        let setting = runner.layer_setting();
        if let Some(ui) = self.job.as_ref().and_then(|job| job.ui.as_ref()) {
            ui.wrapper
                .set_route_navigation(saved.settings.common.v14_showroutenavi);
        }
        let mut services = Services {
            owner: self,
            saved,
            overlay,
            common,
            server,
            seeds,
            setting,
            date: bindings.date,
            scene_request,
            clock,
            copied: false,
        };
        let result = runner.poll_tips_get_command(
            &mut bindings.progress,
            &mut bindings.autosave,
            Execution::Live,
            &mut services,
        );
        if services.copied {
            bindings::publish_autosave(&bindings.autosave, services.saved);
        }
        let complete = result?.unwrap_or(false);
        if complete {
            if let Some(job) = &mut self.job {
                job.retire_ui(runner.scene_layers_mut()?, overlay);
            }
            // Native c300/c39c precede mapping release c3d8 and commanddrop.
            runner.complete_external(shin_core::vm::command::CommandResult::None)?;
            self.job = None;
        }
        Ok(complete)
    }
}
