//! Resource preparation and target publication for a completed memory replay.
use super::{
    drawables::BasicLoadBackend,
    layer_factory::{GerDrawable, LayerFactoryRequest, PreparedBasicDrawable},
    layer_load::{LoadCounters, LoadServices, LoadTransition},
    layers::{Gerokasu2Layers, SpecialLayer},
};
use crate::{
    asset::{mask::MaskTexture, system::AssetServer},
    layer::{DrawableLayer, LayerProperties, LayerPropertiesState, PropertyPolicy},
};
use anyhow::{Result, ensure};
use enum_map::Enum;
use shin_core::vm::command::types::{LayerId, LayerProperty, LayerbankId, MaskFlags, PlaneId};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::sync::Arc;

/// Native 1B8D0 consumes the first matching resource, including a zero/wrapped
/// generation. Kind/parameters are not part of this lookup.
pub fn consume_cache_index(cache: &mut [Option<u32>], generation: u32) -> Option<usize> {
    let index = cache.iter().position(|value| *value == Some(generation))?;
    cache[index] = None;
    Some(index)
}

/// Native 986C4 sets current/target and clears each tween queue. Retain the
/// property owner and oscillators, as the native container/publication does.
pub fn apply_properties(properties: &mut LayerProperties, values: [i32; 90]) {
    let mut state = LayerPropertiesState::new();
    for (index, value) in values.into_iter().enumerate() {
        state.set_property(LayerProperty::from_usize(index), value);
    }
    properties.restore_values(&state);
}
fn special(state: &Gerokasu2Layers, layer: SpecialLayer) -> [i32; 90] {
    std::array::from_fn(|index| state.special_property(layer, LayerProperty::from_usize(index)))
}

impl BasicLoadBackend {
    /// Prepare a private drawable tree before retiring the old runner. Existing
    /// scene transitions and pending pages need the reconstruction mode owner.
    pub async fn prepare_reconstruction(
        &self,
        state: &Gerokasu2Layers,
        descriptors: &super::descriptors::Descriptors,
        reuse_cache: bool,
        cancelled: impl Fn() -> bool,
    ) -> Result<Box<Self>> {
        ensure!(
            !self.reconstruction_has_jobs(),
            "Ger asset jobs must retire before reconstruction"
        );
        ensure!(!cancelled(), "Ger reconstruction cancelled before assets");
        let assets: Arc<AssetServer> = self.reconstruction_assets();
        let scenario = self.reconstruction_scenario();
        let audio = self.reconstruction_audio();
        let device = assets.context().wgpu_device.clone();
        let mut encoder = device.create_command_encoder(&Default::default());
        let (root, mut cache) = {
            let mut context = RenderCloneCtx {
                device: &device,
                encoder: &mut encoder,
            };
            self.reconstruction_snapshot(&mut context, reuse_cache)
        };
        assets.context().wgpu_queue.submit([encoder.finish()]);
        let mut generations: Vec<_> = cache
            .iter()
            .map(|owner| {
                owner
                    .as_ref()
                    .map(|layer| layer.properties().layer_load_counter())
            })
            .collect();
        let mut prepared = Box::new(Self::new(
            assets.clone(),
            scenario.clone(),
            LoadCounters::from_layers(state),
        ));
        if let Some(audio) = audio.as_ref() {
            prepared.attach_movie_audio(audio.clone());
        }
        prepared.set_movie_volume_settings(self.movie_volume_settings());
        prepared.reconstruction_set_root(root);
        prepared.reconstruction_root_mut().remove_selection();
        prepared.screen_mut().reset_page_for_reconstruction();
        for plane in prepared.screen_mut().page_layer_mut().planes_mut() {
            plane.clear_layers();
            plane.clear_mask_texture();
        }
        apply_properties(
            prepared.reconstruction_root_mut().properties_mut(),
            special(state, SpecialLayer::Root),
        );
        apply_properties(
            prepared.screen_mut().properties_mut(),
            special(state, SpecialLayer::Screen),
        );
        apply_properties(
            prepared.screen_mut().page_layer_mut().properties_mut(),
            special(state, SpecialLayer::Page),
        );
        for bank in 0..48 {
            let bank = LayerbankId::new(bank);
            let record = state.record(bank);
            if record.kind() < 0 {
                continue;
            }
            ensure!(
                (0..4).contains(&record.plane()),
                "Ger replay bank plane is invalid"
            );
            let logical = LayerId::try_new(u16::try_from(record.logical_id())?)
                .ok_or_else(|| anyhow::anyhow!("Ger replay logical layer ID is invalid"))?;
            let cached = consume_cache_index(&mut generations, record.initialization_id());
            let mut layer = if let Some(index) = cached {
                cache[index]
                    .take()
                    .expect("consumed generation has a cached owner")
            } else {
                let p = record.parameters();
                let request = LayerFactoryRequest::resolve(
                    &scenario,
                    record.kind(),
                    (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]),
                );
                let Some(request) = request else {
                    continue;
                };
                let resource = if matches!(request, LayerFactoryRequest::Movie { .. }) {
                    PreparedBasicDrawable::Movie(parking_lot::Mutex::new(
                        super::movie::load(
                            &assets,
                            audio.as_ref().ok_or_else(|| {
                                anyhow::anyhow!("Ger reconstructed movie has no audio manager")
                            })?,
                            &request,
                            prepared.movie_volume_settings(),
                        )
                        .await?,
                    ))
                } else {
                    PreparedBasicDrawable::load(&assets, &request).await?
                };
                if !resource.has_drawable() {
                    ensure!(
                        !cancelled(),
                        "Ger reconstruction cancelled after absent factory value"
                    );
                    continue;
                }
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut context = RenderCloneCtx {
                    device: &device,
                    encoder: &mut encoder,
                };
                let mut layer = resource
                    .construct_movie(&mut context)
                    .unwrap_or_else(|| resource.construct());
                // Live LAYERLOAD installs fresh Ger-policy properties; a
                // constructed (uncached) owner needs the same policy.
                layer.set_properties(LayerProperties::with_policy(PropertyPolicy::Gerokasu2));
                assets.context().wgpu_queue.submit([encoder.finish()]);
                ensure!(
                    !cancelled(),
                    "Ger reconstruction cancelled after asset acquisition"
                );
                layer
            };
            apply_properties(
                layer.properties_mut(),
                std::array::from_fn(|index| record.property(LayerProperty::from_usize(index))),
            );
            layer.properties_mut().set_layer_id(logical);
            layer
                .properties_mut()
                .set_layerload_counter1(record.initialization_id());
            if let Some(model) = layer.layer.interaction()
                && record.interaction_completed()
            {
                model.lock().restore_result(record.interaction_result());
            }
            prepared
                .screen_mut()
                .page_layer_mut()
                .get_plane_mut(PlaneId::new(record.plane() as u8))
                .add_layer(bank, layer);
            prepared.loaded_layer(record.kind(), record.parameters().try_into().unwrap())?;
        }
        for index in 0..4 {
            let id = PlaneId::new(index);
            let [mask, flags] = state.plane_mask(id);
            let texture = match usize::try_from(mask)
                .ok()
                .and_then(|i| scenario.mask_info.get(i))
            {
                Some(info) => Some(
                    assets
                        .load::<MaskTexture, _>(&format!("/mask/{}.msk", info.name.as_str()))
                        .await?,
                ),
                None => None,
            };
            let plane = prepared.screen_mut().page_layer_mut().get_plane_mut(id);
            apply_properties(
                plane.properties_mut(),
                special(state, SpecialLayer::Plane(id)),
            );
            if let Some(texture) = texture {
                plane.set_mask_texture(texture, MaskFlags::from_bits_retain(flags));
                ensure!(
                    !cancelled(),
                    "Ger reconstruction cancelled during mask acquisition"
                );
            }
        }
        let t = descriptors.default_transition;
        prepared.set_transition(LoadTransition {
            kind: t[0],
            flags: t[1],
            duration: t[2],
            params: t[3..].try_into().unwrap(),
        });
        Ok(prepared)
    }
}

#[cfg(test)]
mod enumeration_tests;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod retained_tests;
#[cfg(test)]
mod tests;
