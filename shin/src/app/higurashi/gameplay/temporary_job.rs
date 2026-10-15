//! Asset-only preparation and main-thread notification publication/retirement.
//! The command owns its reservation; this job owns the distinct input wrapper.
use super::*;
use crate::{
    adv::higurashi::{
        layers::{HigurashiLayers, Target},
        temporary_wrapper::{ChoiceResources, Completion, Shared},
        touch::Overlay as InputOverlay,
    },
    layer::user::higurashi::{HigurashiLayerKind, Prepared},
};

pub(super) struct Ui {
    pub target: Target,
    pub wrapper: Shared,
}
pub(super) struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Prepared>>>,
    retirement: shin_tasks::LoaderRetirement,
    ready: Option<Prepared>,
    pub ui: Option<Ui>,
    fault: Option<String>,
}
impl Job {
    #[cfg(test)]
    pub fn reference_retirement(&self) -> shin_tasks::LoaderRetirement {
        self.retirement.clone()
    }
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<shin_core::format::scenario::higurashi::HigurashiScenario>,
        kind: HigurashiLayerKind,
        params: [i32; 8],
        setting: u8,
    ) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            Prepared::load(
                &server,
                &scenario,
                kind,
                (
                    params[0], params[1], params[2], params[3], params[4], params[5], params[6],
                    params[7],
                ),
                setting,
            )
            .await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            task: Some(task),
            retirement,
            ready: None,
            ui: None,
            fault: None,
        }
    }
    pub fn publish(
        &mut self,
        target: Target,
        layers: &mut HigurashiLayers,
        saved: &mut SaveData,
        overlay: &mut Option<Box<dyn InputOverlay>>,
        common: &CommonAssets,
        server: &Arc<AssetServer>,
        seeds: &Seeds,
    ) -> Result<bool> {
        if let Some(fault) = &self.fault {
            bail!("Notification preparation/publication failed:{fault}");
        }
        if let Some(result) = self.task.as_mut().and_then(|task| task.poll_naive()) {
            self.task = None;
            match result {
                Ok(prepared) => self.ready = Some(prepared),
                Err(error) => {
                    self.fault = Some(format!("{error:#}"));
                    return Err(error);
                }
            }
        }
        if self.ready.is_some() {
            // Validate before constructor entropy/acquired bits. The captured
            // plane is0 even when a retained logical back selector is nonzero.
            ensure!(
                target.bank == 0
                    && layers
                        .state
                        .target(target.logical)
                        .is_some_and(|current| current.slot == target.slot),
                "Notification scene reservation was replaced"
            );
            let prepared = self.ready.take().unwrap();
            let result = (|| -> Result<()> {
                let mut layer = prepared
                    .construct_with_settings(None, Some((seeds.lock())()), Some(&mut *saved))?
                    .context("Notification factory returned NULL")?;
                layer.properties.control(22, 1., Ticks::ZERO, 0)?;
                layer.properties.control(5, 1., Ticks::ZERO, 0)?;
                let widget = layer
                    .temporary_owner()
                    .context("Notification factory has no private widget")?;
                let child_seeds = seeds.clone();
                let choice = (widget.native_id() == 12).then(|| ChoiceResources {
                    server: server.clone(),
                    font: common.font.clone(),
                    textures: common.message.clone(),
                    seeds: Box::new(move || (child_seeds.lock())()),
                });
                let wrapper = Shared::new(
                    widget,
                    (seeds.lock())(),
                    choice,
                    saved.settings.common.v14_showroutenavi,
                )?;
                // Optional ADV siblings receive the outer identity; the screen
                // root transform applies only to its separately cached plane.
                wrapper.set_parent(Default::default());
                layers.publish_temporary_front(target, layer)?;
                *overlay = Some(Box::new(wrapper.clone()));
                self.ui = Some(Ui { target, wrapper });
                Ok(())
            })();
            if let Err(error) = result {
                // Constructor work is not replayed after a partially consumed
                // entropy/acquisition prefix. Asset errors also retain a fault.
                self.fault = Some(format!("{error:#}"));
                return Err(error);
            }
        }
        if let Some(ui) = &self.ui {
            ui.wrapper
                .set_route_navigation(saved.settings.common.v14_showroutenavi);
            if let Some(fault) = ui.wrapper.fault() {
                bail!("Notification wrapper failed:{fault}");
            }
        }
        Ok(self.ui.is_some())
    }
    pub fn completion(&self) -> Result<Option<Completion>> {
        self.ui
            .as_ref()
            .map_or(Ok(None), |ui| ui.wrapper.completion())
    }
    /// Wrapper first, then original drawable byidentity. The caller releases
    /// its logical mapping through complete_external only after this succeeds.
    pub fn retire_ui(
        &mut self,
        layers: &mut HigurashiLayers,
        overlay: &mut Option<Box<dyn InputOverlay>>,
    ) {
        // Keep the command's strong controller/widget owner through mapping
        // release. Job drop follows complete_external in the caller.
        if let Some(ui) = &self.ui {
            if overlay
                .as_ref()
                .is_some_and(|owner| owner.identity() == ui.wrapper.identity())
            {
                *overlay = None;
            }
            layers.retire_temporary_front(ui.target, ui.wrapper.widget_identity());
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement
                .clone()
                .retire((self.ready.take(), self.ui.take()));
        }
    }
}
