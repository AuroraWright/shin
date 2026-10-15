use super::*;
use crate::adv::higurashi::{
    autosave::Execution,
    choice::{ChoiceRequest, ChoiceServices},
    choice_screen::view::{Screen, Shared},
    touch::Overlay,
};
pub(super) mod character;
pub(super) mod fake;

#[derive(Default)]
pub(super) struct Selection {
    pub ui: Option<Arc<Mutex<Screen>>>,
    pub character: character::Selection,
    pub fake: fake::Selection,
}
struct Services<'a> {
    selection: &'a mut Selection,
    overlay: &'a mut Option<Box<dyn Overlay>>,
    common: &'a CommonAssets,
    server: &'a AssetServer,
    seeds: &'a Seeds,
    clock: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    copied: bool,
    route_navigation: bool,
}
impl ChoiceServices for Services<'_> {
    fn open(&mut self, request: &ChoiceRequest, seen: u32) -> Result<()> {
        let property_seeds = (self.seeds.lock())();
        let shuffle =
            (request.visibility as u32 & 0x1000 != 0).then(crate::engine_random::next_seed);
        let mut screen = Screen::new(
            self.server,
            self.common.font.clone(),
            self.common.message.clone(),
            request,
            seen,
            property_seeds,
            shuffle,
        )?;
        screen.route_navigation = self.route_navigation;
        let screen = Arc::new(Mutex::new(screen));
        *self.overlay = Some(Box::new(Shared(screen.clone())));
        self.selection.ui = Some(screen);
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        Ok(self
            .selection
            .ui
            .as_ref()
            .and_then(|ui| ui.lock().model.result))
    }
    fn packed_time(&mut self) -> Result<u64> {
        let date = (self.clock)()?;
        self.copied = true;
        Ok(bindings::packed_date(date))
    }
}
impl Selection {
    pub fn retire_source(&mut self) {
        self.ui = None;
        self.character = Default::default();
        self.fake = Default::default();
    }
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
        if !matches!(
            runner.pending_command(),
            Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::SELECT { .. })
        ) {
            return Ok(false);
        }
        let route_navigation = saved.settings.common.v14_showroutenavi;
        if let Some(ui) = &self.ui {
            ui.lock().route_navigation = route_navigation;
        }
        let mut services = Services {
            selection: self,
            overlay,
            common,
            server,
            seeds,
            clock,
            copied: false,
            route_navigation,
        };
        let result = runner.poll_choice_command(
            &mut bindings.progress,
            &mut bindings.autosave,
            &mut bindings.choices,
            Execution::Live,
            &mut services,
        );
        //11780 publishes either successful memory copy, without AUTOSAVE's title notice.
        if services.copied {
            bindings::publish_autosave(&bindings.autosave, saved);
        }
        let complete = result?.unwrap_or(false);
        if complete {
            if let Some(ui) = self.ui.take() {
                let id = Arc::as_ptr(&ui) as usize;
                // Original56648/165b4 clears plane0 only if it still owns this UI.
                if overlay.as_ref().is_some_and(|o| o.identity() == id) {
                    *overlay = None;
                }
            }
        }
        Ok(complete)
    }
}
