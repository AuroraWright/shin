use super::*;

#[test]
fn retained_scene_restore_preserves_property_owner_and_oscillators() {
    let mut properties = LayerProperties::new();
    properties
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(42.0);
    properties
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(99.0, shin_core::time::Tween::linear(Ticks::from_f32(30.0)));
    properties.inner.lock().advance(Ticks::from_f32(3.0), false);
    let alias = properties.share();
    let identity = properties.identity();
    let before = {
        let data = properties.inner.lock();
        format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}",
            data.wobbler_x,
            data.wobbler_y,
            data.wobbler_alpha,
            data.wobbler_rotation,
            data.wobbler_scale_x,
            data.wobbler_scale_y
        )
    };
    let mut state = LayerPropertiesState::new();
    state.properties[LayerProperty::TranslateX] = -77;
    properties.restore_state(&state, LayerId::new(3), u32::MAX);
    assert_eq!(properties.identity(), identity);
    assert_eq!(alias.identity(), identity);
    assert_eq!(alias.get_value(LayerProperty::TranslateX), -77.0);
    assert_eq!(properties.get_layer_id(), LayerId::new(3));
    assert_eq!(properties.layer_load_counter(), u32::MAX);
    state.properties[LayerProperty::TranslateX] = 123;
    properties.restore_values(&state);
    assert_eq!(alias.get_value(LayerProperty::TranslateX), 123.0);
    assert_eq!(properties.get_layer_id(), LayerId::new(3));
    assert_eq!(properties.layer_load_counter(), u32::MAX);
    let data = properties.inner.lock();
    assert_eq!(
        format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}",
            data.wobbler_x,
            data.wobbler_y,
            data.wobbler_alpha,
            data.wobbler_rotation,
            data.wobbler_scale_x,
            data.wobbler_scale_y
        ),
        before
    );
    assert!(data.property_tweener(LayerProperty::TranslateX).is_idle());
}

#[test]
fn script_wobble_property_numbers_route_to_rotation_and_each_scale_axis() {
    use num_traits::FromPrimitive;
    for (mode_index, amplitude, expected) in [
        (
            44,
            250.0,
            Mat4::from_cols(
                vec4(0.0, 1.0, 0.0, 0.0),
                vec4(-1.0, 0.0, 0.0, 0.0),
                Vec4::Z,
                vec4(0.0, 0.0, 1000.0, 1.0),
            ),
        ),
        (
            48,
            750.0,
            Mat4::from_cols(
                vec4(1.75, 0.0, 0.0, 0.0),
                Vec4::Y,
                Vec4::Z,
                vec4(0.0, 0.0, 1000.0, 1.0),
            ),
        ),
        (
            52,
            750.0,
            Mat4::from_cols(
                Vec4::X,
                vec4(0.0, 1.75, 0.0, 0.0),
                Vec4::Z,
                vec4(0.0, 0.0, 1000.0, 1.0),
            ),
        ),
    ] {
        let mut data = LayerPropertiesData::new();
        for (index, value) in [
            (mode_index, 3.0),
            (mode_index + 1, 4.0),
            (mode_index + 2, amplitude),
        ] {
            data.property_tweener_mut(LayerProperty::from_i32(index).unwrap())
                .fast_forward_to(value);
        }
        data.advance(Ticks::from_f32(3.0), false);
        assert!(
            data.get_transform_params()
                .transform
                .abs_diff_eq(expected, 0.000001),
            "Property{mode_index} must affect its native rotation/scale axis"
        );
    }
}

#[test]
fn compose_flags_use_native_unsigned_saturation_before_masking() {
    let mut data = LayerPropertiesData::new();
    for (value, expected) in [
        (-1000.0, 0),
        (-1.0, 0),
        (0.5, 0),
        (1.9, 1),
        (7.0, 7),
        (8.0, 0),
        (31.0, 7),
        (2_147_483_648.0, 0),
        (4_294_967_296.0, 7),
    ] {
        data.property_tweener_mut(LayerProperty::ComposeFlags)
            .fast_forward_to(value);
        assert_eq!(data.get_compose_flags().bits(), expected, "Flags{value}");
    }
}

#[test]
fn retained_layer_rebinding_preserves_old_property_snapshots_and_live_tween_guard() {
    let mut original = LayerProperties::new();
    original
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(42.0);
    let alias = original.retain_for_clone();
    let mut snapshot = original.share();
    let old_cell = std::sync::Arc::downgrade(&original.inner.cell());
    let old_state = std::sync::Arc::downgrade(original.inner.cell().state.get().unwrap());
    let mut old_tween = snapshot.property_tweener_mut(LayerProperty::TranslateX);
    let mut replacement = LayerProperties::new();
    replacement
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(700.0);
    let replacement_id = replacement.identity();
    let draws = crate::engine_random::seed_call_count();
    original.rebind(replacement);
    assert_eq!(original.identity(), replacement_id);
    assert_eq!(alias.identity(), replacement_id);
    assert_eq!(alias.get_value(LayerProperty::TranslateX), 700.0);
    assert_eq!(crate::engine_random::seed_call_count(), draws);
    old_tween.fast_forward_to(99.0);
    drop(snapshot);
    assert!(old_cell.upgrade().is_none());
    assert!(
        old_state.upgrade().is_some(),
        "Borrowed old property data stays alive"
    );
    assert_eq!(old_tween.value(), 99.0);
    assert_eq!(alias.get_value(LayerProperty::TranslateX), 700.0);
    drop(old_tween);
    assert!(
        old_state.upgrade().is_none(),
        "Last borrow releases replaced state"
    );
}

#[test]
fn explicit_property_sharing_does_not_share_layer_pointer_rebinding() {
    let mut original = LayerProperties::new();
    original
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(42.0);
    let mut shared = original.share();
    shared.rebind(LayerProperties::new());
    assert_ne!(shared.identity(), original.identity());
    assert_eq!(shared.get_value(LayerProperty::TranslateX), 0.0);
    assert_eq!(original.get_value(LayerProperty::TranslateX), 42.0);
}
use shin_core::{primitives::update::FrameId, time::Tween};

#[test]
fn explicit_property_sharing_is_bidirectional_and_regular_clones_are_independent() {
    let mut original = LayerProperties::new();
    original
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    let independent = original.clone();
    let mut shared = original.share();
    assert_eq!(shared.identity(), original.identity());
    assert_ne!(independent.identity(), original.identity());
    shared
        .property_tweener_mut(LayerProperty::TranslateY)
        .fast_forward_to(20.0);
    assert_eq!(original.get_value(LayerProperty::TranslateY), 20.0);
    assert_eq!(independent.get_value(LayerProperty::TranslateY), 0.0);
    original.set_layer_id(LayerId::new(21));
    assert_eq!(shared.get_layer_id(), LayerId::new(21));
    assert_eq!(independent.get_layer_id(), LayerId::new(0));
    shared.set_layerload_counter1(9);
    assert_eq!(original.inner.lock().layer_load_counter1, 9);
    assert_eq!(independent.inner.lock().layer_load_counter1, 0);
}

#[test]
fn shared_frame_guard_covers_signed_deltas_pauses_wobblers_and_retained_single_owner() {
    let mut props = LayerProperties::new();
    props
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    for (property, value) in [
        (LayerProperty::WobbleXMode, 1.0),
        (LayerProperty::WobbleXPeriod, 10.0),
        (LayerProperty::WobbleXAmplitude, 100.0),
    ] {
        props.property_tweener_mut(property).fast_forward_to(value);
    }
    let mut shared = props.share();
    let mut frame = FrameId::default();
    frame.advance();
    props.advance(frame, Ticks::from_f32(2.0), true);
    let transform = props.get_transform_params().compute_final_transform();
    shared.advance(frame, Ticks::from_f32(9.0), true);
    assert_eq!(shared.get_value(LayerProperty::TranslateX), 20.0);
    assert_eq!(
        shared.get_transform_params().compute_final_transform(),
        transform
    );
    frame.advance();
    props.advance(frame, Ticks::from_f32(2.0), false);
    let transform = props.get_transform_params().compute_final_transform();
    assert_eq!(props.get_value(LayerProperty::TranslateX), 20.0);
    shared.advance(frame, Ticks::from_f32(3.0), true);
    assert_eq!(
        shared.get_transform_params().compute_final_transform(),
        transform
    );
    for dt in [0.0, -1.0, 1.0] {
        let previous_value = props.get_value(LayerProperty::TranslateX);
        frame.advance();
        props.advance(frame, Ticks::from_f32(dt), true);
        let value = props.get_value(LayerProperty::TranslateX);
        if dt <= 0.0 {
            assert_eq!(value, previous_value);
        } else {
            assert!(value > previous_value);
        }
        shared.advance(frame, Ticks::from_f32(8.0), true);
        assert_eq!(shared.get_value(LayerProperty::TranslateX), value);
    }
    drop(shared);
    frame.advance();
    props.advance(frame, Ticks::from_f32(1.0), true);
    let value = props.get_value(LayerProperty::TranslateX);
    props.advance(frame, Ticks::from_f32(1.0), true);
    assert_eq!(props.get_value(LayerProperty::TranslateX), value);
    let mut copy = props.clone();
    assert_ne!(copy.identity(), props.identity());
    // Native6aa00 copies the frame marker too.
    copy.advance(frame, Ticks::from_f32(8.0), true);
    assert_eq!(copy.get_value(LayerProperty::TranslateX), value);
    frame.advance();
    copy.advance(frame, Ticks::from_f32(1.0), true);
    assert_ne!(
        copy.get_value(LayerProperty::TranslateX),
        props.get_value(LayerProperty::TranslateX)
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_shared_properties_frame_guard_and_copy() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x9450, 0x37180b48u32),
        (0x945c, 0x9401ad2d),
        (0x95e0, 0xd63f0100),
        (0x6aa14, 0x88dffd08),
        (0x6aa18, 0xb9000808),
        (0x6aa5c, 0x94024411),
        (0x6aa64, 0x885ffd28),
        (0x6aa70, 0x6b00011f),
        (0x6aa74, 0x540000c1),
        (0x757ec, 0x36000121),
        (0x75c18, 0xc8dffea8),
        (0x75c1c, 0xb5000388),
        (0x75c2c, 0x97fff6ed),
        (0x7598c, 0x91008288),
        (0x75990, 0xc8dffd09),
        (0x75994, 0xb40003c9),
        (0x759ac, 0x97fff925),
        (0x75da0, 0xc8dffd09),
        (0x75da4, 0xb4000129),
        (0x75e08, 0xc8dffd09),
        (0x75e0c, 0xb4000149),
        (0x78848, 0xc8dffd09),
        (0x7884c, 0xb40000e9),
        (0x78864, 0xd63f0100),
        (0x7887c, 0xd63f0120),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
}

#[test]
fn ordinary_properties_and_copies_preserve_native_once_per_frame_updates() {
    let mut props = LayerProperties::new();
    props
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    for (property, value) in [
        (LayerProperty::WobbleXMode, 1.0),
        (LayerProperty::WobbleXPeriod, 10.0),
        (LayerProperty::WobbleXAmplitude, 100.0),
    ] {
        props.property_tweener_mut(property).fast_forward_to(value);
    }
    let mut frame = FrameId::default();
    frame.advance();
    props.advance(frame, Ticks::from_f32(2.0), true);
    let transform = props.get_transform_params().compute_final_transform();
    props.advance(frame, Ticks::from_f32(9.0), true);
    assert_eq!(props.get_value(LayerProperty::TranslateX), 20.0);
    assert_eq!(
        props.get_transform_params().compute_final_transform(),
        transform
    );
    let mut copy = props.clone();
    assert_ne!(copy.identity(), props.identity());
    copy.advance(frame, Ticks::from_f32(8.0), true);
    assert_eq!(copy.get_value(LayerProperty::TranslateX), 20.0);
    assert_eq!(
        copy.get_transform_params().compute_final_transform(),
        transform
    );
    frame.advance();
    props.advance(frame, Ticks::from_f32(2.0), false);
    let paused_transform = props.get_transform_params().compute_final_transform();
    props.advance(frame, Ticks::from_f32(2.0), true);
    assert_eq!(props.get_value(LayerProperty::TranslateX), 20.0);
    assert_eq!(
        props.get_transform_params().compute_final_transform(),
        paused_transform
    );
    copy.advance(frame, Ticks::from_f32(1.0), true);
    assert!((copy.get_value(LayerProperty::TranslateX) - 30.0).abs() < 0.00001);
    for dt in [0.0, -1.0, 1.0] {
        frame.advance();
        props.advance(frame, Ticks::from_f32(dt), true);
        let value = props.get_value(LayerProperty::TranslateX);
        let transform = props.get_transform_params().compute_final_transform();
        props.advance(frame, Ticks::from_f32(8.0), true);
        assert_eq!(props.get_value(LayerProperty::TranslateX), value);
        assert_eq!(
            props.get_transform_params().compute_final_transform(),
            transform
        );
    }
}

#[test]
fn new_properties_skip_native_frame_zero_then_advance_on_frame_one() {
    let mut props = LayerProperties::new();
    props
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue(100.0, Tween::linear(Ticks::from_f32(10.0)));
    let mut frame = FrameId::default();
    props.advance(frame, Ticks::from_f32(9.0), true);
    assert_eq!(props.get_value(LayerProperty::TranslateX), 0.0);
    frame.advance();
    props.advance(frame, Ticks::from_f32(2.0), true);
    assert_eq!(props.get_value(LayerProperty::TranslateX), 20.0);
}

#[test]
fn absent_properties_skip_rng_update_clone_and_fast_forward_until_accessed() {
    let start = crate::engine_random::seed_call_count();
    let mut original = LayerProperties::new();
    let mut absent_copy = original.clone();
    let mut frame = FrameId::default();
    frame.advance();
    original.advance(frame, Ticks::from_f32(10.0), true);
    original.fast_forward();
    absent_copy.fast_forward();
    assert!(original.inner.cell().state.get().is_none());
    assert!(absent_copy.inner.cell().state.get().is_none());
    assert_eq!(original.initialized_identity(), None);
    assert_ne!(original.identity(), absent_copy.identity());
    assert!(
        original
            .reference_tweener_snapshot(LayerProperty::TranslateX)
            .is_none()
    );
    assert!(
        absent_copy
            .reference_tweener_snapshot(LayerProperty::TranslateX)
            .is_none()
    );
    assert_eq!(crate::engine_random::seed_call_count(), start);

    assert_eq!(original.get_value(LayerProperty::TranslateX), 0.0);
    assert_eq!(crate::engine_random::seed_call_count(), start + 7);
    // Native first base-body accessor happens after the absent property update.
    assert_eq!(original.inner.lock().updated_frame, FrameId::default());
    original.advance(frame, Ticks::from_f32(2.0), true);
    assert_eq!(original.inner.lock().updated_frame, frame);
    let mut observed = original
        .reference_tweener_snapshot(LayerProperty::TranslateX)
        .unwrap();
    observed.fast_forward_to(-999.0);
    assert_eq!(original.get_value(LayerProperty::TranslateX), 0.0);
    assert_eq!(original.reference_initialized_frame(), Some(frame));
    assert_eq!(crate::engine_random::seed_call_count(), start + 7);

    let independent = original.clone();
    assert_eq!(crate::engine_random::seed_call_count(), start + 13);
    assert_eq!(independent.inner.lock().updated_frame, frame);
    let mut shared = original.share();
    assert_eq!(crate::engine_random::seed_call_count(), start + 13);
    shared
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(42.0);
    assert_eq!(original.get_value(LayerProperty::TranslateX), 42.0);
    assert_eq!(independent.get_value(LayerProperty::TranslateX), 0.0);
    assert!(absent_copy.inner.cell().state.get().is_none());
    let retained = absent_copy.share();
    assert_eq!(crate::engine_random::seed_call_count(), start + 20);
    assert_eq!(absent_copy.identity(), retained.identity());
    assert_ne!(absent_copy.identity(), original.identity());
    assert_eq!(crate::engine_random::seed_call_count(), start + 20);
}
