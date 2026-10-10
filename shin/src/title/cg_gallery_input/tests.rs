use super::*;
use crate::adv::input_area::{PointerEvent, PointerKind};
use shin_core::{
    format::{
        save::SaveData,
        scenario::{
            info::{PictureBoxInfoItem, PictureId},
            instruction_elements::FromNumber,
            types::U16SmallList,
        },
        text::U16String,
    },
    time::Ticks,
};
fn gallery() -> CgGalleryList {
    let info = (0..24)
        .map(|id| PictureBoxInfoItem {
            name: U16String::new(format!("thumb{id}")),
            picture_ids: U16SmallList::from_contents([PictureId::from_number(id)]),
        })
        .collect();
    let mut save = SaveData::default();
    save.save_vectors.unlocked_cgs.add(0);
    CgGalleryList::new(&info, &save).unwrap()
}
fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id: 0,
        position: glam::ivec2(x, y),
    }
}
fn frame(
    input: &mut CgGalleryInput,
    g: &mut CgGalleryList,
    number: u32,
    time: f64,
    enabled: bool,
    events: &[PointerEvent],
    cancel: bool,
) {
    let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
    state[AppAction::Cancel].is_clicked = cancel;
    input.input_frame(
        g,
        enabled,
        &PointerFrame {
            number,
            time_seconds: time,
            events,
        },
        &state,
        0.0,
    );
}
#[test]
fn half_size_captured_tap_consumes_cancel_and_waits_row_flash() {
    let mut g = gallery();
    let mut input = CgGalleryInput::new();
    input.resize(&mut g, (960, 540));
    frame(
        &mut input,
        &mut g,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 70, 100)],
        true,
    );
    g.update(Ticks::ZERO);
    frame(&mut input, &mut g, 1, 0.16, true, &[], true);
    g.update(Ticks::from_f32(1.0));
    assert_eq!(g.selected(), 0);
    assert!(g.close_flash() == 0.0);
    assert!(!g.closing());
    frame(
        &mut input,
        &mut g,
        2,
        0.18,
        true,
        &[event(PointerKind::Up, 800, 500)],
        true,
    );
    assert_eq!(g.take_sounds(), ["decide"]);
    g.update(Ticks::from_f32(6.0));
    assert!(g.take_requests().is_empty());
    g.prepare_frame();
    assert_eq!(
        g.take_requests(),
        [super::super::cg_gallery::CgRequest::OpenRow(0)]
    );
}
#[test]
fn drag_contact_scrolls_and_release_integrates_fling_without_activation() {
    let mut g = gallery();
    let mut input = CgGalleryInput::new();
    frame(
        &mut input,
        &mut g,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 200, 300)],
        false,
    );
    g.update(Ticks::ZERO);
    frame(&mut input, &mut g, 1, 0.02, true, &[], false);
    g.update(Ticks::from_f32(1.0));
    frame(
        &mut input,
        &mut g,
        2,
        0.04,
        true,
        &[event(PointerKind::Move, 200, 260)],
        true,
    );
    assert_eq!(g.scroll(), 40.0);
    g.update(Ticks::from_f32(1.0));
    frame(
        &mut input,
        &mut g,
        3,
        0.06,
        true,
        &[event(PointerKind::Up, 200, 260)],
        true,
    );
    assert!(g.velocity() < 0.0);
    g.update(Ticks::from_f32(1.0));
    assert!(g.scroll() > 40.0);
    assert!(!g.closing());
    assert!(g.take_sounds().is_empty());
    assert!(g.take_requests().is_empty());
}
#[test]
fn resource_disable_and_resize_cancel_contacts_without_release_or_replay() {
    let mut g = gallery();
    let mut input = CgGalleryInput::new();
    frame(
        &mut input,
        &mut g,
        0,
        0.0,
        true,
        &[event(PointerKind::Down, 1800, 80)],
        false,
    );
    g.update(Ticks::ZERO);
    frame(&mut input, &mut g, 1, 0.16, true, &[], false);
    g.update(Ticks::from_f32(1.0));
    assert!(g.close_flash() > 0.0);
    frame(&mut input, &mut g, 2, 0.18, false, &[], true);
    g.update(Ticks::from_f32(1.0));
    assert_eq!(g.close_flash(), 0.0);
    input.resize(&mut g, (960, 540));
    frame(
        &mut input,
        &mut g,
        3,
        0.20,
        true,
        &[event(PointerKind::Up, 900, 40)],
        false,
    );
    assert!(!g.closing());
    assert!(g.take_sounds().is_empty());
    g.update(Ticks::ZERO);
    frame(
        &mut input,
        &mut g,
        4,
        0.22,
        true,
        &[event(PointerKind::Down, 900, 40)],
        false,
    );
    g.update(Ticks::ZERO);
    frame(&mut input, &mut g, 5, 0.38, true, &[], false);
    g.update(Ticks::from_f32(1.0));
    assert!(g.close_flash() > 0.0);
    input.resize(&mut g, (1920, 1080));
    g.update(Ticks::from_f32(1.0));
    frame(
        &mut input,
        &mut g,
        6,
        0.40,
        true,
        &[event(PointerKind::Up, 1800, 80)],
        false,
    );
    assert_eq!(g.close_flash(), 0.0);
    assert!(!g.closing());
    assert!(g.take_sounds().is_empty());
}

#[test]
fn picture_physical_h_j_page_and_k_l_do_not() {
    use shin_input::{Action, RawInputState};
    use winit::keyboard::KeyCode;
    let mut g = gallery();
    let mut input = CgGalleryInput::new();
    for (number, (key, selected)) in [
        (KeyCode::KeyL, 0),
        (KeyCode::KeyJ, 12),
        (KeyCode::KeyK, 12),
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
        input.input_frame(
            &mut g,
            true,
            &PointerFrame {
                number: number as u32,
                time_seconds: number as f64,
                events: &[],
            },
            &state,
            0.0,
        );
        assert_eq!(g.selected(), selected, "{key:?}");
        g.update(Ticks::from_f32(100.0));
    }
}
