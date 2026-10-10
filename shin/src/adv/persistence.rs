use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, ensure};
use shin_core::format::save::{GameData, SaveData};

use super::VmState;

pub(super) enum ExplicitSave {
    Globals,
    Slot { slot: usize, game: Option<GameData> },
}

struct PendingWrite {
    snapshot: SaveData,
    revision: u64,
    rollback: Option<(usize, Option<GameData>, u8)>,
    worker: Option<std::thread::JoinHandle<Result<()>>>,
    result: Option<Result<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveBoundary {
    Progress,
    Shutdown,
    Config,
}

/// Desktop storage policy; the contents use the original game's save codec.
pub(super) struct SaveStore {
    path: PathBuf,
    pub data: SaveData,
    revision: u64,
    pending_progress: bool,
    failed_revision: Option<u64>,
    written_play_seconds: u32,
    pending: Option<PendingWrite>,
    recovery_source: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveRecoveryKind {
    Corrupt,
    Incompatible,
}
impl SaveRecoveryKind {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Corrupt => {
                "セーブデータが壊れているため、ロードできません。@rはじめからゲームを開始します。"
            }
            Self::Incompatible => {
                "このセーブデータを使用するためには、ソフトをアップデートする必要があります。@rこのままゲームをはじめることもできますが、その場合は現在のセーブデータは削除されます。"
            }
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct SaveRecovery {
    pub kind: SaveRecoveryKind,
    path: PathBuf,
    bytes: Vec<u8>,
}
#[derive(Debug)]
pub(super) struct SaveDecodeError {
    pub recovery: SaveRecovery,
    source: anyhow::Error,
}
impl std::fmt::Display for SaveDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Decoding progress save {}: {}",
            self.recovery.path.display(),
            self.source
        )
    }
}
impl std::error::Error for SaveDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
#[derive(Debug)]
pub(super) struct SaveRecoveryBackupError(anyhow::Error);
impl std::fmt::Display for SaveRecoveryBackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Preserving original progress save before recovery: {:#}",
            self.0
        )
    }
}
impl std::error::Error for SaveRecoveryBackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Keep storage access failures distinct from failures in the native save codec.
#[derive(Debug)]
pub(super) struct SaveReadError {
    path: PathBuf,
    source: std::io::Error,
}
impl std::fmt::Display for SaveReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Reading progress save {}: {}",
            self.path.display(),
            self.source
        )
    }
}
impl std::error::Error for SaveReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl SaveStore {
    pub fn restore_revision(&mut self, previous: &VmState, restored: &VmState) {
        // Reconstruction rebases the VM counter, not the global progress
        // already changed on TitleDemo or another scene before the handoff.
        self.pending_progress |= previous.save_revision != self.revision;
        self.reset_revision(restored);
    }
    pub fn reset_revision(&mut self, state: &VmState) {
        self.revision = state.save_revision;
        self.failed_revision = None;
    }
    pub fn open(path: PathBuf, imported: Option<SaveData>) -> Result<Self> {
        // Validate an existing destination even when importing another save.
        let existing = match fs::read(&path) {
            Ok(bytes) => match SaveData::decode(&bytes) {
                Ok(data) => Some(data),
                Err(source) => {
                    let kind = if source.is::<shin_core::format::save::UnsupportedSaveVersion>() {
                        SaveRecoveryKind::Incompatible
                    } else {
                        SaveRecoveryKind::Corrupt
                    };
                    return Err(SaveDecodeError {
                        recovery: SaveRecovery { kind, path, bytes },
                        source,
                    }
                    .into());
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => return Err(SaveReadError { path, source }.into()),
        };
        let data = imported.or(existing).unwrap_or_default();
        Ok(Self::from_data(path, data, None))
    }
    fn from_data(path: PathBuf, data: SaveData, recovery_source: Option<Vec<u8>>) -> Self {
        let written_play_seconds = data.play_seconds;
        Self {
            path,
            data,
            written_play_seconds,
            revision: 0,
            pending_progress: false,
            failed_revision: None,
            pending: None,
            recovery_source,
        }
    }
    /// Only called after the native notice's final callback. Preserve the
    /// rejected bytes, then publish native defaults without writing them yet.
    pub fn recover(recovery: SaveRecovery) -> Result<Self> {
        let current = match fs::read(&recovery.path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Self::open(recovery.path, None);
            }
            Err(source) => {
                return Err(SaveReadError {
                    path: recovery.path,
                    source,
                }
                .into());
            }
        };
        if current != recovery.bytes {
            // A manual session or external repair changed the file while the
            // notice was open. Load that file; never reset the stale snapshot.
            return Self::open(recovery.path, None);
        }
        let backup =
            preserve_recovery_bytes(&recovery.path, &current).map_err(SaveRecoveryBackupError)?;
        tracing::info!(path = %backup.display(), "Preserved original progress save before recovery");
        Ok(Self::from_data(
            recovery.path,
            SaveData::default(),
            Some(current),
        ))
    }

    pub fn flush(&mut self, state: &VmState) -> Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        self.flush_snapshot(state, SaveBoundary::Progress)
    }
    /// Desktop shutdown writes elapsed time even without a script progress boundary.
    /// Retry a previous failed progress write once; retain ordinary flush frequency.
    pub fn flush_final(&mut self, state: &VmState) -> Result<()> {
        // Join the same worker before a shutdown write can replace its file.
        // A save accepted just before the dialog opens must also be durable.
        let pending = self.drain_explicit();
        pending.and(self.flush_snapshot(state, SaveBoundary::Shutdown))
    }
    /// Native da9a0/db640 edit global memory before constructing d3390's
    /// immutable snapshot. The UI starts its worker only after dialog opening.
    pub fn prepare_explicit(&mut self, state: &VmState, edit: ExplicitSave) -> Result<()> {
        ensure!(self.pending.is_none(), "A save is already pending");
        let rollback = match edit {
            ExplicitSave::Globals => None,
            ExplicitSave::Slot { slot, game } => {
                ensure!(
                    slot < self.data.manual_save_slots.len(),
                    "Invalid manual save slot {slot}"
                );
                let previous = (
                    slot,
                    self.data.manual_save_slots[slot].clone(),
                    self.data.save_menu_position,
                );
                if game.is_some() {
                    self.data.save_menu_position = slot as u8;
                }
                self.data.manual_save_slots[slot] = game;
                Some(previous)
            }
        };
        self.data.settings = state.settings.clone();
        self.data.persist_data = state.persist.clone();
        self.data.save_vectors = state.save_vectors.clone();
        self.data.auto_save_slot = state.auto_save_slot.clone();
        self.pending = Some(PendingWrite {
            snapshot: self.data.clone(),
            revision: state.save_revision,
            rollback,
            worker: None,
            result: None,
        });
        Ok(())
    }

    pub fn start_explicit(&mut self) {
        let job = self
            .pending
            .as_mut()
            .expect("Save snapshot was not prepared");
        if job.worker.is_some() || job.result.is_some() {
            return;
        }
        let snapshot = job.snapshot.clone();
        let path = self.path.clone();
        let recovery_source = self.recovery_source.clone();
        match std::thread::Builder::new()
            .name("shin-save".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                check_recovery_source(&path, recovery_source.as_deref())?;
                write_atomic(&path, &snapshot.encode())?;
                // Native d41e8 waits only the remainder of one second after a
                // successful write. This uses worker wall time, independently
                // of game ticks, focus, pause, frame rate or skipping.
                if let Some(remaining) =
                    std::time::Duration::from_secs(1).checked_sub(started.elapsed())
                {
                    std::thread::sleep(remaining);
                }
                Ok(())
            }) {
            Ok(worker) => job.worker = Some(worker),
            Err(error) => job.result = Some(Err(error).context("Starting save worker")),
        }
    }

    pub fn poll_explicit(&mut self) -> bool {
        let job = self
            .pending
            .as_mut()
            .expect("Save snapshot was not prepared");
        if job
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            job.result = Some(join_worker(job.worker.take().unwrap()));
        }
        job.result.is_some()
    }

    pub fn finish_explicit(&mut self) -> Result<()> {
        ensure!(
            self.pending
                .as_ref()
                .is_some_and(|job| job.result.is_some()),
            "Save worker is not finished"
        );
        let mut job = self.pending.take().unwrap();
        let result = job.result.take().unwrap();
        if result.is_ok() {
            self.recovery_source = None;
            self.revision = job.revision;
            self.pending_progress = false;
            self.written_play_seconds = job.snapshot.play_seconds;
            self.failed_revision = None;
        } else {
            // Native dafb0 restores both entries and the remembered row.
            // Keep newer global changes and elapsed time published while
            // the worker/dialog was pending.
            if let Some((slot, previous, remembered)) = job.rollback {
                self.data.manual_save_slots[slot] = previous;
                self.data.save_menu_position = remembered;
            }
            self.failed_revision = Some(job.revision);
        }
        result
    }

    fn drain_explicit(&mut self) -> Result<()> {
        if self.pending.is_none() {
            return Ok(());
        }
        self.start_explicit();
        let job = self.pending.as_mut().unwrap();
        if let Some(worker) = job.worker.take() {
            job.result = Some(join_worker(worker));
        }
        self.finish_explicit()
    }
    /// Explicit native Config-close SaveGame boundary, independent of progress.
    /// The Config owner already compared settings with its opening snapshot.
    pub fn flush_config(&mut self, state: &VmState) -> Result<()> {
        self.flush_snapshot(state, SaveBoundary::Config)
    }
    /// Explicit native147e0 SaveGame before entering the title scene.
    pub fn flush_title_return(&mut self, state: &VmState) -> Result<()> {
        self.write_snapshot(state, self.data.clone())
    }
    /// Confirmed SaveMenu write: use the entry retained when the menu opened.
    /// Native da9a0 updates the selected slot and remembers its index before SaveGame.
    pub fn write_manual_slot(
        &mut self,
        state: &VmState,
        slot: usize,
        game: GameData,
    ) -> Result<()> {
        self.replace_manual_slot(state, slot, Some(game))
    }

    /// Confirmed SaveMenu deletion uses the same explicit SaveGame boundary.
    pub fn delete_manual_slot(&mut self, state: &VmState, slot: usize) -> Result<()> {
        self.replace_manual_slot(state, slot, None)
    }

    fn replace_manual_slot(
        &mut self,
        state: &VmState,
        slot: usize,
        game: Option<GameData>,
    ) -> Result<()> {
        ensure!(
            slot < self.data.manual_save_slots.len(),
            "Invalid manual save slot {slot}"
        );
        let mut snapshot = self.data.clone();
        // Native db640 deletes only the slot-present flag; the remembered
        // position changes on save/load, not on delete.
        if game.is_some() {
            snapshot.save_menu_position = slot as u8;
        }
        snapshot.manual_save_slots[slot] = game;
        self.write_snapshot(state, snapshot)
    }

    fn flush_snapshot(&mut self, state: &VmState, boundary: SaveBoundary) -> Result<()> {
        let progress_changed = self.pending_progress || state.save_revision != self.revision;
        let clock_changed = self.data.play_seconds != self.written_play_seconds;
        if boundary != SaveBoundary::Config
            && ((!progress_changed && !(boundary == SaveBoundary::Shutdown && clock_changed))
                || (boundary == SaveBoundary::Progress
                    && self.failed_revision == Some(state.save_revision)))
        {
            return Ok(());
        }
        self.write_snapshot(state, self.data.clone())
    }

    fn write_snapshot(&mut self, state: &VmState, mut snapshot: SaveData) -> Result<()> {
        ensure!(self.pending.is_none(), "A save is already pending");
        snapshot.settings = state.settings.clone();
        snapshot.persist_data = state.persist.clone();
        snapshot.save_vectors = state.save_vectors.clone();
        snapshot.auto_save_slot = state.auto_save_slot.clone();
        if let Err(error) = check_recovery_source(&self.path, self.recovery_source.as_deref())
            .and_then(|()| write_atomic(&self.path, &snapshot.encode()))
        {
            // Report once; the next progress change retries with the latest snapshot.
            self.failed_revision = Some(state.save_revision);
            return Err(error);
        }
        self.data = snapshot;
        self.recovery_source = None;
        self.revision = state.save_revision;
        self.pending_progress = false;
        self.written_play_seconds = self.data.play_seconds;
        self.failed_revision = None;
        Ok(())
    }
}

fn check_recovery_source(path: &std::path::Path, expected: Option<&[u8]>) -> Result<()> {
    if let Some(expected) = expected {
        let current = fs::read(path).context("Checking original save before recovery write")?;
        ensure!(
            current == expected,
            "Progress save changed after recovery; reopen the game to load the new file"
        );
    }
    Ok(())
}

fn preserve_recovery_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<PathBuf> {
    static NEXT_BACKUP: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (backup, mut file) = loop {
        let backup = path.with_file_name(format!(
            "{name}.recovery-{time}-{}-{}.bak",
            std::process::id(),
            NEXT_BACKUP.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(file) => break (backup, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("Creating original-save recovery copy"),
        }
    };
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&backup);
        return Err(error).context("Writing original-save recovery copy");
    }
    Ok(backup)
}

impl Drop for SaveStore {
    fn drop(&mut self) {
        if let Err(error) = self.drain_explicit() {
            tracing::warn!(%error, "Completing pending save on shutdown failed");
        }
    }
}

fn join_worker(worker: std::thread::JoinHandle<Result<()>>) -> Result<()> {
    worker
        .join()
        .unwrap_or_else(|_| Err(anyhow::anyhow!("Save worker panicked")))
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    fs::create_dir_all(parent).context("Creating save directory")?;
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let name = format!(
            ".shin-save-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("Creating temporary save"),
        }
    };
    let result = (|| {
        file.write_all(bytes).context("Writing temporary save")?;
        file.sync_all().context("Syncing temporary save")?;
        fs::rename(&temporary, path).context("Replacing progress save")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
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
}
