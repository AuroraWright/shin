use std::sync::Arc;

use anyhow::Result;
use shin_audio::AudioManager;
use shin_core::{
    format::{audio::AudioFile, scenario::Scenario},
    time::{Ticks, Tween},
    vm::command::types::{LayerId, LayerbankId, MaskFlags, PlaneId},
};

use super::{AdvState, VmState};
use crate::{
    asset::{mask::MaskTexture, system::AssetServer},
    layer::{
        DrawableLayer, LayerProperties,
        message_layer::{MessageFlags, MsgsetParams},
        user::UserLayer,
    },
    render::PreRenderContext,
};

pub(super) struct LoadedScene {
    layers: Vec<(LayerbankId, UserLayer)>,
    masks: Vec<(PlaneId, Arc<MaskTexture>, MaskFlags)>,
    bgm: Option<Arc<AudioFile>>,
    effects: Vec<(usize, Arc<AudioFile>)>,
    reused: Vec<ReusedLayer>,
}

pub(super) struct LoadedAudio {
    bgm: Option<Arc<AudioFile>>,
    effects: Vec<(usize, Arc<AudioFile>)>,
    reused: Vec<ReusedLayer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReusedLayer {
    target: LayerbankId,
    source_plane: PlaneId,
    source: LayerbankId,
}

/// Native 1bae0 consumes the first matching load counter in the retained
/// plane/child scan order. A source can supply only one target layer.
fn select_reused_layers(
    sources: &[(PlaneId, LayerbankId, u32)],
    targets: impl IntoIterator<Item = (LayerbankId, u32)>,
) -> Vec<ReusedLayer> {
    let mut available = sources.iter().copied().map(Some).collect::<Vec<_>>();
    targets
        .into_iter()
        .filter_map(|(target, counter)| {
            let source = available
                .iter_mut()
                .find(|entry| entry.is_some_and(|(_, _, value)| value == counter))?
                .take()
                .unwrap();
            Some(ReusedLayer {
                target,
                source_plane: source.0,
                source: source.1,
            })
        })
        .collect()
}

pub(super) fn reuse_plan(adv: &AdvState, state: &VmState) -> Vec<ReusedLayer> {
    let mut sources = Vec::new();
    for (plane, _) in &state.layers.plane_layergroups {
        let group = adv.plane_layer_group(plane);
        for bank in group.get_used_layerbank_ids() {
            let layer = group.get_layer(bank).unwrap();
            sources.push((plane, bank, layer.properties().layer_load_counter()));
        }
    }
    select_reused_layers(
        &sources,
        state
            .layers
            .layerbanks
            .iter()
            .filter_map(|(bank, layer)| layer.layer_type.map(|_| (bank, layer.layer_load_counter))),
    )
}

fn restore_completed_answer(layer: &UserLayer, saved: &super::vm_state::layers::LayerbankState) {
    if saved.is_interation_completed
        && let Some(answer) = saved.interaction_result
    {
        match layer {
            UserLayer::Quiz(quiz) => quiz.restore_completed_answer(answer),
            UserLayer::Cake(cake) => cake.restore_completed_answer(answer),
            UserLayer::Last(last) => last.restore_completed_answer(answer),
            UserLayer::Murderer(view) => view.restore_completed_answer(answer),
            _ => {}
        }
    }
}

impl LoadedScene {
    #[cfg(test)]
    pub(super) fn reference_layers(&self) -> &[(LayerbankId, UserLayer)] {
        &self.layers
    }
    #[cfg(test)]
    pub(super) fn reference_mask_owners(
        &self,
    ) -> Vec<(PlaneId, std::sync::Weak<MaskTexture>, MaskFlags)> {
        self.masks
            .iter()
            .map(|(plane, mask, flags)| (*plane, Arc::downgrade(mask), *flags))
            .collect()
    }

    pub async fn load(
        server: &AssetServer,
        audio: &Arc<AudioManager>,
        scenario: &Scenario,
        state: &VmState,
    ) -> Result<Self> {
        Self::load_with_reuse(server, audio, scenario, state, Vec::new()).await
    }

    pub(super) async fn load_with_reuse(
        server: &AssetServer,
        audio: &Arc<AudioManager>,
        scenario: &Scenario,
        state: &VmState,
        reused: Vec<ReusedLayer>,
    ) -> Result<Self> {
        let mut loaded = Self {
            layers: Vec::new(),
            masks: Vec::new(),
            bgm: None,
            effects: Vec::new(),
            reused,
        };
        for (bank, layer) in &state.layers.layerbanks {
            if let Some(ty) = layer.layer_type {
                if loaded.reused.iter().any(|entry| entry.target == bank) {
                    continue;
                }
                let asset = UserLayer::load(
                    &server.context().wgpu_device,
                    server,
                    audio,
                    scenario,
                    (&state.settings).into(),
                    ty,
                    layer.params,
                )
                .await;
                loaded.layers.push((bank, asset));
            }
        }
        for (plane, group) in &state.layers.plane_layergroups {
            if let Some(mask) = group
                .mask_id
                .repr()
                .and_then(|id| scenario.info_tables().mask_info_opt(id))
            {
                let texture = server.load(mask.path()).await?;
                loaded.masks.push((plane, texture, group.mask_flags));
            }
        }
        if let Some(bgm) = state.audio.bgm {
            loaded.bgm = Some(
                server
                    .load(scenario.info_tables().bgm_info(bgm.bgm_id).path())
                    .await?,
            );
        }
        for (slot, effect) in state.audio.se.iter().enumerate() {
            if let Some(effect) = effect {
                let audio = server
                    .load(scenario.info_tables().se_info(effect.se_id).path())
                    .await?;
                loaded.effects.push((slot, audio));
            }
        }
        Ok(loaded)
    }

    pub fn install(
        self,
        adv: &mut AdvState,
        state: &VmState,
        scenario: &Arc<Scenario>,
        ctx: &PreRenderContext,
    ) -> LoadedAudio {
        for (bank, mut layer) in self.layers {
            let saved = &state.layers.layerbanks[bank];
            // Native 1a754..1a778 restores completed interactive layers after
            // constructing their resources, before publishing the scene.
            restore_completed_answer(&layer, saved);
            layer.set_properties(LayerProperties::from_state(
                &saved.properties,
                saved.layer_id,
                saved.layer_load_counter,
            ));
            adv.plane_layer_group_mut(saved.plane)
                .add_layer(bank, layer);
        }
        for (plane, mask, flags) in self.masks {
            adv.plane_layer_group_mut(plane)
                .set_mask_texture(mask, flags);
        }
        adv.apply_audio_settings(&state.settings);
        restore_message(adv, state, scenario, ctx);
        LoadedAudio {
            bgm: self.bgm,
            effects: self.effects,
            reused: self.reused,
        }
    }
}

impl LoadedAudio {
    /// Native1a278..1a404 resets seven live property owners in place. Validate
    /// the new command before touching them, so rejected candidates are atomic.
    pub(super) fn install_container_properties(
        &mut self,
        source: &AdvState,
        target: &mut AdvState,
        state: &VmState,
    ) {
        fn restore(
            source: &impl DrawableLayer,
            target: &mut impl DrawableLayer,
            state: &crate::layer::LayerPropertiesState,
        ) {
            let mut properties = source.properties().retain_for_clone();
            properties.restore_values(state);
            target.set_properties(properties);
        }
        restore(
            source.root_layer_group(),
            target.root_layer_group_mut(),
            &state.layers.root_layer_group,
        );
        restore(
            source.screen_layer(),
            target.screen_layer_mut(),
            &state.layers.screen_layer,
        );
        restore(
            source.page_layer(),
            target.page_layer_mut(),
            &state.layers.page_layer,
        );
        for (plane, group) in &state.layers.plane_layergroups {
            restore(
                source.plane_layer_group(plane),
                target.plane_layer_group_mut(plane),
                &group.properties,
            );
        }
    }

    /// Move the exact retained drawables only after command-start validation.
    /// Pending/failing workers never remove resources from the live source.
    pub(super) fn install_reused(
        &mut self,
        source: &mut AdvState,
        target: &mut AdvState,
        state: &VmState,
    ) {
        for entry in self.reused.drain(..) {
            let mut layer = source
                .plane_layer_group_mut(entry.source_plane)
                .remove_layer(entry.source, Ticks::ZERO)
                .expect("Retained scene layer remains owned until publication");
            let saved = &state.layers.layerbanks[entry.target];
            layer.properties_mut().restore_state(
                &saved.properties,
                saved.layer_id,
                saved.layer_load_counter,
            );
            restore_completed_answer(&layer, saved);
            target
                .plane_layer_group_mut(saved.plane)
                .add_layer(entry.target, layer);
        }
    }
    /// Apply only after command-start validation. The live global players and
    /// their source positions/tails have moved into the reconstructed scene.
    pub fn install(self, adv: &mut AdvState, state: &VmState, scenario: &Arc<Scenario>) {
        adv.audio
            .bgm_player
            .set_settings_volume(state.settings.v0_bgmvol);
        adv.audio
            .se_player
            .set_settings_volume(state.settings.v1_sfxvol);
        adv.audio.se_player.prepare_restore();
        for (slot, saved) in state.audio.se.iter().enumerate() {
            if saved.is_none() {
                adv.audio.se_player.restore_empty_slot(slot);
            }
        }
        if let Some(bgm) = self.bgm {
            let saved = state.audio.bgm.unwrap();
            let name = &scenario.info_tables().bgm_info(saved.bgm_id).display_name;
            adv.audio.bgm_player.play(
                saved.bgm_id,
                bgm,
                name.as_str(),
                true,
                saved.volume,
                Tween::IMMEDIATE,
            );
        }
        for (slot, audio) in self.effects {
            let saved = state.audio.se[slot].unwrap();
            adv.audio.se_player.play(
                slot as i32,
                saved.se_id,
                audio,
                true,
                saved.volume,
                saved.pan,
                saved.play_speed,
                Tween::IMMEDIATE,
            );
        }
        if state.audio.bgm.is_none() {
            adv.audio.bgm_player.restore_empty();
        }
        adv.audio.se_player.set_paused(adv.audio_pause_count != 0);
    }
}

pub(super) fn restore_message(
    adv: &mut AdvState,
    state: &VmState,
    scenario: &Arc<Scenario>,
    ctx: &PreRenderContext,
) {
    adv.message_layer_mut().set_settings(&state.settings);
    let message = &state.messagebox_state;
    if message.messagebox_shown {
        if let Some(text) = &message.text {
            let mut flags = if state.current_event.is_some() {
                MessageFlags::IGNORE_INPUT
            } else {
                MessageFlags::empty()
            };
            if message.was_read {
                flags |= MessageFlags::READ;
            }
            adv.message_layer_mut().restore_message(
                ctx,
                scenario,
                text,
                MsgsetParams {
                    flags,
                    messagebox_type: message.msginit.messagebox_type,
                    text_layout: message.msginit.text_layout,
                    message_id: message.message_id,
                },
                message.received_syncs,
            );
        }
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::*;

    #[test]
    fn cache_selection_matches_complete_native_worker_and_lookup() {
        let lines =
            include_str!("../../../reverse-engineering/native-save-hydration-cache-golden.txt")
                .lines()
                .collect::<Vec<_>>();
        assert_eq!(lines.len(), 512 * 2);
        for pair in lines.chunks_exact(2) {
            let data = pair[0]
                .split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(data.len(), 98);
            let sources = data[2..50]
                .iter()
                .enumerate()
                .map(|(i, counter)| {
                    (
                        PlaneId::new((i / 12) as u8),
                        LayerbankId::new(i as u8),
                        *counter,
                    )
                })
                .collect::<Vec<_>>();
            let targets = data[50..]
                .iter()
                .enumerate()
                .map(|(i, counter)| (LayerbankId::new(i as u8), *counter));
            let plan = select_reused_layers(&sources, targets);
            let mut actual = [-1i32; 48];
            for entry in plan {
                actual[entry.target.raw() as usize] = entry.source.raw() as i32;
                assert_eq!(
                    entry.source_plane.raw() as usize,
                    entry.source.raw() as usize / 12
                );
            }
            let expected = pair[1]
                .split_whitespace()
                .map(|v| v.parse::<i32>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                actual.as_slice(),
                expected,
                "native case{} profile{}",
                data[0],
                data[1]
            );
        }
    }
}
