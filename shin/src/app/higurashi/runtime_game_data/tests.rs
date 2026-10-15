use super::*;
use shin_core::format::save::higurashi::SaveData;

#[test]
fn runtime_progress_keeps_both_records_while_disk_projection_remains_compatible() {
    let record = |scenario_id| GameDataEntry {
        scenario_id,
        random_seed: 42,
        save_position: 7,
        replay_target: 7,
        response_data: vec![0x23, 0xee],
    };
    let mut current = RuntimeGameData::from(GameData {
        date: SaveDate::now(),
        extra_word: 0,
        entry: record(2000),
        primary_entry: Some(record(0)),
        trailing_values: vec![7, 123],
        byte_variables: vec![3, 4, 5],
    });
    current.progress.extra_word = 0;
    let original = current.clone();
    current.progress.set_active_index(0).unwrap();
    assert_eq!(current.progress.entries, original.progress.entries);
    let first = current.save_data();
    assert!(first.primary_entry.is_none());
    let mut altered = current.clone();
    altered.progress.entries[1].scenario_id = 999;
    altered.progress.entries[1].response_data = vec![255; 1024];
    let mut a = SaveData::default();
    a.auto_save_slot = Some(first);
    let mut b = SaveData::default();
    b.auto_save_slot = Some(altered.save_data());
    assert_eq!(a.encode_raw().unwrap(), b.encode_raw().unwrap());
    let before = altered.clone();
    assert!(altered.progress.set_active_index(2).is_err());
    assert_eq!(altered, before);
    current.progress.set_active_index(1).unwrap();
    assert_eq!(current, original);
    let reloaded = RuntimeGameData::from(a.auto_save_slot.unwrap());
    assert!(reloaded.progress.entries[1].response_data.is_empty());
    assert_eq!(reloaded.progress.entries[0], original.progress.entries[0]);
}
