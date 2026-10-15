use super::super::bustup_catalog::{Background, Character, Outfit, Pose};
use super::*;
use shin_core::format::scenario::instruction_elements::FromNumber;

fn catalog() -> BustupCatalog {
    BustupCatalog {
        backgrounds: (0..23)
            .map(|i| Background {
                primary: PictureId::from_number(i),
                secondary: (i == 1).then(|| PictureId::from_number(100)),
            })
            .collect(),
        characters: (0..3)
            .map(|c| Character {
                outfits: (0..2)
                    .map(|o| Outfit {
                        poses: (0..3)
                            .map(|p| Pose {
                                expressions: (0..13)
                                    .map(|e| {
                                        BustupId::from_number(c * 1000 + o * 100 + p * 20 + e / 2)
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[test]
fn authored_pointer_rows_labels_arrows_and_top_boundaries() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let geometry = [
        (1424.0, 1648.0, 280.0, 1504.0, 248.0, 144.0),
        (1440.0, 1632.0, 408.0, 1520.0, 392.0, 112.0),
        (1440.0, 1632.0, 520.0, 1520.0, 504.0, 112.0),
        (1440.0, 1632.0, 632.0, 1520.0, 616.0, 112.0),
        (1424.0, 1648.0, 760.0, 1504.0, 728.0, 144.0),
    ];
    for (row, (left, right, y, label_x, label_y, size)) in geometry.into_iter().enumerate() {
        m.pointer_probe(&c, glam::vec2(left + 40.0, y + 40.0));
        assert_eq!(m.pointer_selector, -1, "disabled left row{row}");
        // Upper-left belongs to the label; upper/right bounds are exclusive.
        m.pointer_probe(&c, glam::vec2(label_x, label_y));
        assert_eq!(m.pointer_selector, row as i32 * 3 + 2);
        m.pointer_release(&c);
        assert_eq!(m.focus, row);
        assert!(m.take_sounds().is_empty());
        assert!(m.take_requests().is_empty());
        assert_eq!(
            m.selector_hit(&c, glam::vec2(label_x + size - 0.01, label_y + size - 0.01)),
            row as i32 * 3 + 2
        );
        assert_eq!(
            m.selector_hit(&c, glam::vec2(label_x + 1.0, label_y + size)),
            if (1..4).contains(&row) {
                (row as i32 + 1) * 3 + 2
            } else {
                -1
            }
        );
        m.pointer_probe(&c, glam::vec2(right, y));
        assert_eq!(m.pointer_selector, row as i32 * 3 + 1);
        m.pointer_release(&c);
        assert_eq!(m.take_requests().len(), 1);
        assert_eq!(m.take_sounds(), ["move"]);
        assert_eq!(m.selector_hit(&c, glam::vec2(left, y)), row as i32 * 3);
        assert_eq!(m.selector_hit(&c, glam::vec2(right + 80.0, y)), -1);
        assert_eq!(m.selector_hit(&c, glam::vec2(right + 1.0, y + 80.0)), -1);
    }
    for (x, y, expected) in [
        (1764.0, 32.0, 1),
        (1847.99, 115.99, 1),
        (1848.0, 60.0, 0),
        (1764.0, 116.0, 0),
        (1668.0, 32.0, 2),
        (1751.99, 115.99, 2),
        (1752.0, 60.0, 0),
        (1668.0, 31.99, 0),
        (100.0, 162.0, -1),
        (100.0, 979.99, -1),
        (100.0, 980.0, 0),
    ] {
        assert!(m.pointer_probe(&c, glam::vec2(x, y)));
        assert_eq!(m.pointer_target, expected);
        assert_eq!(m.pointer_selector, -1);
    }
}
fn input(menu: &mut BustupMenu, c: &BustupCatalog, controls: Controls) -> bool {
    let mut eligible = true;
    let alive = menu.controller(c, &mut eligible, false, controls);
    assert!(!eligible);
    alive
}
fn focus(menu: &mut BustupMenu, c: &BustupCatalog, row: usize) {
    while menu.focus < row {
        input(
            menu,
            c,
            Controls {
                down: true,
                ..Default::default()
            },
        );
    }
    while menu.focus > row {
        input(
            menu,
            c,
            Controls {
                up: true,
                ..Default::default()
            },
        );
    }
    menu.take_sounds();
}
#[test]
fn native_priority_boundaries_and_input_eligibility() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    for (enabled, pointer) in [(false, false), (true, true)] {
        let mut eligibility = enabled;
        assert!(m.controller(
            &c,
            &mut eligibility,
            pointer,
            Controls {
                cancel: true,
                ..Default::default()
            }
        ));
        assert!(!eligibility);
        assert!(!m.retired);
    }
    input(
        &mut m,
        &c,
        Controls {
            up: true,
            down: true,
            right: true,
            ..Default::default()
        },
    );
    assert_eq!(m.row(), Row::Character);
    assert!(m.take_sounds().is_empty());
    assert!(m.take_requests().is_empty());
    focus(&mut m, &c, 4);
    input(
        &mut m,
        &c,
        Controls {
            down: true,
            right: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.background_index(), 0);
    input(
        &mut m,
        &c,
        Controls {
            left: true,
            right: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.background_index(), 0);
    input(
        &mut m,
        &c,
        Controls {
            page_up: true,
            page_down: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.background_index(), 0);
    assert!(m.take_requests().is_empty());
    assert!(!input(
        &mut m,
        &c,
        Controls {
            cancel: true,
            confirm: true,
            ..Default::default()
        }
    ));
    assert_eq!(m.take_sounds(), ["cancel"]);
    assert!(matches!(m.transition, Some(Transition::Close)));
    assert_eq!(m.panel_offset(), 0.0);
}
#[test]
fn every_selector_clamps_pages_and_preserves_parent_choices() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    for row in 0..5 {
        focus(&mut m, &c, row);
        input(
            &mut m,
            &c,
            Controls {
                page_down: true,
                ..Default::default()
            },
        );
        let (a, b, d, e) = m.selection.indices();
        let index = [a, b, d, e, m.selection.background_index()][row];
        assert_eq!(index, [2, 1, 2, 10, 10][row]);
        assert_eq!(m.take_sounds(), ["move"]);
        assert_eq!(m.take_requests().len(), 1);
        m.update(Ticks::from_f32(1.0));
        assert!((m.arrow_values()[row * 2 + 1] - 0.2).abs() < 1e-6);
    }
    focus(&mut m, &c, 0);
    input(
        &mut m,
        &c,
        Controls {
            page_down: true,
            ..Default::default()
        },
    );
    assert!(m.take_requests().is_empty());
    input(
        &mut m,
        &c,
        Controls {
            page_up: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.indices(), (0, 0, 0, 0));
    input(
        &mut m,
        &c,
        Controls {
            page_down: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.indices(), (2, 1, 2, 10));
    focus(&mut m, &c, 4);
    for _ in 0..3 {
        input(
            &mut m,
            &c,
            Controls {
                page_down: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(m.selection.background_index(), 22);
    for _ in 0..3 {
        input(
            &mut m,
            &c,
            Controls {
                page_up: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(m.selection.background_index(), 0);
    m.take_requests();
    input(
        &mut m,
        &c,
        Controls {
            right: true,
            ..Default::default()
        },
    );
    assert_eq!(
        m.take_requests(),
        [Request::LoadBackground {
            primary: PictureId::from_number(1),
            secondary: Some(PictureId::from_number(100)),
        }]
    );
}
#[test]
fn duplicate_expressions_still_load_and_reflash_from_current_value() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    focus(&mut m, &c, 3);
    input(
        &mut m,
        &c,
        Controls {
            right: true,
            ..Default::default()
        },
    );
    assert_eq!(m.selection.indices(), (0, 0, 0, 1));
    assert_eq!(
        m.take_requests(),
        [Request::LoadBustup(BustupId::from_number(0))]
    );
    m.update(Ticks::from_f32(4.5));
    assert!((m.arrow_values()[7] - 0.5).abs() < 1e-6);
    input(
        &mut m,
        &c,
        Controls {
            right: true,
            ..Default::default()
        },
    );
    assert!((m.arrow_values()[7] - 0.5).abs() < 1e-6);
    m.update(Ticks::from_f32(0.5));
    assert!((m.arrow_values()[7] - 0.6).abs() < 1e-6);
}
#[test]
fn transitions_wait_for_flash_and_retire_only_menu_input() {
    let c = catalog();
    for viewer in [false, true] {
        let mut m = BustupMenu::new(&c);
        assert!(!input(
            &mut m,
            &c,
            Controls {
                confirm: viewer,
                cancel: !viewer,
                ..Default::default()
            }
        ));
        assert!(m.has_blocking_transition());
        m.prepare_frame();
        assert!(m.take_requests().is_empty());
        m.update(Ticks::from_f32(6.5));
        m.prepare_frame();
        assert!(m.take_requests().is_empty());
        m.update(Ticks::from_f32(0.5));
        assert!(m.take_requests().is_empty());
        assert!(m.has_blocking_transition());
        m.prepare_frame();
        assert!(!m.has_blocking_transition());
        assert_eq!(
            m.take_requests(),
            [if viewer {
                Request::OpenViewer
            } else {
                Request::ReturnTitle {
                    argument: 10,
                    music_stop_ticks: 30,
                }
            }]
        );
        assert!(!input(
            &mut m,
            &c,
            Controls {
                right: true,
                ..Default::default()
            }
        ));
        m.prepare_frame();
        assert!(m.take_requests().is_empty());
        if viewer {
            m.update(Ticks::from_f32(8.0));
            assert!((m.panel_offset() - 500.0 / 2.0f32.sqrt()).abs() < 1e-4);
            m.update(Ticks::from_f32(15.0));
            assert_eq!(m.panel_offset(), 500.0);
        }
    }
}

#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_original_bustup_controller_selectors_and_transition_tasks() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let c = BustupCatalog::new(&scenario.info_tables().character_box_info).unwrap();
    let mut m = BustupMenu::new(&c);
    // Walk every authored child boundary through the actual dispatcher. At each
    // level the page jump must clamp, then a single step at the endpoint is silent.
    for ci in 0..c.characters.len() {
        focus(&mut m, &c, 0);
        while m.selection.indices().0 < ci {
            input(
                &mut m,
                &c,
                Controls {
                    right: true,
                    ..Default::default()
                },
            );
        }
        for oi in 0..c.characters[ci].outfits.len() {
            focus(&mut m, &c, 1);
            while m.selection.indices().1 < oi {
                input(
                    &mut m,
                    &c,
                    Controls {
                        right: true,
                        ..Default::default()
                    },
                );
            }
            for pi in 0..c.characters[ci].outfits[oi].poses.len() {
                focus(&mut m, &c, 2);
                while m.selection.indices().2 < pi {
                    input(
                        &mut m,
                        &c,
                        Controls {
                            right: true,
                            ..Default::default()
                        },
                    );
                }
                focus(&mut m, &c, 3);
                let ids = &c.characters[ci].outfits[oi].poses[pi].expressions;
                for (ei, id) in ids.iter().enumerate() {
                    if ei > 0 {
                        input(
                            &mut m,
                            &c,
                            Controls {
                                right: true,
                                ..Default::default()
                            },
                        );
                    }
                    assert_eq!(m.selection.indices(), (ci, oi, pi, ei));
                    assert_eq!(m.selection.bustup(&c), *id);
                    if ei > 0 {
                        assert_eq!(m.take_requests().last(), Some(&Request::LoadBustup(*id)));
                    } else {
                        m.take_requests();
                    }
                }
                m.take_sounds();
                input(
                    &mut m,
                    &c,
                    Controls {
                        page_down: true,
                        ..Default::default()
                    },
                );
                assert!(m.take_requests().is_empty());
                assert!(m.take_sounds().is_empty());
                input(
                    &mut m,
                    &c,
                    Controls {
                        page_up: true,
                        ..Default::default()
                    },
                );
                assert_eq!(m.selection.indices().3, (ids.len() - 1).saturating_sub(10));
                while m.selection.indices().3 > 0 {
                    input(
                        &mut m,
                        &c,
                        Controls {
                            page_up: true,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    focus(&mut m, &c, 4);
    for (i, bg) in c.backgrounds.iter().enumerate() {
        m.take_requests();
        if i > 0 {
            input(
                &mut m,
                &c,
                Controls {
                    right: true,
                    ..Default::default()
                },
            );
            assert_eq!(
                m.take_requests(),
                [Request::LoadBackground {
                    primary: bg.primary,
                    secondary: bg.secondary
                }]
            );
        }
        assert_eq!(m.selection.background_index(), i);
    }
    m.take_sounds();
    input(
        &mut m,
        &c,
        Controls {
            right: true,
            ..Default::default()
        },
    );
    assert!(m.take_requests().is_empty());
    assert!(m.take_sounds().is_empty());
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x39e94, 0x39400028),
        (0x39ea4, 0x34000288),
        (0x39ea8, 0x9400c696),
        (0x39eac, 0x37000a60),
        (0x39ebc, 0x37e80208),
        (0x39ec0, 0x37e002e8),
        (0x39ec8, 0x37c003a8),
        (0x39ecc, 0x37c80408),
        (0x39ed0, 0x37d00548),
        (0x39ed4, 0x37d80628),
        (0x39ed8, 0x37500a28),
        (0x39edc, 0x365808e8),
        (0x39ee0, 0x52800141),
        (0x39ef4, 0x9400c46b),
        (0x39f3c, 0xb9427a68),
        (0x39f40, 0x71000508),
        (0x39f50, 0x71000d1f),
        (0x39f54, 0x5400052c),
        (0x39f78, 0x12800001),
        (0x39f9c, 0x320003e1),
        (0x39fc0, 0x94020518),
        (0x39fd8, 0x321a03e1),
        (0x39ff8, 0x3900029f),
        (0x39ffc, 0x39436268),
        (0x3a010, 0x1a9f17e0),
        (0x3a01c, 0x12800121),
        (0x3a31c, 0xb9427808),
        (0x3a378, 0x1a88b148),
        (0x3a380, 0x1a9fc108),
        (0x3a394, 0xb9025a68),
        (0x3a40c, 0x1a9fc129),
        (0x3a490, 0x1a9fc14a),
        (0x3a4a0, 0x7828792a),
        (0x3a4b0, 0xb9425269),
        (0x3a4d0, 0x1a9fc148),
        (0x3a4e0, 0xb9025268),
        (0x3a4fc, 0x940000f1),
        (0x3a570, 0x1a9fc129),
        (0x3a580, 0x79000109),
        (0x3a59c, 0x94000005),
        (0x3a6d0, 0x52802028),
        (0x3a6dc, 0x79008268),
        (0x3a9fc, 0x52802029),
        (0x3aa08, 0x79008289),
        (0x3a04c, 0x39036008),
        (0x3a1ac, 0x39036008),
        (0x3a1f4, 0x1e27d001),
        (0x3a1fc, 0x321f03e1),
        (0x3dd14, 0x91052100),
        (0x3dd18, 0x9401f736),
        (0x3dd40, 0x321f0fe1),
        (0x3dd48, 0x52800140),
        (0x3debc, 0x9401f6cd),
        (0x3df28, 0x97fffa6e),
        (0x3df38, 0x9403084e),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup controls {a:x}"
        );
    }
    assert_eq!(&exe[0x165d5f + 256..0x165d5f + 261], &[0, 116, 22, 55, 89]);
    for (a, expected) in [(0x1595a4, 0.7f32), (0x159974, 0.2), (0x1597e4, 500.0)] {
        assert_eq!(
            f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected
        );
    }
}

#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_original_pointer_hit_eligibility_and_native_callbacks() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let c = BustupCatalog::new(&scenario.info_tables().character_box_info).unwrap();
    let mut m = BustupMenu::new(&c);
    let mut checked = 0;
    for (ci, character) in c.characters.iter().enumerate() {
        m.selection.select_character(ci);
        for (oi, outfit) in character.outfits.iter().enumerate() {
            m.selection.select_outfit(oi);
            for (pi, pose) in outfit.poses.iter().enumerate() {
                m.selection.select_pose(pi);
                for ei in 0..pose.expressions.len() {
                    m.selection.select_expression(&c, ei);
                    let indices = [ci, oi, pi, ei];
                    let counts = [
                        c.characters.len(),
                        character.outfits.len(),
                        outfit.poses.len(),
                        pose.expressions.len(),
                    ];
                    for row in 0..4 {
                        let (left, right, y) = if row == 0 {
                            (1464.0, 1688.0, 320.0)
                        } else {
                            (1480.0, 1672.0, 448.0 + (row - 1) as f32 * 112.0)
                        };
                        assert_eq!(
                            m.selector_hit(&c, glam::vec2(left, y)),
                            if indices[row] > 0 { row as i32 * 3 } else { -1 }
                        );
                        assert_eq!(
                            m.selector_hit(&c, glam::vec2(right, y)),
                            if indices[row] + 1 < counts[row] {
                                row as i32 * 3 + 1
                            } else {
                                -1
                            }
                        );
                        checked += 2;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 8195 * 8);
    for i in 0..c.backgrounds.len() {
        m.selection.select_background(&c, i);
        assert_eq!(
            m.selector_hit(&c, glam::vec2(1464.0, 800.0)),
            if i > 0 { 12 } else { -1 }
        );
        assert_eq!(
            m.selector_hit(&c, glam::vec2(1688.0, 800.0)),
            if i + 1 < c.backgrounds.len() { 13 } else { -1 }
        );
    }
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (a, expected) in [
        (0x3b478, 0x940248e6),
        (0x3b48c, 0xf9400508),
        (0x3b494, 0x79400115),
        (0x3b4a0, 0x78757914),
        (0x3b510, 0x1e212100),
        (0x3b514, 0x1e2845e2),
        (0x3b518, 0x1e2995c2),
        (0x3b51c, 0x1e209520),
        (0x3b524, 0x321f03e0),
        (0x3b52c, 0x72003edf),
        (0x3b5b4, 0x2a1f03e0),
        (0x3b5e4, 0x51000929),
        (0x3b5e8, 0x6b08013f),
        (0x3b674, 0x528000a0),
        (0x3b6fc, 0x320003e0),
        (0x3b704, 0x34000477),
        (0x3b788, 0x320007e0),
        (0x3b7bc, 0x51000908),
        (0x3b7c0, 0x6b17011f),
        (0x3b848, 0x321e03e0),
        (0x3b8c4, 0x321d03e0),
        (0x3b8cc, 0x34000475),
        (0x3b950, 0x321f07e0),
        (0x3b9a0, 0x51000908),
        (0x3b9a4, 0x6b15011f),
        (0x3ba2c, 0x32000be0),
        (0x3baa8, 0x52800160),
        (0x3bab0, 0x34000474),
        (0x3bb34, 0x52800120),
        (0x3bb90, 0x51000908),
        (0x3bb94, 0x6b14011f),
        (0x3bc1c, 0x52800140),
        (0x3bc9c, 0x321f0be0),
        (0x3bca8, 0x7100051f),
        (0x3bd30, 0x321e07e0),
        (0x3bd4c, 0x51000929),
        (0x3bd50, 0x6b09011f),
        (0x3bdd4, 0x5a9f4100),
        (0x3be10, 0xb900e408),
        (0x3be24, 0x5400042b),
        (0x3be34, 0x540003a5),
        (0x3be44, 0x5400016b),
        (0x3be54, 0x540000e5),
        (0x3be5c, 0xb900e268),
        (0x3be94, 0xb900e268),
        (0x3bed4, 0xb900e269),
        (0x3bee0, 0x97fffd5c),
        (0x3c004, 0xf9007008),
        (0x3c00c, 0xb900e808),
        (0x3c048, 0xb940e008),
        (0x3c08c, 0xb9027809),
        (0x3c090, 0x540001e8),
        (0x3c0ac, 0x9401fcdd),
        (0x3c0c0, 0x321a03e1),
        (0x3c120, 0xf9437108),
        (0x3c140, 0x17fff7bc),
        (0x3c15c, 0xb9027a69),
        (0x3c16c, 0x54000348),
        (0x3c178, 0x5a880501),
        (0x3c180, 0x97fff864),
        (0x3c184, 0x36000280),
        (0x3c208, 0x17fff7e2),
        (0x3c220, 0xd65f03c0),
        (0x3c250, 0xb940e008),
        (0x3c280, 0x9401fde0),
        (0x3c29c, 0x9401fdd9),
        (0x3c2a0, 0xb900e27f),
        (0x3c2dc, 0xb9427a69),
        (0x3c310, 0xf80e4268),
        (0x3c340, 0xd65f03c0),
        (0x3c370, 0xb940e008),
        (0x3c3c0, 0xb900e27f),
        (0x3c430, 0xf80e4268),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup pointer {a:x}"
        );
    }
    for (a, expected) in [
        (0x159694, 32.0f32),
        (0x159658, 116.0f32),
        (0x159480, 1764.0f32),
        (0x15971c, 1848.0f32),
        (0x1596e8, 1752.0f32),
        (0x1593e8, 1668.0f32),
        (0x159664, 162.0f32),
        (0x15940c, 980.0f32),
        (0x159388, 1504.0f32),
        (0x1597a4, 248.0f32),
        (0x159a7c, 144.0f32),
        (0x1596d8, 1424.0f32),
        (0x15969c, 280.0f32),
        (0x159a68, 80.0f32),
        (0x1596dc, 1520.0f32),
        (0x159884, 392.0f32),
        (0x159484, 112.0f32),
        (0x159a74, 1648.0f32),
        (0x1599d4, 1440.0f32),
        (0x1598bc, 408.0f32),
        (0x1597a8, 1632.0f32),
        (0x1598fc, 504.0f32),
        (0x15994c, 520.0f32),
        (0x1596e0, 616.0f32),
        (0x1596e4, 632.0f32),
        (0x1594cc, 728.0f32),
        (0x159984, 760.0f32),
    ] {
        assert_eq!(
            f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
            expected,
            "Native Bustup hit geometry {a:x}"
        );
    }
}

#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_sprite_gallery_navigation_uses_move_in_both_executables() {
    let dir = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap());
    for (name, global, loads) in [
        (
            "uncompressed_executable",
            0x224780usize,
            [0x39f64usize, 0x3a4e8, 0x3a588],
        ),
        (
            "uncompressed_executable_update",
            0x2267e0usize,
            [0x39ec4usize, 0x3a448, 0x3a4e8],
        ),
    ] {
        let file = std::fs::read(dir.parent().unwrap().join(name)).unwrap();
        let exe = &file[256..];
        let u64_at = |at: usize| u64::from_le_bytes(exe[at..at + 8].try_into().unwrap());
        let relocate = |address: usize| {
            (0..exe.len() - 24)
                .step_by(8)
                .find(|&at| u64_at(at) == address as u64 && u64_at(at + 8) == 0x403)
                .map(|at| u64_at(at + 16) as usize)
                .unwrap()
        };
        let cue = relocate(relocate(global));
        assert_eq!(&exe[cue..cue + 5], b"move\0", "{name}");
        for at in loads {
            let instruction = u32::from_le_bytes(exe[at..at + 4].try_into().unwrap());
            assert_eq!(
                instruction,
                0xf9400108 | (((global & 0xfff) / 8) as u32) << 10,
                "{name} at {at:x}"
            );
        }
        let c = catalog();
        let mut m = BustupMenu::new(&c);
        input(
            &mut m,
            &c,
            Controls {
                down: true,
                ..Default::default()
            },
        );
        assert_eq!(m.take_sounds(), ["move"]);
        input(
            &mut m,
            &c,
            Controls {
                right: true,
                ..Default::default()
            },
        );
        assert_eq!(m.take_sounds(), ["move"]);
        input(
            &mut m,
            &c,
            Controls {
                up: true,
                ..Default::default()
            },
        );
        assert_eq!(m.take_sounds(), ["move"]);
    }
}
