use super::*;

#[derive(Deserialize)]
struct Corpus {
    profiles: Vec<Profile>,
    readers: Vec<Reader>,
}
#[derive(Deserialize)]
struct Profile {
    profile: u32,
    bit_position: usize,
    game_key: u32,
    raw: String,
    encrypted: String,
}
#[derive(Deserialize)]
struct Reader {
    name: String,
    input: String,
    accepted: bool,
    bit_position: usize,
    rewritten: Option<String>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!(
        "../../../../../test-fixtures/higurashi/native-save-codec-golden.json"
    ))
    .unwrap()
}

fn profile(n: u32) -> SaveData {
    let mut data = SaveData::default();
    if n == 0 {
        return data;
    }
    data.save_menu_position = ((n * 13) & 127) as u8;
    data.play_seconds = 0xf0000000 + n * 103;
    data.extra_word = 0xc0010000 + n;
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
        select_last_choices: bytes(2, (n + 2) % 5).iter().map(|v| v & 15).collect(),
        unlocked_cgs: words(3),
        unlocked_bgms: words(4),
        unlocked_movies: words(5),
        extra_words_0: words(6),
        extra_words_1: words(7),
    };
    let v = |i: u32, bits: u32| ((n * 17 + i * 23) & ((1 << bits) - 1)) as u8;
    data.settings = Settings {
        common: super::super::Settings {
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
        },
        extra_flag: n & 1 != 0,
        extra_percentage: ((n * 29) & 127) as u8,
    };
    let slot = |i: u32| {
        if n % 3 == 0 && i % 3 == 1 {
            return None;
        }
        let active = (n + i) & 1;
        let entry = |e| GameDataEntry {
            scenario_id: (0x80000000u32 + n + i * 17 + e) as i32,
            random_seed: 0x12345678 + n * 19 + i + e,
            save_position: 0x87654321 + n + i * 31 + e,
            replay_target: 0xdeadbeef,
            response_data: bytes(i + e, (n + i + e) % 9),
        };
        Some(GameData {
            date: SaveDate {
                year: (2026 + n % 3) as u16,
                month: (1 + i % 12) as u8,
                day: (1 + i % 28) as u8,
                hour: (i % 24) as u8,
                minute: ((i * 7) % 60) as u8,
                second: ((i * 11) % 60) as u8,
            },
            extra_word: ((n + i) & 7) as u8,
            entry: entry(active),
            primary_entry: (active != 0).then(|| entry(0)),
            trailing_values: (0..(n + i) % 7)
                .map(|j| ((n * 37 + i * 11 + j * 71) & 1023) as u16)
                .collect(),
            byte_variables: bytes(i, (n * 3 + i) % 11),
        })
    };
    data.auto_save_slot = slot(0);
    data.manual_save_slots = std::array::from_fn(|i| slot(i as u32 + 1));
    data
}

#[test]
fn full_codec_defaults_all_slots_vectors_settings_and_crypto_match_updated_native() {
    let corpus = corpus();
    assert_eq!(corpus.profiles.len(), 33);
    for p in corpus.profiles {
        assert_eq!(GAME_KEY, p.game_key);
        let original = profile(p.profile);
        let raw = hex::decode(&p.raw).unwrap();
        assert_eq!(
            original.encode_raw().unwrap(),
            raw,
            "native writer profile{}",
            p.profile
        );
        let mut reader = BitBuffer::with_data(raw, 0);
        let decoded = SaveData::read(&mut reader).unwrap();
        assert_eq!(reader.bit_position(), p.bit_position);
        assert_eq!(
            decoded.encode_raw().unwrap(),
            original.encode_raw().unwrap()
        );
        for slot in std::iter::once(&decoded.auto_save_slot)
            .chain(&decoded.manual_save_slots)
            .flatten()
        {
            if slot.primary_entry.is_none() {
                assert!(slot.trailing_values.is_empty());
            }
            for entry in std::iter::once(&slot.entry).chain(&slot.primary_entry) {
                assert_eq!(entry.replay_target, entry.save_position);
            }
        }
        let encrypted = hex::decode(p.encrypted).unwrap();
        assert_eq!(
            original.encode().unwrap(),
            encrypted,
            "native crypto profile{}",
            p.profile
        );
        assert_eq!(
            SaveData::decode(&encrypted).unwrap().encode().unwrap(),
            encrypted
        );
        assert!(super::super::SaveData::decode(&encrypted).is_err());
    }
}

#[test]
fn versions_zero_one_and_unsupported_match_updated_native_reader() {
    let corpus = corpus();
    assert_eq!(corpus.readers.len(), 3);
    for p in corpus.readers {
        let mut reader = BitBuffer::with_data(hex::decode(&p.input).unwrap(), 0);
        let result = SaveData::read(&mut reader);
        assert_eq!(result.is_ok(), p.accepted, "{}", p.name);
        assert_eq!(reader.bit_position(), p.bit_position);
        match result {
            Ok(decoded) => {
                assert_eq!(
                    decoded.encode_raw().unwrap(),
                    hex::decode(p.rewritten.unwrap()).unwrap()
                );
                if p.name == "version-one" {
                    assert_eq!(decoded.settings.extra_percentage, 80);
                    assert!(!decoded.settings.extra_flag);
                    assert!(
                        decoded
                            .auto_save_slot
                            .as_ref()
                            .unwrap()
                            .trailing_values
                            .is_empty()
                    );
                    assert_eq!(
                        decoded.manual_save_slots[0]
                            .as_ref()
                            .unwrap()
                            .trailing_values,
                        vec![0, 255, 128]
                    );
                    for slot in std::iter::once(&decoded.auto_save_slot)
                        .chain(&decoded.manual_save_slots)
                        .flatten()
                    {
                        assert!(slot.byte_variables.is_empty());
                    }
                }
            }
            Err(error) => assert_eq!(error.downcast_ref::<UnsupportedSaveVersion>().unwrap().0, 3),
        }
    }
}

#[test]
fn supplied_old_save_migrates_all_fields_to_updated_native_v2_output() {
    // The supplied file is retained byte-for-byte. The expected migration was
    // produced by the updated executable's actual crypto, CRC, reader and writer
    // at both O0 and O3; see old-save-20261015/native/validation.json.
    let old = include_bytes!(
        "../../../../../test-fixtures/higurashi/old-save-20261015/supplied-v1.dat"
    );
    let expected = include_bytes!(
        "../../../../../test-fixtures/higurashi/old-save-20261015/native/updated-O0-v2.bin"
    );
    let raw = super::super::obfuscation::decode(old, GAME_KEY).unwrap();
    assert_eq!(old.len(), 29_700);
    assert_eq!(raw[0], 1);
    let mut reader = BitBuffer::with_data(raw, 0);
    let decoded = SaveData::read(&mut reader).unwrap();
    assert_eq!(reader.bit_position(), 233_475);
    assert_eq!(decoded.encode_raw().unwrap(), expected.as_slice());
    assert_eq!(
        SaveData::decode(old).unwrap().encode_raw().unwrap(),
        expected.as_slice()
    );

    assert_eq!(decoded.play_seconds, 370_810);
    assert_eq!(decoded.persist_data.0.len(), 192);
    assert_eq!(decoded.save_vectors.seen_messages.len(), 5_120);
    assert_eq!(decoded.save_vectors.select_seen_choices.len(), 256);
    assert_eq!(decoded.save_vectors.select_last_choices.len(), 64);
    assert_eq!(decoded.settings.common.v0_bgmvol, 15);
    assert_eq!(decoded.settings.common.v1_sfxvol, 100);
    assert_eq!(decoded.settings.common.v18_showsongtitle, true);
    assert!(!decoded.settings.extra_flag);
    assert_eq!(decoded.settings.extra_percentage, 80);
    let auto = decoded.auto_save_slot.as_ref().unwrap();
    assert_eq!((auto.entry.scenario_id, auto.entry.save_position), (0, 1));
    let manual = decoded.manual_save_slots[0].as_ref().unwrap();
    assert_eq!(
        (manual.entry.scenario_id, manual.entry.save_position),
        (201, 730)
    );
    assert!(decoded.manual_save_slots[1..].iter().all(Option::is_none));
    for slot in [auto, manual] {
        assert!(slot.primary_entry.is_none());
        assert!(slot.trailing_values.is_empty());
        assert!(slot.byte_variables.is_empty());
        assert_eq!(slot.entry.replay_target, slot.entry.save_position);
    }
    // A subsequent ordinary save upgrades the format; decode it through the
    // same public entry point used by LoadGame and preserve the complete payload.
    let upgraded = decoded.encode().unwrap();
    assert_eq!(
        super::super::obfuscation::decode(&upgraded, GAME_KEY).unwrap(),
        expected.as_slice()
    );
    assert_eq!(
        SaveData::decode(&upgraded).unwrap().encode_raw().unwrap(),
        expected.as_slice()
    );
}

#[test]
fn malformed_crc_truncated_payloads_and_unrepresentable_values_return_errors() {
    let encoded = SaveData::default().encode().unwrap();
    for length in 0..4 {
        assert!(SaveData::decode(&encoded[..length]).is_err());
    }
    let mut corrupt = encoded.clone();
    corrupt[12] ^= 0x80;
    assert!(SaveData::decode(&corrupt).is_err());
    for length in 0..40 {
        assert!(SaveData::decode_raw(profile(1).encode_raw().unwrap()[..length].to_vec()).is_err());
    }
    let mut bad = profile(1);
    bad.auto_save_slot.as_mut().unwrap().trailing_values = vec![1024];
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.auto_save_slot.as_mut().unwrap().byte_variables = vec![0; 256];
    assert!(bad.encode().is_err());
    bad = profile(1);
    bad.settings.common.v8 = 4;
    assert!(bad.encode().is_err());
}
