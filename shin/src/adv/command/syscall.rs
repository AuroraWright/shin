use super::prelude::*;

impl StartableCommand for command::runtime::SYSCALL {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        // VM constructor 0x2d90 stores argument != 0 at 0x913c for ID 0.
        // ADV input/update code uses that byte to gate system controls.
        if self.call_id == 0 {
            state.system_controls_allowed = self.argument != 0;
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // Poll 0x7c10 clears ADV playback mode word 0x47c for ID 0.
        if self.call_id == 0 {
            adv_state.playback_modes.reset();
        }
        self.token.finish().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::{
        format::scenario::instruction_elements::NumberSpec,
        vm::{IntoRuntimeForm, VmCtx},
    };

    #[test]
    fn system_control_gate_uses_boolean_argument_and_ignores_other_ids() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 0);
        for (id, argument, allowed) in [
            (0, 0, false),
            (1, 1, false),
            (0, -1, true),
            (0, 0, false),
            (0, 1, true),
        ] {
            let command = command::CompiletimeCommand::SYSCALL(command::compiletime::SYSCALL {
                call_id: NumberSpec::constant(id),
                argument: NumberSpec::constant(argument),
            })
            .into_runtime_form(&ctx);
            super::super::apply_command_state(command, &mut state);
            assert_eq!(state.system_controls_allowed, allowed);
        }
    }

    #[test]
    fn event_lifecycle_gates_system_controls_independently_of_syscall() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 0);
        let begin = command::CompiletimeCommand::EVBEGIN(command::compiletime::EVBEGIN {
            arg: NumberSpec::constant(24),
        })
        .into_runtime_form(&ctx);
        super::super::apply_command_state(begin, &mut state);
        assert_eq!(state.current_event, Some(24));
        assert!(!state.allows_system_controls());
        let end = command::CompiletimeCommand::EVEND(command::compiletime::EVEND {})
            .into_runtime_form(&ctx);
        super::super::apply_command_state(end, &mut state);
        assert!(state.allows_system_controls());
        state.system_controls_allowed = false;
        assert!(!state.allows_system_controls());
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); checks VM and control/persistent state"]
    fn reference_episode_updates_control_and_persistent_state() {
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
        let mut scripter = Scripter::new(&scenario, 0, 42);
        let mut result = CommandResult::None;
        let mut state = VmState::new();
        let mut event_count = 0;
        let mut system_call_count = 0;
        let mut persistent_writes = 0;
        for _ in 0..500_000 {
            let command = scripter.run(result).unwrap();
            result = match command {
                command::RuntimeCommand::SYSCALL(cmd) => {
                    cmd.apply_state(&mut state);
                    system_call_count += 1;
                    cmd.token.finish()
                }
                command::RuntimeCommand::EVBEGIN(cmd) => {
                    cmd.apply_state(&mut state);
                    assert!(!state.allows_system_controls());
                    event_count += 1;
                    cmd.token.finish()
                }
                command::RuntimeCommand::EVEND(cmd) => {
                    cmd.apply_state(&mut state);
                    assert_eq!(state.current_event, None);
                    cmd.token.finish()
                }
                command::RuntimeCommand::SSET(cmd) => {
                    cmd.apply_state(&mut state);
                    persistent_writes += 1;
                    cmd.token.finish()
                }
                command::RuntimeCommand::SGET(cmd) => {
                    cmd.token.finish(state.get_persistent(cmd.slot_number))
                }
                command::RuntimeCommand::MSGSET(cmd) => {
                    cmd.apply_state(&mut state);
                    cmd.token.finish()
                }
                command::RuntimeCommand::MSGCLOSE(cmd) => {
                    cmd.apply_state(&mut state);
                    cmd.token.finish()
                }
                command::RuntimeCommand::AUTOSAVE(cmd) => {
                    cmd.apply_state(&mut state);
                    cmd.token.finish()
                }
                command::RuntimeCommand::UNLOCK(cmd) => {
                    cmd.apply_state(&mut state);
                    cmd.token.finish()
                }
                command::RuntimeCommand::EXIT(cmd) if cmd.arg1 == 0 => {
                    cmd.apply_state(&mut state);
                    assert!(event_count > 0 && system_call_count > 0 && persistent_writes > 0);
                    assert!(!state.system_controls_allowed);
                    assert!(state.save_vectors.unlocked_cgs.iter().next().is_some());
                    assert!(!state.persist.0.is_empty());
                    return;
                }
                other => match other.execute_dummy() {
                    Some(result) => result,
                    None => {
                        assert!(event_count > 0 && system_call_count > 0 && persistent_writes > 0);
                        assert!(!state.system_controls_allowed);
                        assert!(state.save_vectors.unlocked_cgs.iter().next().is_some());
                        return;
                    }
                },
            };
        }
        panic!("Reference episode did not reach EXIT");
    }
}
