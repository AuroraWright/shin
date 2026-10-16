use super::*;
fn input() -> Input {
    Input {
        permission: true,
        first_row: 0,
        last_row: 15,
        ..Input::default()
    }
}
fn active() -> Controller {
    let mut c = Controller::new(16, &(0..16).collect::<Vec<_>>(), 5, 7).unwrap();
    c.update(5., input());
    c
}
#[test]
fn sparse_marker_identity_and_cursor_entries_preserve_parent_metadata_indices() {
    let c = Controller::new(16, &[2, 9], 5, 7).unwrap();
    assert_eq!(
        c.entries(),
        &[
            Entry {
                kind: 0,
                marker_index: -1,
                row: 0
            },
            Entry {
                kind: 1,
                marker_index: 2,
                row: 2
            },
            Entry {
                kind: 97,
                marker_index: -1,
                row: 5
            },
            Entry {
                kind: 98,
                marker_index: -1,
                row: 7
            },
            Entry {
                kind: 1,
                marker_index: 9,
                row: 9
            },
            Entry {
                kind: 100,
                marker_index: -1,
                row: 15
            }
        ]
    );
    let c = Controller::new(1, &[], 0, 0).unwrap();
    assert_eq!(
        c.entries().iter().map(|e| e.kind).collect::<Vec<_>>(),
        [0, 99, 100]
    );
}
#[test]
fn opening_admits_same_frame_input_and_close_waits_for_parent_flash() {
    let mut c = Controller::new(16, &[], 5, 7).unwrap();
    let events = c.update(
        5.,
        Input {
            pressed: 1 << 29,
            ..input()
        },
    );
    assert!(events.contains(&Event::Back));
    assert_eq!(c.phase(), Phase::Closing);
    c.update(
        8.,
        Input {
            parent_close_busy: true,
            ..input()
        },
    );
    assert_eq!(c.progress(), 1.);
    c.update(5., input());
    assert_eq!((c.phase(), c.progress()), (Phase::Finished, 0.));
}
#[test]
fn navigation_overrides_close_and_permission_capture_scroll_gate_commands() {
    let mut c = active();
    let old = c.selected();
    let events = c.update(
        0.,
        Input {
            repeated: 1 << 25,
            pressed: 1 << 29,
            ..input()
        },
    );
    assert_eq!(c.selected(), old + 1);
    assert!(events.contains(&Event::Move));
    assert!(!events.contains(&Event::Back));
    assert_eq!(
        c.update(
            0.,
            Input {
                pressed: 1 << 29,
                ..input()
            }
        ),
        [Event::DisableInput]
    );
    for _ in 0..4 {
        c.update(
            0.,
            Input {
                permission: false,
                ..input()
            },
        );
    }
    assert!(
        c.update(
            1.,
            Input {
                captured: true,
                pressed: 1 << 29,
                ..input()
            }
        )
        .is_empty()
    );
    assert_eq!(c.phase(), Phase::Active);
}
#[test]
fn pointer_row_selection_ignores_keyboard_bounds_and_close_origin_is_explicit() {
    let mut c = active();
    c.probe(5000., 240.);
    let events = c.press();
    assert!(events.contains(&Event::ParentSelect { row: 0 }));
    c.probe(1766., 46.);
    assert_eq!(c.press(), [Event::CloseFlash { held: true }]);
    assert_eq!(c.release(), [Event::Decide, Event::CloseBrowser]);
    assert_eq!(c.phase(), Phase::Closing);
    c.reset();
    assert_eq!(c.parent_target(), -1);
    assert!(c.probe(1874., 154.));
}
#[test]
fn right_fling_closes_child_while_other_velocity_scrolls_and_probe_stops_it() {
    let mut c = active();
    c.reset();
    assert!(c.fling([0., 20.]).is_empty());
    assert_eq!(c.velocity(), 20.);
    c.probe(0., 300.);
    assert_eq!(c.velocity(), 0.);
    assert!(c.dragging());
    assert_eq!(c.hover(), None);
    assert!(c.fling([19.99, 0.]).is_empty());
    assert_eq!(c.fling([20., 0.]), [Event::Back]);
}
#[test]
fn final_yes_jump_and_empty_or_invalid_constructor_contracts() {
    assert!(Controller::new(0, &[], 0, 0).is_err());
    assert!(Controller::new(16, &[16], 0, 0).is_err());
    let mut c = active();
    assert!(c.jump_response(false, 0).is_empty());
    assert!(c.jump_response(true, 1).is_empty());
    assert_eq!(c.jump_response(true, 0), [Event::Jump { row: 5 }]);
    assert_eq!(c.phase(), Phase::Closing);
}
#[test]
fn chapter_text_styles_visibility_and_native_scrollbar_integer_quantization() {
    use artwork::*;
    let c = active();
    let f = frame(0.25, 60., c.entries(), c.selected(), 3, 12, |_| {
        Marker::Choice {
            title: "",
            text: "Choice",
        }
    });
    assert_eq!(f.rows.first().unwrap().index, 1);
    assert_eq!(f.rows.last().unwrap().index, 12);
    assert_eq!(f.rows[0].text, "Choice");
    assert_eq!(f.rows[0].color, [0.75 * 0.7, 1. * 0.7, 1. * 0.7, 0.5]);
    assert_eq!(f.rows[0].position, [1800., 63.]);
    assert_eq!(
        f.panel[1].colors,
        [0x00808080, 0xc0808080, 0x00808080, 0xc0808080]
    );
    assert_eq!(f.scrollbar.unwrap()[1].rect, [1808., 276., 1832., 708.]);
    assert_eq!(
        row_text(
            Entry {
                kind: 99,
                marker_index: -1,
                row: 0
            },
            Marker::Other
        )
        .0,
        "カーソル位置/ゲーム内現在位置"
    );
}
#[test]
fn contact_dispatch_occurs_after_phase_before_scroll_and_close_waits_next_frame() {
    let mut c = Controller::new(16, &(0..16).collect::<Vec<_>>(), 5, 7).unwrap();
    let events = c.update_with_contact(5., input(), |c| {
        assert_eq!(c.phase(), Phase::Active);
        c.probe(0., 240.);
        (true, c.press())
    });
    assert!(events.contains(&Event::ParentSelect { row: 0 }));
    assert!(!events.contains(&Event::DisableInput));
    // Own reveal60->0 has advanced its first literal tick after the callback.
    assert_eq!(c.scroll(), 45.);
    for _ in 0..3 {
        c.update(0., input());
    }
    let events = c.update_with_contact(1., input(), |c| {
        c.probe(1766., 46.);
        (true, c.release())
    });
    assert!(events.contains(&Event::CloseBrowser));
    assert_eq!((c.phase(), c.progress()), (Phase::Closing, 1.));
    c.update(1., input());
    assert_eq!(c.progress(), 0.8);
}
#[test]
fn denied_contact_hook_is_not_polled_and_captured_contact_suppresses_keyboard() {
    let mut c = active();
    c.update_with_contact(
        1.,
        Input {
            permission: false,
            ..input()
        },
        |_| panic!("permission denied"),
    );
    let old = c.selected();
    let events = c.update_with_contact(
        1.,
        Input {
            repeated: 1 << 25,
            pressed: 1 << 29,
            ..input()
        },
        |_| (true, vec![]),
    );
    assert!(events.is_empty());
    assert_eq!(c.selected(), old);
    assert_eq!(c.phase(), Phase::Active);
}
