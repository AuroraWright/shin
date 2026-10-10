//! Native Title episode task10e7e0. Clock polling precedes advancement.
//! The retained owner consumes requests; constructing viewers belongs to it.
use super::{TitleItem, view::RowFeedback};
use crate::{
    adv::input_area::{InputAreaTarget, ModalInputTarget},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::{Easing, Ticks, Tween, Tweener};
use shin_input::ActionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChooserPhase {
    Opening,
    Active,
    Closing,
    Closed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChooserRequest {
    Viewer { item: TitleItem, episode: u8 },
    RestoreTitleRows,
}
enum Task {
    //112d30 waits on the originally clicked flash but dispatches current selection.
    Confirm(usize),
    //112e50 gates opacity/Y, ignoring X, flash and focus.
    Close,
}
struct Row {
    episode: u8,
    x: Tweener,
    y: f32,
    opacity: Tweener,
    focus: Tweener,
    flash: Tweener,
}
pub(super) struct EpisodeChooser {
    item: TitleItem,
    rows: Vec<Row>,
    selected: usize,
    pointer_row: Option<usize>,
    phase: ChooserPhase,
    tasks: Vec<Task>,
    requests: Vec<ChooserRequest>,
    sounds: Vec<&'static str>,
}
fn queue(clock: &mut Tweener, value: f32, ticks: f32, easing: Easing) {
    clock.enqueue(
        value,
        Tween {
            duration: Ticks::from_f32(ticks),
            easing,
        },
    );
}
fn wait(clock: &mut Tweener, ticks: f32) {
    queue(clock, clock.target_value(), ticks, Easing::Linear);
}
fn rate(clock: &mut Tweener, value: f32, speed: f32) {
    clock.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - clock.value()).abs() / speed)),
    );
}
impl EpisodeChooser {
    pub fn new(item: TitleItem, completed: u16) -> anyhow::Result<Self> {
        let count = match item {
            TitleItem::Characters => 8,
            TitleItem::Tips => 7,
            _ => anyhow::bail!("Title item has no episode chooser"),
        };
        let rows: Vec<_> = (0..count)
            .filter(|episode| completed & (1 << (episode + 8)) != 0)
            .enumerate()
            .map(|(index, episode)| {
                let delay = index as f32 * 4.0;
                let mut opacity = Tweener::new(0.0);
                wait(&mut opacity, delay);
                queue(&mut opacity, 1.0, 10.0, Easing::Linear);
                let mut x = Tweener::new(1588.0);
                wait(&mut x, delay);
                queue(&mut x, 1488.0, 10.0, Easing::SineOut);
                Row {
                    episode,
                    x,
                    y: 320.0 + index as f32 * 72.0,
                    opacity,
                    focus: Tweener::new(if index == 0 { 1.0 } else { 0.0 }),
                    flash: Tweener::new(0.0),
                }
            })
            .collect();
        // Disabled Title entries never create this task. Reject invalid direct calls.
        anyhow::ensure!(
            !rows.is_empty(),
            "Title episode chooser has no unlocked episodes"
        );
        Ok(Self {
            item,
            rows,
            selected: 0,
            pointer_row: None,
            phase: ChooserPhase::Opening,
            tasks: Vec::new(),
            requests: Vec::new(),
            sounds: Vec::new(),
        })
    }
    pub fn phase(&self) -> ChooserPhase {
        self.phase
    }
    pub fn selected_episode(&self) -> u8 {
        self.rows[self.selected].episode
    }
    pub fn rows(&self) -> impl Iterator<Item = (u8, RowFeedback)> + '_ {
        self.rows.iter().map(|row| {
            (
                row.episode,
                RowFeedback {
                    x: row.x.value(),
                    y: row.y,
                    opacity: row.opacity.value(),
                    focus: row.focus.value(),
                    flash: row.flash.value(),
                },
            )
        })
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == ChooserPhase::Active
    }
    /// Native100300 snapshots tasks and invokes newest first. A newly installed
    /// opening input task is not eligible for input until the next poll.
    pub fn prepare_frame(&mut self) -> bool {
        let input_ready = self.accepts_input();
        let rows_idle = self.rows.iter().all(|row| row.opacity.is_idle());
        if self.phase == ChooserPhase::Opening && rows_idle {
            self.phase = ChooserPhase::Active;
        }
        let pending = std::mem::take(&mut self.tasks);
        for task in pending.into_iter().rev() {
            match task {
                Task::Confirm(row) if self.rows[row].flash.is_idle() => {
                    self.requests.push(ChooserRequest::Viewer {
                        item: self.item,
                        episode: self.selected_episode(),
                    });
                }
                Task::Close if rows_idle => {
                    self.requests.push(ChooserRequest::RestoreTitleRows);
                    self.phase = ChooserPhase::Closed;
                }
                task => self.tasks.push(task),
            }
        }
        self.tasks.reverse();
        input_ready && self.accepts_input()
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        for row in &mut self.rows {
            for clock in [&mut row.x, &mut row.opacity, &mut row.focus, &mut row.flash] {
                clock.update(delta);
            }
        }
    }
    fn select(&mut self, index: usize) {
        //10fe10 uses different outgoing/incoming rates (.05/.1).
        rate(&mut self.rows[self.selected].focus, 0.0, 0.05);
        self.selected = index;
        rate(&mut self.rows[index].focus, 1.0, 0.1);
    }
    pub fn navigate(&mut self, down: bool, up: bool) {
        if !self.accepts_input() {
            return;
        }
        let count = self.rows.len();
        let next = if down {
            (self.selected + 1) % count
        } else if up {
            (self.selected + count - 1) % count
        } else {
            self.selected
        };
        if next != self.selected {
            self.sounds.push("move");
            self.select(next);
        }
    }
    pub fn confirm(&mut self) {
        if !self.accepts_input() {
            return;
        }
        self.sounds.push("decide");
        let flash = &mut self.rows[self.selected].flash;
        rate(flash, 0.7, 0.2);
        // Second flag40 segment runs at speed.2 from queued target.7.
        queue(flash, 0.0, 3.5, Easing::Linear);
        self.tasks.push(Task::Confirm(self.selected));
    }
    pub fn close(&mut self) {
        if !self.accepts_input() {
            return;
        }
        self.sounds.push("cancel");
        for (index, row) in self.rows.iter_mut().enumerate() {
            let delay = index as f32 * 4.0;
            wait(&mut row.opacity, delay);
            queue(&mut row.opacity, 0.0, 10.0, Easing::Linear);
            wait(&mut row.x, delay);
            queue(&mut row.x, 1588.0, 10.0, Easing::SineIn);
        }
        self.phase = ChooserPhase::Closing;
        self.tasks.push(Task::Close);
    }
    pub fn take_requests(&mut self) -> Vec<ChooserRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    fn row_at(&self, point: Vec2) -> Option<usize> {
        //110170 intentionally retains the Title row704x88 hit box, despite
        // chooser artwork400x72. Earlier packed rows own the16px overlap.
        self.rows.iter().position(|row| {
            point.x >= 1488.0 && point.x < 2192.0 && point.y >= row.y && point.y < row.y + 88.0
        })
    }
    fn leave(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            rate(&mut self.rows[row].flash, 0.0, 0.2);
        }
    }
}
impl InputAreaTarget for EpisodeChooser {
    fn probe(&mut self, point: Vec2) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.pointer_row = self.row_at(point);
        true
    }
    fn reset(&mut self) {
        self.pointer_row = None;
    }
    fn press(&mut self, _: Vec2) {
        if let Some(row) = self.pointer_row {
            rate(&mut self.rows[row].flash, 0.7, 0.2);
            self.select(row);
        }
    }
    fn release(&mut self, _: Vec2) {
        if let Some(row) = self.pointer_row {
            self.select(row);
            self.confirm();
        } else {
            //1103e4 releases blank chooser background as Cancel.
            self.close();
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.leave();
    }
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {
        self.leave();
    }
}
impl ModalInputTarget for EpisodeChooser {
    fn accepts_pointer_input(&self) -> bool {
        self.accepts_input()
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if state[AppAction::Enter].is_clicked {
            self.confirm();
        } else if state[AppAction::Cancel].is_clicked {
            self.close();
        } else {
            self.navigate(
                state[AppAction::AnyDown].is_clicked_or_repeated,
                state[AppAction::Up].is_clicked_or_repeated,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ticks(n: f32) -> Ticks {
        Ticks::from_f32(n)
    }
    fn ready() -> EpisodeChooser {
        let mut chooser = EpisodeChooser::new(TitleItem::Characters, 0x8500).unwrap();
        chooser.advance_clocks(ticks(18.0));
        assert!(!chooser.prepare_frame());
        assert!(chooser.prepare_frame());
        chooser
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.00001, "{a} vs {b}");
    }
    #[test]
    fn completion_bits_pack_rows_and_exclude_episode_eight_only_for_tips() {
        for low in [0, 0xff] {
            for high in 0..=255u16 {
                let bits = high << 8 | low;
                for (item, limit) in [(TitleItem::Characters, 8), (TitleItem::Tips, 7)] {
                    let expected: Vec<_> = (0..limit).filter(|id| high & (1 << id) != 0).collect();
                    let chooser = EpisodeChooser::new(item, bits);
                    if expected.is_empty() {
                        assert!(chooser.is_err());
                        continue;
                    }
                    let chooser = chooser.unwrap();
                    assert_eq!(
                        chooser.rows().map(|(id, _)| id).collect::<Vec<_>>(),
                        expected
                    );
                    assert_eq!(chooser.selected_episode(), expected[0]);
                    for (index, (_, row)) in chooser.rows().enumerate() {
                        assert_eq!(row.y, 320.0 + index as f32 * 72.0);
                    }
                }
            }
        }
        assert!(EpisodeChooser::new(TitleItem::Load, 0xffff).is_err());
    }
    #[test]
    fn staggered_opening_polls_before_clocks_and_ignores_early_input() {
        let mut chooser = EpisodeChooser::new(TitleItem::Characters, 0x8500).unwrap();
        chooser.navigate(true, false);
        chooser.confirm();
        chooser.close();
        assert!(chooser.take_sounds().is_empty());
        assert_eq!(chooser.phase(), ChooserPhase::Opening);
        chooser.advance_clocks(ticks(5.0));
        let rows: Vec<_> = chooser.rows().collect();
        close(rows[0].1.opacity, 0.5);
        close(rows[1].1.opacity, 0.1);
        close(rows[2].1.opacity, 0.0);
        close(
            rows[0].1.x,
            1588.0 - 100.0 * std::f32::consts::FRAC_1_SQRT_2,
        );
        chooser.advance_clocks(ticks(13.0));
        assert_eq!(chooser.phase(), ChooserPhase::Opening);
        assert!(!chooser.prepare_frame());
        assert_eq!(chooser.phase(), ChooserPhase::Active);
        assert!(chooser.prepare_frame());
    }
    #[test]
    fn packed_navigation_wraps_prioritizes_down_and_uses_asymmetric_focus_rates() {
        let mut chooser = ready();
        chooser.navigate(true, true);
        assert_eq!(chooser.selected_episode(), 2);
        chooser.advance_clocks(ticks(5.0));
        let rows: Vec<_> = chooser.rows().collect();
        close(rows[0].1.focus, 0.75);
        close(rows[1].1.focus, 0.5);
        chooser.navigate(true, false);
        assert_eq!(chooser.selected_episode(), 7);
        chooser.navigate(true, false);
        assert_eq!(chooser.selected_episode(), 0);
        chooser.navigate(false, true);
        assert_eq!(chooser.selected_episode(), 7);
        assert_eq!(chooser.take_sounds(), ["move", "move", "move", "move"]);
    }
    #[test]
    fn retained_confirmation_waits_original_flash_then_reads_current_episode() {
        let mut chooser = ready();
        chooser.confirm();
        chooser.advance_clocks(ticks(3.5));
        close(chooser.rows().next().unwrap().1.flash, 0.7);
        chooser.navigate(true, false);
        chooser.prepare_frame();
        assert!(chooser.take_requests().is_empty());
        chooser.advance_clocks(ticks(3.5));
        assert!(chooser.take_requests().is_empty());
        chooser.prepare_frame();
        assert_eq!(
            chooser.take_requests(),
            [ChooserRequest::Viewer {
                item: TitleItem::Characters,
                episode: 2,
            }]
        );
        assert_eq!(chooser.phase(), ChooserPhase::Active);
    }
    #[test]
    fn cancel_waits_all_packed_rows_then_requests_parent_reopening_once() {
        let mut chooser = ready();
        chooser.close();
        assert_eq!(chooser.phase(), ChooserPhase::Closing);
        chooser.navigate(true, false);
        chooser.confirm();
        chooser.advance_clocks(ticks(17.99));
        chooser.prepare_frame();
        assert!(chooser.take_requests().is_empty());
        chooser.advance_clocks(ticks(0.01));
        assert_eq!(chooser.phase(), ChooserPhase::Closing);
        chooser.prepare_frame();
        assert_eq!(chooser.phase(), ChooserPhase::Closed);
        assert_eq!(chooser.take_requests(), [ChooserRequest::RestoreTitleRows]);
        chooser.prepare_frame();
        assert!(chooser.take_requests().is_empty());
        assert_eq!(chooser.take_sounds(), ["cancel"]);
    }
    #[test]
    fn pointer_uses_original_title_bounds_overlap_and_blank_release_cancel() {
        let mut chooser = ready();
        for (point, expected) in [
            (Vec2::new(1488.0, 320.0), Some(0)),
            (Vec2::new(2191.99, 407.99), Some(0)),
            (Vec2::new(1600.0, 392.0), Some(0)),
            (Vec2::new(1600.0, 408.0), Some(1)),
            (Vec2::new(2192.0, 320.0), None),
            (Vec2::new(1487.99, 320.0), None),
        ] {
            assert_eq!(chooser.row_at(point), expected);
        }
        assert!(chooser.probe(Vec2::new(1600.0, 408.0)));
        chooser.press(Vec2::ZERO);
        assert_eq!(chooser.selected_episode(), 2);
        chooser.advance_clocks(ticks(3.5));
        chooser.drag(Vec2::ZERO, Vec2::ZERO);
        chooser.advance_clocks(ticks(3.5));
        close(chooser.rows().nth(1).unwrap().1.flash, 0.0);
        assert!(chooser.take_requests().is_empty());
        chooser.probe(Vec2::new(100.0, 100.0));
        chooser.release(Vec2::ZERO);
        assert_eq!(chooser.phase(), ChooserPhase::Closing);
        assert_eq!(chooser.take_sounds(), ["cancel"]);
    }
    #[test]
    fn physical_pointer_capture_suppresses_controls_and_blank_tap_closes() {
        use crate::adv::input_area::{InputArea, PointerEvent, PointerFrame, PointerKind};
        let mut chooser = ready();
        let mut area = InputArea::new(960, 540);
        let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
        state[AppAction::AnyDown].is_clicked_or_repeated = true;
        let down = PointerEvent {
            kind: PointerKind::Down,
            id: 0,
            position: glam::ivec2(800, 210),
        };
        area.input_frame(
            &mut chooser,
            &PointerFrame {
                number: 1,
                time_seconds: 0.0,
                events: &[down],
            },
            &state,
        );
        // Capture blocks simultaneous Down even before the delayed press.
        assert_eq!(chooser.selected_episode(), 0);
        area.input_frame(
            &mut chooser,
            &PointerFrame {
                number: 2,
                time_seconds: 0.2,
                events: &[],
            },
            &state,
        );
        assert_eq!(chooser.selected_episode(), 2);
        assert!(chooser.take_sounds().is_empty());
        area.input_frame(
            &mut chooser,
            &PointerFrame {
                number: 3,
                time_seconds: 0.21,
                events: &[PointerEvent {
                    kind: PointerKind::Up,
                    id: 0,
                    position: glam::ivec2(800, 210),
                }],
            },
            &state,
        );
        assert_eq!(chooser.take_sounds(), ["decide"]);
        chooser.advance_clocks(ticks(7.0));
        chooser.prepare_frame();
        assert_eq!(
            chooser.take_requests(),
            [ChooserRequest::Viewer {
                item: TitleItem::Characters,
                episode: 2,
            }]
        );
        state = EnumMap::default();
        area.input_frame(
            &mut chooser,
            &PointerFrame {
                number: 4,
                time_seconds: 0.4,
                events: &[PointerEvent {
                    kind: PointerKind::Down,
                    id: 0,
                    position: glam::ivec2(50, 50),
                }],
            },
            &state,
        );
        area.input_frame(
            &mut chooser,
            &PointerFrame {
                number: 5,
                time_seconds: 0.41,
                events: &[PointerEvent {
                    kind: PointerKind::Up,
                    id: 0,
                    position: glam::ivec2(50, 50),
                }],
            },
            &state,
        );
        assert_eq!(chooser.phase(), ChooserPhase::Closing);
        assert_eq!(chooser.take_sounds(), ["cancel"]);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_episode_chooser_arguments_clocks_and_pointer() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x10eac8, 0x36781248u32),
            (0x10ead0, 0x32000be1),
            (0x10ecd4, 0x321f07e1),
            (0x10ed40, 0x5280281b),
            (0x10ed60, 0x52a89748),
            (0x10f1fc, 0x910162c0),
            (0x10f220, 0x321f03e1),
            (0x1101a8, 0xbd46350a),
            (0x1101b0, 0xbd49410b),
            (0x1103e8, 0x320003e1),
            (0x112d40, 0xb9801008),
            (0x112d50, 0x9105e100),
            (0x112d70, 0x97fff45c),
            (0x112edc, 0x320003e1),
            (0x10ffec, 0x2a1403e1),
            (0x10fff0, 0x2a1403e2),
            (0x10fff4, 0x2a1f03e3),
            (0x109d80, 0xaa1f03e3),
            (0x109d84, 0x97fff32b),
            (0x4ff20, 0x940013d8),
            (0x4ff30, 0x9402c050),
            (0x54ecc, 0x97ff4905),
            (0x550d0, 0x97ff4da4),
            (0x550dc, 0xf9400660),
            (0x550e0, 0x97fff9b8),
            (0x562d4, 0x91031d08),
            (0x5630c, 0x97ffa9c9),
            (0x5632c, 0x94030395),
            (0x10f0bc, 0x97feb251),
            (0x10f300, 0x2a1f03e2),
            (0x10f318, 0x97feb1ba),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x1598b0, 1488.0f32),
            (0x159a60, 100.0),
            (0x159724, 0.05),
            (0x159570, 0.1),
            (0x159634, 704.0),
            (0x159940, 88.0),
            (0x1596c8, 400.0),
            (0x159ab4, 72.0),
            (0x159444, 320.0),
            (0x15944c, 1.0 / 512.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        assert_eq!(&exe[0x162112 + 256..0x162118 + 256], b"menu2\0");
        assert_eq!(&exe[0x15c0c7 + 256..0x15c0d2 + 256], b"/chars.txa\0");
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut reader =
            RomReader::new(StatelessFile::new(std::fs::File::open(rom).unwrap()).unwrap()).unwrap();
        let file = reader.find_file("/title.txa").unwrap();
        let mut bytes = Vec::new();
        reader
            .open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let labels = atlas.get_texture("menu2").unwrap();
        assert_eq!(labels.dimensions(), (320, 512));
        for episode in 0..8 {
            assert!(
                (episode * 64..(episode + 1) * 64)
                    .any(|y| (0..320).any(|x| labels.get_pixel(x, y)[3] != 0)),
                "Episode{} chooser label is empty",
                episode + 1
            );
        }
    }
}
