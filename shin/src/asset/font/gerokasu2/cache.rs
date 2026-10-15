//! FNT6 descriptor-identity residency with the shared producer lifecycle.
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use shin_core::format::font::gerokasu2::Gerokasu2Font;

use crate::asset::{
    font::cache::Residency,
    system::cache::{AssetCache, CacheHandle, CacheLoaderHandle, CacheLookupResult},
};

pub(crate) struct Gerokasu2GlyphCache<V> {
    pending: AssetCache<u32, V>,
    pub(super) residents: Mutex<Residency<u32, V>>,
}
impl<V: Send + Sync + 'static> Default for Gerokasu2GlyphCache<V> {
    fn default() -> Self {
        Self {
            pending: AssetCache::new(),
            residents: Mutex::new(Residency::default()),
        }
    }
}
impl<V: Send + Sync + 'static> Gerokasu2GlyphCache<V> {
    pub(super) fn lookup(&self, identity: u32) -> CacheLookupResult<u32, V> {
        let result = self.pending.lookup(identity);
        // Acquire an external owner before touching the resident, matching the
        // existing native cache's protection against eviction during lookup.
        self.residents.lock().touch(identity);
        result
    }
    pub(super) fn finish_load(
        &self,
        identity: u32,
        producer: CacheLoaderHandle<u32, V>,
        value: Arc<V>,
    ) {
        let removed = self.residents.lock().insert(identity, value.clone());
        for key in removed {
            self.pending.forget_expired(&key);
        }
        self.pending.finish_load(producer, value);
    }
    pub(super) fn complete_load(
        &self,
        request: GlyphRequest<V>,
        load: impl FnOnce() -> anyhow::Result<V>,
    ) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(load))
            .map_err(|panic| {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic");
                anyhow::anyhow!("FNT6 glyph worker failed: {message}")
            })
            .and_then(|result| result);
        match result {
            Ok(glyph) => self.finish_load(request.identity, request.producer, Arc::new(glyph)),
            Err(error) => {
                self.residents.lock().fail();
                let error =
                    Arc::new(error.context(format!("Loading FNT6 glyph {:?}", request.codepoint)));
                tracing::error!(identity = request.identity, glyph = ?request.codepoint, error = %error, "FNT6 glyph load failed");
                self.pending.fail_load(request.producer, error);
            }
        }
    }
}

pub(super) struct GlyphRequest<V> {
    pub identity: u32,
    pub codepoint: char,
    pub producer: CacheLoaderHandle<u32, V>,
}

pub(super) struct GlyphBatch<V> {
    pub handles: Vec<CacheHandle<V>>,
    pub requests: Vec<GlyphRequest<V>>,
}

/// The first occurrence of an identity chooses its cold source. Unicode aliases
/// and other fonts then reuse that native record, including its first geometry.
pub(super) fn request_glyphs<V: Send + Sync + 'static>(
    font: &Gerokasu2Font,
    cache: &Gerokasu2GlyphCache<V>,
    characters: &[char],
) -> GlyphBatch<V> {
    let mut unique = IndexMap::new();
    let mut handles = Vec::with_capacity(characters.len());
    let mut requests = Vec::new();
    for &codepoint in characters {
        let identity = font.lookup(u32::from(codepoint)).info.identity;
        let handle = unique.entry(identity).or_insert_with(|| {
            match cache.lookup(identity).try_into_cache_handle() {
                Ok(handle) => handle,
                Err(producer) => {
                    let handle = producer.get_handle();
                    requests.push(GlyphRequest {
                        identity,
                        codepoint,
                        producer,
                    });
                    handle
                }
            }
        });
        handles.push(handle.clone());
    }
    GlyphBatch { handles, requests }
}

#[cfg(test)]
mod tests;
