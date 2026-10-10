#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameId(u32);

impl FrameId {
    #[inline]
    pub fn advance(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

pub struct UpdateTracker {
    frame_id: FrameId,
}

impl UpdateTracker {
    #[inline]
    pub fn new() -> Self {
        Self {
            frame_id: FrameId::default(),
        }
    }

    #[inline]
    pub fn needs_update(&self, frame_id: FrameId) -> bool {
        self.frame_id != frame_id
    }

    #[inline]
    pub fn update(&mut self, frame_id: FrameId) -> bool {
        let result = self.needs_update(frame_id);

        self.frame_id = frame_id;

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_counter_wraps_and_tracker_accepts_each_different_frame() {
        let mut frame = FrameId(u32::MAX);
        let mut tracker = UpdateTracker::new();
        assert!(!tracker.update(FrameId::default()));
        assert!(tracker.update(frame));
        assert!(!tracker.update(frame));
        frame.advance();
        assert_eq!(frame, FrameId::default());
        assert!(tracker.needs_update(frame));
        assert!(tracker.update(frame));
        assert!(!tracker.update(frame));
        frame.advance();
        assert!(tracker.update(frame));
        // Native compares equality, not chronological ordering.
        assert!(tracker.update(FrameId::default()));
    }
}
