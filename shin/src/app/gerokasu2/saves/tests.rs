use super::*;
use clap::Parser;

#[test]
fn ger_progress_path_defaults_to_app_data_and_honors_explicit_paths() {
    let cli = crate::cli::Cli::try_parse_from(["shin", "--game", "gerokasu2"]).unwrap();
    assert!(progress_path(&cli).unwrap().ends_with("shin/gerokasu2/SAVEDATA"));
    let cli = crate::cli::Cli::try_parse_from([
        "shin",
        "--game",
        "gerokasu2",
        "--save-file",
        "chosen/progress.dat",
    ])
    .unwrap();
    assert_eq!(
        progress_path(&cli).unwrap(),
        PathBuf::from("chosen/progress.dat")
    );
    assert!(
        crate::cli::Cli::try_parse_from(["shin", "--game", "gerokasu2", "--save-file", ""])
            .is_err()
    );
    let mut cli = crate::cli::Cli::try_parse_from(["shin", "--game", "gerokasu2"]).unwrap();
    cli.save_file = Some(PathBuf::new());
    assert!(progress_path(&cli).is_err());
}

struct Temporary(std::path::PathBuf);
impl Temporary {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "shin-ger-save-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
    let mut store = Store::load(path.clone()).unwrap();
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
    assert!(Store::load(path.clone()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid encrypted save");
}

#[test]
fn ger_store_coalesces_writes_and_drains_before_last_snapshot() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
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
fn ger_store_async_completion_waits_for_queued_snapshot_and_allows_failed_write_retry() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.settings.v0_bgmvol = 20;
    store.request_flush().unwrap();
    assert!(store.is_flush_pending());
    store.data.settings.v0_bgmvol = 35;
    store.request_flush().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while store.is_flush_pending() {
        store.poll_flush().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(read(&path).unwrap().settings.v0_bgmvol, 35);
    let original = std::fs::read(&path).unwrap();
    store.data.settings.v0_bgmvol = 128;
    store.request_flush().unwrap();
    loop {
        if store.poll_flush().is_err() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!store.is_flush_pending());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    store.data.settings.v0_bgmvol = 55;
    store.request_flush().unwrap();
    while store.is_flush_pending() {
        store.poll_flush().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(read(&path).unwrap().settings.v0_bgmvol, 55);
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

fn manual_game(seed: u32) -> GameData {
    use shin_core::format::save::gerokasu2::{Progress, SaveDate};
    GameData {
        date: SaveDate::default(),
        progress: Progress::fresh(7, seed),
    }
}
fn finish_manual(store: &mut Store) -> Result<()> {
    store.start_manual_write(Duration::ZERO)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(result) = store.poll_manual_write() {
            return result;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn ger_manual_store_stages_in_memory_until_open_question_dispatches_worker() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.save_menu_position = 18;
    store.flush().unwrap();
    let old = std::fs::read(&path).unwrap();
    store
        .begin_manual_write(3, Some(manual_game(42)), true)
        .unwrap();
    store.data.play_seconds = 91;
    // Both ordinary store polling and an unrelated queued globals refresh must
    // preserve the stage/IO boundary while the wait panel is still opening.
    store.request_flush().unwrap();
    for _ in 0..40 {
        assert!(store.poll_manual_write().is_none());
        store.poll_flush().unwrap();
        assert!(store.pending.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), old);
    }
    assert!(store.is_flush_pending());
    assert_eq!(store.data.manual_save_slots[3], Some(manual_game(42)));
    assert_eq!(store.data.save_menu_position, 3);
    finish_manual(&mut store).unwrap();
    assert_eq!(
        read(&path).unwrap().manual_save_slots[3],
        Some(manual_game(42))
    );
    assert_eq!(
        read(&path).unwrap().play_seconds,
        0,
        "manual write uses the constructor snapshot"
    );
    assert_eq!(store.data.play_seconds, 91);
    store.finish().unwrap();
}

#[test]
fn ger_frozen_globals_write_keeps_constructor_settings_and_live_progress_separate() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.settings.v0_bgmvol = 34;
    let snapshot = store.data.clone();
    store.data.settings.v0_bgmvol = 57;
    store.data.save_vectors.seen_messages = vec![0x80000001];
    store
        .request_frozen_flush(snapshot, Duration::ZERO)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.is_flush_pending() {
        store.poll_flush().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(read(&path).unwrap().settings.v0_bgmvol, 34);
    assert!(read(&path).unwrap().save_vectors.seen_messages.is_empty());
    assert_eq!(store.data.settings.v0_bgmvol, 57);
    assert_eq!(store.data.save_vectors.seen_messages, [0x80000001]);
    store.finish().unwrap();
    assert_eq!(read(&path).unwrap().settings.v0_bgmvol, 57);
}

#[test]
fn ger_manual_store_save_overwrite_delete_and_concurrent_transaction_guard() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.save_menu_position = 17;
    store.flush().unwrap();
    assert!(
        store
            .begin_manual_write(100, Some(manual_game(1)), true)
            .is_err()
    );
    assert!(store.manual.is_none());
    store
        .begin_manual_write(99, Some(manual_game(1)), true)
        .unwrap();
    assert_eq!(store.data.manual_save_slots[99], Some(manual_game(1)));
    assert_eq!(store.data.save_menu_position, 99);
    assert!(store.begin_manual_write(0, None, false).is_err());
    finish_manual(&mut store).unwrap();
    assert_eq!(
        read(&path).unwrap().manual_save_slots[99],
        Some(manual_game(1))
    );
    store
        .begin_manual_write(99, Some(manual_game(2)), true)
        .unwrap();
    finish_manual(&mut store).unwrap();
    assert_eq!(
        read(&path).unwrap().manual_save_slots[99],
        Some(manual_game(2))
    );
    store.begin_manual_write(99, None, false).unwrap();
    finish_manual(&mut store).unwrap();
    let disk = read(&path).unwrap();
    assert!(disk.manual_save_slots[99].is_none());
    assert_eq!(disk.save_menu_position, 99);
    assert!(store.poll_manual_write().is_none());
    assert_eq!(std::fs::read_dir(&temporary.0).unwrap().count(), 1);
}

#[test]
fn ger_manual_store_failed_snapshot_rolls_back_only_slot_and_remembered_selection() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.manual_save_slots[4] = Some(manual_game(1));
    store.data.save_menu_position = 19;
    store.flush().unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut invalid = manual_game(2);
    invalid.date.year = 4096;
    store.begin_manual_write(4, Some(invalid), true).unwrap();
    store.data.settings.v0_bgmvol = 63;
    store.data.persist_data.0 = vec![8, 42];
    store.data.save_vectors.unlocked_bgms = vec![0x80000001, 3];
    // A refresh queued behind the failing staged snapshot must not publish it.
    store.request_flush().unwrap();
    assert!(finish_manual(&mut store).is_err());
    assert_eq!(store.data.manual_save_slots[4], Some(manual_game(1)));
    assert_eq!(store.data.save_menu_position, 19);
    assert_eq!(store.data.settings.v0_bgmvol, 63);
    assert_eq!(store.data.persist_data.0, [8, 42]);
    assert_eq!(store.data.save_vectors.unlocked_bgms, [0x80000001, 3]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // The deferred refresh now contains the rolled-back slot and current globals.
    while store.is_flush_pending() {
        store.poll_flush().unwrap();
        std::thread::yield_now();
    }
    let disk = read(&path).unwrap();
    assert_eq!(disk.manual_save_slots[4], Some(manual_game(1)));
    assert_eq!(disk.save_menu_position, 19);
    assert_eq!(disk.settings.v0_bgmvol, 63);
    assert_eq!(disk.save_vectors.unlocked_bgms, [0x80000001, 3]);
    assert_eq!(std::fs::read_dir(&temporary.0).unwrap().count(), 1);
}

#[test]
fn ger_manual_store_predecessor_failure_discards_staged_queue_and_delete_rolls_back() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.manual_save_slots[3] = Some(manual_game(1));
    store.data.save_menu_position = 27;
    store.flush().unwrap();
    let original = std::fs::read(&path).unwrap();
    let (release, ready) = futures::channel::oneshot::channel::<()>();
    store.pending = Some(shin_tasks::async_io::spawn(async move {
        ready.await.unwrap();
        anyhow::bail!("injected predecessor IO failure");
    }));
    store.queued = true;
    store.begin_manual_write(3, None, false).unwrap();
    assert!(store.data.manual_save_slots[3].is_none());
    store.data.settings.v1_sfxvol = 71;
    release.send(()).unwrap();
    assert!(finish_manual(&mut store).is_err());
    assert!(!store.is_flush_pending());
    assert_eq!(store.data.manual_save_slots[3], Some(manual_game(1)));
    assert_eq!(store.data.save_menu_position, 27);
    assert_eq!(store.data.settings.v1_sfxvol, 71);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    for _ in 0..3 {
        store.poll_flush().unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn ger_manual_store_predecessor_and_deferred_globals_cross_one_commit_boundary() {
    crate::init_test_task_pools();
    let temporary = Temporary::new();
    let path = temporary.0.join("progress.dat");
    let mut store = Store::load(path.clone()).unwrap();
    store.data.play_seconds = 1;
    store.data.save_menu_position = 8;
    store.flush().unwrap();
    let before = Store::load(path.clone()).unwrap();
    let (release, ready) = futures::channel::oneshot::channel::<()>();
    store.pending = Some(shin_tasks::async_io::spawn(async move {
        ready.await.unwrap();
        before.flush()
    }));
    store.queued = true;
    store
        .begin_manual_write(2, Some(manual_game(42)), true)
        .unwrap();
    store.data.play_seconds = 2;
    store.request_flush().unwrap();
    assert!(!store.manual.as_ref().unwrap().writing);
    store.start_manual_write(Duration::ZERO).unwrap();
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !store.manual.as_ref().unwrap().writing {
        store.poll_flush().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    // A later invalid globals refresh must not turn a durable manual save into
    // a failed transaction or resurrect the previous slot.
    store.data.settings.v0_bgmvol = 128;
    store.request_flush().unwrap();
    finish_manual(&mut store).unwrap();
    assert_eq!(
        read(&path).unwrap().manual_save_slots[2],
        Some(manual_game(42))
    );
    loop {
        if store.poll_flush().is_err() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(store.data.manual_save_slots[2], Some(manual_game(42)));
    assert_eq!(store.data.save_menu_position, 2);
    assert_eq!(read(&path).unwrap().save_menu_position, 2);
    store.data.settings.v0_bgmvol = 64;
    store.request_flush().unwrap();
    store.finish().unwrap();
    assert_eq!(read(&path).unwrap().settings.v0_bgmvol, 64);
    assert_eq!(std::fs::read_dir(&temporary.0).unwrap().count(), 1);
}
