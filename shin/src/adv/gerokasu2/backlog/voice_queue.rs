//! Original95C10/95D50: serial cues with ten idle browser ticks between them.
use super::history::layout::Voice;

#[derive(Default)]
pub struct Sequence {
    voices: Vec<Voice>,
    index: usize,
    elapsed: f32,
    active: bool,
}
impl Sequence {
    ///95C10 replaces the cue list even when empty. The transport owner stops
    /// its old voice before calling this method.
    pub fn restart(&mut self, voices: Vec<Voice>) {
        self.voices = voices;
        self.index = 0;
        self.elapsed = 0.;
        self.active = true;
    }
    pub fn stop(&mut self) {
        self.active = false;
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }
    /// Browser80C40 supplies literal1, including frames with zero animation
    /// delta. Loading contributes the same PLAYING bit as an active source.
    /// The caller then advances the transport by that same literal tick.
    pub fn advance(&mut self, playing: bool) -> Option<Voice> {
        if !self.active || playing {
            return None;
        }
        if self.index >= self.voices.len() {
            self.active = false;
            return None;
        }
        self.elapsed += 1.;
        if self.elapsed < 10. {
            return None;
        }
        let voice = self.voices[self.index].clone();
        self.index += 1;
        self.elapsed = 0.;
        Some(voice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_gap_loading_restart_stop_and_last_cue_follow_native_order() {
        let cues = vec![
            Voice {
                path: "first|together".into(),
                volume: 0.37,
            },
            Voice {
                path: "".into(),
                volume: 1.,
            },
        ];
        let mut queue = Sequence::default();
        queue.restart(cues.clone());
        for _ in 0..9 {
            assert!(queue.advance(false).is_none());
        }
        for _ in 0..17 {
            assert!(queue.advance(true).is_none());
        }
        assert_eq!(queue.elapsed(), 9.);
        assert_eq!(queue.advance(false), Some(cues[0].clone()));
        for _ in 0..11 {
            assert!(queue.advance(true).is_none());
        }
        for _ in 0..9 {
            assert!(queue.advance(false).is_none());
        }
        assert_eq!(queue.advance(false), Some(cues[1].clone()));
        assert!(queue.active());
        assert!(queue.advance(false).is_none());
        assert!(!queue.active());
        queue.restart(cues);
        queue.advance(false);
        queue.stop();
        for _ in 0..20 {
            assert!(queue.advance(false).is_none());
        }
        queue.restart(vec![]);
        assert!(queue.active());
        assert!(queue.advance(true).is_none());
        assert!(queue.active());
        assert!(queue.advance(false).is_none());
        assert!(!queue.active());
    }
}
