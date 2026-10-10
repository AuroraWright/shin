//! Native MovieMode playback child89e60: cover8b7e0, monitor8a760,
//! movie fade8bb20 and final cover/retirement8bd70. Owner still pending.
use shin_core::time::{Ticks, Tween, Tweener};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaybackEvent {
    Start,
    Stop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Playing,
    MovieFade,
    CoverFade,
    Closed,
}
pub(super) struct MoviePlaybackMotion {
    phase: Phase,
    cover: Tweener,
    movie: Tweener,
    input_enabled: bool,
    frame_prepared: bool,
    events: Vec<PlaybackEvent>,
}
impl MoviePlaybackMotion {
    pub fn new() -> Self {
        let mut cover = Tweener::new(0.0);
        cover.enqueue(1.0, Tween::linear(Ticks::from_f32(30.0)));
        Self {
            phase: Phase::Opening,
            cover,
            movie: Tweener::new(1.0),
            input_enabled: false,
            frame_prepared: false,
            events: Vec::new(),
        }
    }
    ///8b7e0 waits cover completion BEFORE player loading. Newly installed
    ///monitor8a760 cannot run until the following task-group poll. Status1
    ///preempts input; close-monitor8bb20 accepts any nonzero status to stop.
    ///Return true only when an accepted skip needs a cue: controller uses
    ///cancel8a7c8; pointer release uses decide8a9a0.
    pub fn prepare_frame(&mut self, player_ready: bool, player_status: i32, skip: bool) -> bool {
        if self.frame_prepared {
            return false;
        }
        self.frame_prepared = true;
        self.input_enabled = self.phase == Phase::Playing;
        match self.phase {
            Phase::Opening if self.cover.is_idle() && player_ready => {
                self.events.push(PlaybackEvent::Start);
                self.phase = Phase::Playing;
            }
            Phase::Playing if player_status == 1 => self.begin_close(),
            Phase::Playing if skip => {
                self.begin_close();
                return true;
            }
            Phase::MovieFade if self.movie.is_idle() || player_status != 0 => {
                self.events.push(PlaybackEvent::Stop);
                self.cover
                    .enqueue(0.0, Tween::linear(Ticks::from_f32(30.0)));
                self.phase = Phase::CoverFade;
            }
            Phase::CoverFade if self.cover.is_idle() => {
                self.phase = Phase::Closed;
            }
            _ => {}
        }
        false
    }
    fn begin_close(&mut self) {
        self.phase = Phase::MovieFade;
        self.movie
            .enqueue(0.0, Tween::linear(Ticks::from_f32(30.0)));
    }
    ///8a550 polls tasks, updates the player, then movie/cover clocks. Even new
    ///fades advance this frame; their newly installed monitor waits next poll.
    pub fn advance(&mut self, delta: Ticks) {
        if delta > Ticks::ZERO {
            self.movie.update(delta);
            self.cover.update(delta);
        }
        self.frame_prepared = false;
    }
    pub fn cover_opacity(&self) -> f32 {
        self.cover.value()
    }
    /// Input belongs to the monitor installed on an earlier task poll.
    pub fn polls_input(&self) -> bool {
        self.phase == Phase::Playing && !self.frame_prepared
    }
    pub fn movie_opacity(&self) -> f32 {
        self.movie.value()
    }
    ///Native player virtual50 reads started flagC40; Stop clears it. A loaded
    ///paused first frame and a stopped last frame must not cover the gallery.
    pub fn renders_movie(&self) -> bool {
        matches!(self.phase, Phase::Playing | Phase::MovieFade)
    }
    pub fn accepts_input(&self) -> bool {
        self.input_enabled && self.phase == Phase::Playing
    }
    pub fn is_finished(&self) -> bool {
        self.phase == Phase::Closed
    }
    pub fn take_events(&mut self) -> Vec<PlaybackEvent> {
        std::mem::take(&mut self.events)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn playing() -> MoviePlaybackMotion {
        let mut m = MoviePlaybackMotion::new();
        m.prepare_frame(true, 0, false);
        m.advance(Ticks::from_f32(30.0));
        m.prepare_frame(true, 0, false);
        assert_eq!(m.take_events(), [PlaybackEvent::Start]);
        m.advance(Ticks::ZERO);
        m.prepare_frame(true, 0, false);
        assert!(m.accepts_input());
        m.advance(Ticks::ZERO);
        m
    }
    #[test]
    fn loading_and_cover_both_gate_start_and_new_monitor_rejects_skip() {
        let mut m = MoviePlaybackMotion::new();
        assert_eq!((m.cover_opacity(), m.movie_opacity()), (0.0, 1.0));
        m.prepare_frame(true, 1, true);
        m.advance(Ticks::from_f32(15.0));
        assert_eq!(m.cover_opacity(), 0.5);
        assert!(m.take_events().is_empty());
        m.prepare_frame(false, 1, true);
        m.advance(Ticks::from_f32(15.0));
        assert!(m.take_events().is_empty());
        m.prepare_frame(false, 1, true);
        m.advance(Ticks::from_f32(100.0));
        assert!(m.take_events().is_empty());
        assert!(!m.prepare_frame(true, 1, true));
        assert_eq!(m.take_events(), [PlaybackEvent::Start]);
        assert!(!m.accepts_input());
        m.advance(Ticks::ZERO);
        assert!(!m.prepare_frame(true, 1, true));
        assert!(!m.accepts_input());
        assert_eq!(m.phase, Phase::MovieFade);
    }
    #[test]
    fn skip_has_two_thirty_tick_fades_with_next_poll_stop_and_retirement() {
        let mut m = playing();
        assert!(m.prepare_frame(true, 0, true));
        assert!(!m.accepts_input());
        m.advance(Ticks::from_f32(15.0));
        assert_eq!((m.movie_opacity(), m.cover_opacity()), (0.5, 1.0));
        assert!(!m.prepare_frame(true, 0, true));
        m.advance(Ticks::from_f32(15.0));
        assert!(m.take_events().is_empty());
        assert_eq!((m.movie_opacity(), m.cover_opacity()), (0.0, 1.0));
        m.prepare_frame(true, 0, false);
        assert_eq!(m.take_events(), [PlaybackEvent::Stop]);
        m.advance(Ticks::from_f32(15.0));
        assert_eq!(m.cover_opacity(), 0.5);
        assert!(!m.is_finished());
        m.prepare_frame(true, 0, false);
        m.advance(Ticks::from_f32(15.0));
        assert!(!m.is_finished());
        assert_eq!(m.cover_opacity(), 0.0);
        m.prepare_frame(true, 0, false);
        assert!(m.is_finished());
        m.advance(Ticks::from_f32(100.0));
        m.prepare_frame(true, 1, true);
        assert!(m.take_events().is_empty());
    }
    #[test]
    fn completion_preempts_cue_and_shortens_movie_fade_but_not_final_cover_fade() {
        for elapsed in [0.0, 5.0, 20.0] {
            let mut m = playing();
            if elapsed > 0.0 {
                assert!(m.prepare_frame(true, 0, true));
                m.advance(Ticks::from_f32(elapsed));
            } else {
                assert!(!m.prepare_frame(true, 1, true));
                m.advance(Ticks::ZERO);
            }
            assert!(!m.prepare_frame(true, 1, true));
            assert_eq!(m.take_events(), [PlaybackEvent::Stop]);
            assert_eq!(m.cover_opacity(), 1.0);
            m.advance(Ticks::from_f32(29.0));
            m.prepare_frame(true, 1, true);
            assert!(!m.is_finished());
            m.advance(Ticks::from_f32(1.0));
            assert!(!m.is_finished());
            m.prepare_frame(true, 1, true);
            assert!(m.is_finished());
        }
    }
    #[test]
    fn status_two_is_not_natural_completion_but_releases_pending_movie_fade() {
        let mut m = playing();
        assert!(!m.prepare_frame(true, 2, false));
        assert!(m.accepts_input());
        m.advance(Ticks::ZERO);
        assert!(m.prepare_frame(true, 2, true));
        m.advance(Ticks::ZERO);
        m.prepare_frame(true, 2, false);
        assert_eq!(m.take_events(), [PlaybackEvent::Stop]);
    }
    #[test]
    fn clocks_freeze_for_nonpositive_delta_and_tasks_poll_once_per_frame() {
        let mut m = playing();
        assert!(m.prepare_frame(true, 0, true));
        assert!(!m.prepare_frame(true, 1, true));
        assert!(m.take_events().is_empty());
        m.advance(Ticks::from_f32(-10.0));
        assert_eq!(m.movie_opacity(), 1.0);
        m.prepare_frame(true, 0, false);
        m.advance(Ticks::ZERO);
        assert_eq!(m.movie_opacity(), 1.0);
        m.prepare_frame(true, 0, false);
        m.advance(Ticks::from_f32(100.0));
        assert_eq!((m.movie_opacity(), m.cover_opacity()), (0.0, 1.0));
        m.prepare_frame(true, 0, false);
        assert_eq!(m.take_events(), [PlaybackEvent::Stop]);
        m.advance(Ticks::from_f32(100.0));
        assert!(!m.is_finished());
        m.prepare_frame(true, 0, false);
        assert!(m.is_finished());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_movie_playback_fades_loading_status_and_render_gate() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x89ef8, 0x3902e2bf),
        (0x89fc8, 0x781783a8),
        (0x89fe0, 0x9400e154),
        (0x8a058, 0x1e2e1000),
        (0x8a060, 0x9400c4a0),
        (0x8a12c, 0xaa1603e0),
        (0x8a134, 0x1e2e1000),
        (0x8a138, 0x1e27d001),
        (0x8a144, 0x9400c62f),
        (0x8b7f8, 0x91048280),
        (0x8b7fc, 0x9400c07d),
        (0x8b800, 0x36000060),
        (0x8b818, 0xf9401508),
        (0x8b824, 0x37000660),
        (0x8b830, 0xf9402108),
        (0x8b834, 0xd63f0100),
        (0x8a778, 0xc8dffd00),
        (0x8a780, 0xf9402d08),
        (0x8a78c, 0x7100041f),
        (0x8a790, 0x54000081),
        (0x8a7c0, 0x721c051f),
        (0x8a7cc, 0xf943bd08),
        (0x8a830, 0x3902e008),
        (0x8a83c, 0x2a1f03e2),
        (0x8a840, 0x9400c470),
        (0x8a9b0, 0xf9437108),
        (0x8a9d0, 0x17ffff90),
        (0x8bb34, 0x91032260),
        (0x8bb38, 0x9400bfae),
        (0x8bb3c, 0x360000e0),
        (0x8bb4c, 0xf9402d08),
        (0x8bb54, 0x34000700),
        (0x8bb64, 0xf9402508),
        (0x8bb70, 0x1e27d001),
        (0x8bb74, 0x91048260),
        (0x8bb80, 0x9400bfa0),
        (0x8bd74, 0x91048100),
        (0x8bd78, 0x1400bf1e),
        (0x8a570, 0x9401d6d0),
        (0x8a584, 0xd63f0100),
        (0x8a590, 0x9400c530),
        (0x8a59c, 0x9400c52d),
        (0x8a638, 0x1e2b2180),
        (0x8a640, 0x1a9fb500),
        (0x8a674, 0x4eac1d83),
        (0x8a69c, 0x9400f1ad),
        (0x8a6ac, 0xf9402908),
        (0x8a6cc, 0x3d42c100),
        (0x8a6ec, 0x1e230900),
        (0x8a704, 0x4ea81d03),
        (0xbed80, 0x39310288),
        (0xbee54, 0x3931029f),
        (0xbeebc, 0x39710273),
        (0xbc560, 0x39400288),
        (0xbc564, 0x390082c8),
        (0xbc568, 0x39400688),
        (0xbc56c, 0x390086c8),
        (0xbc570, 0x39400a88),
        (0xbc580, 0x39008ac8),
        (0xc04b8, 0x39408688),
        (0xc04bc, 0x34000088),
        (0x8a950, 0x320003e0),
    ] {
        assert_eq!(word(a), expected, "native MovieMode playback{a:x}");
    }
    assert_eq!(word(0x159948), 0.01f32.to_bits());
    let mut m = MoviePlaybackMotion::new();
    assert!(!m.renders_movie());
    m.prepare_frame(true, 0, false);
    m.advance(Ticks::from_f32(30.0));
    assert!(!m.renders_movie());
    m.prepare_frame(true, 0, false);
    assert!(m.renders_movie());
    m.advance(Ticks::ZERO);
    m.prepare_frame(true, 1, false);
    assert!(m.renders_movie());
    m.advance(Ticks::ZERO);
    m.prepare_frame(true, 1, false);
    assert!(!m.renders_movie());
    assert_eq!(m.movie_opacity(), 1.0);
    assert_eq!(m.cover_opacity(), 1.0);
    // The stopped frame is hidden while the separate black cover reveals parent.
    m.advance(Ticks::from_f32(15.0));
    assert!(!m.renders_movie());
    assert_eq!(m.cover_opacity(), 0.5);
}
