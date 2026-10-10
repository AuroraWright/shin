use super::*;
use crate::layer::{PageLayer, ScreenLayer, user::TileLayer};
use shin_core::{primitives::update::FrameId, time::Tween};
use shin_render::shaders::types::RenderClone;

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_whole_layer_guards_and_fresh_copy_markers() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    for (address, expected) in [
        (0x1d7e18, 0x6aa40),
        (0x1d7e20, 0x7bca0),
        (0x1d7ba8, 0x6aa40),
        (0x1d7bb0, 0x78820),
        (0x1d7f00, 0x6aa40),
        (0x1d7f08, 0x7ea20),
        (0x21fb08, 0x6aa40),
        (0x21fb10, 0xe61f0),
        (0x21fbf0, 0x6aa40),
        (0x21fbf8, 0xe6930),
        (0x21fbc0, 0x173e0a),
        (0x224f28, 0x1d9db0),
        (0x1d9dd0, 0x6aa40),
        (0x1d9dd8, 0x9f370),
        (0x1d9de8, 0x9f110),
        (0x21fc80, 0x173e1f),
    ] {
        assert_eq!(
            relocations.get(&address),
            Some(&expected),
            "Native relocation{address:x}"
        );
    }
    for (address, expected) in [
        (0x9e970, 0x97ff5d38u32),
        (0x75e70, 0x97ffd2dcu32),
        (0x9ed38, 0x97ff5caeu32),
        (0x9f138, 0x97fffef6u32),
        (0x9f394, 0x97ff6523u32),
        (0x9f3ac, 0xf9400908u32),
        (0x9f3b8, 0xd63f0100u32),
        (0x36ac4, 0x94010757u32),
        (0xca700, 0x97fd6aacu32),
        (0xc34d0, 0x97fd8738u32),
        (0x251d0, 0x94014d94u32),
        (0x78840, 0x97ffc8a0u32),
        (0x788ac, 0xbd004e60u32),
        (0x788e0, 0xbd005260u32),
        (0x78914, 0xbd005660u32),
        (0x6a9e8, 0xb900081fu32),
        (0x6aa64, 0x885ffd28),
        (0x6aa70, 0x6b00011f),
        (0x6aa74, 0x540000c1),
        (0x6aa90, 0xf9400d08),
        (0x6aaa0, 0xd63f0100),
        (0x7595c, 0x97ffd421),
        (0x76010, 0x97fffe4c),
        (0x79cb8, 0x97fff0ce),
        (0x7bcd8, 0x97fff2d2),
        (0x7bcf4, 0xf9400908),
        (0x7bd00, 0xd63f0100),
        (0x76084, 0xb9404e88),
        (0x76088, 0xb9004e68),
        (0x7608c, 0xb9405288),
        (0x76090, 0xb9005268),
        (0x76094, 0xb9405688),
        (0x76098, 0xb9005668),
        (0xe5558, 0x97fffed2),
        (0xe50c0, 0x97fe43cc),
        (0x7ed28, 0x9400032a),
        (0x7f9f8, 0x97ffc58a),
        (0x71030, 0x97ffe66c),
        (0xe6be8, 0x94000182),
        (0xe7218, 0x97fe2782),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native instruction{address:x}"
        );
    }
}

fn check_body<L: DrawableLayer + RenderClone>(
    mut layer: L,
    phases: impl Fn(&L) -> [f32; 3],
    context: &mut PreRenderContext,
    update: &mut AdvUpdateContext,
) {
    for (property, value) in [
        (LayerProperty::RasterHorizontalTPeriod, 10.0),
        (LayerProperty::RasterVerticalTPeriod, -20.0),
    ] {
        layer
            .properties_mut()
            .property_tweener_mut(property)
            .fast_forward_to(value);
    }
    layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(100.0, Tween::linear(Ticks::from_f32(100.0)));
    update.frame_id = FrameId::default();
    update.delta_ticks = Ticks::from_f32(2.0);
    update.are_animations_allowed = false;
    layer.update(update);
    assert_eq!(phases(&layer), [0.0; 3]);
    update.frame_id.advance();
    layer.update(update);
    assert_eq!(phases(&layer), [0.2, -0.1, 0.0]);
    assert_eq!(layer.properties().get_value(LayerProperty::TranslateX), 0.0);
    update.are_animations_allowed = true;
    update.delta_ticks = Ticks::from_f32(9.0);
    layer.update(update);
    assert_eq!(phases(&layer), [0.2, -0.1, 0.0]);
    assert_eq!(layer.properties().get_value(LayerProperty::TranslateX), 0.0);
    // Layer clone has fresh frame zero; its copied property marker is still1.
    let mut copy = layer.render_clone(&mut context.render_clone_ctx());
    copy.update(update);
    assert_eq!(phases(&copy), [1.1, -0.55, 0.0]);
    assert_eq!(copy.properties().get_value(LayerProperty::TranslateX), 0.0);
    copy.update(update);
    assert_eq!(phases(&copy), [1.1, -0.55, 0.0]);
    update.frame_id.advance();
    update.delta_ticks = Ticks::ZERO;
    layer.update(update);
    update.delta_ticks = Ticks::from_f32(9.0);
    layer.update(update);
    assert_eq!(phases(&layer), [0.2, -0.1, 0.0]);
    update.frame_id.advance();
    update.delta_ticks = Ticks::from_f32(-2.0);
    layer.update(update);
    assert_eq!(phases(&layer), [0.0; 3]);
    update.frame_id = FrameId::default();
    update.delta_ticks = Ticks::from_f32(1.0);
    layer.update(update);
    assert_eq!(phases(&layer), [0.1, -0.05, 0.0]);
}

impl LayerGroup<TileLayer> {
    pub(crate) fn reference_whole_layer_update_guards(
        context: &mut PreRenderContext,
        update: &mut AdvUpdateContext,
    ) {
        check_body(
            Self::new(None),
            Self::reference_effect_phases,
            context,
            update,
        );
        check_body(
            PageLayer::new(1),
            PageLayer::reference_effect_phases,
            context,
            update,
        );
        check_body(
            ScreenLayer::new(1),
            ScreenLayer::reference_effect_phases,
            context,
            update,
        );
        check_body(
            TileLayer::new(FloatColor4::WHITE, glam::Vec4::ZERO),
            TileLayer::reference_effect_phases,
            context,
            update,
        );
        let bank = LayerbankId::new(0);
        let mut group = Self::new(None);
        group.add_layer(bank, TileLayer::new(FloatColor4::WHITE, glam::Vec4::ZERO));
        update.frame_id = FrameId::default();
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(1.0);
        group.update(update);
        group.add_layer(bank, TileLayer::new(FloatColor4::WHITE, glam::Vec4::ZERO));
        group
            .get_layer_mut(bank)
            .unwrap()
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .enqueue_now(10.0, Tween::linear(Ticks::from_f32(10.0)));
        group.update(update);
        assert_eq!(
            group
                .get_layer(bank)
                .unwrap()
                .properties()
                .get_value(LayerProperty::TranslateX),
            0.0,
            "Group guard must exclude even a child installed after its first update"
        );
        update.frame_id.advance();
        group.update(update);
        assert_eq!(
            group
                .get_layer(bank)
                .unwrap()
                .properties()
                .get_value(LayerProperty::TranslateX),
            1.0
        );
        group.add_layer_with_wiper(
            bank,
            TileLayer::new(FloatColor4::BLACK, glam::Vec4::ZERO),
            Some(crate::wiper::AnyWiper::Default(
                crate::wiper::DefaultWiper::new(Ticks::from_f32(10.0)),
            )),
        );
        update.frame_id.advance();
        group.update(update);
        group.fast_forward();
        group.update(update);
        assert!(
            group.has_wiper_for_layerbank(bank),
            "Same-frame poll cannot retire a fast-forwarded wrapper"
        );
        update.frame_id.advance();
        group.update(update);
        assert!(!group.has_wiper_for_layerbank(bank));
        let mut screen = ScreenLayer::new(1);
        screen.pageback(&mut context.render_clone_ctx(), true);
        screen.apply_transition(Some(crate::wiper::AnyWiper::Default(
            crate::wiper::DefaultWiper::new(Ticks::from_f32(10.0)),
        )));
        screen.update(update);
        screen.fast_forward();
        screen.update(update);
        assert!(screen.is_transition_active());
        update.frame_id.advance();
        screen.update(update);
        assert!(!screen.is_transition_active());
        eprintln!(
            "Native whole-layer guards: group/page/screen/tile effect clocks, fresh clone marker versus copied property marker, zero/signed deltas, post-update child replacement and delayed fast-forward cleanup pass"
        );
    }
}
