use super::*;
use crate::adv::input_area::{PointerEvent, PointerKind};
use shin_core::{
    format::scenario::{
        info::{BustupId, CharacterBoxSegment, PictureId},
        instruction_elements::FromNumber,
    },
    time::Ticks,
};

fn catalog() -> BustupCatalog {
    use CharacterBoxSegment::*;
    let mut info = vec![
        Background {
            primary_picture_id: PictureId::from_number(0),
            secondary_picture_id_offset: 0,
        },
        Background {
            primary_picture_id: PictureId::from_number(1),
            secondary_picture_id_offset: 2,
        },
        EndDefinition,
    ];
    for c in 0..2 {
        for o in 0..2 {
            for p in 0..2 {
                for e in 0..2 {
                    info.push(Bustup {
                        bustup_id: BustupId::from_number(c * 8 + o * 4 + p * 2 + e),
                    });
                }
                info.push(if p == 0 {
                    EndExpressionGroup
                } else if o == 0 {
                    EndPoseGroup
                } else {
                    EndDefinition
                });
            }
        }
    }
    BustupCatalog::new(&info).unwrap()
}
fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id: 0,
        position: glam::ivec2(x, y),
    }
}
fn frame(
    input: &mut BustupInput,
    menu: &mut BustupMenu,
    c: &BustupCatalog,
    number: u32,
    time: f64,
    enabled: bool,
    events: &[PointerEvent],
    cancel: bool,
) -> bool {
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::Cancel].is_clicked = cancel;
    let mut eligible = enabled;
    let alive = input.input_frame(
        menu,
        c,
        &mut eligible,
        &PointerFrame {
            number,
            time_seconds: time,
            events,
        },
        &state,
    );
    assert!(!eligible);
    alive
}
#[test]
fn scaled_held_arrow_selects_on_release_and_keeps_pointer_priority() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let mut input = BustupInput::new();
    input.resize(&mut m, &c, (960, 540));
    // Outfit right arrow: original[1632,1712)x[408,488).
    frame(
        &mut input,
        &mut m,
        &c,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 830, 220)],
        true,
    );
    assert_eq!(m.row(), crate::title::bustup::menu::Row::Character);
    frame(&mut input, &mut m, &c, 1, 0.16, true, &[], true);
    assert_eq!(m.row(), crate::title::bustup::menu::Row::Outfit);
    assert_eq!(m.selection().indices(), (0, 0, 0, 0));
    assert!(m.take_sounds().is_empty());
    m.update(Ticks::from_f32(2.0));
    assert!((m.arrow_values()[3] - 0.4).abs() < 1e-6);
    assert!(frame(
        &mut input,
        &mut m,
        &c,
        2,
        0.18,
        true,
        &[event(PointerKind::Up, 0, 0)],
        true
    ));
    assert_eq!(m.selection().indices(), (0, 1, 0, 0));
    assert_eq!(m.take_sounds(), ["move"]);
    assert_eq!(
        m.take_requests(),
        [crate::title::bustup::menu::Request::LoadBustup(
            BustupId::from_number(4)
        )]
    );
    m.update(Ticks::from_f32(5.0));
    assert_eq!(m.arrow_values()[3], 0.0);
    // The now disabled endpoint arrow still captures, but cannot change focus.
    frame(
        &mut input,
        &mut m,
        &c,
        3,
        0.3,
        true,
        &[event(PointerKind::Down, 830, 220)],
        true,
    );
    frame(&mut input, &mut m, &c, 4, 0.5, true, &[], true);
    frame(
        &mut input,
        &mut m,
        &c,
        5,
        0.6,
        true,
        &[event(PointerKind::Up, 830, 220)],
        true,
    );
    assert!(m.take_sounds().is_empty());
    assert!(m.take_requests().is_empty());
    assert!(m.input_alive());
}
#[test]
fn labels_and_blank_capture_are_silent_and_duplicate_frames_do_not_release_twice() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let mut input = BustupInput::new();
    frame(
        &mut input,
        &mut m,
        &c,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 1550, 740)],
        true,
    );
    let up = [event(PointerKind::Up, 1800, 60)];
    frame(&mut input, &mut m, &c, 1, 0.05, true, &up, true);
    assert_eq!(m.row(), crate::title::bustup::menu::Row::Background);
    assert!(m.take_sounds().is_empty());
    assert!(m.take_requests().is_empty());
    frame(&mut input, &mut m, &c, 1, 0.05, true, &up, true);
    assert!(m.input_alive());
    // Blank scene background also consumes a simultaneous controller Cancel.
    frame(
        &mut input,
        &mut m,
        &c,
        2,
        0.1,
        true,
        &[event(PointerKind::Down, 10, 10)],
        true,
    );
    frame(
        &mut input,
        &mut m,
        &c,
        3,
        0.12,
        true,
        &[event(PointerKind::Up, 10, 10)],
        true,
    );
    assert!(m.input_alive());
    assert!(m.take_sounds().is_empty());
}
#[test]
fn drag_disable_cancel_and_resize_fade_held_highlights_without_activation() {
    for reason in 0..4 {
        let c = catalog();
        let mut m = BustupMenu::new(&c);
        let mut input = BustupInput::new();
        frame(
            &mut input,
            &mut m,
            &c,
            0,
            0.0,
            true,
            &[event(PointerKind::Down, 1800, 60)],
            false,
        );
        frame(&mut input, &mut m, &c, 1, 0.16, true, &[], false);
        m.update(Ticks::from_f32(2.0));
        assert!((m.button_values()[0] - 0.4).abs() < 1e-6);
        match reason {
            0 => {
                frame(
                    &mut input,
                    &mut m,
                    &c,
                    2,
                    0.2,
                    true,
                    &[event(PointerKind::Move, 1700, 60)],
                    false,
                );
            }
            1 => {
                frame(&mut input, &mut m, &c, 2, 0.2, false, &[], false);
            }
            2 => {
                frame(
                    &mut input,
                    &mut m,
                    &c,
                    2,
                    0.2,
                    true,
                    &[event(PointerKind::Cancel, 0, 0)],
                    false,
                );
            }
            _ => input.resize(&mut m, &c, (960, 540)),
        }
        m.update(Ticks::from_f32(2.0));
        assert_eq!(m.button_values(), [0.0, 0.0]);
        frame(
            &mut input,
            &mut m,
            &c,
            3,
            0.3,
            true,
            &[event(PointerKind::Up, 1800, 60)],
            false,
        );
        m.prepare_frame();
        assert!(m.input_alive());
        assert!(m.take_sounds().is_empty());
        assert!(m.take_requests().is_empty());
    }
}
#[test]
fn pointer_close_uses_decide_and_transition_waits_held_flash_remainder() {
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let mut input = BustupInput::new();
    frame(
        &mut input,
        &mut m,
        &c,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 1800, 60)],
        false,
    );
    frame(&mut input, &mut m, &c, 1, 0.16, true, &[], false);
    m.update(Ticks::from_f32(3.5));
    assert_eq!(m.button_values()[0], 0.7);
    assert!(!frame(
        &mut input,
        &mut m,
        &c,
        2,
        0.2,
        true,
        &[event(PointerKind::Up, 0, 0)],
        true
    ));
    assert_eq!(m.take_sounds(), ["decide"]);
    m.update(Ticks::from_f32(3.0));
    m.prepare_frame();
    assert!(m.take_requests().is_empty());
    m.update(Ticks::from_f32(0.5));
    m.prepare_frame();
    assert_eq!(
        m.take_requests(),
        [crate::title::bustup::menu::Request::ReturnTitle {
            argument: 10,
            music_stop_ticks: 30
        }]
    );
}

#[test]
fn sprite_gallery_physical_h_j_change_characters_and_k_l_do_not() {
    use shin_input::{Action, RawInputState};
    use winit::keyboard::KeyCode;
    let c = catalog();
    let mut m = BustupMenu::new(&c);
    let mut input = BustupInput::new();
    for (number, (key, expected)) in [
        (KeyCode::KeyL, 0),
        (KeyCode::KeyJ, 1),
        (KeyCode::KeyK, 1),
        (KeyCode::KeyH, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut raw = RawInputState::new();
        raw.keyboard.insert(key);
        let state = AppAction::lower(&raw).map(|_, held| ActionState {
            is_clicked: held,
            is_clicked_or_repeated: held,
            ..Default::default()
        });
        let mut eligible = true;
        assert!(input.input_frame(
            &mut m,
            &c,
            &mut eligible,
            &PointerFrame {
                number: number as u32,
                time_seconds: number as f64,
                events: &[]
            },
            &state
        ));
        assert_eq!(m.selection().indices().0, expected, "{key:?}");
        m.update(Ticks::from_f32(100.0));
    }
}
