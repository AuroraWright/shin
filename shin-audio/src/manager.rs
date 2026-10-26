use kira::{
    AudioManagerSettings, Frame,
    info::Info,
    sound::{Sound, SoundData},
};
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Platform suspension is independent of a sound's own paused/start state.
#[derive(Clone, Default)]
pub struct AudioSuspension(Arc<AtomicBool>);

impl AudioSuspension {
    pub fn set_suspended(&self, suspended: bool) {
        self.0.store(suspended, Ordering::Release);
    }
    pub fn is_suspended(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn wrap<S: SoundData>(&self, data: S) -> SuspendedSoundData<S> {
        SuspendedSoundData {
            data,
            suspension: self.clone(),
        }
    }
}

pub(crate) struct SuspendedSoundData<S> {
    data: S,
    suspension: AudioSuspension,
}
impl<S: SoundData> SoundData for SuspendedSoundData<S> {
    type Error = S::Error;
    type Handle = S::Handle;
    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let (sound, handle) = self.data.into_sound()?;
        Ok((
            Box::new(SuspendedSound {
                sound,
                suspension: self.suspension,
            }),
            handle,
        ))
    }
}
struct SuspendedSound {
    sound: Box<dyn Sound>,
    suspension: AudioSuspension,
}
impl Sound for SuspendedSound {
    fn on_start_processing(&mut self) {
        self.sound.on_start_processing();
    }
    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        if self.suspension.is_suspended() {
            out.fill(Frame::ZERO);
        } else {
            self.sound.process(out, dt, info);
        }
    }
    fn finished(&self) -> bool {
        self.sound.finished()
    }
}

type Backend = kira::backend::cpal::CpalBackend;

/// Frames the native audio thread mixes per AudioOut buffer: Higurashi's
/// sub_2A630 opens a 48 kHz stereo AudioOut with two 1024-byte PCM16
/// buffers, and sub_2A830 refills each released one with 256 frames, taking
/// the sounds' state once per buffer.
const NATIVE_MIX_FRAMES: u32 = 256;
const NATIVE_SAMPLE_RATE: u32 = 48000;

/// The output stream in buffers of the native mix length (5.3 ms), so
/// sounds start, stop and change on the same grid. kira takes new commands
/// once a device buffer, which is 512 frames (10.7 ms) by default on macOS.
fn backend_settings() -> kira::backend::cpal::CpalBackendSettings {
    use kira::backend::cpal::cpal::{
        BufferSize, SupportedBufferSize,
        traits::{DeviceTrait, HostTrait},
    };
    let config = kira::backend::cpal::cpal::default_host()
        .default_output_device()
        .and_then(|device| device.default_output_config().ok())
        .map(|supported| {
            let frames = (supported.sample_rate() * NATIVE_MIX_FRAMES).div_ceil(NATIVE_SAMPLE_RATE);
            let mut config = supported.config();
            if let SupportedBufferSize::Range { min, max } = *supported.buffer_size()
                && (min..=max).contains(&frames)
            {
                config.buffer_size = BufferSize::Fixed(frames);
            }
            config
        });
    kira::backend::cpal::CpalBackendSettings {
        device: None,
        config,
    }
}

pub struct AudioManager {
    manager: Mutex<kira::AudioManager<Backend>>,
    suspension: AudioSuspension,
}

impl AudioManager {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let manager = kira::AudioManager::new(AudioManagerSettings {
            backend_settings: backend_settings(),
            ..Default::default()
        })
        .expect("Failed to create kira audio manager");

        Self {
            manager: Mutex::new(manager),
            suspension: AudioSuspension::default(),
        }
    }

    /// Tests play through the default output, like the game.
    pub fn new_for_tests() -> Self {
        Self::new()
    }

    #[cfg(test)]
    pub(crate) fn reference_sound_count(&self) -> usize {
        self.manager.lock().main_track().num_sounds()
    }

    pub fn play<S: SoundData>(&self, data: S) -> S::Handle
    where
        S::Error: std::fmt::Debug,
    {
        let mut manager = self.manager.lock();

        manager
            .play(self.suspension.wrap(data))
            .expect("Failed to start playing audio")
    }

    /// Play into a category track while retaining platform suspension behavior.
    pub fn play_on_track<S: SoundData>(
        &self,
        track: &mut kira::track::TrackHandle,
        data: S,
    ) -> S::Handle
    where
        S::Error: std::fmt::Debug,
    {
        track
            .play(self.suspension.wrap(data))
            .expect("Failed to start playing audio")
    }

    pub fn suspension(&self) -> AudioSuspension {
        self.suspension.clone()
    }
    pub fn set_suspended(&self, suspended: bool) {
        self.suspension.set_suspended(suspended);
    }

    pub fn kira_manager(&self) -> &Mutex<kira::AudioManager<Backend>> {
        &self.manager
    }
}
