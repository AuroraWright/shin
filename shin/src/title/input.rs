//! Native10d8e0 routes pointer capture before title controller actions.
use super::motion::TitleMotion;
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, ModalInputTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
#[cfg(test)]
use shin_core::time::Ticks;
use shin_input::ActionState;

impl InputAreaTarget for TitleMotion {
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
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.pointer_leave();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {
        self.pointer_leave();
    }
}
impl ModalInputTarget for TitleMotion {
    fn accepts_pointer_input(&self) -> bool {
        self.accepts_input()
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if !self.accepts_input() {
            return;
        }
        // Native confirm precedes Down/Up; all other controller bits ignored.
        if state[AppAction::Enter].is_clicked {
            self.confirm();
        } else {
            self.navigate(
                state[AppAction::AnyDown].is_clicked_or_repeated,
                state[AppAction::Up].is_clicked_or_repeated,
            );
        }
    }
}
pub(super) struct TitleInput {
    area: InputArea,
}
impl TitleInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, model: &mut TitleMotion, extent: (u16, u16)) {
        self.area.disable(model);
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn suspend(&mut self, model: &mut TitleMotion) {
        self.area.disable(model);
    }
    pub fn input_frame(
        &mut self,
        model: &mut TitleMotion,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        self.area.input_frame(model, frame, state);
    }
    /// Native10c108 dispatches tasks/input before updating all feedback clocks.
    #[cfg(test)]
    pub fn update_frame(
        &mut self,
        model: &mut TitleMotion,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        model.prepare_frame();
        self.input_frame(model, frame, state);
        model.advance_clocks(delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adv::input_area::{PointerEvent, PointerKind},
        title::{TitleItem, TitleSetup, motion::Phase},
    };
    use shin_core::format::save::SaveData;
    fn active(item: TitleItem) -> TitleMotion {
        let mut model = TitleMotion::new(TitleSetup::from_save(&SaveData::default(), item as i32));
        model.assets_ready();
        model.advance_clocks(Ticks::from_f32(60.0));
        for _ in 0..3 {
            model.prepare_frame();
        }
        assert_eq!(model.phase(), Phase::Active);
        model
    }
    fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(x, y),
        }
    }
    fn frame(
        input: &mut TitleInput,
        model: &mut TitleMotion,
        n: u32,
        time: f64,
        events: &[PointerEvent],
        state: &EnumMap<AppAction, ActionState>,
    ) {
        input.input_frame(
            model,
            &PointerFrame {
                number: n,
                time_seconds: time,
                events,
            },
            state,
        );
    }
    #[test]
    fn static_half_open_hit_boxes_resolve_overlap_in_packed_order() {
        let model = active(TitleItem::Config);
        for (point, expected) in [
            (Vec2::new(1184.0, 450.0), Some(0)),
            (Vec2::new(1887.99, 537.99), Some(0)),
            (Vec2::new(1184.0, 532.0), Some(0)),
            (Vec2::new(1184.0, 538.0), Some(1)),
            (Vec2::new(1888.0, 450.0), None),
            (Vec2::new(1183.99, 450.0), None),
            (Vec2::new(1184.0, 449.99), None),
            (Vec2::new(1184.0, 783.99), Some(3)),
            (Vec2::new(1184.0, 784.0), None),
        ] {
            assert_eq!(model.row_at(point), expected);
        }
        let mut opening = model;
        opening.open_rows(false);
        assert_eq!(opening.frame().rows[0].x, 1284.0);
        assert_eq!(opening.row_at(Vec2::new(1184.0, 450.0)), Some(0));
        assert!(!opening.pointer_probe(Vec2::new(1184.0, 450.0)));
    }
    #[test]
    fn physical_scaled_delayed_press_selects_and_release_confirms_once() {
        let mut model = active(TitleItem::Config);
        let mut input = TitleInput::new();
        input.resize(&mut model, (960, 540));
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut model,
            1,
            0.0,
            &[event(PointerKind::Down, 600, 280)],
            &state,
        );
        assert_eq!(model.selected(), 2);
        assert!(model.take_sounds().is_empty());
        frame(&mut input, &mut model, 2, 0.16, &[], &state);
        assert_eq!(model.selected(), 1);
        assert!(model.take_sounds().is_empty());
        model.advance_clocks(Ticks::from_f32(2.0));
        assert!((model.frame().rows[1].flash - 0.4).abs() < 0.00001);
        let up = [event(PointerKind::Up, 900, 520)];
        frame(&mut input, &mut model, 3, 0.2, &up, &state);
        // Release coordinates do not replace the captured row.
        assert_eq!(model.selected(), 1);
        assert_eq!(model.take_sounds(), ["decide"]);
        frame(&mut input, &mut model, 3, 0.3, &up, &state);
        assert!(model.take_sounds().is_empty());
        model.advance_clocks(Ticks::from_f32(5.0));
        model.prepare_frame();
        assert_eq!(model.take_child_requests(), vec![TitleItem::Load]);
        assert!(model.take_destination().is_none());
    }
    #[test]
    fn blank_capture_consumes_controller_until_release_and_enter_precedes_navigation() {
        let mut model = active(TitleItem::Config);
        let mut input = TitleInput::new();
        let mut state = EnumMap::<AppAction, ActionState>::default();
        state[AppAction::AnyDown].is_clicked_or_repeated = true;
        frame(
            &mut input,
            &mut model,
            1,
            0.0,
            &[event(PointerKind::Down, 50, 50)],
            &state,
        );
        frame(&mut input, &mut model, 2, 0.16, &[], &state);
        assert_eq!(model.selected(), 2);
        frame(
            &mut input,
            &mut model,
            3,
            0.2,
            &[event(PointerKind::Up, 50, 50)],
            &state,
        );
        assert!(model.take_sounds().is_empty());
        frame(&mut input, &mut model, 4, 0.3, &[], &state);
        assert_eq!(model.selected(), 3);
        assert_eq!(model.take_sounds(), ["move"]);
        state[AppAction::Enter].is_clicked = true;
        frame(&mut input, &mut model, 5, 0.4, &[], &state);
        assert_eq!(model.selected(), 3);
        assert_eq!(model.take_sounds(), ["decide"]);
    }
    #[test]
    fn drag_and_resize_cancel_pressed_flash_without_confirming() {
        for resize in [false, true] {
            let mut model = active(TitleItem::Config);
            let mut input = TitleInput::new();
            let state = EnumMap::default();
            frame(
                &mut input,
                &mut model,
                1,
                0.0,
                &[event(PointerKind::Down, 1200, 560)],
                &state,
            );
            frame(&mut input, &mut model, 2, 0.16, &[], &state);
            model.advance_clocks(Ticks::from_f32(2.0));
            assert!((model.frame().rows[1].flash - 0.4).abs() < 0.00001);
            if resize {
                input.resize(&mut model, (960, 540));
            } else {
                frame(
                    &mut input,
                    &mut model,
                    3,
                    0.2,
                    &[event(PointerKind::Move, 1240, 560)],
                    &state,
                );
            }
            model.advance_clocks(Ticks::from_f32(2.0));
            assert_eq!(model.frame().rows[1].flash, 0.0);
            frame(
                &mut input,
                &mut model,
                4,
                0.3,
                &[event(PointerKind::Up, 1240, 560)],
                &state,
            );
            model.prepare_frame();
            assert!(model.take_destination().is_none());
            assert!(model.take_sounds().is_empty());
            assert_eq!(model.selected(), 1);
        }
    }
    #[test]
    fn repeated_taps_play_one_cue_and_dispatch_once_then_allow_a_new_visit() {
        for (item, y, cue, duration) in [
            (TitleItem::Start, 470, "bell", 37.0),
            (TitleItem::Load, 560, "decide", 7.0),
            (TitleItem::Config, 640, "decide", 7.0),
            (TitleItem::Manual, 720, "decide", 7.0),
        ] {
            let mut model = active(item);
            let mut input = TitleInput::new();
            let state = EnumMap::default();
            for n in 1..=20 {
                frame(
                    &mut input,
                    &mut model,
                    n,
                    n as f64 * 0.01,
                    &[
                        event(PointerKind::Down, 1200, y),
                        event(PointerKind::Up, 1200, y),
                    ],
                    &state,
                );
                if n == 1 {
                    assert_eq!(model.take_sounds(), [cue]);
                } else {
                    assert!(model.take_sounds().is_empty(), "{item:?} tap{n}");
                }
                model.advance_clocks(Ticks::from_f32(0.25));
                model.prepare_frame();
                assert!(model.take_child_requests().is_empty());
                assert!(model.take_destination().is_none());
            }
            model.advance_clocks(Ticks::from_f32(duration - 5.0));
            model.prepare_frame();
            if item == TitleItem::Start {
                assert_eq!(model.take_destination(), Some(item));
                assert!(model.take_destination().is_none());
            } else {
                assert_eq!(model.take_child_requests(), [item]);
                assert!(model.take_child_requests().is_empty());
                model.child_finished(true);
                frame(
                    &mut input,
                    &mut model,
                    21,
                    0.3,
                    &[
                        event(PointerKind::Down, 1200, y),
                        event(PointerKind::Up, 1200, y),
                    ],
                    &state,
                );
                assert_eq!(model.take_sounds(), [cue], "new visit {item:?}");
                model.advance_clocks(Ticks::from_f32(duration));
                model.prepare_frame();
                assert_eq!(model.take_child_requests(), [item]);
            }
        }
    }
    #[test]
    fn other_controller_bits_are_ignored_and_start_uses_bell() {
        let mut model = active(TitleItem::Start);
        let mut input = TitleInput::new();
        let mut state = EnumMap::<AppAction, ActionState>::default();
        for action in [AppAction::Cancel, AppAction::Left, AppAction::Right] {
            state[action].is_clicked = true;
            state[action].is_clicked_or_repeated = true;
        }
        frame(&mut input, &mut model, 1, 0.0, &[], &state);
        assert_eq!(model.selected(), 0);
        assert!(model.take_sounds().is_empty());
        state[AppAction::Enter].is_clicked = true;
        input.update_frame(
            &mut model,
            Ticks::from_f32(37.0),
            &PointerFrame {
                number: 2,
                time_seconds: 0.1,
                events: &[],
            },
            &state,
        );
        assert_eq!(model.take_sounds(), ["bell"]);
        assert!(model.take_destination().is_none());
        input.update_frame(
            &mut model,
            Ticks::ZERO,
            &PointerFrame {
                number: 3,
                time_seconds: 0.2,
                events: &[],
            },
            &state,
        );
        assert_eq!(model.take_destination(), Some(TitleItem::Start));
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    #[ignore = "requires supplied ROM/native executable; decodes title UI sound cues"]
    fn reference_native_title_pointer_controller_geometry_and_sound_cues() {
        use shin_core::{
            format::{audio::AudioSource, rom::RomReader},
            primitives::stateless_reader::StatelessFile,
        };
        use std::io::Read;
        let rom_path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom_path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10e2a4, 0x2d418680),
            (0x10e2fc, 0x1e209500),
            (0x10e308, 0x540000e4),
            (0x10e364, 0xb9022a60),
            (0x10e36c, 0x320003e0),
            (0x10e3b4, 0xb9022808),
            (0x10e404, 0x9105e100),
            (0x10e41c, 0x321a03e1),
            (0x10e470, 0xb9022e75),
            (0x10e4a8, 0x1e2a1001),
            (0x10e55c, 0xb9022e75),
            (0x10e5c4, 0x17fffae3),
            (0x10e654, 0xb9022a68),
            (0x10e5e0, 0xd65f03c0),
            (0x10e5f0, 0xd65f03c0),
            (0x10e6e0, 0xd65f03c0),
            (0x10e754, 0xb9022a68),
            (0x10d91c, 0x37200288),
            (0x10d944, 0x37c801aa),
            (0x10d99c, 0x97fc6645),
            (0x10d1a8, 0x97fc6842),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [(0x159634, 704.0f32), (0x159940, 88.0)] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        let relocations: std::collections::HashMap<u64, u64> = (0x11e040usize..0x152eb8usize)
            .step_by(24)
            .map(|address| {
                let read = |offset: usize| {
                    u64::from_le_bytes(
                        exe[address + 256 + offset..address + 264 + offset]
                            .try_into()
                            .unwrap(),
                    )
                };
                (read(0), read(16))
            })
            .collect();
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(rom_path).unwrap()).unwrap())
                .unwrap();
        let file = rom.find_file("/sysse.bin").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let bank = shin_core::format::sysse::read_sys_se(&bytes).unwrap();
        for (got, name) in [(0x226440, "bell"), (0x2246e0, "decide"), (0x224780, "move")] {
            let address = relocations[&relocations[&got]] as usize + 256;
            assert_eq!(&exe[address..address + name.len()], name.as_bytes());
            assert_eq!(exe[address + name.len()], 0);
            let sound = &bank.sounds[name];
            let mut source = AudioSource::new(sound.decode());
            let mut count = 0;
            while let Some((left, right)) = source.read_sample() {
                assert!(left.is_finite() && right.is_finite());
                count += 1;
            }
            assert_eq!(count, sound.sample_count());
            assert!(count > 0);
            println!("Title cue{name}: {count} samples decoded");
        }
    }
}
