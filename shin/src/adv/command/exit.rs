use super::prelude::*;

impl StartableCommand for command::runtime::EXIT {
    type StateInfo = ();

    fn apply_state(&self, state: &mut VmState) {
        if self.arg1 == 0 {
            state.commit_persistent();
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
        if self.arg1 == 0 {
            CommandStartResult::Exit
        } else {
            self.token.finish().into()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied original and1.03 executables"]
    fn reference_exit_completion_saves_then_queues_title_factory() {
        let folder = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .parent()
            .unwrap()
            .to_path_buf();
        for (name, save_call, title_call) in [
            ("uncompressed_executable", 0x940302d5, 0x9403d5ec),
            ("uncompressed_executable_update", 0x940302b5, 0x9403d81c),
        ] {
            let exe = std::fs::read(folder.join(name)).unwrap();
            // Native EXIT commits pending persistent writes, schedules the
            // same SaveGame/callback as confirmed menu return, then selects
            // the title's Continue row with factory argument -1.
            for (address, instruction) in [
                (0x4c50, 0xf9400a60),
                (0x4c54, 0x94002b97),
                (0x4c58, 0xf9400660),
                (0x4c5c, 0x94003ee1),
                (0x4c60, 0x12800008),
                (0x14818, 0x320003e2),
                (0x1481c, save_call),
                (0x187e4, 0x12800000),
                (0x187f0, title_call),
                (0x187f8, 0x94003ade),
            ] {
                assert_eq!(
                    u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                    instruction,
                    "{name} native{address:x}"
                );
            }
        }
    }
}
