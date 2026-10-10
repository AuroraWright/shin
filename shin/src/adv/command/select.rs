use super::prelude::*;
use crate::adv::choice::ChoiceMenu;

#[derive(Debug)]
pub struct SELECT {
    token: Option<command::token::SELECT>,
    choice_set_base: u16,
    choice_index: u16,
}

impl StartableCommand for command::runtime::SELECT {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        // Constructor 0xd90 snapshots only when no message text is active.
        if state.messagebox_state.text.is_none() {
            state.commit_persistent();
            state.advance_save_position();
        }
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let base = self.choice_set_base;
        adv_state.choice_menu = Some(ChoiceMenu::new(
            context.pre_render,
            adv_state.choice_font.clone(),
            adv_state.choice_textures.clone(),
            &self.variants,
            self.choice_visibility_mask,
            |index| {
                base != 0
                    && vm_state
                        .save_vectors
                        .select_seen_choices
                        .contains(base as usize - 1 + index)
            },
        ));
        Yield(
            SELECT {
                token: Some(self.token),
                choice_set_base: base,
                choice_index: self.choice_index,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for SELECT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let result = adv_state.choice_menu.as_ref()?.model.result?;
        record_choice(vm_state, self.choice_set_base, self.choice_index, result);
        adv_state.choice_menu = None;
        Some(self.token.take().unwrap().finish(result))
    }
}

pub(super) fn record_choice(state: &mut VmState, base: u16, choice_index: u16, result: i32) {
    if state.persistent_commits_enabled {
        state.route.record_choice(result);
    }
    state.advance_save_position();
    // Both table indexes are one-based on disk; zero disables history.
    // The original completion handler stores result+1 in the last-choice table.
    if state.persistent_commits_enabled && base != 0 {
        state
            .save_vectors
            .select_seen_choices
            .add(base as usize - 1 + result as usize);
    }
    if state.persistent_commits_enabled && choice_index != 0 {
        let index = choice_index as usize - 1;
        state.save_vectors.select_last_choices.resize(
            state.save_vectors.select_last_choices.len().max(index + 1),
            0,
        );
        state.save_vectors.select_last_choices[index] = (result + 1) as u8;
    }
    state.capture_autosave();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_original_choice_indexes_and_one_based_last_choice() {
        let mut state = VmState::new();
        record_choice(&mut state, 8, 2, 3);
        assert_eq!(state.save_position, 1);
        assert_eq!(
            state
                .auto_save_slot
                .as_ref()
                .unwrap()
                .entry
                .route_data
                .reader()
                .read_choice()
                .unwrap(),
            3
        );
        assert!(state.save_vectors.select_seen_choices.contains(10));
        assert_eq!(state.save_vectors.select_last_choices, [0, 4]);
        record_choice(&mut state, 0, 0, 0);
        let mut reader = state
            .auto_save_slot
            .as_ref()
            .unwrap()
            .entry
            .route_data
            .reader();
        assert_eq!(reader.read_choice().unwrap(), 3);
        assert_eq!(reader.read_choice().unwrap(), 0);
        assert_eq!(state.save_position, 2);
        assert!(!state.save_vectors.select_seen_choices.contains(0));
        assert_eq!(state.save_vectors.select_last_choices, [0, 4]);
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); checks VM routing, not rendering"]
    fn reference_debug_menu_routes_to_episode_instead_of_looping() {
        use crate::adv::choice::ChoiceModel;
        use shin_core::{
            format::rom::RomReader, primitives::stateless_reader::StatelessFile, vm::Scripter,
        };
        use std::{fs::File, io::Read};

        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut scripter = Scripter::new(&scenario, 36, 42);
        let mut result = CommandResult::None;
        let mut choices = 0;
        let mut state = VmState::new();
        for _ in 0..500_000 {
            let command = scripter.run(result).unwrap();
            result = match command {
                command::RuntimeCommand::SELECT(command) => {
                    let mut model =
                        ChoiceModel::new(command.variants.len(), command.choice_visibility_mask);
                    // Main story, then episode 1 in the debug menu.
                    model.update(shin_core::time::Ticks::from_f32(100.0));
                    model.update(shin_core::time::Ticks::ZERO);
                    model.move_selection(1);
                    model.move_selection(1);
                    model.confirm();
                    model.update(shin_core::time::Ticks::from_f32(7.0));
                    model.update(shin_core::time::Ticks::ZERO);
                    model.update(shin_core::time::Ticks::from_f32(100.0));
                    model.update(shin_core::time::Ticks::ZERO);
                    let selected = model.result.expect("No visible reference choice");
                    record_choice(
                        &mut state,
                        command.choice_set_base,
                        command.choice_index,
                        selected,
                    );
                    choices += 1;
                    command.token.finish(selected)
                }
                other => match other.execute_dummy() {
                    Some(result) => result,
                    None => {
                        assert!(choices >= 2, "Expected the debug and episode menus");
                        return;
                    }
                },
            };
        }
        panic!("Reference entry 36 did not finish after choosing episode 1");
    }
}
