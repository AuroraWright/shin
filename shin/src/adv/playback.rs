/// User playback modes are reset by SYSCALL 0 (original ADV word 0x47c).
#[derive(Debug, Default)]
pub struct PlaybackModes {
    pub skip: bool,
    pub autoplay: bool,
}

impl PlaybackModes {
    pub fn reset(&mut self) {
        self.skip = false;
        self.autoplay = false;
    }

    pub fn is_skipping(&self, controls_allowed: bool, held: bool) -> bool {
        controls_allowed && (self.skip || held)
    }

    pub fn toggle_skip(&mut self) {
        self.skip = !self.skip;
    }

    pub fn toggle_autoplay(&mut self) {
        self.autoplay = !self.autoplay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_system_controls_blocks_both_skip_modes() {
        let mut modes = PlaybackModes::default();
        assert!(modes.is_skipping(true, true));
        assert!(!modes.is_skipping(false, true));
        modes.toggle_skip();
        assert!(modes.is_skipping(true, false));
        assert!(!modes.is_skipping(false, false));
        modes.toggle_autoplay();
        assert!(modes.autoplay);
        assert!(modes.skip);
        modes.toggle_skip();
        assert!(!modes.skip && modes.autoplay);
        modes.toggle_skip();
        modes.toggle_autoplay();
        assert!(modes.skip && !modes.autoplay);
        modes.reset();
        assert!(!modes.autoplay && !modes.skip);
    }
}
