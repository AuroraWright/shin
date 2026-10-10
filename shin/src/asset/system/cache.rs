//! Weak block ownership with synchronous glyph waits and asynchronous asset waits.
//! Producers retire their own generation on success, error or cancellation.
use std::{
    hash::Hash,
    sync::{Arc, Weak},
};

use anyhow::{Result, anyhow};
use futures::{
    FutureExt,
    channel::oneshot,
    future::{BoxFuture, Shared},
};
use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;

type LoadResult<V> = std::result::Result<Arc<V>, Arc<anyhow::Error>>;
type Pending<V> = Shared<BoxFuture<'static, LoadResult<V>>>;
type Entries<K, V> = Arc<Mutex<IndexMap<K, CacheEntry<V>>>>;

enum CacheEntry<V> {
    Loaded(Weak<V>),
    Loading(CacheLoadHandle<V>),
}

pub struct AssetCache<K, V> {
    cache: Entries<K, V>,
}

impl<K: Hash + Eq + Clone, V: Send + Sync + 'static> AssetCache<K, V> {
    #[cfg(test)]
    pub(crate) fn reference_ownership_counts(&self) -> (usize, usize) {
        let cache = self.cache.lock();
        let live = cache
            .values()
            .filter(|entry| matches!(entry, CacheEntry::Loaded(value) if value.strong_count() > 0))
            .count();
        let pending = cache
            .values()
            .filter(|entry| matches!(entry, CacheEntry::Loading(_)))
            .count();
        (live, pending)
    }

    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(IndexMap::new())),
        }
    }

    pub fn lookup(&self, key: K) -> CacheLookupResult<K, V> {
        let mut cache = self.cache.lock();
        match cache.get(&key) {
            Some(CacheEntry::Loaded(value)) => {
                if let Some(value) = value.upgrade() {
                    return CacheLookupResult::Loaded(value);
                }
            }
            Some(CacheEntry::Loading(handle)) => return CacheLookupResult::Loading(handle.clone()),
            None => {}
        }
        let (sender, receiver) = oneshot::channel::<LoadResult<V>>();
        let pending = async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(Arc::new(anyhow!("Asset producer retired"))))
        }
        .boxed()
        .shared();
        let result = CacheLoadHandle {
            inner: Arc::new(OnceCell::new()),
            pending,
        };
        cache.insert(key.clone(), CacheEntry::Loading(result.clone()));
        CacheLookupResult::LoadRequired(CacheLoaderHandle {
            key,
            cache: self.cache.clone(),
            result,
            sender: Some(sender),
        })
    }

    /// Remove metadata for a retired resident without touching live owners or
    /// a newer pending generation of the same key.
    pub fn forget_expired(&self, key: &K) {
        let mut cache = self.cache.lock();
        if matches!(cache.get(key), Some(CacheEntry::Loaded(value)) if value.strong_count() == 0) {
            cache.shift_remove(key);
        }
    }

    pub fn finish_load(&self, mut handle: CacheLoaderHandle<K, V>, asset: Arc<V>) {
        assert!(
            Arc::ptr_eq(&self.cache, &handle.cache),
            "Producer belongs to another cache"
        );
        handle.complete(Ok(asset));
    }

    pub fn fail_load(&self, mut handle: CacheLoaderHandle<K, V>, error: Arc<anyhow::Error>) {
        assert!(
            Arc::ptr_eq(&self.cache, &handle.cache),
            "Producer belongs to another cache"
        );
        handle.complete(Err(error));
    }
}

/// Owns one pending generation. Dropping it cancels that generation and wakes
/// all its waiters; old waiters can never remove a replacement generation.
#[must_use]
pub struct CacheLoaderHandle<K: Hash + Eq, V> {
    key: K,
    cache: Entries<K, V>,
    result: CacheLoadHandle<V>,
    sender: Option<oneshot::Sender<LoadResult<V>>>,
}
impl<K: Hash + Eq, V> CacheLoaderHandle<K, V> {
    pub fn get_handle(&self) -> CacheHandle<V> {
        CacheHandle::Loading(self.result.clone())
    }
    fn complete(&mut self, result: LoadResult<V>) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        {
            let mut cache = self.cache.lock();
            if matches!(cache.get(&self.key), Some(CacheEntry::Loading(handle))
                if Arc::ptr_eq(&handle.inner, &self.result.inner))
            {
                match &result {
                    Ok(value) => {
                        *cache.get_mut(&self.key).unwrap() =
                            CacheEntry::Loaded(Arc::downgrade(value));
                    }
                    Err(_) => {
                        cache.shift_remove(&self.key);
                    }
                }
            }
        }
        // Both parked threads and async wakers can look up further blocks.
        // Publish outside the map lock so they can safely reenter the cache.
        let _ = self.result.inner.set(result.clone());
        let _ = sender.send(result);
    }
}
impl<K: Hash + Eq, V> Drop for CacheLoaderHandle<K, V> {
    fn drop(&mut self) {
        if self.sender.is_some() {
            self.complete(Err(Arc::new(anyhow!("Asset load cancelled"))));
        }
    }
}

#[derive(Debug)]
pub struct CacheLoadHandle<V> {
    inner: Arc<OnceCell<LoadResult<V>>>,
    pending: Pending<V>,
}
impl<V> Clone for CacheLoadHandle<V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            pending: self.pending.clone(),
        }
    }
}
impl<V> CacheLoadHandle<V> {
    pub async fn wait_async(self) -> Result<Arc<V>> {
        self.pending.await.map_err(|error| anyhow!("{error:#}"))
    }
    pub fn try_wait_ref(&self) -> Result<&V> {
        self.inner
            .wait()
            .as_ref()
            .map(|value| value.as_ref())
            .map_err(|error| anyhow!("{error:#}"))
    }
}

pub enum CacheLookupResult<K: Hash + Eq, V> {
    Loaded(Arc<V>),
    Loading(CacheLoadHandle<V>),
    LoadRequired(CacheLoaderHandle<K, V>),
}
impl<K: Hash + Eq, V> CacheLookupResult<K, V> {
    pub fn try_into_cache_handle(
        self,
    ) -> std::result::Result<CacheHandle<V>, CacheLoaderHandle<K, V>> {
        match self {
            Self::Loaded(value) => Ok(CacheHandle::Loaded(value)),
            Self::Loading(handle) => Ok(CacheHandle::Loading(handle)),
            Self::LoadRequired(producer) => Err(producer),
        }
    }
}

#[derive(Debug)]
pub enum CacheHandle<V> {
    Loaded(Arc<V>),
    Loading(CacheLoadHandle<V>),
}
impl<V> CacheHandle<V> {
    pub async fn wait_async(self) -> Result<Arc<V>> {
        match self {
            Self::Loaded(value) => Ok(value),
            Self::Loading(handle) => handle.wait_async().await,
        }
    }
    pub fn try_wait_ref(&self) -> Result<&V> {
        match self {
            Self::Loaded(value) => Ok(value),
            Self::Loading(handle) => handle.try_wait_ref(),
        }
    }
}
impl<V> Clone for CacheHandle<V> {
    fn clone(&self) -> Self {
        match self {
            Self::Loaded(value) => Self::Loaded(value.clone()),
            Self::Loading(handle) => Self::Loading(handle.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
