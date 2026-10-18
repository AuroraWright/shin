//! Independent live VM2530 -> actual menu factories/full owned snapshot oracle.
use super::super::Prepared;
use crate::{
    adv::higurashi::autosave::{AutosaveServices, Execution, ProgressEntry, SaveProgress},
    app::higurashi::{
        gameplay::bindings::Bindings,
        runtime_game_data::RuntimeGameData,
        runtime_saves::{Slot, Slots},
    },
};
use shin_core::{
    format::save::{
        PersistData,
        higurashi::{SaveData, SaveDate},
    },
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
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
const HASH: u64 = 0xcbf29ce484222325;
fn add(mut h: u64, word: u32) -> u64 {
    for byte in word.to_le_bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    h
}
fn data_hash(mut h: u64, slot: &Slot) -> u64 {
    h = add(h, u32::from(slot.valid));
    let d = &slot.data;
    let date = crate::app::higurashi::gameplay::bindings::packed_date(d.date);
    h = add(h, date as u32);
    h = add(h, (date >> 32) as u32);
    h = add(h, d.progress.active_index() as u32);
    h = add(h, d.progress.extra_word);
    for e in &d.progress.entries {
        for word in [
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
        ] {
            h = add(h, word);
        }
        h = add(h, e.response_data.len() as u32);
        for &b in &e.response_data {
            h = add(h, u32::from(b));
        }
    }
    h = add(h, d.trailing_values.len() as u32);
    for &v in &d.trailing_values {
        h = add(h, u32::from(v));
    }
    h = add(h, d.byte_variables.len() as u32);
    for &b in &d.byte_variables {
        h = add(h, u32::from(b));
    }
    h
}
fn slots_hash(slots: &Slots, settings: &SaveData) -> u64 {
    std::iter::once(&slots.auto)
        .chain(&slots.manual)
        .fold(add(HASH, u32::from(settings.save_menu_position)), data_hash)
}
fn current_hash(prepared: &Prepared) -> u64 {
    if let Some(data) = prepared.current() {
        data_hash(
            HASH,
            &Slot {
                valid: true,
                data: data.clone(),
            },
        )
    } else {
        data_hash(HASH, &Slots::loaded(&SaveData::default()).auto)
    }
}
struct Calendar {
    calls: u32,
}
impl AutosaveServices for Calendar {
    fn packed_time(&mut self) -> anyhow::Result<u64> {
        self.calls += 1;
        Ok(0x003b3a170d0a07ea)
    }
    fn notify(&mut self, _: &str) -> anyhow::Result<()> {
        panic!("11780 must not create a notification");
    }
}
struct Expected {
    op: char,
    source: u64,
    current: u64,
    slots: u64,
    persistent: usize,
    bytes: usize,
    calendar: u32,
}
fn snapshot(
    op: char,
    bindings: &Bindings,
    prepared: &Prepared,
    slots: &Slots,
    settings: &SaveData,
    calendar: &Calendar,
) -> Expected {
    Expected {
        op,
        source: data_hash(
            HASH,
            &Slot {
                valid: true,
                data: bindings.runtime_game_data().unwrap(),
            },
        ),
        current: current_hash(prepared),
        slots: slots_hash(slots, settings),
        persistent: bindings.progress.pending_persistent().len(),
        bytes: bindings.progress.pending_bytes().len(),
        calendar: calendar.calls,
    }
}

#[test]
#[ignore = "original SystemMenu1c804/b2090/b20f0/full GameData menu snapshot with live native pending commit/11780 boundaries"]
fn higurashi_live_save_menu_preparation_copies_vm2530_without_commits_or_date_refresh() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for origin in [0, 1, 2] {
        for salt in [0, 1, 2, 7, 31, 100] {
            for selected in [0, 49, 99] {
                for enabled in [false, true] {
                    let mut settings = SaveData::default();
                    settings.save_menu_position = selected;
                    settings.persist_data =
                        PersistData((0..8).map(|i| (0x8000 + i * 11 + salt) as i16).collect());
                    settings.save_vectors.seen_messages = vec![0xdeadbeef, 0x87654321];
                    settings.save_vectors.select_seen_choices = vec![0x98765432];
                    settings.save_vectors.select_last_choices = vec![255, 128, 7];
                    let mut slots = Slots {
                        auto: profile(0, salt),
                        manual: std::array::from_fn(|i| profile(i as u32 + 1, salt)),
                    };
                    let source = profile(102, salt).data;
                    let date = source.date;
                    let mut bindings = Bindings::new_runtime(&settings, &source).unwrap();
                    bindings.autosave = slots.autosave();
                    bindings.memory.commits_enabled = enabled;
                    assert!(
                        bindings
                            .progress
                            .dispatch_variables(
                                &Cmd::SSET {
                                    args: [2, (70000 + salt) as i32]
                                },
                                Execution::Live,
                                0
                            )
                            .unwrap()
                            .is_some()
                    );
                    assert!(
                        bindings
                            .progress
                            .dispatch_variables(
                                &Cmd::OpcodeDB {
                                    args: [0, (511 + salt) as i32]
                                },
                                Execution::Live,
                                0
                            )
                            .unwrap()
                            .is_some()
                    );
                    let source = bindings.runtime_game_data().unwrap();
                    let histories = (
                        bindings.history.bits().words().to_vec(),
                        bindings.choices.seen().words().to_vec(),
                        bindings.choices.last().to_vec(),
                    );
                    let prepared = Prepared::new(
                        (origin == 0).then_some(&source),
                        &settings,
                        &slots,
                        true,
                        (1280, 720),
                    )
                    .unwrap();
                    assert_eq!(prepared.current().is_some(), origin == 0);
                    let immutable = current_hash(&prepared);
                    let mut calendar = Calendar { calls: 0 };
                    input += &format!("N {origin} {selected} 1 {salt} {}\n", u32::from(enabled));
                    expected.push(snapshot(
                        'N', &bindings, &prepared, &slots, &settings, &calendar,
                    ));
                    owners += 1;
                    for (frame, micros, delta) in [(1, 0, 1), (2, 10000, 10), (3, 20000, 1)] {
                        input += &format!("F {frame} {micros} 1 0 0 {delta} 0\n");
                        expected.push(snapshot(
                            'F', &bindings, &prepared, &slots, &settings, &calendar,
                        ));
                    }
                    // Source mutations after the factory copy cannot alias the retained menu.
                    input += "M\n";
                    let active = bindings.progress.progress().active_index();
                    let e = &mut bindings.progress.progress_mut().entries[active];
                    e.random_seed ^= 0xa5a5a5a5;
                    e.response_data[0] ^= 0x5a;
                    expected.push(snapshot(
                        'M', &bindings, &prepared, &slots, &settings, &calendar,
                    ));
                    assert_eq!(current_hash(&prepared), immutable);
                    input += "C\n";
                    bindings.progress.commit_pending(enabled).unwrap();
                    expected.push(snapshot(
                        'C', &bindings, &prepared, &slots, &settings, &calendar,
                    ));
                    assert_eq!(
                        bindings.progress.pending_persistent().len(),
                        usize::from(!enabled)
                    );
                    assert_eq!(
                        bindings.progress.pending_bytes().len(),
                        usize::from(!enabled)
                    );
                    input += "A 123456\n";
                    bindings
                        .autosave
                        .capture_message_start(&mut bindings.progress, 123456, &mut calendar)
                        .unwrap();
                    slots.update_auto(&bindings.autosave);
                    expected.push(snapshot(
                        'A', &bindings, &prepared, &slots, &settings, &calendar,
                    ));
                    assert_eq!(bindings.date, date);
                    assert_eq!(current_hash(&prepared), immutable);
                    input += "F 4 30000 1 0 0 0 0\n";
                    expected.push(snapshot(
                        'F', &bindings, &prepared, &slots, &settings, &calendar,
                    ));
                    input += "X\n";
                    assert_eq!(
                        histories,
                        (
                            bindings.history.bits().words().to_vec(),
                            bindings.choices.seen().words().to_vec(),
                            bindings.choices.last().to_vec()
                        )
                    );
                }
            }
        }
    }
    let native = std::env::var("HIGURASHI_SAVE_MENU_OWNER_NATIVE")
        .unwrap_or_else(|_| "/tmp/higurashi-native-save-menu-owner".into());
    let font = std::env::var("HIGURASHI_NEWRODIN_FNT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    let mut child = Command::new(native)
        .args(["game-data/higurashi/uncompressed_executable_update", &font])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let data = input.clone();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_MENU_OWNER_CAPTURE") {
        let p = std::path::Path::new(&dir);
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join("native-input.txt"), &input).unwrap();
        std::fs::write(p.join("native-output.txt"), &output.stdout).unwrap();
        std::fs::write(p.join("native-stderr.txt"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = stdout.lines().filter(|r| r.starts_with("O ")).collect();
    assert_eq!(rows.len(), expected.len());
    for (index, (native, e)) in rows.iter().zip(&expected).enumerate() {
        let w: Vec<_> = native.split_whitespace().collect();
        assert_eq!(w[1], e.op.to_string());
        assert_eq!(
            u64::from_str_radix(w[15], 16).unwrap(),
            e.source,
            "Source payload observation{index}"
        );
        assert_eq!(
            u64::from_str_radix(w[16], 16).unwrap(),
            e.current,
            "Prepared menu payload observation{index}"
        );
        assert_eq!(
            u64::from_str_radix(w[17], 16).unwrap(),
            e.slots,
            "Complete101-slot observation{index}"
        );
        assert_eq!(
            w[21].parse::<usize>().unwrap(),
            e.persistent,
            "Pending persistent queue{index}"
        );
        assert_eq!(
            w[22].parse::<usize>().unwrap(),
            e.bytes,
            "Pending byte queue{index}"
        );
        assert_eq!(
            w[6].parse::<u32>().unwrap(),
            e.calendar,
            "Calendar observation{index}"
        );
        assert_eq!(w[23], "1", "Both native modes enable long press");
    }
    eprintln!(
        "Live SaveMenu preparation:{owners} native owners/{} exact complete source-menu-global snapshot observations; production Bindings/Prepared, pending commits/date preservation, independent two-record/fullword/u16/byte payloads and original factories pass",
        expected.len()
    );
}

#[test]
fn higurashi_save_menu_preparation_rejects_invalid_physical_extent_and_saved_selection() {
    let mut settings = SaveData::default();
    let slots = Slots::loaded(&settings);
    for extent in [(0, 720), (1280, 0), (0, 0)] {
        assert!(Prepared::new(None, &settings, &slots, true, extent).is_err());
    }
    for index in [100, 127, 255] {
        settings.save_menu_position = index;
        assert!(Prepared::new(None, &settings, &slots, true, (1280, 720)).is_err());
    }
}
