use super::super::*;
use crate::{
    adv::higurashi::{
        autosave::Execution,
        character_selection::{
            command::{CharacterRequest, CharacterServices},
            view::View,
            wrapper::Wrapper,
        },
        layers::{HigurashiLayers, Target},
        touch::Overlay,
    },
    layer::user::higurashi::HigurashiUserLayer,
};

pub(in crate::app::higurashi::gameplay) struct Job {
    pub task: Option<shin_tasks::AsyncTask<Result<View>>>,
    pub retirement: shin_tasks::LoaderRetirement,
    loaded: Option<View>,
    pub ui: Option<(Target, View)>,
    fault: Option<String>,
}
impl Job {
    pub(in crate::app::higurashi::gameplay) fn new(
        server: Arc<AssetServer>,
        common: &CommonAssets,
        mask: u32,
    ) -> Self {
        let message = common.message.clone();
        let task =
            shin_tasks::async_io::spawn_loader(
                async move { View::load(&server, message, mask).await },
            );
        let retirement = task.retirement_handle().unwrap();
        Self {
            task: Some(task),
            retirement,
            loaded: None,
            ui: None,
            fault: None,
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
                .retire((self.loaded.take(), self.ui.take()));
        }
    }
}
#[derive(Default)]
pub(in crate::app::higurashi::gameplay) struct Selection {
    pub job: Option<Job>,
}
struct Services<'a> {
    selection: &'a mut Selection,
    overlay: &'a mut Option<Box<dyn Overlay>>,
    common: &'a CommonAssets,
    server: &'a Arc<AssetServer>,
    seeds: &'a Seeds,
    clock: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    route_navigation: bool,
    copied: bool,
}
impl CharacterServices for Services<'_> {
    fn prepare(
        &mut self,
        request: &CharacterRequest,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool> {
        let job = self
            .selection
            .job
            .get_or_insert_with(|| Job::new(self.server.clone(), self.common, request.mask));
        if let Some(fault) = &job.fault {
            bail!("Higurashi character atlas load failed:{fault}");
        }
        if let Some(result) = job.task.as_mut().and_then(|task| task.poll_naive()) {
            job.task = None;
            match result {
                Ok(view) => job.loaded = Some(view),
                Err(error) => {
                    job.fault = Some(format!("{error:#}"));
                    return Err(error);
                }
            }
        }
        if job.loaded.is_some() {
            ensure!(
                target.bank == 0
                    && layers
                        .state
                        .target(target.logical)
                        .is_some_and(|t| t.slot == target.slot),
                "Higurashi character scene reservation was replaced"
            );
            let mut view = job.loaded.take().unwrap();
            view.route_navigation = self.route_navigation;
            let retained = view.retain();
            let mut layer = HigurashiUserLayer::character_selection(view, (self.seeds.lock())());
            layer.properties.control(22, 1.0, Ticks::ZERO, 0)?;
            layer.properties.control(5, 1.0, Ticks::ZERO, 0)?;
            let wrapper = Wrapper::new(retained.retain(), (self.seeds.lock())())?;
            layers.publish_temporary_front(target, layer)?;
            *self.overlay = Some(Box::new(wrapper));
            job.ui = Some((target, retained));
        }
        Ok(job.ui.is_some())
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        Ok(self
            .selection
            .job
            .as_ref()
            .and_then(|job| job.ui.as_ref())
            .and_then(|(_, view)| view.model.lock().result))
    }
    fn packed_time(&mut self) -> Result<u64> {
        let date = (self.clock)()?;
        self.copied = true;
        Ok(bindings::packed_date(date))
    }
}
impl Selection {
    pub(in crate::app::higurashi::gameplay) fn poll(
        &mut self,
        runner: &mut HigurashiRunner,
        bindings: &mut Bindings,
        saved: &mut SaveData,
        overlay: &mut Option<Box<dyn Overlay>>,
        common: &CommonAssets,
        server: &Arc<AssetServer>,
        seeds: &Seeds,
        clock: &mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    ) -> Result<bool> {
        if !matches!(
            runner.pending_command(),
            Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::CHARSELECT { .. })
        ) {
            return Ok(false);
        }
        let route_navigation = saved.settings.common.v14_showroutenavi;
        if let Some((target, view)) = self.job.as_mut().and_then(|job| job.ui.as_mut()) {
            view.route_navigation = route_navigation;
            if let Some(actual) = runner
                .scene_layers_mut()?
                .layer_mut(*target)
                .and_then(|l| l.character_view_mut())
            {
                if Arc::ptr_eq(&actual.model, &view.model) {
                    actual.route_navigation = route_navigation;
                }
            }
        }
        let mut services = Services {
            selection: self,
            overlay,
            common,
            server,
            seeds,
            clock,
            route_navigation,
            copied: false,
        };
        let result = runner.poll_character_command(
            &mut bindings.progress,
            &mut bindings.autosave,
            &mut bindings.choices,
            Execution::Live,
            &mut services,
        );
        if services.copied {
            bindings::publish_autosave(&bindings.autosave, saved);
        }
        let complete = result?.unwrap_or(false);
        if complete {
            if let Some((target, view)) = self.job.as_ref().and_then(|job| job.ui.as_ref()) {
                let id = Arc::as_ptr(&view.model) as usize;
                runner
                    .scene_layers_mut()?
                    .retire_character_front(*target, id);
                if overlay.as_ref().is_some_and(|o| o.identity() == id) {
                    *overlay = None;
                }
            }
            self.job = None;
        }
        Ok(complete)
    }
}
