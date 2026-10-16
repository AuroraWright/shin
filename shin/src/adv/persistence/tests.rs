use super::*;

fn rejected_save(path: &std::path::Path, bytes: &[u8]) -> SaveRecovery {
    fs::write(path, bytes).unwrap();
    SaveStore::open(path.to_owned(), None)
        .err()
        .unwrap()
        .downcast::<SaveDecodeError>()
        .unwrap()
        .recovery
}
fn recovery_copies(directory: &std::path::Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bak"))
        .collect()
}
#[test]
fn acknowledged_recovery_preserves_rejected_bytes_before_default_save() {
    let directory = directory();
    let path = directory.join("progress.dat");
    let rejected = [1, 2, 3];
    let recovery = rejected_save(&path, &rejected);
    assert_eq!(recovery.kind, SaveRecoveryKind::Corrupt);
    assert!(recovery_copies(&directory).is_empty());
    let mut store = SaveStore::recover(recovery).unwrap();
    assert_eq!(store.data.encode_raw(), SaveData::default().encode_raw());
    assert_eq!(fs::read(&path).unwrap(), rejected);
    let copies = recovery_copies(&directory);
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read(&copies[0]).unwrap(), rejected);
    let mut state = VmState::for_scenario(19, 99);
    state.capture_autosave();
    store.flush(&state).unwrap();
    assert_eq!(
        SaveData::decode(&fs::read(&path).unwrap())
            .unwrap()
            .auto_save_slot
            .unwrap()
            .entry
            .scenario_id,
        19
    );
    assert!(store.recovery_source.is_none());
    assert_eq!(fs::read(&copies[0]).unwrap(), rejected);
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn recovery_reopens_data_changed_while_notice_was_open() {
    let directory = directory();
    let path = directory.join("progress.dat");
    let recovery = rejected_save(&path, &[1, 2, 3]);
    let mut latest = SaveData::default();
    latest.settings.v0_bgmvol = 29;
    latest.save_menu_position = 99;
    let encoded = latest.encode();
    fs::write(&path, &encoded).unwrap();
    let store = SaveStore::recover(recovery).unwrap();
    assert_eq!(store.data.settings.v0_bgmvol, 29);
    assert_eq!(store.data.save_menu_position, 99);
    assert!(store.recovery_source.is_none() && recovery_copies(&directory).is_empty());
    assert_eq!(fs::read(&path).unwrap(), encoded);
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn recovery_first_writes_refuse_to_replace_a_newer_manual_file() {
    let directory = directory();
    let path = directory.join("progress.dat");
    let recovery = rejected_save(&path, &[1, 2, 3]);
    let mut store = SaveStore::recover(recovery).unwrap();
    let mut latest = SaveData::default();
    latest.settings.v0_bgmvol = 29;
    let encoded = latest.encode();
    fs::write(&path, &encoded).unwrap();
    let mut state = VmState::for_scenario(19, 99);
    state.capture_autosave();
    assert!(
        store
            .flush(&state)
            .unwrap_err()
            .to_string()
            .contains("changed after recovery")
    );
    assert_eq!(fs::read(&path).unwrap(), encoded);
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 99,
                game: Some(state.snapshot_game_data()),
            },
        )
        .unwrap();
    store.start_explicit();
    wait_ready(&mut store);
    assert!(
        store
            .finish_explicit()
            .unwrap_err()
            .to_string()
            .contains("changed after recovery")
    );
    assert!(store.data.manual_save_slots[99].is_none());
    assert!(store.flush_final(&state).is_err());
    assert_eq!(fs::read(&path).unwrap(), encoded);
    assert_eq!(
        fs::read(&recovery_copies(&directory)[0]).unwrap(),
        [1, 2, 3]
    );
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

fn wait_ready(store: &mut SaveStore) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !store.poll_explicit() {
        assert!(std::time::Instant::now() < deadline, "Save worker stalled");
        std::thread::yield_now();
    }
}

#[test]
fn explicit_save_keeps_snapshot_immutable_and_suppresses_overlapping_writes() {
    let directory = directory();
    let path = directory.join("async.dat");
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    let mut state = VmState::for_scenario(7, 42);
    state.primary_entry = Some(VmState::for_scenario(19, 99).snapshot_entry());
    state.capture_autosave();
    let mut game = state.snapshot_game_data();
    game.refresh_timestamp();
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 99,
                game: Some(game.clone()),
            },
        )
        .unwrap();
    let captured = store.data.encode();
    assert!(
        !path.exists(),
        "Snapshot preparation must not write the file"
    );
    assert!(store.finish_explicit().is_err());
    assert!(
        store
            .prepare_explicit(&state, ExplicitSave::Globals)
            .is_err()
    );
    let (send, receive) = std::sync::mpsc::channel();
    let snapshot = store.pending.as_ref().unwrap().snapshot.clone();
    let output = path.clone();
    store.pending.as_mut().unwrap().worker = Some(std::thread::spawn(move || {
        receive.recv().unwrap();
        write_atomic(&output, &snapshot.encode())
    }));
    // Starting twice must retain the same blocked worker and snapshot.
    store.start_explicit();
    state.set_persistent(24, 17);
    state.commit_persistent();
    store.data.play_seconds = 32;
    store.flush(&state).unwrap();
    assert!(!path.exists() && !store.poll_explicit());
    send.send(()).unwrap();
    wait_ready(&mut store);
    store.finish_explicit().unwrap();
    assert_eq!(fs::read(&path).unwrap(), captured);
    assert_eq!(store.data.play_seconds, 32);
    assert!(
        store.finish_explicit().is_err(),
        "Owner completion is once only"
    );
    // A later progress boundary includes globals changed during the save.
    store.flush(&state).unwrap();
    let reopened = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(reopened.persist_data.get(24), 17);
    assert_eq!(reopened.play_seconds, 32);
    let written = reopened.manual_save_slots[99].as_ref().unwrap();
    assert_eq!(
        format!("{:?}", (&written.entry, &written.primary_entry)),
        format!("{:?}", (&game.entry, &game.primary_entry))
    );
    assert_eq!(
        written.date_time.and_utc().timestamp(),
        game.date_time.and_utc().timestamp()
    );
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_async_save_restores_both_entries_and_remembered_selection() {
    let directory = directory();
    let parent = directory.join("parent");
    let mut store = SaveStore::open(parent.join("async.dat"), None).unwrap();
    let mut state = VmState::for_scenario(7, 42);
    state.primary_entry = Some(VmState::for_scenario(19, 99).snapshot_entry());
    let old = state.snapshot_game_data();
    store.data.manual_save_slots[99] = Some(old.clone());
    store.data.save_menu_position = 17;
    fs::write(&parent, b"Blocked save directory").unwrap();
    let replacement = VmState::for_scenario(21, 1).snapshot_game_data();
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 99,
                game: Some(replacement),
            },
        )
        .unwrap();
    assert_eq!(store.data.save_menu_position, 99);
    store.start_explicit();
    wait_ready(&mut store);
    assert!(store.finish_explicit().is_err());
    assert_eq!(store.data.save_menu_position, 17);
    assert_eq!(
        format!("{:?}", store.data.manual_save_slots[99]),
        format!("{:?}", Some(old))
    );
    assert_eq!(fs::read(&parent).unwrap(), b"Blocked save directory");
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shutdown_drains_same_worker_before_writing_newer_clock_and_globals() {
    let directory = directory();
    let path = directory.join("shutdown.dat");
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    let mut state = VmState::for_scenario(7, 42);
    let mut game = state.snapshot_game_data();
    game.refresh_timestamp();
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 3,
                game: Some(game.clone()),
            },
        )
        .unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let snapshot = store.pending.as_ref().unwrap().snapshot.clone();
    let output = path.clone();
    store.pending.as_mut().unwrap().worker = Some(std::thread::spawn(move || {
        receive.recv().unwrap();
        write_atomic(&output, &snapshot.encode())
    }));
    store.data.play_seconds = 45;
    state.set_persistent(24, 17);
    state.commit_persistent();
    send.send(()).unwrap();
    store.flush_final(&state).unwrap();
    assert!(store.pending.is_none());
    let saved = SaveData::decode(&fs::read(path).unwrap()).unwrap();
    assert_eq!(saved.play_seconds, 45);
    assert_eq!(saved.persist_data.get(24), 17);
    let written = saved.manual_save_slots[3].as_ref().unwrap();
    assert_eq!(
        format!("{:?}", (&written.entry, &written.primary_entry)),
        format!("{:?}", (&game.entry, &game.primary_entry))
    );
    assert_eq!(
        written.date_time.and_utc().timestamp(),
        game.date_time.and_utc().timestamp()
    );
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn actual_worker_preserves_native_one_second_minimum_using_wall_time() {
    let directory = directory();
    let mut store = SaveStore::open(directory.join("timed.dat"), None).unwrap();
    store
        .prepare_explicit(&VmState::new(), ExplicitSave::Globals)
        .unwrap();
    let started = std::time::Instant::now();
    store.start_explicit();
    wait_ready(&mut store);
    assert!(started.elapsed() >= std::time::Duration::from_secs(1));
    store.finish_explicit().unwrap();
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

fn directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "shin-save-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn manual_slot_boundaries_round_trip_retained_entries_and_preserve_globals() {
    let directory = directory();
    let path = directory.join("manual.dat");
    let mut state = VmState::for_scenario(7, 42);
    state.route.record_choice(3);
    state.advance_save_position();
    state.primary_entry = Some(VmState::for_scenario(19, 99).snapshot_entry());
    let retained = state.snapshot_game_data();
    let inactive = format!("{:?}", retained.primary_entry);
    state.capture_autosave();
    state.settings.v3_sysvol = 23;
    let mut imported = SaveData::default();
    imported.play_seconds = 123;
    imported.manual_save_slots[0] = state.auto_save_slot.clone();
    let mut store = SaveStore::open(path.clone(), Some(imported)).unwrap();
    store.reset_revision(&state);
    let revision = state.save_revision;
    // Saving must use the opening entry even if the live state changes.
    state.save_position = 9;
    store
        .write_manual_slot(&state, 99, retained.clone())
        .unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.save_menu_position, 99);
    assert_eq!(
        saved.manual_save_slots[99]
            .as_ref()
            .unwrap()
            .date_time
            .and_utc()
            .timestamp(),
        retained.date_time.and_utc().timestamp()
    );
    let entry = &saved.manual_save_slots[99].as_ref().unwrap().entry;
    assert_eq!(
        (entry.scenario_id, entry.random_seed, entry.save_position),
        (7, 42, 1)
    );
    assert_eq!(entry.route_data.reader().read_choice().unwrap(), 3);
    assert!(saved.manual_save_slots[0].is_some());
    assert_eq!(
        format!(
            "{:?}",
            saved.manual_save_slots[99].as_ref().unwrap().primary_entry
        ),
        inactive
    );
    assert_eq!(
        format!("{:?}", saved.auto_save_slot.as_ref().unwrap().primary_entry),
        inactive
    );
    assert_eq!(saved.settings.v3_sysvol, 23);
    assert_eq!(saved.play_seconds, 123);
    store.delete_manual_slot(&state, 0).unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert!(saved.manual_save_slots[0].is_none());
    assert!(saved.manual_save_slots[99].is_some());
    assert_eq!(
        saved.save_menu_position, 99,
        "delete retains the last save/load position"
    );
    assert_eq!(state.save_revision, revision);
    assert_eq!(state.save_position, 9);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_manual_slot_writes_keep_slots_and_allow_explicit_retry() {
    let directory = directory();
    let path = directory.join("manual.dat");
    let state = VmState::new();
    let mut game = GameData::from_entry(state.snapshot_entry());
    game.primary_entry = Some(VmState::for_scenario(7, 99).snapshot_entry());
    let retained = format!("{:?}", game.primary_entry);
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    store.write_manual_slot(&state, 0, game.clone()).unwrap();
    let original = fs::read(&path).unwrap();
    assert!(store.write_manual_slot(&state, 100, game.clone()).is_err());
    assert!(store.delete_manual_slot(&state, usize::MAX).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    // A directory cannot be atomically replaced by a file.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(store.write_manual_slot(&state, 99, game.clone()).is_err());
    assert!(store.data.manual_save_slots[99].is_none());
    assert_eq!(store.data.save_menu_position, 0);
    assert!(store.delete_manual_slot(&state, 0).is_err());
    assert!(store.data.manual_save_slots[0].is_some());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir(&path).unwrap();
    store.write_manual_slot(&state, 99, game).unwrap();
    store.delete_manual_slot(&state, 0).unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert!(saved.manual_save_slots[0].is_none());
    assert!(saved.manual_save_slots[99].is_some());
    assert_eq!(saved.save_menu_position, 99);
    assert_eq!(
        format!(
            "{:?}",
            saved.manual_save_slots[99].as_ref().unwrap().primary_entry
        ),
        retained
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn config_boundary_writes_without_progress_and_retries_failed_replacement() {
    let directory = directory();
    let path = directory.join("config.dat");
    let mut state = VmState::new();
    let mut imported = SaveData::default();
    imported.save_menu_position = 37;
    state.capture_autosave();
    imported.manual_save_slots[99] = state.auto_save_slot.clone();
    let mut store = SaveStore::open(path.clone(), Some(imported)).unwrap();
    store.reset_revision(&state);
    let revision = state.save_revision;
    state.settings.v3_sysvol = 23;
    store.data.play_seconds = 12;
    store.flush(&state).unwrap();
    assert!(
        !path.exists(),
        "ordinary frames still avoid settings-only I/O"
    );
    fs::create_dir(&path).unwrap();
    assert!(store.flush_config(&state).is_err());
    assert_eq!(store.data.settings.v3_sysvol, 55);
    assert_eq!(store.written_play_seconds, 0);
    fs::remove_dir(&path).unwrap();
    store.flush_config(&state).unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.settings.v3_sysvol, 23);
    assert_eq!(saved.play_seconds, 12);
    assert_eq!(saved.save_menu_position, 37);
    assert!(saved.manual_save_slots[99].is_some());
    assert_eq!(state.save_revision, revision);
    assert_eq!(store.written_play_seconds, 12);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn progress_snapshot_keeps_current_settings_in_the_original_save_codec() {
    let directory = directory();
    let path = directory.join("settings.dat");
    let mut state = VmState::new();
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    state.settings.v0_bgmvol = 31;
    state.settings.v9_msgspeed = 87;
    state.settings.v19 = 0x55aa55;
    state.capture_autosave();
    store.flush(&state).unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.settings.v0_bgmvol, 31);
    assert_eq!(saved.settings.v9_msgspeed, 87);
    assert_eq!(saved.settings.v19, 0x55aa55);
    assert_eq!(store.data.settings.v0_bgmvol, 31);
    assert_eq!(store.data.settings.v9_msgspeed, 87);
    assert_eq!(store.data.settings.v19, 0x55aa55);

    // Elapsed-time shutdown snapshots must not restore stale settings.
    state.settings.v3_sysvol = 19;
    store.data.play_seconds = 2;
    store.flush_final(&state).unwrap();
    let saved = SaveData::decode(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.settings.v3_sysvol, 19);
    assert_eq!(saved.settings.v9_msgspeed, 87);
    assert_eq!(saved.play_seconds, 2);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn elapsed_time_joins_progress_writes_and_final_flush_without_per_frame_io() {
    let directory = directory();
    let path = directory.join("clock.dat");
    let mut state = VmState::new();
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    store.data.play_seconds = 1;
    store.flush(&state).unwrap();
    assert!(!path.exists(), "clock alone does not write on each update");
    state.capture_autosave();
    store.flush(&state).unwrap();
    assert_eq!(
        SaveStore::open(path.clone(), None)
            .unwrap()
            .data
            .play_seconds,
        1
    );
    let saved = fs::read(&path).unwrap();
    store.data.play_seconds = 2;
    store.flush(&state).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
    store.reset_revision(&state); // Scene restoration keeps the global clock dirty.
    store.flush_final(&state).unwrap();
    assert_eq!(
        SaveStore::open(path.clone(), None)
            .unwrap()
            .data
            .play_seconds,
        2
    );
    let saved = fs::read(&path).unwrap();
    store.flush_final(&state).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn restoration_keeps_unwritten_global_unlocks_dirty() {
    let directory = directory();
    let path = directory.join("startup.dat");
    let initial = SaveData::default();
    fs::write(&path, initial.encode()).unwrap();
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    let mut previous = VmState::new();
    previous.save_vectors.unlocked_movies.add(0);
    previous.save_revision = 1;
    let mut restored = VmState::new();
    restored.save_vectors = previous.save_vectors.clone();
    store.restore_revision(&previous, &restored);
    store.flush_final(&restored).unwrap();
    assert!(
        SaveStore::open(path.clone(), None)
            .unwrap()
            .data
            .save_vectors
            .unlocked_movies
            .contains(0)
    );
    let saved = fs::read(&path).unwrap();
    store.flush_final(&restored).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn final_clock_write_retries_failed_atomic_replacement() {
    let directory = directory();
    let path = directory.join("clock.dat");
    fs::create_dir(&path).unwrap();
    let state = VmState::new();
    let mut store = SaveStore::open(directory.join("missing.dat"), None).unwrap();
    store.path = path.clone();
    store.data.play_seconds = 45;
    assert!(store.flush_final(&state).is_err());
    assert_eq!(store.written_play_seconds, 0);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir(&path).unwrap();
    store.flush_final(&state).unwrap();
    assert_eq!(SaveStore::open(path, None).unwrap().data.play_seconds, 45);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn committed_progress_and_routes_survive_disk_reload_without_losing_settings() {
    let directory = directory();
    let path = directory.join("progress.dat");
    let mut imported = SaveData::default();
    imported.play_seconds = 123;
    imported.save_menu_position = 17;
    let mut state = VmState::for_scenario(7, 42);
    state.route.record_persistent(-14);
    state.route.record_choice(3);
    state.advance_save_position();
    state.capture_autosave();
    imported.manual_save_slots[99] = state.auto_save_slot.clone();
    let settings = imported.settings.clone();
    let mut store = SaveStore::open(path.clone(), Some(imported)).unwrap();
    state.set_persistent(0, 65535);
    state.set_persistent(1, 77); // Not committed until the boundary.
    state.commit_persistent();
    state.set_persistent(1, 99); // Pending writes must not leak to disk.
    super::super::gallery::apply(
        &mut state,
        [super::super::gallery::GalleryUnlock::Picture(32)],
    );
    store.flush(&state).unwrap();
    let saved = fs::read(&path).unwrap();
    store.flush(&state).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
    let reopened = SaveStore::open(path, None).unwrap();
    assert_eq!(reopened.data.persist_data.get(0), -1);
    assert_eq!(reopened.data.persist_data.get(1), 77);
    assert!(reopened.data.save_vectors.unlocked_cgs.contains(32));
    super::super::gallery::apply(
        &mut state,
        [
            super::super::gallery::GalleryUnlock::Music(65),
            super::super::gallery::GalleryUnlock::Movie(3),
        ],
    );
    store.flush(&state).unwrap();
    let gallery = SaveStore::open(store.path.clone(), None).unwrap();
    assert!(gallery.data.save_vectors.unlocked_bgms.contains(65));
    assert!(gallery.data.save_vectors.unlocked_movies.contains(3));
    assert_eq!(reopened.data.play_seconds, 123);
    assert_eq!(reopened.data.save_menu_position, 17);
    assert_eq!(
        format!("{settings:?}"),
        format!("{:?}", reopened.data.settings)
    );
    assert!(reopened.data.manual_save_slots[99].is_some());
    let entry = &reopened.data.auto_save_slot.as_ref().unwrap().entry;
    assert_eq!(
        (entry.scenario_id, entry.random_seed, entry.save_position),
        (7, 42, 1)
    );
    let mut reader = entry.route_data.reader();
    assert_eq!(reader.read_persistent().unwrap(), -14);
    assert_eq!(reader.read_choice().unwrap(), 3);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn corrupt_destinations_and_failed_replacements_preserve_existing_data() {
    let directory = directory();
    let path = directory.join("invalid.dat");
    fs::write(&path, b"invalid").unwrap();
    assert!(SaveStore::open(path.clone(), Some(SaveData::default())).is_err());
    assert_eq!(fs::read(path).unwrap(), b"invalid");
    let mut store = SaveStore::open(directory.join("destination"), None).unwrap();
    fs::create_dir(&store.path).unwrap();
    fs::write(store.path.join("keep"), b"existing").unwrap();
    let mut state = VmState::new();
    state.capture_autosave();
    assert!(store.flush(&state).is_err());
    assert!(store.data.auto_save_slot.is_none());
    assert_eq!(fs::read(store.path.join("keep")).unwrap(), b"existing");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn checksum_valid_malformed_save_bodies_recover_without_losing_bytes() {
    fn bytes(value: &serde_json::Value) -> Vec<u8> {
        let hex = value.as_str().unwrap();
        assert_eq!(hex.len() % 2, 0);
        (0..hex.len())
            .step_by(2)
            .map(|start| u8::from_str_radix(&hex[start..start + 2], 16).unwrap())
            .collect()
    }
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/malformed-save-body-fixtures.json")
    ))
    .unwrap();
    let valid = bytes(&fixtures["valid_packet_hex"]);
    for fixture in fixtures["cases"].as_array().unwrap() {
        let directory = directory();
        let path = directory.join("progress.dat");
        let packet = bytes(&fixture["packet_hex"]);
        let raw = bytes(&fixture["raw_hex"]);
        assert_eq!(SaveData::deobfuscate(&packet).unwrap(), raw);
        fs::write(&path, &packet).unwrap();

        // Real file opening must classify the malformed body without a
        // worker panic, publishing defaults or modifying rejected bytes.
        let error = SaveStore::open(path.clone(), None)
            .err()
            .expect("malformed body must be rejected")
            .downcast::<SaveDecodeError>()
            .unwrap();
        assert!(error.recovery.kind == SaveRecoveryKind::Corrupt);
        assert_eq!(fs::read(&path).unwrap(), packet);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);

        // This call corresponds to the already-tested final notice
        // acknowledgement; preservation must precede default publication.
        let store = SaveStore::recover(error.recovery).unwrap();
        assert_eq!(store.data.encode(), SaveData::default().encode());
        assert_eq!(fs::read(&path).unwrap(), packet);
        let backup = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|candidate| candidate != &path)
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), packet);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    // A repaired file arriving while the notice is open supersedes its
    // captured rejected bytes; recovery must reopen the valid native save.
    let directory = directory();
    let path = directory.join("progress.dat");
    let invalid = bytes(&fixtures["cases"][3]["packet_hex"]);
    fs::write(&path, invalid).unwrap();
    let error = SaveStore::open(path.clone(), None)
        .err()
        .unwrap()
        .downcast::<SaveDecodeError>()
        .unwrap();
    fs::write(&path, &valid).unwrap();
    let store = SaveStore::recover(error.recovery).unwrap();
    assert_eq!(
        store.data.encode(),
        SaveData::decode(&valid).unwrap().encode()
    );
    assert!(store.data.auto_save_slot.is_some());
    assert_eq!(fs::read(&path).unwrap(), valid);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}
