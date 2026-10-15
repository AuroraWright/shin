//! Exercise the production Group scheduler, using whole actual layer owners.
use super::*;
use crate::{
    asset::system::AssetServer,
    layer::{
        owner::LayerOwner,
        user::{EffectLayer, TileLayer},
    },
    wiper::DefaultWiper,
};
use shin_core::{primitives::update::FrameId, time::Tween};

fn set(layer: &mut UserLayer, property: LayerProperty, value: f32) {
    layer
        .properties_mut()
        .property_tweener_mut(property)
        .fast_forward_to(value);
}
fn pending(owner: &LayerOwner<UserLayer>) -> Vec<crate::layer::owner::RetainedLayer> {
    let layer = owner.read();
    let UserLayer::Effect(effect) = &*layer else {
        panic!("actual Effect variant")
    };
    effect.reference_pending_children().to_vec()
}
fn effect(position: f32) -> UserLayer {
    let mut effect: UserLayer = EffectLayer::new().into();
    set(&mut effect, LayerProperty::RenderPosition, position);
    set(&mut effect, LayerProperty::ShowLayer, 0.0);
    effect
}
fn tile() -> UserLayer {
    let mut tile: UserLayer = TileLayer::new(
        FloatColor4::WHITE,
        glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
    )
    .into();
    set(&mut tile, LayerProperty::RenderPosition, 2000.0);
    tile
}

impl LayerGroup {
    pub(crate) fn reference_integrated_owned_capture(
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
    ) {
        let bank = LayerbankId::new(0);
        let effect_bank = LayerbankId::new(2);
        let mut final_owners = 0;
        for kind in 0..4 {
            for clear_by_recapture in [false, true] {
                let mut group = Self::new(None);
                group.add_layer(bank, tile());
                let mut weak: Vec<Box<dyn Fn() -> usize>> =
                    vec![Box::new(group.layer_owner(bank).unwrap().reference_weak())];
                // kind 1: active wrapper; kind 2: nested active wrappers;
                // kind 3: a second invisible Effect captures the first Effect.
                for _ in 0..kind.min(2) {
                    if kind == 3 {
                        break;
                    }
                    weak.push(Box::new(
                        group.layers[0]
                            .layer
                            .shared_owner()
                            .unwrap()
                            .reference_weak(),
                    ));
                    group.add_layer_with_wiper(
                        bank,
                        tile(),
                        Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
                    );
                    weak.push(Box::new(group.layer_owner(bank).unwrap().reference_weak()));
                }
                let target = group.layer_owner(bank).unwrap();
                target
                    .write()
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .enqueue_now(10.0, Tween::linear(Ticks::from_f32(10.0)));
                let wrapper = group.layers[0].layer.shared_owner().unwrap();
                weak.push(Box::new(wrapper.reference_weak()));
                let selected_identity = if kind == 0 || kind == 3 {
                    target.reference_identity()
                } else {
                    wrapper.reference_identity()
                };
                group.add_layer(effect_bank, effect(1000.0));
                let first = group.layer_owner(effect_bank).unwrap();
                weak.push(Box::new(first.reference_weak()));
                let last = if kind == 3 {
                    group.add_layer(LayerbankId::new(3), effect(500.0));
                    let last = group.layer_owner(LayerbankId::new(3)).unwrap();
                    weak.push(Box::new(last.reference_weak()));
                    last
                } else {
                    first.clone()
                };
                group.pre_render(context, &TransformParams::default());
                {
                    let children = pending(&first);
                    assert_eq!(children.len(), 1);
                    assert_eq!(children[0].reference_identity(), selected_identity);
                }
                if kind == 3 {
                    let children = pending(&last);
                    assert_eq!(children.len(), 1);
                    assert_eq!(children[0].reference_identity(), first.reference_identity());
                }
                let copy = group.render_clone(&mut context.render_clone_ctx());
                assert!(pending(&copy.layer_owner(effect_bank).unwrap()).is_empty());
                if kind == 3 {
                    assert!(pending(&copy.layer_owner(LayerbankId::new(3)).unwrap()).is_empty());
                }
                drop(copy);
                let mut frame = FrameId::default();
                frame.advance();
                let update = AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(5.0),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: true,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                };
                group.update(&update);
                assert_eq!(
                    target
                        .read()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    5.0
                );
                // Retained dispatch uses the same frame guard and mutable cell.
                let mut selected = pending(&first).pop().unwrap();
                selected.update(&update);
                assert_eq!(
                    target
                        .read()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    5.0
                );
                drop(selected);
                group.remove_layer(bank, Ticks::ZERO);
                group.add_layer(bank, tile()); // The old captured owner must not redirect to this bank.
                assert_ne!(
                    group.layer_owner(bank).unwrap().reference_identity(),
                    target.reference_identity()
                );
                pending(&first)[0].clone().fast_forward();
                assert_eq!(
                    target
                        .read()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    10.0
                );
                assert_eq!(
                    group
                        .get_layer(bank)
                        .unwrap()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    0.0
                );
                // The ordinary wrapper is not a captured native child; only its target is.
                let wrapper_weak = wrapper.reference_weak();
                drop(wrapper);
                if kind == 0 || kind == 3 {
                    assert_eq!(wrapper_weak(), 0);
                }
                drop(target);
                drop(first);
                group.clear_layers();
                assert!(!pending(&last).is_empty());
                if clear_by_recapture {
                    last.write().pre_render_with_owned_lower(
                        context,
                        &TransformParams::default(),
                        &[],
                    );
                    assert!(pending(&last).is_empty());
                }
                drop(last);
                drop(group); // Retire the owning prepared render list as well.
                assert!(
                    weak.iter().all(|probe| probe() == 0),
                    "kind{kind},recapture{clear_by_recapture}"
                );
                final_owners += weak.len();
            }
        }
        println!(
            "Integrated Group owned capture:8 cases, ordinary/active/nested wrappers and nested Effects, exact selected identities, bank reuse, same-cell mutation, once-only updates, copy reset, recapture/destruction; {final_owners} final Weak probes"
        );
    }
}
