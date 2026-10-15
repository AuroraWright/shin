use std::{
    f32::consts::SQRT_2,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering},
    },
};

use kira::{Frame, info::Info, sound::Sound};
use ringbuf::{HeapCons, traits::Consumer as _};
use shin_core::{
    format::audio::{AudioFrameSource, AudioSource},
    time::{Ticks, Tween, Tweener},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use tracing::debug;

use crate::{AudioData, resampler::Resampler};

pub const COMMAND_BUFFER_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    SetVolume(Volume, Tween),
    EnqueueVolume(Volume, Tween),
    SetPanning(Pan, Tween),
    EnqueuePanning(Pan, Tween),
    SetPlaybackRate(f32, Tween),
    SetParameters(Volume, Pan, f32, Tween),
    ResumeParameters(Volume, Pan, f32, Tween),
    Stop(Tween),
    FinishVolumeAndStop(Tween),
    FinishVolumeAndPan,
}

pub(crate) struct Shared {
    pub wait_status: AtomicI32,
    pub paused: AtomicBool,
    // Movies publish current gain, rather than queued per-frame transitions.
    // Bit32 marks a pending value; the low32 bits retain the exact f32.
    pub immediate_volume: AtomicU64,
    pub immediate_stop: AtomicBool,
    // Integer milliseconds for message/voice animation; sample precision below
    // supplies the microsecond clock used by native media waits.
    pub position: AtomicU32,
    pub position_samples: AtomicU32,
    pub drained_position_samples: AtomicU32,
    pub sample_rate: u32,
    // used for lip sync
    pub amplitude: AtomicU32,
}

impl Shared {
    fn new(paused: bool, sample_rate: u32) -> Self {
        Self {
            wait_status: AtomicI32::new(0),
            paused: AtomicBool::new(paused),
            immediate_volume: AtomicU64::new(0),
            immediate_stop: AtomicBool::new(false),
            position: AtomicU32::new(0),
            position_samples: AtomicU32::new(0),
            drained_position_samples: AtomicU32::new(0),
            sample_rate,
            amplitude: AtomicU32::new(0),
        }
    }
}

/// The playback state of a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    /// The sound is playing normally.
    Playing,
    /// The sound is fading out, and when the fade-out
    /// is finished, playback will stop.
    Stopping,
    /// The sound has stopped and can no longer be resumed.
    Stopped,
}

pub struct SampleProvider<S: AudioFrameSource + Send> {
    source: AudioSource<S>,
    loop_start: Option<u32>,
    loop_count: u32,
    resampler: Resampler,
    fractional_position: f64,
    reached_eof: bool,
}

impl<S: AudioFrameSource + Send> SampleProvider<S> {
    #[cfg(test)]
    fn new(audio: S, loop_start: Option<u32>) -> Self {
        Self::with_loop_count(audio, loop_start, 0)
    }

    fn with_loop_count(audio: S, loop_start: Option<u32>, loop_count: u32) -> Self {
        Self {
            source: AudioSource::new(audio),
            loop_start,
            loop_count,
            resampler: Resampler::new(0),
            fractional_position: 0.0,
            reached_eof: false,
        }
    }

    fn loop_back(&mut self) -> bool {
        let Some(start) = self.loop_start else {
            return false;
        };
        if self.loop_count == 1 {
            return false;
        }
        if self.loop_count > 1 {
            self.loop_count -= 1;
        }
        self.source
            .samples_seek(start)
            .expect("Could not seek to loop start");
        true
    }

    fn push_frame_to_resampler(&mut self) {
        if self.source.inner().is_failed() {
            return;
        }
        if let (Some(_), Some(end)) = (self.loop_start, self.source.inner().loop_end()) {
            if self.source.current_samples_position() >= end {
                self.loop_back();
            }
        }
        let frame = match self.source.read_sample() {
            Some((left, right)) => Frame { left, right },
            None => {
                if self.source.inner().is_failed() {
                    return;
                }
                if self.loop_back() {
                    return self.push_frame_to_resampler();
                } else {
                    self.reached_eof = true;
                    Frame::ZERO
                }
            }
        };

        let next_sample_index = self.source.current_samples_position();
        self.resampler
            .push_frame(frame, next_sample_index.saturating_sub(1));
    }

    fn next(&mut self, dt: f64) -> Frame {
        if self.source.inner().is_failed() {
            return Frame::ZERO;
        }
        let out = self.resampler.get(self.fractional_position as f32);
        self.fractional_position += dt * self.source.sample_rate() as f64;
        while self.fractional_position >= 1.0 {
            self.fractional_position -= 1.0;
            self.push_frame_to_resampler();
            if self.source.inner().is_failed() {
                return Frame::ZERO;
            }
        }

        out
    }
}

pub struct AudioSound<S: AudioFrameSource + Send> {
    command_consumer: HeapCons<Command>,
    shared: Arc<Shared>,
    state: PlaybackState,
    volume: Tweener,
    panning: Tweener,
    volume_fade: Tweener,
    sample_provider: SampleProvider<S>,
    playback_rate: Tweener,
}

impl<S: AudioFrameSource + Send> AudioSound<S> {
    pub fn new(data: AudioData<S>, command_consumer: HeapCons<Command>) -> Self {
        assert!(data.settings.playback_rate.is_finite() && data.settings.playback_rate > 0.0);
        debug!("Creating audio sound");

        let mut volume_fade = Tweener::new(0.0);
        volume_fade.enqueue_now(1.0, data.settings.fade_in);

        let shared = Arc::new(Shared::new(data.settings.paused, data.source.sample_rate()));

        let res = AudioSound {
            command_consumer,
            shared,
            state: PlaybackState::Playing,
            volume: Tweener::new(data.settings.volume.0),
            panning: Tweener::new(data.settings.pan.0),
            volume_fade,
            sample_provider: SampleProvider::with_loop_count(
                data.source,
                data.settings.loop_start,
                data.settings.loop_count,
            ),
            playback_rate: Tweener::new(data.settings.playback_rate),
        };

        // make sure the wait_status is reflective of the actual state right after the handle creation
        res.shared.wait_status.store(
            res.wait_status().bits(),
            std::sync::atomic::Ordering::SeqCst,
        );

        res
    }

    pub(crate) fn install_tweens(&mut self, tweens: crate::AudioTweens) {
        self.volume = tweens.volume;
        self.panning = tweens.panning;
        self.playback_rate = tweens.playback_rate;
        self.volume_fade = Tweener::new(1.0);
        self.shared.wait_status.store(
            self.wait_status().bits(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    fn stop(&mut self, fade_out_tween: Tween) {
        self.state = PlaybackState::Stopping;
        // Stop fades begin at the current gain,without a concurrent volume ramp.
        self.volume
            .enqueue_now(self.volume.value(), Tween::IMMEDIATE);
        self.volume_fade.enqueue_now(0.0, fade_out_tween);
    }

    fn retarget_volume(&mut self, volume: Volume, tween: Tween) {
        if self.state == PlaybackState::Playing {
            // Native volume has one interpolator,including initial fade-in.
            let current = self.volume.value() * self.volume_fade.value();
            self.volume.enqueue_now(current, Tween::IMMEDIATE);
            self.volume_fade.enqueue_now(1.0, Tween::IMMEDIATE);
        }
        self.volume.enqueue_now(volume.0, tween);
    }

    fn wait_status(&self) -> AudioWaitStatus {
        if self.state == PlaybackState::Stopped
            || self.sample_provider.source.inner().is_failed()
                && self.sample_provider.source.inner().failure_ends_playback()
        {
            // Stopped sounds leave the pool. Native NXA failures report state2
            // while retaining the paused backend voice; neither contributes waits.
            return AudioWaitStatus::empty();
        }
        let mut result = AudioWaitStatus::PLAYING;
        if !self.volume.is_idle()
            || self.state == PlaybackState::Playing && !self.volume_fade.is_idle()
        {
            result |= AudioWaitStatus::VOLUME_TWEENING;
        }
        if !self.panning.is_idle() {
            result |= AudioWaitStatus::PANNING_TWEENING;
        }
        if !self.playback_rate.is_idle() {
            result |= AudioWaitStatus::PLAY_SPEED_TWEENING;
        }

        result
    }

    pub(crate) fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }
}

impl<S: AudioFrameSource + Send> Sound for AudioSound<S> {
    fn on_start_processing(&mut self) {
        while let Some(command) = self.command_consumer.try_pop() {
            match command {
                // note: unlike in the layer props, we do the "enqueue_now" thing here
                // bacause we don't want to wait for previous audio changes to be applied
                // ideally, this should never allocate the tweener queue
                Command::SetVolume(volume, tween) => self.retarget_volume(volume, tween),
                Command::EnqueueVolume(volume, tween) => self.volume.enqueue(volume.0, tween),
                Command::SetPanning(panning, tween) => self.panning.enqueue_now(panning.0, tween),
                Command::EnqueuePanning(pan, tween) => self.panning.enqueue(pan.0, tween),
                Command::SetPlaybackRate(rate, tween) => {
                    self.playback_rate.enqueue_now(rate, tween)
                }
                Command::SetParameters(volume, pan, rate, tween) => {
                    self.retarget_volume(volume, tween);
                    self.panning.enqueue_now(pan.0, tween);
                    self.playback_rate.enqueue_now(rate, tween);
                }
                Command::ResumeParameters(volume, pan, rate, tween) => {
                    // Older SE repeats can revive a matching stopping entry.
                    // Preserve its current audible gain and source position.
                    if self.state != PlaybackState::Stopped {
                        let current = self.volume.value() * self.volume_fade.value();
                        self.volume.enqueue_now(current, Tween::IMMEDIATE);
                        self.volume_fade.enqueue_now(1.0, Tween::IMMEDIATE);
                        self.state = PlaybackState::Playing;
                        self.retarget_volume(volume, tween);
                        self.panning.enqueue_now(pan.0, tween);
                        self.playback_rate.enqueue_now(rate, tween);
                    }
                }
                Command::Stop(tween) => self.stop(tween),
                Command::FinishVolumeAndStop(tween) => {
                    if self.state != PlaybackState::Stopped {
                        self.volume.fast_forward();
                        self.volume_fade.fast_forward();
                        let target = self.volume.value() * self.volume_fade.value();
                        self.volume.fast_forward_to(target);
                        self.volume_fade.fast_forward_to(1.0);
                        self.stop(tween);
                    }
                }
                Command::FinishVolumeAndPan => {
                    if self.state != PlaybackState::Stopped {
                        self.volume.fast_forward();
                        self.volume_fade.fast_forward();
                        self.panning.fast_forward();
                    }
                }
            }
        }

        let volume = self.shared.immediate_volume.swap(0, Ordering::AcqRel);
        if volume >> 32 != 0 && self.state == PlaybackState::Playing {
            self.retarget_volume(Volume(f32::from_bits(volume as u32)), Tween::IMMEDIATE);
        }
        // Retirement must win even when the queued commands include a fade or
        // the source is paused. No source samples are consumed after this point.
        if self.shared.immediate_stop.load(Ordering::Acquire) {
            self.state = PlaybackState::Stopped;
        }

        self.shared.wait_status.store(
            self.wait_status().bits(),
            std::sync::atomic::Ordering::SeqCst,
        );
        // TODO: compute the amplitude
        let samples = self.sample_provider.source.current_samples_position();
        self.shared
            .position_samples
            .store(samples, std::sync::atomic::Ordering::SeqCst);
        let position = samples as u64 * 1000 / self.sample_provider.source.sample_rate() as u64;
        self.shared.position.store(
            position.try_into().unwrap(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        for frame in out {
            *frame = self.process_frame(dt);
        }
    }

    fn finished(&self) -> bool {
        let result = self.state == PlaybackState::Stopped;
        if result {
            debug!("Audio sound finished");
            self.shared
                .wait_status
                .store(self.wait_status().bits(), Ordering::SeqCst);
        }
        result
    }
}

impl<S: AudioFrameSource + Send> AudioSound<S> {
    fn process_frame(&mut self, dt: f64) -> Frame {
        let dt_ticks = Ticks::from_seconds(dt as f32);

        // update tweeners
        self.volume.update(dt_ticks);
        self.panning.update(dt_ticks);
        self.playback_rate.update(dt_ticks);
        self.volume_fade.update(dt_ticks);

        if self.state == PlaybackState::Stopping && self.volume_fade.is_idle() {
            self.state = PlaybackState::Stopped
        }

        // Native pause suspends the source, not the control interpolators.
        // Stop must still finish and release a paused sound.
        if self.state == PlaybackState::Stopped
            || self.sample_provider.source.inner().is_failed()
            || self
                .shared
                .paused
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Frame::ZERO;
        }
        let mut f = self
            .sample_provider
            .next(dt * f64::from(self.playback_rate.value()));

        self.shared.drained_position_samples.store(
            self.sample_provider.resampler.drained_frame_index(),
            Ordering::Release,
        );

        if self.sample_provider.reached_eof && self.sample_provider.resampler.outputting_silence() {
            self.state = PlaybackState::Stopped;
        }

        let pan = self.panning.value();
        let volume = self.volume_fade.value() * self.volume.value();

        f *= volume;
        if pan != 0.0 {
            let pan = (pan + 1.0) * 0.5;
            f = Frame::new(f.left * (1.0 - pan).sqrt(), f.right * pan.sqrt()) * SQRT_2
        }

        f
    }
}

#[cfg(test)]
#[path = "repeat_count_tests.rs"]
mod repeat_count_tests;

#[cfg(test)]
mod reconstruction_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioSettings;
    use crate::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use kira::{AudioManager, AudioManagerSettings};
    use shin_core::format::audio::AudioBuffer;

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
    fn reference_rom_nxa_loop_end_and_nonrepeat_last_sample() {
        use shin_core::{
            format::{
                audio::{AudioDecoder, read_audio},
                rom::RomReader,
            },
            primitives::stateless_reader::StatelessFile,
        };
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        for path in [
            "/bgm/og_cross_op.nxa",
            "/bgm/og_x_op.nxa",
            "/bgm/umib_197.nxa",
            "/bgm/umib_198.nxa",
            "/bgm/umib_243.nxa",
            "/bgm/umib_250.nxa",
            "/se/umise_082.nxa",
            "/se/umilse_052.nxa",
            "/se/umise_088.nxa",
        ] {
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let file = Arc::new(read_audio(&bytes).unwrap());
            let info = file.info().clone();
            let mut looping = SampleProvider::new(
                AudioDecoder::new_pcm16(file.clone()).unwrap(),
                Some(info.loop_start),
            );
            looping.source.samples_seek(info.loop_end - 3).unwrap();
            for position in info.loop_end - 2..=info.loop_end {
                looping.push_frame_to_resampler();
                assert_eq!(looping.source.current_samples_position(), position);
            }
            for position in info.loop_start + 1..=info.loop_start + 4 {
                looping.push_frame_to_resampler();
                assert_eq!(
                    looping.source.current_samples_position(),
                    position,
                    "Authored loop end {path}"
                );
                assert!(!looping.reached_eof);
                assert!(!looping.source.inner().is_failed());
            }
            let mut counted = SampleProvider::with_loop_count(
                AudioDecoder::new_pcm16(file.clone()).unwrap(),
                Some(info.loop_start),
                2,
            );
            let mut reference = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
            counted.source.samples_seek(info.loop_end - 2).unwrap();
            reference.samples_seek(info.loop_end - 2).unwrap();
            for _ in 0..2 {
                counted.push_frame_to_resampler();
                let frame = counted.resampler.latest_frame();
                assert_eq!(Some((frame.left, frame.right)), reference.read_sample());
            }
            reference.samples_seek(info.loop_start).unwrap();
            for _ in 0..4 {
                counted.push_frame_to_resampler();
                let frame = counted.resampler.latest_frame();
                assert_eq!(Some((frame.left, frame.right)), reference.read_sample());
            }
            assert_eq!(counted.loop_count, 1);
            counted.source.samples_seek(info.loop_end - 2).unwrap();
            reference.samples_seek(info.loop_end - 2).unwrap();
            for _ in info.loop_end - 2..info.num_samples {
                counted.push_frame_to_resampler();
                let frame = counted.resampler.latest_frame();
                assert_eq!(Some((frame.left, frame.right)), reference.read_sample());
                assert!(!counted.reached_eof);
            }
            counted.push_frame_to_resampler();
            assert!(counted.reached_eof, "Final finite-repeat tail {path}");
            let mut once = SampleProvider::new(AudioDecoder::new_pcm16(file).unwrap(), None);
            once.source.samples_seek(info.loop_end - 3).unwrap();
            for position in info.loop_end - 2..=info.num_samples {
                once.push_frame_to_resampler();
                assert_eq!(once.source.current_samples_position(), position);
                assert!(!once.reached_eof);
            }
            once.push_frame_to_resampler();
            assert!(once.reached_eof);
            assert_eq!(once.source.current_samples_position(), info.num_samples);
            println!(
                "NXA authored loop boundary {path}: start{}, end{}, nonrepeat end{}",
                info.loop_start, info.loop_end, info.num_samples
            );
        }
    }

    pub(super) struct ConstantSource {
        sample_rate: u32,
        position: u32,
        length: u32,
        ramp: bool,
    }
    impl AudioFrameSource for ConstantSource {
        fn max_frame_size(&self) -> usize {
            64
        }
        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
            let end = (self.position + 64).min(self.length);
            let read = self.position < end;
            for sample in self.position..end {
                let value = if self.ramp {
                    0.1 + sample as f32 * 0.00001
                } else {
                    0.25
                };
                destination.push((value, value));
            }
            self.position = end;
            read
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position.min(self.length);
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    pub(super) fn manager() -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap()
    }

    pub(super) fn sound(length: u32, pan: f32) -> AudioData<ConstantSource> {
        AudioData {
            source: ConstantSource {
                sample_rate: 1000,
                position: 0,
                length,
                ramp: false,
            },
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,

                fade_in: Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume: Volume::default(),
                pan: Pan(pan),
            },
        }
    }

    #[test]
    fn batched_processing_preserves_pcm_pause_suspension_and_retirement() {
        fn render(batch: usize) -> Vec<f32> {
            let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
                backend_settings: MockBackendSettings { sample_rate: 1000 },
                internal_buffer_size: batch,
                ..Default::default()
            })
            .unwrap();
            let suspension = crate::AudioSuspension::default();
            let mut data = sound(10_000, -0.25);
            data.source.ramp = true;
            data.settings.volume = Volume(0.4);
            data.settings.playback_rate = 1.5;
            let handle = manager.play(suspension.wrap(data)).unwrap();
            let mut pcm = Vec::new();
            for _ in 0..256 / batch {
                manager.backend_mut().on_start_processing();
                manager.backend_mut().process();
                pcm.extend_from_slice(manager.backend_mut().samples());
            }
            for platform_pause in [false, true] {
                handle.set_paused(!platform_pause);
                suspension.set_suspended(platform_pause);
                let position = handle.position_samples();
                for _ in 0..3 {
                    manager.backend_mut().on_start_processing();
                    manager.backend_mut().process();
                    assert!(
                        manager
                            .backend_mut()
                            .samples()
                            .iter()
                            .all(|&sample| sample == 0.0)
                    );
                }
                let frozen = handle.position_samples();
                manager.backend_mut().on_start_processing();
                manager.backend_mut().process();
                assert_eq!(handle.position_samples(), frozen);
                assert!(frozen >= position);
            }
            suspension.set_suspended(false);
            handle.set_paused(false);
            handle.stop_immediate();
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
            assert!(
                manager
                    .backend_mut()
                    .samples()
                    .iter()
                    .all(|&sample| sample == 0.0)
            );
            pcm
        }
        let per_sample = render(1);
        assert!(per_sample.iter().any(|sample| sample.abs() > 0.0));
        for batch in [16, 128] {
            assert_eq!(render(batch), per_sample);
        }
    }

    pub(super) fn advance(manager: &mut AudioManager<MockBackend>, samples: u32) -> Frame {
        let backend = manager.backend_mut();
        let mut frame = Frame::ZERO;
        for _ in 0..samples {
            backend.on_start_processing();
            frame = backend.process();
        }
        frame
    }

    #[test]
    fn microsecond_clock_retains_sample_fraction_and_pause_state() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut data = sound(2000, 0.0);
        data.source.sample_rate = 48_000;
        let handle = manager.play(data).unwrap();
        advance(&mut manager, 800);
        // Positions are published at the start of the next processing block.
        // Publish the completed block without consuming another source sample.
        manager.backend_mut().on_start_processing();
        assert_eq!(handle.position_samples(), 800);
        assert_eq!(handle.position_millis(), 16);
        assert_eq!(handle.position_micros(), 16_666);
        handle.set_paused(true);
        advance(&mut manager, 480);
        assert_eq!(handle.position_micros(), 16_666);
        handle.set_paused(false);
        advance(&mut manager, 1);
        manager.backend_mut().on_start_processing();
        assert_eq!(handle.position_samples(), 801);
        assert_eq!(handle.position_micros(), 16_687);
    }

    fn corrupt_nxa_file() -> Arc<crate::AudioFile> {
        use shin_core::format::audio::read_audio;
        // Valid NXA container with an invalid code3 Opus packet (zero frames).
        let mut bytes = Vec::from(*b"NXA1");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&50u32.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        for value in [1u16, 2, 960, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [960u32, 0, 960] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.resize(48, 0);
        bytes.extend_from_slice(&[0xff, 0]);
        Arc::new(read_audio(&bytes).unwrap())
    }

    #[test]
    #[ignore = "requires the Loopback Audio system output device"]
    fn reference_loopback_nxa_failure_and_voice_segment_ownership() {
        use crate::{AudioManager, AudioSegment};
        use shin_core::format::audio::AudioDecoder;
        let manager = AudioManager::new_for_tests();
        fn verify<S: AudioFrameSource + Send + 'static>(
            manager: &AudioManager,
            source: S,
            loop_start: Option<u32>,
        ) {
            let mut settings = sound(1000, 0.0).settings;
            settings.loop_start = loop_start;
            let mut handle = manager.play(AudioData { source, settings });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !handle.get_wait_status().is_empty() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            for _ in 0..32 {
                handle.set_paused(false);
                handle.set_volume(Volume(0.5), Tween::IMMEDIATE).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
                assert_eq!(handle.position_samples(), 0);
                assert_eq!(handle.get_wait_status(), AudioWaitStatus::empty());
                assert_eq!(
                    Arc::strong_count(&handle.shared),
                    2,
                    "Failed callback voice must remain owned"
                );
                assert_eq!(manager.reference_sound_count(), 1);
            }
            handle
                .stop(Tween::linear(Ticks::from_seconds(0.02)))
                .unwrap();
            // Kira defers destruction through its unused-resource queue. Pool
            // membership, rather than Arc destruction, measures retirement.
            while manager.reference_sound_count() > 0 {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(handle.position_samples(), 0);
        }
        let file = corrupt_nxa_file();
        for loop_start in [None, Some(0)] {
            verify(
                &manager,
                AudioDecoder::new_pcm16(file.clone()).unwrap(),
                loop_start,
            );
            verify(
                &manager,
                AudioSegment::new(AudioDecoder::new_pcm16(file.clone()).unwrap(), 0, None).unwrap(),
                loop_start,
            );
        }
        println!(
            "Loopback Audio: NXA and voice-segment decode errors freeze samples, clear waits, retain ownership through controls, and retire on Stop"
        );
    }

    #[test]
    fn corrupt_nxa_and_voice_segment_retain_failed_voice_until_stop() {
        use crate::AudioSegment;
        use shin_core::format::audio::AudioDecoder;
        let file = corrupt_nxa_file();
        fn verify<S: AudioFrameSource + Send + 'static>(source: S, loop_start: Option<u32>) {
            let mut manager = manager();
            let mut settings = sound(1000, 0.0).settings;
            settings.loop_start = loop_start;
            let mut handle = manager.play(AudioData { source, settings }).unwrap();
            assert_eq!(advance(&mut manager, 10), Frame::ZERO);
            for _ in 0..32 {
                handle.set_paused(false);
                handle.set_volume(Volume(0.5), Tween::IMMEDIATE).unwrap();
                assert_eq!(advance(&mut manager, 32), Frame::ZERO);
                assert_eq!(handle.position_samples(), 0);
                assert_eq!(handle.get_wait_status(), AudioWaitStatus::empty());
                assert_eq!(manager.main_track().num_sounds(), 1);
            }
            handle
                .stop(Tween::linear(Ticks::from_seconds(0.02)))
                .unwrap();
            assert_eq!(advance(&mut manager, 30), Frame::ZERO);
            assert_eq!(manager.main_track().num_sounds(), 0);
            assert_eq!(handle.position_samples(), 0);
            assert_eq!(handle.get_wait_status(), AudioWaitStatus::empty());
        }
        for loop_start in [None, Some(0)] {
            verify(AudioDecoder::new_pcm16(file.clone()).unwrap(), loop_start);
            verify(
                AudioSegment::new(AudioDecoder::new_pcm16(file.clone()).unwrap(), 0, None).unwrap(),
                loop_start,
            );
        }
    }

    #[test]
    fn decoder_failure_freezes_buffered_pcm_but_keeps_controls_and_retirement_alive() {
        struct FailedSource {
            source: ConstantSource,
            failed: Arc<AtomicBool>,
        }
        impl AudioFrameSource for FailedSource {
            fn is_failed(&self) -> bool {
                self.failed.load(std::sync::atomic::Ordering::Acquire)
            }
            fn max_frame_size(&self) -> usize {
                self.source.max_frame_size()
            }
            fn sample_rate(&self) -> u32 {
                self.source.sample_rate()
            }
            fn pre_skip(&self) -> u32 {
                self.source.pre_skip()
            }
            fn pre_roll(&self) -> u32 {
                self.source.pre_roll()
            }
            fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
                self.source.read_frame(destination)
            }
            fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
                self.source.samples_seek(position)
            }
            fn current_sample_position(&self) -> u32 {
                self.source.current_sample_position()
            }
        }
        let mut manager = manager();
        let failed = Arc::new(AtomicBool::new(false));
        let data = sound(1000, 0.0);
        let mut handle = manager
            .play(AudioData {
                source: FailedSource {
                    source: data.source,
                    failed: failed.clone(),
                },
                settings: data.settings,
            })
            .unwrap();
        assert!(advance(&mut manager, 100).left > 0.0);
        failed.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(advance(&mut manager, 1), Frame::ZERO);
        let frozen = handle.position_samples();
        assert_eq!(frozen, 100);
        for _ in 0..32 {
            handle.set_paused(false);
            handle.set_volume(Volume(0.5), Tween::IMMEDIATE).unwrap();
            assert_eq!(advance(&mut manager, 64), Frame::ZERO);
            assert_eq!(handle.position_samples(), frozen);
            assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
            assert_eq!(manager.main_track().num_sounds(), 1);
        }
        handle
            .stop(Tween::linear(Ticks::from_seconds(0.02)))
            .unwrap();
        assert_eq!(advance(&mut manager, 10), Frame::ZERO);
        assert_eq!(manager.main_track().num_sounds(), 1);
        assert_eq!(advance(&mut manager, 20), Frame::ZERO);
        assert_eq!(manager.main_track().num_sounds(), 0);
        assert_eq!(handle.position_samples(), frozen);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::empty());
    }

    #[test]
    fn pause_freezes_source_and_resumes_the_same_pcm_sequence() {
        let mut paused_manager = manager();
        let mut control_manager = manager();
        let mut data = sound(1000, 0.0);
        data.source.ramp = true;
        let paused_handle = paused_manager.play(data).unwrap();
        let mut data = sound(1000, 0.0);
        data.source.ramp = true;
        let control_handle = control_manager.play(data).unwrap();
        assert_eq!(
            advance(&mut paused_manager, 100),
            advance(&mut control_manager, 100)
        );
        paused_handle.set_paused(true);
        for _ in 0..100 {
            paused_handle.set_paused(true);
        }
        // Position publication happens at the start of a processing block.
        assert_eq!(advance(&mut paused_manager, 1), Frame::ZERO);
        let position = paused_handle.position_millis();
        assert_eq!(advance(&mut paused_manager, 499), Frame::ZERO);
        assert_eq!(paused_handle.position_millis(), position);
        assert!(
            paused_handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        paused_handle.set_paused(false);
        for _ in 0..100 {
            assert_eq!(
                advance(&mut paused_manager, 1),
                advance(&mut control_manager, 1)
            );
            assert_eq!(
                paused_handle.position_millis(),
                control_handle.position_millis()
            );
        }
    }

    #[test]
    fn platform_suspension_freezes_existing_and_new_sounds_without_changing_local_pause() {
        let suspension = crate::AudioSuspension::default();
        let mut suspended_manager = manager();
        let mut baseline = manager();
        let mut data = sound(5000, 0.0);
        data.source.ramp = true;
        let mut original = suspended_manager.play(suspension.wrap(data)).unwrap();
        let mut data = sound(5000, 0.0);
        data.source.ramp = true;
        let mut baseline_handle = baseline.play(data).unwrap();
        for _ in 0..100 {
            assert_eq!(
                advance(&mut suspended_manager, 1),
                advance(&mut baseline, 1)
            );
        }
        original
            .set_volume(Volume(0.5), Tween::linear(Ticks::from_seconds(0.1)))
            .unwrap();
        baseline_handle
            .set_volume(Volume(0.5), Tween::linear(Ticks::from_seconds(0.1)))
            .unwrap();
        suspension.set_suspended(true);
        suspended_manager.backend_mut().on_start_processing();
        let frozen = original.position_millis();
        let mut data = sound(5000, 0.0);
        data.settings.volume = Volume(0.0);
        let new_active = suspended_manager.play(suspension.wrap(data)).unwrap();
        let mut data = sound(5000, 0.0);
        data.settings.volume = Volume(0.0);
        data.settings.paused = true;
        let new_paused = suspended_manager.play(suspension.wrap(data)).unwrap();
        for _ in 0..500 {
            assert_eq!(advance(&mut suspended_manager, 1), Frame::ZERO);
        }
        assert_eq!(original.position_millis(), frozen);
        assert_eq!(new_active.position_millis(), 0);
        assert_eq!(new_paused.position_millis(), 0);
        for handle in [&original, &new_active, &new_paused] {
            assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
        }
        suspension.set_suspended(false);
        for _ in 0..100 {
            assert_eq!(
                advance(&mut suspended_manager, 1),
                advance(&mut baseline, 1)
            );
            assert_eq!(
                original.position_millis(),
                baseline_handle.position_millis()
            );
        }
        assert!(new_active.position_millis() > 0);
        assert_eq!(new_paused.position_millis(), 0);
        new_paused.set_paused(false);
        advance(&mut suspended_manager, 100);
        assert!(new_paused.position_millis() > 0);
    }

    #[test]
    fn initially_paused_sound_accepts_parameters_and_stop_without_consuming_source() {
        let mut manager = manager();
        let mut data = sound(1000, 0.0);
        data.settings.paused = true;
        let mut handle = manager.play(data).unwrap();
        assert_eq!(advance(&mut manager, 500), Frame::ZERO);
        assert_eq!(handle.position_millis(), 0);
        handle
            .set_parameters(
                Volume(0.4),
                Pan(-0.5),
                2.0,
                Tween::linear(Ticks::from_f32(4.0)),
            )
            .unwrap();
        assert_eq!(advance(&mut manager, 100), Frame::ZERO);
        assert_eq!(handle.position_millis(), 0);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        handle.stop(Tween::linear(Ticks::from_f32(4.0))).unwrap();
        assert_eq!(advance(&mut manager, 100), Frame::ZERO);
        assert!(handle.get_wait_status().is_empty());
        handle.set_paused(false);
        assert_eq!(advance(&mut manager, 100), Frame::ZERO);
        assert_eq!(handle.position_millis(), 0);
    }

    #[test]
    fn playback_rate_changes_pcm_slope_source_position_and_eof() {
        for rate in [0.01, 0.5, 1.0, 1.5, 2.0, 10.0] {
            let mut manager = manager();
            let mut data = sound(10000, 0.0);
            data.source.ramp = true;
            data.settings.playback_rate = rate;
            let handle = manager.play(data).unwrap();
            let first = advance(&mut manager, 800);
            let second = advance(&mut manager, 40);
            let expected_delta = 40.0 * rate * 0.00001;
            assert!(
                (second.left - first.left - expected_delta).abs() < 0.000002,
                "rate{rate}: {first:?}->{second:?}"
            );
            assert!((second.right - first.right - expected_delta).abs() < 0.000002);
            assert!((handle.position_millis() as f32 - 840.0 * rate).abs() <= rate + 1.0);

            let mut manager = self::manager();
            let mut data = sound(100, 0.0);
            data.settings.playback_rate = rate;
            let handle = manager.play(data).unwrap();
            advance(&mut manager, (50.0 / rate) as u32);
            assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
            advance(&mut manager, (60.0 / rate).ceil() as u32 + 5);
            assert!(
                handle.get_wait_status().is_empty(),
                "rate{rate} must finish according to source time"
            );
        }
    }

    #[test]
    fn playback_rate_preserves_loops_and_wall_clock_fades() {
        for rate in [0.01, 0.5, 2.0, 10.0] {
            let mut manager = manager();
            let mut data = sound(100, 0.0);
            data.settings.playback_rate = rate;
            data.settings.loop_start = Some(20);
            let mut handle = manager.play(data).unwrap();
            advance(&mut manager, (200.0 / rate).ceil() as u32);
            assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
            assert!(handle.position_millis() <= 100);
            let frame = advance(&mut manager, 8);
            assert!((frame.left - 0.25).abs() < 0.00001);
            handle
                .stop(Tween::linear(Ticks::from_millis(20.0)))
                .unwrap();
            advance(&mut manager, 5);
            assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
            advance(&mut manager, 20);
            assert!(
                handle.get_wait_status().is_empty(),
                "Fade duration must remain20ms at rate{rate}"
            );
        }
    }

    #[test]
    fn rate_tween_integrates_source_time_and_retargets_from_current_speed() {
        let mut manager = manager();
        let mut handle = manager.play(sound(10000, 0.0)).unwrap();
        advance(&mut manager, 100);
        let before = handle.position_millis();
        handle
            .set_playback_rate(3.0, Tween::linear(Ticks::from_millis(20.0)))
            .unwrap();
        advance(&mut manager, 10);
        assert!(
            handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAY_SPEED_TWEENING)
        );
        let half = handle.position_millis();
        assert!((half as i32 - before as i32 - 15).abs() <= 2);
        advance(&mut manager, 12);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        assert!((handle.position_millis() as i32 - before as i32 - 47).abs() <= 3);
        handle
            .set_playback_rate(10.0, Tween::linear(Ticks::from_millis(100.0)))
            .unwrap();
        advance(&mut manager, 5);
        assert!(
            handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAY_SPEED_TWEENING)
        );
        handle.set_playback_rate(0.5, Tween::IMMEDIATE).unwrap();
        advance(&mut manager, 4);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        let before = handle.position_millis();
        advance(&mut manager, 40);
        assert!((handle.position_millis() as i32 - before as i32 - 20).abs() <= 1);
        assert!(handle.set_playback_rate(0.0, Tween::IMMEDIATE).is_err());
        assert!(
            handle
                .set_playback_rate(f32::NAN, Tween::IMMEDIATE)
                .is_err()
        );
    }

    #[test]
    fn signed_pan_reaches_both_channels_without_nan() {
        for (pan, left, right) in [
            (-1.0, 0.25 * SQRT_2, 0.0),
            (0.0, 0.25, 0.25),
            (1.0, 0.0, 0.25 * SQRT_2),
        ] {
            let mut manager = manager();
            manager.play(sound(100, pan)).unwrap();
            let frame = advance(&mut manager, 16);
            assert!(
                (frame.left - left).abs() < 0.00001,
                "pan={pan}, frame={frame:?}"
            );
            assert!(
                (frame.right - right).abs() < 0.00001,
                "pan={pan}, frame={frame:?}"
            );
        }
    }

    #[test]
    fn wait_for_fade_out_and_clear_interpolators_at_eof() {
        let mut manager = manager();
        let mut handle = manager.play(sound(100, 0.0)).unwrap();
        handle
            .stop(Tween::linear(Ticks::from_millis(10.0)))
            .unwrap();
        advance(&mut manager, 5);
        assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
        advance(&mut manager, 20);
        assert!(handle.get_wait_status().is_empty());

        let mut handle = manager.play(sound(16, 0.0)).unwrap();
        handle
            .set_volume(Volume(0.5), Tween::linear(Ticks::from_seconds(1.0)))
            .unwrap();
        handle
            .set_panning(Pan(-1.0), Tween::linear(Ticks::from_seconds(1.0)))
            .unwrap();
        advance(&mut manager, 5);
        assert!(
            handle
                .get_wait_status()
                .contains(AudioWaitStatus::VOLUME_TWEENING | AudioWaitStatus::PANNING_TWEENING)
        );
        advance(&mut manager, 32);
        assert!(handle.get_wait_status().is_empty());
    }

    #[test]
    fn stop_freezes_an_in_flight_volume_tween_before_fading() {
        let mut manager = manager();
        let mut data = sound(10000, 0.0);
        data.settings.volume = Volume(0.2);
        let mut handle = manager.play(data).unwrap();
        handle
            .set_volume(Volume(1.0), Tween::linear(Ticks::from_millis(100.0)))
            .unwrap();
        let start = advance(&mut manager, 50);
        assert!((start.left - 0.15).abs() < 0.00001);
        handle
            .stop(Tween::linear(Ticks::from_millis(50.0)))
            .unwrap();
        let half = advance(&mut manager, 25);
        assert!(
            (half.left - 0.075).abs() < 0.00001,
            "Stop must fade the current gain,without continuing toward its old target: {half:?}"
        );
        advance(&mut manager, 30);
        assert!(handle.get_wait_status().is_empty());
    }

    #[test]
    fn volume_retarget_during_fade_in_starts_at_current_audible_gain() {
        for grouped in [false, true] {
            let mut manager = manager();
            let mut data = sound(10000, 0.0);
            data.settings.volume = Volume(0.8);
            data.settings.fade_in = Tween::linear(Ticks::from_millis(100.0));
            let mut handle = manager.play(data).unwrap();
            let current = advance(&mut manager, 25);
            assert!((current.left - 0.05).abs() < 0.00001);
            assert!(
                handle
                    .get_wait_status()
                    .contains(AudioWaitStatus::VOLUME_TWEENING)
            );
            let tween = Tween::linear(Ticks::from_millis(50.0));
            if grouped {
                handle
                    .set_parameters(Volume(0.6), Pan(0.0), 1.0, tween)
                    .unwrap();
            } else {
                handle.set_volume(Volume(0.6), tween).unwrap();
            }
            let half = advance(&mut manager, 25);
            assert!(
                (half.left - 0.1).abs() < 0.00001,
                "Retarget must interpolate currentgain0.2->0.6: {half:?}"
            );
            let end = advance(&mut manager, 30);
            assert!((end.left - 0.15).abs() < 0.00001);
            assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        }
    }

    #[test]
    fn queued_volume_preserves_active_ramp_and_replacement_clears_it() {
        let mut manager = manager();
        let mut data = sound(10000, 0.0);
        data.settings.volume = Volume(0.0);
        let mut handle = manager.play(data).unwrap();
        handle
            .enqueue_volume(Volume(1.0), Tween::linear(Ticks::from_millis(100.0)))
            .unwrap();
        handle
            .enqueue_volume(Volume(0.0), Tween::linear(Ticks::from_millis(100.0)))
            .unwrap();
        let half = advance(&mut manager, 50);
        assert!(
            (half.left - 0.125).abs() < 0.00001,
            "first ramp was replaced: {half:?}"
        );
        let descending = advance(&mut manager, 100);
        assert!(
            (descending.left - 0.125).abs() < 0.00001,
            "queued second ramp did not run: {descending:?}"
        );
        assert!(
            handle
                .get_wait_status()
                .contains(AudioWaitStatus::VOLUME_TWEENING)
        );
        handle.set_volume(Volume(0.8), Tween::IMMEDIATE).unwrap();
        let replaced = advance(&mut manager, 5);
        assert!((replaced.left - 0.2).abs() < 0.00001);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        advance(&mut manager, 100);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
    }

    #[test]
    fn resume_matching_repeat_during_stop_preserves_gain_and_source_position() {
        let mut manager = manager();
        let mut data = sound(10000, 0.0);
        data.settings.volume = Volume(0.8);
        let mut handle = manager.play(data).unwrap();
        advance(&mut manager, 50);
        handle
            .stop(Tween::linear(Ticks::from_millis(100.0)))
            .unwrap();
        let fading = advance(&mut manager, 50);
        assert!((fading.left - 0.1).abs() < 0.00001);
        let position = handle.position_samples();
        handle
            .resume_parameters(
                Volume(0.6),
                Pan(0.0),
                1.0,
                Tween::linear(Ticks::from_millis(100.0)),
            )
            .unwrap();
        let half = advance(&mut manager, 50);
        assert!((half.left - 0.125).abs() < 0.00001);
        assert!(handle.position_samples() > position);
        let end = advance(&mut manager, 100);
        assert!((end.left - 0.15).abs() < 0.00001);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        handle.stop(Tween::IMMEDIATE).unwrap();
        advance(&mut manager, 5);
        assert!(handle.get_wait_status().is_empty());
        // A source already removed by the backend cannot be resurrected.
        handle
            .resume_parameters(Volume(1.0), Pan(0.0), 1.0, Tween::IMMEDIATE)
            .unwrap();
        advance(&mut manager, 5);
        assert!(handle.get_wait_status().is_empty());
    }
}
