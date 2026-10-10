use super::prelude::*;

impl StartableCommand for command::runtime::CHARS {
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        state
            .information
            .select_character_grid(self.arg1, self.arg2)
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &VmState,
        _changed: bool,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if vm_state.persistent_commits_enabled {
            adv_state.notifications.enqueue(
                crate::adv::information::NotificationKind::Characters,
                vm_state.information.notification_style,
            );
        }
        self.token.finish().into()
    }
}
