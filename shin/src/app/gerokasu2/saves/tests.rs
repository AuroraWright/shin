use super::*;

struct Temporary(std::path::PathBuf);
impl Temporary {
    fn new() -> Self {
        let folder = std::env::temp_dir().join(format!(
            "shin-ger-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        Self(folder)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ger_store_preserves_existing_file_on_invalid_snapshot_and_corrupt_import() {
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone(), None).unwrap();
    store.data.persist_data.0 = vec![-1, 731];
    store.data.save_vectors.seen_messages = vec![0x80000001, 0];
    store.flush().unwrap();
    let original = std::fs::read(&path).unwrap();
    assert_eq!(
        read(&path).unwrap().save_vectors.seen_messages,
        [0x80000001, 0]
    );
    store.data.settings.v0_bgmvol = 128;
    assert!(store.flush().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(&temporary.0).unwrap().count(), 1);
    std::fs::write(&path, b"invalid encrypted save").unwrap();
    assert!(Store::load(path.clone(), None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid encrypted save");
}

#[test]
fn ger_store_coalesces_writes_and_drains_before_last_snapshot() {
    shin_tasks::create_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone(), None).unwrap();
    store.data.play_seconds = 1;
    store.request_flush().unwrap();
    for value in 2..=10 {
        store.data.play_seconds = value;
        store.request_flush().unwrap();
    }
    store.data.play_seconds = 99;
    store.finish().unwrap();
    assert_eq!(read(&path).unwrap().play_seconds, 99);
    assert_eq!(std::fs::read_dir(&temporary.0).unwrap().count(), 1);
}

#[test]
fn ger_native_slot_selection_keeps_both_entries_and_raw_route_tail() {
    use shin_core::format::save::gerokasu2::{Progress, ProgressEntry, SaveDate};
    let game = GameData {
        date: SaveDate::default(),
        progress: Progress::new(
            1,
            [
                ProgressEntry {
                    scenario_id: 7,
                    responses: vec![0xaa, 0xff, 0],
                    ..Default::default()
                },
                ProgressEntry {
                    scenario_id: 9,
                    random_seed: 42,
                    save_position: 383,
                    replay_target: 383,
                    responses: vec![0x03, 0, 0xff],
                    ..Default::default()
                },
            ],
        )
        .unwrap(),
    };
    let mut data = SaveData::default();
    data.auto_save_slot = Some(game.clone());
    data.manual_save_slots[99] = Some(game.clone());
    assert_eq!(slot(&data, 0).unwrap().progress, game.progress);
    assert_eq!(slot(&data, 100).unwrap().progress, game.progress);
    assert!(slot(&data, 1).is_err());
    assert!(slot(&data, 101).is_err());
}
