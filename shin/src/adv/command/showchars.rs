use super::prelude::*;
use crate::adv::character_screen::CharacterScreen;

#[derive(Debug)]
pub struct SHOWCHARS {
    token: Option<command::token::SHOWCHARS>,
    previous_system_controls: bool,
    grid: i32,
    initial_page: i32,
    error: Option<anyhow::Error>,
}
impl StartableCommand for command::runtime::SHOWCHARS {
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        let previous = state.system_controls_allowed;
        state.system_controls_allowed = false;
        previous
    }
    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        previous_system_controls: bool,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state.playback_modes.reset();
        let grid = vm_state.information.character_grid;
        let initial_page = vm_state.information.character_grid_argument;
        let error = match CharacterScreen::new(context.asset_server, scenario, grid, initial_page) {
            Ok(screen) => {
                adv_state.character_screen = Some(screen);
                None
            }
            Err(error) => Some(error),
        };
        Yield(
            SHOWCHARS {
                token: Some(self.token),
                previous_system_controls,
                grid,
                initial_page,
                error,
            }
            .into(),
        )
    }
}
impl UpdatableCommand for SHOWCHARS {
    fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }
    fn update(
        &mut self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.error.is_some() {
            return None;
        }
        if adv_state.character_screen.is_none() {
            match CharacterScreen::new(context.asset_server, scenario, self.grid, self.initial_page)
            {
                Ok(screen) => adv_state.character_screen = Some(screen),
                Err(error) => self.error = Some(error),
            }
            return None;
        }
        if !adv_state.character_screen.as_ref()?.model.is_closed() {
            return None;
        }
        adv_state.character_screen = None;
        vm_state.system_controls_allowed = self.previous_system_controls;
        Some(self.token.take().unwrap().finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::vm::IntoRuntimeForm;
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_showchars_gate_callback_and_task_timing() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, opcode) in [
            (0x47f8, 0x386a6a6bu32),
            (0x4804, 0x38296a6bu32),
            (0x4808, 0x382a6a7fu32),
            (0xe348, 0x39406c09u32),
            (0xe358, 0x52922788u32),
            (0xe35c, 0x38286949u32),
            (0xe3d0, 0x39406408u32),
            (0xe41c, 0xb8696902u32),
            (0xe430, 0xb8696903u32),
            (0xe43c, 0x94011b31u32),
            (0x538fc, 0x94019ec9u32),
            (0x53904, 0xbd45a500u32),
            (0x5390c, 0xbd497508u32),
            (0x53914, 0x321a03e1u32),
            (0x5391c, 0x9401a039u32),
            (0x53930, 0x9401a034u32),
            (0x557f4, 0x1e249008u32),
            (0x557fc, 0x321f03e1u32),
            (0x5580c, 0x9401987du32),
            (0x56074, 0x9401965fu32),
            (0x56078, 0x360000a0u32),
            (0x56090, 0x97fff66cu32),
            (0x561a8, 0xb900e67fu32),
            (0x561c4, 0xd63f0100u32),
            (0x561d0, 0x3902c268u32),
            (0x50888, 0x1e2703e0u32),
            (0x50894, 0x9401aa93u32),
            (0x55c2c, 0x39002928u32),
            (0x55c30, 0x97fff41cu32),
        ] {
            assert_eq!(word(a), opcode, "native SHOWCHARS{a:x}");
        }
        for (a, v) in [
            (0x1595a4, 0.7f32),
            (0x159974, 0.2),
            (0x159570, 0.1),
            (0x15999c, -0.1),
        ] {
            assert_eq!(word(a), v.to_bits());
        }
    }

    #[test]
    fn live_gate_captures_previous_permission_and_completed_replay_preserves_it() {
        for permission in [true, false] {
            let runtime = command::compiletime::SHOWCHARS {}
                .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
            let mut state = VmState::new();
            state.system_controls_allowed = permission;
            let previous = runtime.apply_state(&mut state);
            assert_eq!(previous, permission);
            assert!(!state.system_controls_allowed);
            state.system_controls_allowed = permission;
            super::super::apply_command_state_ref(
                &command::RuntimeCommand::SHOWCHARS(runtime),
                &mut state,
            )
            .unwrap();
            assert_eq!(state.system_controls_allowed, permission);
        }
    }
}
