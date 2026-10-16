//! TitleDemo113b00: worker gate114a90, playback114280, skip114140.
pub(super) mod scene;
use shin_core::time::{Ticks, Tween, Tweener};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Playing,
    Fading,
    Queued,
}

pub(super) struct TitleDemoMotion {
    phase: Phase,
    opacity: Tweener,
    destination: Option<i32>,
}

impl TitleDemoMotion {
    pub fn new() -> Self {
        Self {
            phase: Phase::Loading,
            opacity: Tweener::new(0.0),
            destination: None,
        }
    }

    /// First task114a90 polls the worker before113db0 advances the clock.
    /// A missing movie table entry completes its worker without a player.
    pub fn worker_ready(&mut self, has_player: bool) {
        if self.phase != Phase::Loading {
            return;
        }
        self.opacity = Tweener::new(1.0);
        if has_player {
            self.phase = Phase::Playing;
        } else {
            self.queue_title();
        }
    }

    /// Poll movie completion before clock advancement/player update. The owner
    /// supplies the native input gate and only fresh Enter/Cancel/pointer edges.
    /// Return true once when the owner must play the native confirmation cue.
    pub fn prepare_frame(&mut self, movie_finished: bool, skip_edge: bool) -> bool {
        match self.phase {
            Phase::Playing if movie_finished => self.queue_title(),
            Phase::Playing if skip_edge => {
                self.phase = Phase::Fading;
                self.opacity
                    .enqueue(0.0, Tween::linear(Ticks::from_f32(90.0)));
                return true;
            }
            Phase::Fading if movie_finished || self.opacity.is_idle() => self.queue_title(),
            _ => {}
        }
        false
    }

    pub fn advance(&mut self, delta: Ticks) {
        if delta > Ticks::ZERO {
            self.opacity.update(delta);
        }
    }

    pub fn opacity(&self) -> f32 {
        self.opacity.value()
    }

    pub fn accepts_input(&self) -> bool {
        self.phase == Phase::Playing
    }

    pub fn take_title_argument(&mut self) -> Option<i32> {
        self.destination.take()
    }

    fn queue_title(&mut self) {
        self.phase = Phase::Queued;
        self.destination = Some(-1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_blocks_input_and_completion_then_absent_movie_queues_initial_title_once() {
        let mut motion = TitleDemoMotion::new();
        assert!(!motion.prepare_frame(true, true));
        motion.advance(Ticks::from_f32(1000.0));
        assert_eq!(motion.opacity(), 0.0);
        assert_eq!(motion.take_title_argument(), None);
        motion.worker_ready(false);
        assert_eq!(motion.take_title_argument(), Some(-1));
        motion.worker_ready(true);
        motion.prepare_frame(true, true);
        assert_eq!(motion.take_title_argument(), None);
    }

    #[test]
    fn skip_fades_for_ninety_ticks_and_dispatches_on_following_task_poll() {
        let mut motion = TitleDemoMotion::new();
        motion.worker_ready(true);
        assert_eq!(motion.opacity(), 1.0);
        assert!(motion.prepare_frame(false, true));
        for frame in 1..=180 {
            motion.advance(Ticks::from_f32(0.5));
            assert!((motion.opacity() - (1.0 - frame as f32 / 180.0)).abs() < 0.00001);
            assert_eq!(motion.take_title_argument(), None);
            if frame < 180 {
                assert!(!motion.prepare_frame(false, true));
            }
        }
        assert!(!motion.prepare_frame(false, false));
        assert_eq!(motion.take_title_argument(), Some(-1));
        motion.prepare_frame(true, true);
        assert_eq!(motion.take_title_argument(), None);
    }

    #[test]
    fn natural_completion_preempts_skip_and_can_finish_an_in_progress_fade() {
        for skip in [false, true] {
            let mut motion = TitleDemoMotion::new();
            motion.worker_ready(true);
            if skip {
                assert!(motion.prepare_frame(false, true));
                motion.advance(Ticks::from_f32(15.0));
                let opacity = motion.opacity();
                motion.advance(Ticks::from_f32(-100.0));
                assert_eq!(motion.opacity(), opacity);
            }
            assert!(!motion.prepare_frame(true, true));
            assert_eq!(motion.take_title_argument(), Some(-1));
        }
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_first_boot_and_title_demo_gates() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x659e0, 0xf9400408),
            (0x659e4, 0x12800009),
            (0x659e8, 0x320003e0),
            (0x659ec, 0xb9007509),
            (0x66490, 0xd65f03c0),
            (0x665c8, 0x9402b94a),
            (0x1151c8, 0x97fffa4e),
            (0x11455c, 0x2a1f03e1),
            (0x114560, 0x97ff5778),
            (0x114aa0, 0x35000168),
            (0x114ab4, 0x7100081f),
            (0x114abc, 0xf9400660),
            (0x114ac0, 0x97fffd08),
            (0x114ac8, 0xb9001268),
            (0x114074, 0x12800000),
            (0x114080, 0x97ffe7c8),
            (0x114150, 0x320003e8),
            (0x11415c, 0xbd49e101),
            (0x114174, 0x97fe9e23),
            (0x1142b0, 0x7100041f),
            (0x1142cc, 0x97ffe735),
            (0x11431c, 0x6a09011f),
            (0x11433c, 0x97ffff81),
            (0x114ca8, 0x97fe9b52),
            (0x114cc4, 0x7100041f),
            (0x114cd8, 0x97ffe4b2),
            (0x1146a0, 0x910262a0),
            (0x1146a8, 0x97fef4c2),
            (0x1146ac, 0x79c01a81),
            (0x1146b8, 0x97fef4be),
            (0x1146bc, 0x79c01281),
            (0x1146c8, 0x97fef4ba),
            (0x114f44, 0x97feb57b),
            (0x113e64, 0x7e21d800),
            (0x113e68, 0x1e210803),
            (0xc03e8, 0x1a9fb500),
            (0xc0428, 0x97fe5906),
            (0x56854, 0x6e21dc00),
            (0x5685c, 0x6ea1b800),
            (0x100204, 0xf940067a),
            (0x100300, 0xf85f8ee0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native instruction {address:x}"
            );
        }
        assert_eq!(
            f32::from_le_bytes(exe[0x1599e0 + 256..0x1599e0 + 260].try_into().unwrap()),
            90.0
        );
        assert_eq!(
            &exe[0x187128 + 256..0x187128 + 256 + 17],
            b"N4shin9TitleDemoE"
        );
    }
}
