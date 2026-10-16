use anyhow::{Context, Result, bail};
use shin_core::{
    format::{
        save::{GameData, GameDataEntry, SaveData},
        scenario::Scenario,
    },
    vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand},
    },
};

use super::{StartableCommand, VmState, command};

/// The native stop can occur at command entry (a displayed message or an open
/// choice), or after an interactive response has been read into VM memory.
pub enum ReconstructionStop {
    Command(RuntimeCommand),
    Response(CommandResult),
}

/// Script and scene description rebuilt from the entrypoint. GPU resources and
/// audio playback must be restored separately before this becomes a live scene.
pub struct ReconstructedScene {
    pub scripter: Scripter,
    pub state: VmState,
    pub stop: ReconstructionStop,
}

impl ReconstructedScene {
    /// Native f4f0 searches without publishing reads, unlocks or autosaves, then
    /// prepares f220 at2dc8. A current unread message or open choice can make
    /// that target no later than the source, in which case there is no jump.
    pub(super) fn next_chapter_game_data(
        scenario: &Scenario,
        game: &GameData,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Option<Self>> {
        let mut probe = Self::from_game_data(scenario, game, globals, command_limit)?;
        probe.state.persistent_commits_enabled = false;
        let finish = |state: &VmState| -> Result<Option<Self>> {
            if state.chapter_skip_position <= game.entry.save_position {
                return Ok(None);
            }
            let mut target = state.snapshot_game_data();
            target.entry.save_position = state.chapter_skip_position;
            Self::from_game_data(scenario, &target, globals, command_limit).map(Some)
        };
        let result = match probe.stop {
            ReconstructionStop::Response(result) => result,
            ReconstructionStop::Command(command) => {
                if Self::chapter_search_blocks(&command, &probe.state) {
                    return finish(&probe.state);
                }
                command
                    .execute_dummy()
                    .context("Chapter search starts at EXIT")?
            }
        };
        Self::search_next_chapter(&mut probe.scripter, &mut probe.state, result, command_limit)?;
        finish(&probe.state)
    }

    pub(super) fn chapter_search_blocks(command: &RuntimeCommand, state: &VmState) -> bool {
        matches!(command, RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_))
            || (matches!(command, RuntimeCommand::MSGSET(_))
                && state.current_event.is_none()
                && state.settings.v11_disallowskipunread
                && !state.messagebox_state.was_read)
    }

    /// Advance only the copied CPU state. The caller decides whether to prepare
    /// a target; no GPU, audio, persistence or live breakpoint owner is used.
    pub(super) fn search_next_chapter(
        scripter: &mut Scripter,
        state: &mut VmState,
        mut result: CommandResult,
        command_limit: usize,
    ) -> Result<()> {
        state.persistent_commits_enabled = false;
        for _ in 0..command_limit {
            let command = scripter.run(result).context("Searching next chapter")?;
            match command {
                RuntimeCommand::SGET(command) => {
                    let value = command.apply_state(state);
                    result = command.token.finish(value);
                }
                RuntimeCommand::RESUME(_) => {
                    // The replacement owns another replay; never cross it in
                    // this probe or invent an interactive response.
                    return Ok(());
                }
                RuntimeCommand::EXIT(command) if command.arg1 == 0 => return Ok(()),
                command => {
                    command::apply_command_state_ref(&command, state)?;
                    if matches!(command, RuntimeCommand::AUTOSAVE(_))
                        || Self::chapter_search_blocks(&command, state)
                    {
                        return Ok(());
                    }
                    result = command
                        .execute_dummy()
                        .context("Chapter search reaches EXIT")?;
                }
            }
        }
        bail!("Next chapter not reached within reconstruction command limit")
    }

    /// Replay the selected route while retaining the inactive native entry.
    pub fn from_game_data(
        scenario: &Scenario,
        game: &GameData,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        let mut scene = Self::from_save(scenario, &game.entry, globals, command_limit)?;
        scene.state.primary_entry = game.primary_entry.clone();
        Ok(scene)
    }

    /// Native saved-scene worker 1b3b4 restarts an event checkpoint at2dc8
    /// before hydrating its layers. Event MSGSET captures are valid Continue
    /// data, but their EVBEGIN boundary must not become the live load target.
    pub(super) fn from_game_data_for_load(
        scenario: &Scenario,
        game: &GameData,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        let scene = Self::from_game_data(scenario, game, globals, command_limit)?;
        if scene.state.current_event.is_some()
            && scene.state.chapter_skip_position < game.entry.save_position
        {
            let mut target = game.clone();
            target.entry.save_position = scene.state.chapter_skip_position;
            return Self::from_game_data(scenario, &target, globals, command_limit);
        }
        Ok(scene)
    }

    pub(super) fn publishes_continue_checkpoint(&self) -> bool {
        !matches!(
            &self.stop,
            ReconstructionStop::Command(RuntimeCommand::EVBEGIN(_) | RuntimeCommand::EVEND(_))
        )
    }

    pub(super) fn skip_event_game_data(
        scenario: &Scenario,
        game: &GameData,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        let mut scene = Self::skip_event(scenario, &game.entry, globals, command_limit)?;
        scene.state.primary_entry = game.primary_entry.clone();
        Ok(scene)
    }
    /// Native f600 replays the current boundary without publishing global writes,
    /// advances the event, then prepares a normal mode0 scene at its end (f220).
    /// Movie completion alone is insufficient: masks, music and subsequent layer
    /// commands in the event must reach their authored final state as well.
    pub(super) fn skip_event(
        scenario: &Scenario,
        entry: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        let mut probe = Self::from_save(scenario, entry, globals, command_limit)?;
        if probe.state.current_event.is_none() {
            bail!("Event skip requested outside an event")
        }
        probe.state.persistent_commits_enabled = false;
        let mut result = match probe.stop {
            ReconstructionStop::Response(result) => result,
            ReconstructionStop::Command(command) => command
                .execute_dummy()
                .context("Event boundary is scenario EXIT")?,
        };
        for _ in 0..command_limit {
            let command = probe
                .scripter
                .run(result)
                .context("Advancing skipped event")?;
            match command {
                RuntimeCommand::SGET(command) => {
                    let value = command.apply_state(&mut probe.state);
                    result = command.token.finish(value);
                }
                RuntimeCommand::RESUME(_) | RuntimeCommand::EXIT(_) => {
                    bail!("Event ends in scene replacement before EVEND")
                }
                command => {
                    let interactive =
                        matches!(command, RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_));
                    command::apply_command_state_ref(&command, &mut probe.state)?;
                    if probe.state.current_event.is_none() || interactive {
                        // Rebuild at the boundary rather than installing the probe:
                        // simulation must not publish read flags, persistent
                        // writes, unlocks, trophies or autosaves during its search.
                        let target = probe.state.snapshot_entry();
                        return Self::from_save(scenario, &target, globals, command_limit);
                    }
                    result = command
                        .execute_dummy()
                        .context("Skipped event reaches EXIT")?;
                }
            }
        }
        bail!("Event end not reached within reconstruction command limit")
    }

    pub fn from_save(
        scenario: &Scenario,
        entry: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        Self::from_save_with_observer(scenario, entry, globals, command_limit, |_, _, _| {})
    }

    pub(super) fn from_save_with_observer(
        scenario: &Scenario,
        entry: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
        mut observe: impl FnMut(&RuntimeCommand, &VmState, Option<i32>),
    ) -> Result<Self> {
        let mut scripter = Scripter::new(scenario, entry.scenario_id, entry.random_seed);
        let mut state = VmState::for_scenario(entry.scenario_id, entry.random_seed);
        state.settings = globals.settings.clone();
        state.persist = globals.persist_data.clone();
        state.save_vectors = globals.save_vectors.clone();
        state.auto_save_slot = globals.auto_save_slot.clone();
        state.persistent_commits_enabled = false;
        let mut route = entry.route_data.reader();
        let mut result = CommandResult::None;
        let mut stop = (entry.save_position == 0).then_some(ReconstructionStop::Response(result));

        for _ in 0..command_limit {
            if stop.is_some() {
                break;
            }
            let command = scripter.run(result).context("Executing saved route")?;
            result = match command {
                RuntimeCommand::SGET(command) => {
                    // Native 0x4cf0 reads the stream; current globals and pending
                    // SSET values do not determine historical branch decisions.
                    command
                        .token
                        .finish(route.read_persistent().context("Reading saved SGET")?)
                }
                command @ RuntimeCommand::SELECT(_) => {
                    let RuntimeCommand::SELECT(select) = &command else {
                        unreachable!()
                    };
                    select.apply_state(&mut state);
                    if state.save_position >= entry.save_position {
                        stop = Some(ReconstructionStop::Command(command));
                        break;
                    }
                    let value = route.read_choice().context("Reading saved SELECT")?;
                    observe(&command, &state, Some(value));
                    command::replay_choice_result(
                        &mut state,
                        select.choice_set_base,
                        select.choice_index,
                        value,
                    );
                    let RuntimeCommand::SELECT(select) = command else {
                        unreachable!()
                    };
                    select.token.finish(value)
                }
                command @ RuntimeCommand::QUIZ(_) => {
                    let RuntimeCommand::QUIZ(quiz) = &command else {
                        unreachable!()
                    };
                    let target = quiz.apply_state(&mut state);
                    if state.save_position >= entry.save_position {
                        stop = Some(ReconstructionStop::Command(command));
                        break;
                    }
                    let value = route.read_quiz().context("Reading saved QUIZ")?;
                    observe(&command, &state, Some(value));
                    command::replay_quiz_result(&mut state, target, value);
                    let RuntimeCommand::QUIZ(quiz) = command else {
                        unreachable!()
                    };
                    quiz.token.finish(value)
                }
                RuntimeCommand::RESUME(_) => {
                    bail!("Saved position is beyond RESUME scene replacement")
                }
                command => {
                    command::apply_command_state_ref(&command, &mut state)?;
                    observe(&command, &state, None);
                    if state.save_position >= entry.save_position {
                        stop = Some(ReconstructionStop::Command(command));
                        break;
                    }
                    command
                        .execute_dummy()
                        .context("Saved position is beyond scenario EXIT")?
                }
            };
            if state.save_position >= entry.save_position {
                stop = Some(ReconstructionStop::Response(result));
            }
        }

        let stop =
            stop.context("Saved position not reached within reconstruction command limit")?;
        state.route = route.into_recorder();
        state.persistent_commits_enabled = true;
        Ok(Self {
            scripter,
            state,
            stop,
        })
    }
}

#[cfg(test)]
mod native_replay_tests;

#[cfg(test)]
mod chapter_tests;

#[cfg(test)]
mod filtered_select_tests;

#[cfg(test)]
mod movie_route_tests;

#[cfg(test)]
pub(super) fn assert_reference_load_feedback<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
) {
    let assets = shin_tasks::block_on(crate::adv::assets::AdvAssets::load(server)).unwrap();
    let mut frame = shin_core::primitives::update::FrameId::default();
    frame.advance();
    for kind in [super::SceneLoadKind::Saved, super::SceneLoadKind::History] {
        tests::verify_live_scene_load_fade(context, server, &assets, frame, kind);
    }
}

#[cfg(test)]
mod tests;
