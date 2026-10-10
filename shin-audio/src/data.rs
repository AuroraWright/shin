//! Implements the SoundData trait for the Kira audio library.

use std::sync::Arc;

use anyhow::Result;
use kira::sound::{Sound, SoundData};
use ringbuf::{traits::Split as _, HeapRb};
use shin_core::format::audio::{AudioDecoder, AudioFile, AudioFrameSource};

use super::AudioSettings;
use crate::{
    handle::AudioHandle,
    sound::{AudioSound, COMMAND_BUFFER_CAPACITY},
};
use shin_core::time::Tweener;

pub struct AudioData<S: AudioFrameSource> {
    pub source: S,
    pub settings: AudioSettings,
}

/// Parameter transitions prepared while an asset is loading. Their clocks start
/// with playback, rather than with the request or an audio command afterward.
pub struct AudioTweens {
    pub volume: Tweener,
    pub panning: Tweener,
    pub playback_rate: Tweener,
}

pub struct AudioDataWithTweens<S: AudioFrameSource> {
    data: AudioData<S>,
    tweens: AudioTweens,
}

impl<S: AudioFrameSource> AudioData<S> {
    pub fn with_tweens(self, tweens: AudioTweens) -> AudioDataWithTweens<S> {
        AudioDataWithTweens { data: self, tweens }
    }
}

impl<S: AudioFrameSource + Send + 'static> SoundData for AudioDataWithTweens<S> {
    type Error = anyhow::Error;
    type Handle = AudioHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let (mut sound, handle) = self.data.split();
        sound.install_tweens(self.tweens);
        Ok((Box::new(sound), handle))
    }
}

impl AudioData<AudioDecoder<Arc<AudioFile>>> {
    pub fn from_audio_file(audio: Arc<AudioFile>, settings: AudioSettings) -> Self {
        Self {
            // Native NXA playback uses nn::codec::DecodeOpusInterleaved's
            // signed16 output, shared with the voice lip-sync detector.
            source: AudioDecoder::new_pcm16(audio).expect("Failed to create audio decoder"),
            settings,
        }
    }
}

impl<S: AudioFrameSource + Send + 'static> SoundData for AudioData<S> {
    type Error = anyhow::Error;
    type Handle = AudioHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let (sound, handle) = self.split();
        Ok((Box::new(sound), handle))
    }
}

impl<S: AudioFrameSource + Send> AudioData<S> {
    fn split(self) -> (AudioSound<S>, AudioHandle) {
        let (command_producer, command_consumer) = HeapRb::new(COMMAND_BUFFER_CAPACITY).split();

        let sound = AudioSound::new(self, command_consumer);
        let shared = sound.shared();

        (
            sound,
            AudioHandle {
                command_producer,
                shared,
            },
        )
    }
}
