//! MusicMode315b0 consumes shared InputArea before controller/axis input.
use super::MusicGallery;
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::ActionState;

impl InputAreaTarget for MusicGallery {
    fn probe(&mut self, point: Vec2) -> bool {
        self.pointer_probe(point)
    }
    fn reset(&mut self) {
        self.pointer_reset();
    }
    fn press(&mut self, _: Vec2) {
        self.pointer_press();
    }
    fn release(&mut self, _: Vec2) {
        self.pointer_release();
    }
    fn long_press(&mut self, _: Vec2) {} //321d0 is a no-op.
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.pointer_drag(current.y - previous.y);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.pointer_gesture(velocity);
    }
    fn cancel(&mut self) {
        self.pointer_cancel();
    }
}
pub(crate) struct MusicGalleryInput {
    area: InputArea,
}
impl MusicGalleryInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, gallery: &mut MusicGallery, extent: (u16, u16)) {
        self.area.disable(gallery);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn input_frame(
        &mut self,
        gallery: &mut MusicGallery,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        gallery.prepare_frame();
        if !gallery.accepts_input() {
            self.area.disable(gallery);
            return;
        }
        if self.area.update(gallery, frame) || !gallery.controller_ready() {
            return;
        }
        if state[AppAction::Cancel].is_clicked {
            gallery.cancel();
        } else if state[AppAction::Enter].is_clicked {
            gallery.play();
        } else if state[AppAction::ConfigReset].is_clicked {
            gallery.stop();
        } else {
            let column = if state[AppAction::Left].is_clicked_or_repeated {
                -1
            } else if state[AppAction::Right].is_clicked_or_repeated {
                1
            } else {
                0
            };
            let row = if state[AppAction::Up].is_clicked_or_repeated {
                -1
            } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                1
            } else if state[AppAction::CharacterPageUp].is_clicked_or_repeated {
                -8
            } else if state[AppAction::CharacterPageDown].is_clicked_or_repeated {
                8
            } else {
                0
            };
            if column != 0 || row != 0 {
                gallery.navigate(column, row);
            } else {
                gallery.analog_scroll(axis);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::input_area::{PointerEvent, PointerKind};
    use shin_core::{
        format::{
            save::SaveData,
            scenario::{
                info::{BgmId, MusicBoxInfoItem},
                instruction_elements::FromNumber,
            },
        },
        time::Ticks,
    };
    fn gallery() -> MusicGallery {
        let items: Vec<_> = (0..25)
            .map(|i| MusicBoxInfoItem {
                bgm_id: BgmId::from_number(i),
                name_index: i as u16 + 1,
                once_flag: 0,
            })
            .collect();
        let mut save = SaveData::default();
        for i in 0..25 {
            save.save_vectors.unlocked_bgms.add(i);
        }
        let mut m = MusicGallery::new(&items, &save).unwrap();
        m.publish_resources();
        m.update(Ticks::from_f32(60.0), false);
        m.update(Ticks::ZERO, false);
        m.take_requests();
        m
    }
    #[test]
    fn trackpad_wheel_scrolls_music_gallery_through_live_input_dispatch() {
        let mut gallery = gallery();
        let mut input = MusicGalleryInput::new();
        for (number, (amount, expected)) in [
            (-3.25, 104.0),
            (-0.125, 108.0),
            (3.25, 4.0),
            (0.0, 4.0),
            (100.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let (actions, scroll) = crate::app::wheel_scroll_test_input(amount);
            input.input_frame(
                &mut gallery,
                &PointerFrame {
                    number: number as u32,
                    time_seconds: number as f64 / 60.0,
                    events: &[],
                },
                &actions,
                scroll,
            );
            assert_eq!(gallery.list().scroll(), expected);
            assert!(gallery.take_sounds().is_empty());
            assert!(gallery.take_music_requests().is_empty());
        }
    }
    fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(x, y),
        }
    }
    fn frame(
        input: &mut MusicGalleryInput,
        m: &mut MusicGallery,
        n: u32,
        time: f64,
        events: &[PointerEvent],
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        input.input_frame(
            m,
            &PointerFrame {
                number: n,
                time_seconds: time,
                events,
            },
            state,
            axis,
        );
    }
    fn clicked() -> ActionState {
        ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        }
    }
    #[test]
    fn physical_half_size_tap_plays_original_row_and_blocks_simultaneous_cancel() {
        let mut m = gallery();
        let mut input = MusicGalleryInput::new();
        input.resize(&mut m, (960, 540));
        let mut state = EnumMap::default();
        state[AppAction::Cancel] = clicked();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 500, 100)],
            &state,
            0.0,
        );
        assert_eq!(m.phase(), crate::title::music_gallery::MusicPhase::Active);
        m.update(Ticks::ZERO, false);
        frame(
            &mut input,
            &mut m,
            1,
            0.05,
            &[event(PointerKind::Up, 0, 0)],
            &state,
            0.0,
        );
        assert_eq!(m.list().selected(), 1);
        assert_eq!(m.list().playing(), Some(1));
        assert!(m.take_sounds().is_empty());
        assert_eq!(m.take_music_requests().len(), 1);
    }
    #[test]
    fn background_capture_consumes_controls_and_rejected_side_allows_them() {
        let mut m = gallery();
        let mut input = MusicGalleryInput::new();
        let mut state = EnumMap::default();
        state[AppAction::Enter] = clicked();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 10, 10)],
            &state,
            0.0,
        );
        assert!(m.take_music_requests().is_empty());
        m.update(Ticks::ZERO, false);
        frame(
            &mut input,
            &mut m,
            1,
            0.05,
            &[event(PointerKind::Up, 10, 10)],
            &state,
            0.0,
        );
        assert!(m.take_music_requests().is_empty());
        m.update(Ticks::ZERO, false);
        frame(
            &mut input,
            &mut m,
            2,
            0.1,
            &[event(PointerKind::Down, 10, 300)],
            &state,
            0.0,
        );
        assert_eq!(m.list().playing(), Some(0));
        assert_eq!(m.take_music_requests().len(), 1);
    }
    #[test]
    fn music_physical_h_j_page_and_k_l_do_not() {
        use shin_input::{Action, RawInputState};
        use winit::keyboard::KeyCode;
        let mut m = gallery();
        let mut input = MusicGalleryInput::new();
        for (number, (key, selected)) in [
            (KeyCode::KeyL, 0),
            (KeyCode::KeyJ, 16),
            (KeyCode::KeyK, 16),
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
            frame(
                &mut input,
                &mut m,
                number as u32,
                number as f64,
                &[],
                &state,
                0.0,
            );
            assert_eq!(m.list().selected(), selected, "{key:?}");
            m.update(Ticks::from_f32(100.0), false);
        }
    }

    #[test]
    fn controller_priority_and_page_navigation_obey_scroll_gate() {
        let mut m = gallery();
        let mut input = MusicGalleryInput::new();
        let mut state = EnumMap::default();
        state[AppAction::Right] = clicked();
        state[AppAction::CharacterPageDown] = clicked();
        frame(&mut input, &mut m, 0, 0.0, &[], &state, 1.0);
        assert_eq!(m.list().selected(), 17);
        assert_eq!(m.list().scroll(), 0.0);
        m.update(Ticks::from_f32(2.0), false);
        state[AppAction::Cancel] = clicked();
        frame(&mut input, &mut m, 1, 0.01, &[], &state, 0.0);
        assert_eq!(m.phase(), crate::title::music_gallery::MusicPhase::Active);
        m.update(Ticks::from_f32(2.0), false);
        state[AppAction::Cancel] = Default::default();
        state[AppAction::Left] = clicked();
        state[AppAction::Up] = clicked();
        state[AppAction::AnyDown] = clicked();
        state[AppAction::CharacterPageUp] = clicked();
        frame(&mut input, &mut m, 2, 0.02, &[], &state, 1.0);
        assert_eq!(m.list().selected(), 14); // Left+Up dominate alternatives.
        m.update(Ticks::from_f32(4.0), false);
        state[AppAction::Cancel] = clicked();
        state[AppAction::Enter] = clicked();
        state[AppAction::ConfigReset] = clicked();
        frame(&mut input, &mut m, 3, 0.03, &[], &state, 0.0);
        assert_eq!(m.phase(), crate::title::music_gallery::MusicPhase::Closing);
        assert!(m.take_music_requests().is_empty());
        assert_eq!(m.take_sounds(), ["move", "move", "cancel"]);
    }
    #[test]
    fn physical_drag_converts_delta_and_gesture_before_capture_reset() {
        let mut m = gallery();
        let mut input = MusicGalleryInput::new();
        input.resize(&mut m, (960, 540));
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut m,
            0,
            0.0,
            &[event(PointerKind::Down, 480, 200)],
            &state,
            0.0,
        );
        m.update(Ticks::ZERO, false);
        frame(
            &mut input,
            &mut m,
            1,
            0.2,
            &[event(PointerKind::Move, 480, 160)],
            &state,
            0.0,
        );
        assert_eq!(m.list().scroll(), 80.0);
        m.update(Ticks::ZERO, false);
        frame(
            &mut input,
            &mut m,
            2,
            0.21,
            &[event(PointerKind::Up, 0, 0)],
            &state,
            0.0,
        );
        assert!(m.velocity() < -130.0 && m.velocity() > -140.0);
        assert!(m.take_music_requests().is_empty());
        m.update(Ticks::from_f32(1.0), false);
        assert!(m.list().scroll() > 210.0 && m.list().scroll() < 220.0);
        // Resizing disables capture but does not erase a previously released fling.
        input.resize(&mut m, (1920, 1080));
        assert!(m.velocity() < 0.0);
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_music_pointer_scroll_and_inertia() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x31bf8, 0x540006ab),
        (0x31c08, 0x54000625),
        (0x31c1c, 0x5400058b),
        (0x31c44, 0x110c8108),
        (0x31c78, 0x5400028d),
        (0x31ca0, 0x1e38000a),
        (0x31cc0, 0x7101615f),
        (0x31cc4, 0x540000cd),
        (0x31d30, 0x11022109),
        (0x31d34, 0x110ef108),
        (0x31df8, 0xb9016a60),
        (0x31e0c, 0xf900b67f),
        (0x31f18, 0xf9000109),
        (0x32280, 0xb9016a7f),
        (0x32290, 0x1e293901),
        (0x3229c, 0x1e213800),
        (0x322bc, 0x97fff97d),
        (0x322c8, 0x94022406),
        (0x32300, 0xbd016c01),
        (0x30748, 0x1e213801),
        (0x30764, 0x54000264),
        (0x30790, 0x9403b12c),
        (0x307ac, 0x54000045),
        (0x31774, 0x1e220821),
        (0x317a0, 0x940226d0),
        (0x308ec, 0x321f03eb),
        (0x3091c, 0x321f7beb),
        (0x30950, 0x540002aa),
    ] {
        assert_eq!(word(a), expected, "native MusicMode input{a:x}");
    }
    for (a, value) in [
        (0x159a20, 150.0f32),
        (0x159944, 992.0),
        (0x159694, 32.0),
        (0x159ab8, f32::EPSILON),
        (0x1595d8, 0.95),
        (0x159570, 0.1),
    ] {
        assert_eq!(
            word(a),
            value.to_bits(),
            "native MusicMode input constant{a:x}"
        );
    }
}
