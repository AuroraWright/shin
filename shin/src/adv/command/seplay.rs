use std::ops::Not;

use shin_core::time::Tween;

use super::prelude::*;
use crate::adv::vm_state::audio::SeState;

impl StartableCommand for command::runtime::SEPLAY {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.audio.se[self.se_slot as usize] = self.no_repeat.not().then_some(SeState {
            se_id: self.se_data_id,
            volume: self.volume,
            pan: self.pan,
            play_speed: self.play_speed as f32 * 0.001,
        });
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let se_info = scenario.info_tables().se_info(self.se_data_id);

        adv_state.audio.se_player.request(
            context.asset_server.clone(),
            se_info.path().to_owned(),
            self.se_slot,
            self.se_data_id,
            !self.no_repeat,
            self.volume,
            self.pan,
            self.play_speed as f32 * 0.001,
            Tween::linear(self.fade_in_time),
        );

        self.token.finish().into()
    }
}
