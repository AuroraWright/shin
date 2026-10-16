//! Captured browser presses; a7730/a7940 commit on release, a7f30 cancels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Close,
    Row(usize),
}
#[derive(Debug, Default)]
pub(crate) struct BacklogPointer {
    held: bool,
    captured: Option<Target>,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    Press(Target),
    Release(Target),
    Cancel(Target),
}
impl BacklogPointer {
    pub fn cancel_capture(&mut self) -> Option<Event> {
        self.captured.take().map(Event::Cancel)
    }

    /// Ignore held buttons during opening: restoring eligibility is not a
    /// fresh press. A canceled gesture stays canceled until the next press.
    pub fn update(&mut self, held: bool, eligible: bool, hit: Option<Target>) -> Option<Event> {
        let pressed = held && !self.held;
        let released = !held && self.held;
        self.held = held;
        if !eligible {
            return self.captured.take().map(Event::Cancel);
        }
        if pressed {
            self.captured = hit;
            return hit.map(Event::Press);
        }
        if let Some(target) = self.captured {
            if hit != Some(target) {
                self.captured = None;
                return Some(Event::Cancel(target));
            }
            if released {
                self.captured = None;
                return Some(Event::Release(target));
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_commits_captured_target_and_passive_hover_does_nothing() {
        let mut pointer = BacklogPointer::default();
        assert_eq!(pointer.update(false, true, Some(Target::Close)), None);
        assert_eq!(
            pointer.update(true, true, Some(Target::Close)),
            Some(Event::Press(Target::Close))
        );
        assert_eq!(pointer.update(true, true, Some(Target::Close)), None);
        assert_eq!(
            pointer.update(false, true, Some(Target::Close)),
            Some(Event::Release(Target::Close))
        );
    }
    #[test]
    fn leaving_target_or_closing_cancels_without_reacquisition() {
        for hit in [None, Some(Target::Row(1)), Some(Target::Close)] {
            let mut pointer = BacklogPointer::default();
            pointer.update(true, true, Some(Target::Row(0)));
            assert_eq!(
                pointer.update(true, true, hit),
                Some(Event::Cancel(Target::Row(0)))
            );
            assert_eq!(pointer.update(true, true, Some(Target::Row(0))), None);
            assert_eq!(pointer.update(false, true, Some(Target::Row(0))), None);
        }
        let mut pointer = BacklogPointer::default();
        pointer.update(true, true, Some(Target::Close));
        assert_eq!(
            pointer.update(false, false, Some(Target::Close)),
            Some(Event::Cancel(Target::Close))
        );
    }
    #[test]
    fn held_opening_button_cannot_start_a_gesture_on_open() {
        let mut pointer = BacklogPointer::default();
        assert_eq!(pointer.update(true, false, Some(Target::Close)), None);
        assert_eq!(pointer.update(true, true, Some(Target::Close)), None);
        assert_eq!(pointer.update(false, true, Some(Target::Close)), None);
        assert_eq!(
            pointer.update(true, true, Some(Target::Close)),
            Some(Event::Press(Target::Close))
        );
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_pointer_callbacks_and_sound_cues() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Hold/cancel flash callbacks, close release and navigation/cancel calls.
        for (address, expected) in [
            (0xa7758, 0x910a4260),
            (0xa775c, 0x94004f31),
            (0xa7764, 0xbd45a500),
            (0xa776c, 0xbd497501),
            (0xa779c, 0xb901a268),
            (0xa789c, 0x1e212901),
            (0xa7970, 0xf9437108),
            (0xa797c, 0x97fdfe4d),
            (0xa7998, 0x17fffb62),
            (0xa7f4c, 0x94004d35),
            (0xa7f54, 0xbd497501),
            (0xa7f58, 0x1e2703e0),
            (0xa7f64, 0x94004ea7),
            (0xa7f68, 0xb902827f),
            (0xa7fc4, 0x94004d17),
            (0xa7fcc, 0xbd497501),
            (0xa7fd0, 0x1e2703e0),
            (0xa7fdc, 0x94004e89),
            (0xa6c54, 0xf943c108),
            (0xa6c60, 0x97fe0194),
            (0xa6ca4, 0xf943bd08),
            (0xa6cb0, 0x97fe0180),
            (0xa6cb8, 0x940046e6),
            (0xa6cc0, 0x97fffe98),
        ] {
            assert_eq!(
                word(address),
                expected,
                "native backlog pointer {address:x}"
            );
        }
        let relocation = |address: u64| {
            exe[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find(|entry| u64::from_le_bytes(entry[..8].try_into().unwrap()) == address)
                .map(|entry| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                .unwrap()
        };
        for (got, expected) in [
            (0x2246e0, "decide"),
            (0x224778, "cancel"),
            (0x224780, "move"),
        ] {
            let string = relocation(relocation(got)) as usize + 256;
            let end = exe[string..].iter().position(|&v| v == 0).unwrap() + string;
            assert_eq!(std::str::from_utf8(&exe[string..end]).unwrap(), expected);
        }
        assert_eq!(f32::from_bits(word(0x1595a4)), 0.7);
        assert_eq!(f32::from_bits(word(0x159974)), 0.2);
    }
}
