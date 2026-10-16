//! Message-free private resource scene for SaveMenu's D6AA0 worker.
//! Registered as a sibling of `save_menu` so its pure controller stays portable.
use super::{
    card_select::CardSelectModel,
    layer_factory::{GerDrawable, LayerFactoryRequest, PreparedBasicDrawable},
    layers::{Gerokasu2Layers, SpecialLayer},
    reconstruction::apply_properties,
    root::Gerokasu2Root,
    save_menu::preview::{Descriptor, scene_descriptor},
};
use crate::{
    asset::{mask::MaskTexture, system::AssetServer},
    layer::DrawableLayer,
};
use anyhow::{Result, ensure};
use enum_map::Enum;
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario,
    vm::command::types::{LayerId, LayerProperty, LayerbankId, MaskFlags, PlaneId},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn values(state: &Gerokasu2Layers, special: SpecialLayer) -> [i32; 91] {
    std::array::from_fn(|index| state.special_property(special, LayerProperty::from_usize(index)))
}

fn fresh_root(state: &Gerokasu2Layers) -> Gerokasu2Root {
    let mut root = Gerokasu2Root::new();
    apply_properties(root.properties_mut(), values(state, SpecialLayer::Root));
    apply_properties(
        root.screen_mut().properties_mut(),
        values(state, SpecialLayer::Screen),
    );
    apply_properties(
        root.screen_mut().page_layer_mut().properties_mut(),
        values(state, SpecialLayer::Page),
    );
    root
}

fn reset_plane(root: &mut Gerokasu2Root, state: &Gerokasu2Layers, id: PlaneId) {
    let plane = root.screen_mut().page_layer_mut().get_plane_mut(id);
    apply_properties(
        plane.properties_mut(),
        values(state, SpecialLayer::Plane(id)),
    );
    plane.clear_layers();
    plane.clear_mask_texture();
}

/// CardSelect virtual50 (2FD20), before the optional completed-result setter.
/// Unlike virtual30 fast-forward, it resets only these authored presentation
/// values and leaves controller phase, selection and other tweens untouched.
fn prepare_card(card: &mut CardSelectModel) {
    for item in &mut card.cards {
        let y = item.tweens[3].target_value() + -300.0;
        item.tweens[3].fast_forward_to(y);
        item.tweens[4].fast_forward_to(2.0);
        item.tweens[6].fast_forward_to(1.0);
    }
    card.caption_y.fast_forward_to(0.0);
    card.caption_alpha.fast_forward_to(1.0);
}

/// Prepare privately; this function never attaches a live Message, publishes
/// audio/gallery events, reuses generation caches or installs transitions.
/// The capture host must update the root once at zero ticks before pre-render.
pub(crate) async fn prepare(
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    state: Arc<Gerokasu2Layers>,
    cancelled: Arc<AtomicBool>,
) -> Result<Gerokasu2Root> {
    let mut root = fresh_root(&state);
    // Original loads all plane masks before acquiring the first bank resource.
    for index in 0..4 {
        let id = PlaneId::new(index);
        reset_plane(&mut root, &state, id);
        let [mask, flags] = state.plane_mask(id);
        if let Some(info) = usize::try_from(mask)
            .ok()
            .and_then(|index| scenario.mask_info.get(index))
        {
            let texture = assets
                .load::<MaskTexture, _>(&format!("/mask/{}.msk", info.name.as_str()))
                .await?;
            root.screen_mut()
                .page_layer_mut()
                .get_plane_mut(id)
                .set_mask_texture(texture, MaskFlags::from_bits_retain(flags));
            ensure!(
                !cancelled.load(Ordering::Acquire),
                "Ger preview cancelled after mask acquisition"
            );
        }
    }
    for index in 0..48 {
        let bank = LayerbankId::new(index);
        let record = state.record(bank);
        if record.kind() < 0 {
            continue;
        }
        let descriptor = scene_descriptor(
            Descriptor {
                kind: record.kind(),
                parameters: record.parameters().try_into().unwrap(),
            },
            |id| {
                usize::try_from(id)
                    .ok()
                    .and_then(|index| scenario.movie_info.get(index))
                    .map(|info| info.linked_picture_id)
            },
        );
        let p = descriptor.parameters;
        let Some(request) = LayerFactoryRequest::resolve(
            &scenario,
            descriptor.kind,
            (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]),
        ) else {
            // Native nil factory skips this bank, distinct from a Null layer.
            continue;
        };
        let resource = PreparedBasicDrawable::load(&assets, &request).await?;
        // Original dereferences a nil job value here. Return a desktop error
        // so the retained menu can publish its explicit failed-preview fallback.
        let mut layer = resource.try_construct()?;
        ensure!(
            !cancelled.load(Ordering::Acquire),
            "Ger preview cancelled after bank acquisition"
        );
        apply_properties(
            layer.properties_mut(),
            std::array::from_fn(|index| record.property(LayerProperty::from_usize(index))),
        );
        let logical = LayerId::try_new(u16::try_from(record.logical_id())?)
            .ok_or_else(|| anyhow::anyhow!("Ger preview logical layer ID is invalid"))?;
        layer.properties_mut().set_layer_id(logical);
        // Native does not copy the saved generation into this private owner.
        if let GerDrawable::Card(card) = &mut layer.layer {
            let mut model = card.inner_ref().model.lock();
            prepare_card(&mut model);
            if record.interaction_completed() {
                model.restore_result(record.interaction_result());
            }
        }
        // Animation virtual90 is a no-op for Sakura. Native Rain performs its
        // derived warmup here; the shared Rain owner does that before its first
        // zero-tick capture update. No pixels are prepared before that update.
        ensure!(
            (0..4).contains(&record.plane()),
            "Ger preview plane is invalid"
        );
        root.screen_mut()
            .page_layer_mut()
            .get_plane_mut(PlaneId::new(record.plane() as u8))
            .add_layer(bank, layer);
    }
    Ok(root)
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
