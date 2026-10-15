//! Complete original native save bodies, including both encryption stages.
use super::*;

const VERSION_PROBES: [&str; 6] = ["00", "02", "03", "7f", "80", "ff"];
fn native_lines() -> Vec<&'static str> {
    include_str!("native-golden.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect()
}
fn crypto_input(n: u32) -> Vec<u8> {
    (0..n).map(|i| (i * 71 + n * 17) as u8).collect()
}
fn profile(n: u32) -> SaveData {
    let mut data = SaveData::default();
    if n == 0 {
        return data;
    }
    data.save_menu_position = ((n * 13) & 127) as u8;
    data.play_seconds = 0xf0000000 + n * 103;
    data.persist_data.0 = (0..n % 5).map(|i| (0x8000 + n * 17 + i) as i16).collect();
    let words = |salt: u32| {
        (0..(n + salt) % 5)
            .map(|i| 0x80000001 ^ (n * 0x12345) ^ (salt * 0x10203) ^ (i * 0xabcdef))
            .collect()
    };
    let bytes = |salt: u32, count: u32| {
        (0..count)
            .map(|i| (n * 19 + salt * 7 + i * 13) as u8)
            .collect::<Vec<_>>()
    };
    data.save_vectors = SaveVectors {
        seen_messages: words(0),
        select_seen_choices: words(1),
        select_last_choices: bytes(2, (n + 2) % 5).into_iter().map(|v| v & 15).collect(),
        unlocked_cgs: words(3),
        unlocked_bgms: words(4),
        unlocked_movies: words(5),
    };
    let v = |i: u32, bits: u32| ((n * 17 + i * 23) & ((1 << bits) - 1)) as u8;
    data.settings = Settings {
        v0_bgmvol: v(0, 7),
        v1_sfxvol: v(1, 7),
        v2_voicevol: v(2, 7),
        v3_sysvol: v(3, 7),
        v4_voicefocus: v(4, 1) != 0,
        v5_voicepanapot: v(5, 1) != 0,
        v6: v(6, 1) != 0,
        v7: v(7, 2),
        v8: v(8, 2),
        v9_msgspeed: v(9, 7),
        v10_skipspeed: v(10, 7),
        v11_disallowskipunread: v(11, 1) != 0,
        v12: v(12, 1) != 0,
        v13_msgwinalpha: v(13, 7),
        v14_showroutenavi: v(14, 1) != 0,
        v15: v(15, 1) != 0,
        v16_showtoucheffect: v(16, 1) != 0,
        v17_showscenetitle: v(17, 1) != 0,
        v18_showsongtitle: v(18, 1) != 0,
        v19: 0x81234567 ^ n,
    };
    let slot = |i: u32| {
        if n % 3 == 0 && i % 3 == 1 {
            return None;
        }
        let active = ((n + i) & 1) as usize;
        let entry = |e| ProgressEntry {
            scenario_id: (0x80000000u32 + n + i * 17 + e) as i32,
            random_seed: 0x12345678 + n * 19 + i + e,
            save_position: 0x87654321 + n + i * 31 + e,
            replay_target: 0xdeadbeef,
            responses: bytes(i + e, (n + i + e) % 9),
        };
        let date = if n == 17 {
            // Original reader accepts raw fields, even outside civil time.
            SaveDate {
                year: 4095,
                month: 0,
                day: 31,
                hour: 31,
                minute: 63,
                second: 63,
            }
        } else {
            SaveDate {
                year: (2026 + n % 3) as u16,
                month: (1 + i % 12) as u8,
                day: (1 + i % 28) as u8,
                hour: (i % 24) as u8,
                minute: ((i * 7) % 60) as u8,
                second: ((i * 11) % 60) as u8,
            }
        };
        Some(GameData {
            date,
            progress: Progress::new(
                active,
                [
                    entry(0),
                    if active == 1 {
                        entry(1)
                    } else {
                        ProgressEntry::default()
                    },
                ],
            )
            .unwrap(),
        })
    };
    data.auto_save_slot = slot(0);
    data.manual_save_slots = std::array::from_fn(|i| slot(i as u32 + 1));
    data
}
fn normalized_slot(slot: &Option<GameData>) -> Option<GameData> {
    slot.as_ref().map(|data| {
        let mut data = data.clone();
        let active = data.progress.active_index();
        for entry in &mut data.progress.entries[..=active] {
            entry.replay_target = entry.save_position;
        }
        data
    })
}
fn assert_profile(line: &str) {
    let fields: Vec<_> = line.split_whitespace().collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0], "P");
    let n = fields[1].parse::<u32>().unwrap();
    let native_position = fields[2].parse::<usize>().unwrap();
    assert_eq!(fields[3].parse::<u32>().unwrap(), GAME_KEY);
    let original = profile(n);
    let native_raw = hex::decode(fields[4]).unwrap();
    assert_eq!(
        original.encode_raw().unwrap(),
        native_raw,
        "native raw profile {n}"
    );
    let mut reader = BitBuffer::with_data(native_raw, 0);
    let decoded = SaveData::read(&mut reader).unwrap();
    assert_eq!(
        reader.bit_position(),
        native_position,
        "native cursor profile {n}"
    );
    assert_eq!(decoded.save_menu_position, original.save_menu_position);
    assert_eq!(decoded.play_seconds, original.play_seconds);
    assert_eq!(decoded.persist_data.0, original.persist_data.0);
    assert_eq!(decoded.save_vectors, original.save_vectors);
    assert!(decoded.settings.serialized_eq(&original.settings));
    assert_eq!(
        decoded.auto_save_slot,
        normalized_slot(&original.auto_save_slot)
    );
    for (i, (before, after)) in original
        .manual_save_slots
        .iter()
        .zip(&decoded.manual_save_slots)
        .enumerate()
    {
        assert_eq!(
            *after,
            normalized_slot(before),
            "native slot {i}, profile {n}"
        );
    }
    assert_eq!(
        decoded.encode_raw().unwrap(),
        original.encode_raw().unwrap()
    );
    let native_encrypted = hex::decode(fields[5]).unwrap();
    assert_eq!(
        original.encode().unwrap(),
        native_encrypted,
        "native encrypted profile {n}"
    );
    assert_eq!(
        SaveData::decode(&native_encrypted)
            .unwrap()
            .encode()
            .unwrap(),
        native_encrypted
    );
    assert!(super::super::super::SaveData::decode(&native_encrypted).is_err());
}

#[test]
fn ger_full_save_codec_matches_33_complete_native_profiles() {
    assert_eq!(GAME_KEY, 0x144965dd);
    let lines = native_lines();
    assert_eq!(lines.len(), 73);
    for line in &lines[..33] {
        assert_profile(line);
    }
}

#[test]
fn ger_save_versions_match_original_acceptance_and_cursor() {
    let lines = native_lines();
    for (input, line) in VERSION_PROBES.iter().zip(&lines[33..39]) {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields[0], "D");
        let mut reader = BitBuffer::with_data(hex::decode(input).unwrap(), 0);
        let decoded = SaveData::read(&mut reader);
        assert_eq!(decoded.is_ok(), fields[1] == "1");
        assert_eq!(reader.bit_position(), fields[2].parse::<usize>().unwrap());
        match decoded {
            Ok(save) => assert_eq!(save.encode_raw().unwrap(), hex::decode(fields[3]).unwrap()),
            Err(error) => assert_eq!(
                error.downcast_ref::<UnsupportedSaveVersion>().unwrap().0,
                u32::from_str_radix(input, 16).unwrap()
            ),
        }
    }
}

#[test]
fn ger_crypto_matches_original_with_every_word_tail_and_empty_input() {
    let lines = native_lines();
    for (n, line) in lines[39..].iter().enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields[0], "T");
        let raw = crypto_input(n as u32);
        let native_encrypted = hex::decode(fields[1]).unwrap();
        assert_eq!(
            obfuscation::encode(&raw, GAME_KEY),
            native_encrypted,
            "native crypto length {n}"
        );
        assert_eq!(
            obfuscation::decode(&native_encrypted, GAME_KEY).unwrap(),
            raw
        );
    }
}

#[test]
fn ger_save_rejects_corrupt_truncated_and_unrepresentable_data() {
    let encoded = SaveData::default().encode().unwrap();
    for length in 0..4 {
        assert!(SaveData::decode(&encoded[..length]).is_err());
    }
    let mut corrupt = encoded.clone();
    corrupt[12] ^= 0x80;
    assert!(SaveData::decode(&corrupt).is_err());
    let raw = profile(1).encode_raw().unwrap();
    for length in 0..40 {
        assert!(SaveData::decode_raw(raw[..length].to_vec()).is_err());
    }
    let mut bad = profile(1);
    bad.save_menu_position = 128;
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.save_vectors.select_last_choices.push(16);
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.persist_data.0 = vec![0; 65536];
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.settings.v8 = 4;
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.auto_save_slot.as_mut().unwrap().date.year = 4096;
    assert!(bad.encode().is_err());
    // A claimed response count must not allocate 4 GiB before the first byte.
    let mut writer = BitBuffer::empty();
    for width in [12, 4, 5, 5, 6, 6] {
        writer.write_int(width, 0u32);
    }
    writer.write_bool(false);
    writer.align();
    for _ in 0..3 {
        writer.write_int(32, 0u32);
    }
    writer.write_int(32, u32::MAX);
    let mut reader = BitBuffer::with_data(writer.into_data().0, 0);
    assert!(GameData::read(&mut reader).is_err());
    assert_eq!(reader.bit_position(), 168);
}

#[test]
fn ger_disk_progress_normalizes_only_serialized_entries_and_preserves_raw_time() {
    let mut save = profile(17);
    let slot = save.auto_save_slot.as_mut().unwrap();
    slot.progress = Progress::new(0, slot.progress.entries.clone()).unwrap();
    let inactive = slot.progress.entries[1].clone();
    let memory = slot.clone().into_memory();
    assert_eq!(GameData::from_memory(&memory).unwrap(), *slot);
    assert_eq!(SaveDate::from_packed(slot.date.packed()), slot.date);
    assert!(GameData::from_memory(&super::super::AutosaveMemory::default()).is_none());
    let decoded = SaveData::decode(&save.encode().unwrap()).unwrap();
    let restored = decoded.auto_save_slot.unwrap();
    assert_eq!(
        restored.date,
        SaveDate {
            year: 4095,
            month: 0,
            day: 31,
            hour: 31,
            minute: 63,
            second: 63
        }
    );
    assert_eq!(restored.progress.entries[1], ProgressEntry::default());
    assert_ne!(restored.progress.entries[1], inactive);
    assert_eq!(
        restored.progress.active_entry().replay_target,
        restored.progress.active_entry().save_position
    );
}

#[test]
#[ignore = "requires supplied executable, extracted fonts and original save-codec oracle"]
fn ger_native_save_codec_regenerates_complete_golden_corpus() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut child = Command::new(
        std::env::var_os("SHIN_GEROKASU2_SAVE_CODEC_ORACLE").expect("native save-codec oracle"),
    )
    .args([
        std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
        std::env::var_os("SHIN_GEROKASU2_NORMAL_FONT").unwrap(),
        std::env::var_os("SHIN_GEROKASU2_BOLD_FONT").unwrap(),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
    let mut queries = String::new();
    for n in 0..33 {
        queries.push_str(&format!("P {n}\n"));
    }
    for raw in VERSION_PROBES {
        queries.push_str(&format!("D {raw}\n"));
    }
    for n in 0..34 {
        let raw = crypto_input(n);
        queries.push_str(&format!(
            "T {}\n",
            if raw.is_empty() {
                "-".to_owned()
            } else {
                hex::encode(raw)
            }
        ));
    }
    child
        .stdin
        .take()
        .unwrap()
        .write_all(queries.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().collect::<Vec<_>>(), native_lines());
    for line in output.lines().take(33) {
        assert_profile(line);
    }
    eprintln!(
        "GEROKASU2_SAVE_CODEC_NATIVE profiles=33 versions=6 crypto_tails=34 raw_bytes=121856 encrypted_bytes=121988 original_complete_writer_reader=verified SDK_disk_IO=excluded"
    );
}
