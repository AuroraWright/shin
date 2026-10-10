use super::bustup_layer::*;
use crate::asset::bustup::Bustup;
use crate::{
    layer::{DrawableLayer, LayerGroup},
    update::{AdvUpdatable, AdvUpdateContext},
};
use shin_core::{
    primitives::update::FrameId,
    time::Ticks,
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_render::shaders::types::RenderClone;
use std::cell::RefCell;
use std::sync::Arc;

pub(super) fn reference_callbacks(
    context: &mut crate::render::PreRenderContext,
    server: &Arc<crate::asset::system::AssetServer>,
    asset: Arc<Bustup>,
) {
    let bank = LayerbankId::new(4);
    let count = asset.mouth_blocks.len();
    assert!(count > 0);
    let character = asset.character_id.raw();
    let make = |x| {
        let mut layer = BustupLayer::new(asset.clone(), None);
        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(x);
        layer
    };
    let events: RefCell<Vec<(i32, f32)>> = RefCell::new(Vec::new());
    let assert_events = |expected: &[(i32, f32)]| {
        let actual = events.borrow();
        assert_eq!(
            actual.len(),
            expected.len(),
            "Callback count/order: {actual:?}"
        );
        for (&(character, pan), &(expected_character, expected_pan)) in actual.iter().zip(expected)
        {
            assert_eq!(character, expected_character);
            assert!(
                (pan - expected_pan).abs() < 0.000001,
                "Pan {pan} versus {expected_pan}"
            );
        }
    };
    let callback = |character, pan| {
        events.borrow_mut().push((character, pan));
        Some(2)
    };
    let mut frame = FrameId::default();
    let mut update = AdvUpdateContext {
        frame_id: frame,
        delta_ticks: Ticks::from_f32(1.0),
        asset_server: server,
        device: context.device,
        queue: context.queue,
        are_animations_allowed: true,
        voice_callback: Some(&callback),
        movie_volume_settings: Default::default(),
    };
    let mut group = LayerGroup::<BustupLayer>::new(None);
    // Native generic marker starts at zero, so a zero-frame update is skipped.
    group.add_layer(bank, make(-480.0));
    group.update(&update);
    assert!(events.borrow().is_empty());
    frame.advance();
    update.frame_id = frame;
    group.update(&update);
    assert_eq!(*events.borrow(), vec![(character, -0.5)]);
    assert_eq!(
        group.get_layer(bank).unwrap().reference_mouth_state(),
        (2 * count / 3).min(count - 1) as u32
    );
    events.borrow_mut().clear();
    group.update(&update);
    assert!(events.borrow().is_empty());
    // Anonymous slot retains both source and target, and receives its final
    // update before the wipe/countdown retire it on this very frame.
    group.add_layer_with_wiper(
        bank,
        make(480.0),
        Some(crate::wiper::AnyWiper::Default(
            crate::wiper::DefaultWiper::new(Ticks::from_f32(2.0)),
        )),
    );
    group.remove_layer(bank, Ticks::from_f32(2.0));
    group.add_layer(bank, make(240.0));
    group
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(9600.0);
    let mut back = group.render_clone(&mut context.render_clone_ctx());
    frame.advance();
    update.frame_id = frame;
    group.update(&update);
    back.update(&update);
    let expected = vec![(character, -0.5), (character, 0.5), (character, 0.25)];
    assert_eq!(
        *events.borrow(),
        [expected.clone(), expected.clone()].concat()
    );
    events.borrow_mut().clear();
    group.update(&update);
    back.update(&update);
    assert!(events.borrow().is_empty());
    frame.advance();
    update.frame_id = frame;
    group.update(&update);
    back.update(&update);
    assert_eq!(*events.borrow(), [expected.clone(), expected].concat());
    events.borrow_mut().clear();
    frame.advance();
    update.frame_id = frame;
    group.update(&update);
    back.update(&update);
    assert_eq!(*events.borrow(), vec![(character, 0.25); 2]);
    // Newly cloned objects have a fresh whole-layer marker even at the same
    // frame; callbacks execute although copied properties skip that frame.
    events.borrow_mut().clear();
    let mut clone = group.render_clone(&mut context.render_clone_ctx());
    clone.update(&update);
    assert_eq!(*events.borrow(), vec![(character, 0.25)]);
    events.borrow_mut().clear();
    let layer = group.get_layer_mut(bank).unwrap();
    layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::Prop27)
        .fast_forward_to(0.9);
    frame.advance();
    update.frame_id = frame;
    group.update(&update);
    assert!(events.borrow().is_empty());
    assert_eq!(group.get_layer(bank).unwrap().reference_mouth_state(), 0);
    group
        .get_layer_mut(bank)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::Prop27)
        .fast_forward_to(1.0);
    frame.advance();
    update.frame_id = frame;
    update.are_animations_allowed = false;
    group.update(&update);
    assert_eq!(*events.borrow(), vec![(character, 0.25)]);
    frame.advance();
    update.frame_id = frame;
    update.voice_callback = None;
    group.update(&update);
    assert_eq!(group.get_layer(bank).unwrap().reference_mouth_state(), 0);
    let no_mouth_asset = Bustup {
        character_id: asset.character_id,
        origin_x: asset.origin_x,
        origin_y: asset.origin_y,
        effective_width: asset.effective_width,
        effective_height: asset.effective_height,
        bustup_id: asset.bustup_id,
        base_blocks: asset.base_blocks.clone(),
        face1: asset.face1.clone(),
        face2: asset.face2.clone(),
        mouth_blocks: vec![],
        eye_blocks: asset.eye_blocks.clone(),
    };
    let mut no_mouth = BustupLayer::new(Arc::new(no_mouth_asset), None);
    events.borrow_mut().clear();
    update.voice_callback = Some(&callback);
    frame.advance();
    update.frame_id = frame;
    no_mouth.update(&update);
    assert!(events.borrow().is_empty());
    // Native e61f0 updates active then pending PAGEBACK, even before the
    // pending page is visible. Screen transition sources get a final callback.
    use crate::layer::ScreenLayer;
    use shin_core::vm::command::types::PlaneId;
    let mut screen = ScreenLayer::new(1);
    screen
        .page_layer_mut()
        .get_plane_mut(PlaneId::new(0))
        .add_layer(bank, make(-240.0).into());
    screen.pageback(&mut context.render_clone_ctx(), true);
    screen
        .page_layer_mut()
        .get_plane_mut(PlaneId::new(0))
        .add_layer(bank, make(720.0).into());
    events.borrow_mut().clear();
    update.are_animations_allowed = true;
    frame.advance();
    update.frame_id = frame;
    screen.update(&update);
    assert_events(&[(character, -0.25), (character, 0.75)]);
    screen.apply_transition(Some(crate::wiper::AnyWiper::Default(
        crate::wiper::DefaultWiper::new(Ticks::from_f32(1.0)),
    )));
    events.borrow_mut().clear();
    frame.advance();
    update.frame_id = frame;
    screen.update(&update);
    assert_events(&[(character, -0.25), (character, 0.75)]);
    assert!(!screen.is_transition_active());
    events.borrow_mut().clear();
    frame.advance();
    update.frame_id = frame;
    screen.update(&update);
    assert_events(&[(character, 0.75)]);
    // A listener can change shared properties. Native36ac4 completes the
    // drawable base phases before36b20 invokes that listener, so this new
    // period affects the following frame, not the current frame.
    let mut phased = make(0.0);
    phased
        .properties_mut()
        .property_tweener_mut(LayerProperty::RasterHorizontalTPeriod)
        .fast_forward_to(10.0);
    let callback_props = RefCell::new(phased.properties().share());
    let callback_count = std::cell::Cell::new(0);
    let change_period = |_, _| {
        callback_count.set(callback_count.get() + 1);
        callback_props
            .borrow_mut()
            .property_tweener_mut(LayerProperty::RasterHorizontalTPeriod)
            .fast_forward_to(100.0);
        Some(2)
    };
    update.voice_callback = Some(&change_period);
    update.delta_ticks = Ticks::from_f32(1.0);
    frame.advance();
    update.frame_id = frame;
    phased.update(&update);
    assert!((phased.reference_effect_phases()[0] - 0.1).abs() < 0.000001);
    assert_eq!(callback_count.get(), 1);
    phased.update(&update);
    assert_eq!(callback_count.get(), 1);
    assert!((phased.reference_effect_phases()[0] - 0.1).abs() < 0.000001);
    frame.advance();
    update.frame_id = frame;
    phased.update(&update);
    assert_eq!(callback_count.get(), 2);
    assert!((phased.reference_effect_phases()[0] - 0.11).abs() < 0.000001);
    println!(
        "Native drawable base-before-bustup callback: listener period change applies next frame, duplicate frame skips phases and callback"
    );
    println!(
        "Native bustup callbacks: source/target order, anonymous completion frame, successive clone order, pending PAGEBACK and screen completion, fresh clone/duplicate guards, local pan, Prop27, listener and mouth gates pass"
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU or audio required"]
fn reference_native_bustup_callback_gates_local_pan_and_update_order() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x36ac4, 0x94010757u32),
        (0x36ad4, 0x9400fc4bu32),
        (0x36adc, 0x52800361u32),
        (0x36ae4, 0x1e380008u32),
        (0x36ae8, 0x34000828u32),
        (0x36af0, 0xb4000474u32),
        (0x36af4, 0xa94a2668u32),
        (0x36afc, 0x54000400u32),
        (0x36b10, 0xf9400908u32),
        (0x36b20, 0xd63f0100u32),
        (0x36b40, 0x37f801f4u32),
        (0x36b50, 0x1b087e89u32),
        (0x36b74, 0x1a8ab128u32),
        (0x369bc, 0x9400fc91u32),
        (0x369c0, 0xc8dffe60u32),
        (0x369c8, 0x9400fac2u32),
        (0x369d4, 0x9400ec67u32),
        (0x369dc, 0x4ea01c01u32),
        (0x369e4, 0x9401adb7u32),
        (0x36a04, 0x1e215801u32),
        (0x36a08, 0x1e222000u32),
        (0x36a0c, 0x1e228c28u32),
        (0x754e4, 0xaa0003f4u32),
        (0x75518, 0xaa1403e0u32),
        (0x7551c, 0x97fffe41u32),
        (0x755ac, 0xaa1403e0u32),
        (0x755b4, 0x97ffff0bu32),
        (0x71b80, 0x2d4a8400u32),
        (0x71b94, 0x9400c127u32),
        (0x71ba4, 0x9400bf7bu32),
        (0x71bd4, 0x9400c023u32),
        (0x14ed0, 0x940086b0u32),
        (0x14edc, 0x940086b1u32),
        (0x14ef4, 0x140411cfu32),
        (0x7bcf4, 0xf9400908u32),
        (0x7bd00, 0xd63f0100u32),
        (0x7ea74, 0xd63f0100u32),
        (0x7ebc0, 0xd63f0100u32),
        (0x7ebe4, 0xd63f0100u32),
        (0xe6978, 0xd63f0100u32),
        (0x11594, 0xd63f0100u32),
        (0x115b8, 0xd63f0100u32),
        (0xe6214, 0x91018288u32),
        (0xe6220, 0xf9400908u32),
        (0xe622c, 0xd63f0100u32),
        (0xe6230, 0x9101a288u32),
        (0xe6238, 0xb4000109u32),
        (0xe6254, 0x17fe11fbu32),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native instruction{address:x}"
        );
    }
}
