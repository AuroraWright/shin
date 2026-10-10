use std::{collections::BTreeMap, sync::Arc};

use parking_lot::Mutex;

use super::GpuFontGlyph;
use crate::asset::system::cache::{AssetCache, CacheLoaderHandle, CacheLookupResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct GlyphKey {
    pub font: u32,
    pub codepoint: char,
}

struct Resident<V> {
    value: Arc<V>,
    age: u32,
}

pub(super) struct Residency<K, V> {
    entries: BTreeMap<K, Resident<V>>,
    age: u32,
}
impl<K: Ord + Copy, V> Default for Residency<K, V> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            age: 0,
        }
    }
}
impl<K: Ord + Copy, V> Residency<K, V> {
    pub(super) fn touch(&mut self, key: K) {
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.age < self.age {
                self.age += 1;
                entry.age = self.age;
            }
        }
    }
    pub(super) fn insert(&mut self, key: K, value: Arc<V>) -> Vec<K> {
        self.age += 1;
        self.entries.insert(
            key,
            Resident {
                value,
                age: self.age,
            },
        );
        // Original68980: compact ages at65536, then retire at most32 oldest
        // unowned records whenever the total resident count reaches1024.
        if self.age >= 65536 {
            let mut ordered: Vec<_> = self.entries.iter().map(|(&k, v)| (v.age, k)).collect();
            ordered.sort_unstable();
            for (i, (_, key)) in ordered.into_iter().enumerate() {
                self.entries.get_mut(&key).unwrap().age = i as u32 + 1;
            }
            self.age = self.entries.len() as u32;
        }
        let mut removed = Vec::new();
        if self.entries.len() >= 1024 {
            let mut unused: Vec<_> = self
                .entries
                .iter()
                .filter(|(_, entry)| Arc::strong_count(&entry.value) == 1)
                .map(|(&key, entry)| (entry.age, key))
                .collect();
            unused.sort_unstable();
            for (_, key) in unused.into_iter().take(32) {
                self.entries.remove(&key);
                removed.push(key);
            }
        }
        removed
    }
    #[cfg(test)]
    pub(super) fn snapshot(&self) -> BTreeMap<K, (usize, u32)> {
        self.entries
            .iter()
            .map(|(&key, entry)| (key, (Arc::strong_count(&entry.value) - 1, entry.age)))
            .collect()
    }
    #[cfg(test)]
    pub(super) fn set_age(&mut self, age: u32) {
        self.age = age;
    }
    #[cfg(test)]
    pub(super) fn fail(&mut self) {
        self.age += 1;
    }
    #[cfg(test)]
    pub(super) fn test_owner(&self, key: K) -> Arc<V> {
        self.entries[&key].value.clone()
    }
}

/// Native retained glyph cache, shared by all fonts in one device context.
/// Pending producers retain the existing generation/error/cancellation rules.
pub struct FontGlyphCache {
    pending: AssetCache<GlyphKey, GpuFontGlyph>,
    residents: Mutex<Residency<GlyphKey, GpuFontGlyph>>,
}
impl Default for FontGlyphCache {
    fn default() -> Self {
        Self {
            pending: AssetCache::new(),
            residents: Mutex::new(Residency::default()),
        }
    }
}
impl FontGlyphCache {
    pub(super) fn lookup(&self, key: GlyphKey) -> CacheLookupResult<GlyphKey, GpuFontGlyph> {
        // Acquire the external owner before updating age, so eviction cannot
        // retire a resident being handed to a competing text owner.
        let result = self.pending.lookup(key);
        self.residents.lock().touch(key);
        result
    }
    pub(super) fn finish_load(
        &self,
        key: GlyphKey,
        producer: CacheLoaderHandle<GlyphKey, GpuFontGlyph>,
        value: Arc<GpuFontGlyph>,
    ) {
        let removed = self.residents.lock().insert(key, value.clone());
        for key in removed {
            self.pending.forget_expired(&key);
        }
        // Do not wake waiters while the residency lock is held.
        self.pending.finish_load(producer, value);
    }
    pub(super) fn fail_load(
        &self,
        producer: CacheLoaderHandle<GlyphKey, GpuFontGlyph>,
        error: Arc<anyhow::Error>,
    ) {
        // A native cold allocation failure still consumes an age value, but
        // neither inserts a resident nor triggers eviction/compaction.
        self.residents.lock().age += 1;
        self.pending.fail_load(producer, error);
    }
}

#[cfg(test)]
mod tests;
