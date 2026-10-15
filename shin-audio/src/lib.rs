//! Glue together `shin-core` and `kira` to provide an API to play NXA audio files.

mod data;
mod handle;
mod manager;
mod resampler;
mod segment;
mod sound;

pub use data::{AudioData, AudioDataWithTweens, AudioTweens};
pub use handle::AudioHandle;
pub use manager::{AudioManager, AudioSuspension};
pub use segment::AudioSegment;
pub use shin_core::format::audio::AudioFile;
use shin_core::{
    time::Tween,
    vm::command::types::{Pan, Volume},
};

pub struct AudioSettings {
    /// Begin with source playback suspended, without consuming samples.
    pub paused: bool,
    pub fade_in: Tween,
    pub loop_start: Option<u32>,
    /// Total passes when a loop start is supplied: 0 repeats indefinitely,
    /// 1 plays through EOF, and larger counts loop before the final EOF pass.
    pub loop_count: u32,
    pub volume: Volume,
    pub pan: Pan,
    /// Source-time multiplier,positive and finite;1.0 plays at authored speed.
    pub playback_rate: f32,
}

/// Category mixer owner with an identity that survives movement of the handle.
#[derive(Debug)]
pub struct AudioTrack {
    inner: kira::track::TrackHandle,
    identity: u64,
}
impl From<kira::track::TrackHandle> for AudioTrack {
    fn from(inner: kira::track::TrackHandle) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            inner,
            identity: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}
impl AudioTrack {
    pub fn id(&self) -> u64 {
        self.identity
    }
}
impl std::ops::Deref for AudioTrack {
    type Target = kira::track::TrackHandle;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for AudioTrack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Convert a linear category gain to Kira's logarithmic mixer units.
pub fn amplitude_to_decibels(amplitude: f32) -> kira::Decibels {
    if amplitude <= 0.0 {
        kira::Decibels::SILENCE
    } else {
        kira::Decibels(20.0 * amplitude.log10())
    }
}

#[cfg(any(test, feature = "reference-tests"))]
pub mod test_support;
