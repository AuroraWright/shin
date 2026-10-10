use glam::Vec2;
use shin_core::time::{Easing, Ticks, Tween, Tweener};

pub(super) const WIDTH: f32 = 1200.0;
pub(super) const GAP: f32 = 32.0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Initial,
    Opening,
    Waiting,
    Flash,
    Closing,
    Done,
}
#[derive(Debug)]
pub(super) struct RowMotion {
    pub x: Tweener,
    pub alpha: Tweener,
    pub focus: Tweener,
    pub flash: Tweener,
}
impl RowMotion {
    fn new(focused: bool) -> Self {
        Self {
            x: Tweener::new(-300.0),
            alpha: Tweener::new(0.0),
            focus: Tweener::new(if focused { 1.0 } else { 0.0 }),
            flash: Tweener::new(0.0),
        }
    }
    fn idle(&self) -> bool {
        self.x.is_idle() && self.alpha.is_idle() && self.focus.is_idle() && self.flash.is_idle()
    }
    fn update(&mut self, delta: Ticks) {
        self.x.update(delta);
        self.alpha.update(delta);
        self.focus.update(delta);
        self.flash.update(delta);
    }
}
/// Original script indexes survive filtering and delayed completion (nativef0ac0).
#[derive(Debug)]
pub struct ChoiceModel {
    pub indices: Vec<usize>,
    pub selected: Option<usize>,
    pub result: Option<i32>,
    pub(super) phase: Phase,
    pub(super) rows: Vec<RowMotion>,
    pub(super) heights: Vec<f32>,
    silent_open: bool,
    sounds: Vec<&'static str>,
    pointer_held: bool,
    pointer_row: Option<usize>,
}
impl ChoiceModel {
    pub fn new(count: usize, mask: i32) -> Self {
        let seed = (mask as u32 & 0x1000 != 0).then(crate::engine_random::next_seed);
        Self::with_shuffle_seed(count, mask, seed)
    }

    fn with_shuffle_seed(count: usize, mask: i32, seed: Option<u32>) -> Self {
        let mut indices: Vec<_> = (0..count.min(12))
            .filter(|&i| mask as u32 & (1 << i) != 0)
            .collect();
        if let Some(seed) = seed {
            shuffle(&mut indices, seed);
        }
        let silent_open = mask as u32 & 0x8000 != 0;
        let selected = (silent_open && !indices.is_empty()).then_some(0);
        let rows = (0..indices.len())
            .map(|i| RowMotion::new(selected == Some(i)))
            .collect();
        let heights = vec![70.0; indices.len()];
        Self {
            indices,
            selected,
            result: None,
            phase: Phase::Initial,
            rows,
            heights,
            silent_open,
            sounds: Vec::new(),
            pointer_held: false,
            pointer_row: None,
        }
    }
    pub(super) fn select(&mut self, selected: Option<usize>) {
        self.select_with_sound(selected, true);
    }

    fn select_with_sound(&mut self, selected: Option<usize>, sound: bool) {
        if self.phase != Phase::Waiting || self.selected == selected {
            return;
        }
        if let Some(old) = self.selected {
            let row = &mut self.rows[old];
            row.focus
                .enqueue_now(0.0, Tween::linear(Ticks::from_f32(row.focus.value() / 0.1)));
        }
        if let Some(new) = selected {
            let row = &mut self.rows[new];
            row.focus.enqueue_now(
                1.0,
                Tween::linear(Ticks::from_f32((1.0 - row.focus.value()) / 0.25)),
            );
        }
        self.selected = selected;
        if sound {
            self.sounds.push("move");
        }
    }
    pub fn move_selection(&mut self, delta: i32) {
        if self.phase == Phase::Waiting && !self.indices.is_empty() {
            let next = match self.selected {
                Some(i) => (i as i32 + delta).rem_euclid(self.indices.len() as i32) as usize,
                None if delta < 0 => self.indices.len() - 1,
                None => 0,
            };
            self.select(Some(next));
        }
    }
    pub fn confirm(&mut self) {
        if self.phase != Phase::Waiting {
            return;
        }
        let Some(selected) = self.selected else {
            self.sounds.push("error");
            return;
        };
        let flash = &mut self.rows[selected].flash;
        flash.enqueue_now(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
        self.phase = Phase::Flash;
        self.sounds.push("seldecide");
    }
    pub(super) fn input(
        &mut self,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        vertical_held: bool,
        confirm: bool,
    ) {
        if self.phase != Phase::Waiting {
            return;
        }
        let next = if self.indices.is_empty() {
            None
        } else if up {
            Some(match self.selected {
                Some(i) if i > 0 => i - 1,
                _ => self.indices.len() - 1,
            })
        } else if down {
            Some(self.selected.map_or(0, |i| (i + 1) % self.indices.len()))
        } else if left && !vertical_held {
            Some(0)
        } else if right && !vertical_held {
            Some(self.indices.len() - 1)
        } else {
            self.selected
        };
        // Native navigation consumes the frame only if focus actually changes.
        if next != self.selected {
            self.select(next);
        } else if confirm {
            self.confirm();
        }
    }

    pub(super) fn pointer_state(&mut self, position: Vec2, held: bool) {
        if self.phase != Phase::Waiting {
            self.pointer_held = held;
            self.pointer_row = None;
            return;
        }
        let hit = self.row_at(position);
        if held && !self.pointer_held {
            self.pointer_row = hit;
            if let Some(row) = hit {
                self.select_with_sound(Some(row), false);
                let flash = &mut self.rows[row].flash;
                flash.enqueue_now(
                    0.7,
                    Tween::linear(Ticks::from_f32((0.7 - flash.value()).abs() / 0.2)),
                );
            }
        } else if !held && self.pointer_held {
            if let Some(pressed) = self.pointer_row.take() {
                if let Some(row) = hit {
                    self.select_with_sound(Some(row), false);
                    self.confirm();
                } else {
                    self.cancel_pointer_flash(pressed);
                }
            }
        } else if held && hit.is_none() {
            if let Some(row) = self.pointer_row.take() {
                self.cancel_pointer_flash(row);
            }
        }
        self.pointer_held = held;
    }

    fn cancel_pointer_flash(&mut self, row: usize) {
        let flash = &mut self.rows[row].flash;
        flash.enqueue_now(0.0, Tween::linear(Ticks::from_f32(flash.value() / 0.2)));
    }
    pub(super) fn cancel_pointer(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            self.cancel_pointer_flash(row);
        }
        self.pointer_held = false;
    }
    pub(super) fn ignore_pointer_state(&mut self, held: bool) {
        self.cancel_pointer();
        // Preserve the physical edge while a modal/fade consumes input, so a
        // still-held restore click cannot become a fresh press afterward.
        self.pointer_held = held;
    }
    pub(super) fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub(crate) fn update(&mut self, delta: Ticks) {
        // Native state transitions inspect tween busy flags before updating them.
        // Frame overshoot is not carried into a new state (f08f8/f0954/f0a68).
        match self.phase {
            Phase::Initial => {
                for (i, row) in self.rows.iter_mut().enumerate() {
                    let delay = Tween::linear(Ticks::from_f32(i as f32 * 4.0));
                    row.x.enqueue(-300.0, delay);
                    row.x.enqueue(
                        0.0,
                        Tween {
                            duration: Ticks::from_f32(20.0),
                            easing: Easing::SineOut,
                        },
                    );
                    row.alpha.enqueue(0.0, delay);
                    row.alpha.enqueue(1.0, Tween::linear(Ticks::from_f32(20.0)));
                }
                if !self.silent_open {
                    self.sounds.push("select");
                }
                self.phase = Phase::Opening;
                if self.rows.iter().all(RowMotion::idle) {
                    self.phase = Phase::Waiting;
                }
            }
            Phase::Opening if self.rows.iter().all(RowMotion::idle) => self.phase = Phase::Waiting,
            Phase::Flash if self.rows[self.selected.unwrap()].flash.is_idle() => {
                let selected = self.selected.unwrap();
                for (i, row) in self.rows.iter_mut().enumerate() {
                    if i == selected {
                        continue;
                    }
                    let delay = Tween::linear(Ticks::from_f32(i as f32 * 4.0));
                    row.x.enqueue(0.0, delay);
                    row.x.enqueue(
                        300.0,
                        Tween {
                            duration: Ticks::from_f32(20.0),
                            easing: Easing::SineIn,
                        },
                    );
                    row.alpha.enqueue(1.0, delay);
                    row.alpha.enqueue(0.0, Tween::linear(Ticks::from_f32(20.0)));
                }
                let delay = Tween::linear(Ticks::from_f32(self.rows.len() as f32 * 4.0 + 20.0));
                let row = &mut self.rows[selected];
                row.alpha.enqueue(1.0, delay);
                row.alpha.enqueue(0.0, Tween::linear(Ticks::from_f32(30.0)));
                self.phase = Phase::Closing;
            }
            Phase::Closing if self.rows.iter().all(RowMotion::idle) => {
                self.phase = Phase::Done;
                self.result = Some(self.indices[self.selected.unwrap()] as i32);
            }
            _ => {}
        }
        for row in &mut self.rows {
            row.update(delta);
        }
    }
    pub(super) fn row_at(&self, position: Vec2) -> Option<usize> {
        if position.x < 460.0 || position.x >= 1460.0 {
            return None;
        }
        let mut y = self.top();
        for (i, &height) in self.heights.iter().enumerate() {
            if position.y >= y && position.y < y + height {
                return Some(i);
            }
            y += height + GAP;
        }
        None
    }
    pub(super) fn top(&self) -> f32 {
        let height =
            self.heights.iter().sum::<f32>() + GAP * self.heights.len().saturating_sub(1) as f32;
        (1080.0 - height) * 0.35
    }
}

fn shuffle(indices: &mut [usize], seed: u32) {
    // f00d8 draws exactly one global seed even for an empty/single-row menu.
    // f013c uses libc++ uniform_int_distribution<long> with MINSTD, then swaps
    // forward through the remaining suffix. Preserve original script indexes.
    let mut state = (seed % 2_147_483_647).max(1);
    for i in 0..indices.len().saturating_sub(1) {
        let range = (indices.len() - i) as u32;
        let bits = u32::BITS - (range - 1).leading_zeros();
        let mask = (1 << bits) - 1;
        let limit = 0x7fff_fffe & !mask;
        let offset = loop {
            state = (u64::from(state) * 48_271 % 2_147_483_647) as u32;
            let value = state - 1;
            if value < limit && (value & mask) < range {
                break (value & mask) as usize;
            }
        };
        indices.swap(i, i + offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_input_priority_horizontal_endpoints_and_pointer_gestures() {
        let mut model = ChoiceModel::new(7, 0b1010010);
        ready(&mut model);
        model.take_sounds();
        // Up wins over Down and confirmation, wrapping from no selection.
        model.input(true, true, false, false, true, true);
        assert_eq!(model.selected, Some(2));
        assert_eq!(model.phase, Phase::Waiting);
        assert_eq!(model.take_sounds(), ["move"]);
        // Horizontal input is suppressed while a vertical direction is held.
        model.input(false, false, true, false, true, false);
        assert_eq!(model.selected, Some(2));
        assert!(model.take_sounds().is_empty());
        model.input(false, false, true, true, false, false);
        assert_eq!(model.selected, Some(0));
        model.input(false, false, false, true, false, true);
        assert_eq!(model.selected, Some(2));
        assert_eq!(model.phase, Phase::Waiting);
        model.input(false, false, false, true, false, true);
        assert_eq!(model.phase, Phase::Flash);
        finish(&mut model);
        assert_eq!(model.result, Some(6));

        let mut model = ChoiceModel::new(7, 0b1010010);
        ready(&mut model);
        model.take_sounds();
        let position = Vec2::new(960.0, model.top() + 20.0);
        model.pointer_state(position, false);
        assert_eq!(model.selected, None, "Hover does not change selected focus");
        model.pointer_state(position, true);
        assert_eq!(model.selected, Some(0));
        assert_eq!(model.phase, Phase::Waiting, "Press only starts flash");
        model.update(Ticks::from_f32(3.5));
        assert_eq!(model.rows[0].flash.value(), 0.7);
        assert!(
            model.take_sounds().is_empty(),
            "Press/hover have no move cue"
        );
        model.pointer_state(Vec2::ZERO, true);
        model.update(Ticks::from_f32(3.5));
        assert_eq!(model.rows[0].flash.value(), 0.0);
        model.pointer_state(position, false);
        assert_eq!(
            model.phase,
            Phase::Waiting,
            "Cancelled press cannot confirm"
        );
        model.pointer_state(position, true);
        model.update(Ticks::from_f32(1.0));
        model.cancel_pointer();
        model.pointer_state(position, false);
        model.update(Ticks::from_f32(1.0));
        assert_eq!(
            model.phase,
            Phase::Waiting,
            "Modal cancellation consumes release"
        );
        assert_eq!(model.rows[0].flash.value(), 0.0);
        model.ignore_pointer_state(true);
        model.pointer_state(position, true);
        model.pointer_state(position, false);
        assert_eq!(
            model.phase,
            Phase::Waiting,
            "Held restore click stays consumed"
        );
        assert_eq!(model.rows[0].flash.value(), 0.0);
        model.pointer_state(position, true);
        model.update(Ticks::from_f32(1.0));
        model.pointer_state(position, false);
        assert_eq!(model.phase, Phase::Flash);
        assert_eq!(model.take_sounds(), ["seldecide"]);
        finish(&mut model);
        assert_eq!(model.result, Some(1));
    }

    #[test]
    fn native_pointer_hits_exclude_decoration_and_gaps() {
        let model = ChoiceModel::new(2, 3);
        let y = model.top();
        assert_eq!(model.row_at(Vec2::new(459.99, y)), None);
        assert_eq!(model.row_at(Vec2::new(460.0, y)), Some(0));
        assert_eq!(model.row_at(Vec2::new(1459.99, y)), Some(0));
        assert_eq!(model.row_at(Vec2::new(1460.0, y)), None);
        assert_eq!(model.row_at(Vec2::new(960.0, y - 0.01)), None);
        assert_eq!(model.row_at(Vec2::new(960.0, y + 69.99)), Some(0));
        assert_eq!(model.row_at(Vec2::new(960.0, y + 70.0)), None);
        assert_eq!(model.row_at(Vec2::new(960.0, y + 102.0)), Some(1));
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_select_input_and_pointer_callbacks() {
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
            (0xf1054, 0x37c00109),
            (0xf1058, 0x37c80169),
            (0xf105c, 0x37d00ca9),
            (0xf1064, 0x39402fe9),
            (0xf1078, 0x540005cd),
            (0xf1098, 0x1a88a7f4),
            (0xf1144, 0x54fffb00),
            (0xf11f8, 0x1a8803f4),
            (0xf1224, 0x1e222000),
            (0xf1238, 0x540001e5),
            (0xf1254, 0x540000a8),
            (0xf1264, 0x5400010c),
            (0xf1288, 0xb9008408),
            (0xf12c4, 0x37f804f4),
            (0xf1330, 0xb9408674),
            (0xf135c, 0x97ff29a9),
            (0xf1374, 0x37f807e8),
            (0xf145c, 0xb9008268),
            (0xf14c4, 0xb9008668),
            (0x15744, 0xf9405108),
            (0x15994, 0xf9405508),
            (0x15d3c, 0xf9405908),
        ] {
            assert_eq!(word(a), expected, "native SELECT input {a:x}");
        }
        assert_eq!(word(0x159688), 460.0f32.to_bits());
        assert_eq!(word(0x1598a8), 1460.0f32.to_bits());
    }
    #[test]
    fn native_shuffle_matches_libcpp_and_returns_original_filtered_index() {
        let mask = 0x1000 | (1 << 1) | (1 << 4) | (1 << 6) | (1 << 8) | (1 << 11);
        // Independent libc++ minstd_rand + uniform_int_distribution<long>
        // outputs, including zero/modulus seed normalization and rejection.
        for (seed, expected) in [
            (0, [4, 6, 1, 8, 11]),
            (1, [4, 6, 1, 8, 11]),
            (42, [11, 8, 4, 6, 1]),
            (2_147_483_646, [11, 4, 8, 1, 6]),
            (2_147_483_647, [4, 6, 1, 8, 11]),
            (3_499_211_612, [6, 4, 11, 1, 8]),
            (u32::MAX, [4, 6, 1, 8, 11]),
        ] {
            let mut model = ChoiceModel::with_shuffle_seed(15, mask, Some(seed));
            assert_eq!(model.indices, expected, "seed {seed}");
            assert_eq!(model.selected, None);
            ready(&mut model);
            model.move_selection(1);
            model.confirm();
            finish(&mut model);
            assert_eq!(model.result, Some(expected[0] as i32));
        }
        let mut preselected = ChoiceModel::with_shuffle_seed(12, mask | 0x8000, Some(42));
        assert_eq!(preselected.selected, Some(0));
        ready(&mut preselected);
        assert!(preselected.take_sounds().is_empty());
        preselected.confirm();
        finish(&mut preselected);
        assert_eq!(preselected.result, Some(11));
        for count in 0..=1 {
            let model = ChoiceModel::with_shuffle_seed(count, 0x9001, Some(42));
            assert_eq!(model.indices.len(), count);
            assert_eq!(model.selected, (count != 0).then_some(0));
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_select_shuffle_and_global_random_stream() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xf00d0, 0x366004d4),
            (0xf00d8, 0x94002e7a),
            (0xf00fc, 0x1a9f1508),
            (0xf0108, 0xf100251f),
            (0xf0128, 0xd1000516),
            (0xf013c, 0x94000541),
            (0xf0150, 0xf900028a),
            (0xf0158, 0x91002294),
            (0xf0160, 0xeb15029f),
            (0xf1654, 0xdac01109),
            (0xf1678, 0xcb0a012c),
            (0xf184c, 0xaa1f03e5),
            (0xf1874, 0x510004d3),
            (0xf187c, 0x54fffec9),
            (0xf1884, 0x0a0e0273),
            (0xf194c, 0xeb0800bf),
            (0xf1950, 0x54fff762),
            (0xf8ee4, 0x5282ae2a),
            (0xf8ef0, 0xb903b34a),
            (0xf8ef8, 0x4a4a794a),
            (0xf8f20, 0x1b187d4a),
            (0xf8f30, 0xf109b91f),
            (0xf8f50, 0xf906b91f),
            (0xfbae8, 0x9100052a),
            (0xfbb14, 0x91063529),
            (0xfbb28, 0x2a0e020e),
            (0xfbb4c, 0x1b0d7d8c),
            (0xfbb54, 0x4a4e0529),
            (0xfbb64, 0xf904e10a),
            (0xfbb70, 0x4a492d29),
            (0xfbb74, 0x0a091d08),
            (0xfbb84, 0x4a080128),
            (0xfbb8c, 0x4a484914),
            (0x35c50, 0x9403179c),
            (0x75efc, 0x940216f1),
            (0xc9a94, 0x9400c80b),
            (0x6ba34, 0x94024023),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native shuffle/random {address:x}"
            );
        }
    }

    fn ready(model: &mut ChoiceModel) {
        model.update(Ticks::from_f32(100.0));
        model.update(Ticks::ZERO);
    }
    fn finish(model: &mut ChoiceModel) {
        model.update(Ticks::from_f32(7.0));
        model.update(Ticks::ZERO);
        model.update(Ticks::from_f32(100.0));
        model.update(Ticks::ZERO);
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_select_animation_geometry_and_completion() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        let relocations: std::collections::BTreeMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        for (got, cell, name) in [
            (0x225570, 0x1d3950, "select"),
            (0x225578, 0x1d3958, "seldecide"),
            (0x224780, 0x1d3938, "move"),
            (0x2246e8, 0x1d3940, "error"),
        ] {
            assert_eq!(relocations.get(&got), Some(&cell));
            let start = relocations[&cell] as usize + 256;
            assert_eq!(&exe[start..start + name.len()], name.as_bytes());
        }
        for (a, v) in [
            (0x159ae4, -300.0f32),
            (0x159730, 300.0),
            (0x159694, 32.0),
            (0x15942c, 0.35),
            (0x159588, 360.0),
            (0x159a80, 1560.0),
            (0x159974, 0.2),
            (0x1595a4, 0.7),
        ] {
            assert_eq!(word(a), v.to_bits(), "native SELECT constant {a:x}");
        }
        for (a, v) in [
            (0xefa90, 0xf2e84809),
            (0xefaa0, 0xa935a3a9),
            (0xefbbc, 0x4eaa1d40),
            (0xefbd4, 0x94004557),
            (0xefbf4, 0x4ead1da0),
            (0xefc38, 0xa94026a8),
            (0xf017c, 0xbd46950a),
            (0xf0184, 0x1e261009),
            (0xf0194, 0x1e292820),
            (0xf01dc, 0x1e220821),
            (0xf0268, 0x3941e668),
            (0xf026c, 0x37380088),
            (0xf0274, 0xb9007e68),
            (0xf0280, 0xb9007e7f),
            (0xf084c, 0x1e26900a),
            (0xf0854, 0x1e22100d),
            (0xf0864, 0x321903e1),
            (0xf0880, 0x321f03e1),
            (0xf08ac, 0x2a1f03e1),
            (0xf0904, 0x37000ee0),
            (0xf0954, 0xb9807e68),
            (0xf0980, 0xbd47310c),
            (0xf09c4, 0x320003e1),
            (0xf0a0c, 0x1e2a2961),
            (0xf0a3c, 0x1e27d001),
            (0xf0ac0, 0xb9408d08),
            (0xf0adc, 0xd63f0100),
            (0xf1030, 0xbd405400),
            (0xf1040, 0x54000ccb),
            (0xf10f0, 0xbd45a500),
            (0xf1100, 0x321a03e1),
            (0xf1114, 0x321a03e1),
            (0xf1128, 0xb9008268),
            (0xf1184, 0x1e2703e0),
            (0xf11ac, 0x1e2a1001),
            (0xf0df0, 0x1e2e080f),
            (0xf0f18, 0x1e2f0983),
            (0xf0f48, 0xbd4096cc),
            (0xf0d08, 0xbd488900),
            (0xf0d10, 0xbd488d01),
            (0xf0bdc, 0x1e341001),
            (0xf0c3c, 0xbd457d0e),
        ] {
            assert_eq!(word(a), v, "native SELECT animation {a:x}");
        }
    }

    #[test]
    fn native_staggered_open_no_default_selection_and_delayed_result() {
        let mut model = ChoiceModel::new(7, 0b1010010);
        model.confirm();
        assert_eq!(model.phase, Phase::Initial);
        model.update(Ticks::from_f32(10.0));
        assert_eq!(model.selected, None);
        for (i, row) in model.rows.iter().enumerate() {
            let progress = (10.0 - i as f32 * 4.0) / 20.0;
            assert!((row.alpha.value() - progress).abs() < 0.00001);
            assert!(
                (row.x.value() + 300.0 * (1.0 - (progress * std::f32::consts::FRAC_PI_2).sin()))
                    .abs()
                    < 0.0001
            );
        }
        model.move_selection(1);
        assert_eq!(model.selected, None);
        model.update(Ticks::from_f32(18.0));
        assert_eq!(model.phase, Phase::Opening);
        model.update(Ticks::ZERO);
        assert_eq!(model.phase, Phase::Waiting);
        model.confirm();
        assert_eq!(model.take_sounds(), ["select", "error"]);
        model.move_selection(-1);
        assert_eq!(model.selected, Some(2));
        model.confirm();
        assert_eq!(model.result, None);
        model.update(Ticks::from_f32(3.5));
        assert!((model.rows[2].flash.value() - 0.7).abs() < 0.00001);
        model.update(Ticks::from_f32(3.5));
        assert_eq!(model.phase, Phase::Flash);
        model.update(Ticks::ZERO);
        assert_eq!(model.phase, Phase::Closing);
        model.update(Ticks::from_f32(32.0));
        assert_eq!(model.rows[2].alpha.value(), 1.0);
        assert!(model.rows[0].x.is_idle() && model.rows[1].alpha.value() == 0.0);
        model.update(Ticks::from_f32(15.0));
        assert!((model.rows[2].alpha.value() - 0.5).abs() < 0.00001);
        assert_eq!(model.result, None);
        model.update(Ticks::from_f32(15.0));
        assert_eq!(model.result, None);
        model.update(Ticks::ZERO);
        assert_eq!(model.result, Some(6));
        model.move_selection(1);
        model.confirm();
        assert_eq!(model.result, Some(6));
    }
    #[test]
    fn native_preselection_focus_retarget_and_row_geometry() {
        let mut model = ChoiceModel::new(2, 0x8003);
        assert_eq!(model.selected, Some(0));
        ready(&mut model);
        assert!(model.take_sounds().is_empty());
        model.move_selection(1);
        model.update(Ticks::from_f32(2.0));
        assert!((model.rows[0].focus.value() - 0.8).abs() < 0.00001);
        assert!((model.rows[1].focus.value() - 0.5).abs() < 0.00001);
        model.move_selection(-1);
        model.update(Ticks::from_f32(0.8));
        assert_eq!(model.rows[0].focus.value(), 1.0);
        assert!((model.rows[1].focus.value() - 0.42).abs() < 0.00001);
        assert!((model.top() - 317.8).abs() < 0.0001);
        assert_eq!(model.row_at(Vec2::new(960.0, 330.0)), Some(0));
        assert_eq!(model.row_at(Vec2::new(960.0, 405.0)), None);
        assert_eq!(model.row_at(Vec2::new(960.0, 430.0)), Some(1));
        model.confirm();
        finish(&mut model);
        assert_eq!(model.result, Some(0));
    }
}
