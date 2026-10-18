use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
};

fn root() -> PathBuf {
    std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_owned()
        })
}

fn run(name: &str, input: &str) -> String {
    let root = root();
    let out = root.join("reverse-engineering/gerokasu2/save-preview");
    std::fs::create_dir_all(&out).unwrap();
    let binary = std::env::var_os(if name.ends_with("-render") {
        "SHIN_GEROKASU2_SAVE_PREVIEW_RENDER_NATIVE"
    } else {
        "SHIN_GEROKASU2_SAVE_PREVIEW_NATIVE"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| out.join(name));
    if std::env::var_os("SHIN_GEROKASU2_SAVE_PREVIEW_PREBUILT").is_none() {
        assert!(
            Command::new("clang")
                .args(["-std=c11", "-O2"])
                .arg(root.join(format!("reverse-engineering/gerokasu2/{name}.c")))
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(out.join(format!("{name}.input")), input).unwrap();
    let mut child = Command::new(binary)
        .args([
            root.join("game-data/gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.as_bytes().to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join(format!("{name}.output")), &result.stdout).unwrap();
    std::fs::write(out.join(format!("{name}.stderr")), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

struct Driver {
    owner: Controller,
    labels: [u8; 100],
    label_y: [u32; 100],
    replay_starts: u32,
    texture_starts: u32,
    joins: u32,
}
impl Driver {
    fn new() -> Self {
        Self {
            owner: Controller::default(),
            labels: [4; 100],
            label_y: [0; 100],
            replay_starts: 0,
            texture_starts: 0,
            joins: 0,
        }
    }
    fn events(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::StartReplay(_) => self.replay_starts += 1,
                Event::StartTexture(_) => self.texture_starts += 1,
                Event::ReplayPublished(_) | Event::ReplayDiscarded(_) => self.joins += 1,
                Event::ClearLabels { slot } => self.labels[slot] = 4,
                Event::SetLabels { slot, labels } => {
                    self.labels[slot] = match labels {
                        Labels::Empty => 0,
                        Labels::Loading => 1,
                        Labels::Saved {
                            first_is_empty: false,
                        } => 2,
                        Labels::Saved {
                            first_is_empty: true,
                        } => 3,
                    };
                    self.label_y[slot] = (labels.first_line().1[1] as f32).to_bits();
                }
                _ => {}
            }
        }
    }
    fn line(&self, op: char) -> String {
        let mut s = format!(
            "P {op} {} {} {} {} {} {}",
            self.owner.replay.map_or(-1, |r| r.slot as i32),
            self.owner.discard_replay as u8,
            self.owner.texture.map_or(-1, |r| r.slot as i32),
            self.replay_starts,
            self.texture_starts,
            self.joins
        );
        for row in self.owner.rows {
            write!(
                s,
                " {}",
                u8::from(row.snapshot.is_some())
                    | (u8::from(row.texture) << 1)
                    | (u8::from(row.dirty) << 2)
            )
            .unwrap();
        }
        for slot in 0..100 {
            write!(s, " {} {}", self.labels[slot], self.label_y[slot]).unwrap();
        }
        s
    }
}

#[test]
#[ignore = "executes original complete SaveMenu update and bounded movie/cache blocks; CPU only"]
fn ger_native_save_preview_scheduler_and_movie_descriptors_match() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut frames = 0;
    for pattern in 0..4 {
        for replay_delay in [0, 2, 11] {
            for texture_delay in [0, 3, 17] {
                for base in [0., 373., 6000., 19296.] {
                    let mut driver = Driver::new();
                    let occupied = std::array::from_fn(|i| match pattern {
                        0 => false,
                        1 => i % 7 == 0,
                        2 => i % 3 == 0,
                        _ => true,
                    });
                    writeln!(input, "N {pattern}").unwrap();
                    expected.push(driver.line('N'));
                    let mut replay_age = 0;
                    let mut texture_age = 0;
                    let mut previous_replay = None;
                    let mut previous_texture = None;
                    for frame in 0..110 {
                        if [5, 24, 43, 68, 87].contains(&frame) {
                            let slot = driver.owner.replay.map_or((frame * 17) % 100, |r| r.slot);
                            writeln!(input, "I {slot}").unwrap();
                            let events = driver.owner.invalidate(slot);
                            driver.events(events);
                            expected.push(driver.line('I'));
                        }
                        let replay = driver.owner.replay_request();
                        let texture = driver.owner.texture_request();
                        replay_age = if replay.is_some() && replay == previous_replay {
                            replay_age + 1
                        } else {
                            0
                        };
                        texture_age = if texture.is_some() && texture == previous_texture {
                            texture_age + 1
                        } else {
                            0
                        };
                        previous_replay = replay;
                        previous_texture = texture;
                        let ready = replay.is_some() && replay_age >= replay_delay;
                        let texture_ready = texture.is_some() && texture_age >= texture_delay;
                        let first_empty = replay.is_some_and(|r| r.slot % 2 == 0);
                        let scroll = match frame {
                            0..=17 => base,
                            18..=34 => 0.,
                            35..=55 => 7000.,
                            56..=75 => base,
                            76..=90 => 19296.,
                            _ => 0.,
                        };
                        writeln!(
                            input,
                            "F {scroll} {} {} {}",
                            ready as u8, first_empty as u8, texture_ready as u8
                        )
                        .unwrap();
                        let completions = Completions {
                            replay: ready.then(|| (replay.unwrap(), first_empty)),
                            texture: texture_ready.then(|| texture.unwrap()),
                        };
                        let events = driver.owner.frame(scroll, &occupied, completions);
                        driver.events(events);
                        expected.push(driver.line('F'));
                        frames += 1;
                    }
                }
            }
        }
    }
    let mut conversions = 0;
    for kind in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
        for index in [i32::MIN, -7, -1, 0, 1, 2, 3, 99, i32::MAX] {
            for linked in [i16::MIN, -1, 0, 7, i16::MAX] {
                writeln!(input, "M {kind} {index} {linked}").unwrap();
                let descriptor = scene_descriptor(
                    Descriptor {
                        kind,
                        parameters: std::array::from_fn(|i| {
                            if i == 0 { index } else { 0x1020300 + i as i32 }
                        }),
                    },
                    |id| (0..3).contains(&id).then_some(linked),
                );
                let mut s = format!("M {}", descriptor.kind);
                for p in descriptor.parameters {
                    write!(s, " {p}").unwrap();
                }
                expected.push(s);
                conversions += 1;
            }
        }
    }
    let actual = run("native-save-preview", &input);
    let actual: Vec<_> = actual.lines().collect();
    assert_eq!(actual.len(), expected.len());
    for (index, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "observation{index}");
    }
    println!(
        "GER_SAVE_PREVIEW_RUST profiles=144 frames={frames} conversions={conversions} observations={} exact",
        expected.len()
    );
}

#[test]
#[ignore = "executes original preview target construction,ready capture/cache and fullscreen copies; CPU only"]
fn ger_native_save_preview_capture_cache_and_copy_packets_match() {
    let mut input = String::new();
    let targets = [
        [336, 189],
        [1, 1],
        [1920, 1080],
        [960, 540],
        [480, 270],
        [337, 193],
        [1280, 720],
        [2200, 1200],
    ];
    for [w, h] in targets {
        for status in 0..4 {
            for result in 0..2 {
                writeln!(input, "{w} {h} {status} {result}").unwrap();
            }
        }
    }
    let actual = run("native-save-preview-render", &input);
    let mut lines = actual.lines();
    let mut profiles = 0;
    let mut packets = 0;
    for target @ [w, h] in targets {
        for status in 0..4 {
            for result in 0..2 {
                assert_eq!(
                    lines.next().unwrap(),
                    format!("C {w} {h} {status} {result}")
                );
                let extents = capture_extents(target);
                let mut allocations = vec![extents[0]];
                allocations.extend_from_slice(&extents[..extents.len() - 1]);
                for (id, [aw, ah]) in allocations.iter().enumerate() {
                    assert_eq!(
                        lines.next().unwrap(),
                        format!("A {id} {aw} {ah} {}", u8::from(id == 0))
                    );
                }
                assert_eq!(lines.next(), Some("G"));
                assert_eq!(lines.next(), Some("S 4"));
                assert_eq!(lines.next(), Some("I 0"));
                let (final_id, draws, passes) = if status == 2 {
                    let id = allocations.len();
                    assert_eq!(lines.next().unwrap(), format!("A {id} {w} {h} 0"));
                    if result == 1 {
                        for expected in ["Z", "U 1 0", "Q", "B 1 1", "K 4 4278190080", "R", "E"] {
                            assert_eq!(lines.next(), Some(expected));
                        }
                        for dest in 2..=id {
                            assert_eq!(lines.next().unwrap(), format!("B {dest} 0"));
                            let line = lines.next().unwrap();
                            let words: Vec<u32> = line
                                .split_whitespace()
                                .skip(1)
                                .map(|x| x.parse().unwrap())
                                .collect();
                            assert_eq!(&words[..5], &[dest as u32, 2, 1, (dest - 1) as u32, 72]);
                            let matrix = [
                                2f32, 0., 0., 0., 0., -2., 0., 0., 0., 0., 1., 0., -1., 1., -0., 1.,
                            ]
                            .map(f32::to_bits);
                            for (a, e) in words[5..21].iter().zip(matrix) {
                                assert!(
                                    *a == e || f32::from_bits(*a) == 0. && f32::from_bits(e) == 0.
                                );
                            }
                            assert_eq!(
                                &words[21..],
                                &[
                                    0,
                                    0,
                                    1f32.to_bits(),
                                    u32::MAX,
                                    0,
                                    0,
                                    2f32.to_bits(),
                                    0,
                                    1f32.to_bits(),
                                    u32::MAX,
                                    2f32.to_bits(),
                                    0,
                                    0,
                                    2f32.to_bits(),
                                    1f32.to_bits(),
                                    u32::MAX,
                                    0,
                                    2f32.to_bits()
                                ]
                            );
                            assert_eq!(lines.next(), Some("E"));
                            packets += 1;
                        }
                        (id as i32, 1, id)
                    } else {
                        assert_eq!(lines.next().unwrap(), format!("B {id} 0"));
                        assert_eq!(lines.next(), Some("K 1 4278190080"));
                        assert_eq!(lines.next(), Some("E"));
                        (id as i32, 0, 1)
                    }
                } else {
                    (-1, 0, 0)
                };
                let count = allocations.len() + usize::from(status == 2);
                for poll in 0..2 {
                    assert_eq!(
                        lines.next().unwrap(),
                        format!("F {poll} {final_id} {count} {draws} {passes}")
                    );
                }
                profiles += 1;
            }
        }
    }
    assert!(lines.next().is_none());
    println!("GER_SAVE_PREVIEW_CAPTURE_RUST profiles={profiles} copy_packets={packets} exact");
}
