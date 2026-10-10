use super::{History, decode_block};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct SysseOracle {
    child: Option<Child>,
    input: Option<BufWriter<ChildStdin>>,
    output: BufReader<ChildStdout>,
    streams: usize,
    samples: u64,
    blocks: u64,
    public_streams: usize,
}
impl SysseOracle {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_REFERENCE_SYSSE_ORACLE").unwrap())
            .arg(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Launch guarded native SYSSE oracle");
        Self {
            input: Some(BufWriter::new(child.stdin.take().unwrap())),
            output: BufReader::new(child.stdout.take().unwrap()),
            child: Some(child),
            streams: 0,
            samples: 0,
            blocks: 0,
            public_streams: 0,
        }
    }
    fn compare(&mut self, channels: u32, sample_count: u32, data: &[u8], initial: [i16; 4]) {
        assert!((1..=2).contains(&channels));
        let stride = channels as usize * 16;
        let blocks = data.len() / stride;
        let native_blocks = blocks.min((sample_count as usize).div_ceil(2400).max(1) * 80);
        let mut histories = [
            History {
                history_1: initial[0] as i32,
                history_2: initial[1] as i32,
            },
            History {
                history_1: initial[2] as i32,
                history_2: initial[3] as i32,
            },
        ];
        let mut decoded = Vec::new();
        for chunk in data.chunks_exact(stride).take(native_blocks) {
            for channel in 0..channels as usize {
                assert!(chunk[channel * 16] >> 4 <= 4, "Unsupported native filter");
            }
            let left = decode_block(&mut histories[0], chunk[..16].try_into().unwrap());
            if channels == 2 {
                let right = decode_block(&mut histories[1], chunk[16..32].try_into().unwrap());
                for (left, right) in left.into_iter().zip(right) {
                    decoded.extend([left, right]);
                }
            } else {
                decoded.extend(left);
            }
        }
        let count = (sample_count as usize).min(blocks * 30);
        decoded.truncate(count * channels as usize);
        let input = self.input.as_mut().unwrap();
        input.write_all(b"NSY1").unwrap();
        for value in [channels, sample_count, u32::try_from(data.len()).unwrap()] {
            input.write_all(&value.to_le_bytes()).unwrap();
        }
        for value in initial {
            input.write_all(&value.to_le_bytes()).unwrap();
        }
        input.write_all(data).unwrap();
        input.flush().unwrap();
        let mut header = [0; 16];
        self.output
            .read_exact(&mut header)
            .expect("Native SYSSE response");
        assert_eq!(
            u32::from_le_bytes(header[..4].try_into().unwrap()) as usize,
            count
        );
        let actual_history: [i16; 4] = std::array::from_fn(|i| {
            i16::from_le_bytes(header[4 + i * 2..6 + i * 2].try_into().unwrap())
        });
        assert_eq!(
            actual_history,
            [
                histories[0].history_1 as i16,
                histories[0].history_2 as i16,
                histories[1].history_1 as i16,
                histories[1].history_2 as i16
            ],
            "Native SYSSE final histories"
        );
        let calls = u32::from_le_bytes(header[12..].try_into().unwrap());
        assert!((2..=blocks as u32 / 80 + 3).contains(&calls));
        let mut bytes = vec![0; decoded.len() * 2];
        self.output.read_exact(&mut bytes).unwrap();
        for (i, (expected, pair)) in decoded.iter().zip(bytes.chunks_exact(2)).enumerate() {
            let actual = i16::from_le_bytes(pair.try_into().unwrap());
            assert_eq!(
                *expected, actual,
                "Native SYSSE PCM sample{i}, channels{channels}, initial{initial:?}"
            );
        }
        if initial == [0; 4] {
            use crate::format::audio::AudioSource;
            let sound = super::super::Sound {
                channel_count: if channels == 1 {
                    super::ChannelCount::Mono
                } else {
                    super::ChannelCount::Stereo
                },
                sample_rate: 48000,
                sample_count,
                sample_data: data.to_vec(),
            };
            let mut source = AudioSource::new(sound.decode());
            for frame in bytes.chunks_exact(channels as usize * 2) {
                // Verify the public source uses the independently decoded
                // native integers. CPAL uses the canonical signed16 float scale; this is
                // not proof of original DSP mixing or device output.
                let scaled = |pair: &[u8]| {
                    (i16::from_le_bytes(pair.try_into().unwrap()) as f32 / 32768.0).clamp(-1.0, 1.0)
                };
                let left = scaled(&frame[..2]);
                let right = if channels == 1 {
                    left
                } else {
                    scaled(&frame[2..])
                };
                assert_eq!(
                    source.read_sample(),
                    Some((left, right)),
                    "Native PCM through public SYSSE source"
                );
            }
            assert_eq!(source.read_sample(), None);
            assert_eq!(source.current_samples_position(), count as u32);
            self.public_streams += 1;
        }
        self.streams += 1;
        self.samples += decoded.len() as u64;
        self.blocks += native_blocks as u64 * channels as u64;
    }
    fn finish(mut self) -> (usize, u64, u64) {
        drop(self.input.take());
        assert!(self.child.take().unwrap().wait().unwrap().success());
        eprintln!(
            "Native SYSSE public AudioSource integration: {} streams match native PCM integers under canonical CPAL float scaling",
            self.public_streams
        );
        (self.streams, self.samples, self.blocks)
    }
}
impl Drop for SysseOracle {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn header_filter_includes_native_high_bit() {
    // Nativefe6c8/fe6cc indexes pairs with (header>>1)&0x78, so bit7 is
    // part of the filter index. Only indices0..4 have reviewed coefficients.
    assert_eq!(super::BlockHeader(0x85).shift(), 5);
    assert_eq!(super::BlockHeader(0x85).filter(), 8);
    assert_eq!(super::BlockHeader(0xf3).filter(), 15);
}

#[test]
fn pcm16_float_scale_preserves_every_native_integer_sample() {
    // Native SYSSE submits format1, copied as PCM16 by2f484. CPAL's pinned
    // dasp_sample conversion represents PCM16 in [-1,1) with denominator32768.
    // Its inverse multiplies32768 before converting to i16.
    for sample in i16::MIN..=i16::MAX {
        let normalized = super::convert_sample(sample);
        assert!(normalized >= -1.0 && normalized < 1.0);
        assert_eq!(
            (normalized * 32768.0) as i16,
            sample,
            "Native PCM16 sample{sample} must survive the float audio boundary"
        );
    }
}

#[test]
fn partial_sound_blocks_preserve_complete_native_pcm_prefix() {
    use super::ChannelCount;
    use crate::format::audio::AudioSource;
    for channels in [1, 2] {
        let stride = channels * 16;
        for blocks in [0u32, 1, 2] {
            for tail in [1, stride - 1] {
                for count in [0, 1, 29, 30, 31, 60, 63] {
                    let mut data = Vec::new();
                    for _ in 0..blocks {
                        data.extend([0]);
                        data.extend([0x12; 15]);
                        if channels == 2 {
                            data.extend([0]);
                            data.extend([0x34; 15]);
                        }
                    }
                    data.extend(std::iter::repeat_n(0x35, tail));
                    let sound = super::super::Sound {
                        channel_count: if channels == 1 {
                            ChannelCount::Mono
                        } else {
                            ChannelCount::Stereo
                        },
                        sample_rate: 48000,
                        sample_count: count,
                        sample_data: data,
                    };
                    let mut decoder = AudioSource::new(sound.decode());
                    let decoded: Vec<_> = std::iter::from_fn(|| decoder.read_sample()).collect();
                    let expected: Vec<_> = (0..count.min(blocks * 30))
                        .map(|i| {
                            // Original decoder's filter0/shift0/low-nibble-first PCM:
                            // mono[2,1], stereo[(2,4),(1,3)] per repeating two frames.
                            // Float scaling preserves PCM16 for CPAL, not proof
                            // of original DSP mixing or device output.
                            let left = (if i % 2 == 0 { 2.0 } else { 1.0 }) / 32768.0;
                            let right = if channels == 1 {
                                left
                            } else {
                                (if i % 2 == 0 { 4.0 } else { 3.0 }) / 32768.0
                            };
                            (left, right)
                        })
                        .collect();
                    assert_eq!(decoded, expected);
                    assert_eq!(decoder.current_samples_position(), count.min(blocks * 30));
                    assert!(decoder.read_sample().is_none());
                }
            }
        }
    }
}

#[test]
#[ignore = "requires supplied executable (SHIN_REFERENCE_EXECUTABLE)"]
fn reference_native_sysse_pcm16_queue_format() {
    let executable = std::fs::read(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").unwrap()).unwrap();
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    assert_eq!(relocations.get(&0x2248e0), Some(&0x1d3bb8));
    assert_eq!(relocations.get(&(0x1d3bb8 + 0x10 + 0x38)), Some(&0x2ba50));
    for (address, bytes, expected) in [
        (0x2ba50, 0xe8, 0xa926d86ea15ee9deu64),
        (0x2f3d0, 0xc8, 0xaf790fcd2d9b26dd),
    ] {
        let actual = executable[address + 256..address + 256 + bytes]
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(actual, expected);
    }
    // SYSSE format1, byte count, voice queue conversion dispatch, and its
    // PCM16 memcpy branch. Format2 has separate float clipping/quantization;
    // that asymmetric float encoder is not the SYSSE integer path.
    for (address, expected) in [
        (0xfe7c4, 0x320003e3u32),
        (0xfe7d0, 0x531f7902),
        (0xfe7dc, 0xd63f0100),
        (0x2bac4, 0x94000e43),
        (0x2f484, 0x531f7842),
        (0x2f488, 0x1403b56e),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected
        );
    }
    for (index, width) in [1u32, 2, 4].into_iter().enumerate() {
        let address = 0x16577c + index * 4 + 256;
        assert_eq!(
            u32::from_le_bytes(executable[address..address + 4].try_into().unwrap()),
            width
        );
    }
    for (index, scale) in [32767.0f32, 32768.0].into_iter().enumerate() {
        let address = 0x164ca8 + index * 4 + 256;
        assert_eq!(
            u32::from_le_bytes(executable[address..address + 4].try_into().unwrap()),
            scale.to_bits()
        );
    }
}

#[test]
#[ignore = "requires supplied executable and compiled native-sysse.c oracle"]
fn reference_native_sysse_filters_shifts_history_and_frame_tails() {
    let mut oracle = SysseOracle::new();
    for channels in [1, 2] {
        for filter in 0..5 {
            for shift in 0..16 {
                for [first, second] in [
                    [0, 0],
                    [32767, 32767],
                    [-32768, -32768],
                    [32767, -32768],
                    [-32768, 32767],
                    [1, -1],
                    [-32, 63],
                    [1024, -2048],
                ] {
                    for pattern in 0..5 {
                        let mut data = Vec::new();
                        for channel in 0..channels {
                            data.push((filter << 4) | shift);
                            for byte in 0..15 {
                                data.push(match pattern {
                                    0 => 0,
                                    1 => 0x77,
                                    2 => 0x88,
                                    3 => 0x78,
                                    _ => ((byte + channel * 7) as u8).wrapping_mul(17),
                                });
                            }
                        }
                        oracle.compare(channels, 30, &data, [first, second, second, first]);
                    }
                }
            }
        }
        for blocks in [0, 1, 2, 79, 80, 81, 159, 160, 161] {
            for tail in [0, 1, channels as usize * 16 - 1] {
                let mut data = Vec::new();
                for block in 0..blocks {
                    for channel in 0..channels {
                        data.push((((block + channel) % 5) << 4 | (block % 16)) as u8);
                        for byte in 0..15 {
                            data.push((block as u8).wrapping_mul(31).wrapping_add(byte * 17));
                        }
                    }
                }
                data.extend(std::iter::repeat_n(0x35, tail));
                for count in [0, 1, 29, 30, 31, 2399, 2400, 2401, 4799, 4800, 4801] {
                    oracle.compare(channels, count, &data, [127, -91, -53, 211]);
                }
            }
        }
    }
    let (streams, samples, blocks) = oracle.finish();
    eprintln!(
        "Native SYSSE: {streams} mono/stereo streams, {blocks} channel blocks, {samples} exact PCM16 samples and exact final histories; all five filters/all16shifts, saturation, nibble order,80block frames and declared-sample/partial-block tails"
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and compiled native-sysse.c oracle"]
fn reference_rom_all_sysse_native_pcm() {
    use crate::{
        format::rom::{IndexEntry, RomReader},
        primitives::stateless_reader::StatelessFile,
    };
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
    assert_eq!(files.len(), 109_791);
    let mut oracle = SysseOracle::new();
    let mut containers = 0;
    let mut sounds = 0;
    let mut mono = 0;
    let mut stereo = 0;
    let mut filters = [0usize; 16];
    for (path, file) in files {
        if file.size() < 4 {
            continue;
        }
        let mut reader = rom.open_file(file).unwrap();
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        if magic != *b"SYSE" {
            continue;
        }
        let mut data = magic.to_vec();
        reader.read_to_end(&mut data).unwrap();
        let sysse = super::super::read_sys_se(&data).unwrap_or_else(|e| panic!("Read {path}: {e}"));
        containers += 1;
        for (name, sound) in sysse.sounds {
            let channels = sound.channel_count as u32;
            assert!(channels == 1 || channels == 2);
            assert_eq!(sound.sample_data.len() % (16 * channels as usize), 0);
            for block in sound.sample_data.chunks_exact(16) {
                filters[(block[0] >> 4) as usize] += 1;
            }
            oracle.compare(channels, sound.sample_count, &sound.sample_data, [0; 4]);
            sounds += 1;
            if channels == 1 {
                mono += 1;
            } else {
                stereo += 1;
            }
            eprintln!(
                "Native SYSSE asset {path}/{name}: channels{channels},{} declared frames",
                sound.sample_count
            );
        }
    }
    assert_eq!((containers, sounds, mono, stereo), (1, 18, 5, 13));
    let (streams, samples, blocks) = oracle.finish();
    assert_eq!(streams, sounds);
    assert_eq!(blocks, filters.iter().sum::<usize>() as u64);
    assert_eq!((blocks, samples), (102_129, 3_063_403));
    eprintln!(
        "All supplied SYSSE: {containers} containers/{sounds} sounds ({mono} mono/{stereo} stereo), {blocks} channel blocks, {samples} exact native PCM16 samples, filters{filters:?}; integer decode/trim only, native PCM-to-float/mixer/device output excluded"
    );
}
