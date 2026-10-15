use super::*;
use crate::adv::higurashi::autosave::{ProgressEntry, SaveProgress};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn profile(i: u32, salt: u32) -> Slot {
    Slot {
        valid: (i + salt) % 3 != 0,
        data: RuntimeGameData::from_progress(
            SaveProgress::new(
                ((i + salt) & 1) as usize,
                0xfedc0000 + i * 17 + salt,
                std::array::from_fn(|j| {
                    let j = j as u32;
                    ProgressEntry {
                        scenario_id: (100 + i * 13 + j * 10000 + salt) as i32,
                        random_seed: 0x87650000 + i * 31 + j * 7 + salt,
                        save_position: 0x234000 + i * 100 + j * 19 + salt,
                        replay_target: 0x234100 + i * 100 + j * 23 + salt,
                        response_data: (0..3 + (i + j + salt) % 7)
                            .map(|k| (i * 11 + j * 17 + k * 37 + salt) as u8)
                            .collect(),
                    }
                }),
            )
            .unwrap(),
            SaveDate {
                year: (2000 + i + salt) as u16,
                month: (1 + i % 12) as u8,
                day: (1 + i % 28) as u8,
                hour: (i % 24) as u8,
                minute: ((i * 3) % 60) as u8,
                second: ((i * 7) % 60) as u8,
            },
            (0..4 + (i + salt) % 5)
                .map(|k| (0xf800 + i * 31 + k * 977 + salt) as u16)
                .collect(),
            (0..7 + (i + salt) % 9)
                .map(|k| (i * 19 + k * 41 + salt) as u8)
                .collect(),
        ),
    }
}
fn add(mut h: u64, word: u32) -> u64 {
    for byte in word.to_le_bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    h
}
const HASH: u64 = 0xcbf29ce484222325;
fn data_hash(mut h: u64, slot: &Slot) -> u64 {
    h = add(h, u32::from(slot.valid));
    let date = super::super::super::gameplay::bindings::packed_date(slot.data.date);
    h = add(h, date as u32);
    h = add(h, (date >> 32) as u32);
    h = add(h, slot.data.progress.active_index() as u32);
    h = add(h, slot.data.progress.extra_word);
    for entry in &slot.data.progress.entries {
        for word in [
            entry.scenario_id as u32,
            entry.random_seed,
            entry.save_position,
            entry.replay_target,
        ] {
            h = add(h, word);
        }
        h = add(h, entry.response_data.len() as u32);
        for byte in &entry.response_data {
            h = add(h, u32::from(*byte));
        }
    }
    h = add(h, slot.data.trailing_values.len() as u32);
    for word in &slot.data.trailing_values {
        h = add(h, u32::from(*word));
    }
    h = add(h, slot.data.byte_variables.len() as u32);
    for byte in &slot.data.byte_variables {
        h = add(h, u32::from(*byte));
    }
    h
}
fn slots_hash(slots: &Slots, settings: &SaveData) -> u64 {
    std::iter::once(&slots.auto)
        .chain(&slots.manual)
        .fold(add(HASH, u32::from(settings.save_menu_position)), data_hash)
}
fn observation(
    slots: &Slots,
    settings: &SaveData,
    current: &RuntimeGameData,
    written: u64,
    queued: &Slot,
) -> String {
    format!(
        "T {:016x} {:016x} {written:016x} {:016x}",
        slots_hash(slots, settings),
        data_hash(
            HASH,
            &Slot {
                valid: true,
                data: current.clone()
            }
        ),
        data_hash(HASH, queued)
    )
}

#[test]
#[ignore = "actual updated Save/Delete/Load callbacks and complete101 runtime rows; requires native-save-menu-model executable"]
fn higurashi_manual_slot_transactions_and_full_load_copies_match_native() {
    let now = SaveDate {
        year: 2026,
        month: 10,
        day: 13,
        hour: 23,
        minute: 58,
        second: 59,
    };
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    let empty = Slots::loaded(&SaveData::default()).auto;
    for salt in [0, 1, 2, 7, 31, 100] {
        for remembered in [0, 49, 99] {
            for selected in [0, 1, 49, 98, 99] {
                for action in ['S', 'D', 'L'] {
                    for success in [false, true] {
                        if action == 'L' && !profile(selected as u32 + 1, salt).valid {
                            continue;
                        }
                        let mut settings = SaveData::default();
                        settings.save_menu_position = remembered;
                        settings.extra_word = 0x1234cdef;
                        settings.persist_data.0 = vec![13, -1, 7];
                        let mut slots = Slots {
                            auto: profile(0, salt),
                            manual: std::array::from_fn(|i| profile(i as u32 + 1, salt)),
                        };
                        let mut current = profile(102, salt).data;
                        let mut written = 0;
                        let mut queued = empty.clone();
                        input += &format!("N 0 {remembered} 1 {salt}\nF 1 1\nF 1 10\nF 1 1\nT\n");
                        expected.push(observation(&slots, &settings, &current, written, &queued));
                        if action == 'L' {
                            // Stage0 is an intermediate Question notification, not a load.
                            input += &format!("L {selected} 0 0\nT\n");
                            expected
                                .push(observation(&slots, &settings, &current, written, &queued));
                            input += &format!("L {selected} {} 1\nT\n", u32::from(!success));
                            if success {
                                current = slots.load_manual(&mut settings, selected).unwrap();
                            }
                            expected
                                .push(observation(&slots, &settings, &current, written, &queued));
                            if success {
                                input += "F 1 10\nF 1 1\nT\n";
                                queued = Slot {
                                    valid: true,
                                    data: current.clone(),
                                };
                                expected.push(observation(
                                    &slots, &settings, &current, written, &queued,
                                ));
                            }
                        } else {
                            // A No answer must leave every row and menu currentData untouched.
                            input += &format!("{action} {selected} 1\nT\n");
                            expected
                                .push(observation(&slots, &settings, &current, written, &queued));
                            input += &format!("{action} {selected} 0\nT\n");
                            let write = if action == 'S' {
                                slots
                                    .begin_save(&mut settings, selected, &mut current, now)
                                    .unwrap()
                            } else {
                                slots.begin_delete(&settings, selected).unwrap()
                            };
                            let frozen = write.snapshot();
                            written = slots_hash(&frozen.slots, &frozen.settings);
                            expected
                                .push(observation(&slots, &settings, &current, written, &queued));
                            // The writer keeps the full snapshot and its disk bytes even after
                            // completion rolls back the source or a later owner changes globals.
                            let disk = frozen.encode().unwrap();
                            input += &format!("W {}\nT\nF 1 0\n", u32::from(success));
                            write.complete(&mut slots, &mut settings, success);
                            expected
                                .push(observation(&slots, &settings, &current, written, &queued));
                            settings.extra_word = 0;
                            settings.persist_data.0.clear();
                            slots.auto.data.progress.extra_word = 0;
                            slots.manual[selected].data.progress.entries[1]
                                .response_data
                                .clear();
                            assert_eq!(slots_hash(&frozen.slots, &frozen.settings), written);
                            assert_eq!(frozen.encode().unwrap(), disk);
                            assert_eq!(frozen.settings.extra_word, 0x1234cdef);
                            assert_eq!(frozen.settings.persist_data.0, [13, -1, 7]);
                        }
                        input += "X\n";
                        owners += 1;
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_MENU_MODEL").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output_text = String::from_utf8(output.stdout).unwrap();
    let actual: Vec<_> = output_text
        .lines()
        .filter(|l| l.starts_with("T "))
        .collect();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "native complete-row transaction observation {i}");
    }
    assert_eq!(output_text.lines().filter(|l| *l == "X").count(), owners);
    let dir = "/tmp/higurashi-save-menu-transactions-capture";
    std::fs::create_dir_all(dir).unwrap();
    for (name, bytes) in [
        ("input.txt", input.as_bytes()),
        ("output.txt", output_text.as_bytes()),
        ("stderr.txt", output.stderr.as_slice()),
    ] {
        std::fs::write(format!("{dir}/{name}"), bytes).unwrap();
    }
    println!(
        "Manual slots:{owners} native owners/{} exact complete101-row hashes;both active/dormant records,invalid nonempty backups,fullword/u16/byte banks,Save/Delete success-failure,No answers,final-stage Load/flags0 factory,alias-free fullsettings/frozen disk snapshots pass",
        expected.len()
    );
}
