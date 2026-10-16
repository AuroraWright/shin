use super::*;

#[test]
fn ger_preview_private_hierarchy_restores_properties_without_live_owners() {
    let mut state = Gerokasu2Layers::default();
    for owner in -3..=-1 {
        for index in 0..90 {
            state
                .control(owner, index, owner * 100_003 + index * 71, 0)
                .unwrap();
        }
    }
    for plane in 0..4 {
        state.select_plane(plane).unwrap();
        for index in 0..90 {
            state
                .control(-4, index, plane * 997 + index * 23, 0)
                .unwrap();
        }
    }
    let mut root = fresh_root(&state);
    for index in 0..4 {
        reset_plane(&mut root, &state, PlaneId::new(index));
    }
    assert!(!root.has_message());
    assert!(!root.screen().has_pending_page());
    assert!(!root.screen().is_transition_active());
    for owner in [
        SpecialLayer::Root,
        SpecialLayer::Screen,
        SpecialLayer::Page,
        SpecialLayer::Plane(PlaneId::new(0)),
        SpecialLayer::Plane(PlaneId::new(1)),
        SpecialLayer::Plane(PlaneId::new(2)),
        SpecialLayer::Plane(PlaneId::new(3)),
    ] {
        let props = match owner {
            SpecialLayer::Root => root.properties(),
            SpecialLayer::Screen => root.screen().properties(),
            SpecialLayer::Page => root.screen().page_layer().properties(),
            SpecialLayer::Plane(id) => root.screen().page_layer().get_plane(id).properties(),
        };
        for (index, value) in values(&state, owner).into_iter().enumerate() {
            let tween = props.property_tweener(LayerProperty::from_usize(index));
            assert_eq!(tween.value().to_bits(), (value as f32).to_bits());
            assert_eq!(tween.target_value().to_bits(), (value as f32).to_bits());
            assert!(tween.is_idle());
        }
    }
}

#[test]
fn ger_preview_card_post_restore_sets_visible_pose_before_completed_result() {
    let mut card = CardSelectModel::new(15, 42).unwrap();
    let original = card.clone();
    prepare_card(&mut card);
    assert_eq!(card.phase, original.phase);
    assert_eq!(card.selected, original.selected);
    assert_eq!(card.listener, original.listener);
    assert_eq!(card.caption_y.value(), 0.);
    assert_eq!(card.caption_alpha.value(), 1.);
    for (item, old) in card.cards.iter().zip(&original.cards) {
        for index in [0, 1, 2, 5] {
            assert_eq!(
                format!("{:?}", item.tweens[index]),
                format!("{:?}", old.tweens[index])
            );
        }
        assert_eq!(item.tweens[3].value(), old.tweens[3].target_value() - 300.);
        assert_eq!(item.tweens[4].value(), 2.);
        assert_eq!(item.tweens[6].value(), 1.);
    }
    let pose = card.cards.clone();
    card.restore_result(123);
    assert_eq!(card.phase, -1);
    assert_eq!(format!("{:?}", card.cards), format!("{pose:?}"));
}
