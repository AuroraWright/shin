//! Ger EEC0/E500/E820 sparse archive and immutable restoration forks.
use anyhow::Result;

#[derive(Clone)]
pub struct Entry<T> {
    pub boundary: u32,
    pub state: T,
}
#[derive(Clone)]
pub struct Archive<T> {
    entries: Vec<Entry<T>>,
}
pub struct Fork<T> {
    pub archive: Archive<T>,
    pub restored: Option<Entry<T>>,
}
impl<T> Default for Archive<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}
impl<T> Archive<T> {
    pub(crate) fn from_entries(entries: Vec<Entry<T>>) -> Self {
        Self { entries }
    }
    pub(crate) fn into_entries(self) -> Vec<Entry<T>> {
        self.entries
    }
    pub fn entries(&self) -> &[Entry<T>] {
        &self.entries
    }
    pub fn capture(&mut self, boundary: u32, state: impl FnOnce() -> Result<T>) -> Result<bool> {
        if boundary / 200 + 1 <= self.entries.len() as u32 {
            return Ok(false);
        }
        self.entries.push(Entry {
            boundary,
            state: state()?,
        });
        Ok(true)
    }
}
impl<T: Clone> Archive<T> {
    pub fn clone_newest(&self) -> Fork<T> {
        let mut archive = self.clone();
        let restored = archive.entries.pop();
        Fork { archive, restored }
    }
    pub fn fork_for_restore(&self, saved_position: u32) -> Fork<T> {
        // E500 restores newest and copies its predecessors; E820 searches only
        // that copied archive and removes the selected checkpoint itself.
        let Fork {
            mut archive,
            mut restored,
        } = self.clone_newest();
        let mut first = 0;
        let mut remaining = archive.entries.len();
        while remaining != 0 {
            let half = remaining / 2;
            let middle = first + half;
            if archive.entries[middle].boundary < saved_position {
                first = middle + 1;
                remaining -= half + 1;
            } else {
                remaining = half;
            }
        }
        let selected = first.saturating_sub(1);
        if let Some(entry) = archive.entries.get(selected) {
            restored = Some(entry.clone());
            archive.entries.truncate(selected);
        }
        // Preserve native unsigned lower_bound behavior across wrapped input;
        // sorting would change the original archive and selected payload.
        Fork { archive, restored }
    }
}

#[cfg(test)]
mod tests;
