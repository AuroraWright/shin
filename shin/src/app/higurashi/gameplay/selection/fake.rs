use super::super::*;
use crate::adv::higurashi::{
    autosave::{AutosaveMemory, AutosaveServices},
    fake_select::{
        command::Services as CommandServices,
        view::{Screen, Shared},
    },
    touch::Overlay,
};

#[derive(Default)]
pub(in crate::app::higurashi::gameplay) struct Selection {
    pub ui: Option<Shared>,
    fault: Option<String>,
}
struct Services<'a> {
    selection: &'a mut Selection,
    overlay: &'a mut Option<Box<dyn Overlay>>,
    saved: &'a mut SaveData,
    common: &'a CommonAssets,
    server: &'a AssetServer,
    seeds: &'a Seeds,
    clock: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
}
impl CommandServices for Services<'_> {
    fn open(&mut self) -> Result<()> {
        if let Some(fault) = &self.selection.fault {
            bail!("FakeSelect publication failed:{fault}");
        }
        let property_seeds = (self.seeds.lock())();
        let raw_seed = crate::engine_random::next_seed();
        let result = Screen::new(
            self.server,
            self.common.font.clone(),
            self.common.message.clone(),
            property_seeds,
            raw_seed,
        );
        let screen = match result {
            Ok(screen) => screen,
            Err(error) => {
                self.selection.fault = Some(format!("{error:#}"));
                return Err(error);
            }
        };
        let shared = Shared(Arc::new(Mutex::new(screen)));
        *self.overlay = Some(Box::new(shared.clone()));
        self.selection.ui = Some(shared);
        Ok(())
    }
    fn callback_received(&self) -> bool {
        self.selection
            .ui
            .as_ref()
            .is_some_and(|ui| ui.0.lock().model.complete())
    }
}
impl AutosaveServices for Services<'_> {
    fn packed_time(&mut self) -> Result<u64> {
        Ok(bindings::packed_date((self.clock)()?))
    }
    fn captured(&mut self, memory: &AutosaveMemory) -> Result<()> {
        bindings::publish_autosave(memory, self.saved);
        Ok(())
    }
    fn notify(&mut self, _: &str) -> Result<()> {
        bail!("FakeSelect11780 must not queue an AUTOSAVE notice")
    }
}
impl Selection {
    pub fn poll(
        &mut self,
        runner: &mut HigurashiRunner,
        bindings: &mut Bindings,
        saved: &mut SaveData,
        overlay: &mut Option<Box<dyn Overlay>>,
        common: &CommonAssets,
        server: &AssetServer,
        seeds: &Seeds,
        clock: &mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    ) -> Result<bool> {
        let mut services = Services {
            selection: self,
            overlay,
            saved,
            common,
            server,
            seeds,
            clock,
        };
        let complete = runner
            .poll_fake_select_command(
                &mut bindings.progress,
                &mut bindings.autosave,
                &mut services,
            )?
            .unwrap_or(false);
        if complete {
            if let Some(ui) = services.selection.ui.take() {
                if services
                    .overlay
                    .as_ref()
                    .is_some_and(|owner| owner.identity() == ui.identity())
                {
                    *services.overlay = None;
                }
            }
        }
        Ok(complete)
    }
}
