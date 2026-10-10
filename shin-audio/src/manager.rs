use kira::{
    clock::clock_info::ClockInfoProvider,
    manager::AudioManagerSettings,
    modulator::value_provider::ModulatorValueProvider,
    sound::{Sound, SoundData},
    Frame, OutputDestination,
};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
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
    fn output_destination(&mut self) -> OutputDestination {
        self.sound.output_destination()
    }
    fn on_start_processing(&mut self) {
        self.sound.on_start_processing();
    }
    fn process(
        &mut self,
        dt: f64,
        clocks: &ClockInfoProvider,
        modulators: &ModulatorValueProvider,
    ) -> Frame {
        if self.suspension.is_suspended() {
            Frame::ZERO
        } else {
            self.sound.process(dt, clocks, modulators)
        }
    }
    fn finished(&self) -> bool {
        self.sound.finished()
    }
}

type Backend = kira::manager::backend::cpal::CpalBackend;

pub struct AudioManager {
    manager: Mutex<kira::manager::AudioManager<Backend>>,
    suspension: AudioSuspension,
}

impl AudioManager {
    #[allow(clippy::new_without_default)]
    #[cfg(not(test))]
    pub fn new() -> Self {
        let manager = kira::manager::AudioManager::new(AudioManagerSettings::default())
            .expect("Failed to create kira audio manager");

        Self {
            manager: Mutex::new(manager),
            suspension: AudioSuspension::default(),
        }
    }

    #[cfg(test)]
    pub fn new() -> Self {
        Self::new_for_tests()
    }

    /// Hardware tests always use the system output named `Loopback Audio`.
    /// Missing or unusable test devices fail instead of using the default output.
    pub fn new_for_tests() -> Self {
        use cpal::traits::{DeviceTrait, HostTrait};
        use kira::manager::backend::cpal::CpalBackendSettings;

        let mut output_names = Vec::new();
        let device = cpal::default_host()
            .output_devices()
            .expect("Failed to enumerate test audio outputs")
            .find(|device| {
                let name = device.name().expect("Failed to read test audio device name");
                let matches = name.eq_ignore_ascii_case("Loopback Audio");
                output_names.push(name);
                matches
            })
            .unwrap_or_else(|| {
                panic!("Tests require the system audio output device named 'Loopback Audio'; available outputs: {output_names:?}")
            });
        tracing::info!(device = %device.name().expect("Failed to read test audio device name"), "Using test audio output");
        let manager = kira::manager::AudioManager::new(AudioManagerSettings {
            backend_settings: CpalBackendSettings {
                device: Some(device),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("Failed to create audio manager on the 'Loopback Audio' test output");
        Self {
            manager: Mutex::new(manager),
            suspension: AudioSuspension::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_sound_count(&self) -> u16 {
        self.manager.lock().num_sounds()
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

    pub fn suspension(&self) -> AudioSuspension {
        self.suspension.clone()
    }
    pub fn set_suspended(&self, suspended: bool) {
        self.suspension.set_suspended(suspended);
    }

    pub fn kira_manager(&self) -> &Mutex<kira::manager::AudioManager<Backend>> {
        &self.manager
    }
}
