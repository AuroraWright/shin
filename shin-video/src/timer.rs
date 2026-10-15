use shin_audio::AudioHandle;
use shin_core::{time::Ticks, vm::command::types::Volume};

pub struct IndependentTimer {
    /// How many time units are there in one second
    time_base: u32,
    /// Accumulate elapsed engine ticks before quantizing to the movie timescale.
    /// Rounding every frame loses time when the timescale is not divisible by 60.
    elapsed_ticks: f64,
}

impl IndependentTimer {
    pub fn new(time_base: u32) -> IndependentTimer {
        IndependentTimer {
            time_base,
            elapsed_ticks: 0.0,
        }
    }

    pub fn update(&mut self, delta_time: Ticks) -> u64 {
        let ticks = f64::from(delta_time.as_f32());
        if ticks > 0.0 {
            self.elapsed_ticks += ticks;
        }
        self.time()
    }

    pub fn time(&self) -> u64 {
        (self.elapsed_ticks * f64::from(self.time_base) / f64::from(Ticks::TICKS_PER_SECOND)) as u64
    }
}

pub struct AudioTiedTimer {
    time_base: u32,
    audio_handle: AudioHandle,
    stopped: bool,
}

impl AudioTiedTimer {
    pub fn new(time_base: u32, audio_handle: AudioHandle) -> AudioTiedTimer {
        AudioTiedTimer {
            time_base,
            audio_handle,
            stopped: false,
        }
    }

    pub fn update(&mut self, _delta_time: Ticks) -> u64 {
        self.time()
    }

    pub fn time(&self) -> u64 {
        // Native bf148 selects cumulative audio position while an audio voice
        // exists. Movie audio supplies silence after EOF to keep this advancing.
        u64::from(self.audio_handle.position_samples()) * u64::from(self.time_base)
            / u64::from(self.audio_handle.sample_rate())
    }
}

pub enum Timer {
    Independent(IndependentTimer),
    AudioTiedTimer(AudioTiedTimer),
}

impl Timer {
    pub fn new_independent(time_base: u32) -> Timer {
        Timer::Independent(IndependentTimer::new(time_base))
    }

    pub fn new_audio_tied(time_base: u32, audio_handle: AudioHandle) -> Timer {
        Timer::AudioTiedTimer(AudioTiedTimer::new(time_base, audio_handle))
    }

    pub fn update(&mut self, delta_time: Ticks) -> u64 {
        match self {
            Timer::Independent(timer) => timer.update(delta_time),
            Timer::AudioTiedTimer(timer) => timer.update(delta_time),
        }
    }

    pub fn time(&self) -> u64 {
        match self {
            Timer::Independent(timer) => timer.time(),
            Timer::AudioTiedTimer(timer) => timer.time(),
        }
    }

    // Why is this a function on a timer?
    // because timer keeps the audio handle :/
    pub fn set_audio_volume(&mut self, volume: Volume) {
        let Timer::AudioTiedTimer(timer) = self else {
            return;
        };

        if timer.stopped {
            return;
        }
        timer.audio_handle.set_volume_immediate(volume);
    }

    pub fn stop_audio(&mut self) {
        if let Timer::AudioTiedTimer(timer) = self {
            if !timer.stopped {
                timer.audio_handle.stop_immediate();
                timer.stopped = true;
            }
        }
    }

    pub fn start_audio(&self) {
        if let Timer::AudioTiedTimer(timer) = self {
            if !timer.stopped {
                timer.audio_handle.set_paused(false);
            }
        }
    }

    pub fn pause_audio(&self) {
        if let Timer::AudioTiedTimer(timer) = self {
            if !timer.stopped {
                timer.audio_handle.set_paused(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{AudioManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use shin_audio::{AudioData, AudioSettings};
    use shin_core::{
        format::audio::{AudioBuffer, AudioFrameSource},
        time::Tween,
        vm::command::types::{AudioWaitStatus, Pan},
    };

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_elapsed_clock_baseline() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, instruction) in [
            (0xbdfa0, 0xf9455260),     // video error handler selects video decoder
            (0xbdfa8, 0xf9404108),     // query video decoder error detail
            (0xbdfc4, 0x39310e68),     // video error sets backend terminal failure flag
            (0xbdfe0, 0xf9400d08),     // video error pauses retained voice
            (0xbdfec, 0x9401225d),     // video error freezes independent clock
            (0xbdff0, 0xb90c467f),     // video error clears actual-playing flag
            (0xbe894, 0xf945be60),     // audio error handler selects audio decoder
            (0xbe89c, 0xf9404108),     // query audio decoder error detail
            (0xbe8b8, 0x39310e68),     // audio error sets same terminal failure flag
            (0xbe8d4, 0xf9400d08),     // audio error pauses retained voice
            (0xbe8e0, 0x94012020),     // audio error freezes independent clock
            (0xbe8e4, 0xb90c467f),     // audio error clears actual-playing flag
            (0xbd7d4, 0x7140081f),     // input worker distinguishes normal EOF status
            (0xbd7d8, 0x540002c0),     // normal EOF does not enter error state
            (0xbd7ec, 0x39310e68),     // input error sets backend terminal failure flag
            (0xbd7f8, 0x54000121),     // only active clock requires pause
            (0xbd808, 0xf9400d08),     // error selects retained voice Pause virtual slot
            (0xbd80c, 0xd63f0100),     // pause retained audio instead of destroying voice
            (0xbd814, 0x94012453),     // freeze independent clock on input failure
            (0xbd818, 0xb90c467f),     // error clears actual clock-playing flag
            (0xbef00, 0x34000068),     // terminal query checks failure before EOF/repeat
            (0xbef04, 0x320003f4),     // failure immediately returns terminal state
            (0xbdedc, 0x34fff968),     // failure prevents prepared decoder from restarting
            (0xbcbac, 0xf9453269),     // retained movie duration before video duration check
            (0xbcbb4, 0xeb08013f),     // compare retained duration with video durationUs
            (0xbcbb8, 0x54000d6a),     // keep larger existing duration
            (0xbcbbc, 0xf9053268),     // retain longer video duration
            (0xbccf8, 0xf9453269),     // retained movie duration before audio duration check
            (0xbcd00, 0xeb08013f),     // compare retained duration with audio durationUs
            (0xbcd04, 0x5400004a),     // keep larger existing duration
            (0xbcd08, 0xf9053268),     // retain longer audio duration
            (0xbe44c, 0x394393e8),     // audio decoder output flags
            (0xbe450, 0x36100088),     // decoder end-of-stream flag bit2
            (0xbe45c, 0x089ffd09),     // publish audio decoder EOF
            (0xbe468, 0x37001548),     // decoder EOF enters finite duration tail
            (0xbe714, 0xf9453269),     // load maximum media duration
            (0xbe718, 0xeb080128),     // subtract decoder EOF timestamp
            (0xbe71c, 0x540005ed),     // no finite silence when duration covered
            (0xbe730, 0xb98c526a),     // sample rate for missing duration PCM
            (0xbe73c, 0x9acb0908),     // remaining microseconds to samples
            (0xbe740, 0x1b092148),     // combine whole seconds and fractional samples
            (0xbe764, 0x940178f7),     // zero finite EOF-tail PCM
            (0xbe7b4, 0x8b080128),     // tail timestamp includes repeat cycle offset
            (0xbef14, 0x36000308),     // completion requires video decoder EOF
            (0xbef20, 0x360002a8),     // completion requires audio decoder EOF
            (0xbef68, 0xf9453288),     // completion compares maximum declared movie duration
            (0xbef70, 0x54fffca2),     // completion only when clock reaches duration
            (0xbf138, 0xf9468ea0),     // audio voice selects sample clock
            (0xbf13c, 0xb40000a0),     // only missing voice selects independent clock
            (0xbf144, 0xf9401508),     // audio position virtual slot
            (0xbf148, 0xd63f0100),     // read cumulative audio time
            (0xc0068, 0x39408a88),     // repeat gate
            (0xc006c, 0x35000508),     // repeat bypasses silent tail
            (0xc0074, 0x08dffd08),     // audio decoder EOF
            (0xc0078, 0x360004a8),     // require EOF before padding
            (0xc0080, 0xb5000468),     // require empty decoded queue
            (0xc0084, 0xb98c5288),     // audio sample rate
            (0xc0098, 0x9362fd08),     // divide rate by ten
            (0xc00ac, 0x54000302),     // skip padding when queue already filled
            (0xc00d0, 0x9401729c),     // zero the PCM buffer
            (0xc00d8, 0x0b150108),     // add silent samples to queue count
            (0xc00ec, 0xf9401d08),     // audio PCM enqueue slot
            (0xc00f8, 0x320003e3),     // PCM16 format
            (0xc00fc, 0xaa1503e4),     // enqueue silent sample count
            (0xc0100, 0xd63f0100),     // queue silent PCM
            (0x1068ec, 0xf9401e60u32), // paused clock reads retained absolute endpoint
            (0x1068f4, 0x97ffd39f),    // running clock reads engine counter fb770
            (0x106900, 0xf9401a68),    // retained start baseline
            (0x106904, 0xcb080008),    // elapsed is endpoint minus start
            (0x106920, 0xf9401669),    // accumulated prior elapsed
            (0x106928, 0x8b080134),    // result adds retained elapsed, without frame quantization
            (0x106984, 0xf9001e60),    // pause captures counter endpoint
            (0x1069fc, 0xcb090009),    // resume computes paused interval
            (0x106a00, 0x8b080128),    // excludes pause interval from baseline
            (0xbf154, 0x94011ddf),     // no-audio movie reads this clock
            (0x2b9d0, 0xfd403a68),     // audio-backed clock reads cumulative sample position
            (0x2b9fc, 0x1e611800),     // converts total samples to time after accumulation
        ] {
            let actual =
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
            assert_eq!(
                actual, instruction,
                "Native clock instruction at {address:x}"
            );
        }
        assert_eq!(&executable[0x162db9 + 256..0x162db9 + 267], b"durationUs\0");
        assert_eq!(
            &executable[0x15dcc8 + 256..0x15dcc8 + 270],
            b"video : error\0"
        );
        assert_eq!(
            &executable[0x15e036 + 256..0x15e036 + 270],
            b"audio : error\0"
        );
        let units = f64::from_le_bytes(
            executable[0x164cf8 + 256..0x164d00 + 256]
                .try_into()
                .unwrap(),
        );
        assert_eq!(units, 0.000001, "Audio counter reports microseconds");
    }

    #[test]
    fn movie_timescales_preserve_subframe_time_and_long_running_elapsed() {
        for time_base in [1, 1000, 24_000, 30_000, 60_000, 90_000] {
            let mut split = IndependentTimer::new(time_base);
            let mut whole = IndependentTimer::new(time_base);
            for _ in 0..60 {
                split.update(Ticks::from_f32(1.0));
            }
            assert_eq!(split.time(), u64::from(time_base));
            assert_eq!(split.time(), whole.update(Ticks::from_f32(60.0)));
            // Ten minutes of small fractional updates must retain the same clock
            // as one elapsed baseline. In a 1000-unit track the old code lost 24s.
            for _ in 60..36_000 {
                split.update(Ticks::from_f32(1.0));
            }
            assert_eq!(split.time(), u64::from(time_base) * 600);
        }
    }

    #[test]
    fn fractional_movie_updates_match_rational_deadlines_without_rewinding() {
        for time_base in [1, 1000, 60_000, 90_000] {
            let mut timer = IndependentTimer::new(time_base);
            let mut eighths = 0u64;
            for step in [1u64, 3, 4, 7, 8, 13, 32].into_iter().cycle().take(4096) {
                eighths += step;
                let expected = eighths * u64::from(time_base) / 480;
                assert_eq!(timer.update(Ticks::from_f32(step as f32 / 8.0)), expected);
                assert_eq!(timer.update(Ticks::ZERO), expected);
                assert_eq!(timer.update(Ticks::from_f32(-1.0)), expected);
            }
        }
    }

    struct ShortSource {
        position: u32,
    }

    fn stalled_movie_audio(paused: bool) -> (AudioManager<MockBackend>, Timer) {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut handle = manager
            .play(AudioData {
                source: crate::audio::MovieAudioSource::new(ShortSource { position: 0 }),
                settings: AudioSettings {
                    paused,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(0.0),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        // Simulate a callback that has not started (as in the crash report).
        // Also saturate the legacy queue to prove neither new control uses it.
        for _ in 0..8 {
            handle.set_volume(Volume(0.9), Tween::IMMEDIATE).unwrap();
        }
        assert!(handle.set_volume(Volume(0.9), Tween::IMMEDIATE).is_err());
        (manager, Timer::new_audio_tied(60_000, handle))
    }

    #[test]
    fn stalled_movie_callback_coalesces_gain_and_applies_latest_value() {
        let (mut manager, mut timer) = stalled_movie_audio(false);
        for index in 0..4096 {
            timer.set_audio_volume(Volume(index as f32 / 4096.0));
        }
        timer.set_audio_volume(Volume(0.5));
        assert_eq!(timer.time(), 0);
        let mut peak = 0.0f32;
        for _ in 0..50 {
            manager.backend_mut().on_start_processing();
            let sample = manager.backend_mut().process();
            assert_eq!(sample.left, sample.right);
            peak = peak.max(sample.left);
            assert!(sample.left <= 0.125);
        }
        assert_eq!(peak, 0.125, "Only the latest gain should reach the mixer");
        assert!(timer.time() > 0);
    }

    #[test]
    fn stalled_movie_retirement_survives_full_queue_pause_and_unwind() {
        struct MovieOwner(Timer);
        impl Drop for MovieOwner {
            fn drop(&mut self) {
                self.0.stop_audio();
            }
        }
        for paused in [false, true] {
            for unwind in [false, true] {
                let (mut manager, timer) = stalled_movie_audio(paused);
                let mut owner = MovieOwner(timer);
                for _ in 0..4096 {
                    owner.0.set_audio_volume(Volume(1.0));
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut owner = owner;
                    if unwind {
                        panic!("injected movie owner failure");
                    }
                    owner.0.stop_audio();
                    owner.0.start_audio();
                    owner.0.set_audio_volume(Volume(1.0));
                    owner.0.stop_audio();
                }));
                assert_eq!(result.is_err(), unwind);
                for _ in 0..20 {
                    manager.backend_mut().on_start_processing();
                    let sample = manager.backend_mut().process();
                    assert_eq!((sample.left, sample.right), (0.0, 0.0));
                }
                assert_eq!(manager.main_track().num_sounds(), 0, "Retired voice must be removed");
            }
        }
    }
    impl AudioFrameSource for ShortSource {
        fn max_frame_size(&self) -> usize {
            100
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
            if self.position == 100 {
                return false;
            }
            destination.extend(std::iter::repeat_n((0.25, 0.25), 100));
            self.position = 100;
            true
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position.min(100);
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    #[test]
    fn prepared_movie_audio_remains_paused_until_publication_starts_it() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: ShortSource { position: 0 },
                settings: AudioSettings {
                    paused: true,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(1.0),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        let timer = Timer::new_audio_tied(60_000, handle);
        for _ in 0..200 {
            manager.backend_mut().on_start_processing();
            let sample = manager.backend_mut().process();
            assert_eq!((sample.left, sample.right), (0.0, 0.0));
        }
        let Timer::AudioTiedTimer(audio) = &timer else {
            unreachable!()
        };
        assert_eq!(audio.audio_handle.position_millis(), 0);
        timer.start_audio();
        let mut audible = false;
        for _ in 0..50 {
            manager.backend_mut().on_start_processing();
            audible |= manager.backend_mut().process().left > 0.0;
        }
        assert!(audible);
        assert!(audio.audio_handle.position_millis() > 0);
        timer.pause_audio();
        manager.backend_mut().on_start_processing();
        let frozen = audio.audio_handle.position_millis();
        for _ in 0..150 {
            manager.backend_mut().on_start_processing();
            let sample = manager.backend_mut().process();
            assert_eq!((sample.left, sample.right), (0.0, 0.0));
        }
        assert_eq!(audio.audio_handle.position_millis(), frozen);
        assert!(
            audio
                .audio_handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        timer.start_audio();
        let mut resumed = false;
        for _ in 0..20 {
            manager.backend_mut().on_start_processing();
            resumed |= manager.backend_mut().process().left > 0.0;
        }
        assert!(resumed);
        assert!(audio.audio_handle.position_millis() > frozen);
    }

    #[test]
    fn retired_movie_audio_ignores_future_gain_start_and_duplicate_retirement() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: crate::audio::MovieAudioSource::new(ShortSource { position: 0 }),
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(0.0),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        let mut timer = Timer::new_audio_tied(60_000, handle);
        for _ in 0..20 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        // Decoder failure freezes the retained voice; settings still reach it.
        timer.pause_audio();
        manager.backend_mut().on_start_processing();
        let frozen = timer.time();
        for _ in 0..100 {
            timer.set_audio_volume(Volume(0.5));
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        assert_eq!(timer.time(), frozen);
        let Timer::AudioTiedTimer(audio) = &timer else {
            unreachable!()
        };
        assert!(
            audio
                .audio_handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        timer.stop_audio();
        for _ in 0..20 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        let frozen = timer.time();
        // The callback has removed the sound: without retirement state these
        // commands fill its orphaned ring buffer after eight updates and panic.
        for index in 0..100 {
            timer.set_audio_volume(Volume(index as f32 / 100.0));
            timer.stop_audio();
            timer.start_audio();
            timer.pause_audio();
        }
        assert_eq!(timer.time(), frozen);
        let Timer::AudioTiedTimer(audio) = &timer else {
            unreachable!()
        };
        assert!(audio.audio_handle.get_wait_status().is_empty());
        assert_eq!(manager.main_track().num_sounds(), 0);
    }

    #[test]
    fn audio_sample_clock_ignores_game_delta_and_silent_eof_tail_keeps_advancing() {
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(AudioData {
                source: crate::audio::MovieAudioSource::new(ShortSource { position: 0 }),
                settings: AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(0.0),
                    pan: Pan::default(),
                },
            })
            .unwrap();
        let mut timer = AudioTiedTimer::new(60_000, handle);
        assert_eq!(
            timer.update(Ticks::from_seconds(0.4)),
            0,
            "Game elapsed cannot lead the actual audio sample clock"
        );
        for _ in 0..120 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
            timer.update(Ticks::from_seconds(0.001));
        }
        assert!(
            timer
                .audio_handle
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        let end = timer.time();
        assert!(end >= 6_000);
        for _ in 0..1000 {
            manager.backend_mut().on_start_processing();
            assert_eq!(manager.backend_mut().process(), kira::Frame::ZERO);
            let expected = u64::from(timer.audio_handle.position_samples()) * 60;
            assert_eq!(timer.update(Ticks::from_seconds(60.0)), expected);
        }
        assert!(timer.time() >= end + 59_990);
        timer.audio_handle.stop(Tween::IMMEDIATE).unwrap();
        for _ in 0..20 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        assert!(
            timer.audio_handle.get_wait_status().is_empty(),
            "Movie completion releases padded audio"
        );
    }
}
