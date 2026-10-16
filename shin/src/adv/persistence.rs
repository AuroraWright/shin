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

pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
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
mod tests;
