use super::*;
use crate::adv::higurashi::{
    autosave::Execution, kakera::KakeraServices, progress::ProgressMemory,
};
use crate::app::higurashi::runtime_game_data::RuntimeGameData;

/// Owned656c0 factory arguments. Native mode0 enters the separate Kakera scene.
pub(in crate::app::higurashi) struct SceneRequest {
    pub mode: i32,
    pub data: RuntimeGameData,
}
struct Services<'a> {
    request: &'a mut Option<SceneRequest>,
    queued: &'a mut bool,
    date: shin_core::format::save::higurashi::SaveDate,
    clock: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    copied: bool,
}
impl KakeraServices for Services<'_> {
    fn packed_time(&mut self) -> Result<u64> {
        let date = (self.clock)()?;
        self.copied = true;
        Ok(bindings::packed_date(date))
    }
    fn queue_scene(&mut self, mode: i32, source: &ProgressMemory) -> Result<()> {
        let data = RuntimeGameData::from_progress(
            source.progress().clone(),
            self.date,
            source.trailing_values()?,
            source.bytes().to_vec(),
        );
        *self.request = Some(SceneRequest { mode, data });
        *self.queued = true;
        Ok(())
    }
}
pub(super) fn poll(
    runner: &mut HigurashiRunner,
    bindings: &mut Bindings,
    saved: &mut SaveData,
    request: &mut Option<SceneRequest>,
    queued: &mut bool,
    clock: &mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
) -> Result<bool> {
    let mut services = Services {
        request,
        queued,
        date: bindings.date,
        clock,
        copied: false,
    };
    let result = runner.poll_kakera_command(
        &mut bindings.progress,
        &mut bindings.autosave,
        Execution::Live,
        &mut services,
    );
    if services.copied {
        bindings::publish_autosave(&bindings.autosave, saved);
    }
    Ok(result?.unwrap_or(false))
}
