use super::*;
fn active(characters: i32, information: bool) -> SystemMenuModel {
    let mut model = SystemMenuModel::new(characters, information);
    model.update(22.0);
    assert_eq!(model.phase(), SystemMenuPhase::Opening);
    model.update(0.0);
    assert_eq!(model.phase(), SystemMenuPhase::Active);
    model
}
#[test]
fn menu_snapshot_retains_route_metadata_and_autosaved_title_after_vm_changes() {
    let mut vm = super::super::VmState::for_scenario(7, 42);
    vm.save_info.set_save_info(1, "Chapter A".into());
    vm.snapshot_scene_title();
    vm.capture_autosave();
    vm.information.select_character_grid(12, 3);
    vm.information.acquire_tips(&[4, 2]);
    vm.save_position = 8;
    let snapshot = SystemMenuSnapshot::from_vm(&vm);
    vm.save_info.set_save_info(1, "Chapter B".into());
    vm.snapshot_scene_title();
    vm.capture_autosave();
    vm.information.select_character_grid(-1, 0);
    vm.information.acquired_tips.clear();
    vm.save_position = 9;
    assert_eq!(snapshot.scene_title, "Chapter A");
    assert_eq!(snapshot.information.character_grid, 12);
    assert_eq!(snapshot.information.character_grid_argument, 3);
    assert_eq!(snapshot.information.acquired_tips, [4, 2]);
    assert_eq!(snapshot.entry.entry.scenario_id, 7);
    assert_eq!(snapshot.entry.entry.random_seed, 42);
    assert_eq!(snapshot.entry.entry.save_position, 8);
    assert_eq!(snapshot.model().items(), &[0, 1, 2, 3, 4, 5, 6]);
    let latest = SystemMenuSnapshot::from_vm(&vm);
    assert_eq!(latest.scene_title, "Chapter B");
    assert_eq!(latest.model().items(), &[0, 1, 2, 5, 6]);
}
#[test]
fn manual_child_handoff_retains_menu_gate_until_final_response_and_reopens_selection() {
    for (characters, information) in [(-1, false), (0, false), (-1, true), (0, true)] {
        let mut menu = active(characters, information);
        let selected = menu.items().iter().position(|&item| item == 5).unwrap();
        menu.selected = selected;
        menu.pointer_probe(glam::vec2(30.0, 80.0));
        menu.confirm();
        menu.update(7.0);
        menu.update(0.0);
        assert_eq!(menu.take_item(), Some(5));
        assert_eq!(menu.phase(), SystemMenuPhase::Child);
        assert_eq!(menu.pointer_row, None);
        assert_eq!(menu.take_sounds(), ["decide"]);
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        input[AppAction::Cancel].is_clicked = true;
        input[AppAction::Enter].is_clicked = true;
        input[AppAction::Up].is_clicked_or_repeated = true;
        menu.input(&input);
        menu.confirm();
        menu.close();
        assert!(!menu.pointer_probe(glam::vec2(30.0, 80.0)));
        menu.pointer_gesture(glam::vec2(-100.0, 0.0));
        assert_eq!(menu.selected(), selected);
        assert_eq!(menu.take_item(), None);
        assert!(menu.take_sounds().is_empty());
        menu.update(5.0);
        assert!(
            (menu.feedback(0).unwrap().0 - (std::f32::consts::FRAC_PI_4).cos()).abs() < 0.00001
        );
        assert!(
            (menu.detail_feedback().0 - (1.0 - (std::f32::consts::FRAC_PI_4).cos())).abs()
                < 0.00001
        );
        let mut manual = super::super::manual::ManualModel::new(24).unwrap();
        manual.update(10.0);
        let mut cancel = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        cancel[AppAction::Cancel].is_clicked = true;
        manual.input(&cancel);
        manual.update(7.0);
        manual.update(0.0);
        assert_eq!(manual.phase(), super::super::manual::ManualPhase::Closing);
        menu.child_response(0);
        assert_eq!(menu.phase(), SystemMenuPhase::Child);
        menu.update(100.0);
        assert_eq!(menu.phase(), SystemMenuPhase::Child);
        for row in 0..menu.items().len() {
            assert_eq!(menu.feedback(row), Some((0.0, 0.0)));
        }
        assert_eq!(menu.detail_feedback(), (1.0, 0.0));
        manual.update(10.0);
        assert_eq!(manual.phase(), super::super::manual::ManualPhase::Closed);
        menu.child_response(1);
        assert_eq!(menu.phase(), SystemMenuPhase::Opening);
        menu.child_response(1); // Duplicate host delivery must not queue another opening.
        menu.input(&cancel);
        assert_eq!(menu.phase(), SystemMenuPhase::Opening);
        menu.update(22.0);
        assert_eq!(menu.phase(), SystemMenuPhase::Opening);
        menu.update(0.0);
        assert_eq!(menu.phase(), SystemMenuPhase::Active);
        assert_eq!(menu.selected(), selected);
        assert_eq!(menu.detail_feedback(), (0.0, 1.0));
        assert!(menu.take_sounds().is_empty());
        // The reopened menu accepts a subsequent selection and normal close.
        let mut up = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        up[AppAction::Up].is_clicked_or_repeated = true;
        menu.input(&up);
        assert_eq!(menu.selected(), selected - 1);
        assert_eq!(menu.take_sounds(), ["move"]);
        menu.input(&cancel);
        assert_eq!(menu.phase(), SystemMenuPhase::Closing);
        assert_eq!(menu.take_sounds(), ["cancel"]);
        menu.update(22.0);
        menu.update(0.0);
        assert_eq!(menu.phase(), SystemMenuPhase::Closed);
    }
}
#[test]
fn early_child_return_appends_reopening_after_existing_retreat() {
    let mut menu = active(0, true);
    menu.confirm();
    menu.update(7.0);
    menu.update(0.0);
    assert_eq!(menu.take_item(), Some(0));
    menu.update(5.0);
    menu.child_response(1);
    // Native1d460 appends rather than clearing the1e010 motion queues.
    menu.update(17.0);
    assert_eq!(menu.phase(), SystemMenuPhase::Opening);
    assert_eq!(menu.feedback(6).unwrap().0, 0.0);
    menu.update(22.0);
    menu.update(0.0);
    assert_eq!(menu.phase(), SystemMenuPhase::Active);
    assert_eq!(menu.feedback(6).unwrap().0, 1.0);
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_system_menu_manual_child_loading_retreat_and_callback() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x1e028, 0xa94ba408),
        (0x1e02c, 0xaa0003f3),
        (0x1e030, 0xeb08013f),
        (0x1e034, 0x540003e0),
        (0x1e038, 0x1e2703e8),
        (0x1e03c, 0x1e249009),
        (0x1e040, 0x1e20100b),
        (0x1e044, 0x4ea81d0a),
        (0x1e048, 0xd286f4f6),
        (0x1e04c, 0xf2bd37b6),
        (0x1e050, 0xaa1f03f4),
        (0x1e054, 0x321d03f5),
        (0x1e058, 0xf2cde9b6),
        (0x1e05c, 0xf2fa6f56),
        (0x1e060, 0x4ea81d00),
        (0x1e064, 0x4eaa1d41),
        (0x1e068, 0x8b150100),
        (0x1e06c, 0x321903e1),
        (0x1e070, 0x2a1f03e2),
        (0x1e074, 0x94027663),
        (0x1e078, 0x4ea81d00),
        (0x1e07c, 0x4ea91d21),
        (0x1e080, 0x320003e1),
        (0x1e084, 0x2a1f03e2),
        (0x1e088, 0x9402765e),
        (0x1e08c, 0xa94ba668),
        (0x1e090, 0xcb080129),
        (0x1e094, 0x9343fd29),
        (0x1e098, 0x9b167d29),
        (0x1e09c, 0x1e2b294a),
        (0x1e0a0, 0x91000694),
        (0x1e0a4, 0x9102e2b5),
        (0x1e0a8, 0xeb09029f),
        (0x1e0ac, 0x54fffda3),
        (0x1e0b0, 0x1e249008),
        (0x1e0b4, 0x1e2e1000),
        (0x1e0b8, 0x320003e1),
        (0x1e0bc, 0x2a1f03e2),
        (0x1e0c0, 0x4ea81d01),
        (0x1e0c4, 0x91036260),
        (0x1e0c8, 0x9402764e),
        (0x1e0cc, 0x4ea81d01),
        (0x1e0d0, 0x6d4123e9),
        (0x1e0d4, 0x9104c260),
        (0x1e0d8, 0xa9447bfd),
        (0x1e0dc, 0xa9434ff4),
        (0x1e0e0, 0x1e2703e0),
        (0x1e0e4, 0xa94257f6),
        (0x1e0e8, 0x2a1f03e1),
        (0x1e0ec, 0x2a1f03e2),
        (0x1e0f0, 0x6cc52beb),
        (0x1e0f4, 0x14027643),
        (0x1e2c4, 0xf9400a74),
        (0x1e2c8, 0x90000da8),
        (0x1e2cc, 0x912d2108),
        (0x1e2d0, 0xa9004fe8),
        (0x1e2d4, 0x910003e8),
        (0x1e2d8, 0xf90013e8),
        (0x1e2dc, 0xd10103a8),
        (0x1e2e0, 0x910003e0),
        (0x1e2e4, 0x9401a387),
        (0x1e2e8, 0x91056280),
        (0x1e2ec, 0xd10103a1),
        (0x1e2f0, 0x94038760),
        (0x1e32c, 0xaa1303e0),
        (0x1e330, 0x97ffff38),
        (0x1fe90, 0xb9400028),
        (0x1fe94, 0x7100051f),
        (0x1fe98, 0x54000061),
        (0x1fe9c, 0xf9400400),
        (0x1fea0, 0x17fff570),
        (0x1fea4, 0xd65f03c0),
        (0x1d47c, 0xa94ba408),
        (0x1d480, 0xaa0003f3),
        (0x1d484, 0xeb08013f),
        (0x1d488, 0x54000400),
        (0x1d48c, 0x1e2703e8),
        (0x1d490, 0x1e2e1009),
        (0x1d494, 0x1e24900a),
        (0x1d498, 0x1e20100c),
        (0x1d49c, 0x4ea81d0b),
        (0x1d4a0, 0xd286f4f6),
        (0x1d4a4, 0xf2bd37b6),
        (0x1d4a8, 0xaa1f03f4),
        (0x1d4ac, 0x321d03f5),
        (0x1d4b0, 0xf2cde9b6),
        (0x1d4b4, 0xf2fa6f56),
        (0x1d4b8, 0x4ea81d00),
        (0x1d4bc, 0x4eab1d61),
        (0x1d4c0, 0x8b150100),
        (0x1d4c4, 0x321903e1),
        (0x1d4c8, 0x2a1f03e2),
        (0x1d4cc, 0x9402794d),
        (0x1d4d0, 0x4ea91d20),
        (0x1d4d4, 0x4eaa1d41),
        (0x1d4d8, 0x321f03e1),
        (0x1d4dc, 0x2a1f03e2),
        (0x1d4e0, 0x94027948),
        (0x1d4e4, 0xa94ba668),
        (0x1d4e8, 0xcb080129),
        (0x1d4ec, 0x9343fd29),
        (0x1d4f0, 0x9b167d29),
        (0x1d4f4, 0x1e2c296b),
        (0x1d4f8, 0x91000694),
        (0x1d4fc, 0x9102e2b5),
        (0x1d500, 0xeb09029f),
        (0x1d504, 0x54fffda3),
        (0x1d508, 0x1e249008),
        (0x1d50c, 0x1e2703e0),
        (0x1d510, 0x321f03e1),
        (0x1d514, 0x2a1f03e2),
        (0x1d518, 0x4ea81d01),
        (0x1d51c, 0x91036260),
        (0x1d520, 0x94027938),
        (0x1d524, 0x1e2e1000),
        (0x1d528, 0x4ea81d01),
        (0x1d52c, 0x9104c260),
        (0x1d530, 0x2a1f03e1),
        (0x1d534, 0x2a1f03e2),
        (0x1d538, 0x94027932),
        (0x1d53c, 0x6d4223e9),
        (0x1d540, 0x91006260),
        (0x1d544, 0x6d412beb),
        (0x1d548, 0xa9457bfd),
        (0x1d54c, 0xa9444ff4),
        (0x1d550, 0xa94357f6),
        (0x1d554, 0xfc4607ec),
        (0x1d558, 0x140136d2),
        (0x1d2d4, 0x91006260),
        (0x1d2d8, 0x9401398a),
        (0x1d2dc, 0x3707ede0),
        (0x870bc, 0xb9401008),
        (0x870c0, 0x35000168),
        (0x870c4, 0x91006008),
        (0x870c8, 0xaa0003f3),
        (0x870cc, 0xc8dffd00),
        (0x870d0, 0x97fe85a4),
        (0x870d4, 0x7100081f),
        (0x870d8, 0x540000a1),
        (0x870dc, 0xf9400660),
        (0x870e0, 0x97fffca4),
        (0x870e4, 0x12800008),
        (0x870e8, 0xb9001268),
        (0x86380, 0xb9410808),
        (0x86384, 0xaa0003f3),
        (0x86388, 0x350003e8),
        (0x8638c, 0x900006e1),
        (0x86390, 0x9104c421),
        (0x86394, 0x910003e0),
        (0x86398, 0x320003e2),
        (0x8639c, 0x94025a19),
        (0x863a0, 0x9102e268),
        (0x863a4, 0xc8dffd00),
        (0x863a8, 0x910003e1),
        (0x863ac, 0x940247a5),
        (0x863b0, 0x360001e0),
        (0x863b4, 0x900006f4),
        (0x863b8, 0x9104c694),
        (0x863bc, 0xb9410a68),
        (0x863c0, 0x11000509),
        (0x863c4, 0x11000902),
        (0x863c8, 0x910003e0),
        (0x863cc, 0xaa1403e1),
        (0x863d0, 0xb9010a69),
        (0x863d4, 0x94025a0b),
        (0x863d8, 0x9102e268),
        (0x863dc, 0xc8dffd00),
        (0x863e0, 0x910003e1),
        (0x863e4, 0x94024797),
        (0x863e8, 0x3707fea0),
        (0x863ec, 0xb9410a68),
        (0x863f0, 0x5280bb89),
        (0x863f4, 0x1b097d08),
        (0x863f8, 0x51177108),
        (0x863fc, 0x1e220100),
        (0x86400, 0xbd016a60),
        (0x87590, 0xf9400413),
        (0x87594, 0x9105c260),
        (0x87598, 0x9400d116),
        (0x8759c, 0x360000c0),
        (0x875a0, 0x320003e0),
        (0x875a4, 0xa9457bfd),
        (0x875a8, 0xa9444ff4),
        (0x875ac, 0x910183ff),
        (0x875b0, 0xd65f03c0),
        (0x875b4, 0xb90003ff),
        (0x875b8, 0xf9405260),
        (0x875bc, 0xb4000640),
        (0x875c0, 0xf9400008),
        (0x875c4, 0xf9401908),
        (0x875c8, 0x910003e1),
        (0x875cc, 0x910003f4),
        (0x875d0, 0xd63f0100),
        (0x877ac, 0xd0000688),
        (0x877b0, 0xbd400020),
        (0x877b4, 0xf9400413),
        (0x877b8, 0xbd499d01),
        (0x877bc, 0x1e210800),
        (0x877c0, 0xbd40c261),
        (0x877c4, 0x1e202820),
        (0x877c8, 0x1e202008),
        (0x877cc, 0x320003e0),
        (0x877d0, 0xbd00c260),
        (0x877d4, 0x5400018c),
        (0x877d8, 0xb900c27f),
        (0x877dc, 0xb81fc3a0),
        (0x877e0, 0xf9405260),
        (0x877e4, 0xb4000160),
        (0x877e8, 0xf9400008),
        (0x877ec, 0xf9401908),
        (0x877f0, 0xd10013a1),
        (0x877f4, 0xd63f0100),
        (0x877f8, 0x2a1f03e0),
        (0x877fc, 0x320003e8),
        (0x87800, 0x3902c268),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "native instruction at {address:x}"
        );
    }
}
#[test]
fn pointer_rectangles_are_static_half_open_and_keep_packed_order() {
    for (characters, information) in [(-1, false), (0, false), (-1, true), (0, true)] {
        let model = SystemMenuModel::new(characters, information);
        for row in 0..model.items.len() {
            let y = 72.0 + row as f32 * 128.0;
            assert_eq!(model.row_at(glam::vec2(24.0, y)), Some(row));
            assert_eq!(model.row_at(glam::vec2(419.999, y + 119.999)), Some(row));
            for point in [
                glam::vec2(23.999, y),
                glam::vec2(420.0, y),
                glam::vec2(24.0, y + 120.0),
            ] {
                assert_eq!(model.row_at(point), None);
            }
            assert_eq!(model.render_rect(row).unwrap().x, -480.0);
        }
        assert_eq!(model.row_at(glam::vec2(f32::NAN, 80.0)), None);
    }
}
#[test]
fn pointer_press_leave_and_reset_match_native_flash_callbacks() {
    let mut model = active(-1, false);
    assert!(model.pointer_probe(glam::vec2(30.0, 72.0 + 3.0 * 128.0)));
    model.pointer_press();
    assert_eq!(model.selected(), 3);
    assert!(model.take_sounds().is_empty());
    model.update(1.0);
    assert!((model.feedback(3).unwrap().1 - 0.2).abs() < 0.00001);
    model.pointer_leave();
    model.update(1.0);
    assert_eq!(model.feedback(3).unwrap().1, 0.0);
    assert_eq!(model.pointer_row, None);
    model.pointer_probe(glam::vec2(30.0, 80.0));
    model.pointer_press();
    model.update(1.0);
    model.pointer_reset();
    model.update(2.5);
    assert_eq!(model.feedback(0).unwrap().1, 0.7);
    assert_eq!(model.pointer_row, None);
}
#[test]
fn pointer_release_uses_last_probe_and_blank_background_closes() {
    let mut model = active(-1, false);
    model.pointer_probe(glam::vec2(30.0, 80.0));
    model.pointer_press();
    model.update(1.0);
    // Explicitly exercise native callback state; this does not assert platform
    // delivery re-probes during a captured press.
    model.pointer_probe(glam::vec2(30.0, 72.0 + 3.0 * 128.0));
    model.pointer_release();
    assert_eq!(model.selected(), 3);
    assert_eq!(model.take_sounds(), ["decide"]);
    model.update(7.0);
    model.update(0.0);
    assert_eq!(model.take_item(), Some(5));
    let mut model = active(0, true);
    assert!(model.pointer_probe(glam::vec2(1200.0, 500.0)));
    model.pointer_press();
    assert_eq!(model.selected(), 0);
    model.pointer_release();
    assert_eq!(model.phase(), SystemMenuPhase::Closing);
    assert_eq!(model.take_sounds(), ["cancel"]);
}
#[test]
fn leftward_swipe_has_strict_speed_and_native_angle_gates() {
    for (velocity, closes) in [
        (glam::vec2(-20.0, 0.0), false),
        (glam::vec2(-20.001, 0.0), true),
        (glam::vec2(40.0, 0.0), false),
        (glam::vec2(0.0, 40.0), false),
        (glam::vec2(0.0, -40.0), false),
        (glam::vec2(-40.0, 22.0), true),
        (glam::vec2(-40.0, -22.0), true),
        (glam::vec2(-40.0, 24.0), false),
        (glam::vec2(-40.0, -24.0), false),
        (glam::vec2(f32::NAN, 0.0), false),
        (glam::vec2(-f32::INFINITY, 0.0), true),
    ] {
        let mut model = active(0, true);
        model.pointer_gesture(velocity);
        assert_eq!(
            model.phase() == SystemMenuPhase::Closing,
            closes,
            "{velocity:?}"
        );
        assert_eq!(
            model.take_sounds(),
            if closes { vec!["cancel"] } else { vec![] }
        );
    }
    let mut opening = SystemMenuModel::new(0, true);
    assert!(!opening.pointer_probe(glam::vec2(30.0, 80.0)));
    opening.pointer_gesture(glam::vec2(-100.0, 0.0));
    assert_eq!(opening.phase(), SystemMenuPhase::Opening);
    let mut confirming = active(0, true);
    confirming.confirm();
    confirming.pointer_gesture(glam::vec2(-100.0, 0.0));
    assert_eq!(confirming.phase(), SystemMenuPhase::Active);
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_system_menu_pointer_swipe_and_row_drawing() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x1e6b4, 0xb980b268),
        (0x1e6b8, 0x8b0802c8),
        (0x1e6bc, 0x1e271000),
        (0x1e6c0, 0x9e230101),
        (0x1e6c4, 0x4eaa1d42),
        (0x1e6c8, 0x4eab1d63),
        (0x1e6cc, 0x9402bc5d),
        (0x1e6d0, 0x1e212865),
        (0x1e6d4, 0x1e202068),
        (0x1e6d8, 0x1e202844),
        (0x1e6dc, 0x1e214ca3),
        (0x1e6e0, 0x1e254c2d),
        (0x1e6e4, 0x1e202048),
        (0x1e6e8, 0x1e204c81),
        (0x1e6ec, 0x1e244c0e),
        (0x1e6f0, 0x4ea11c20),
        (0x1e6f4, 0x4ea31c61),
        (0x1e6f8, 0x9402bc46),
        (0x1e6fc, 0x4ea01c0f),
        (0x1e700, 0x4ea11c2c),
        (0x1e704, 0x4eae1dc0),
        (0x1e708, 0x4ead1da1),
        (0x1e70c, 0x9402bc41),
        (0x1e710, 0x1e212120),
        (0x1e714, 0x1e294582),
        (0x1e718, 0x1e2895e2),
        (0x1e71c, 0x1e209500),
        (0x1e720, 0x54000144),
        (0x1e780, 0xf81e0ff3),
        (0x1e784, 0xa9017bfd),
        (0x1e788, 0x910043fd),
        (0x1e78c, 0xaa0003f3),
        (0x1e790, 0x97ffffac),
        (0x1e794, 0xb900b660),
        (0x1e798, 0xa9417bfd),
        (0x1e79c, 0x320003e0),
        (0x1e7a0, 0xf84207f3),
        (0x1e7a4, 0xd65f03c0),
        (0x1e7e0, 0x12800008),
        (0x1e7e4, 0xb900b408),
        (0x1e7e8, 0xd65f03c0),
        (0x1e80c, 0xb980b408),
        (0x1e810, 0x37f80268),
        (0x1e814, 0xf9405c09),
        (0x1e818, 0x5280170a),
        (0x1e81c, 0x9b0a2508),
        (0x1e820, 0xaa0003f3),
        (0x1e824, 0x91018100),
        (0x1e828, 0x940272fe),
        (0x1e82c, 0xf00009c8),
        (0x1e830, 0xbd45a500),
        (0x1e834, 0xf00009c8),
        (0x1e838, 0xbd497501),
        (0x1e83c, 0x321a03e1),
        (0x1e840, 0x2a1f03e2),
        (0x1e844, 0x9402746f),
        (0x1e848, 0xb940b668),
        (0x1e84c, 0xb940ae69),
        (0x1e850, 0x6b08013f),
        (0x1e854, 0x54000040),
        (0x1e858, 0xb900ae68),
        (0x1e8ec, 0xb940b408),
        (0x1e8f0, 0xaa0003f3),
        (0x1e8f4, 0x37f80128),
        (0x1e8f8, 0xb940ae69),
        (0x1e8fc, 0x6b08013f),
        (0x1e900, 0x54000040),
        (0x1e904, 0xb900ae68),
        (0x1e908, 0xa9417bfd),
        (0x1e90c, 0xaa1303e0),
        (0x1e910, 0xf84207f3),
        (0x1e914, 0x17fffb13),
        (0x1e918, 0xd0001028),
        (0x1e91c, 0xf943bd08),
        (0x1e920, 0xf9400100),
        (0x1e924, 0x1e2e1000),
        (0x1e928, 0x94002262),
        (0x1e92c, 0xaa1303e0),
        (0x1e930, 0x97fffdb8),
        (0x1e934, 0xb940aa68),
        (0x1e938, 0x11000508),
        (0x1e93c, 0xb900aa68),
        (0x1e9e0, 0xb980b408),
        (0x1e9e4, 0x37f80288),
        (0x1e9e8, 0xf81e0ff3),
        (0x1e9ec, 0xa9017bfd),
        (0x1e9f0, 0x910043fd),
        (0x1e9f4, 0xf9405c09),
        (0x1e9f8, 0x5280170a),
        (0x1e9fc, 0x9b0a2508),
        (0x1ea00, 0xaa0003f3),
        (0x1ea04, 0x91018100),
        (0x1ea08, 0x94027286),
        (0x1ea0c, 0xf00009c8),
        (0x1ea10, 0xbd497501),
        (0x1ea14, 0x1e2703e0),
        (0x1ea18, 0x321a03e1),
        (0x1ea1c, 0x2a1f03e2),
        (0x1ea20, 0x940273f8),
        (0x1ea24, 0x12800008),
        (0x1ea28, 0xb900b668),
        (0x1ea2c, 0xa9417bfd),
        (0x1ea30, 0xf84207f3),
        (0x1eab4, 0x4ea01c0a),
        (0x1eab8, 0x4ea11c29),
        (0x1eabc, 0x4ea11c20),
        (0x1eac0, 0x4eaa1d41),
        (0x1eac4, 0xaa0003f3),
        (0x1eac8, 0x9403f832),
        (0x1eacc, 0x4ea01c08),
        (0x1ead0, 0x1e2a0940),
        (0x1ead4, 0x1e290921),
        (0x1ead8, 0x1e212801),
        (0x1eadc, 0x1e21c020),
        (0x1eae0, 0x1e202000),
        (0x1eae4, 0x540000a6),
        (0x1eae8, 0x1e269001),
        (0x1eaec, 0x1e212000),
        (0x1eaf0, 0x540000ec),
        (0x1eaf4, 0x1400001e),
        (0x1eaf8, 0x4ea11c20),
        (0x1eafc, 0x9403f829),
        (0x1eb00, 0x1e269001),
        (0x1eb04, 0x1e212000),
        (0x1eb08, 0x5400032d),
        (0x1eb0c, 0xf00009c8),
        (0x1eb10, 0xbd44bd00),
        (0x1eb14, 0xf00009c8),
        (0x1eb18, 0xbd481501),
        (0x1eb1c, 0xf00009c8),
        (0x1eb20, 0x1e201900),
        (0x1eb24, 0x1e210800),
        (0x1eb28, 0xbd4a2101),
        (0x1eb2c, 0x1e212000),
        (0x1eb30, 0x540000ac),
        (0x1eb34, 0xf00009c8),
        (0x1eb38, 0xbd443501),
        (0x1eb3c, 0x1e212000),
        (0x1eb40, 0x54000165),
        (0x1eb44, 0xd0001028),
        (0x1eb48, 0xf943bd08),
        (0x1eb4c, 0xf9400100),
        (0x1eb50, 0x1e2e1000),
        (0x1eb54, 0x940021d7),
        (0x1eb58, 0xaa1303e0),
        (0x1eb5c, 0x97fffd2d),
        (0x1eb60, 0xb940aa68),
        (0x1eb64, 0x11000508),
        (0x1eb68, 0xb900aa68),
        (0x1ec70, 0xb980b408),
        (0x1ec74, 0x37f80288),
        (0x1ec78, 0xf81e0ff3),
        (0x1ec7c, 0xa9017bfd),
        (0x1ec80, 0x910043fd),
        (0x1ec84, 0xf9405c09),
        (0x1ec88, 0x5280170a),
        (0x1ec8c, 0x9b0a2508),
        (0x1ec90, 0xaa0003f3),
        (0x1ec94, 0x91018100),
        (0x1ec98, 0x940271e2),
        (0x1ec9c, 0xf00009c8),
        (0x1eca0, 0xbd497501),
        (0x1eca4, 0x1e2703e0),
        (0x1eca8, 0x321a03e1),
        (0x1ecac, 0x2a1f03e2),
        (0x1ecb0, 0x94027354),
        (0x1ecb4, 0x12800008),
        (0x1ecb8, 0xb900b668),
        (0x1ecbc, 0xa9417bfd),
        (0x1ecc0, 0xf84207f3),
        (0x1d83c, 0xbc5b83a0),
        (0x1d840, 0x1e2029a2),
        (0x1d844, 0xbd400fe0),
        (0x1d848, 0x1e2029c3),
        (0x1d84c, 0x4ead1da0),
        (0x1d850, 0x4eae1dc1),
        (0x1d854, 0x910343e0),
        (0x1d858, 0x4ea81d0d),
        (0x1d85c, 0x9402a515),
        (0x1d860, 0xb980ae68),
        (0x1d864, 0xeb08031f),
        (0x1d868, 0x1e2c1000),
        (0x1d86c, 0xb84606a8),
        (0x1d870, 0x1b197d08),
        (0x1d874, 0x1e220101),
        (0x1d878, 0x1e2f1821),
        (0x1d87c, 0x4ea01c02),
        (0x1d880, 0x1e200d00),
        (0x1d884, 0x1e222802),
        (0x1d888, 0x1e2a2823),
        (0x1d88c, 0x910343e0),
        (0x1d890, 0x9402a51c),
        (0x1d894, 0x4eab1d60),
        (0x1d898, 0x4eab1d61),
        (0x1d89c, 0x4eab1d62),
        (0x1d8a0, 0x4eac1d83),
        (0x1d8a4, 0x9400e3ab),
        (0x1d8a8, 0x910243e0),
        (0x1d8ac, 0x2d1207e0),
        (0x1d8b0, 0x2d130fe2),
        (0x1d8b4, 0x9400e3e3),
        (0x1d8b8, 0x2a0003e1),
        (0x1d8bc, 0x910343e0),
        (0x1d8c0, 0x9402a51c),
        (0x1d8c4, 0xaa1503e0),
        (0x1d8c8, 0x940276f6),
        (0x1d8cc, 0x4ea01c03),
        (0x1d8d0, 0x4eab1d60),
        (0x1d8d4, 0x4eab1d61),
        (0x1d8d8, 0x4eab1d62),
        (0x1d8dc, 0x9400e39d),
        (0x1d8e0, 0x910343e0),
        (0x1d8e4, 0x910243e1),
        (0x1d8e8, 0x2d1207e0),
        (0x1d8ec, 0x2d130fe2),
        (0x1d8f0, 0x9402a588),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "native{address:x}"
        );
    }
    for (address, expected) in [
        (0x1594bc, 3.141592025756836_f32),
        (0x159814, 180.0),
        (0x159a20, 150.0),
        (0x159434, -150.0),
        (0x1595a4, 0.7),
        (0x159974, 0.2),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
}
#[test]
fn dynamic_items_preserve_original_atlas_indices_and_geometry() {
    for (characters, information, expected) in [
        (-1, false, vec![0, 1, 2, 5, 6]),
        (0, false, vec![0, 1, 2, 3, 5, 6]),
        (-1, true, vec![0, 1, 2, 4, 5, 6]),
        (0, true, vec![0, 1, 2, 3, 4, 5, 6]),
    ] {
        let model = active(characters, information);
        assert_eq!(model.items(), expected);
        for (row, &item) in expected.iter().enumerate() {
            assert_eq!(
                model.hit_rect(row),
                Some(vec4(24.0, 72.0 + row as f32 * 128.0, 396.0, 120.0))
            );
            assert_eq!(model.render_rect(row), model.hit_rect(row));
            assert!((model.atlas_rect(row).unwrap().y - item as f32 / 7.0).abs() < 0.000001);
        }
        assert_eq!(model.render_rect(expected.len()), None);
    }
}
#[test]
fn opening_and_closing_follow_staggered_sine_clocks_and_next_frame_phase_check() {
    let mut model = SystemMenuModel::new(0, true);
    assert_eq!(model.render_rect(0).unwrap().x, -480.0);
    model.update(2.0);
    assert!(
        (model.feedback(0).unwrap().0 - (std::f32::consts::PI * 0.1).sin()).abs() < 0.000001
    );
    assert_eq!(model.feedback(1).unwrap().0, 0.0);
    assert_eq!(model.feedback(6).unwrap().0, 0.0);
    model.update(20.0);
    model.update(0.0);
    model.close();
    model.update(2.0);
    assert!(
        (model.feedback(0).unwrap().0 - (std::f32::consts::PI * 0.1).cos()).abs() < 0.000001
    );
    assert_eq!(model.feedback(1).unwrap().0, 1.0);
    model.update(20.0);
    assert_eq!(model.phase(), SystemMenuPhase::Closing);
    model.update(0.0);
    assert_eq!(model.phase(), SystemMenuPhase::Closed);
    assert_eq!(model.detail_feedback(), (1.0, 0.0));
    assert_eq!(model.take_sounds(), ["cancel"]);
}
#[test]
fn controller_priority_wraps_and_endpoints_match_native() {
    use AppAction as A;
    for characters in [-1, 0] {
        for information in [false, true] {
            for start in 0..active(characters, information).items.len() {
                for bits in 0..64 {
                    let mut model = active(characters, information);
                    model.selected = start;
                    let mut input = enum_map::EnumMap::<A, shin_input::ActionState>::default();
                    for (bit, action) in
                        [A::Cancel, A::Enter, A::Up, A::AnyDown, A::Left, A::Right]
                            .into_iter()
                            .enumerate()
                    {
                        input[action].is_clicked = bits & (1 << bit) != 0;
                        input[action].is_clicked_or_repeated = bits & (1 << bit) != 0;
                    }
                    model.input(&input);
                    if bits & 1 != 0 {
                        assert_eq!(model.phase, SystemMenuPhase::Closing);
                        assert_eq!(model.take_sounds(), ["cancel"]);
                    } else if bits & 2 != 0 {
                        assert!(model.confirmation);
                        assert_eq!(model.selected, start);
                        assert_eq!(model.take_sounds(), ["decide"]);
                    } else {
                        let count = model.items.len();
                        let expected = if bits & 4 != 0 {
                            (start + count - 1) % count
                        } else if bits & 8 != 0 {
                            (start + 1) % count
                        } else if bits & 16 != 0 {
                            0
                        } else if bits & 32 != 0 {
                            count - 1
                        } else {
                            start
                        };
                        assert_eq!(model.selected, expected);
                        assert_eq!(
                            model.take_sounds(),
                            if expected == start {
                                vec![]
                            } else {
                                vec!["move"]
                            }
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn confirmation_waits_for_seven_tick_flash_and_phase_poll_before_original_item() {
    let mut model = active(-1, false);
    model.selected = 3; // Packed3 is original item5.
    model.confirm();
    model.update(3.5);
    assert!((model.feedback(3).unwrap().1 - 0.7).abs() < 0.000001);
    assert_eq!(model.take_item(), None);
    model.update(3.5);
    assert_eq!(model.take_item(), None);
    model.update(0.0);
    assert_eq!(model.take_item(), Some(5));
    assert_eq!(model.take_item(), None);
    assert_eq!(model.take_sounds(), ["decide"]);
}
#[test]
fn y_closes_system_menu_before_confirm_and_directions() {
    let mut model = active(0, true);
    let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
    input[AppAction::CharacterState].is_clicked = true;
    input[AppAction::Enter].is_clicked = true;
    input[AppAction::AnyDown].is_clicked_or_repeated = true;
    model.input(&input);
    assert_eq!(model.phase(), SystemMenuPhase::Closing);
    assert_eq!(model.selected(), 0);
    assert_eq!(model.take_sounds(), ["cancel"]);
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_system_menu_controller_animation_and_geometry() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x1d2e0, 0x910083e8),
        (0x1d2e4, 0x94029daf),
        (0x1d2e8, 0xb94027e8),
        (0x1d2ec, 0x52801009),
        (0x1d2f0, 0x72a40009),
        (0x1d2f4, 0x6a09011f),
        (0x1d2f8, 0x540001e0),
        (0x1d2fc, 0xf0001028),
        (0x1d300, 0xf943bd08),
        (0x1d304, 0xf9400100),
        (0x1d308, 0x1e2e1000),
        (0x1d30c, 0x940027e9),
        (0x1d310, 0xaa1303e0),
        (0x1d314, 0x9400033f),
        (0x1d318, 0xb940aa68),
        (0x1d31c, 0x11000508),
        (0x1d320, 0xb900aa68),
        (0x1d324, 0xa94bd674),
        (0x1d328, 0xeb15029f),
        (0x1d32c, 0x54ffebc1),
        (0x1d330, 0x17ffff66),
        (0x1d334, 0x37e00108),
        (0x1d338, 0xb940ae68),
        (0x1d33c, 0xb9402be9),
        (0x1d340, 0x37c00169),
        (0x1d344, 0x37c801c9),
        (0x1d348, 0x37d00469),
        (0x1d34c, 0x36dfea69),
        (0x1d350, 0x14000017),
        (0x1d354, 0xaa1303e0),
        (0x1d358, 0x94000082),
        (0x1d35c, 0xa94bd674),
        (0x1d360, 0xeb15029f),
        (0x1d364, 0x54ffea01),
        (0x1d368, 0x17ffff58),
        (0x1d36c, 0x7100011f),
        (0x1d370, 0x540001ed),
        (0x1d374, 0x51000514),
        (0x1d378, 0x1400001a),
        (0x1d37c, 0xa94bae6a),
        (0x1d380, 0xcb0a016a),
        (0x1d384, 0x5286f4eb),
        (0x1d388, 0x72bd37ab),
        (0x1d38c, 0xd343fd4a),
        (0x1d390, 0x1b0b7d4a),
        (0x1d394, 0x11000509),
        (0x1d398, 0x6b0a013f),
        (0x1d39c, 0x1a88a7f4),
        (0x1d3a0, 0x6b14011f),
        (0x1d3a4, 0x54ffe7a0),
        (0x1d3a8, 0x1400000e),
        (0x1d3ac, 0xa94baa69),
        (0x1d3b0, 0xcb090149),
        (0x1d3b4, 0x5286f4ea),
        (0x1d3b8, 0x72bd37aa),
        (0x1d3bc, 0xd343fd29),
        (0x1d3c0, 0x1b0a7d29),
        (0x1d3c4, 0x51000534),
        (0x1d3c8, 0x6b14011f),
        (0x1d3cc, 0x54ffe660),
        (0x1d3d0, 0x14000004),
        (0x1d3d4, 0x2a1f03f4),
        (0x1d3d8, 0x6b14011f),
        (0x1d3dc, 0x54ffe5e0),
        (0x1d3e0, 0xf0001028),
        (0x1d3e4, 0xf943c108),
        (0x1d3e8, 0xf9400100),
        (0x1d3ec, 0x1e2e1000),
        (0x1d3f0, 0x940027b0),
        (0x1d3f4, 0xb940ae68),
        (0x1d3f8, 0x6b14011f),
        (0x1d3fc, 0x54ffe4e0),
        (0x1d400, 0xb900ae74),
        (0x1d404, 0xa94bd674),
        (0x1d408, 0xeb15029f),
        (0x1d40c, 0x54ffe4c1),
        (0x1d48c, 0x1e2703e8),
        (0x1d490, 0x1e2e1009),
        (0x1d494, 0x1e24900a),
        (0x1d498, 0x1e20100c),
        (0x1d49c, 0x4ea81d0b),
        (0x1d4a0, 0xd286f4f6),
        (0x1d4a4, 0xf2bd37b6),
        (0x1d4a8, 0xaa1f03f4),
        (0x1d4ac, 0x321d03f5),
        (0x1d4b0, 0xf2cde9b6),
        (0x1d4b4, 0xf2fa6f56),
        (0x1d4b8, 0x4ea81d00),
        (0x1d4bc, 0x4eab1d61),
        (0x1d4c0, 0x8b150100),
        (0x1d4c4, 0x321903e1),
        (0x1d4c8, 0x2a1f03e2),
        (0x1d4cc, 0x9402794d),
        (0x1d4d0, 0x4ea91d20),
        (0x1d4d4, 0x4eaa1d41),
        (0x1d4d8, 0x321f03e1),
        (0x1d4dc, 0x2a1f03e2),
        (0x1d4e0, 0x94027948),
        (0x1d4e4, 0xa94ba668),
        (0x1d4e8, 0xcb080129),
        (0x1d4ec, 0x9343fd29),
        (0x1d4f0, 0x9b167d29),
        (0x1d4f4, 0x1e2c296b),
        (0x1d4f8, 0x91000694),
        (0x1d4fc, 0x9102e2b5),
        (0x1d500, 0xeb09029f),
        (0x1d504, 0x54fffda3),
        (0x1d508, 0x1e249008),
        (0x1d50c, 0x1e2703e0),
        (0x1d510, 0x321f03e1),
        (0x1d514, 0x2a1f03e2),
        (0x1d518, 0x4ea81d01),
        (0x1d51c, 0x91036260),
        (0x1d520, 0x94027938),
        (0x1d524, 0x1e2e1000),
        (0x1d528, 0x4ea81d01),
        (0x1d52c, 0x9104c260),
        (0x1d530, 0x2a1f03e1),
        (0x1d534, 0x2a1f03e2),
        (0x1d538, 0x94027932),
        (0x1e038, 0x1e2703e8),
        (0x1e03c, 0x1e249009),
        (0x1e040, 0x1e20100b),
        (0x1e044, 0x4ea81d0a),
        (0x1e048, 0xd286f4f6),
        (0x1e04c, 0xf2bd37b6),
        (0x1e050, 0xaa1f03f4),
        (0x1e054, 0x321d03f5),
        (0x1e058, 0xf2cde9b6),
        (0x1e05c, 0xf2fa6f56),
        (0x1e060, 0x4ea81d00),
        (0x1e064, 0x4eaa1d41),
        (0x1e068, 0x8b150100),
        (0x1e06c, 0x321903e1),
        (0x1e070, 0x2a1f03e2),
        (0x1e074, 0x94027663),
        (0x1e078, 0x4ea81d00),
        (0x1e07c, 0x4ea91d21),
        (0x1e080, 0x320003e1),
        (0x1e084, 0x2a1f03e2),
        (0x1e088, 0x9402765e),
        (0x1e08c, 0xa94ba668),
        (0x1e090, 0xcb080129),
        (0x1e094, 0x9343fd29),
        (0x1e098, 0x9b167d29),
        (0x1e09c, 0x1e2b294a),
        (0x1e0a0, 0x91000694),
        (0x1e0a4, 0x9102e2b5),
        (0x1e0a8, 0xeb09029f),
        (0x1e0ac, 0x54fffda3),
        (0x1e0b0, 0x1e249008),
        (0x1e0b4, 0x1e2e1000),
        (0x1e0b8, 0x320003e1),
        (0x1e0bc, 0x2a1f03e2),
        (0x1e0c0, 0x4ea81d01),
        (0x1e0c4, 0x91036260),
        (0x1e0c8, 0x9402764e),
        (0x1e0cc, 0x4ea81d01),
        (0x1e0d0, 0x6d4123e9),
        (0x1e0d4, 0x9104c260),
        (0x1e0d8, 0xa9447bfd),
        (0x1e0dc, 0xa9434ff4),
        (0x1e0e0, 0x1e2703e0),
        (0x1e0e4, 0xa94257f6),
        (0x1e0e8, 0x2a1f03e1),
        (0x1e0ec, 0x2a1f03e2),
        (0x1e0f0, 0x6cc52beb),
        (0x1e0f4, 0x14027643),
        (0x1c570, 0xb9408668),
        (0x1c574, 0x37f80908),
        (0x1c578, 0x320007e8),
        (0x1c57c, 0x9100237c),
        (0x1c580, 0xb900ebe8),
        (0x1c694, 0xa9492668),
        (0x1c698, 0xeb09011f),
        (0x1c69c, 0x54000900),
        (0x1c6a0, 0x9103a3f4),
        (0x1c6a4, 0x321e03e8),
        (0x1c6a8, 0x9100229c),
        (0x1c6ac, 0xb900ebe8),
        (0x1cb80, 0x929ff6e8),
        (0x1cb84, 0xf2a00008),
        (0x1cb88, 0x3903427f),
        (0x1cb8c, 0xa90aa27f),
        (0x1d860, 0xb980ae68),
        (0x1d864, 0xeb08031f),
        (0x1d868, 0x1e2c1000),
        (0x1d86c, 0xb84606a8),
        (0x1d870, 0x1b197d08),
        (0x1d874, 0x1e220101),
        (0x1d878, 0x1e2f1821),
        (0x1d87c, 0x4ea01c02),
        (0x1d880, 0x1e200d00),
        (0x1d884, 0x1e222802),
        (0x1d888, 0x1e2a2823),
        (0x1d88c, 0x910343e0),
        (0x1d890, 0x9402a51c),
        (0x1f90c, 0xf9400413),
        (0x1f910, 0xb980ae68),
        (0x1f914, 0xf9405e69),
        (0x1f918, 0x5280170a),
        (0x1f91c, 0x9b0a2508),
        (0x1f920, 0x91018100),
        (0x1f924, 0x94027033),
        (0x1f928, 0x360000a0),
        (0x1f92c, 0x320003e0),
        (0x1f930, 0xa9417bfd),
        (0x1f934, 0xf84207f3),
        (0x1f938, 0xd65f03c0),
        (0x1f93c, 0xaa1303e0),
        (0x1f940, 0x97fff9f0),
        (0x1f944, 0x2a1f03e0),
        (0x1f948, 0xa9417bfd),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "native{address:x}"
        );
    }
    for (address, expected) in [
        (0x1597d8, 396.0f32),
        (0x159a18, 120.0),
        (0x1598fc, 504.0),
        (0x1597dc, 840.0),
        (0x1596cc, 1.0 / 7.0),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
    assert_eq!(
        &exe[0x164d50 + 256..0x164d57 + 256],
        &[0, 32, 45, 65, 80, 95, 127]
    );
    assert_eq!(&exe[0x15e877 + 256..0x15e87f + 256], b"sysmenu\0");
}
#[test]
#[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
fn reference_native_system_menu_supplied_atlas() {
    use shin_core::{
        format::rom::{IndexEntry, RomReader},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let candidates: Vec<_> = rom
        .traverse()
        .filter_map(|(path, entry)| {
            if path.ends_with(".txa") && !path.trim_start_matches('/').contains('/') {
                if let IndexEntry::File(file) = entry {
                    return Some((path, *file));
                }
            }
            None
        })
        .collect();
    for (path, file) in candidates {
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        if let Some(&index) = archive.name_to_index.get("sysmenu") {
            assert_eq!(archive.textures[index].dimensions(), (792, 840));
            eprintln!(
                "Native sysmenu atlas: {path},792x840,seven120px rows and two396px columns"
            );
            return;
        }
    }
    panic!("Supplied root archives contain no sysmenu atlas");
}
