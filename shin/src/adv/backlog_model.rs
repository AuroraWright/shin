//! Native MessageBrowser row geometry, cursor and scroll state.
use glam::Vec2;
use shin_core::{
    layout::{MessageTextParser, ParsedCommand},
    time::{Ticks, Tween, Tweener},
};

use super::backlog::BacklogItem;

pub(super) const VIEW_HEIGHT: f32 = 730.0;

pub(super) fn voice_paths(item: &BacklogItem) -> Vec<String> {
    let BacklogItem::Message { text, .. } = item else {
        return Vec::new();
    };
    MessageTextParser::new(text)
        .filter_map(|command| match command {
            ParsedCommand::Voice(path) if !path.is_empty() => Some(path),
            _ => None,
        })
        .collect()
}

/// a6a64/a6b14: shoulder buttons skip ordinary messages, then clamp to
/// the beginning/end row if there is no further authored checkpoint.
pub(super) fn checkpoint_target(
    history: &super::backlog::Backlog,
    selected: usize,
    forward: bool,
) -> Option<usize> {
    if selected >= history.entries.len() {
        return None;
    }
    let checkpoint =
        |row: &super::backlog::BacklogEntry| !matches!(row.item, BacklogItem::Message { .. });
    let target = if forward {
        history.entries[selected + 1..]
            .iter()
            .position(checkpoint)
            .map_or(history.entries.len() - 1, |i| selected + 1 + i)
    } else {
        history.entries[..selected]
            .iter()
            .rposition(checkpoint)
            .unwrap_or(0)
    };
    (target != selected).then_some(target)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    pub top: f32,
    pub height: f32,
}

#[derive(Debug)]
pub(super) struct BacklogModel {
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    scroll: Tweener,
    end: f32,
    velocity: f32,
}

impl BacklogModel {
    /// Rebase the native document's current-entry origin onto its first row.
    /// This preserves all row/viewport differences, including negative native
    /// positions of preceding rows. The producer omits the trailing14px gap.
    pub fn new(sizes: impl IntoIterator<Item = (f32, bool)>, current: usize) -> Self {
        let mut top = 0.0;
        let rows: Vec<_> = sizes
            .into_iter()
            .map(|(text_height, voiced)| {
                assert!(text_height.is_finite() && text_height >= 0.0);
                let height = if voiced {
                    text_height.max(76.0)
                } else {
                    text_height
                } + 20.0;
                let row = Row { top, height };
                top = (top + height) + 14.0;
                row
            })
            .collect();
        let end = if rows.is_empty() { 0.0 } else { top - 14.0 };
        let selected = (!rows.is_empty()).then(|| current.min(rows.len() - 1));
        // a82cc: native starts800px above the current row, then fits it.
        let initial = selected.map_or(0.0, |i| {
            let row = rows[i];
            let start = (row.top - 800.0).max(0.0);
            if row.top - start + row.height > VIEW_HEIGHT {
                (row.top + row.height - VIEW_HEIGHT).clamp(0.0, (end - VIEW_HEIGHT).max(0.0))
            } else {
                start
            }
        });
        Self {
            rows,
            selected,
            scroll: Tweener::new(initial),
            end,
            velocity: 0.0,
        }
    }

    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn max_scroll(&self) -> f32 {
        (self.end - VIEW_HEIGHT).max(0.0)
    }
    pub fn content_height(&self) -> f32 {
        self.end
    }
    pub fn accepts_keyboard(&self) -> bool {
        self.scroll.is_idle()
    }

    fn set_scroll(&mut self, position: f32) {
        self.scroll
            .fast_forward_to(position.clamp(0.0, self.max_scroll()));
    }

    /// a6ad4..a6b10: arrows move one or four rows, clamp without wrapping.
    /// Returns whether the native move cue should be played. It consumes
    /// conflicting confirm input only if the selected row actually changes.
    pub fn navigate(&mut self, delta: i32) -> bool {
        if !self.accepts_keyboard() {
            return false;
        }
        let Some(old) = self.selected else {
            return false;
        };
        let index = (old as i64 + i64::from(delta)).clamp(0, self.rows.len() as i64 - 1) as usize;
        if index == old {
            return false;
        }
        self.select(index);
        true
    }

    pub fn select(&mut self, index: usize) {
        self.selected = Some(index);
        self.velocity = 0.0;
        let row = self.rows[index];
        let current = self.scroll();
        let target = if row.top < current {
            row.top
        } else if row.top - current + row.height > VIEW_HEIGHT {
            row.top + row.height - VIEW_HEIGHT
        } else {
            current
        };
        self.scroll.fast_forward_to(current);
        self.scroll.enqueue(
            target.clamp(0.0, self.max_scroll()),
            Tween::linear(Ticks::from_f32(4.0)),
        );
    }

    /// Native hit tests truncate the scroll to an integer, exclude14px gaps,
    /// and suppress row targets while fling velocity is nonzero.
    pub fn row_at(&self, point: Vec2) -> Option<usize> {
        if self.velocity != 0.0
            || !(136.0..1784.0).contains(&point.x)
            || !(188.0..918.0).contains(&point.y)
        {
            return None;
        }
        let y = point.y - 188.0 + self.scroll().trunc();
        self.rows
            .iter()
            .position(|row| y >= row.top && y < row.top + row.height)
    }
    pub fn pointer_select(&mut self, point: Vec2) -> bool {
        let Some(index) = self.row_at(point) else {
            return false;
        };
        self.select(index);
        true
    }

    /// a6ba4: continuous axis is32px per input update, not scaled by dt.
    pub fn axis(&mut self, amount: f32) {
        if !self.accepts_keyboard() || amount == 0.0 {
            return;
        }
        self.set_scroll(self.scroll() + amount * 32.0);
        self.fit_cursor();
    }
    pub fn drag(&mut self, delta_y: f32) {
        self.set_scroll(self.scroll() - delta_y);
        self.fit_cursor();
    }
    pub fn release(&mut self, velocity_y: f32) {
        self.velocity = velocity_y;
    }

    fn fit_cursor(&mut self) {
        let Some(mut index) = self.selected else {
            return;
        };
        let mut direction = 0;
        loop {
            let row = self.rows[index];
            if row.top < self.scroll() {
                if direction == -1 || index + 1 == self.rows.len() {
                    break;
                }
                direction = 1;
                index += 1;
            } else if row.top + row.height > self.scroll() + VIEW_HEIGHT {
                if direction == 1 || index == 0 {
                    break;
                }
                direction = -1;
                index -= 1;
            } else {
                break;
            }
        }
        self.selected = Some(index);
    }

    pub fn update(&mut self, delta: Ticks) {
        // a3de0 scales the fling with dt, but a3fc0 updates its scroll tweener
        // with literal1; a long frame must not complete a four-frame tween.
        if self.velocity != 0.0 {
            let previous = self.scroll();
            self.set_scroll(previous - self.velocity * delta.as_f32());
            if (self.scroll() - previous).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.fit_cursor();
                self.velocity *= 0.95f32.powf(delta.as_f32());
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        self.scroll.update(Ticks::from_f32(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shoulder_navigation_skips_messages_includes_all_authored_checkpoint_kinds_and_clamps_edges()
    {
        use super::super::backlog::{Backlog, BacklogEntry};
        let history = Backlog {
            entries: (0..8)
                .map(|i| BacklogEntry {
                    save_position: i + 1,
                    item: match i {
                        2 => BacklogItem::Choice {
                            title: String::new(),
                            text: String::new(),
                            response: 0,
                        },
                        4 => BacklogItem::Quiz {
                            argument: 0,
                            mode: Some(0),
                            response: 0,
                        },
                        6 => BacklogItem::Checkpoint {
                            title: String::new(),
                            text: String::new(),
                        },
                        _ => BacklogItem::Message {
                            message_id: i,
                            text: String::new(),
                            messagebox_type: shin_core::vm::command::types::MessageboxType::Neutral,
                        },
                    },
                })
                .collect(),
        };
        for (selected, forward, target) in [
            (0, true, Some(2)),
            (2, true, Some(4)),
            (4, true, Some(6)),
            (6, true, Some(7)),
            (7, true, None),
            (7, false, Some(6)),
            (6, false, Some(4)),
            (4, false, Some(2)),
            (2, false, Some(0)),
            (0, false, None),
        ] {
            assert_eq!(checkpoint_target(&history, selected, forward), target);
        }
        assert_eq!(checkpoint_target(&Backlog::default(), 0, true), None);
        assert_eq!(checkpoint_target(&history, 8, false), None);
    }
    #[test]
    fn backlog_rows_include_voice_minimum_padding_and_no_trailing_gap() {
        let model = BacklogModel::new([(50.0, false), (50.0, true), (120.0, true)], 0);
        assert_eq!(
            model
                .rows
                .iter()
                .map(|r| (r.top, r.height))
                .collect::<Vec<_>>(),
            [(0.0, 70.0), (84.0, 96.0), (194.0, 140.0)]
        );
        assert_eq!(model.end, 334.0);
        assert_eq!(model.max_scroll(), 0.0);
        assert_eq!(model.row_at(glam::vec2(136.0, 258.0)), None);
        assert_eq!(model.row_at(glam::vec2(136.0, 272.0)), Some(1));
        assert_eq!(model.row_at(glam::vec2(1784.0, 272.0)), None);
    }

    #[test]
    fn backlog_navigation_clamps_and_tween_uses_four_frames_not_elapsed_ticks() {
        let mut model = BacklogModel::new([(100.0, false); 12], 0);
        assert_eq!(model.scroll(), 0.0);
        assert!(!model.navigate(-1));
        assert!(model.navigate(4));
        assert_eq!(model.selected, Some(4));
        assert!(!model.navigate(1));
        for _ in 0..4 {
            model.update(Ticks::from_f32(100.0));
        }
        assert!(model.accepts_keyboard());
        assert!(model.navigate(4));
        let target = model.rows[8].top + model.rows[8].height - 730.0;
        model.update(Ticks::from_f32(100.0));
        assert_eq!(model.scroll(), target * 0.25);
        for _ in 0..3 {
            model.update(Ticks::ZERO);
        }
        assert_eq!(model.scroll(), target);
        assert!(model.navigate(100));
        for _ in 0..4 {
            model.update(Ticks::ZERO);
        }
        assert_eq!(model.selected, Some(11));
        assert_eq!(model.scroll(), model.max_scroll());
        assert!(!model.navigate(1));
    }

    #[test]
    fn backlog_axis_drag_fling_remains_bounded_with_tall_rows() {
        let mut model = BacklogModel::new([(1000.0, false), (100.0, false), (1000.0, false)], 0);
        model.axis(1.0);
        assert_eq!(model.scroll(), 322.0);
        model.drag(50.0);
        assert_eq!(model.scroll(), 272.0);
        model.release(-20.0);
        assert_eq!(model.row_at(glam::vec2(200.0, 300.0)), None);
        model.update(Ticks::from_f32(2.0));
        assert_eq!(model.scroll(), 312.0);
        assert!((model.velocity + 18.05).abs() < 0.001);
        for _ in 0..5 {
            model.drag(-10000.0);
            model.drag(10000.0);
        }
        assert!(model.selected.unwrap() < 3);
        let empty = BacklogModel::new([], 0);
        assert_eq!(empty.selected, None);
    }
    #[test]
    fn trackpad_wheel_scrolls_backlog_with_fractional_and_large_displacements() {
        let mut model = BacklogModel::new([(1000.0, false), (100.0, false), (1000.0, false)], 0);
        model.set_scroll(0.0);
        for (amount, expected) in [
            (-3.25, 104.0),
            (-0.125, 108.0),
            (3.25, 4.0),
            (0.0, 4.0),
            (100.0, 0.0),
        ] {
            let (_, scroll) = crate::app::wheel_scroll_test_input(amount);
            model.axis(scroll);
            assert_eq!(model.scroll(), expected);
        }
    }

    #[test]
    fn inertia_stops_when_clamped_motion_or_zero_delta_has_no_displacement() {
        let mut model = BacklogModel::new([(1000.0, false)], 0);
        model.drag(10000.0);
        model.release(20.0);
        model.update(Ticks::from_f32(1.0));
        assert_eq!(model.velocity, 0.0);
        model.release(-20.0);
        model.update(Ticks::ZERO);
        assert_eq!(model.velocity, 0.0);
        model.release(-20.0);
        model.drag(-10.0);
        assert_eq!(
            model.velocity, -20.0,
            "Native drag does not clear the fling field"
        );
        model.update(Ticks::from_f32(1.0));
        assert_eq!(model.scroll(), 30.0);
    }

    #[test]
    fn only_nonempty_message_voice_tags_reserve_voice_row_space() {
        use shin_core::vm::command::types::MessageboxType;
        let message = BacklogItem::Message {
            message_id: 1,
            text: "名前@r@vone|two.@v.@vthree.本文".into(),
            messagebox_type: MessageboxType::Neutral,
        };
        assert_eq!(voice_paths(&message), ["one|two", "three"]);
        let choice = BacklogItem::Choice {
            title: String::new(),
            text: "@vone.答え".into(),
            response: 0,
        };
        assert!(voice_paths(&choice).is_empty());
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_rows_navigation_and_scroll() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xa2ae8, 0x1e281400u32),
            (0xa2aec, 0x1e204d00u32),
            (0xa2b14, 0x1e292800u32),
            (0xa2b1c, 0xbd000380u32),
            (0xa2b40, 0x1e2b2800u32),
            (0xa2ba0, 0x9b1456c8u32),
            (0xa2be4, 0x1e359001u32),
            (0xa2be8, 0x1e212800u32),
            (0xa6ad4, 0x51000718u32),
            (0xa6ae4, 0x11000718u32),
            (0xa6af4, 0x51001318u32),
            (0xa6b04, 0x11001318u32),
            (0xa6ba4, 0x1e220821u32),
            (0xa6d0c, 0x1e221001u32),
            (0xa6d1c, 0x94005339u32),
            (0xa7838, 0x1e380008u32),
            (0xac3bc, 0xb40004a9u32),
            (0xac3c8, 0x39000509u32),
            (0xa3fc0, 0xaa1503e0u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native backlog row/scroll {address:x}"
            );
        }
        for (address, expected) in [
            (0x159904, 76.0f32),
            (0x159618, 730.0),
            (0x159694, 32.0),
            (0x1595d8, 0.95),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }
}
