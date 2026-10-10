use super::prelude::*;

impl StartableCommand for command::runtime::UNLOCK {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {
        // Reconstruction runs the native no-op RunState, not the unlock poll.
        if !_state.persistent_commits_enabled {
            return;
        }
        // Reference poll 0x7cd0 selects CG/BGM/movie bitsets for types 0/1/2.
        let set = match self.unlock_type {
            0 => &mut _state.save_vectors.unlocked_cgs,
            1 => &mut _state.save_vectors.unlocked_bgms,
            2 => &mut _state.save_vectors.unlocked_movies,
            _ => return,
        };
        _state.save_revision = _state.save_revision.wrapping_add(1);
        for &index in &self.unlock_indices {
            set.add(index.try_into().expect("Negative unlock index"));
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        self.token.finish().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::{
        format::scenario::{instruction_elements::NumberSpec, types::U8SmallNumberList},
        vm::{IntoRuntimeForm, VmCtx},
    };

    #[test]
    fn unlocks_selected_gallery_only_and_keeps_existing_entries() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 0);
        for ty in [0, 1, 2, 3] {
            let command = command::CompiletimeCommand::UNLOCK(command::compiletime::UNLOCK {
                unlock_type: ty,
                unlock_indices: U8SmallNumberList::from_iter([
                    NumberSpec::constant(32),
                    NumberSpec::constant(256),
                ]),
            })
            .into_runtime_form(&ctx);
            super::super::apply_command_state(command, &mut state);
            assert_eq!(state.save_vectors.unlocked_cgs.contains(32), true);
            assert_eq!(state.save_vectors.unlocked_bgms.contains(256), ty >= 1);
            assert_eq!(state.save_vectors.unlocked_movies.contains(256), ty >= 2);
        }
    }
}
