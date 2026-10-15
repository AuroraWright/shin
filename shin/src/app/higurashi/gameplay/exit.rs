//! Original448c closing and15e54 fragment return,with other destinations owned
//! until their concrete save/TIPS services accept them.
use super::*;
use crate::adv::higurashi::{
    audio::HigurashiAudio, exit::Destination, input::InputState, ui_state::HigurashiUiState,
};
use crate::app::higurashi::runtime_game_data::RuntimeGameData;

pub(super) struct Request {
    pub destination: Destination,
    pub data: RuntimeGameData,
}
pub(super) enum SaveDestination {
    Title,
    Tips(super::super::tips_menu::factory::Request),
}
pub(super) fn poll(
    runner: &mut HigurashiRunner,
    bindings: &mut Bindings,
    saved: &mut SaveData,
    audio: &mut HigurashiAudio,
    ui: &mut HigurashiUiState,
    input: &mut InputState,
    modes_ready: bool,
    kakera: &mut Option<kakera::SceneRequest>,
    queued: &mut bool,
    request: &mut Option<Request>,
    save_id: &mut Option<u64>,
    save_destination: &mut Option<SaveDestination>,
    tips: &mut Option<super::super::tips_menu::factory::Request>,
    bonus: &mut Option<super::bonus::SceneRequest>,
    scenario: &shin_core::format::scenario::higurashi::HigurashiScenario,
    storage: Option<Arc<dyn super::super::load_game::Storage>>,
    runtime_saves: &Option<super::super::runtime_saves::Slots>,
    server: &Arc<AssetServer>,
    save_modes: &mut Vec<crate::adv::higurashi::save_game::SaveGame>,
) -> Result<bool> {
    let date = bindings.date;
    let mut base = saved.clone();
    bindings.publish_globals(&mut base);
    let mut slots = runtime_saves
        .clone()
        .unwrap_or_else(|| super::super::runtime_saves::Slots::loaded(saved));
    slots.update_auto(&bindings.autosave);
    let result = runner.poll_exit_command(
        &mut bindings.progress,
        bindings.memory.commits_enabled,
        audio,
        ui,
        input,
        modes_ready,
        |destination, source, audio| {
            if destination == Destination::Bonus {
                ensure!(bonus.is_none() && !*queued, "Bonus ADV return queued twice");
                //4738..47e0 creates fresh data, not the completed fragment's
                //VM2530. Draw the native MT seed now, while the source lives.
                *bonus = Some(super::bonus::SceneRequest {
                    data: super::bonus::fresh(crate::engine_random::next_seed()),
                    flags: 0,
                });
                *queued = true;
                return Ok(());
            }
            let data = RuntimeGameData::from_progress(
                source.progress().clone(),
                date,
                source.trailing_values()?,
                source.bytes().to_vec(),
            );
            let save_target = if destination == Destination::Tips {
                let returned = super::super::tips_menu::factory::from_exit(
                    &data,
                    scenario,
                    &mut base.save_vectors,
                    1,
                )?;
                saved
                    .save_vectors
                    .extra_words_0
                    .clone_from(&base.save_vectors.extra_words_0);
                audio.browser_music_multiplier(1.0);
                if !returned.save {
                    source.progress_mut().set_active_index(0)?;
                    *tips = Some(returned.request);
                    *queued = true;
                    return Ok(());
                }
                Some(SaveDestination::Tips(returned.request))
            } else if destination == Destination::SaveThenTitle {
                Some(SaveDestination::Title)
            } else {
                None
            };
            if let Destination::Kakera { selected } = destination {
                audio.browser_music_multiplier(1.0); // Actual15e54/32700 before656c0.
                *kakera = Some(kakera::SceneRequest {
                    mode: selected,
                    data,
                });
                *queued = true;
            } else if save_target.is_some() && storage.is_some() {
                ensure!(save_id.is_none(), "A retained save task already exists");
                let storage = storage.as_ref().unwrap().clone();
                let mut snapshot = base.clone();
                snapshot.persist_data = source.persistent().clone();
                let slots = slots.clone();
                let recover = storage.recover_write_errors();
                let write = Arc::new(move || storage.write(&slots.disk(&snapshot).encode()?));
                save_modes.push(
                    crate::adv::higurashi::save_game::SaveGame::new(server.clone(), write, 1, 0)
                        .with_desktop_recovery(
                            recover,
                            crate::adv::higurashi::save_game::RecoveryPolicy::RetryOnly,
                        ),
                );
                *save_id = Some(0);
                *save_destination = save_target;
            } else {
                ensure!(
                    request.is_none(),
                    "A retained EXIT destination already exists"
                );
                *request = Some(Request { destination, data });
            }
            Ok(())
        },
    )?;
    Ok(result.unwrap_or(false))
}
