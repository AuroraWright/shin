//! Ger native file import and atomic publication to an explicitly chosen path.
use anyhow::{Context, Result, ensure};
use shin_core::format::save::gerokasu2::{GameData, SaveData};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

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
    dirty: bool,
}
impl Store {
    pub fn load(path: PathBuf, imported: Option<SaveData>) -> Result<Self> {
        let data = if let Some(data) = imported {
            data
        } else if path.exists() {
            read(&path)?
        } else {
            SaveData::default()
        };
        Ok(Self {
            data,
            path,
            pending: None,
            queued: false,
            dirty: false,
        })
    }
    pub fn request_flush(&mut self) -> Result<()> {
        self.dirty = true;
        self.queued = true;
        self.poll_flush()
    }
    pub fn poll_flush(&mut self) -> Result<()> {
        if let Some(task) = self.pending.as_mut() {
            if let Some(result) = task.poll_naive() {
                self.pending = None;
                result?;
            }
        }
        if self.pending.is_none() && self.queued {
            let snapshot = Self {
                data: self.data.clone(),
                path: self.path.clone(),
                pending: None,
                queued: false,
                dirty: false,
            };
            self.pending = Some(shin_tasks::async_io::spawn(async move { snapshot.flush() }));
            self.queued = false;
        }
        Ok(())
    }
    /// Drain prior writes before the final snapshot so an older worker cannot
    /// rename its file over the latest progress during orderly shutdown.
    pub fn finish(&mut self) -> Result<()> {
        if let Some(task) = self.pending.take() {
            shin_tasks::block_on(task)?;
        }
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
