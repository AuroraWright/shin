//! Comparisons consume separately frozen original execution, never regenerate it.
use super::command::{Command, Services};
use crate::adv::higurashi::{
    autosave::{AutosaveMemory, AutosaveServices, ProgressEntry, SaveProgress},
    progress::ProgressMemory,
    unlock,
};
use anyhow::Result;
use shin_core::format::save::{PersistData, higurashi::SaveData};
use std::path::PathBuf;

#[derive(Default)]
struct Transport {
    child: bool,
    done: bool,
    clocks: u32,
    events: Vec<u32>,
    fail_clock: bool,
}
impl AutosaveServices for Transport {
    fn packed_time(&mut self) -> Result<u64> {
        anyhow::ensure!(!self.fail_clock, "controlled unavailable calendar");
        self.clocks += 1;
        self.events.push(8);
        Ok(0x00123456789abcde + u64::from(self.clocks))
    }
    fn notify(&mut self, _: &str) -> Result<()> {
        panic!("FAKESELECT11780 has no notification");
    }
}
impl Services for Transport {
    fn open(&mut self) -> Result<()> {
        assert!(!self.child);
        self.child = true;
        // Explicit checked factory/priority/attach transport, matching the
        // native command probe. Real widget and Gameplay have separate proofs.
        self.events.extend([2, 3, 4]);
        Ok(())
    }
    fn callback_received(&self) -> bool {
        self.done
    }
}
fn bytes(seed: u32, count: usize) -> Vec<u8> {
    (0..count)
        .map(|i| seed.wrapping_add(i as u32 * 17) as u8)
        .collect()
}
fn values(seed: u32, count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|i| (seed.wrapping_add(i as u32 * 17) as u16).to_le_bytes())
        .collect()
}
fn entries(seed: u32, auto: bool) -> [ProgressEntry; 2] {
    std::array::from_fn(|entry| {
        let word = |i: u32| {
            if auto { !seed } else { seed }
                .wrapping_add(entry as u32 * if auto { 13 } else { 31 })
                .wrapping_add(i * if auto { 7 } else { 11 })
        };
        ProgressEntry {
            scenario_id: word(0) as i32,
            random_seed: word(1),
            save_position: word(2),
            replay_target: word(3),
            response_data: bytes(
                seed.wrapping_add(entry as u32 * if auto { 23 } else { 19 }),
                if auto { 2 + entry } else { 3 + entry * 4 },
            ),
        }
    })
}
fn fixture(active: usize, seed: u32) -> (ProgressMemory, AutosaveMemory) {
    (
        ProgressMemory::new(
            PersistData(vec![0; 64]),
            SaveProgress::new(active, seed ^ 0x12345678, entries(seed, false)).unwrap(),
            bytes(seed.wrapping_add(1), 11),
        )
        .with_trailing_bytes(values(seed, 7)),
        AutosaveMemory {
            valid: false,
            packed_time: 0,
            progress: SaveProgress::new(0, 0, entries(seed, true)).unwrap(),
        },
    )
}
// Canonical field transport only. This makes no claim about Rust object ABI.
fn hash_data(
    progress: &SaveProgress,
    valid: bool,
    time: u64,
    trailing: &[u8],
    variables: &[u8],
) -> String {
    let mut raw = vec![0u8; 24];
    raw[0] = u8::from(valid);
    raw[2..10].copy_from_slice(&time.to_le_bytes());
    raw[12..16].copy_from_slice(&(progress.active_index() as u32).to_le_bytes());
    raw[16..20].copy_from_slice(&progress.extra_word.to_le_bytes());
    for entry in &progress.entries {
        for value in [
            entry.scenario_id as u32,
            entry.random_seed,
            entry.save_position,
            entry.replay_target,
        ] {
            raw.extend(value.to_le_bytes());
        }
    }
    for (data, element) in [
        (progress.entries[0].response_data.as_slice(), 1),
        (progress.entries[1].response_data.as_slice(), 1),
        (trailing, 2),
        (variables, 1),
    ] {
        raw.extend(((data.len() / element) as u32).to_le_bytes());
        raw.extend(data);
    }
    let hash = raw.into_iter().fold(0xcbf29ce484222325u64, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
fn number<'a>(words: &mut impl Iterator<Item = &'a str>) -> u32 {
    words.next().unwrap().parse().unwrap()
}
fn hex<'a>(words: &mut impl Iterator<Item = &'a str>) -> u32 {
    u32::from_str_radix(words.next().unwrap(), 16).unwrap()
}
fn banks(settings: &SaveData) -> [&[u32]; 3] {
    [
        &settings.save_vectors.unlocked_cgs,
        &settings.save_vectors.unlocked_bgms,
        &settings.save_vectors.unlocked_movies,
    ]
}
fn retire(kind: &mut Option<char>, command: &mut Option<Command>, services: &mut Transport) {
    if kind.take().is_some() {
        services.events.push(1);
        *command = None;
        if services.child {
            services.events.extend([6, 7]);
            services.child = false;
        }
        services.done = false;
    }
}

#[test]
#[ignore = "frozen updated T/U/L/D/F normal+adjusted constructors/live/replay/forward, acquired banks and11780;CPU only"]
fn higurashi_command_tail_matches_frozen_native_progress_and_gallery_banks() {
    let reference = PathBuf::from(std::env::var("SHIN_HIGURASHI_COMMAND_TAIL_REFERENCE").unwrap());
    let input = std::fs::read_to_string(reference.join("commands-input.txt")).unwrap();
    let output = std::fs::read_to_string(reference.join("commands-output.txt")).unwrap();
    assert_eq!(input.lines().count(), output.lines().count());
    let (mut progress, mut memory) = fixture(0, 0);
    let mut settings = SaveData::default();
    let mut services = Transport::default();
    let (mut profile, mut sequence, mut seed, mut boundary) = (0, 0, 0, 0);
    let mut command = None;
    let mut kind = None;
    let (mut profiles, mut live_unlocks, mut live_fake, mut observations) = (0, 0, 0, 0);
    for (request, observed) in input.lines().zip(output.lines()) {
        let mut words = request.split_whitespace();
        let op = words.next().unwrap();
        let native: serde_json::Value = serde_json::from_str(observed).unwrap();
        let mut result = -1i32;
        let mut mode = 0;
        if op == "N" {
            assert!(kind.is_none());
            profile = number(&mut words);
            let active = number(&mut words) as usize;
            seed = hex(&mut words);
            boundary = hex(&mut words);
            (progress, memory) = fixture(active, seed);
            settings = SaveData::default();
            services = Transport::default();
            sequence = 0;
            profiles += 1;
        } else {
            sequence += 1;
            match op {
                "B" => {
                    let bank = number(&mut words);
                    let length = number(&mut words) as usize;
                    let capacity = number(&mut words) as usize;
                    let mut values: Vec<_> = (0..capacity).map(|_| hex(&mut words)).collect();
                    values.truncate(length);
                    *match bank {
                        0 => &mut settings.save_vectors.unlocked_cgs,
                        1 => &mut settings.save_vectors.unlocked_bgms,
                        2 => &mut settings.save_vectors.unlocked_movies,
                        _ => unreachable!(),
                    } = values;
                }
                "X" => retire(&mut kind, &mut command, &mut services),
                "C" => {
                    assert_eq!(kind, Some('F'));
                    assert!(services.child);
                    services.done = true;
                    services.events.push(5);
                }
                "P" => {
                    mode = number(&mut words);
                    result = if mode == 0 {
                        i32::from(
                            command
                                .as_mut()
                                .unwrap()
                                .poll_live(&mut progress, &mut memory, boundary, &mut services)
                                .unwrap(),
                        )
                    } else {
                        1
                    };
                }
                _ => {
                    let adjusted = number(&mut words);
                    assert!(adjusted < 2);
                    retire(&mut kind, &mut command, &mut services);
                    kind = Some(op.chars().next().unwrap());
                    match op {
                        "T" => {
                            let _: i32 = words.next().unwrap().parse().unwrap();
                            mode = number(&mut words);
                            // Original endpoint only locks/unlocks its platform
                            // mutex. Desktop acknowledges with no platform work.
                            if mode == 0 {
                                services.events.extend([10, 11]);
                            }
                            result = 1;
                        }
                        "L" => {
                            let _raw = words.next().unwrap();
                            mode = number(&mut words);
                            result = 1;
                        }
                        "D" => {
                            let count = number(&mut words);
                            let _format = words.next().unwrap();
                            for _ in 0..count {
                                let _: i32 = words.next().unwrap().parse().unwrap();
                            }
                            mode = number(&mut words);
                            result = 1;
                        }
                        "U" => {
                            let bank = number(&mut words) as u8;
                            let count = number(&mut words);
                            let ids: Vec<i32> = (0..count)
                                .map(|_| words.next().unwrap().parse().unwrap())
                                .collect();
                            mode = number(&mut words);
                            if mode == 0 {
                                unlock::poll(bank, &ids, Some(&mut settings)).unwrap();
                                live_unlocks += 1;
                            }
                            result = 1;
                        }
                        "F" => {
                            mode = number(&mut words);
                            command = Some(Command::new());
                            result = if mode == 0 {
                                live_fake += 1;
                                i32::from(
                                    command
                                        .as_mut()
                                        .unwrap()
                                        .poll_live(
                                            &mut progress,
                                            &mut memory,
                                            boundary,
                                            &mut services,
                                        )
                                        .unwrap(),
                                )
                            } else {
                                1
                            };
                        }
                        _ => panic!("command-tail input {op}"),
                    }
                }
            }
        }
        assert!(words.next().is_none(), "unused input {request}");
        assert_eq!(native["op"], op);
        assert_eq!(native["profile"], profile);
        assert_eq!(native["sequence"], sequence);
        assert_eq!(native["mode"], mode);
        assert_eq!(native["result"], result, "{request}");
        assert_eq!(
            native["events"],
            serde_json::json!(services.events),
            "{request}"
        );
        for (bank, actual) in banks(&settings).into_iter().enumerate() {
            assert_eq!(native["banks"][bank]["length"], actual.len(), "{request}");
            assert_eq!(
                native["banks"][bank]["words"],
                serde_json::json!(actual),
                "{request}"
            );
            // Nintendo allocator capacity is outside this logical bank proof.
            assert!(native["banks"][bank]["capacity"].as_u64().unwrap() >= actual.len() as u64);
        }
        assert_eq!(
            native["current_data_hash"],
            hash_data(
                progress.progress(),
                true,
                0x0011223344556677,
                progress.trailing_bytes(),
                progress.bytes()
            ),
            "{request}"
        );
        assert_eq!(
            native["auto_data_hash"],
            hash_data(
                &memory.progress,
                memory.valid,
                memory.packed_time,
                &values(!seed, 5),
                &bytes((!seed).wrapping_add(1), 9)
            ),
            "{request}"
        );
        assert_eq!(native["calendar_calls"], services.clocks, "{request}");
        assert_eq!(native["fake_child"], u32::from(services.child));
        assert_eq!(native["fake_done"], u32::from(services.done));
        assert_eq!(native["fake_refs"], if services.child { 2 } else { 0 });
        assert_eq!(native["overlay"], u32::from(services.child));
        assert_eq!(progress.responses().1, 0);
        assert!(progress.pending_persistent().is_empty());
        assert!(progress.pending_bytes().is_empty());
        services.events.clear();
        observations += 1;
    }
    assert!(kind.is_none());
    assert!(profiles >= 100 && live_unlocks >= 20 && live_fake >= 4);
    eprintln!(
        "Command-tail:{profiles} frozen native profiles/{observations} states;{live_unlocks} live UNLOCK/{live_fake} first-live FAKESELECT;logical acquired bank lengths+words,full current/autosave fields+responses+u16/byte tails,normal/adjusted/replay/forward/calendar/owner transport pass;T/L/D no-effect endpoint observations checked,actual desktop dispatch has separate Gameplay proof"
    );
}

#[test]
fn higurashi_fake_select_clock_failure_retains_attachment_and_native_copy_prefix() {
    let (mut progress, mut memory) = fixture(1, 42);
    let before = memory.clone();
    let source = progress.progress().clone();
    let mut services = Transport {
        fail_clock: true,
        ..Default::default()
    };
    let mut command = Command::new();
    assert!(
        command
            .poll_live(&mut progress, &mut memory, 999, &mut services)
            .is_err()
    );
    assert_eq!(services.events, [2, 3, 4]);
    assert!(services.child && memory.valid);
    assert_eq!(memory.progress, before.progress);
    assert_eq!(memory.packed_time, before.packed_time);
    assert_eq!(progress.progress().entries[0], source.entries[0]);
    assert_eq!(
        progress.progress().entries[1].response_data,
        source.entries[1].response_data
    );
    assert_eq!(progress.progress().active_entry().save_position, 999);
    assert_eq!(progress.progress().active_entry().replay_target, 999);
    services.fail_clock = false;
    assert!(
        !command
            .poll_live(&mut progress, &mut memory, 999, &mut services)
            .unwrap()
    );
    assert_eq!(services.events, [2, 3, 4, 8]);
    let captured = memory.clone();
    services.done = true;
    for _ in 0..3 {
        assert!(
            command
                .poll_live(&mut progress, &mut memory, 1000, &mut services)
                .unwrap()
        );
        assert_eq!(memory, captured);
    }
    assert_eq!(services.clocks, 1);
    let mut saved = SaveData::default();
    let before = saved.clone();
    assert!(unlock::poll(0, &[1, -1], Some(&mut saved)).is_err());
    assert_eq!(saved.encode_raw().unwrap(), before.encode_raw().unwrap());
    assert!(unlock::poll(255, &[-1], None).is_ok());
    assert!(unlock::poll(0, &[], None).is_ok());
}
