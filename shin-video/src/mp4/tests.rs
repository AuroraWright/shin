use super::*;
use shin_core::{
    format::rom::{IndexEntry, RomReader},
    primitives::stateless_reader::StatelessFile,
};
use std::{collections::BTreeMap, fmt::Write as _, fs, io::Cursor, path::Path, process::Command};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Packet {
    time: i64,
    bytes: usize,
    hash: u64,
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

fn samples<S: Read + Seek>(track: &mut Mp4TrackReader<S>) -> (u32, Vec<Packet>) {
    let (id, scale) = track.get_mp4_track_info(|t| (t.track_id(), t.timescale()));
    let mut packets = Vec::new();
    while let Some(sample) = track.next_sample().unwrap() {
        let pts = i128::from(sample.start_time) + i128::from(sample.rendering_offset);
        // av_rescale_q rounds to the nearest microsecond, unlike duration().
        let time = (pts * 1_000_000 + i128::from(scale / 2)) / i128::from(scale);
        packets.push(Packet {
            time: time.try_into().unwrap(),
            bytes: sample.bytes.len(),
            hash: hash(&sample.bytes),
        });
    }
    assert!(track.next_sample().unwrap().is_none());
    track.rewind();
    assert_eq!(
        track.next_sample().unwrap().map(|s| hash(&s.bytes)),
        packets.first().map(|p| p.hash)
    );
    (id, packets)
}

struct Trace {
    duration: u64,
    channels: u32,
    rate: u32,
    tracks: BTreeMap<u32, Vec<Packet>>,
    count: usize,
}

fn trace(text: &str, name: &str, audio: bool) -> Trace {
    let mut lines = text.lines();
    let meta: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    assert_eq!(meta.len(), 7);
    assert_eq!(&meta[..2], &["M", name]);
    assert_eq!(meta[3], "0");
    assert_eq!(meta[4], if audio { "1" } else { "-1" });
    let mut result = Trace {
        duration: meta[2].parse().unwrap(),
        channels: meta[5].parse().unwrap(),
        rate: meta[6].parse().unwrap(),
        tracks: BTreeMap::new(),
        count: 0,
    };
    loop {
        let row: Vec<_> = lines.next().unwrap().split_whitespace().collect();
        if row[0] == "E" {
            assert_eq!(row.len(), 6);
            result.count = row[1].parse().unwrap();
            assert_eq!(row[2], "2", "first two H.264 frames decoded");
            assert_eq!(row[3], if audio { "2" } else { "0" });
            assert_eq!(row[4], if audio { "2048" } else { "0" });
            assert!(
                row[5].parse::<u32>().unwrap() > 0,
                "native source callback exercised"
            );
            break;
        }
        assert_eq!(row.len(), 5);
        assert_eq!(row[0], "P");
        result
            .tracks
            .entry(row[1].parse().unwrap())
            .or_default()
            .push(Packet {
                time: row[2].parse().unwrap(),
                bytes: row[3].parse().unwrap(),
                hash: u64::from_str_radix(row[4], 16).unwrap(),
            });
    }
    assert_eq!(
        lines.next().unwrap(),
        format!("D {name}"),
        "native close/destruction balanced"
    );
    assert!(lines.next().is_none());
    assert_eq!(
        result.count,
        result.tracks.values().map(Vec::len).sum::<usize>()
    );
    result
}

#[test]
#[ignore = "requires supplied original/1.03 executables, ROMs and libavformat/libavcodec; CPU only"]
fn reference_native_movie_preparation_packets_and_retirement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let supplied = root.join("game-data/umineko");
    let out = std::env::var_os("SHIN_NATIVE_MOVIE_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-movie-{}", std::process::id()))
        });
    fs::create_dir_all(&out).unwrap();
    let helper = out.join("movie-load");
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
            .arg(root.join("reverse-engineering/native-movie-load.c"))
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
    let mut summary = String::from(
        "rom\tmovie\traw_duration_us\tedited_duration_us\ttrack\traw_packets\tedited_packets\tpts_shift_us\n",
    );
    let mut movies = 0;
    let mut packet_count = 0;
    for archive in ["data.rom", "patch.rom"] {
        let mut rom = RomReader::new(
            StatelessFile::new(fs::File::open(supplied.join(archive)).unwrap()).unwrap(),
        )
        .unwrap();
        let names: Vec<_> = rom
            .traverse()
            .filter_map(|(name, entry)| {
                (name.ends_with(".mp4") && matches!(entry, IndexEntry::File(_))).then_some(name)
            })
            .collect();
        for name in names {
            let file = rom.find_file(&name).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let asset = out.join("current.mp4");
            fs::write(&asset, &bytes).unwrap();
            let input = out.join("input.txt");
            fs::write(&input, format!("0 {} {name}\n", bytes.len())).unwrap();
            let mut mp4 = Mp4::new(Cursor::new(bytes)).unwrap();
            let duration = mp4.duration_us();
            let audio = mp4.audio_track.is_some();
            let (channels, rate) = mp4.audio_track.as_ref().map_or((0, 0), |t| {
                t.get_mp4_track_info(|t| {
                    (
                        t.channel_config().unwrap() as u32,
                        t.sample_freq_index().unwrap().freq(),
                    )
                })
            });
            let mut expected = BTreeMap::from([samples(&mut mp4.video_track)]);
            if let Some(t) = &mut mp4.audio_track {
                let (id, packets) = samples(t);
                expected.insert(id, packets);
            }
            let mut version_traces = Vec::new();
            for version in ["uncompressed_executable", "uncompressed_executable_update"] {
                let mut modes = Vec::new();
                for raw in [true, false] {
                    let stem = format!(
                        "{archive}-{}-{version}-{}",
                        name.trim_start_matches('/').replace('/', "_"),
                        if raw { "raw" } else { "edited" }
                    );
                    let trace_path = out.join(format!("{stem}.trace"));
                    let log = out.join(format!("{stem}.log"));
                    let mut cmd = Command::new(&helper);
                    cmd.arg(supplied.join(version)).arg(&asset);
                    if raw {
                        cmd.arg("raw");
                    }
                    let status = cmd
                        .stdin(fs::File::open(&input).unwrap())
                        .stdout(fs::File::create(&trace_path).unwrap())
                        .stderr(fs::File::create(&log).unwrap())
                        .status()
                        .unwrap();
                    assert!(
                        status.success(),
                        "{stem}: {}",
                        fs::read_to_string(&log).unwrap()
                    );
                    let output = fs::read_to_string(&trace_path).unwrap();
                    let native = trace(&output, &name, audio);
                    assert_eq!((native.channels, native.rate), (channels, rate), "{stem}");
                    if raw {
                        assert!(
                            native.duration.abs_diff(duration) <= 1,
                            "{stem}: native {} vs port {duration}",
                            native.duration
                        );
                        assert_eq!(
                            native.tracks, expected,
                            "{stem}: complete compressed packet metadata, bytes hashes and presentation timestamps"
                        );
                    } else {
                        // Edited SDK semantics are measured separately; they are
                        // not attributed to the unavailable Nintendo SDK.
                        assert!(native.duration <= duration + 1, "{stem}");
                        for (&id, edited) in &native.tracks {
                            let raw = &expected[&id];
                            let mut at = 0;
                            let mut shift: Option<i64> = None;
                            for packet in edited {
                                while at < raw.len()
                                    && (raw[at].hash, raw[at].bytes) != (packet.hash, packet.bytes)
                                {
                                    at += 1;
                                }
                                assert!(
                                    at < raw.len(),
                                    "{stem}: edited payload not in raw track {id}"
                                );
                                let delta = packet.time - raw[at].time;
                                if let Some(s) = shift {
                                    assert!(
                                        (delta - s).abs() <= 1,
                                        "{stem}: edit-list shift changes"
                                    );
                                } else {
                                    shift = Some(delta);
                                }
                                at += 1;
                            }
                            if version == "uncompressed_executable" {
                                writeln!(
                                    summary,
                                    "{archive}\t{name}\t{duration}\t{}\t{id}\t{}\t{}\t{}",
                                    native.duration,
                                    raw.len(),
                                    edited.len(),
                                    shift.unwrap()
                                )
                                .unwrap();
                            }
                        }
                    }
                    modes.push(output);
                }
                version_traces.push(modes);
            }
            assert_eq!(
                version_traces[0], version_traces[1],
                "{archive} {name}: original/update native traces"
            );
            movies += 1;
            packet_count += expected.values().map(Vec::len).sum::<usize>();
            println!(
                "{archive} {name}: {} packets; raw metadata/PTS/payloads match, edited SDK measured, both native owners retired",
                expected.values().map(Vec::len).sum::<usize>()
            );
        }
    }
    assert!(movies > 0);
    fs::write(out.join("summary.tsv"), summary).unwrap();
    println!(
        "Native Movie: {movies} supplied files, {packet_count} raw packets/version; original/update identical in both SDK modes; output:{}",
        out.display()
    );
}
