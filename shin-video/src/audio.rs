use std::{
    io::{Read, Seek},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Context;
use shin_core::format::audio::{AudioBuffer, AudioFrameSource};
use symphonia::core::{
    audio::AudioBufferRef,
    codecs::{CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_AAC},
    formats::Packet,
};

use crate::mp4::Mp4TrackReader;

/// Native nonrepeating movies queue silent PCM after decoder EOF. Their audio
/// voice remains the clock until the movie owner stops it at the video deadline.
pub(crate) struct MovieAudioSource<S> {
    source: S,
    eof: bool,
    decoder_eof: Arc<AtomicBool>,
    silence_samples: u32,
}

impl<S: AudioFrameSource> MovieAudioSource<S> {
    pub(crate) fn new(source: S) -> Self {
        Self {
            source,
            eof: false,
            decoder_eof: Arc::new(AtomicBool::new(false)),
            silence_samples: 0,
        }
    }
    pub(crate) fn decoder_eof(&self) -> Arc<AtomicBool> {
        self.decoder_eof.clone()
    }
    fn silence_frame_size(&self) -> usize {
        (self.source.sample_rate() / 10).max(1) as usize
    }
}

impl<S: AudioFrameSource> AudioFrameSource for MovieAudioSource<S> {
    fn is_failed(&self) -> bool {
        self.source.is_failed()
    }
    fn max_frame_size(&self) -> usize {
        self.source.max_frame_size().max(self.silence_frame_size())
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
        if !self.eof && self.source.read_frame(destination) {
            return true;
        }
        if self.source.is_failed() {
            return false;
        }
        self.eof = true;
        self.decoder_eof.store(true, Ordering::Release);
        let count = self.silence_frame_size();
        destination.extend(std::iter::repeat_n((0.0, 0.0), count));
        self.silence_samples = self.silence_samples.saturating_add(count as u32);
        true
    }
    fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
        let offset = self.source.samples_seek(position)?;
        self.eof = false;
        self.decoder_eof.store(false, Ordering::Release);
        self.silence_samples = 0;
        Ok(offset)
    }
    fn current_sample_position(&self) -> u32 {
        self.source
            .current_sample_position()
            .saturating_add(self.silence_samples)
    }
}

/// One continuous voice across movie cycles. Each cycle restores decoder
/// priming, then supplies finite silence to the common movie deadline.
pub(crate) struct RepeatingMovieAudioSource<S: AudioFrameSource> {
    source: shin_core::format::audio::AudioSource<S>,
    duration_us: u64,
    cycle: u64,
    position: u32,
    eof: bool,
    failed: Arc<AtomicBool>,
}

impl<S: AudioFrameSource> RepeatingMovieAudioSource<S> {
    pub(crate) fn new(source: S, duration_us: u64) -> anyhow::Result<Self> {
        anyhow::ensure!(
            u128::from(duration_us) * u128::from(source.sample_rate()) >= 1_000_000,
            "Repeating movie duration must cover at least one audio sample"
        );
        Ok(Self {
            source: shin_core::format::audio::AudioSource::new(source),
            duration_us,
            cycle: 0,
            position: 0,
            eof: false,
            failed: Arc::new(AtomicBool::new(false)),
        })
    }
    pub(crate) fn decode_failed(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }
    fn boundary(&self) -> u64 {
        (u128::from(self.cycle + 1)
            * u128::from(self.duration_us)
            * u128::from(self.source.sample_rate())
            / 1_000_000) as u64
    }
}

impl<S: AudioFrameSource> AudioFrameSource for RepeatingMovieAudioSource<S> {
    fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire) || self.source.inner().is_failed()
    }
    fn max_frame_size(&self) -> usize {
        1024
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    // The inner sample source applies codec priming on every rewind.
    fn pre_skip(&self) -> u32 {
        0
    }
    fn pre_roll(&self) -> u32 {
        0
    }
    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        if self.is_failed() {
            self.failed.store(true, Ordering::Release);
            return false;
        }
        if u64::from(self.position) == self.boundary() {
            if let Err(error) = self.source.samples_seek(0) {
                tracing::error!(%error, "Rewinding movie AAC failed");
                self.failed.store(true, Ordering::Release);
                return false;
            }
            self.cycle += 1;
            self.eof = false;
        }
        let count = (self.boundary() - u64::from(self.position)).min(1024) as u32;
        for _ in 0..count {
            let sample = if self.eof {
                (0.0, 0.0)
            } else {
                self.source.read_sample().unwrap_or_else(|| {
                    self.eof = true;
                    (0.0, 0.0)
                })
            };
            if self.source.inner().is_failed() {
                self.failed.store(true, Ordering::Release);
                destination.clear();
                return false;
            }
            destination.push(sample);
        }
        self.position += count;
        true
    }
    fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
        anyhow::ensure!(!self.is_failed(), "Repeating movie audio has failed");
        // Movie players only rewind through the source; arbitrary restored movie
        // seeking needs presentation/edit-list recovery before it is exposed.
        anyhow::ensure!(position == 0, "Repeating movie source only supports rewind");
        self.source.samples_seek(0)?;
        self.cycle = 0;
        self.position = 0;
        self.eof = false;
        Ok(0)
    }
    fn current_sample_position(&self) -> u32 {
        self.position
    }
}

pub struct AacFrameSource<S: Read + Seek> {
    track: Mp4TrackReader<S>,
    decoder: symphonia::default::codecs::AacDecoder,
    samples_position: u32,
    pending_frame: Option<Vec<(f32, f32)>>,
    failed: Arc<AtomicBool>,
    sample_rate: u32,
}

impl<S: Read + Seek> AacFrameSource<S> {
    pub fn new(track: Mp4TrackReader<S>) -> anyhow::Result<Self> {
        let mp4a = track
            .get_mp4_track_info(|t| t.trak.mdia.minf.stbl.stsd.mp4a.clone())
            .context("Could not find mp4a atom")?;

        let mut codec_params = CodecParameters::new();
        codec_params
            .for_codec(CODEC_TYPE_AAC)
            .with_sample_rate(mp4a.samplerate.value() as u32);

        if let Some(esds) = mp4a.esds {
            let dec_specific = esds.es_desc.dec_config.dec_specific;

            let dec_specific_data = vec![
                // manually serialize the decoder specific info
                (dec_specific.profile << 3) + (dec_specific.freq_index >> 1),
                (dec_specific.freq_index << 7) + (dec_specific.chan_conf << 3),
            ];

            codec_params.extra_data = Some(dec_specific_data.into_boxed_slice());
        };

        let decoder = symphonia::default::codecs::AacDecoder::try_new(
            &codec_params,
            &DecoderOptions::default(),
        )
        .context("Creating AAC decoder")?;
        let sample_rate = decoder
            .codec_params()
            .sample_rate
            .context("AAC sample rate is missing")?;
        anyhow::ensure!(sample_rate > 0, "AAC sample rate is zero");

        Ok(Self {
            track,
            decoder,
            samples_position: 0,
            pending_frame: None,
            failed: Arc::new(AtomicBool::new(false)),
            sample_rate,
        })
    }

    pub(crate) fn decode_failed(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }

    fn record_failure(&self, error: &anyhow::Error) {
        tracing::error!(%error, "Movie AAC decoder failed");
        self.failed.store(true, Ordering::Release);
    }

    fn decode_frame(&mut self) -> anyhow::Result<Option<Vec<(f32, f32)>>> {
        let Some(frame) = self.track.next_sample()? else {
            return Ok(None);
        };
        let packet =
            Packet::new_from_slice(1, frame.start_time, frame.duration as u64, &frame.bytes);
        let buffer = self.decoder.decode(&packet).context("Decoding AAC frame")?;
        let AudioBufferRef::F32(buffer) = buffer else {
            anyhow::bail!("Unsupported AAC sample format");
        };
        let planes = buffer.planes();
        let samples = match planes.planes() {
            [mono] => mono.iter().map(|&sample| (sample, sample)).collect(),
            [left, right] => left.iter().copied().zip(right.iter().copied()).collect(),
            _ => anyhow::bail!("Unsupported AAC channel count"),
        };
        Ok(Some(samples))
    }
}

impl<S: Read + Seek> AudioFrameSource for AacFrameSource<S> {
    fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    fn max_frame_size(&self) -> usize {
        self.decoder.last_decoded().capacity().max(1024)
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn pre_skip(&self) -> u32 {
        2112
    }

    fn pre_roll(&self) -> u32 {
        2112
    }

    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        if self.is_failed() {
            return false;
        }
        let frame = match self.pending_frame.take() {
            Some(frame) => Some(frame),
            None => match self.decode_frame() {
                Ok(frame) => frame,
                Err(error) => {
                    self.record_failure(&error);
                    return false;
                }
            },
        };
        let Some(frame) = frame else {
            return false;
        };
        self.samples_position += frame.len() as u32;
        destination.extend(frame);
        true
    }

    fn samples_seek(&mut self, sample_position: u32) -> anyhow::Result<u32> {
        anyhow::ensure!(!self.is_failed(), "Movie AAC decoder has failed");
        // Decode the prefix after resetting so the overlapping AAC transform
        // has the same history as sequential playback. Retain the target block
        // and let AudioSource remove its offset and requested preroll.
        self.track.rewind();
        self.decoder.reset();
        self.pending_frame = None;
        self.samples_position = 0;
        loop {
            if sample_position == self.samples_position {
                return Ok(0);
            }
            let frame = match self.decode_frame() {
                Ok(frame) => frame,
                Err(error) => {
                    self.record_failure(&error);
                    return Err(error.context("Seeking movie AAC"));
                }
            };
            let Some(frame) = frame else {
                anyhow::bail!(
                    "AAC seek position {sample_position} exceeds stream length {}",
                    self.samples_position
                );
            };
            let end = self.samples_position + frame.len() as u32;
            if sample_position < end {
                self.pending_frame = Some(frame);
                return Ok(sample_position - self.samples_position);
            }
            self.samples_position = end;
        }
    }

    fn current_sample_position(&self) -> u32 {
        self.samples_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::Mp4;
    use kira::{
        manager::{
            backend::mock::{MockBackend, MockBackendSettings},
            AudioManager, AudioManagerSettings,
        },
        track::TrackId,
    };
    use shin_audio::{AudioData, AudioSettings};
    use shin_core::{
        format::{audio::AudioSource, rom::RomReader},
        primitives::stateless_reader::StatelessFile,
        time::Tween,
        vm::command::types::{AudioWaitStatus, Pan, Volume},
    };
    use std::{fs::File, io::Cursor};

    struct PrimedRamp {
        position: u32,
    }
    impl AudioFrameSource for PrimedRamp {
        fn max_frame_size(&self) -> usize {
            6
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            3
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
            if self.position != 0 {
                return false;
            }
            destination.extend((0..6).map(|sample| (sample as f32, sample as f32)));
            self.position = 6;
            true
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            anyhow::ensure!(position <= 6, "Seek past source EOF");
            self.position = 0;
            Ok(position)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    #[test]
    fn repeating_audio_rewind_error_is_permanent_failure_instead_of_eof() {
        struct FailedRewind(PrimedRamp);
        impl AudioFrameSource for FailedRewind {
            fn max_frame_size(&self) -> usize {
                self.0.max_frame_size()
            }
            fn sample_rate(&self) -> u32 {
                self.0.sample_rate()
            }
            fn pre_skip(&self) -> u32 {
                self.0.pre_skip()
            }
            fn pre_roll(&self) -> u32 {
                self.0.pre_roll()
            }
            fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
                self.0.read_frame(destination)
            }
            fn samples_seek(&mut self, _: u32) -> anyhow::Result<u32> {
                anyhow::bail!("injected AAC rewind failure")
            }
            fn current_sample_position(&self) -> u32 {
                self.0.current_sample_position()
            }
        }
        let repeating =
            RepeatingMovieAudioSource::new(FailedRewind(PrimedRamp { position: 0 }), 10_000)
                .unwrap();
        let failed = repeating.decode_failed();
        let mut source = AudioSource::new(repeating);
        for _ in 0..10 {
            assert!(source.read_sample().is_some());
        }
        assert!(!failed.load(Ordering::Acquire));
        assert_eq!(source.read_sample(), None);
        assert!(failed.load(Ordering::Acquire));
        assert_eq!(source.current_samples_position(), 10);
        assert_eq!(source.read_sample(), None);
        assert!(source.samples_seek(0).is_err());
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); no GPU or sound device required"]
    fn reference_aac_read_and_seek_failures_freeze_retained_voice() {
        use crate::test_support::{opening_bytes, FaultingReader};
        let bytes = opening_bytes();
        for repeat in [false, true] {
            for warmup in [0, 8000] {
                let fail = Arc::new(AtomicBool::new(false));
                let mp4 = Mp4::new(FaultingReader {
                    bytes: Cursor::new(bytes.clone()),
                    fail: fail.clone(),
                })
                .unwrap();
                let source = AacFrameSource::new(mp4.audio_track.unwrap()).unwrap();
                let failed = source.decode_failed();
                let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
                    backend_settings: MockBackendSettings {
                        sample_rate: 48_000,
                    },
                    ..Default::default()
                })
                .unwrap();
                let settings = AudioSettings {
                    paused: false,
                    playback_rate: 1.0,
                    track: TrackId::Main,
                    fade_in: Tween::IMMEDIATE,
                    loop_start: None,
                    loop_count: 0,
                    volume: Volume(1.0),
                    pan: Pan::default(),
                };
                let mut decoder_eof = None;
                let mut handle = if repeat {
                    manager
                        .play(AudioData {
                            source: RepeatingMovieAudioSource::new(source, 102_037_333).unwrap(),
                            settings,
                        })
                        .unwrap()
                } else {
                    let source = MovieAudioSource::new(source);
                    decoder_eof = Some(source.decoder_eof());
                    manager.play(AudioData { source, settings }).unwrap()
                };
                for _ in 0..warmup {
                    manager.backend_mut().on_start_processing();
                    let _ = manager.backend_mut().process();
                }
                fail.store(true, Ordering::Release);
                for _ in 0..4096 {
                    manager.backend_mut().on_start_processing();
                    let _ = manager.backend_mut().process();
                    if failed.load(Ordering::Acquire) {
                        break;
                    }
                }
                assert!(failed.load(Ordering::Acquire));
                if let Some(eof) = &decoder_eof {
                    assert!(
                        !eof.load(Ordering::Acquire),
                        "AAC failure must not publish normal EOF"
                    );
                }
                manager.backend_mut().on_start_processing();
                let frozen = handle.position_samples();
                if warmup == 0 {
                    assert_eq!(frozen, 0);
                } else {
                    assert!(frozen >= warmup);
                }
                for _ in 0..32 {
                    handle.set_paused(false);
                    handle.set_volume(Volume(0.5), Tween::IMMEDIATE).unwrap();
                    for _ in 0..128 {
                        manager.backend_mut().on_start_processing();
                        assert_eq!(manager.backend_mut().process(), kira::Frame::ZERO);
                    }
                    assert_eq!(handle.position_samples(), frozen);
                    assert_eq!(manager.num_sounds(), 1);
                    assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
                }
                handle.stop(Tween::IMMEDIATE).unwrap();
                for _ in 0..10 {
                    manager.backend_mut().on_start_processing();
                    let _ = manager.backend_mut().process();
                }
                assert_eq!(manager.num_sounds(), 0);
            }
        }
        let fail = Arc::new(AtomicBool::new(false));
        let mp4 = Mp4::new(FaultingReader {
            bytes: Cursor::new(bytes.clone()),
            fail: fail.clone(),
        })
        .unwrap();
        let mut source = AacFrameSource::new(mp4.audio_track.unwrap()).unwrap();
        fail.store(true, Ordering::Release);
        let error = source.samples_seek(10_000).unwrap_err();
        assert!(format!("{error:#}").contains("injected movie AAC source read failure"));
        assert!(source.is_failed());
        assert!(!source.read_frame(&mut AudioBuffer::with_capacity(1024)));
        assert!(source.samples_seek(0).is_err());

        // The same terminal signal must cover codec errors, not just I/O.
        let mut mp4 = Mp4::new(Cursor::new(bytes.clone())).unwrap();
        let sample = mp4
            .audio_track
            .as_mut()
            .unwrap()
            .next_sample()
            .unwrap()
            .unwrap();
        let offset = bytes
            .windows(sample.bytes.len())
            .position(|candidate| candidate == sample.bytes.as_ref())
            .unwrap();
        let mut damaged = bytes;
        // Leading ID_CCE (010) requests a coupling-channel element that the
        // current AAC decoder reports as unsupported. MP4 metadata is intact.
        damaged[offset] = 0x40;
        let mp4 = Mp4::new(Cursor::new(damaged)).unwrap();
        let mut source = AacFrameSource::new(mp4.audio_track.unwrap()).unwrap();
        assert!(!source.read_frame(&mut AudioBuffer::with_capacity(1024)));
        assert!(source.is_failed());
        assert_eq!(source.current_sample_position(), 0);
    }

    #[test]
    fn repeating_audio_keeps_cumulative_samples_priming_and_fractional_cycle_boundaries() {
        let repeating = RepeatingMovieAudioSource::new(PrimedRamp { position: 0 }, 10_500).unwrap();
        let mut source = AudioSource::new(repeating);
        // Each10.5ms cycle carries three audible samples followed by finite
        // padding. Alternating10/11 sample boundaries retain the fractional half.
        for cycle in 0..4096u32 {
            let start = cycle * 21 / 2;
            let end = (cycle + 1) * 21 / 2;
            for offset in 0..end - start {
                let sample = if offset < 3 { (offset + 3) as f32 } else { 0.0 };
                assert_eq!(
                    source.read_sample(),
                    Some((sample, sample)),
                    "cycle{cycle} sample{offset}"
                );
                assert_eq!(source.current_samples_position(), start + offset + 1);
            }
        }
        assert_eq!(source.current_samples_position(), 43_008);
        source.samples_seek(0).unwrap();
        assert_eq!(source.current_samples_position(), 0);
        assert_eq!(source.read_sample(), Some((3.0, 3.0)));
        assert!(RepeatingMovieAudioSource::new(PrimedRamp { position: 0 }, 999).is_err());
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); no GPU or sound device required"]
    fn reference_opening_aac_decodes_seeks_and_mixes() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let file = rom.find_file("/movie/op1.mp4").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let mp4 = Mp4::new(Cursor::new(bytes)).unwrap();
        // Raw AAC mdhd duration includes encoder padding beyond the edit-list
        // presentation duration. The current demuxer uses this raw timeline.
        assert_eq!(mp4.duration_us(), 102_037_333);
        assert_eq!(
            mp4.audio_track
                .as_ref()
                .unwrap()
                .get_mp4_track_info(|t| t.trak.mdia.mdhd.duration),
            4_897_792
        );
        let track = mp4.audio_track.unwrap();
        let source = AacFrameSource::new(track.clone()).unwrap();
        assert_eq!(source.sample_rate(), 48_000);
        let mut source = AudioSource::new(source);
        let mut samples = Vec::new();
        while let Some(sample) = source.read_sample() {
            assert!(sample.0.is_finite() && sample.1.is_finite());
            samples.push(sample);
        }
        assert_eq!(samples.len(), 4_895_680);
        let prefix: Vec<u8> = samples
            .iter()
            .take(96_000)
            .flat_map(|(l, r)| l.to_le_bytes().into_iter().chain(r.to_le_bytes()))
            .collect();
        std::fs::write("/tmp/shin-opening-aac-prefix.f32", prefix).unwrap();
        assert_eq!(source.current_samples_position(), samples.len() as u32);
        assert!(samples.iter().any(|&(l, r)| l.abs().max(r.abs()) > 0.1));
        for position in [0, 1023, 8197, samples.len() - 20, samples.len()] {
            source.samples_seek(position as u32).unwrap();
            assert_eq!(source.current_samples_position(), position as u32);
            for &expected in samples.iter().skip(position).take(64) {
                assert_eq!(
                    source.read_sample(),
                    Some(expected),
                    "AAC seek sample mismatch at {position}"
                );
            }
            if position >= samples.len() - 20 {
                assert_eq!(source.read_sample(), None);
            }
        }

        // AudioSource seeks backward by preroll, so a target just past EOF
        // can seek successfully and then yield no samples. Beyond that
        // preroll window the underlying frame seek must return an error.
        source.samples_seek(samples.len() as u32 + 1).unwrap();
        assert_eq!(source.read_sample(), None);
        let beyond_preroll = samples.len() as u32 + source.inner().pre_roll() + 1;
        assert!(source.samples_seek(beyond_preroll).is_err());

        // Exercise real AAC EOF through the production movie wrapper. Seeking
        // via AudioSource preserves decoder preroll and audible preskip.
        let padded_source = MovieAudioSource::new(AacFrameSource::new(track.clone()).unwrap());
        let decoder_eof = padded_source.decoder_eof();
        let mut padded = AudioSource::new(padded_source);
        padded.samples_seek(samples.len() as u32 - 20).unwrap();
        for &expected in &samples[samples.len() - 20..] {
            assert_eq!(padded.read_sample(), Some(expected));
        }
        assert!(!decoder_eof.load(Ordering::Acquire));
        for _ in 0..9600 {
            assert_eq!(padded.read_sample(), Some((0.0, 0.0)));
        }
        assert!(decoder_eof.load(Ordering::Acquire));
        assert_eq!(
            padded.current_samples_position(),
            samples.len() as u32 + 9600
        );

        padded.samples_seek(0).unwrap();
        assert!(!decoder_eof.load(Ordering::Acquire));
        assert_eq!(padded.read_sample(), Some(samples[0]));

        // Cross an actual supplied AAC loop boundary without recreating a
        // sound/voice. The decoded prefix repeats and position never rewinds.
        let mut repeating = AudioSource::new(
            RepeatingMovieAudioSource::new(
                AacFrameSource::new(track.clone()).unwrap(),
                102_037_333,
            )
            .unwrap(),
        );
        for &expected in &samples {
            assert_eq!(repeating.read_sample(), Some(expected));
        }
        let boundary = 102_037_333u64 * 48_000 / 1_000_000;
        for _ in samples.len() as u64..boundary {
            assert_eq!(repeating.read_sample(), Some((0.0, 0.0)));
        }
        assert_eq!(u64::from(repeating.current_samples_position()), boundary);
        for (index, &expected) in samples.iter().take(1024).enumerate() {
            assert_eq!(repeating.read_sample(), Some(expected));
            assert_eq!(
                u64::from(repeating.current_samples_position()),
                boundary + index as u64 + 1
            );
        }

        let manager = || {
            AudioManager::<MockBackend>::new(AudioManagerSettings {
                backend_settings: MockBackendSettings {
                    sample_rate: 48_000,
                },
                ..Default::default()
            })
            .unwrap()
        };
        let data = |volume| AudioData {
            source: AacFrameSource::new(track.clone()).unwrap(),
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,
                track: TrackId::Main,
                fade_in: Tween::IMMEDIATE,
                loop_start: None,
                loop_count: 0,
                volume,
                pan: Pan::default(),
            },
        };
        let mut reference = manager();
        let AudioData { source, settings } = data(Volume(1.0));
        let reference_handle = reference
            .play(AudioData {
                source: MovieAudioSource::new(source),
                settings,
            })
            .unwrap();
        let mut clock = crate::timer::AudioTiedTimer::new(60_000, reference_handle);
        let mut submillisecond = false;
        let mut scaled = manager();
        let mut handle = scaled.play(data(Volume(0.5))).unwrap();
        let mut peak = 0.0f32;
        for index in 0..96_000 {
            if index == 60_000 {
                handle.set_volume(Volume(0.0), Tween::IMMEDIATE).unwrap();
            }
            if index == 72_000 {
                handle.set_volume(Volume(0.25), Tween::IMMEDIATE).unwrap();
            }
            reference.backend_mut().on_start_processing();
            scaled.backend_mut().on_start_processing();
            let expected = reference.backend_mut().process();
            let actual = scaled.backend_mut().process();
            let expected_time = u64::from(handle.position_samples()) * 60_000 / 48_000;
            assert_eq!(
                clock.update(shin_core::time::Ticks::from_seconds(60.0)),
                expected_time
            );
            submillisecond |= handle.position_millis() == 0 && clock.time() > 0;
            peak = peak.max(expected.left.abs()).max(expected.right.abs());
            let gain = if index < 60_000 {
                0.5
            } else if index < 72_000 {
                0.0
            } else {
                0.25
            };
            assert!((actual.left - expected.left * gain).abs() < 0.000001);
            assert!((actual.right - expected.right * gain).abs() < 0.000001);
        }
        assert!(
            submillisecond,
            "Movie clock must preserve submillisecond audio position"
        );
        assert!(peak > 0.01);
        assert!(handle.position().as_seconds() > 1.99);
        assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
    }
}
