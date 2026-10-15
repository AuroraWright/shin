//! Native TipsMode106a30: filtered rows, captured input and text-pane motion.
//! Rendering and async /tips.txa ownership are supplied by TipsScreen.
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    format::scenario::info::TipsInfoItem,
    time::{Ticks, Tween, Tweener},
};
use shin_input::ActionState;

use super::input_area::{InputAreaTarget, ModalInputTarget};
use crate::app::AppAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipsPhase {
    Opening,
    Active,
    CloseFlash,
    Closing,
    Closed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipsPointerTarget {
    Body,
    Outside,
    Close,
}
#[derive(Debug)]
pub struct TipsRow {
    pub scenario_index: usize,
    pub title_index: u16,
    selection: Tweener,
    flash: Tweener,
    /// Measured headline/content extent, including native top/bottom padding.
    height: f32,
}
impl TipsRow {
    pub fn selection(&self) -> f32 {
        self.selection.value()
    }
    pub fn flash(&self) -> f32 {
        self.flash.value()
    }
}
#[derive(Debug)]
pub struct TipsModel {
    rows: Vec<TipsRow>,
    selected: usize,
    phase: TipsPhase,
    progress: f32,
    scroll: f32,
    velocity: f32,
    close_flash: Tweener,
    target: TipsPointerTarget,
    /// Native -1 outside list;0 selected row/gap;1+ packed unselected row.
    hit: i32,
    body_drag: bool,
    axis: f32,
    sounds: Vec<&'static str>,
    responses: Vec<i32>,
}
impl TipsModel {
    /// With an acquired list, episode is ignored and duplicates collapse in
    /// scenario order. Without it, episode=-1 includes every scenario tip.
    pub fn new(items: &[TipsInfoItem], episode: i32, acquired: Option<&[i32]>) -> Option<Self> {
        let rows: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                acquired.map_or(episode == -1 || i32::from(item.episode) == episode, |ids| {
                    i32::try_from(*index).is_ok_and(|index| ids.contains(&index))
                })
            })
            .enumerate()
            .map(|(packed, (scenario_index, item))| TipsRow {
                scenario_index,
                title_index: item.title_index,
                selection: Tweener::new(if packed == 0 { 1.0 } else { 0.0 }),
                flash: Tweener::new(0.0),
                height: 0.0,
            })
            .collect();
        // Native calls selected row0 unconditionally. Reject corrupt/empty data
        // so the screen factory can report the failure instead of indexing it.
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            rows,
            selected: 0,
            phase: TipsPhase::Opening,
            progress: 0.0,
            scroll: 0.0,
            velocity: 0.0,
            close_flash: Tweener::new(0.0),
            target: TipsPointerTarget::Body,
            hit: -1,
            body_drag: false,
            axis: 0.0,
            sounds: Vec::new(),
            responses: Vec::new(),
        })
    }
    pub fn rows(&self) -> &[TipsRow] {
        &self.rows
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn phase(&self) -> TipsPhase {
        self.phase
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn scroll(&self) -> f32 {
        self.scroll
    }
    pub fn content_height(&self) -> f32 {
        self.rows[self.selected].height
    }
    pub fn velocity(&self) -> f32 {
        self.velocity
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn take_responses(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.responses)
    }
    pub fn set_row_height(&mut self, row: usize, height: f32) {
        if height.is_finite() && height >= 0.0 {
            if let Some(row) = self.rows.get_mut(row) {
                row.height = height;
            }
            self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        }
    }
    fn max_scroll(&self) -> f32 {
        (self.rows[self.selected].height - 730.0).max(0.0)
    }
    /// Owner sets the current right-stick Y before native InputArea dispatch;
    /// contacts consume the frame and suppress both axis and button controls.
    pub fn set_axis(&mut self, axis: f32) {
        self.axis = if axis.is_finite() { axis } else { 0.0 };
    }
    pub fn input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if self.phase != TipsPhase::Active {
            return;
        }
        let next = if state[AppAction::Up].is_clicked_or_repeated {
            self.selected.saturating_sub(1)
        } else if state[AppAction::AnyDown].is_clicked_or_repeated {
            (self.selected + 1).min(self.rows.len() - 1)
        } else {
            self.selected
        };
        if next != self.selected {
            self.select(next);
            self.sounds.push("move");
        } else if state[AppAction::Cancel].is_clicked {
            self.request_close("cancel");
        } else if self.axis != 0.0 {
            self.scroll = (self.scroll + self.axis * 32.0).clamp(0.0, self.max_scroll());
            self.velocity = 0.0;
        }
    }
    fn select(&mut self, row: usize) {
        if self.selected == row {
            return;
        }
        retarget(&mut self.rows[self.selected].selection, 0.0, 0.1);
        retarget(&mut self.rows[row].selection, 1.0, 0.25);
        self.selected = row;
        self.scroll = 0.0;
        self.velocity = 0.0;
    }
    pub fn update(&mut self, ticks: f32) {
        if !ticks.is_finite() || ticks < 0.0 {
            return;
        }
        // Native task group precedes inertia and feedback clocks. CloseFlash
        // tests idle before advancing this frame and sends response0 once.
        match self.phase {
            TipsPhase::Opening => {
                self.progress = (self.progress + ticks * 0.1).min(1.0);
                if self.progress == 1.0 {
                    self.phase = TipsPhase::Active;
                }
            }
            TipsPhase::CloseFlash if self.close_flash.is_idle() => {
                self.phase = TipsPhase::Closing;
                self.responses.push(0);
            }
            TipsPhase::Closing => {
                self.progress = (self.progress - ticks * 0.1).max(0.0);
                if self.progress == 0.0 {
                    self.phase = TipsPhase::Closed;
                    self.responses.push(1);
                }
            }
            _ => {}
        }
        if self.velocity != 0.0 {
            let next = (self.scroll - self.velocity * ticks).clamp(0.0, self.max_scroll());
            if (self.scroll - next).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.scroll = next;
                self.velocity *= 0.95_f32.powf(ticks);
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        let delta = Ticks::from_f32(ticks);
        self.close_flash.update(delta);
        for row in &mut self.rows {
            row.selection.update(delta);
            row.flash.update(delta);
        }
    }
    fn request_close(&mut self, cue: &'static str) {
        if self.phase != TipsPhase::Active {
            return;
        }
        flash(&mut self.close_flash);
        self.phase = TipsPhase::CloseFlash;
        self.sounds.push(cue);
    }
    fn list_hit(&self, p: Vec2) -> i32 {
        if !(86.0..552.0).contains(&p.x) || !(188.0..918.0).contains(&p.y) {
            return -1;
        }
        for row in 0..self.rows.len() {
            if row == self.selected {
                continue;
            }
            let top = 193.0 + row as f32 * 88.0;
            if (90.0..548.0).contains(&p.x) && (top..top + 88.0).contains(&p.y) {
                return row as i32 + 1;
            }
        }
        0
    }
}
fn retarget(tween: &mut Tweener, value: f32, speed: f32) {
    let duration = (value - tween.value()).abs() / speed;
    tween.enqueue_now(value, Tween::linear(Ticks::from_f32(duration)));
}
fn flash(tween: &mut Tweener) {
    retarget(tween, 0.7, 0.2);
    tween.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
}
impl InputAreaTarget for TipsModel {
    fn probe(&mut self, p: Vec2) -> bool {
        if self.phase != TipsPhase::Active {
            return false;
        }
        self.hit = -1;
        self.body_drag = false;
        self.target = if (1764.0..1848.0).contains(&p.x) && (32.0..116.0).contains(&p.y) {
            TipsPointerTarget::Close
        } else if (150.0..992.0).contains(&p.y) {
            TipsPointerTarget::Body
        } else {
            TipsPointerTarget::Outside
        };
        if self.target == TipsPointerTarget::Body {
            self.hit = self.list_hit(p);
            if self.hit < 0 && (602.0..1834.0).contains(&p.x) && (188.0..918.0).contains(&p.y) {
                self.body_drag = true;
                self.velocity = 0.0;
            }
        }
        true
    }
    fn reset(&mut self) {
        self.target = TipsPointerTarget::Body;
        self.hit = -1;
        self.body_drag = false;
    }
    fn press(&mut self, _: Vec2) {
        if self.target == TipsPointerTarget::Close {
            retarget(&mut self.close_flash, 0.7, 0.2);
        } else if self.target == TipsPointerTarget::Body && self.hit >= 1 {
            let row = self.hit as usize - 1;
            retarget(&mut self.rows[row].flash, 0.7, 0.2);
            self.select(row);
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.target == TipsPointerTarget::Close {
            self.request_close("decide");
        } else if self.target == TipsPointerTarget::Body && self.hit >= 1 {
            let row = self.hit as usize - 1;
            flash(&mut self.rows[row].flash);
            self.select(row);
            self.sounds.push("decide");
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        if self.target != TipsPointerTarget::Body {
            self.cancel();
        } else if self.hit >= 1 {
            retarget(&mut self.rows[self.hit as usize - 1].flash, 0.0, 0.2);
        } else if self.hit < 0 && self.body_drag {
            self.scroll = (self.scroll - (current.y - previous.y)).clamp(0.0, self.max_scroll());
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.target == TipsPointerTarget::Body
            && self.hit < 0
            && self.body_drag
            && velocity.y.is_finite()
        {
            self.velocity = velocity.y;
        }
    }
    fn cancel(&mut self) {
        if self.target != TipsPointerTarget::Body {
            retarget(&mut self.close_flash, 0.0, 0.2);
            self.target = TipsPointerTarget::Outside;
        } else if self.hit >= 1 {
            retarget(&mut self.rows[self.hit as usize - 1].flash, 0.0, 0.2);
        }
    }
}
impl ModalInputTarget for TipsModel {
    fn accepts_pointer_input(&self) -> bool {
        self.phase == TipsPhase::Active
    }
    fn controller_input(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input(state);
    }
}

#[cfg(test)]
mod tests {
    use super::super::input_area::{InputArea, PointerEvent, PointerFrame, PointerKind};
    use super::*;
    use glam::{IVec2, vec2};
    fn items() -> Vec<TipsInfoItem> {
        (0..10)
            .map(|i| TipsInfoItem {
                episode: (i % 2) as u8,
                title_index: i,
                title: shin_core::format::text::U16String::new(format!("Tip{i}")),
                content: shin_core::format::text::U16String::new("Content"),
            })
            .collect()
    }
    fn active() -> TipsModel {
        let mut model = TipsModel::new(&items(), -1, None).unwrap();
        for i in 0..10 {
            model.set_row_height(i, 1730.0);
        }
        model.update(10.0);
        model
    }
    #[test]
    fn trackpad_wheel_scrolls_tips_without_changing_selection() {
        let mut model = active();
        for (amount, expected) in [
            (-3.25, 104.0),
            (-0.125, 108.0),
            (3.25, 4.0),
            (0.0, 4.0),
            (100.0, 0.0),
        ] {
            let (input, scroll) = crate::app::wheel_scroll_test_input(amount);
            model.set_axis(scroll);
            model.input(&input);
            assert_eq!(model.scroll(), expected);
            assert_eq!(model.selected(), 0);
            assert!(model.take_sounds().is_empty());
        }
    }
    fn controls(actions: &[AppAction]) -> EnumMap<AppAction, ActionState> {
        let mut state = EnumMap::default();
        for &action in actions {
            state[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        state
    }
    #[test]
    fn filtering_preserves_scenario_order_and_acquired_list_overrides_episode() {
        let model = TipsModel::new(&items(), 0, Some(&[7, 2, 7, -1, 99])).unwrap();
        assert_eq!(
            model
                .rows()
                .iter()
                .map(|r| r.scenario_index)
                .collect::<Vec<_>>(),
            [2, 7]
        );
        assert_eq!(model.rows()[0].title_index, 2);
        assert_eq!(model.rows()[0].selection(), 1.0);
        assert_eq!(model.rows()[1].selection(), 0.0);
        let model = TipsModel::new(&items(), 1, None).unwrap();
        assert_eq!(
            model
                .rows()
                .iter()
                .map(|r| r.scenario_index)
                .collect::<Vec<_>>(),
            [1, 3, 5, 7, 9]
        );
        assert!(TipsModel::new(&items(), -1, Some(&[])).is_none());
        assert!(TipsModel::new(&items(), 9, None).is_none());
    }
    #[test]
    fn controller_clamps_and_changed_selection_consumes_cancel_and_axis() {
        let mut model = active();
        model.set_axis(1.0);
        model.input(&controls(&[AppAction::AnyDown, AppAction::Cancel]));
        assert_eq!(model.selected(), 1);
        assert_eq!(model.scroll(), 0.0);
        assert_eq!(model.phase(), TipsPhase::Active);
        assert_eq!(model.take_sounds(), ["move"]);
        model.update(2.0);
        assert!((model.rows()[0].selection() - 0.8).abs() < 1e-6);
        assert_eq!(model.rows()[1].selection(), 0.5);
        model.input(&controls(&[]));
        assert_eq!(model.scroll(), 32.0);
        model.input(&controls(&[AppAction::Up, AppAction::AnyDown]));
        assert_eq!(model.selected(), 0);
        assert_eq!(model.scroll(), 0.0);
        model.input(&controls(&[AppAction::Up, AppAction::AnyDown]));
        assert_eq!(model.selected(), 0); // Up at endpoint still wins over Down.
        assert_eq!(model.scroll(), 32.0);
        model.input(&controls(&[AppAction::Up, AppAction::Cancel]));
        assert_eq!(model.phase(), TipsPhase::CloseFlash);
        assert_eq!(model.take_sounds(), ["move", "cancel"]);
    }
    #[test]
    fn pointer_hit_skips_selected_and_captures_row_until_reset() {
        let mut model = active();
        assert!(model.probe(vec2(100.0, 200.0)));
        assert_eq!(model.hit, 0);
        model.release(Vec2::ZERO);
        assert!(model.take_sounds().is_empty());
        assert!(model.probe(vec2(100.0, 300.0)));
        assert_eq!(model.hit, 2);
        model.press(Vec2::ZERO);
        assert_eq!(model.selected(), 1);
        assert!(model.take_sounds().is_empty());
        model.update(3.5);
        assert_eq!(model.rows()[1].flash(), 0.7);
        model.drag(vec2(100.0, 300.0), vec2(1800.0, 40.0));
        assert_eq!(model.hit, 2);
        model.release(vec2(1800.0, 40.0));
        assert_eq!(model.phase(), TipsPhase::Active);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.reset();
        assert_eq!(model.hit, -1);
        assert!(!model.body_drag);
        for p in [vec2(89.0, 300.0), vec2(548.0, 300.0), vec2(100.0, 918.0)] {
            model.probe(p);
            model.release(p);
            assert!(model.take_sounds().is_empty());
        }
    }
    #[test]
    fn body_drag_inertia_bounds_and_stationary_update_match_native() {
        let mut model = active();
        model.probe(vec2(602.0, 188.0));
        model.drag(vec2(700.0, 500.0), vec2(700.0, 300.0));
        assert_eq!(model.scroll(), 200.0);
        model.gesture(vec2(100.0, -20.0));
        model.reset(); // Gesture momentum survives capture reset.
        model.update(2.0);
        assert_eq!(model.scroll(), 240.0);
        assert!((model.velocity() + 18.05).abs() < 1e-5);
        model.update(100.0);
        assert_eq!(model.scroll(), 1000.0);
        model.update(1.0);
        assert_eq!(model.velocity(), 0.0);
        model.set_axis(-0.5);
        model.input(&controls(&[]));
        assert_eq!(model.scroll(), 984.0);
        model.probe(vec2(1834.0, 500.0)); // Half-open content pane.
        model.drag(vec2(1834.0, 500.0), vec2(1834.0, 100.0));
        model.gesture(vec2(0.0, -50.0));
        assert_eq!(model.scroll(), 984.0);
        assert_eq!(model.velocity(), 0.0);
        model.probe(vec2(700.0, 500.0));
        model.gesture(vec2(0.0, -10.0));
        model.update(0.0); // Native no-displacement branch cancels inertia.
        assert_eq!(model.velocity(), 0.0);
    }
    #[test]
    fn close_flash_returns_zero_then_slide_returns_one_once() {
        let mut model = active();
        model.probe(vec2(1764.0, 32.0));
        model.release(Vec2::ZERO);
        assert_eq!(model.take_sounds(), ["decide"]);
        model.update(7.0);
        assert_eq!(model.phase(), TipsPhase::CloseFlash);
        assert!(model.take_responses().is_empty());
        model.update(0.0);
        assert_eq!(model.phase(), TipsPhase::Closing);
        assert_eq!(model.progress(), 1.0);
        assert_eq!(model.take_responses(), [0]);
        model.update(5.0);
        assert_eq!(model.progress(), 0.5);
        model.update(5.0);
        assert_eq!(model.take_responses(), [1]);
        model.update(20.0);
        assert!(model.take_responses().is_empty());
        assert_eq!(model.phase(), TipsPhase::Closed);
    }
    #[test]
    fn shared_contact_dispatch_consumes_controller_and_axis() {
        let mut model = active();
        model.set_axis(1.0);
        let mut area = InputArea::new(1920, 1080);
        let cancel = controls(&[AppAction::Cancel, AppAction::AnyDown]);
        let event = [PointerEvent {
            kind: PointerKind::Down,
            id: 0,
            position: IVec2::new(700, 500),
        }];
        area.input_frame(
            &mut model,
            &PointerFrame {
                number: 1,
                time_seconds: 0.0,
                events: &event,
            },
            &cancel,
        );
        area.input_frame(
            &mut model,
            &PointerFrame {
                number: 2,
                time_seconds: 0.2,
                events: &[],
            },
            &cancel,
        );
        assert_eq!(model.phase(), TipsPhase::Active);
        assert_eq!(model.selected(), 0);
        assert_eq!(model.scroll(), 0.0);
        let event = [PointerEvent {
            kind: PointerKind::Up,
            id: 0,
            position: IVec2::new(1800, 40),
        }];
        area.input_frame(
            &mut model,
            &PointerFrame {
                number: 3,
                time_seconds: 0.3,
                events: &event,
            },
            &cancel,
        );
        assert_eq!(model.phase(), TipsPhase::Active);
        assert!(model.take_sounds().is_empty());
    }
    #[test]
    #[ignore = "requires supplied ROM/executable (SHIN_REFERENCE_ROM)"]
    fn reference_native_tips_selection_pointer_scroll_and_close() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::io::Read;
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x106c2c, 0xb400023b),
            (0x106c40, 0x6b18015f),
            (0x106c70, 0xb940c348),
            (0x106c74, 0x3100051f),
            (0x106c7c, 0x39400269),
            (0x106c80, 0x6b09011f),
            (0x106cf8, 0x6b09011f),
            (0x106cfc, 0x1e280da0),
            (0x1072a0, 0xbd030260),
            (0x1072ac, 0x1e212809),
            (0x107414, 0x1e2703e0),
            (0x1074dc, 0x1e2a2920),
            (0x1074e8, 0x1e282808),
            (0x1074f0, 0xb901127f),
            (0x1077d4, 0x97ffe237),
            (0x1077f8, 0x1e280800),
            (0x1077fc, 0x1e203822),
            (0x10783c, 0xbd4ab902),
            (0x107844, 0x540001e4),
            (0x107864, 0x1e200920),
            (0x107874, 0x1e20c000),
            (0x108db0, 0x37c00629),
            (0x108db4, 0x36c80689),
            (0x108e74, 0x51000509),
            (0x108e78, 0x0aa97d35),
            (0x108e88, 0x372803a8),
            (0x108eb0, 0x1e220821),
            (0x108ef4, 0xb901127f),
            (0x1092d4, 0xb9010808),
            (0x109378, 0xb9010a60),
            (0x1093c4, 0x39043268),
            (0x1093c8, 0xb901127f),
            (0x109000, 0x6b09011f),
            (0x109004, 0x540003c0),
            (0x109074, 0x11000514),
            (0x10a100, 0x1e210800),
            (0x10a108, 0x1e212800),
            (0x10a110, 0x1e212000),
            (0x10a430, 0x91066260),
            (0x10a434, 0x97fec56f),
            (0x10a438, 0x360000a0),
            (0x108c44, 0x39045008),
            (0x108c48, 0xb90003ff),
            (0x108c68, 0xd63f0100),
            (0x10a54c, 0x1e210800),
            (0x10a554, 0x1e202820),
            (0x10a55c, 0x320003e0),
            (0x10a568, 0xb900c67f),
            (0x10a584, 0xd63f0100),
            (0x10a590, 0x3902c268),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, value) in [
            (0x159570, 0.1_f32),
            (0x15999c, -0.1),
            (0x159974, 0.2),
            (0x1595a4, 0.7),
            (0x1595d8, 0.95),
            (0x159694, 32.0),
            (0x159a70, -730.0),
            (0x159ab8, f32::EPSILON),
            (0x159670, 86.0),
            (0x159384, 188.0),
            (0x15968c, 552.0),
            (0x1594c8, 918.0),
            (0x159374, 602.0),
            (0x1593d4, 1834.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        assert_eq!(&exe[0x15bce1 + 256..0x15bce1 + 266], b"/tips.txa\0");
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        let mut read = |name| {
            let file = rom.find_file(name).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        let scenario = Scenario::new(read("/main.snr").into()).unwrap();
        let items = &scenario.info_tables().tips_info;
        let atlas =
            shin_core::format::texture_archive::read_texture_archive(&read("/tips.txa")).unwrap();
        assert!(atlas.name_to_index.contains_key("items"));
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| item.title_index < 26));
        let acquired = [items.len() as i32 - 1, 0, 0, -1, i32::MAX];
        let model = TipsModel::new(items, 99, Some(&acquired)).unwrap();
        assert_eq!(
            model
                .rows()
                .iter()
                .map(|r| r.scenario_index)
                .collect::<Vec<_>>(),
            [0, items.len() - 1]
        );
        for episode in 0..8 {
            let expected: Vec<_> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| i32::from(item.episode) == episode)
                .map(|(i, _)| i)
                .collect();
            let actual = TipsModel::new(items, episode, None);
            if expected.is_empty() {
                assert!(actual.is_none());
            } else {
                assert_eq!(
                    actual
                        .unwrap()
                        .rows()
                        .iter()
                        .map(|r| r.scenario_index)
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
        eprintln!(
            "Native Tips: {} scenario entries, {} atlas textures",
            items.len(),
            atlas.textures.len()
        );
    }
}
