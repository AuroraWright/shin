//! Portable corpus emitted by complete original and 1.03 native save codecs.
use super::*;

fn packets() -> Vec<(&'static str, &'static str)> {
    let lines: Vec<_> =
        include_str!("../../../../test-fixtures/native-game-data-codec-golden.txt")
            .lines()
            .collect();
    assert_eq!(lines.len(), 256);
    lines
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

fn decode_packet(input: &str, output: &str) -> GameData {
    let tokens: Vec<_> = input.split_whitespace().collect();
    let prefix: usize = tokens[0].parse().unwrap();
    let fields: Vec<_> = output.split_whitespace().collect();
    let ending: usize = fields[0].parse().unwrap();
    let bytes = hex::decode(fields[1]).unwrap();
    let mut reader = BitBuffer::with_data(bytes.clone(), prefix);
    assert!(reader.read_bool().unwrap());
    let data = GameData::read(&mut reader).unwrap();
    assert_eq!(reader.read_int::<u16>(16).unwrap(), 0xabcd);
    assert_eq!(reader.bit_position(), ending);
    assert_eq!(
        data.date_time,
        NaiveDate::from_ymd_opt(2026, 10, 9)
            .unwrap()
            .and_hms_opt(23, 59, 58)
            .unwrap()
    );
    let check_entry = |entry: &GameDataEntry, start: usize| {
        assert_eq!(
            entry.scenario_id,
            u32::from_str_radix(tokens[start], 16).unwrap() as i32
        );
        assert_eq!(
            entry.random_seed,
            u32::from_str_radix(tokens[start + 1], 16).unwrap()
        );
        assert_eq!(
            entry.save_position,
            u32::from_str_radix(tokens[start + 2], 16).unwrap()
        );
        let route = if tokens[start + 3] == "-" {
            Vec::new()
        } else {
            hex::decode(tokens[start + 3]).unwrap()
        };
        assert_eq!(entry.route_data.0, route);
    };
    if tokens[1] == "1" {
        check_entry(data.primary_entry.as_ref().unwrap(), 2);
        check_entry(&data.entry, 6);
    } else {
        assert!(data.primary_entry.is_none());
        check_entry(&data.entry, 2);
    }
    let mut writer = BitBuffer::empty();
    writer.write_int(prefix as u8, 0xa5u8 & ((1u8 << prefix) - 1));
    writer.write_bool(true);
    data.write(&mut writer);
    writer.write_int(16, 0xabcdu16);
    let (written, position) = writer.into_data();
    assert_eq!(position, ending);
    assert_eq!(&written[..bytes.len()], &bytes);
    assert!(written[bytes.len()..].iter().all(|&byte| byte == 0));
    data
}

#[test]
fn game_data_matches_both_complete_native_codecs_at_all_bit_offsets() {
    for (input, output) in packets() {
        decode_packet(input, output);
    }
}

#[test]
fn encrypted_progress_preserves_both_entries_across_all_save_slots() {
    let data: Vec<_> = packets()
        .into_iter()
        .map(|(a, b)| decode_packet(a, b))
        .collect();
    let mut progress = SaveData::default();
    progress.auto_save_slot = Some(data[8].clone());
    progress.manual_save_slots = std::array::from_fn(|i| Some(data[i].clone()));
    let encoded = progress.encode();
    let decoded = SaveData::decode(&encoded).unwrap();
    // A metadata/menu edit rewrites the entire progress file. Every inactive
    // entry and route must survive, including entries in other save slots.
    assert_eq!(decoded.encode(), encoded);
    for (before, after) in std::iter::once(progress.auto_save_slot.as_ref().unwrap())
        .chain(
            progress
                .manual_save_slots
                .iter()
                .map(|slot| slot.as_ref().unwrap()),
        )
        .zip(
            std::iter::once(decoded.auto_save_slot.as_ref().unwrap()).chain(
                decoded
                    .manual_save_slots
                    .iter()
                    .map(|slot| slot.as_ref().unwrap()),
            ),
        )
    {
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }
}

#[test]
fn encrypted_progress_matches_complete_native_savegame_worker_files() {
    let inputs = packets();
    let files: Vec<_> =
        include_str!("../../../../test-fixtures/native-save-write-worker-golden.txt")
            .lines()
            .collect();
    assert_eq!(files.len(), inputs.len());
    for (index, (line, (input, output))) in files.into_iter().zip(inputs).enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0].parse::<usize>().unwrap(), index);
        let bytes = hex::decode(fields[4]).unwrap();
        assert_eq!(bytes.len(), fields[3].parse::<usize>().unwrap());
        let game = decode_packet(input, output);
        let mut expected = SaveData::default();
        expected.play_seconds = index as u32 * 37;
        expected.save_menu_position = (index % 100) as u8;
        expected.auto_save_slot = Some(game.clone());
        expected.manual_save_slots[index % 100] = Some(game);
        // Exact whole-file comparison covers the native global snapshot,
        // absent slots, settings, alignment, CRC and both encryption stages.
        assert_eq!(expected.encode(), bytes, "native worker context {index}");
        assert_eq!(
            format!("{:?}", SaveData::decode(&bytes).unwrap()),
            format!("{expected:?}"),
            "native worker decoded context {index}"
        );
    }
}
