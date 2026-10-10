use super::prelude::*;

impl StartableCommand for command::runtime::TIPSGET {
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        state.information.acquire_tips(&self.tip_ids)
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
                crate::adv::information::NotificationKind::Tips,
                vm_state.information.notification_style,
            );
        }
        self.token.finish().into()
    }
}
