use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn hash(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn bytes(length: usize, seed: u32) -> Vec<u8> {
    (0..length)
        .map(|i| seed.wrapping_add(i as u32 * 17) as u8)
        .collect()
}
fn progress(active: usize, seed: u32, length: usize, destination: bool) -> SaveProgress {
    SaveProgress::new(
        active,
        if destination {
            !seed
        } else {
            seed ^ 0x12345678
        },
        std::array::from_fn(|i| {
            let base = if destination {
                (!seed).wrapping_add(i as u32 * 7)
            } else {
                seed.wrapping_add(i as u32 * 31)
            };
            let step = if destination { 3 } else { 11 };
            ProgressEntry {
                scenario_id: base as i32,
                random_seed: base.wrapping_add(step),
                save_position: base.wrapping_add(step * 2),
                replay_target: base.wrapping_add(step * 3),
                response_data: bytes(
                    length + i,
                    if destination {
                        (!seed).wrapping_add(i as u32 * 23)
                    } else {
                        seed.wrapping_add(i as u32 * 19)
                    },
                ),
            }
        }),
    )
    .unwrap()
}
fn append_progress(out: &mut String, p: &SaveProgress) {
    out.push_str(&format!(" {} {:08x}", p.active_index(), p.extra_word));
    for entry in &p.entries {
        out.push_str(&format!(
            " {:08x} {:08x} {:08x} {:08x} {} {:016x}",
            entry.scenario_id as u32,
            entry.random_seed,
            entry.save_position,
            entry.replay_target,
            entry.response_data.len(),
            hash(&entry.response_data)
        ));
    }
}
struct Services {
    time: u64,
    clocks: usize,
    notifications: Vec<String>,
    fail_clock: bool,
    fail_publish: bool,
    publications: usize,
    fail_notify: bool,
}
impl AutosaveServices for Services {
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock unavailable");
        self.clocks += 1;
        Ok(self.time)
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        ensure!(!self.fail_notify, "notification unavailable");
        self.notifications.push(text.into());
        Ok(())
    }
    fn captured(&mut self, _: &AutosaveMemory) -> Result<()> {
        ensure!(!self.fail_publish, "publication unavailable");
        self.publications += 1;
        Ok(())
    }
}
fn services(time: u64) -> Services {
    Services {
        time,
        clocks: 0,
        notifications: Vec::new(),
        fail_clock: false,
        fail_publish: false,
        publications: 0,
        fail_notify: false,
    }
}
fn hex(text: &str) -> String {
    if text.is_empty() {
        "-".into()
    } else {
        text.bytes().map(|b| format!("{b:02x}")).collect()
    }
}

#[test]
#[ignore = "original updated AUTOSAVE constructor, replay/live polls, unsigned saved boundary and11780 deep copies"]
fn higurashi_autosave_matches_native_poll_flags_copy_order_and_retained_vectors() {
    let labels = [
        (String::new(), String::new()),
        ("Title".into(), String::new()),
        (String::new(), "Chapter".into()),
        ("T".repeat(23), "C".repeat(22)),
        ("Title".repeat(25), "Chapter".repeat(29)),
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for active in 0..2 {
        for seed in [0u32, 1, u32::MAX] {
            for initial in [0u32, 1, 199, 200, 0x80000000, u32::MAX - 1, u32::MAX] {
                for (length, destination_length) in [
                    (0, 0),
                    (1, 64),
                    (64, 1),
                    (1023, 2048),
                    (2048, 1023),
                    (4095, 4095),
                ] {
                    for (first, second) in &labels {
                        profiles += 1;
                        let time = 0x0056341200000000u64 ^ u64::from(seed);
                        input.push_str(&format!("N {active} {seed:x} {length} {destination_length} {initial:x} {seed:x} {time:x} {} {}\n", hex(first), hex(second)));
                        let mut source = progress(active, seed, length, false);
                        source.entries[active].save_position = seed;
                        source.entries[active].replay_target = seed ^ 0xff;
                        let mut memory = AutosaveMemory {
                            valid: false,
                            packed_time: !time,
                            progress: progress(1 - active, seed, destination_length, true),
                        };
                        let mut owner = AutosaveCommand::new();
                        let mut metadata = super::super::save_metadata::SaveMetadata::new();
                        metadata.set(0, first).unwrap();
                        metadata.set(1, second).unwrap();
                        metadata.capture_chapter();
                        let mut platform = services(time);
                        let mut boundary = initial;
                        let sentinels = [
                            hash(&bytes(13, seed)),
                            hash(&bytes(17, seed.wrapping_add(1))),
                            hash(&bytes(19, !seed)),
                            hash(&bytes(23, (!seed).wrapping_add(1))),
                        ];
                        for op in ['N', 'R', 'R', 'B', 'R', 'L', 'M'] {
                            let mut result = false;
                            match op {
                                'N' => {}
                                'R' => {
                                    input.push_str("R\n");
                                    result = owner.poll_replay(&source, boundary).unwrap();
                                }
                                'B' => {
                                    boundary = seed;
                                    input.push_str(&format!("B {boundary:x}\n"));
                                }
                                'L' => {
                                    input.push_str("L\n");
                                    result = owner
                                        .poll_live(
                                            &mut source,
                                            &mut memory,
                                            boundary,
                                            &metadata,
                                            &mut platform,
                                        )
                                        .unwrap();
                                }
                                'M' => {
                                    input.push_str("M\n");
                                    for e in &mut source.entries {
                                        e.scenario_id ^= 0xff;
                                        for byte in &mut e.response_data {
                                            *byte ^= 0x55;
                                        }
                                    }
                                }
                                _ => unreachable!(),
                            }
                            let [armed, reached] = owner.flags();
                            let notice = platform
                                .notifications
                                .last()
                                .map(String::as_str)
                                .unwrap_or("");
                            let mut row = format!(
                                "T {} {armed} {reached} 1 {} {} {} {:016x}",
                                u8::from(result),
                                platform.notifications.len(),
                                u8::from(memory.valid),
                                if platform.clocks == 0 { "K" } else { "KCEN" },
                                memory.packed_time
                            );
                            append_progress(&mut row, &source);
                            append_progress(&mut row, &memory.progress);
                            for v in sentinels {
                                row.push_str(&format!(" {v:016x}"));
                            }
                            row.push_str(&format!(
                                " {:016x} 00123456789abcde",
                                hash(notice.as_bytes())
                            ));
                            expected.push(row);
                        }
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_AUTOSAVE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (i, (line, want)) in native.lines().zip(&expected).enumerate() {
        assert_eq!(line, want, "Autosave snapshot{i}");
    }
    println!(
        "Higurashi autosave:{} native profiles/{} snapshots match; constructor flags, unsigned active saved boundary, repeated replay,11780 two-boundary/two-entry copies, vector growth/shrink/tails, source/sink independence and clock/ASCII-codec/notification order pass; checked services, no disk writes",
        profiles,
        expected.len()
    );
}

#[test]
fn autosave_service_failure_retries_notification_without_recopying_progress() {
    let mut source = progress(1, 42, 17, false);
    let mut memory = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: progress(0, 123, 64, true),
    };
    let before = source.clone();
    let before_memory = memory.clone();
    let mut metadata = super::super::save_metadata::SaveMetadata::new();
    metadata.set(0, "ひぐらし").unwrap();
    metadata.set(1, "章").unwrap();
    metadata.capture_chapter();
    let mut platform = services(42);
    platform.fail_clock = true;
    let mut owner = AutosaveCommand::new();
    assert!(
        owner
            .poll_live(&mut source, &mut memory, 9, &metadata, &mut platform)
            .is_err()
    );
    assert_eq!(source.active_entry().save_position, 9);
    assert_eq!(source.active_entry().replay_target, 9);
    assert_eq!(source.entries[0], before.entries[0]);
    assert_eq!(
        source.entries[1].response_data,
        before.entries[1].response_data
    );
    assert!(memory.valid);
    assert_eq!(memory.progress, before_memory.progress);
    assert_eq!(memory.packed_time, before_memory.packed_time);
    assert!(owner.poll_replay(&source, 9).is_err());
    assert_eq!(owner.flags(), [1, 0]);
    platform.fail_clock = false;
    platform.fail_publish = true;
    assert!(
        owner
            .poll_live(&mut source, &mut memory, 9, &metadata, &mut platform)
            .is_err()
    );
    assert_eq!(memory.progress, source);
    assert_eq!(platform.clocks, 1);
    assert_eq!(platform.publications, 0);
    assert!(platform.notifications.is_empty());
    let published_copy = memory.clone();
    platform.time = 999;
    platform.fail_publish = false;
    platform.fail_notify = true;
    assert!(
        owner
            .poll_live(&mut source, &mut memory, 9, &metadata, &mut platform)
            .is_err()
    );
    assert_eq!(source.active_entry().save_position, 9);
    assert_eq!(source.active_entry().replay_target, 9);
    assert_eq!(memory.progress, source);
    assert_eq!(memory, published_copy);
    assert_eq!(platform.clocks, 1);
    assert_eq!(platform.publications, 1);
    let captured = memory.clone();
    source.entries[1].response_data.push(0xff);
    metadata.set(0, "Later").unwrap();
    metadata.capture_chapter();
    platform.time = 999;
    platform.fail_notify = false;
    assert!(
        owner
            .poll_live(&mut source, &mut memory, 10, &metadata, &mut platform)
            .unwrap()
    );
    assert_eq!(memory, captured);
    assert_eq!(platform.clocks, 1);
    assert_eq!(platform.publications, 1);
    assert_eq!(platform.notifications, ["ひぐらし 章"]);
    assert!(
        owner
            .poll_live(&mut source, &mut memory, 11, &metadata, &mut platform)
            .unwrap()
    );
    assert_eq!(platform.clocks, 1);
    assert_eq!(platform.notifications.len(), 1);
    assert!(SaveProgress::new(2, 0, before.entries).is_err());
}
