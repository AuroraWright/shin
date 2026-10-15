//! Support for NXA format, storing opus audio. (The format might be specific for Nintendo Switch?)
//!
//! It is a simple container storing opus frames mostly as-is. The only addition compared to usual opus formats are loop points.
//!
//! The header specifies loop start and loop end points in samples. When looping is enabled and loop end is reached, the decoder seeks to the loop start.

mod audio_source;

use std::io::Read;

use anyhow::{Result, bail};
pub use audio_source::{AudioBuffer, AudioFrameSource, AudioSource};
use binrw::{BinRead, BinWrite};
use opus::Channels;

#[derive(BinRead, BinWrite, Debug)]
#[brw(little, magic = b"NXA1")]
#[br(assert(version == 1 || version == 2))]
struct NxaHeader {
    version: u32,
    file_size: u32,
    #[brw(align_after = 0x10)]
    info: AudioInfo,
}

/// Provides some metadata about the audio
///
/// Stuff that would usually go into opus headers
#[derive(BinRead, BinWrite, Debug, Clone)]
pub struct AudioInfo {
    /// Sample rate, in Hz.
    pub sample_rate: u32,
    /// Number of channels (usually 1 or 2).
    pub channel_count: u16,
    /// Size of frame in bytes
    pub frame_size: u16,
    /// Amount of samples in one frame
    pub frame_samples: u16,
    /// Amount of samples to skip, needed because of the way opus works
    pub pre_skip: u16,
    /// Amount of samples in the whole file
    pub num_samples: u32,
    /// Where to start playing after looping in samples.
    pub loop_start: u32,
    /// Where to loop from in samples (usually happens at the end of the file).
    pub loop_end: u32,
}

// A fully in-memory, but not yet decoded audio file
pub struct AudioFile {
    info: AudioInfo,
    data: Vec<u8>,
    packet_header_bytes: u8,
}

impl AudioFile {
    pub fn info(&self) -> &AudioInfo {
        &self.info
    }

    pub fn decode(self) -> Result<AudioDecoder<Self>> {
        AudioDecoder::new(self)
    }

    pub fn read_frames(self) -> AudioFileFrameReader<Self> {
        AudioFileFrameReader::new(self)
    }
}

impl AsRef<AudioFile> for AudioFile {
    fn as_ref(&self) -> &AudioFile {
        self
    }
}

pub struct AudioFileFrameReader<F: AsRef<AudioFile>> {
    file: F,
    bytes_position: usize,
}

impl<F: AsRef<AudioFile>> AudioFileFrameReader<F> {
    pub fn new(file: F) -> Self {
        Self {
            file,
            bytes_position: 0,
        }
    }

    pub fn audio_info(&self) -> &AudioInfo {
        &self.file.as_ref().info
    }

    pub fn frame_size(&self) -> usize {
        self.audio_info().frame_size as usize
    }

    fn frame_samples(&self) -> usize {
        self.audio_info().frame_samples as usize
    }

    pub fn frames_position(&self) -> usize {
        self.bytes_position / self.frame_size()
    }

    pub fn seek_to_frames(&mut self, new_frames_position: usize) {
        self.bytes_position = self.frame_size() * new_frames_position;
    }

    pub fn get_next_frame(&mut self) -> Option<&[u8]> {
        let size = self.frame_size();
        if size == 0 {
            return None;
        }
        // Native2fa60..2fa68 only submits complete encoded packets. An
        // incomplete final packet is retained until ordinary source EOF.
        let end = self.bytes_position.checked_add(size)?;
        let data = self.file.as_ref().data.get(self.bytes_position..end)?;
        self.bytes_position = end;
        if self.file.as_ref().packet_header_bytes == 8 {
            let length = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
            data.get(8..8usize.checked_add(length)?)
        } else {
            Some(data)
        }
    }

    pub fn has_next_frame(&self) -> bool {
        let size = self.frame_size();
        size != 0
            && self
                .file
                .as_ref()
                .data
                .len()
                .saturating_sub(self.bytes_position)
                >= size
    }
}

pub struct AudioDecoder<F: AsRef<AudioFile>> {
    frame_iter: AudioFileFrameReader<F>,
    buffer: Box<[f32]>,
    decoder: opus::Decoder,
    pcm16_buffer: Option<Box<[i16]>>,
    failed: bool,
    sample_position: u32,
}

impl<F: AsRef<AudioFile>> AudioDecoder<F> {
    pub fn new(file: F) -> Result<Self> {
        let info = &file.as_ref().info;
        if info.frame_samples == 0 {
            bail!("NXA frame sample count must be nonzero");
        }
        let decoder = opus::Decoder::new(
            info.sample_rate,
            match info.channel_count {
                1 => Channels::Mono,
                2 => Channels::Stereo,
                count => bail!("Unsupported NXA channel count: {count}"),
            },
        )?;
        let buffer =
            vec![0.0; info.frame_samples as usize * info.channel_count as usize].into_boxed_slice();
        Ok(Self {
            frame_iter: AudioFileFrameReader::new(file),
            buffer,
            decoder,
            pcm16_buffer: None,
            failed: false,
            sample_position: 0,
        })
    }

    /// Decode through Opus's signed16-bit API, preserving native NXA playback
    /// and voice detector quantization/clipping while returning normalized samples.
    pub fn new_pcm16(file: F) -> Result<Self> {
        let mut decoder = Self::new(file)?;
        decoder.pcm16_buffer = Some(vec![0; decoder.buffer.len()].into_boxed_slice());
        Ok(decoder)
    }

    pub fn audio_info(&self) -> &AudioInfo {
        self.frame_iter.audio_info()
    }

    fn frame_samples(&self) -> usize {
        self.frame_iter.frame_samples()
    }
}

impl<F: AsRef<AudioFile>> AudioFrameSource for AudioDecoder<F> {
    fn is_failed(&self) -> bool {
        self.failed
    }

    fn failure_ends_playback(&self) -> bool {
        true
    }

    fn max_frame_size(&self) -> usize {
        self.audio_info().frame_samples as usize
    }

    fn sample_rate(&self) -> u32 {
        self.audio_info().sample_rate
    }

    fn pre_skip(&self) -> u32 {
        self.audio_info().pre_skip as u32
    }

    fn pre_roll(&self) -> u32 {
        // Native2f94c..2f954 warms up four encoded packets.
        u32::from(self.audio_info().frame_samples) * 4
    }

    fn read_frame(&mut self, destination: &mut AudioBuffer) -> bool {
        // Native2f9a0 refuses further decode after2fc38 latches an error.
        // An explicit seek clears that latch; EOF itself never sets it.
        if self.failed {
            return false;
        }
        // copy the important info to not annoy the borrow checker below
        let &AudioInfo {
            channel_count: channels,
            ..
        } = self.audio_info();

        let Some(data) = self.frame_iter.get_next_frame() else {
            return false;
        };

        let decoded = if let Some(pcm16) = &mut self.pcm16_buffer {
            self.decoder.decode(data, pcm16, false)
        } else {
            self.decoder.decode_float(data, &mut self.buffer, false)
        };
        let decoded = match decoded {
            Ok(count) => count,
            Err(error) => {
                tracing::error!(%error, "NXA Opus decoder failed");
                self.failed = true;
                return false;
            }
        };
        let samples = decoded * usize::from(channels);
        if let Some(pcm16) = &self.pcm16_buffer {
            for (output, sample) in self.buffer[..samples].iter_mut().zip(pcm16.iter()) {
                *output = f32::from(*sample) / 32768.0;
            }
        }
        // Native2fb70..2fb98 clips the logical count to num_samples after
        // pre-skip. The public AudioSource removes pre-skip, so retain that
        // prefix here and bound the raw samples by pre_skip + num_samples.
        let info = self.audio_info();
        let raw_end = u64::from(info.pre_skip) + u64::from(info.num_samples);
        let emitted = decoded.min(raw_end.saturating_sub(u64::from(self.sample_position)) as usize);
        let buffer = &self.buffer[..emitted * usize::from(channels)];

        match channels {
            1 => {
                for &sample in buffer.iter() {
                    destination.push((sample, sample));
                }
            }
            2 => {
                for sample in buffer.chunks_exact(2) {
                    destination.push((sample[0], sample[1]));
                }
            }
            _ => panic!("Unsupported channel count: {}", channels),
        }

        self.sample_position = self.sample_position.saturating_add(emitted as u32);

        true
    }

    fn samples_seek(&mut self, samples_position: u32) -> Result<u32> {
        if samples_position > self.audio_info().num_samples {
            bail!(
                "Seek position {} is out of bounds (the file is {} samples)",
                samples_position,
                self.audio_info().num_samples
            );
        }

        let samples_position = samples_position as usize;

        let frames_position = samples_position / self.frame_samples();
        let in_frame_position = samples_position % self.frame_samples();

        if let Err(error) = self.decoder.reset_state() {
            self.failed = true;
            return Err(error.into());
        }
        self.frame_iter.seek_to_frames(frames_position);
        self.sample_position = (frames_position * self.frame_samples()) as u32;
        self.failed = false;

        Ok(in_frame_position.try_into().unwrap())
    }

    fn current_sample_position(&self) -> u32 {
        self.sample_position
    }

    fn prepare_samples_seek(&mut self, sample_position: u32) -> Result<u32> {
        // Native2f944..2f968 clamps the logical target, chooses its packet
        // minus four, then adds pre-skip to the discard count. Adding pre-skip
        // before packet selection would change the Opus reset point.
        let target = sample_position.min(self.audio_info().num_samples);
        let frames = u32::from(self.audio_info().frame_samples);
        let packet = (target / frames).saturating_sub(4);
        let raw_start = packet * frames;
        self.samples_seek(raw_start)?;
        Ok(target - raw_start + self.pre_skip())
    }

    fn loop_end(&self) -> Option<u32> {
        Some(self.audio_info().loop_end)
    }
}

pub fn read_audio(data: &[u8]) -> Result<AudioFile> {
    read_audio_version(data, Some(2))
}

/// Higurashi NXA1 v1 retains the48-byte header; each fixed packet includes
/// the8-byte Nintendo Opus length/final-range prefix that v2 omits.
/// Updated Higurashi's append archive also contains v2 packets. Select this
/// reader explicitly so Umineko keeps its version2 contract.
pub fn read_higurashi_audio(data: &[u8]) -> Result<AudioFile> {
    read_audio_version(data, None)
}

fn read_audio_version(data: &[u8], expected_version: Option<u32>) -> Result<AudioFile> {
    let mut cur = std::io::Cursor::new(data);
    let header = NxaHeader::read_le(&mut cur)?;
    anyhow::ensure!(
        expected_version.is_none_or(|version| header.version == version),
        "Expected NXA version{expected_version:?}, got{}",
        header.version
    );
    if header.file_size as usize != data.len() {
        bail!(
            "NXA declared file size {} does not match actual size {}",
            header.file_size,
            data.len()
        );
    }

    let mut data = Vec::new();
    cur.read_to_end(&mut data)?;
    let version = header.version;

    if version == 1 {
        let stride = usize::from(header.info.frame_size);
        anyhow::ensure!(stride > 8, "Higurashi NXA packet has no Opus payload");
        for packet in data.chunks_exact(stride) {
            let length = u32::from_be_bytes(packet[..4].try_into().unwrap()) as usize;
            anyhow::ensure!(
                length > 0 && length <= stride - 8,
                "Invalid Higurashi NXA packet length"
            );
        }
    }

    Ok(AudioFile {
        info: header.info,
        data,
        packet_header_bytes: if version == 1 { 8 } else { 0 },
    })
}

#[cfg(test)]
mod higurashi_tests;

#[cfg(test)]
mod pcm16_tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied executable and native-nxa-bounds.c oracle"]
    fn reference_native_nxa_logical_seek_packet_and_preskip() {
        use std::{collections::HashMap, process::Command, sync::Arc};
        let mut cases = 0;
        for frames in [960, 120, 480] {
            for channels in [Channels::Mono, Channels::Stereo] {
                let count = if channels == Channels::Mono { 1 } else { 2 };
                let mut encoder =
                    opus::Encoder::new(48000, channels, opus::Application::Audio).unwrap();
                let input: Vec<i16> = (0..frames * count)
                    .map(|i| if i % 48 < 24 { 22000 } else { -22000 })
                    .collect();
                let mut packet = vec![0; 4000];
                let size = encoder.encode(&input, &mut packet).unwrap();
                packet.truncate(size);
                assert!(size <= 1024);
                for pre_skip in [312, 0, 64] {
                    let declared = frames as u32 * 20 - u32::from(pre_skip);
                    let file = Arc::new(AudioFile {
                        packet_header_bytes: 0,
                        info: AudioInfo {
                            sample_rate: 48000,
                            channel_count: count as u16,
                            frame_size: size as u16,
                            frame_samples: frames as u16,
                            pre_skip,
                            num_samples: declared,
                            loop_start: 0,
                            loop_end: declared,
                        },
                        data: packet.repeat(20),
                    });
                    for requested in [
                        frames as u32 * 5 - 210,
                        0,
                        1,
                        frames as u32 * 4 - 1,
                        frames as u32 * 4,
                        frames as u32 * 5 - 1,
                        frames as u32 * 5,
                        declared - 1,
                        declared,
                        declared + 1,
                        declared + 10000,
                        0x80000000,
                        u32::MAX - 1,
                    ] {
                        let output = Command::new(
                            std::env::var_os("SHIN_REFERENCE_NXA_BOUNDS_ORACLE").unwrap(),
                        )
                        .arg(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").unwrap())
                        .args([
                            (size * 20).to_string(),
                            size.to_string(),
                            frames.to_string(),
                            declared.to_string(),
                            pre_skip.to_string(),
                            count.to_string(),
                            requested.to_string(),
                        ])
                        .output()
                        .unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let result = String::from_utf8(output.stdout).unwrap();
                        let values: HashMap<_, _> = result
                            .split_whitespace()
                            .map(|field| {
                                let (key, value) = field.split_once('=').unwrap();
                                (key, value.parse::<u32>().unwrap())
                            })
                            .collect();
                        assert_eq!(values["loop"], 0);
                        assert_eq!(values["resets"], 1);
                        let mut source =
                            AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
                        source.samples_seek(requested).unwrap();
                        let packet_index = source.inner().frame_iter.frames_position();
                        assert_eq!(
                            48 + packet_index as u32 * size as u32,
                            values["seek_byte"],
                            "Native seek packet: fs{frames}/skip{pre_skip}/requested{requested}"
                        );
                        assert_eq!(source.current_samples_position(), values["seek_position"]);
                        let skip = source.current_samples_position() + u32::from(pre_skip)
                            - source.inner().current_sample_position();
                        assert_eq!(skip, values["seek_skip"]);
                        // Independently reset libopus at the native-selected packet,
                        // then discard the native-reported warmup+pre-skip count.
                        let mut decoder = opus::Decoder::new(48000, channels).unwrap();
                        let mut pcm = vec![0; frames * count];
                        let mut remaining_skip = values["seek_skip"] as usize;
                        let mut emitted = 0;
                        for _ in packet_index..20 {
                            assert_eq!(decoder.decode(&packet, &mut pcm, false).unwrap(), frames);
                            for sample in pcm.chunks_exact(count) {
                                if remaining_skip > 0 {
                                    remaining_skip -= 1;
                                    continue;
                                }
                                if emitted >= values["frames"] {
                                    break;
                                }
                                assert_eq!(
                                    source.read_sample(),
                                    Some((
                                        f32::from(sample[0]) / 32768.0,
                                        f32::from(sample[count - 1]) / 32768.0
                                    ))
                                );
                                emitted += 1;
                            }
                        }
                        assert_eq!(emitted, values["frames"]);
                        assert_eq!(source.read_sample(), None);
                        assert_eq!(source.current_samples_position(), values["position"]);
                        assert!(!source.inner().is_failed());
                        cases += 1;
                    }
                }
            }
        }
        println!(
            "Native NXA seek: {cases} cases, exact packet/discard/logical clocks and independently reset host PCM; Nintendo Opus PCM bits excluded"
        );
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
    fn reference_rom_nxa_packet_bounds_and_declared_tails() {
        use crate::{
            format::rom::{IndexEntry, RomReader},
            primitives::stateless_reader::StatelessFile,
        };
        use std::{collections::BTreeMap, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var_os("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let files: Vec<_> = rom
            .traverse()
            .filter_map(|(path, entry)| match entry {
                IndexEntry::File(file) => Some((path, *file)),
                _ => None,
            })
            .collect();
        assert_eq!(files.len(), 109791);
        let mut count = 0;
        let mut partial = 0;
        let mut padded = 0;
        let mut short = 0;
        let mut padding = 0u64;
        let mut metadata = BTreeMap::new();
        let mut loop_ends = BTreeMap::new();
        let mut nonzero_loop_starts = 0;
        for (path, file) in files {
            if file.size() < 48 {
                continue;
            }
            let mut reader = rom.open_file(file).unwrap();
            let mut bytes = [0; 48];
            reader.read_exact(&mut bytes[..4]).unwrap();
            if &bytes[..4] != b"NXA1" {
                continue;
            }
            reader.read_exact(&mut bytes[4..]).unwrap();
            let header = NxaHeader::read_le(&mut std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(header.file_size as usize, file.size() as usize, "{path}");
            assert!(header.info.frame_size > 0 && header.info.frame_samples > 0);
            assert!((1..=2).contains(&header.info.channel_count));
            let payload = u64::from(header.file_size) - 48;
            let packets = payload / u64::from(header.info.frame_size);
            partial += usize::from(payload % u64::from(header.info.frame_size) != 0);
            let raw_samples = packets * u64::from(header.info.frame_samples);
            let end = u64::from(header.info.num_samples) + u64::from(header.info.pre_skip);
            *loop_ends
                .entry(i64::from(header.info.loop_end) - i64::from(header.info.num_samples))
                .or_insert(0usize) += 1;
            nonzero_loop_starts += usize::from(header.info.loop_start != 0);
            if header.info.loop_end != header.info.num_samples || header.info.loop_start != 0 {
                println!(
                    "NXA loop asset {path}: declared{}, start{}, end{}",
                    header.info.num_samples, header.info.loop_start, header.info.loop_end
                );
            }
            padded += usize::from(raw_samples > end);
            short += usize::from(raw_samples < end);
            padding += raw_samples.saturating_sub(end);
            *metadata
                .entry((
                    header.info.sample_rate,
                    header.info.channel_count,
                    header.info.frame_samples,
                    header.info.pre_skip,
                ))
                .or_insert(0usize) += 1;
            count += 1;
        }
        println!(
            "Supplied NXA headers: {count} assets, {partial} incomplete packet tails, {padded} padded assets/{padding} metadata padding frames, {short} shorter-than-declared assets; rate/channel/frame-samples/preskip{metadata:?}"
        );
        assert!(count > 0);
        assert_eq!(
            partial, 0,
            "Provided NXA packet tails must be reviewed before native-equivalence claims"
        );
        assert_eq!(
            (count, padded, short, padding),
            (105970, 105896, 0, 50490199)
        );
        let expected: BTreeMap<_, _> =
            [((48000, 1, 960, 312), 105594), ((48000, 2, 960, 312), 376)]
                .into_iter()
                .collect();
        assert_eq!(metadata, expected);
        println!(
            "Supplied NXA loop fields: end-minus-declared{loop_ends:?}, nonzero loop starts{nonzero_loop_starts}"
        );
        assert_eq!(loop_ends, [(-1, 7), (0, 105963)].into_iter().collect());
        assert_eq!(nonzero_loop_starts, 2);
    }

    #[test]
    #[ignore = "requires supplied executable and native-nxa-bounds.c oracle"]
    fn reference_native_nxa_declared_sample_tails() {
        use std::{collections::HashMap, process::Command, sync::Arc};
        let mut cases = 0;
        for channels in [Channels::Mono, Channels::Stereo] {
            let count = if channels == Channels::Mono { 1 } else { 2 };
            let mut encoder =
                opus::Encoder::new(48000, channels, opus::Application::Audio).unwrap();
            let input: Vec<i16> = (0..960 * count)
                .map(|i| if i % 48 < 24 { 22000 } else { -22000 })
                .collect();
            let mut packet = vec![0; 4000];
            let size = encoder.encode(&input, &mut packet).unwrap();
            packet.truncate(size);
            assert!(size <= 1024);
            for packets in [1, 2, 6] {
                for pre_skip in [0, 64, 960] {
                    let available = (packets * 960u32).saturating_sub(u32::from(pre_skip));
                    for declared in [0, 1, available, available + 1] {
                        let output = Command::new(
                            std::env::var_os("SHIN_REFERENCE_NXA_BOUNDS_ORACLE").unwrap(),
                        )
                        .arg(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").unwrap())
                        .args([
                            (size * packets as usize).to_string(),
                            size.to_string(),
                            "960".into(),
                            declared.to_string(),
                            pre_skip.to_string(),
                            count.to_string(),
                            u32::MAX.to_string(),
                        ])
                        .output()
                        .unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        let result = String::from_utf8(output.stdout).unwrap();
                        let values: HashMap<_, _> = result
                            .split_whitespace()
                            .map(|field| {
                                let (key, value) = field.split_once('=').unwrap();
                                (key, value.parse::<u32>().unwrap())
                            })
                            .collect();
                        assert_eq!(values["loop"], 0);
                        assert_eq!(values["failed"], 0);
                        let file = Arc::new(AudioFile {
                            packet_header_bytes: 0,
                            info: AudioInfo {
                                sample_rate: 48000,
                                channel_count: count as u16,
                                frame_size: size as u16,
                                frame_samples: 960,
                                pre_skip,
                                num_samples: declared,
                                loop_start: 0,
                                loop_end: declared,
                            },
                            data: packet.repeat(packets as usize),
                        });
                        let mut source = AudioSource::new(AudioDecoder::new_pcm16(file).unwrap());
                        let mut frames = 0;
                        while source.read_sample().is_some() {
                            frames += 1;
                        }
                        assert_eq!(
                            frames, values["frames"],
                            "Native NXA declared tail: packets{packets}/skip{pre_skip}/declared{declared}"
                        );
                        assert_eq!(source.current_samples_position(), values["position"]);
                        assert_eq!(source.read_sample(), None);
                        assert!(!source.inner().is_failed());
                        cases += 1;
                    }
                }
            }
        }
        println!(
            "Native NXA wrapper: {cases} mono/stereo declared-tail/preskip cases; host stub supplies decoded frame counts, not Nintendo Opus PCM bits"
        );
    }

    #[test]
    fn nxa_incomplete_final_packet_preserves_complete_pcm_prefix() {
        for channels in [Channels::Mono, Channels::Stereo] {
            let count = if channels == Channels::Mono { 1 } else { 2 };
            let mut encoder =
                opus::Encoder::new(48000, channels, opus::Application::Audio).unwrap();
            let input: Vec<i16> = (0..960 * count)
                .map(|i| if i % 48 < 24 { 22000 } else { -22000 })
                .collect();
            let mut packet = vec![0; 4000];
            let size = encoder.encode(&input, &mut packet).unwrap();
            packet.truncate(size);
            let mut expected = vec![0; 960 * count];
            assert_eq!(
                opus::Decoder::new(48000, channels)
                    .unwrap()
                    .decode(&packet, &mut expected, false)
                    .unwrap(),
                960
            );
            for prefix in [0, 1] {
                for tail in [1, size - 1] {
                    for pre_skip in [0, 64] {
                        let mut data = if prefix == 1 {
                            packet.clone()
                        } else {
                            Vec::new()
                        };
                        data.extend_from_slice(&packet[..tail]);
                        let file = std::sync::Arc::new(AudioFile {
                            packet_header_bytes: 0,
                            info: AudioInfo {
                                sample_rate: 48000,
                                channel_count: count as u16,
                                frame_size: size as u16,
                                frame_samples: 960,
                                pre_skip,
                                num_samples: 1920,
                                loop_start: 0,
                                loop_end: 1920,
                            },
                            data,
                        });
                        let mut source =
                            AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
                        if prefix == 1 {
                            for frame in expected.chunks_exact(count).skip(usize::from(pre_skip)) {
                                assert_eq!(
                                    source.read_sample(),
                                    Some((
                                        f32::from(frame[0]) / 32768.0,
                                        f32::from(frame[count - 1]) / 32768.0
                                    ))
                                );
                            }
                        }
                        let length = (prefix * 960u32).saturating_sub(u32::from(pre_skip));
                        for _ in 0..8 {
                            assert_eq!(source.read_sample(), None);
                            assert!(!source.inner().is_failed());
                            assert_eq!(source.current_samples_position(), length);
                        }
                        let mut frames = AudioFileFrameReader::new(file);
                        for _ in 0..prefix {
                            assert!(frames.has_next_frame());
                            assert_eq!(frames.get_next_frame(), Some(packet.as_slice()));
                        }
                        assert!(!frames.has_next_frame());
                        assert_eq!(frames.get_next_frame(), None);
                        assert_eq!(frames.frames_position(), prefix as usize);
                    }
                }
            }
        }
    }

    #[test]
    fn nxa_decode_error_freezes_until_explicit_seek() {
        let mut encoder =
            opus::Encoder::new(48000, Channels::Mono, opus::Application::Audio).unwrap();
        let input: Vec<i16> = (0..960)
            .map(|i| if i % 48 < 24 { 24000 } else { -24000 })
            .collect();
        let mut packet = vec![0; 4000];
        let size = encoder.encode(&input, &mut packet).unwrap();
        packet.truncate(size);
        assert!(size >= 2);
        let mut invalid = vec![0; size];
        invalid[0] = 0xff; // code3 packet declaring zero frames
        assert!(
            opus::Decoder::new(48000, Channels::Mono)
                .unwrap()
                .decode(&invalid, &mut [0; 960], false)
                .is_err()
        );
        for pcm16 in [false, true] {
            for valid_prefix in [false, true] {
                let mut data = if valid_prefix {
                    packet.clone()
                } else {
                    Vec::new()
                };
                data.extend_from_slice(&invalid);
                let file = AudioFile {
                    packet_header_bytes: 0,
                    info: AudioInfo {
                        sample_rate: 48000,
                        channel_count: 1,
                        frame_size: size as u16,
                        frame_samples: 960,
                        pre_skip: 0,
                        num_samples: if valid_prefix { 1920 } else { 960 },
                        loop_start: 0,
                        loop_end: if valid_prefix { 1920 } else { 960 },
                    },
                    data,
                };
                let decoder = if pcm16 {
                    AudioDecoder::new_pcm16(file)
                } else {
                    AudioDecoder::new(file)
                }
                .unwrap();
                let mut source = AudioSource::new(decoder);
                let prefix = if valid_prefix { 960 } else { 0 };
                for _ in 0..prefix {
                    assert!(source.read_sample().is_some());
                }
                assert_eq!(source.current_samples_position(), prefix);
                assert_eq!(source.read_sample(), None);
                assert!(source.inner().is_failed());
                for _ in 0..32 {
                    assert_eq!(source.read_sample(), None);
                    assert_eq!(source.current_samples_position(), prefix);
                }
                // Original seek2f940 clears the error latch and resets Opus.
                source.samples_seek(0).unwrap();
                assert!(!source.inner().is_failed());
                for _ in 0..prefix {
                    assert!(source.read_sample().is_some());
                }
                assert_eq!(source.read_sample(), None);
                assert!(source.inner().is_failed());
                assert_eq!(source.current_samples_position(), prefix);
            }
        }
    }

    #[test]
    fn nxa_short_packet_uses_actual_decoded_count_and_normal_eof() {
        for channels in [Channels::Mono, Channels::Stereo] {
            let count = if channels == Channels::Mono { 1 } else { 2 };
            let mut encoder =
                opus::Encoder::new(48000, channels, opus::Application::Audio).unwrap();
            let input: Vec<i16> = (0..120 * count)
                .map(|i| if i % 24 < 12 { 20000 } else { -20000 })
                .collect();
            let mut packet = vec![0; 4000];
            let size = encoder.encode(&input, &mut packet).unwrap();
            packet.truncate(size);
            let mut expected = vec![0; 120 * count];
            assert_eq!(
                opus::Decoder::new(48000, channels)
                    .unwrap()
                    .decode(&packet, &mut expected, false)
                    .unwrap(),
                120
            );
            let file = AudioFile {
                packet_header_bytes: 0,
                info: AudioInfo {
                    sample_rate: 48000,
                    channel_count: count as u16,
                    frame_size: size as u16,
                    frame_samples: 960,
                    pre_skip: 0,
                    num_samples: 120,
                    loop_start: 0,
                    loop_end: 120,
                },
                data: packet,
            };
            let mut source = AudioSource::new(AudioDecoder::new_pcm16(file).unwrap());
            for frame in expected.chunks_exact(count) {
                let left = f32::from(frame[0]) / 32768.0;
                let right = f32::from(frame[count - 1]) / 32768.0;
                assert_eq!(source.read_sample(), Some((left, right)));
            }
            assert_eq!(source.current_samples_position(), 120);
            assert_eq!(source.read_sample(), None);
            assert!(!source.inner().is_failed());
            assert_eq!(source.current_samples_position(), 120);
        }
    }

    #[test]
    #[ignore = "requires supplied executable (SHIN_REFERENCE_EXECUTABLE)"]
    fn reference_native_nxa_signed16_decoder_abi() {
        let exe = std::fs::read(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").unwrap()).unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // DecodeOpusInterleaved's Ps argument is signed short*, not float*.
        // Original JUMP_SLOT symbol102 -> GOT223d30 -> PLT11cf10.
        let relocation = &exe[0x153638 + 256..0x153650 + 256];
        assert_eq!(
            u64::from_le_bytes(relocation[..8].try_into().unwrap()),
            0x223d30
        );
        assert_eq!(
            u64::from_le_bytes(relocation[8..16].try_into().unwrap()),
            0x6600000402
        );
        let name = word(0x154e88 + 102 * 24) as usize;
        let start = 0x1568f8 + name + 256;
        let end = start + exe[start..].iter().position(|&b| b == 0).unwrap();
        assert_eq!(
            &exe[start..end],
            b"_ZN2nn5codec21DecodeOpusInterleavedEPNS0_15OpusDecoderTypeEPmPiPsmPKvm"
        );
        let hash = exe[0x2f980 + 256..0x2fc6c + 256]
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(hash, 0xf4dfd609eaefe758);
        // Bytes per frame = channels*2; output byte pointer and byte offset
        // use this same stride. Decoder call returns interleaved PCM16.
        for (a, bits) in [
            (0x11cf10, 0xf0000830),
            (0x11cf14, 0xf9469a11),
            (0x11cf18, 0x9134c210),
            (0x11cf1c, 0xd61f0220),
            (0x2f74c, 0x531f7829),
            (0x2fa1c, 0x8b394103),
            (0x2fa30, 0xaa1503e0),
            (0x2fa38, 0x9403b536),
            (0x2fa3c, 0x35000f20),
            (0x2fa50, 0xb9407269),
            (0x2fa54, 0x1b086539),
            (0x2fb60, 0x1ac80b2a),
            (0x2f9a0, 0x3942a008),
            (0x2f9a4, 0x34000068),
            (0x2f9a8, 0x2a1f03e0),
            (0x2fc30, 0x320003e8),
            (0x2fc34, 0x2a1f03e0),
            (0x2fc38, 0x3902a268),
            (0x2f940, 0x3902a01f),
            // The NXA player treats decoder false as error, pauses the
            // retained backend voice, and reports logical state2.
            (0xf4548, 0x360012c0),
            (0xf47ac, 0x320003e8),
            (0xf47b0, 0x390ac268),
            (0xf47bc, 0xf9400d08),
            (0xf47c0, 0xd63f0100),
            (0xf4bac, 0x394ac288),
            (0xf4bb0, 0x35000068),
            (0xf4bbc, 0x321f03f4),
            (0xf4bcc, 0x2a1403e0),
        ] {
            assert_eq!(word(a), bits, "Native NXA signed16 anchor {a:x}");
        }
    }

    #[test]
    fn pcm16_decoder_matches_libopus_clipping_channels_and_preskip() {
        let mut encoder =
            opus::Encoder::new(48000, Channels::Stereo, opus::Application::Audio).unwrap();
        let samples: Vec<i16> = (0..960)
            .flat_map(|i| {
                [
                    if i % 48 < 24 { 30000 } else { -30000 },
                    if i % 32 < 16 { 10000 } else { -10000 },
                ]
            })
            .collect();
        let mut packet = vec![0; 4000];
        let length = encoder.encode(&samples, &mut packet).unwrap();
        packet.truncate(length);
        let mut expected = vec![0_i16; 1920];
        let mut native = opus::Decoder::new(48000, Channels::Stereo).unwrap();
        assert_eq!(native.decode(&packet, &mut expected, false).unwrap(), 960);
        for declared in [0, 1, 700, 896] {
            let file = AudioFile {
                packet_header_bytes: 0,
                info: AudioInfo {
                    sample_rate: 48000,
                    channel_count: 2,
                    frame_size: length as u16,
                    frame_samples: 960,
                    pre_skip: 64,
                    num_samples: declared,
                    loop_start: 0,
                    loop_end: declared,
                },
                data: packet.clone(),
            };
            let mut actual = AudioSource::new(AudioDecoder::new_pcm16(file).unwrap());
            for sample in expected.chunks_exact(2).skip(64).take(declared as usize) {
                assert_eq!(
                    actual.read_sample(),
                    Some((
                        f32::from(sample[0]) / 32768.0,
                        f32::from(sample[1]) / 32768.0
                    ))
                );
            }
            assert_eq!(actual.read_sample(), None);
            assert_eq!(actual.current_samples_position(), declared);
            assert!(!actual.inner().is_failed());
            actual.samples_seek(u32::MAX).unwrap();
            assert_eq!(actual.current_samples_position(), declared);
            assert_eq!(actual.read_sample(), None);
            actual.samples_seek(1).unwrap();
            let target = declared.min(1);
            assert_eq!(actual.current_samples_position(), target);
            let sample = if target < declared {
                let pair = &expected[(64 + target as usize) * 2..][..2];
                Some((f32::from(pair[0]) / 32768.0, f32::from(pair[1]) / 32768.0))
            } else {
                None
            };
            assert_eq!(actual.read_sample(), sample);
        }
    }
}
