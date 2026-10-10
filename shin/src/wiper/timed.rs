use shin_core::time::Ticks;
use shin_render::{
    RenderRequestBuilder, render_pass::RenderPass, shaders::types::texture::TextureSource,
};

use crate::{
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::Wiper,
};

pub trait TimedWiper {
    fn batch_parameters_match(&self, other: &Self) -> bool;

    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        assert!(words.iter().all(|&word| word == 0));
    }

    fn batch_uses_timer(&self) -> bool {
        true
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    );
}

#[derive(Debug, Clone)]
pub(super) struct TimedWiperState {
    current_time: Ticks,
    total_time: Ticks,
}

impl TimedWiperState {
    pub(super) fn can_batch_with(&self, other: &Self) -> bool {
        // Native11b134..11b16c compares both clocks against float epsilon.
        (self.current_time.as_f32() - other.current_time.as_f32()).abs() < f32::EPSILON
            && (self.total_time.as_f32() - other.total_time.as_f32()).abs() < f32::EPSILON
    }
    #[cfg(test)]
    pub(crate) fn reference_time_words(&self) -> [u32; 2] {
        [
            self.current_time.as_f32().to_bits(),
            self.total_time.as_f32().to_bits(),
        ]
    }
    pub fn new(total_time: Ticks) -> Self {
        // Native WiperBase (0x11afdc..0x11b014) makes nonpositive durations
        // already complete and keeps the progress denominator nonzero.
        if total_time <= Ticks::ZERO {
            let complete = Ticks::from_f32(1.0);
            return Self {
                current_time: complete,
                total_time: complete,
            };
        }
        Self {
            current_time: Ticks::ZERO,
            total_time,
        }
    }

    pub fn update(&mut self, allow_running_animations: bool, delta_time: Ticks) {
        if allow_running_animations {
            self.current_time += delta_time;
            if self.current_time >= self.total_time {
                self.current_time = self.total_time;
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.current_time < self.total_time
    }

    pub fn fast_forward(&mut self) {
        self.current_time = self.total_time;
    }

    pub fn get_progress(&self) -> f32 {
        self.current_time / self.total_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonpositive_wipes_start_complete_with_finite_progress() {
        for duration in [0.0, -1.0, -60.0] {
            let mut state = TimedWiperState::new(Ticks::from_f32(duration));
            assert!(!state.is_running());
            assert_eq!(state.get_progress(), 1.0);
            state.update(false, Ticks::from_f32(30.0));
            assert_eq!(state.get_progress(), 1.0);
            state.update(true, Ticks::from_f32(30.0));
            assert!(!state.is_running());
            assert_eq!(state.get_progress(), 1.0);
            state.fast_forward();
            assert_eq!(state.get_progress(), 1.0);
        }
    }

    #[test]
    fn positive_wipes_pause_clamp_and_fast_forward() {
        let mut state = TimedWiperState::new(Ticks::from_f32(30.0));
        assert!(state.is_running());
        assert_eq!(state.get_progress(), 0.0);
        state.update(false, Ticks::from_f32(15.0));
        assert_eq!(state.get_progress(), 0.0);
        state.update(true, Ticks::from_f32(15.0));
        assert_eq!(state.get_progress(), 0.5);
        let mut forwarded = state.clone();
        forwarded.fast_forward();
        assert!(!forwarded.is_running());
        assert_eq!(forwarded.get_progress(), 1.0);
        assert_eq!(state.get_progress(), 0.5);
        state.update(true, Ticks::from_f32(100.0));
        assert!(!state.is_running());
        assert_eq!(state.get_progress(), 1.0);
    }
}

#[derive(Debug)]
pub struct TimedWiperWrapper<T> {
    pub(super) state: TimedWiperState,
    pub(super) inner: T,
    updated_frame: shin_core::primitives::update::FrameId,
}

impl<T: Clone> Clone for TimedWiperWrapper<T> {
    fn clone(&self) -> Self {
        // Native11b030 copies the clocks but calls6a9e0 to reset the frame
        // marker; property copies instead preserve their original marker.
        Self {
            state: self.state.clone(),
            inner: self.inner.clone(),
            updated_frame: Default::default(),
        }
    }
}

impl<T> TimedWiperWrapper<T> {
    pub fn from_inner(inner: T, total_time: Ticks) -> Self {
        Self {
            state: TimedWiperState::new(total_time),
            inner,
            updated_frame: Default::default(),
        }
    }
    fn begin_frame(&mut self, frame: shin_core::primitives::update::FrameId) -> bool {
        if self.updated_frame == frame {
            return false;
        }
        self.updated_frame = frame;
        true
    }
}

impl<T: TimedWiper> TimedWiperWrapper<T> {
    #[cfg(test)]
    pub(super) fn reference_apply_batch_words(&mut self, words: &[u32; 9]) {
        self.state.current_time = Ticks::from_f32(f32::from_bits(words[0]));
        self.state.total_time = Ticks::from_f32(f32::from_bits(words[1]));
        self.inner.reference_set_batch_words(&words[2..]);
    }

    pub(super) fn can_batch_with(&self, other: &Self) -> bool {
        self.inner.batch_parameters_match(&other.inner)
            && (!self.inner.batch_uses_timer() || self.state.can_batch_with(&other.state))
    }
}

pub(super) fn batch_float_matches(left: f32, right: f32) -> bool {
    (left - right).abs() < f32::EPSILON
}

impl<T: AdvUpdatable> AdvUpdatable for TimedWiperWrapper<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        // The native generic gate surrounds both timer and effect updates.
        if !self.begin_frame(context.frame_id) {
            return;
        }
        self.state
            .update(context.are_animations_allowed, context.delta_ticks);
        self.inner.update(context);
    }
}

impl<T: TimedWiper + AdvUpdatable> Wiper for TimedWiperWrapper<T> {
    fn is_running(&self) -> bool {
        self.state.is_running()
    }

    fn fast_forward(&mut self) {
        self.state.fast_forward()
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
    ) {
        self.inner.render(
            pass,
            render_request_builder,
            texture_target,
            texture_source,
            self.state.get_progress(),
        );
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_all_wipe_frame_guards_and_copy_marker_reset() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let header_word =
        |offset: usize| u32::from_le_bytes(exe[offset..offset + 4].try_into().unwrap());
    assert_eq!(&exe[..4], b"NSO0");
    let bss_start = header_word(0x34) + header_word(0x38);
    let bss_end = bss_start + header_word(0x3c);
    assert_eq!(bss_start, 0x226658);
    assert_eq!(bss_end, 0x243000);
    assert!((bss_start..bss_end).contains(&0x24100c));
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    let relocs: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|o| {
            let u64_at = |a: usize| u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap());
            (u64_at(o), u64_at(o + 16))
        })
        .collect();
    for (base, body, copy, base_call) in [
        (0x1d6c70, 0x11b1e0, 0x62510, 0x62538),
        (0x1d9f60, 0x11b1e0, 0xa14e0, 0xa150c),
        (0x220d28, 0x11b1e0, 0xef3a0, 0xef3c8),
        (0x223878, 0x11b1e0, 0x11c320, 0x11c348),
        (0x222f40, 0x11b1e0, 0x116c50, 0x116c78),
        (0x223170, 0x11b1e0, 0x11a730, 0x11a758),
        (0x21f4f8, 0x11b1e0, 0xdc630, 0xdc658),
        (0x21e778, 0xd1080, 0xd0d90, 0xd0db8),
        (0x223210, 0x11b1e0, 0x11ac80, 0x11aca8),
        (0x1d7588, 0x6ca20, 0x6c860, 0x6c888),
    ] {
        assert_eq!(relocs[&(base + 0x20)], 0x6aa40);
        assert_eq!(relocs[&(base + 0x28)], body);
        assert_eq!(relocs[&(base + 0x30)], copy);
        let instruction = word(base_call);
        assert_eq!(instruction >> 26, 0b100101);
        let displacement = (((instruction & 0x03ffffff) << 6) as i32 >> 6) as i64 * 4;
        assert_eq!(base_call as i64 + displacement, 0x11b030);
    }
    for (address, expected) in NATIVE_WIPE_FRAME_WORDS {
        assert_eq!(word(address), expected, "Native{address:x}");
    }
}

#[cfg(test)]
const NATIVE_WIPE_FRAME_WORDS: [(usize, u32); 23] = [
    (0xfbaa0, 0xd0000a28),
    (0xfbaa4, 0x91003108),
    (0xfbaa8, 0x88dffd00),
    (0xfb130, 0xd0000a2a),
    (0xfb134, 0x9100314a),
    (0xfb144, 0x885ffd48),
    (0xfb148, 0x11000508),
    (0xfb14c, 0x8809fd48),
    (0xfb150, 0x35ffffa9),
    (0xfb154, 0x17fffab7),
    (0x6a9e8, 0xb900081f),
    (0x11b044, 0x97fd3e67),
    (0x11b068, 0xb9401e68),
    (0x11b06c, 0xb9001e88),
    (0x11b070, 0xb9402268),
    (0x11b074, 0xb9002288),
    (0x6aa5c, 0x94024411),
    (0x6aa64, 0x885ffd28),
    (0x6aa68, 0x880afd20),
    (0x6aa70, 0x6b00011f),
    (0x6aa74, 0x540000c1),
    (0x6aa90, 0xf9400d08),
    (0x6aaa0, 0xd63f0100),
];
