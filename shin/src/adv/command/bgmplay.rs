use std::ops::Not;

use shin_core::{format::scenario::info::BgmInfoItem, time::Tween};

use super::prelude::*;
use crate::adv::vm_state::audio::BgmState;

impl StartableCommand for command::runtime::BGMPLAY {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.audio.bgm = self.no_repeat.not().then_some(BgmState {
            bgm_id: self.bgm_data_id,
            volume: self.volume,
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
        let bgm_info @ BgmInfoItem {
            name: _,
            display_name,
            linked_bgm_id: _,
        } = scenario.info_tables().bgm_info(self.bgm_data_id);

        let changed = adv_state.audio.bgm_player.request(
            context.asset_server.clone(),
            bgm_info.path().to_owned(),
            self.bgm_data_id,
            !self.no_repeat,
            self.volume,
            Tween::linear(self.fade_in_time),
        );

        if changed {
            adv_state
                .bgm_title
                .show(context.asset_server, display_name.as_str());
            adv_state
                .pending_gallery_unlocks
                .extend(crate::adv::gallery::played_bgm(
                    scenario.info_tables(),
                    self.bgm_data_id.into_i32(),
                ));
        }
        self.token.finish().into()
    }
}
