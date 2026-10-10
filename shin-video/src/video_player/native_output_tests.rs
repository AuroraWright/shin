use super::*;
use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
use std::{collections::VecDeque, fmt::Write as _, fs, path::Path, process::Command};

#[derive(Clone, Copy)]
struct Case {
    kind: u32,
    count: u32,
    bytes: usize,
    step: u64,
    eos: u64,
    duration: u64,
    base: u64,
    flags: u32,
    error: bool,
    free: u32,
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Packet {
    id: u32,
    time: u64,
    bytes: usize,
    hash: u64,
}

fn packet(case: Case, id: u32) -> Packet {
    let bytes: Vec<_> = (0..case.bytes)
        .map(|i| (17 * case.kind as usize + 29 * id as usize + i) as u8)
        .collect();
    Packet {
        id,
        time: case.base + u64::from(id) * case.step,
        bytes: bytes.len(),
        hash: hash(&bytes),
    }
}

fn parse_packet(row: &[&str]) -> Packet {
    assert_eq!(row.len(), 5);
    Packet {
        id: row[1].parse().unwrap(),
        time: row[2].parse().unwrap(),
        bytes: row[3].parse().unwrap(),
        hash: u64::from_str_radix(row[4], 16).unwrap(),
    }
}

fn check_case(case: Case, index: usize, lines: &mut std::str::Lines<'_>) -> (usize, usize) {
    let start = case.flags & 1 != 0;
    let paused = case.flags & 2 != 0;
    let looping = case.flags & 4 != 0;
    let mut buffering = case.flags & 8 == 0;
    let mut starts = u32::from(start && !buffering);
    let mut pauses = u32::from(starts == 1 && paused);
    let initial = vec![
        index as u32,
        case.kind,
        u32::from(start),
        u32::from(paused),
        u32::from(buffering),
        starts - pauses,
    ];
    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(row[0], "I");
    assert_eq!(
        row[1..]
            .iter()
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>(),
        initial
    );
    let mut active = starts > pauses;
    let mut queued: VecDeque<Packet> = VecDeque::new();
    let mut consumed = 0;
    let mut cancelled = false;
    let mut released = case.count;
    'frames: for id in 0..case.count {
        loop {
            let full = if case.kind == 1 {
                queued
                    .front()
                    .zip(queued.back())
                    .is_some_and(|(a, b)| b.time - a.time >= 100_000)
            } else {
                // Native audio threshold is one fifth of a second. Video
                // readiness controls the buffering gate while video exists.
                queued.iter().map(|p| p.bytes / 4).sum::<usize>() >= 48_000 / 5
            };
            if !full {
                break;
            }
            if case.kind == 1 && buffering {
                buffering = false;
                if start && !paused && !active {
                    starts += 1;
                    active = true;
                }
            }
            if case.flags & 16 != 0 {
                for expected in &queued {
                    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
                    assert_eq!(row[0], "R");
                    assert_eq!(
                        parse_packet(&row),
                        *expected,
                        "queued owners before close in condition wait"
                    );
                }
                assert_eq!(
                    lines.next().unwrap(),
                    "X 0 0 2",
                    "native close clears alive/delegate; external owner retains source while worker exits"
                );
                queued.clear();
                cancelled = true;
                released = id + 1;
                break 'frames;
            }
            let expected = queued.pop_front().unwrap();
            let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(row[0], "C");
            assert_eq!(
                parse_packet(&row),
                expected,
                "native FIFO consumer and owned bytes"
            );
            consumed += 1;
        }
        queued.push_back(packet(case, id));
    }
    let padding = if !case.error && !cancelled && case.kind == 4 && case.eos < case.duration {
        let samples = (case.duration - case.eos) * 48_000 / 1_000_000;
        let bytes = samples as usize * 4;
        queued.push_back(Packet {
            id: case.count,
            time: case.base + case.eos,
            bytes,
            hash: hash(&vec![0; bytes]),
        });
        Some(samples as usize)
    } else {
        None
    };
    if case.error && active {
        pauses += 1;
        active = false;
    }
    for expected in &queued {
        let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
        assert_eq!(row[0], "Q");
        assert_eq!(
            parse_packet(&row),
            *expected,
            "native queued copy/metadata or EOF silence"
        );
    }
    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(row[0], "O");
    let fields: Vec<u32> = row[1..].iter().map(|s| s.parse().unwrap()).collect();
    assert_eq!(
        fields,
        vec![
            consumed,
            released,
            consumed + u32::from(cancelled),
            u32::from(!case.error && !cancelled),
            u32::from(start),
            u32::from(paused),
            u32::from(buffering),
            u32::from(case.error),
            u32::from(active),
            starts,
            pauses
        ]
    );
    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(row[0], "F");
    assert_eq!(
        row[1..]
            .iter()
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>(),
        (0..=case.free).collect::<Vec<_>>(),
        "input slots returned exactly once through native vector growth"
    );
    let audio_eof = Arc::new(AtomicBool::new(false));
    let deadline = MovieDeadline {
        duration: case.duration,
        audio_eof: Some(audio_eof.clone()),
    };
    for video in [false, true] {
        for audio in [false, true] {
            audio_eof.store(audio, Ordering::Release);
            for buffered in [false, true] {
                for time in [case.duration - 1, case.duration, case.duration + 1] {
                    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
                    assert_eq!(row[0], "T");
                    let numbers: Vec<u64> = row[1..].iter().map(|s| s.parse().unwrap()).collect();
                    // A buffering native clock returns zero. Compare the
                    // actual port deadline with that effective clock value.
                    let effective_time = if buffered { 0 } else { time };
                    let expected =
                        case.error || (!looping && deadline.is_finished(video, effective_time));
                    assert_eq!(
                        numbers,
                        vec![
                            u64::from(video),
                            u64::from(audio),
                            u64::from(buffered),
                            time,
                            u64::from(expected)
                        ],
                        "port/native completion flags and deadline"
                    );
                }
            }
        }
    }
    if active {
        pauses += 1;
    }
    let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(row[0], "S");
    assert_eq!(
        row[1..]
            .iter()
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 0, starts, pauses, starts, pauses],
        "native stop suspends voice/clock once"
    );
    assert_eq!(lines.next().unwrap(), format!("D {index}"));
    if let Some(samples) = padding {
        compare_port_silence(case, samples);
    }
    (consumed as usize, queued.len())
}

fn compare_port_silence(case: Case, native_samples: usize) {
    use shin_core::format::audio::{AudioBuffer, AudioFrameSource};
    struct FinitePcm {
        left: u32,
        samples: usize,
        position: u32,
    }
    impl AudioFrameSource for FinitePcm {
        fn max_frame_size(&self) -> usize {
            self.samples.max(1)
        }
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, out: &mut AudioBuffer) -> bool {
            if self.left == 0 {
                return false;
            }
            self.left -= 1;
            self.position += self.samples as u32;
            out.extend(std::iter::repeat_n((0.25, -0.25), self.samples));
            true
        }
        fn samples_seek(&mut self, _: u32) -> anyhow::Result<u32> {
            anyhow::bail!("Not a seek case")
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }
    let movie = crate::audio::MovieAudioSource::new(FinitePcm {
        left: case.count,
        samples: case.bytes / 4,
        position: 0,
    });
    let eof = movie.decoder_eof();
    let mut source = shin_core::format::audio::AudioSource::new(movie);
    for _ in 0..case.count as usize * (case.bytes / 4) {
        assert_eq!(source.read_sample(), Some((0.25, -0.25)));
        assert!(!eof.load(Ordering::Acquire));
    }
    // Native allocates a finite tail; the port supplies chunks until its movie
    // owner reaches the same deadline. Compare the native tail's entire span.
    for _ in 0..native_samples.max(1) {
        assert_eq!(source.read_sample(), Some((0.0, 0.0)));
        assert!(eof.load(Ordering::Acquire));
    }
}

#[test]
#[ignore = "requires supplied original/1.03 executables, ROM and libavformat/libavcodec; CPU only"]
fn reference_native_movie_output_queues_eof_pause_and_retirement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let supplied = root.join("umineko_switch_files");
    let out = std::env::var_os("SHIN_NATIVE_MOVIE_OUTPUT_WORKER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-movie-workers-{}", std::process::id()))
        });
    fs::create_dir_all(&out).unwrap();
    let helper = out.join("movie-output");
    let flags = Command::new("pkg-config")
        .args([
            "--cflags",
            "--libs",
            "libavformat",
            "libavcodec",
            "libavutil",
        ])
        .output()
        .unwrap();
    assert!(flags.status.success());
    assert!(
        Command::new("clang")
            .args(["-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(root.join("reverse-engineering/native-movie-output.c"))
            .args(
                std::str::from_utf8(&flags.stdout)
                    .unwrap()
                    .split_whitespace()
            )
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut rom = RomReader::new(
        StatelessFile::new(fs::File::open(supplied.join("data.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/movie/op1.mp4").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let asset = out.join("op1.mp4");
    fs::write(&asset, bytes).unwrap();
    let mut cases = Vec::new();
    for kind in [1, 4] {
        for flags in 0..16 {
            cases.push(Case {
                kind,
                count: if kind == 1 { 8 } else { 12 },
                bytes: if kind == 1 { 16 } else { 4096 },
                step: if kind == 1 { 50_000 } else { 21_333 },
                eos: if kind == 1 { 400_000 } else { 256_000 },
                duration: 500_000,
                base: 700_000,
                flags,
                error: false,
                free: 16,
            });
        }
        for flags in [1, 3, 9, 15] {
            cases.push(Case {
                kind,
                count: 8,
                bytes: 8192,
                step: 50_000,
                eos: 400_000,
                duration: 500_000,
                base: 0,
                flags,
                error: true,
                free: 4,
            });
        }
        // Cross native deque blocks, independent of timing pressure.
        cases.push(Case {
            kind,
            count: 64,
            bytes: 4,
            step: 0,
            eos: 100_000,
            duration: 200_000,
            base: 300_000,
            flags: 9,
            error: false,
            free: 0,
        });
        cases.push(Case {
            kind,
            count: 0,
            bytes: 4,
            step: 0,
            eos: 0,
            duration: 100_000,
            base: 0,
            flags: 9,
            error: true,
            free: 0,
        });
        cases.push(Case {
            kind,
            count: if kind == 1 { 8 } else { 12 },
            bytes: if kind == 1 { 16 } else { 4096 },
            step: if kind == 1 { 50_000 } else { 21_333 },
            eos: 400_000,
            duration: 500_000,
            base: 0,
            flags: 25,
            error: false,
            free: 4,
        });
    }
    for eos in [
        149_999, 150_000, 150_001, 149_980, 149_979, 149_978, 64_000, 160_000,
    ] {
        cases.push(Case {
            kind: 4,
            count: 3,
            bytes: 4096,
            step: 21_333,
            eos,
            duration: 150_000,
            base: 0,
            flags: 9,
            error: false,
            free: 0,
        });
    }
    let mut input = String::new();
    for c in &cases {
        writeln!(
            input,
            "{} {} {} {} {} {} {} {} {} {}",
            c.kind,
            c.count,
            c.bytes,
            c.step,
            c.eos,
            c.duration,
            c.base,
            c.flags,
            u32::from(c.error),
            c.free
        )
        .unwrap();
    }
    let input_path = out.join("input.txt");
    fs::write(&input_path, input).unwrap();
    let mut traces = Vec::new();
    let mut totals = (0, 0);
    for version in ["uncompressed_executable", "uncompressed_executable_update"] {
        let trace = out.join(format!("{version}.trace"));
        let log = out.join(format!("{version}.log"));
        let status = Command::new(&helper)
            .arg(supplied.join(version))
            .arg(&asset)
            .stdin(fs::File::open(&input_path).unwrap())
            .stdout(fs::File::create(&trace).unwrap())
            .stderr(fs::File::create(&log).unwrap())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "{version}: {}",
            fs::read_to_string(&log).unwrap()
        );
        let output = fs::read_to_string(&trace).unwrap();
        let mut lines = output.lines();
        let mut counts = (0, 0);
        for (index, &case) in cases.iter().enumerate() {
            let (consumed, queued) = check_case(case, index, &mut lines);
            counts.0 += consumed;
            counts.1 += queued;
        }
        assert!(lines.next().is_none());
        if traces.is_empty() {
            totals = counts;
        } else {
            assert_eq!(counts, totals);
        }
        traces.push(output);
    }
    assert_eq!(
        traces[0], traces[1],
        "original/update native worker and owner traces"
    );
    println!(
        "Native Movie output: {} cases/version; {} consumed/{} retained packets; {} completion gates; real port deadline/silence comparisons; source/SDK slots/wait holders/native allocations retired; output:{}",
        cases.len(),
        totals.0,
        totals.1,
        cases.len() * 24,
        out.display()
    );
}
