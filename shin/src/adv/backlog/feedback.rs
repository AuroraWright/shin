//! LogEntryView+38 confirmation flash; ad590 waits for the cached view to idle.
use shin_core::time::{Ticks, Tween, Tweener};
use std::{collections::BTreeMap, ops::Range};

#[derive(Default)]
pub(crate) struct RowConfirmationFeedback {
    flashes: BTreeMap<usize, Tweener>,
}
impl RowConfirmationFeedback {
    pub fn cache(&mut self, index: usize) {
        self.flashes
            .entry(index)
            .or_insert_with(|| Tweener::new(0.0));
    }
    pub fn retain(&mut self, range: Range<usize>) {
        self.flashes.retain(|&index, _| range.contains(&index));
    }
    pub fn request(&mut self, index: usize) {
        // Native a71a0 only flashes a currently cached LogEntryView. The
        // confirmation task proceeds immediately if that view is absent.
        let Some(flash) = self.flashes.get_mut(&index) else {
            return;
        };
        let current = flash.value();
        flash.fast_forward_to(current);
        flash.enqueue(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
        );
        flash.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
    }
    pub fn is_idle(&self, index: usize) -> bool {
        self.flashes.get(&index).is_none_or(Tweener::is_idle)
    }
    /// ad5bc searches the live browser cursor, not the opening feedback row.
    pub fn selected_is_idle(&self, selected: Option<usize>) -> bool {
        selected.is_none_or(|index| self.is_idle(index))
    }
    pub fn value(&self, index: usize) -> f32 {
        self.flashes.get(&index).map_or(0.0, Tweener::value)
    }
    pub fn update(&mut self, delta: Ticks) {
        for flash in self.flashes.values_mut() {
            flash.update(delta);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_waits_for_full_row_peak_and_return() {
        let mut feedback = RowConfirmationFeedback::default();
        feedback.cache(2);
        feedback.request(2);
        assert!(!feedback.is_idle(2));
        feedback.update(Ticks::from_f32(3.5));
        assert_eq!(feedback.value(2), 0.7);
        assert!(!feedback.is_idle(2));
        feedback.update(Ticks::from_f32(3.5));
        assert_eq!(feedback.value(2), 0.0);
        assert!(feedback.is_idle(2));
    }
    #[test]
    fn absent_or_discarded_view_has_no_pending_feedback() {
        let mut feedback = RowConfirmationFeedback::default();
        feedback.request(2);
        assert!(feedback.is_idle(2));
        assert_eq!(feedback.value(2), 0.0);
        feedback.cache(2);
        feedback.request(2);
        feedback.update(Ticks::from_f32(1.0));
        feedback.retain(3..5);
        assert!(feedback.is_idle(2));
        feedback.cache(2);
        assert_eq!(feedback.value(2), 0.0);
        assert!(feedback.is_idle(2));
    }
    #[test]
    fn preparing_visible_cache_retains_inflight_flash_and_other_rows_are_independent() {
        let mut feedback = RowConfirmationFeedback::default();
        feedback.cache(2);
        feedback.cache(3);
        feedback.request(2);
        feedback.update(Ticks::from_f32(1.0));
        feedback.retain(1..4);
        feedback.cache(2);
        assert_eq!(feedback.value(2), 0.2);
        assert_eq!(feedback.value(3), 0.0);
        feedback.request(3);
        feedback.update(Ticks::from_f32(1.0));
        assert_eq!(feedback.value(2), 0.4);
        assert_eq!(feedback.value(3), 0.2);
    }
    #[test]
    fn dialog_creation_gate_tracks_current_cached_selection_and_eviction() {
        let mut feedback = RowConfirmationFeedback::default();
        feedback.cache(1);
        feedback.cache(2);
        feedback.request(1);
        feedback.update(Ticks::from_f32(1.0));
        assert!(!feedback.selected_is_idle(Some(1)));
        assert!(feedback.selected_is_idle(Some(2)));
        assert!(feedback.selected_is_idle(None));
        assert!(feedback.selected_is_idle(Some(3)));
        feedback.retain(2..4);
        assert!(feedback.selected_is_idle(Some(1)));
        feedback.cache(1);
        assert!(feedback.selected_is_idle(Some(1)));
    }
    #[test]
    fn retrigger_preserves_current_value_and_native_rate() {
        let mut feedback = RowConfirmationFeedback::default();
        feedback.cache(2);
        feedback.request(2);
        feedback.update(Ticks::from_f32(5.0));
        assert!((feedback.value(2) - 0.4).abs() < 0.00001);
        feedback.request(2);
        feedback.update(Ticks::from_f32(1.5));
        assert_eq!(feedback.value(2), 0.7);
        feedback.update(Ticks::from_f32(3.5));
        assert!(feedback.is_idle(2));
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_confirmation_row_flash_and_dialog_gate() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        for (address, expected) in [
            (0xa7218, 0x9100e100),
            (0xa721c, 0x94005081),
            (0xa7224, 0xbd45a500),
            (0xa722c, 0xbd497508),
            (0xa7234, 0x321a03e1),
            (0xa723c, 0x940051f1),
            (0xa7240, 0x1e2703e0),
            (0xa7244, 0x4ea81d01),
            (0xa7250, 0x940051ec),
            (0xad5bc, 0xb941a28a),
            (0xad5c8, 0x6b0a017f),
            (0xad5e4, 0x54000100),
            (0xad5ec, 0xb40000c8),
            (0xad5f0, 0x9100e100),
            (0xad5f4, 0x940038ff),
            (0xad5f8, 0x36000060),
            (0xad5fc, 0x320003e0),
            (0xad604, 0x910003e0),
            (0xad624, 0x320003e1),
            (0xa4ed4, 0xbd4a750c),
            (0xa4f5c, 0xbd400f60),
            (0xa4f60, 0x1e213801),
            (0xa4f6c, 0x97fff431),
            (0xa4f90, 0xbc410f61),
            (0xa5254, 0x9100e340),
            (0xa5258, 0x94005892),
            (0xa525c, 0x1e202008),
            (0xa5260, 0x540002ed),
            (0xa5268, 0xbd400360),
            (0xa526c, 0x1e282803),
            (0xa5270, 0x4ea81d00),
            (0xa5274, 0x4ea81d01),
            (0xa5278, 0x4eac1d82),
            (0xa5280, 0x9400868c),
            (0xa5284, 0x4eaa1d40),
            (0xa5288, 0x4eaa1d41),
            (0xa528c, 0x4eaa1d42),
            (0xa5290, 0x4ea91d23),
            (0xa5294, 0x97fec52f),
            (0xa52a4, 0x97fec567),
            (0xa52b0, 0x940086a0),
            (0xa52b8, 0x940086a6),
            (0xc6dd8, 0x320003e0),
            (0xc6de0, 0x94002270),
            (0xc6de8, 0x321a03e1),
            (0xc6df4, 0x321e03e1),
            (0xc6dfc, 0x940022e9),
        ] {
            assert_eq!(
                word(address),
                expected,
                "native backlog row confirmation {address:x}"
            );
        }
        for (address, expected) in [
            (0x159340, 136.0f32),
            (0x159a74, 1648.0),
            (0x1595a4, 0.7),
            (0x159974, 0.2),
        ] {
            assert_eq!(f32::from_bits(word(address)), expected);
        }
    }
}
