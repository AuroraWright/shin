//! Glue together `shin-core` and `kira` to provide an API to play NXA audio files.

mod data;
mod handle;
mod manager;
mod resampler;
mod segment;
mod sound;

pub use data::{AudioData, AudioDataWithTweens, AudioTweens};
pub use handle::AudioHandle;
use kira::track::TrackId;
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
    pub track: TrackId,
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
