//! Shared native SaveMenu (d5bc0/d95e0); screen and storage own asynchronous work.
pub(super) mod dialog;
pub(super) mod preview;
#[cfg(test)]
pub(super) mod render_reference;
pub(super) mod scene;
pub(crate) mod screen;
pub mod text;
pub mod view;
use super::input_area::InputAreaTarget;
use glam::Vec2;
use shin_core::{
    format::save::{GameData, SaveData},
    time::{Ticks, Tween, Tweener},
};

pub const SLOT_COUNT: usize = 100;
pub const ROW_STRIDE: f32 = 186.0;
pub const ROW_HEIGHT: f32 = 172.0;
pub const VIEWPORT_HEIGHT: f32 = 730.0;
pub const MAX_SCROLL: f32 = 17856.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMenuPhase {
    Loading,
    Opening,
    Active,
    RowFlash,
    Confirmation,
    Writing,
    CloseFlash,
    Closing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotAction {
    Save,
    Load,
    Delete,
}
impl SlotAction {
    pub fn prompt(self, overwrite: bool) -> &'static str {
        match self {
            Self::Save if overwrite => {
                "セーブを行います。@r内容は上書きされますが、よろしいですか？"
            }
            Self::Save => "セーブを行います。よろしいですか？",
            Self::Load => "ロードします。よろしいですか？",
            Self::Delete => "このセーブデータを削除します。よろしいですか？",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SaveMenuRequest {
    Confirmation {
        action: SlotAction,
        slot: usize,
        overwrite: bool,
    },
    Save {
        slot: usize,
        game: GameData,
    },
    Delete {
        slot: usize,
    },
    Load {
        slot: usize,
        game: GameData,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SaveMenuInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub previous_page: bool,
    pub next_page: bool,
    pub confirm: bool,
    pub delete: bool,
    pub close: bool,
    pub axis: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMenuPointer {
    Outside,
    Close,
    Content(Option<usize>),
    InvalidContent,
}

/// Entry presence selects native mode0 Save or mode1 Load. Slots are a retained
/// menu snapshot; the owner publishes successful writes back with write_finished.
pub struct SaveMenuModel {
    entry: Option<GameData>,
    slots: [Option<GameData>; SLOT_COUNT],
    selected: usize,
    remembered: u8,
    phase: SaveMenuPhase,
    progress: f32,
    scroll: Tweener,
    row_flash: [Tweener; SLOT_COUNT],
    close_flash: Tweener,
    action: Option<SlotAction>,
    dialog_finished: bool,
    write_finished: bool,
    write_backup: Option<(Option<GameData>, u8)>,
    load: Option<GameData>,
    requests: Vec<SaveMenuRequest>,
    responses: Vec<i32>,
    sounds: Vec<&'static str>,
    pointer: SaveMenuPointer,
    velocity: f32,
}

fn retarget(t: &mut Tweener, value: f32) {
    t.enqueue_now(
        value,
        Tween::linear(Ticks::from_f32((value - t.value()).abs() / 0.2)),
    );
}
fn flash(t: &mut Tweener) {
    retarget(t, 0.7);
    t.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
}

impl SaveMenuModel {
    pub fn new(data: &SaveData, entry: Option<GameData>) -> Self {
        let selected = usize::from(data.save_menu_position).min(SLOT_COUNT - 1);
        // Native d5e04 centers the remembered row: index*186-365+86.
        let scroll = (selected as f32 * ROW_STRIDE - 279.0).clamp(0.0, MAX_SCROLL);
        Self {
            entry,
            slots: data.manual_save_slots.clone(),
            selected,
            remembered: data.save_menu_position,
            phase: SaveMenuPhase::Loading,
            progress: 0.0,
            scroll: Tweener::new(scroll),
            row_flash: std::array::from_fn(|_| Tweener::new(0.0)),
            close_flash: Tweener::new(0.0),
            action: None,
            dialog_finished: false,
            write_finished: false,
            write_backup: None,
            load: None,
            requests: Vec::new(),
            responses: Vec::new(),
            sounds: Vec::new(),
            pointer: SaveMenuPointer::InvalidContent,
            velocity: 0.0,
        }
    }
    /// Native First waits for its resource job before installing opening work.
    /// The screen calls this on completion and does not advance that frame.
    pub fn assets_ready(&mut self) {
        if self.phase == SaveMenuPhase::Loading {
            self.phase = SaveMenuPhase::Opening;
        }
    }
    pub fn is_save(&self) -> bool {
        self.entry.is_some()
    }
    pub fn phase(&self) -> SaveMenuPhase {
        self.phase
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn remembered(&self) -> u8 {
        self.remembered
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn row_flash(&self, slot: usize) -> f32 {
        self.row_flash[slot].value()
    }
    pub fn close_flash(&self) -> f32 {
        self.close_flash.value()
    }
    pub fn slots(&self) -> &[Option<GameData>; SLOT_COUNT] {
        &self.slots
    }
    pub fn pointer_target(&self) -> SaveMenuPointer {
        self.pointer
    }
    pub fn velocity(&self) -> f32 {
        self.velocity
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == SaveMenuPhase::Active && self.scroll.is_idle()
    }
    pub fn take_requests(&mut self) -> Vec<SaveMenuRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_responses(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.responses)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn visible_slots(&self) -> impl Iterator<Item = usize> + '_ {
        // Native d6994 considers the entire canvas for preview scheduling,
        // including rows beneath the caption/footer, rather than its stencil.
        (0..SLOT_COUNT).filter(|&slot| {
            let top = 188.0 + slot as f32 * ROW_STRIDE - self.scroll.value();
            top < 1080.0 && top + ROW_HEIGHT >= 0.0
        })
    }

    pub fn input(&mut self, input: SaveMenuInput) {
        if !self.accepts_input() {
            return;
        }
        // Native repeat-bit priority; clamping a direction still permits actions.
        let delta = if input.up {
            -1
        } else if input.down {
            1
        } else if input.left {
            -4
        } else if input.right {
            4
        } else if input.previous_page {
            -10
        } else if input.next_page {
            10
        } else {
            0
        };
        let next = (self.selected as i32 + delta).clamp(0, 99) as usize;
        if next != self.selected {
            self.velocity = 0.0;
            self.selected = next;
            self.reveal_selection();
            self.sounds.push("move");
        } else if input.confirm {
            self.velocity = 0.0;
            self.request_action(if self.is_save() {
                SlotAction::Save
            } else {
                SlotAction::Load
            });
        } else if input.delete {
            // Native d97a8 consumes the delete bit even in Load mode.
            if self.is_save() {
                self.velocity = 0.0;
                self.request_action(SlotAction::Delete);
            }
        } else if input.close {
            flash(&mut self.close_flash);
            self.phase = SaveMenuPhase::CloseFlash;
            self.sounds.push("cancel");
        } else if input.axis.is_finite() && input.axis != 0.0 {
            self.velocity = 0.0;
            // Native analog scrolling changes selection only if it leaves view.
            self.set_drag_scroll(self.scroll.value() + input.axis * 32.0);
        }
    }

    fn reveal_selection(&mut self) {
        let top = self.selected as f32 * ROW_STRIDE;
        let target = if top < self.scroll.value() {
            top.max(0.0)
        } else if top + ROW_HEIGHT > self.scroll.value() + VIEWPORT_HEIGHT {
            (top + ROW_HEIGHT - VIEWPORT_HEIGHT).min(MAX_SCROLL)
        } else {
            self.scroll.value()
        };
        // Native enqueues four ticks even if no scroll distance is needed.
        self.scroll
            .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
    }

    fn request_action(&mut self, action: SlotAction) {
        // Native error path still starts the row flash.
        flash(&mut self.row_flash[self.selected]);
        if action != SlotAction::Save && self.slots[self.selected].is_none() {
            self.sounds.push("error");
            return;
        }
        self.action = Some(action);
        self.phase = SaveMenuPhase::RowFlash;
        self.sounds.push("decide");
    }

    fn set_drag_scroll(&mut self, scroll: f32) {
        let scroll = scroll.clamp(0.0, MAX_SCROLL);
        while scroll > self.selected as f32 * ROW_STRIDE {
            self.selected += 1;
        }
        while scroll + VIEWPORT_HEIGHT < self.selected as f32 * ROW_STRIDE + ROW_HEIGHT {
            self.selected -= 1;
        }
        self.scroll = Tweener::new(scroll);
    }

    /// Generic Dialog supplies intermediate final_flag0 and completion final_flag1.
    pub fn dialog_response(&mut self, button: i32, final_flag: i32) {
        if !matches!(
            self.phase,
            SaveMenuPhase::Confirmation | SaveMenuPhase::Writing
        ) {
            return;
        }
        if final_flag == 0 && self.phase == SaveMenuPhase::Confirmation && button == 0 {
            match self.action.unwrap() {
                SlotAction::Save => {
                    self.requests.push(SaveMenuRequest::Save {
                        slot: self.selected,
                        game: self.entry.as_ref().unwrap().clone(),
                    });
                    self.phase = SaveMenuPhase::Writing;
                }
                SlotAction::Delete => {
                    self.requests.push(SaveMenuRequest::Delete {
                        slot: self.selected,
                    });
                    self.phase = SaveMenuPhase::Writing;
                }
                SlotAction::Load => {} // Native db1a0 waits for final1/Yes.
            }
        }
        if final_flag == 1 {
            if self.action == Some(SlotAction::Load) && button == 0 {
                self.load = Some(self.slots[self.selected].as_ref().unwrap().clone());
                self.remembered = self.selected as u8;
                self.phase = SaveMenuPhase::Closing;
            } else if self.phase == SaveMenuPhase::Writing {
                self.dialog_finished = true;
                self.finish_write_if_ready();
            } else {
                self.phase = SaveMenuPhase::Active;
                self.action = None;
            }
        }
    }

    /// Native da9a0/db640 publish the selected slot before SaveGame opens its
    /// updating dialog; dafb0 restores the old context if the result fails.
    pub fn write_started(&mut self, game: Option<GameData>) {
        if self.phase != SaveMenuPhase::Writing
            || self.write_backup.is_some()
            || self.write_finished
        {
            return;
        }
        self.write_backup = Some((self.slots[self.selected].clone(), self.remembered));
        if game.is_some() {
            self.remembered = self.selected as u8;
        }
        self.slots[self.selected] = game;
    }

    /// Called once after the worker and its updating dialog finish.
    pub fn write_finished(&mut self, success: bool, game: Option<GameData>) {
        if self.phase != SaveMenuPhase::Writing || self.write_finished {
            return;
        }
        if success {
            match self.action.unwrap() {
                SlotAction::Save => {
                    assert!(
                        game.is_some(),
                        "successful save must supply its timestamped entry"
                    );
                    self.slots[self.selected] = game;
                    self.remembered = self.selected as u8;
                }
                SlotAction::Delete => self.slots[self.selected] = None,
                SlotAction::Load => unreachable!(),
            }
        } else if let Some((previous, remembered)) = self.write_backup.take() {
            self.slots[self.selected] = previous;
            self.remembered = remembered;
        }
        self.write_backup = None;
        self.write_finished = true;
        self.finish_write_if_ready();
    }
    fn finish_write_if_ready(&mut self) {
        if self.dialog_finished && self.write_finished {
            self.phase = SaveMenuPhase::Active;
            self.action = None;
            self.dialog_finished = false;
            self.write_finished = false;
        }
    }

    pub fn update(&mut self, ticks: f32) {
        if !ticks.is_finite() || ticks < 0.0 {
            return;
        }
        // Native d6708 integrates momentum before slot feedback updates.
        if self.velocity != 0.0 {
            let previous = self.scroll.value();
            let next = (previous - self.velocity * ticks).clamp(0.0, MAX_SCROLL);
            if (previous - next).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.set_drag_scroll(next);
                self.velocity *= 0.95f32.powf(ticks);
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        // Native task work precedes this frame's feedback clocks.
        match self.phase {
            SaveMenuPhase::Opening => {
                self.progress = (self.progress + ticks * 0.1).min(1.0);
                if self.progress == 1.0 {
                    self.phase = SaveMenuPhase::Active;
                }
            }
            SaveMenuPhase::RowFlash if self.row_flash[self.selected].is_idle() => {
                self.phase = SaveMenuPhase::Confirmation;
                self.requests.push(SaveMenuRequest::Confirmation {
                    action: self.action.unwrap(),
                    slot: self.selected,
                    overwrite: self.slots[self.selected].is_some(),
                });
            }
            SaveMenuPhase::CloseFlash if self.close_flash.is_idle() => {
                self.responses.push(0);
                self.phase = SaveMenuPhase::Closing;
            }
            SaveMenuPhase::Closing => {
                self.progress = (self.progress - ticks * 0.1).max(0.0);
                if self.progress == 0.0 {
                    self.phase = SaveMenuPhase::Closed;
                    if let Some(entry) = self.load.take() {
                        self.requests.push(SaveMenuRequest::Load {
                            slot: self.selected,
                            game: entry,
                        });
                    } else {
                        self.responses.push(1);
                    }
                }
            }
            _ => {}
        }
        let delta = Ticks::from_f32(ticks);
        self.scroll.update(delta);
        self.close_flash.update(delta);
        for row in &mut self.row_flash {
            row.update(delta);
        }
    }
}

impl InputAreaTarget for SaveMenuModel {
    fn probe(&mut self, p: Vec2) -> bool {
        if self.phase != SaveMenuPhase::Active || !p.is_finite() {
            return false;
        }
        if (1764.0..1848.0).contains(&p.x) && (32.0..116.0).contains(&p.y) {
            self.pointer = SaveMenuPointer::Close;
            return true;
        }
        if !(150.0..992.0).contains(&p.y) {
            self.pointer = SaveMenuPointer::Outside;
            return true;
        }
        if !(136.0..1784.0).contains(&p.x) || !(188.0..918.0).contains(&p.y) {
            self.pointer = SaveMenuPointer::InvalidContent;
            return false;
        }
        // Native truncates the scroll-adjusted Y before division/remainder.
        let y = (self.scroll.value() - 188.0 + p.y) as i32;
        let slot = y / 186;
        self.pointer = SaveMenuPointer::Content(
            (y % 186 < 172 && (0..100).contains(&slot)).then_some(slot as usize),
        );
        if self.velocity.abs() >= f32::EPSILON {
            self.velocity = 0.0;
            self.pointer = SaveMenuPointer::Content(None);
        }
        true
    }
    fn reset(&mut self) {
        self.pointer = SaveMenuPointer::InvalidContent;
    }
    fn press(&mut self, _: Vec2) {
        match self.pointer {
            SaveMenuPointer::Close => retarget(&mut self.close_flash, 0.7),
            SaveMenuPointer::Content(Some(slot)) => {
                retarget(&mut self.row_flash[slot], 0.7);
                self.selected = slot;
                self.reveal_selection();
            }
            _ => {}
        }
    }
    fn release(&mut self, _: Vec2) {
        match self.pointer {
            SaveMenuPointer::Close => {
                flash(&mut self.close_flash);
                self.phase = SaveMenuPhase::CloseFlash;
                self.sounds.push("cancel");
            }
            SaveMenuPointer::Content(Some(slot)) => {
                self.selected = slot;
                self.reveal_selection();
                self.request_action(if self.is_save() {
                    SlotAction::Save
                } else {
                    SlotAction::Load
                });
            }
            _ => {}
        }
    }
    fn long_press(&mut self, _: Vec2) {
        if let SaveMenuPointer::Content(Some(slot)) = self.pointer {
            if self.is_save() {
                self.selected = slot;
                self.reveal_selection();
                self.request_action(SlotAction::Delete);
            } else {
                retarget(&mut self.row_flash[slot], 0.0);
            }
            self.pointer = SaveMenuPointer::Content(None);
        }
    }
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        if !previous.is_finite() || !current.is_finite() {
            return;
        }
        match self.pointer {
            SaveMenuPointer::Close | SaveMenuPointer::Outside => {
                retarget(&mut self.close_flash, 0.0);
                self.pointer = SaveMenuPointer::Outside;
            }
            SaveMenuPointer::Content(slot) => {
                if let Some(slot) = slot {
                    retarget(&mut self.row_flash[slot], 0.0);
                }
                self.pointer = SaveMenuPointer::Content(None);
                self.set_drag_scroll(self.scroll.value() - (current.y - previous.y));
            }
            SaveMenuPointer::InvalidContent => {}
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if matches!(self.pointer, SaveMenuPointer::Content(_)) && velocity.y.is_finite() {
            self.velocity = velocity.y;
        }
    }
    fn cancel(&mut self) {
        match self.pointer {
            SaveMenuPointer::Close | SaveMenuPointer::Outside => {
                retarget(&mut self.close_flash, 0.0);
                self.pointer = SaveMenuPointer::Outside;
            }
            SaveMenuPointer::Content(Some(slot)) => retarget(&mut self.row_flash[slot], 0.0),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::VmState;

    #[test]
    fn pointer_bounds_gaps_and_momentum_stop_use_native_capture_roles() {
        let mut model = open(0, true, false);
        assert!(model.probe(Vec2::new(1764.0, 32.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Close);
        assert!(!model.probe(Vec2::new(1848.0, 32.0 + 118.0)));
        assert!(model.probe(Vec2::new(0.0, 149.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Outside);
        assert!(!model.probe(Vec2::new(136.0, 187.0)));
        assert!(model.probe(Vec2::new(136.0, 188.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Content(Some(0)));
        assert!(model.probe(Vec2::new(136.0, 360.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Content(None));
        assert!(model.probe(Vec2::new(136.0, 374.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Content(Some(1)));
        assert!(!model.probe(Vec2::new(1784.0, 400.0)));
        assert!(!model.probe(Vec2::new(200.0, 918.0)));
        assert!(model.probe(Vec2::new(200.0, 992.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Outside);
        model.probe(Vec2::new(200.0, 360.0));
        model.gesture(Vec2::new(0.0, -30.0));
        model.update(1.0);
        assert_eq!(model.scroll(), 30.0);
        assert_eq!(model.velocity(), -28.5);
        assert!(model.probe(Vec2::new(200.0, 400.0)));
        assert_eq!(model.pointer_target(), SaveMenuPointer::Content(None));
        assert_eq!(model.velocity(), 0.0);
        model.release(Vec2::ZERO);
        assert_eq!(model.phase(), SaveMenuPhase::Active);
        assert!(model.take_sounds().is_empty());
    }

    #[test]
    fn held_row_release_uses_captured_slot_and_remaining_flash_distance() {
        let mut model = open(0, true, false);
        model.probe(Vec2::new(200.0, 400.0));
        model.press(Vec2::ZERO);
        assert_eq!(model.selected(), 1);
        model.update(2.0);
        assert!((model.row_flash(1) - 0.4).abs() < 1e-6);
        model.release(Vec2::new(200.0, 800.0));
        assert_eq!(model.selected(), 1, "release uses the captured row");
        assert_eq!(model.phase(), SaveMenuPhase::RowFlash);
        model.update(5.0); // .4->.7 takes1.5 ticks, then.7->0 takes3.5.
        model.update(0.0);
        assert!(matches!(
            model.take_requests().as_slice(),
            [SaveMenuRequest::Confirmation { slot: 1, .. }]
        ));
    }

    #[test]
    fn dragging_cancels_row_action_and_scrolls_then_friction_stops_at_boundary() {
        let mut model = open(50, true, true);
        let p = Vec2::new(200.0, 500.0);
        model.probe(p);
        model.press(p);
        model.update(1.0);
        model.drag(p, p - Vec2::new(0.0, 40.0));
        assert_eq!(model.scroll(), 9061.0);
        assert_eq!(model.pointer_target(), SaveMenuPointer::Content(None));
        model.gesture(Vec2::new(0.0, -20.0));
        model.reset();
        model.update(2.0);
        assert_eq!(model.scroll(), 9101.0);
        assert!((model.velocity() + 18.05).abs() < 1e-5);
        assert!(model.take_requests().is_empty());
        model.update(10000.0);
        assert_eq!(model.scroll(), MAX_SCROLL);
        assert_eq!(model.velocity(), 0.0);
        let p = Vec2::new(1800.0, 60.0);
        model.probe(p);
        model.press(p);
        model.update(1.0);
        model.drag(p, p + Vec2::new(20.0, 20.0));
        model.release(p);
        assert_eq!(model.phase(), SaveMenuPhase::Active);
        model.update(10.0);
        assert_eq!(model.close_flash(), 0.0);
        assert!(model.take_responses().is_empty());
    }

    #[test]
    fn native_input_area_long_press_deletes_in_save_mode_and_cancels_in_load_mode() {
        use super::super::input_area::{InputArea, PointerEvent, PointerFrame, PointerKind};
        for save in [true, false] {
            let mut model = open(0, save, true);
            let mut area = InputArea::new(960, 540);
            area.set_long_press_enabled(true);
            let down = [PointerEvent {
                kind: PointerKind::Down,
                id: 3,
                position: glam::IVec2::new(100, 100),
            }];
            assert!(area.update(
                &mut model,
                &PointerFrame {
                    number: 1,
                    time_seconds: 0.0,
                    events: &down
                }
            ));
            assert!(area.update(
                &mut model,
                &PointerFrame {
                    number: 2,
                    time_seconds: 0.2,
                    events: &[]
                }
            ));
            model.update(4.0);
            assert!(area.update(
                &mut model,
                &PointerFrame {
                    number: 3,
                    time_seconds: 1.2,
                    events: &[]
                }
            ));
            let up = [PointerEvent {
                kind: PointerKind::Up,
                id: 3,
                position: glam::IVec2::ZERO,
            }];
            area.update(
                &mut model,
                &PointerFrame {
                    number: 4,
                    time_seconds: 1.3,
                    events: &up,
                },
            );
            model.update(7.0);
            model.update(0.0);
            if save {
                assert!(matches!(
                    model.take_requests().as_slice(),
                    [SaveMenuRequest::Confirmation {
                        action: SlotAction::Delete,
                        slot: 0,
                        ..
                    }]
                ));
            } else {
                assert!(model.take_requests().is_empty());
                assert_eq!(model.phase(), SaveMenuPhase::Active);
                assert_eq!(model.row_flash(0), 0.0);
            }
        }
    }

    #[test]
    #[ignore = "requires supplied decrypted Switch executable"]
    fn reference_native_save_menu_navigation_callback_phases_and_geometry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../game-data/umineko/uncompressed_executable");
        let bytes = std::fs::read(path).unwrap();
        for (address, expected) in [
            (0xd9630, 0x37c00269u32),
            (0xd9634, 0x37c80289),
            (0xd9638, 0x37d002a9),
            (0xd963c, 0x37d802c9),
            (0xd9640, 0x375002e9),
            (0xd9648, 0x7215013f),
            (0xd967c, 0x51000509),
            (0xd9684, 0x11000509),
            (0xd968c, 0x51001109),
            (0xd9694, 0x11001109),
            (0xd969c, 0x51002909),
            (0xd96a0, 0x71018d3f),
            (0xd96a4, 0x52800c6a),
            (0xd98d0, 0xb9401268),
            (0xd98f8, 0x34000528),
            (0xdb1b8, 0x340000e8),
            (0xdb1c0, 0x34000148),
            (0xdb660, 0x340000c8),
            (0xdb68c, 0x3904a11f),
            (0xdaa70, 0x390002b6),
            (0xdb24c, 0x39000288),
            (0xd9b28, 0xb9016c08),
            (0xd9b64, 0xb9016a60),
            (0xd9ba4, 0xb9016a6a),
            (0xd9c04, 0x1e380008),
            (0xd9c28, 0x7102b15f),
            (0xd9c2c, 0x54000123),
            (0xd9c74, 0x54000064),
            (0xd9c7c, 0xf900011f),
            (0xda26c, 0x1e293901),
            (0xda270, 0x1e213800),
            (0xda300, 0x97ff83f8),
            (0xda340, 0xbd017001),
            (0xda1a8, 0xb9016e7f),
            (0xd67e4, 0x94011917),
            (0xd67fc, 0x1e212000),
            (0xd9860, 0xb901727f),
        ] {
            assert_eq!(
                u32::from_le_bytes(bytes[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x159a08, MAX_SCROLL),
            (0x159618, VIEWPORT_HEIGHT),
            (0x159370, ROW_HEIGHT),
            (0x159440, -188.0),
            (0x159694, 32.0),
            (0x159658, 116.0),
            (0x159480, 1764.0),
            (0x15971c, 1848.0),
            (0x159a20, 150.0),
            (0x159944, 992.0),
            (0x1594c8, 918.0),
            (0x159340, 136.0),
            (0x1595dc, 1784.0),
            (0x159384, 188.0),
            (0x1595d8, 0.95),
            (0x159570, 0.1),
        ] {
            assert_eq!(
                f32::from_le_bytes(bytes[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native float{address:x}"
            );
        }
        for (address, action, overwrite) in [
            (0x161f10, SlotAction::Save, true),
            (0x161087, SlotAction::Save, false),
            (0x161b6a, SlotAction::Load, false),
            (0x161b98, SlotAction::Delete, false),
        ] {
            let expected = action.prompt(overwrite).as_bytes();
            assert_eq!(
                &bytes[address + 256..address + 256 + expected.len()],
                expected
            );
            assert_eq!(bytes[address + 256 + expected.len()], 0);
        }
    }

    fn open(position: u8, save: bool, occupied: bool) -> SaveMenuModel {
        let mut data = SaveData::default();
        data.save_menu_position = position;
        let entry = VmState::for_scenario(7, 42).snapshot_entry();
        if occupied {
            data.manual_save_slots[position as usize] = Some(GameData::from_entry(entry.clone()));
        }
        let mut model = SaveMenuModel::new(&data, save.then_some(GameData::from_entry(entry)));
        model.update(100.0);
        assert_eq!(model.phase(), SaveMenuPhase::Loading);
        model.assets_ready();
        assert_eq!(model.progress(), 0.0);
        model.update(10.0);
        assert!(model.accepts_input());
        model
    }
    fn confirm(model: &mut SaveMenuModel, delete: bool) {
        model.input(SaveMenuInput {
            confirm: !delete,
            delete,
            ..Default::default()
        });
        model.update(7.0);
        assert_eq!(
            model.phase(),
            SaveMenuPhase::RowFlash,
            "task observes feedback before its clock update"
        );
        model.update(0.0);
        assert_eq!(model.phase(), SaveMenuPhase::Confirmation);
    }

    #[test]
    fn selection_centers_remembered_slot_and_navigation_precedes_actions() {
        let mut model = open(50, true, false);
        assert_eq!(model.scroll(), 9021.0);
        assert_eq!(
            model.visible_slots().collect::<Vec<_>>(),
            [47, 48, 49, 50, 51, 52, 53]
        );
        model.input(SaveMenuInput {
            up: true,
            down: true,
            confirm: true,
            close: true,
            ..Default::default()
        });
        assert_eq!(model.selected(), 49);
        assert_eq!(model.take_sounds(), ["move"]);
        assert!(model.take_requests().is_empty());
        assert!(
            !model.accepts_input(),
            "native four-tick lock applies even within viewport"
        );
        model.input(SaveMenuInput {
            next_page: true,
            ..Default::default()
        });
        assert_eq!(model.selected(), 49);
        model.update(4.0);
        model.input(SaveMenuInput {
            next_page: true,
            ..Default::default()
        });
        assert_eq!(model.selected(), 59);
        model.update(4.0);
        assert_eq!(model.scroll(), 10416.0);
        let mut last = open(99, false, false);
        assert_eq!(last.scroll(), MAX_SCROLL);
        last.input(SaveMenuInput {
            down: true,
            close: true,
            ..Default::default()
        });
        assert_eq!(
            last.phase(),
            SaveMenuPhase::CloseFlash,
            "clamped movement allows close"
        );
    }

    #[test]
    fn empty_load_and_delete_flash_error_without_opening_dialog() {
        for save in [false, true] {
            let mut model = open(0, save, false);
            model.input(SaveMenuInput {
                confirm: !save,
                delete: save,
                ..Default::default()
            });
            assert_eq!(model.take_sounds(), ["error"]);
            assert_eq!(model.phase(), SaveMenuPhase::Active);
            model.update(3.5);
            assert!((model.row_flash(0) - 0.7).abs() < 1e-6);
            assert!(model.take_requests().is_empty());
        }
        let mut load = open(0, false, true);
        load.input(SaveMenuInput {
            delete: true,
            close: true,
            ..Default::default()
        });
        assert_eq!(load.phase(), SaveMenuPhase::Active);
        assert!(load.take_sounds().is_empty());
    }

    #[test]
    fn save_commits_intermediate_once_and_waits_for_storage_and_dialog() {
        let mut model = open(99, true, false);
        confirm(&mut model, false);
        assert!(matches!(
            model.take_requests().as_slice(),
            [SaveMenuRequest::Confirmation {
                action: SlotAction::Save,
                slot: 99,
                overwrite: false
            }]
        ));
        model.dialog_response(0, 0);
        let requests = model.take_requests();
        let [SaveMenuRequest::Save { slot: 99, game }] = requests.as_slice() else {
            panic!("save request missing")
        };
        assert_eq!((game.entry.scenario_id, game.entry.random_seed), (7, 42));
        model.dialog_response(0, 0);
        assert!(model.take_requests().is_empty());
        model.write_finished(true, Some(game.clone()));
        assert_eq!(model.phase(), SaveMenuPhase::Writing);
        assert!(model.slots()[99].is_some());
        assert!(!model.accepts_input());
        model.dialog_response(0, 1);
        assert!(model.accepts_input());
        confirm(&mut model, false);
        assert!(matches!(
            model.take_requests().as_slice(),
            [SaveMenuRequest::Confirmation {
                overwrite: true,
                ..
            }]
        ));
        model.dialog_response(1, 0);
        model.dialog_response(1, 1);
        assert!(model.accepts_input());
        assert!(model.take_requests().is_empty());
    }

    #[test]
    fn delete_retains_selection_and_failed_write_retains_slot() {
        let mut model = open(37, true, true);
        model.remembered = 12;
        confirm(&mut model, true);
        model.take_requests();
        model.dialog_response(0, 0);
        assert!(matches!(
            model.take_requests().as_slice(),
            [SaveMenuRequest::Delete { slot: 37 }]
        ));
        model.dialog_response(0, 1);
        assert_eq!(model.phase(), SaveMenuPhase::Writing);
        model.write_finished(false, None);
        assert!(model.slots()[37].is_some());
        assert_eq!(model.remembered(), 12);
        assert!(model.accepts_input());
        confirm(&mut model, true);
        model.take_requests();
        model.dialog_response(0, 0);
        model.write_finished(true, None);
        model.dialog_response(0, 1);
        assert!(model.slots()[37].is_none());
        assert_eq!(model.remembered(), 12);
        assert!(model.accepts_input());
    }

    #[test]
    fn load_waits_for_final_dialog_and_fade_before_scene_handoff() {
        let mut model = open(37, false, true);
        confirm(&mut model, false);
        model.take_requests();
        model.dialog_response(0, 0);
        assert_eq!(model.phase(), SaveMenuPhase::Confirmation);
        assert!(model.take_requests().is_empty());
        model.dialog_response(0, 1);
        assert_eq!(model.phase(), SaveMenuPhase::Closing);
        assert!(
            model.take_responses().is_empty(),
            "successful load does not reopen parent menu"
        );
        model.update(9.0);
        assert!(model.take_requests().is_empty());
        model.update(1.0);
        assert!(matches!(
            model.take_requests().as_slice(),
            [SaveMenuRequest::Load { slot: 37, .. }]
        ));
        assert_eq!(model.phase(), SaveMenuPhase::Closed);
        model.update(100.0);
        assert!(model.take_requests().is_empty());
        assert!(model.take_responses().is_empty());
    }

    #[test]
    fn ordinary_close_emits_both_callbacks_and_analog_scroll_clamps() {
        let mut model = open(0, true, false);
        model.input(SaveMenuInput {
            axis: 1000.0,
            ..Default::default()
        });
        assert_eq!(model.scroll(), MAX_SCROLL);
        assert_eq!(model.selected(), 96);
        model.input(SaveMenuInput {
            axis: -1000.0,
            ..Default::default()
        });
        assert_eq!(model.scroll(), 0.0);
        assert_eq!(model.selected(), 3);
        model.input(SaveMenuInput {
            close: true,
            ..Default::default()
        });
        model.update(7.0);
        assert_eq!(model.phase(), SaveMenuPhase::CloseFlash);
        model.update(0.0);
        assert_eq!(model.take_responses(), [0]);
        assert_eq!(model.progress(), 1.0);
        model.update(10.0);
        assert_eq!(model.take_responses(), [1]);
        model.update(10.0);
        assert!(model.take_responses().is_empty());
    }
    #[test]
    fn trackpad_wheel_scrolls_save_and_load_without_activating_a_slot() {
        for save in [false, true] {
            let mut model = open(0, save, false);
            for (amount, expected) in [
                (-3.25, 104.0),
                (-0.125, 108.0),
                (3.25, 4.0),
                (0.0, 4.0),
                (100.0, 0.0),
            ] {
                let (input, axis) = crate::app::wheel_scroll_test_input(amount);
                assert!(input.values().all(|state| !state.is_clicked));
                model.input(SaveMenuInput {
                    axis,
                    ..Default::default()
                });
                assert_eq!(model.scroll(), expected);
                assert!(model.take_requests().is_empty());
                assert_eq!(model.phase(), SaveMenuPhase::Active);
            }
        }
    }
}
