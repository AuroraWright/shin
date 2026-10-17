//! Ger native file import and atomic publication to game-specific desktop storage.
use anyhow::{Context, Result, ensure};
use shin_core::format::save::gerokasu2::{GameData, SaveData};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) fn progress_path(cli: &crate::cli::Cli) -> Result<PathBuf> {
    let path = cli.save_file();
    ensure!(!path.as_os_str().is_empty(), "Ger save path is empty");
    Ok(path)
}

pub fn read(path: &Path) -> Result<SaveData> {
    SaveData::decode(
        &std::fs::read(path).with_context(|| format!("Reading Ger save {}", path.display()))?,
    )
    .with_context(|| format!("Decoding Ger save {}", path.display()))
}
pub fn slot(data: &SaveData, slot: u8) -> Result<&GameData> {
    ensure!(slot <= 100, "Ger save slot must be0..100");
    if slot == 0 {
        data.auto_save_slot.as_ref()
    } else {
        data.manual_save_slots[usize::from(slot - 1)].as_ref()
    }
    .with_context(|| format!("Ger save slot {slot} is empty"))
}

pub struct Store {
    pub data: SaveData,
    pub path: PathBuf,
    pending: Option<shin_tasks::AsyncTask<Result<()>>>,
    queued: bool,
    queued_snapshot: Option<SaveData>,
    queued_minimum: Duration,
    dirty: bool,
    pending_manual: bool,
    manual: Option<ManualWrite>,
}
struct ManualWrite {
    snapshot: SaveData,
    slot: usize,
    previous: Option<GameData>,
    remembered: u8,
    remember: bool,
    armed: bool,
    writing: bool,
    deferred_flush: bool,
    result: Option<Result<()>>,
}
impl Store {
    pub fn load(path: PathBuf) -> Result<Self> {
        let data = if path.exists() {
            read(&path)?
        } else {
            SaveData::default()
        };
        Ok(Self {
            data,
            path,
            pending: None,
            queued: false,
            queued_snapshot: None,
            queued_minimum: Duration::ZERO,
            dirty: false,
            pending_manual: false,
            manual: None,
        })
    }
    #[cfg(test)]
    pub fn request_flush(&mut self) -> Result<()> {
        self.dirty = true;
        if let Some(manual) = self.manual.as_mut() {
            // One manual operation publishes one staged snapshot. Later live
            // globals are a successor, never part of a rollbackable operation.
            manual.deferred_flush = true;
            return self.poll_flush();
        }
        self.queued = true;
        self.queued_snapshot = None;
        self.queued_minimum = Duration::ZERO;
        self.poll_flush()
    }
    /// SaveGame B02B8 deep-copies the entire SaveData before its child opens.
    /// Dispatch that constructor snapshot, preserving subsequent live changes.
    pub fn request_frozen_flush(&mut self, snapshot: SaveData, minimum: Duration) -> Result<()> {
        ensure!(
            self.manual.is_none(),
            "Ger frozen globals write overlaps a manual transaction"
        );
        self.dirty = true;
        self.queued = true;
        self.queued_snapshot = Some(snapshot);
        self.queued_minimum = minimum;
        self.poll_flush()
    }
    /// Native menu slot0..99. Stage only that record and optional remembered
    /// selection, preserving all other live globals. A second operation is
    /// rejected until poll_manual_write consumes this operation's result.
    pub fn begin_manual_write(
        &mut self,
        slot: u8,
        replacement: Option<GameData>,
        remember: bool,
    ) -> Result<()> {
        ensure!(usize::from(slot) < 100, "Ger manual slot must be0..99");
        ensure!(self.manual.is_none(), "Ger manual write is already pending");
        let index = usize::from(slot);
        let previous = std::mem::replace(&mut self.data.manual_save_slots[index], replacement);
        let remembered = self.data.save_menu_position;
        if remember {
            self.data.save_menu_position = slot;
        }
        self.manual = Some(ManualWrite {
            snapshot: self.data.clone(),
            slot: index,
            previous,
            remembered,
            remember,
            armed: false,
            writing: false,
            deferred_flush: false,
            result: None,
        });
        self.dirty = true;
        Ok(())
    }
    /// B24A0 first dispatches the worker after the no-button Question opens.
    /// Merely staging the menu record must never start a file publication.
    pub fn start_manual_write(&mut self, minimum: Duration) -> Result<()> {
        let manual = self
            .manual
            .as_mut()
            .context("Ger manual write is not staged")?;
        if manual.result.is_none() && !manual.armed {
            manual.armed = true;
            self.queued = true;
            self.queued_minimum = minimum;
        }
        self.poll_flush()
    }
    /// Wait for any predecessor and the operation's staged publication. Writes
    /// requested after that snapshot started are deferred until this result is
    /// consumed; a later globals-write failure cannot undo a committed slot.
    pub fn poll_manual_write(&mut self) -> Option<Result<()>> {
        self.manual.as_ref()?;
        if let Err(error) = self.poll_flush() {
            return Some(Err(error));
        }
        self.manual.as_ref()?.result.as_ref()?;
        let mut manual = self.manual.take().unwrap();
        self.queued |= manual.deferred_flush;
        manual.result.take()
    }
    fn complete_pending(&mut self, result: Result<()>, was_manual: bool) -> Result<()> {
        if self.manual.is_none() {
            if result.is_err() {
                self.queued = false;
            }
            return result;
        }
        if was_manual || result.is_err() {
            let manual = self.manual.as_mut().unwrap();
            if result.is_err() {
                self.data.manual_save_slots[manual.slot] = manual.previous.take();
                if manual.remember {
                    self.data.save_menu_position = manual.remembered;
                }
            }
            // No queued snapshot containing a failed staged slot may escape.
            self.queued = false;
            self.queued_snapshot = None;
            manual.result = Some(result);
        }
        Ok(())
    }
    pub fn poll_flush(&mut self) -> Result<()> {
        if let Some(task) = self.pending.as_mut() {
            if let Some(result) = task.poll_naive() {
                self.pending = None;
                let was_manual = std::mem::take(&mut self.pending_manual);
                self.complete_pending(result, was_manual)?;
            }
        }
        if self
            .manual
            .as_ref()
            .is_some_and(|m| !m.armed || m.result.is_some())
        {
            return Ok(());
        }
        if self.pending.is_none() && self.queued {
            let snapshot = Self {
                data: if let Some(manual) = self.manual.as_ref() {
                    manual.snapshot.clone()
                } else {
                    self.queued_snapshot
                        .take()
                        .unwrap_or_else(|| self.data.clone())
                },
                path: self.path.clone(),
                pending: None,
                queued: false,
                queued_snapshot: None,
                queued_minimum: Duration::ZERO,
                dirty: false,
                pending_manual: false,
                manual: None,
            };
            let minimum = std::mem::take(&mut self.queued_minimum);
            self.pending = Some(shin_tasks::async_io::spawn(async move {
                let started = Instant::now();
                snapshot.flush()?;
                if let Some(remaining) = minimum.checked_sub(started.elapsed()) {
                    std::thread::sleep(remaining);
                }
                Ok(())
            }));
            self.pending_manual = self.manual.is_some();
            if let Some(manual) = self.manual.as_mut() {
                manual.writing = true;
            }
            self.queued = false;
        }
        Ok(())
    }
    /// A menu must wait for both the active write and a coalesced successor.
    pub fn is_flush_pending(&self) -> bool {
        self.pending.is_some()
            || self.queued
            || self.manual.as_ref().is_some_and(|m| m.result.is_none())
            || self.manual.as_ref().is_some_and(|m| m.deferred_flush)
    }
    /// Drain prior writes before the final snapshot so an older worker cannot
    /// rename its file over the latest progress during orderly shutdown.
    pub fn finish(&mut self) -> Result<()> {
        if self.manual.is_some() {
            self.start_manual_write(Duration::ZERO)?;
        }
        while self.manual.is_some() {
            if let Some(result) = self.poll_manual_write() {
                result?;
                break;
            }
            if let Some(task) = self.pending.take() {
                let was_manual = std::mem::take(&mut self.pending_manual);
                self.complete_pending(shin_tasks::block_on(task), was_manual)?;
            }
        }
        if let Some(task) = self.pending.take() {
            shin_tasks::block_on(task)?;
        }
        self.pending_manual = false;
        self.queued = false;
        self.flush()?;
        self.dirty = false;
        Ok(())
    }
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
    /// Encode before opening any file. A failed native-width validation cannot
    /// damage the previous save; rename publishes a complete encrypted body.
    pub fn flush(&self) -> Result<()> {
        ensure!(
            self.manual.is_none(),
            "Ger manual write must publish through its task"
        );
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let bytes = self.data.encode()?;
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let name = self
            .path
            .file_name()
            .context("Ger save path has no filename")?
            .to_string_lossy();
        let temporary = self.path.with_file_name(format!(
            ".{name}.gerokasu2-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.with_context(|| format!("Publishing Ger save {}", self.path.display()))
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        if self.dirty || self.pending.is_some() {
            if let Err(error) = self.finish() {
                tracing::error!(%error,"Ger save worker failed during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests;
