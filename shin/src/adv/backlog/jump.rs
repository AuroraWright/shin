//! SceneJump marker ordering and cursor scrolling (a8470/a94c0).
use super::{Backlog, BacklogItem};
use anyhow::{Result, ensure};
use shin_core::time::{Ticks, Tween, Tweener};

pub(crate) const ROW_HEIGHT: f32 = 60.0;
pub(crate) const VIEW_HEIGHT: f32 = 720.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub(crate) enum Kind {
    Beginning = 0,
    Checkpoint = 1,
    Cursor = 0x61,
    Current = 0x62,
    CursorAndCurrent = 0x63,
    End = 0x64,
}
impl Kind {
    pub fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Beginning => "先頭",
            Self::Checkpoint => return None,
            Self::Cursor => "カーソル位置",
            Self::Current => "ゲーム内現在位置",
            Self::CursorAndCurrent => "カーソル位置/ゲーム内現在位置",
            Self::End => "後尾",
        })
    }
    fn is_cursor(self) -> bool {
        matches!(self, Self::Cursor | Self::CursorAndCurrent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Marker {
    pub kind: Kind,
    /// Native signed payload-1 for special markers; checkpoint text comes from
    /// this authored backlog row. All marker targets are document row indexes.
    pub authored_row: Option<usize>,
    pub target_row: usize,
}

pub(crate) struct SceneJump {
    pub markers: Vec<Marker>,
    pub selected: usize,
    cursor_marker: usize,
    scroll: Tweener,
    scrolling_this_frame: bool,
    velocity: f32,
}
impl SceneJump {
    pub fn new(history: &Backlog, current_row: usize, cursor_row: usize) -> Result<Self> {
        ensure!(
            !history.entries.is_empty(),
            "Scene jump has no backlog rows"
        );
        ensure!(
            current_row < history.entries.len() && cursor_row < history.entries.len(),
            "Scene jump marker lies outside its history"
        );
        let mut markers = vec![Marker {
            kind: Kind::Beginning,
            authored_row: None,
            target_row: 0,
        }];
        for (index, row) in history.entries.iter().enumerate() {
            if !matches!(row.item, BacklogItem::Message { .. }) {
                markers.push(Marker {
                    kind: Kind::Checkpoint,
                    authored_row: Some(index),
                    target_row: index,
                });
            }
        }
        if current_row == cursor_row {
            markers.push(Marker {
                kind: Kind::CursorAndCurrent,
                authored_row: None,
                target_row: cursor_row,
            });
        } else {
            markers.push(Marker {
                kind: Kind::Cursor,
                authored_row: None,
                target_row: cursor_row,
            });
            markers.push(Marker {
                kind: Kind::Current,
                authored_row: None,
                target_row: current_row,
            });
        }
        markers.push(Marker {
            kind: Kind::End,
            authored_row: None,
            target_row: history.entries.len() - 1,
        });
        // adc30 sorts by target first, then marker type. Duplicated targets are
        // intentional: beginning/checkpoint and marker/end remain separate rows.
        markers.sort_by_key(|marker| (marker.target_row, marker.kind));
        let cursor_marker = markers
            .iter()
            .position(|marker| marker.kind.is_cursor())
            .unwrap();
        let max_scroll = (markers.len() as f32 * ROW_HEIGHT - VIEW_HEIGHT).max(0.0);
        let initial = (cursor_marker as f32 * ROW_HEIGHT - 330.0).clamp(0.0, max_scroll);
        let initial = (initial / ROW_HEIGHT).trunc() * ROW_HEIGHT;
        Ok(Self {
            markers,
            selected: cursor_marker,
            cursor_marker,
            scroll: Tweener::new(initial),
            scrolling_this_frame: false,
            velocity: 0.0,
        })
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn max_scroll(&self) -> f32 {
        (self.markers.len() as f32 * ROW_HEIGHT - VIEW_HEIGHT).max(0.0)
    }
    pub fn accepts_keyboard(&self) -> bool {
        self.scroll.is_idle()
    }
    pub fn selected_target(&self) -> usize {
        self.markers[self.selected].target_row
    }
    /// Returns a parent document target only when the marker selection changes.
    /// The live task must move the browser cursor too (a9574..a9584).
    pub fn navigate(
        &mut self,
        delta: i32,
        available: std::ops::RangeInclusive<usize>,
    ) -> Option<usize> {
        if !self.accepts_keyboard() {
            return None;
        }
        let index = (self.selected as i64 + i64::from(delta))
            .clamp(0, self.markers.len() as i64 - 1) as usize;
        self.select_available(index, available)
    }
    pub fn reset_cursor(&mut self, available: std::ops::RangeInclusive<usize>) -> Option<usize> {
        if !self.accepts_keyboard() {
            return None;
        }
        self.select_available(self.cursor_marker, available)
    }
    fn select_available(
        &mut self,
        index: usize,
        available: std::ops::RangeInclusive<usize>,
    ) -> Option<usize> {
        let first = self
            .markers
            .partition_point(|marker| marker.target_row < *available.start());
        let end = self
            .markers
            .partition_point(|marker| marker.target_row <= *available.end());
        if first >= end {
            return None;
        }
        let index = index.clamp(first, end - 1);
        if index == self.selected {
            return None;
        }
        self.selected = index;
        let top = index as f32 * ROW_HEIGHT;
        let current = self.scroll();
        let target = if top - current < ROW_HEIGHT {
            (top - ROW_HEIGHT).max(0.0)
        } else if top - current + ROW_HEIGHT > 660.0 {
            (top - 600.0).min(self.max_scroll())
        } else {
            current
        };
        self.scroll.fast_forward_to(current);
        self.scroll
            .enqueue(target, Tween::linear(Ticks::from_f32(4.0)));
        Some(self.selected_target())
    }
    /// Wheel/direct scrolling keeps selection strictly inside the viewport;
    /// a9630 moves rows touching either viewport edge to the next interior row.
    pub fn axis(&mut self, axis: f32) -> Option<usize> {
        if !axis.is_finite() || axis == 0.0 || !self.accepts_keyboard() {
            return None;
        }
        let target = (self.scroll() + axis * 100.0).clamp(0.0, self.max_scroll());
        self.scrolling_this_frame = true;
        self.direct_scroll(target)
    }
    fn direct_scroll(&mut self, target: f32) -> Option<usize> {
        self.scroll.fast_forward_to(target);
        let old = self.selected;
        while self.selected + 1 < self.markers.len() && self.selected as f32 * ROW_HEIGHT <= target
        {
            self.selected += 1;
        }
        while self.selected > 0 && (self.selected + 1) as f32 * ROW_HEIGHT >= target + VIEW_HEIGHT {
            self.selected -= 1;
        }
        (old != self.selected).then(|| self.selected_target())
    }
    /// aa758..aa800: hit rows by Y only. A press on a valid moving row stops
    /// inertia and suppresses that click; close/blocked presses leave it alone.
    pub fn press_row(&mut self, y: f32) -> Option<usize> {
        if !(240.0..960.0).contains(&y) {
            return None;
        }
        let position = (y - 240.0 + self.scroll()) as i32;
        let index = position / 60;
        if position < -59 || index < 0 || index as usize >= self.markers.len() {
            return None;
        }
        if self.velocity.abs() >= f32::EPSILON {
            self.velocity = 0.0;
            self.scrolling_this_frame = true;
            return None;
        }
        Some(index as usize)
    }
    pub fn pointer_select(
        &mut self,
        index: usize,
        available: std::ops::RangeInclusive<usize>,
    ) -> Option<usize> {
        self.select_available(index.min(self.markers.len() - 1), available)
    }
    pub fn drag(&mut self, dy: f32) -> Option<usize> {
        self.scrolling_this_frame = true;
        self.direct_scroll((self.scroll() - dy).clamp(0.0, self.max_scroll()))
    }
    pub fn release(&mut self, velocity: glam::Vec2) -> bool {
        let angle = velocity.y.atan2(velocity.x) / f32::from_bits(0x40490fd8) * 180.0;
        if angle > -30.0 && angle < 30.0 && velocity.length() >= 20.0 {
            true
        } else {
            self.velocity = velocity.y;
            false
        }
    }
    #[cfg(test)]
    pub fn update(&mut self) {
        self.update_with_delta(Ticks::from_f32(1.0));
    }
    pub fn update_with_delta(&mut self, delta: Ticks) -> Option<usize> {
        let mut selected = None;
        if self.velocity.abs() >= f32::EPSILON {
            let current = self.scroll();
            let target = (current - self.velocity * delta.as_f32()).clamp(0.0, self.max_scroll());
            if (current - target).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                selected = self.direct_scroll(target);
                self.velocity *= 0.95f32.powf(delta.as_f32());
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        // Idle scroll settles on the nearest sixty-pixel row (integer remainder,
        // rounding upward at30). Its tween advances one tick per update regardless
        // of the host frame delta; active direct scrolling suppresses settling.
        if self.velocity.abs() < f32::EPSILON
            && !std::mem::take(&mut self.scrolling_this_frame)
            && self.scroll.is_idle()
        {
            let value = self.scroll() as i32;
            let remainder = value % 60;
            if remainder != 0 {
                let target = value - remainder + if remainder > 29 { 60 } else { 0 };
                self.scroll
                    .enqueue(target as f32, Tween::linear(Ticks::from_f32(4.0)));
            }
        }
        self.scroll.update(Ticks::from_f32(1.0));
        self.scrolling_this_frame = false;
        selected
    }
}

#[cfg(test)]
mod tests {
    use crate::adv::backlog::BacklogEntry;
    use super::*;
    use shin_core::vm::command::types::MessageboxType;
    #[test]
    fn trackpad_wheel_scrolls_backlog_jump_list() {
        let mut model = SceneJump::new(&history(30, true), 0, 0).unwrap();
        model.update_with_delta(Ticks::from_f32(10.0));
        model.direct_scroll(0.0);
        for (amount, expected) in [
            (-3.25, 325.0),
            (-0.125, 337.5),
            (3.25, 12.5),
            (0.0, 12.5),
            (100.0, 0.0),
        ] {
            let (_, scroll) = crate::app::wheel_scroll_test_input(amount);
            model.axis(scroll);
            assert_eq!(model.scroll(), expected);
        }
    }
    fn history(n: usize, checkpoints: bool) -> Backlog {
        Backlog {
            entries: (0..n)
                .map(|i| BacklogEntry {
                    save_position: i as u32 + 1,
                    item: if checkpoints {
                        BacklogItem::Checkpoint {
                            title: format!("Chapter{i}"),
                            text: "Text".into(),
                        }
                    } else {
                        BacklogItem::Message {
                            message_id: i as u32,
                            text: "Message".into(),
                            messagebox_type: MessageboxType::Neutral,
                        }
                    },
                })
                .collect(),
        }
    }
    #[test]
    fn coincident_positions_merge_only_cursor_and_current_and_keep_endpoints() {
        let menu = SceneJump::new(&history(1, true), 0, 0).unwrap();
        assert_eq!(
            menu.markers
                .iter()
                .map(|m| (m.kind, m.authored_row, m.target_row))
                .collect::<Vec<_>>(),
            [
                (Kind::Beginning, None, 0),
                (Kind::Checkpoint, Some(0), 0),
                (Kind::CursorAndCurrent, None, 0),
                (Kind::End, None, 0)
            ]
        );
        assert_eq!(menu.selected, 2);
        assert_eq!(menu.scroll(), 0.0);
        assert_eq!(
            menu.markers[menu.selected].kind.label(),
            Some("カーソル位置/ゲーム内現在位置")
        );
    }
    #[test]
    fn marker_order_uses_target_then_type_and_default_selection_tracks_cursor() {
        let menu = SceneJump::new(&history(10, false), 7, 2).unwrap();
        assert_eq!(
            menu.markers
                .iter()
                .map(|m| (m.kind, m.target_row))
                .collect::<Vec<_>>(),
            [
                (Kind::Beginning, 0),
                (Kind::Cursor, 2),
                (Kind::Current, 7),
                (Kind::End, 9)
            ]
        );
        assert_eq!(menu.selected, 1);
        let menu = SceneJump::new(&history(10, false), 2, 7).unwrap();
        assert_eq!(menu.selected, 2);
    }
    #[test]
    fn choices_quizzes_and_autosaves_all_supply_authored_markers() {
        let mut history = history(4, false);
        history.entries[1].item = BacklogItem::Choice {
            title: "Question".into(),
            text: "Answer".into(),
            response: 1,
        };
        history.entries[2].item = BacklogItem::Quiz {
            argument: 0,
            mode: Some(2),
            response: 27,
        };
        history.entries[3].item = BacklogItem::Checkpoint {
            title: "Episode".into(),
            text: "Chapter".into(),
        };
        let menu = SceneJump::new(&history, 0, 2).unwrap();
        assert_eq!(
            menu.markers
                .iter()
                .filter_map(|m| m.authored_row)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }
    #[test]
    fn initial_scroll_snaps_to_native_sixty_pixel_grid_and_clamps() {
        for cursor in 0..30 {
            let menu = SceneJump::new(&history(30, true), cursor, cursor).unwrap();
            let selected = cursor + 2;
            assert_eq!(menu.selected, selected);
            let expected =
                ((selected as f32 * 60.0 - 330.0).clamp(0.0, 1260.0) / 60.0).trunc() * 60.0;
            assert_eq!(menu.scroll(), expected);
        }
    }
    #[test]
    fn navigation_clamps_to_native_available_parent_rows_and_freezes_during_scroll() {
        let mut menu = SceneJump::new(&history(30, true), 10, 10).unwrap();
        assert_eq!(menu.navigate(100, 8..=12), Some(12));
        assert!(menu.navigate(1, 8..=12).is_none());
        for _ in 0..4 {
            menu.update();
        }
        assert_eq!(menu.navigate(-100, 8..=12), Some(8));
        for _ in 0..4 {
            menu.update();
        }
        assert_eq!(menu.reset_cursor(8..=12), Some(10));
        for _ in 0..4 {
            menu.update();
        }
        assert!(menu.reset_cursor(8..=12).is_none());
        assert!(menu.navigate(1, 50..=60).is_none());
    }
    #[test]
    fn moving_near_view_edges_fits_with_one_row_margin_over_four_updates() {
        let mut menu = SceneJump::new(&history(30, true), 0, 0).unwrap();
        let last = menu.markers.len() - 1;
        assert_eq!(menu.navigate(last as i32, 0..=29), Some(29));
        assert_eq!(menu.scroll(), 0.0);
        let target = menu.max_scroll();
        for step in 1..=4 {
            menu.update();
            assert_eq!(menu.scroll(), target * step as f32 / 4.0);
        }
        assert_eq!(menu.navigate(-(last as i32), 0..=29), Some(0));
        for _ in 0..4 {
            menu.update();
        }
        assert_eq!(menu.scroll(), 0.0);
    }
    #[test]
    fn direct_scroll_keeps_cursor_inside_view_and_settles_to_native_row_grid() {
        let mut menu = SceneJump::new(&history(30, true), 0, 0).unwrap();
        assert_eq!(menu.axis(1.0), None); // Existing cursor marker is already interior.
        assert_eq!(menu.selected, 2);
        // First update suppresses settling; the next four finish its tween.
        // Start from beginning so scrolling past100 forces an interior marker.
        for _ in 0..5 {
            menu.update();
        }
        assert_eq!(menu.navigate(-100, 0..=29), Some(0));
        for _ in 0..4 {
            menu.update();
        }
        let target = menu.axis(1.0);
        assert!(target.is_some());
        let top = menu.selected as f32 * 60.0;
        assert!(top > menu.scroll());
        assert!(top + 60.0 < menu.scroll() + 720.0);
        menu.update(); // Direct-scroll callback owns this frame.
        assert_eq!(menu.scroll(), 100.0);
        for expected in [105.0, 110.0, 115.0, 120.0] {
            menu.update();
            assert_eq!(menu.scroll(), expected);
        }
    }
    #[test]
    fn idle_settling_uses_integer_remainder_and_rounds_up_at_thirty() {
        for (position, target) in [
            (29.0, 0.0),
            (29.9, 0.0),
            (30.0, 60.0),
            (30.1, 60.0),
            (59.9, 60.0),
            (60.1, 60.1),
        ] {
            let mut menu = SceneJump::new(&history(30, true), 0, 0).unwrap();
            let _ = menu.axis(position / 100.0);
            menu.update();
            for _ in 0..4 {
                menu.update();
            }
            assert!(
                (menu.scroll() - target).abs() < 0.0001,
                "position{position}"
            );
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_scene_jump_markers_sort_selection_scroll_and_labels() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, expected) in [
            (0xa8664, 0x320003e9u32),
            (0xa8668, 0x29006509),
            (0xa877c, 0x6b0a033f),
            (0xa8790, 0x929ff388),
            (0xa87ac, 0x929ff3c8),
            (0xa8960, 0xb9419d13),
            (0xa89ec, 0x929ff3aa),
            (0xa8a6c, 0x51000513),
            (0xa8a7c, 0x929ff368),
            (0xa8b98, 0x321f016b),
            (0xa8b9c, 0x71018d7f),
            (0xa8bb8, 0xb901230a),
            (0xa8bcc, 0x510b4108),
            (0xa8bd4, 0x5105294a),
            (0xa8c04, 0x1b097d08),
            (0xadd00, 0x6b0a013f),
            (0xadd10, 0x6b0b011f),
            (0xa94e8, 0x321e0fe8),
            (0xa9504, 0x540000a5),
            (0xa9508, 0x5100f108),
            (0xa9528, 0x5400018d),
            (0xa9544, 0x51096108),
            (0xa9558, 0x1e221001),
            (0xa9584, 0xb901a128),
            (0xa9324, 0x1e2e1000),
            (0xa932c, 0x940049c9),
            // Both Switch input paths map physical bits2/3 into action6/7.
            (0xc4478, 0x53027d8e),
            (0xc448c, 0x331a01cd),
            (0xc4490, 0x1219020e),
            (0xc4574, 0x53027d8e),
            (0xc4588, 0x331a01cd),
            (0xc458c, 0x1219020e),
            // Browser priority: cancel, marker menu, voice replay, direct jump.
            (0xa6b6c, 0x37e809a8),
            (0xa6b70, 0x37300da8),
            (0xa6b74, 0x37e01048),
            (0xa6b78, 0x373810c8),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                expected,
                "native scene jump {a:x}"
            );
        }
        for (address, kind) in [
            (0x1621a3, Kind::Beginning),
            (0x161045, Kind::Cursor),
            (0x1609ee, Kind::Current),
            (0x15cd6e, Kind::CursorAndCurrent),
            (0x160a07, Kind::End),
        ] {
            let end = exe[address + 256..].iter().position(|&b| b == 0).unwrap();
            assert_eq!(
                std::str::from_utf8(&exe[address + 256..address + 256 + end]).unwrap(),
                kind.label().unwrap()
            );
        }
    }
    #[test]
    fn wheel_scroll_is_bounded_and_empty_or_invalid_histories_are_recoverable() {
        assert!(SceneJump::new(&Backlog::default(), 0, 0).is_err());
        assert!(SceneJump::new(&history(3, false), 3, 0).is_err());
        assert!(SceneJump::new(&history(3, false), 0, 3).is_err());
        let mut menu = SceneJump::new(&history(30, true), 0, 0).unwrap();
        let _ = menu.axis(1.0);
        assert_eq!(menu.scroll(), 100.0);
        let _ = menu.axis(f32::NAN);
        assert_eq!(menu.scroll(), 100.0);
        let _ = menu.axis(100.0);
        assert_eq!(menu.scroll(), menu.max_scroll());
        let _ = menu.axis(-100.0);
        assert_eq!(menu.scroll(), 0.0);
    }
}
