use super::prelude::*;
use crate::audio::VoicePlayFlags;

impl StartableCommand for command::runtime::VOICEPLAY {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {
        // Voices are transient and are not restarted by state replay.
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state.message_layer_mut().voice_player_mut().play(
            scenario,
            &self.name,
            0,
            0,
            VoicePlayFlags::from_bits_truncate(self.flags),
            self.volume,
        );
        self.token.finish().into()
    }
}
