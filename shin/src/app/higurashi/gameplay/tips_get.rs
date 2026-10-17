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
            for index in 0..self.common.scenario.tips_info.len() {
                let scenario = super::super::tips_menu::script_id(&self.common.scenario, index)
                    .context("TIPS source index has no supported script entry")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adv::higurashi::{progress::ProgressMemory, reference_gpu_assets},
        asset::system::{AssetLoadContext, LayeredAssetIo},
        cli::Game,
    };
    use shin_core::format::scenario::higurashi::HigurashiVariant;

    #[test]
    #[ignore = "actual base scenario/CommonAssets on Metal; original TIPSGET callback, empty/all188 acquired states; no audio, storage or gameplay jobs"]
    fn patchless_original_tipsget_opening_callback_preserves_saved_and_live_progress() {
        let (device, queue, _) = reference_gpu_assets();
        let root = std::path::PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("data.rom")).unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: Game::Higurashi,
                wgpu_device: device,
                wgpu_queue: queue,
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let common = CommonAssets {
            scenario: server.load_sync("/main.snr").unwrap(),
            font: server.load_sync("/newrodin.fnt").unwrap(),
            system: server.load_sync("/systex.txa").unwrap(),
            message: server.load_sync("/msgtex.txa").unwrap(),
            effects: server.load_sync("/effects.txa").unwrap(),
            sysse: server.load_sync("/sysse.bin").unwrap(),
        };
        assert_eq!(common.scenario.variant, HigurashiVariant::Original);
        assert_eq!(common.scenario.tips_info.len(), 188);
        assert!(
            common
                .scenario
                .tips_info
                .iter()
                .all(|tip| tip.updated_field.is_none())
        );
        for index in 0..188 {
            assert_eq!(
                super::super::super::tips_menu::script_id(&common.scenario, index),
                Some(1000 + index as u16)
            );
        }
        let initial = SaveData::decode(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/higurashi/old-save-20261015/supplied-v1.dat"
        )))
        .unwrap();
        let data = RuntimeGameData::from(initial.auto_save_slot.clone().unwrap());
        let mut source =
            ProgressMemory::new(initial.persist_data.clone(), data.progress, vec![4, 1, 255])
                .with_trailing_values(vec![0, 255, 1023]);
        source.restore_response_cursor(1).unwrap();
        assert_eq!(source.progress().active_entry().scenario_id, 0);
        let progress_before = format!("{source:?}");
        let seeds: Seeds = Arc::new(Mutex::new(Box::new(|| {
            panic!("observational TIPS callback must not consume seeds")
        })));
        for acquired in [false, true] {
            let mut saved = initial.clone();
            saved.save_vectors.extra_words_0.clear();
            if acquired {
                saved.save_vectors.extra_words_0.resize(12, 0);
                for index in 0..188 {
                    saved.save_vectors.extra_words_0[index * 2 / 32] |= 1 << (index * 2 % 32);
                }
            }
            let saved_before = serde_json::to_vec(&saved).unwrap();
            let mut owner = TipsGet::default();
            let mut overlay = None;
            let mut scene_request = None;
            let mut clock =
                || -> Result<_> { panic!("observational TIPS callback must not sample date") };
            let mut services = Services {
                owner: &mut owner,
                saved: &mut saved,
                overlay: &mut overlay,
                common: &common,
                server: &server,
                seeds: &seeds,
                setting: 0,
                date: data.date,
                scene_request: &mut scene_request,
                clock: &mut clock,
                copied: false,
            };
            TipsGetServices::opening_copied(&mut services, &source).unwrap();
            assert!(!services.copied);
            drop(services);
            assert_eq!(serde_json::to_vec(&saved).unwrap(), saved_before);
            assert_eq!(format!("{source:?}"), progress_before);
            assert!(owner.job.is_none());
            assert!(overlay.is_none());
            assert!(scene_request.is_none());
            eprintln!(
                "PATCHLESS_TIPSGET_CALLBACK_PASS original_rows=188 acquired={acquired} saved_and_progress_unchanged=true"
            );
        }
    }
}
