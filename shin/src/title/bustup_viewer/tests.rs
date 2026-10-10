use super::super::{bustup_catalog::BustupCatalog, bustup_menu};
use super::*;
use shin_core::format::scenario::{
    info::{BustupId, CharacterBoxSegment, PictureId},
    instruction_elements::FromNumber,
};

fn setup() -> (BustupMenu, ScreenLayer, BustupViewer) {
    use CharacterBoxSegment::*;
    let catalog = BustupCatalog::new(&vec![
        Background {
            primary_picture_id: PictureId::from_number(0),
            secondary_picture_id_offset: 0,
        },
        EndDefinition,
        Bustup {
            bustup_id: BustupId::from_number(0),
        },
        EndDefinition,
    ])
    .unwrap();
    let mut menu = BustupMenu::new(&catalog);
    let mut eligible = true;
    menu.controller(
        &catalog,
        &mut eligible,
        false,
        bustup_menu::Controls {
            confirm: true,
            ..Default::default()
        },
    );
    menu.update(Ticks::from_f32(7.0));
    menu.prepare_frame();
    assert_eq!(menu.take_requests(), [bustup_menu::Request::OpenViewer]);
    menu.take_sounds();
    let mut screen = ScreenLayer::new(1);
    screen
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(-200.0);
    let viewer = BustupViewer::new(&mut menu, &mut screen);
    (menu, screen, viewer)
}
fn tick(menu: &mut BustupMenu, screen: &mut ScreenLayer, delta: f32) {
    let delta = Ticks::from_f32(delta);
    menu.update(delta);
    screen
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .update(delta);
    let props = screen
        .page_layer_mut()
        .get_plane_mut(PlaneId::new(0))
        .properties_mut();
    for property in [
        LayerProperty::CameraPositionX,
        LayerProperty::CameraPositionY,
    ] {
        props.property_tweener_mut(property).update(delta);
    }
}
fn activate(menu: &mut BustupMenu, screen: &mut ScreenLayer, viewer: &mut BustupViewer) {
    viewer.prepare_frame(menu, screen, true);
    assert!(!viewer.input_enabled);
    tick(menu, screen, 30.0);
    viewer.prepare_frame(menu, screen, true);
    assert_eq!(viewer.phase, Phase::InstallingInput);
    assert!(!viewer.input_enabled);
    viewer.prepare_frame(menu, screen, true);
    assert!(viewer.input_enabled);
}
fn camera_position(screen: &ScreenLayer) -> Vec2 {
    let props = screen.page_layer().get_plane(PlaneId::new(0)).properties();
    Vec2::new(
        props.get_value(LayerProperty::CameraPositionX),
        props.get_value(LayerProperty::CameraPositionY),
    )
}

#[test]
fn centering_waiter_blocks_completion_frame_and_return_waits_for_both_clocks() {
    let (mut menu, mut screen, mut viewer) = setup();
    assert!(!menu.input_alive());
    viewer.prepare_frame(&mut menu, &screen, true);
    tick(&mut menu, &mut screen, 10.0);
    assert_eq!(menu.controls_visibility(), 0.0);
    let x = screen.properties().get_value(LayerProperty::TranslateX);
    assert!((x + 100.0).abs() < 0.001);
    assert_eq!(
        screen.properties().get_value(LayerProperty::TranslateZ),
        1000.0
    );
    tick(&mut menu, &mut screen, 20.0);
    viewer.prepare_frame(&mut menu, &screen, true);
    assert!(!viewer.input_enabled);
    viewer.prepare_frame(&mut menu, &screen, true);
    let mut eligible = true;
    viewer.controller(
        &mut menu,
        &mut screen,
        &mut eligible,
        false,
        Controls {
            right: true,
            ..Default::default()
        },
    );
    assert!(!eligible);
    viewer.advance(Ticks::ZERO, &mut screen);
    assert_eq!(camera_position(&screen), Vec2::new(10.0, 0.0));
    let mut eligible = true;
    viewer.controller(
        &mut menu,
        &mut screen,
        &mut eligible,
        false,
        Controls {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(viewer.take_sounds(), ["cancel"]);
    tick(&mut menu, &mut screen, 15.0);
    assert_eq!(menu.controls_visibility(), 0.5);
    assert!((screen.properties().get_value(LayerProperty::TranslateX) + 141.42136).abs() < 0.001);
    viewer.prepare_frame(&mut menu, &screen, true);
    assert!(!viewer.is_finished());
    // Native40110 also waits for the panel, even if screen centering is done.
    screen
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward();
    viewer.prepare_frame(&mut menu, &screen, true);
    assert!(!viewer.is_finished());
    tick(&mut menu, &mut screen, 15.0);
    viewer.prepare_frame(&mut menu, &screen, true);
    assert!(viewer.is_finished());
    assert!(!viewer.input_enabled);
    assert!(menu.input_alive());
    assert_eq!(menu.panel_offset(), 0.0);
    assert_eq!(menu.controls_visibility(), 1.0);
    assert_eq!(camera_position(&screen), Vec2::ZERO);
}

#[test]
fn native_pan_priority_limits_and_signed_inertia_publish_to_the_retained_group() {
    let (mut menu, mut screen, mut viewer) = setup();
    activate(&mut menu, &mut screen, &mut viewer);
    let mut eligible = true;
    viewer.controller(
        &mut menu,
        &mut screen,
        &mut eligible,
        false,
        Controls {
            up: true,
            down: true,
            left: true,
            right: true,
            axes: Vec2::ONE,
            ..Default::default()
        },
    );
    assert_eq!(viewer.position(), Vec2::splat(-10.0));
    viewer.gesture(Vec2::new(40.0, 80.0));
    for delta in [0.0, -1.0, 2.0] {
        let old = viewer.position;
        let velocity = viewer.velocity;
        viewer.advance(Ticks::from_f32(delta), &mut screen);
        assert_eq!(
            viewer.position,
            (old - velocity).clamp(Vec2::new(-420.0, -130.0), Vec2::new(420.0, 130.0))
        );
        assert!((viewer.velocity - velocity * 0.95f32.powf(delta)).length() < 0.0001);
        assert_eq!(camera_position(&screen), viewer.position());
    }
    let mut eligible = true;
    viewer.controller(
        &mut menu,
        &mut screen,
        &mut eligible,
        false,
        Controls {
            axes: Vec2::new(100.0, 100.0),
            ..Default::default()
        },
    );
    assert_eq!(viewer.position(), Vec2::new(420.0, 130.0));
    assert_eq!(viewer.velocity, Vec2::ZERO);
    viewer.drag(Vec2::ZERO, Vec2::new(2000.0, 2000.0));
    assert_eq!(viewer.position(), Vec2::new(-420.0, -130.0));
    viewer.gesture(Vec2::new(10.0, 20.0));
    assert_eq!(viewer.velocity, Vec2::new(10.0, 11.25));
    assert!(viewer.probe());
    assert_eq!(viewer.velocity, Vec2::ZERO);
    viewer.gesture(Vec2::new(10.0, 20.0));
    let mut eligible = true;
    viewer.controller(
        &mut menu,
        &mut screen,
        &mut eligible,
        false,
        Controls {
            cancel: true,
            ..Default::default()
        },
    );
    let clock = screen
        .page_layer()
        .get_plane(PlaneId::new(0))
        .properties()
        .property_tweener(LayerProperty::CameraPositionX);
    assert!(clock.is_waiting());
    viewer.advance(Ticks::from_f32(1.0), &mut screen);
    assert!(
        screen
            .page_layer()
            .get_plane(PlaneId::new(0))
            .properties()
            .property_tweener(LayerProperty::CameraPositionX)
            .is_waiting(),
        "closing inertia overwrote return tween"
    );
}

#[test]
fn scaled_shared_pointer_captures_before_controller_and_tap_uses_decide_sound() {
    use crate::adv::input_area::{PointerEvent, PointerKind};
    let (mut menu, mut screen, mut viewer) = setup();
    activate(&mut menu, &mut screen, &mut viewer);
    let mut input = BustupViewerInput::new((960, 540));
    let event = |kind, x, y| PointerEvent {
        kind,
        id: 0,
        position: glam::ivec2(x, y),
    };
    let mut state = EnumMap::<AppAction, ActionState>::default();
    state[AppAction::Left].is_clicked = true;
    for (number, time, events) in [
        (1, 0.0, vec![event(PointerKind::Down, 480, 270)]),
        (2, 0.1, vec![event(PointerKind::Move, 600, 400)]),
    ] {
        let mut eligible = true;
        input.input_frame(
            &mut viewer,
            &mut menu,
            &mut screen,
            &mut eligible,
            &PointerFrame {
                number,
                time_seconds: time,
                events: &events,
            },
            &state,
            Vec2::ZERO,
        );
        assert!(!eligible);
    }
    assert_eq!(viewer.position(), Vec2::new(-240.0, -130.0));
    // Disabled native6b0a0 cancels a held contact without generating tap close.
    let mut eligible = false;
    input.input_frame(
        &mut viewer,
        &mut menu,
        &mut screen,
        &mut eligible,
        &PointerFrame {
            number: 3,
            time_seconds: 0.2,
            events: &[],
        },
        &state,
        Vec2::ZERO,
    );
    assert!(!viewer.is_closing());
    for (number, time, events) in [
        (4, 0.3, vec![event(PointerKind::Down, 480, 270)]),
        (5, 0.4, vec![event(PointerKind::Up, 0, 0)]),
    ] {
        let mut eligible = true;
        input.input_frame(
            &mut viewer,
            &mut menu,
            &mut screen,
            &mut eligible,
            &PointerFrame {
                number,
                time_seconds: time,
                events: &events,
            },
            &state,
            Vec2::ZERO,
        );
    }
    assert!(viewer.is_closing());
    assert_eq!(viewer.take_sounds(), ["decide"]);
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_opening_viewer_centering_pan_inertia_and_return_waiters() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x38240, 0x9400f6b8u32),
        (0x38248, 0xbd465100u32),
        (0x3824c, 0x94020c25u32),
        (0x3c830, 0x97ffafccu32),
        (0x3c834, 0x7100081fu32),
        (0x3c840, 0x321e0fe1u32),
        (0x3c844, 0x94009997u32),
        (0x3c854, 0x321e0fe1u32),
        (0x3c858, 0x97ffdcf6u32),
        (0x3c86c, 0x940099a9u32),
        (0x3c870, 0x370001a0u32),
        (0x3c884, 0x32000be1u32),
        (0x3c890, 0x97ffdbdcu32),
        (0x3c898, 0x97fff52eu32),
        (0x3c8ac, 0x940099a5u32),
        (0x3c908, 0x9400e506u32),
        (0x3c910, 0x1e27d001u32),
        (0x3c914, 0x321f03e1u32),
        (0x3c920, 0x9401fc38u32),
        (0x3c928, 0x1e249001u32),
        (0x3c934, 0x9103c100u32),
        (0x3c93c, 0x9401fc31u32),
        (0x3c980, 0x52802029u32),
        (0x3cb84, 0x94030d4bu32),
        (0x3cbb0, 0x2d178261u32),
        (0x3cbbc, 0x94038021u32),
        (0x3cbc0, 0x1e200940u32),
        (0x3cbd4, 0x9403801bu32),
        (0x3cbe4, 0xbd00ca61u32),
        (0x3cc60, 0x52800aa1u32),
        (0x3cc6c, 0x9401f99du32),
        (0x3ccac, 0x52800ac1u32),
        (0x3ccb8, 0x9401f98au32),
        (0x3ce28, 0x721c051fu32),
        (0x3ce44, 0xaa1303e0u32),
        (0x3ce54, 0x3900029fu32),
        (0x3ce74, 0x1e349000u32),
        (0x3ce80, 0x1e269003u32),
        (0x3ce88, 0x370000c8u32),
        (0x3ce98, 0x721f011fu32),
        (0x3ceac, 0x721d011fu32),
        (0x3cee8, 0x2d178660u32),
        (0x3ceec, 0xf80c427fu32),
        (0x3cf80, 0x9400e368u32),
        (0x3cf88, 0xbd465100u32),
        (0x3cf98, 0x9401fa9au32),
        (0x3cfb8, 0x52800aa1u32),
        (0x3cfd0, 0x9401fa8cu32),
        (0x3d010, 0x52800ac1u32),
        (0x3d028, 0x9401fa76u32),
        (0x3d064, 0x9103c100u32),
        (0x3d068, 0x9401fa66u32),
        (0x3d07c, 0x9107e100u32),
        (0x3d084, 0x9401fa5fu32),
        (0x3d0c8, 0x52802029u32),
        (0x3d1f0, 0xf80c401fu32),
        (0x3d2f8, 0x1e3c5002u32),
        (0x3d308, 0x1e203842u32),
        (0x3d30c, 0x1e212860u32),
        (0x3d3d0, 0xbd00c400u32),
        (0x3d3d8, 0x1e200820u32),
        (0x3fe14, 0x9400d7c3u32),
        (0x3fe18, 0x9401eef6u32),
        (0x3fe20, 0x370005e0u32),
        (0x3fe90, 0x94030078u32),
        (0x40130, 0x9400d6fcu32),
        (0x40134, 0x9401ee2fu32),
        (0x40138, 0x360000a0u32),
        (0x40150, 0x9107e100u32),
        (0x40154, 0x9401ee27u32),
        (0x40158, 0x360000a0u32),
        (0x40170, 0x97ffe6f8u32),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "native{address:x}"
        );
    }
    for (address, expected) in [
        (0x159650, -200.0f32),
        (0x1595d8, 0.95),
        (0x1596ec, -420.0),
        (0x159620, 420.0),
        (0x159488, -130.0),
        (0x1597b0, 130.0),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}
