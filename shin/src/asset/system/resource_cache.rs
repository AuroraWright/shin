//! Weak resource ownership with asynchronous sharing of pending ID loads.
//! A failed or cancelled producer wakes its waiters and permits a later retry.
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Weak},
};

use anyhow::{Result, anyhow};
use futures::{
    FutureExt,
    channel::oneshot,
    future::{BoxFuture, Shared},
};
use parking_lot::Mutex;

type SharedResult<V> = std::result::Result<Arc<V>, Arc<anyhow::Error>>;
type Pending<V> = Shared<BoxFuture<'static, SharedResult<V>>>;
enum Entry<V> {
    Loaded(Weak<V>),
    Loading {
        result: Pending<V>,
        generation: Arc<()>,
    },
}
type Entries<V> = Arc<Mutex<BTreeMap<u32, Entry<V>>>>;

pub struct ResourceCache<V: Send + Sync + 'static> {
    entries: Entries<V>,
}
impl<V: Send + Sync + 'static> Default for ResourceCache<V> {
    fn default() -> Self {
        Self {
            entries: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}
enum Request<V: Send + Sync + 'static> {
    Ready(Arc<V>),
    Waiting(Pending<V>),
    Produce(Producer<V>),
}
struct Producer<V: Send + Sync + 'static> {
    entries: Entries<V>,
    id: u32,
    generation: Arc<()>,
    sender: Option<oneshot::Sender<SharedResult<V>>>,
}
impl<V: Send + Sync + 'static> Producer<V> {
    fn finish(&mut self, result: SharedResult<V>) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        {
            let mut entries = self.entries.lock();
            if matches!(entries.get(&self.id), Some(Entry::Loading { generation, .. })
                if Arc::ptr_eq(generation, &self.generation))
            {
                match &result {
                    Ok(value) => {
                        entries.insert(self.id, Entry::Loaded(Arc::downgrade(value)));
                    }
                    Err(_) => {
                        entries.remove(&self.id);
                    }
                }
            }
        }
        // Wake outside the lock: wakers may immediately look up another ID.
        let _ = sender.send(result);
    }
}
impl<V: Send + Sync + 'static> Drop for Producer<V> {
    fn drop(&mut self) {
        self.finish(Err(Arc::new(anyhow!("Resource load cancelled"))));
    }
}
impl<V: Send + Sync + 'static> ResourceCache<V> {
    #[cfg(test)]
    pub(crate) fn reference_ownership_counts(&self) -> (usize, usize) {
        let entries = self.entries.lock();
        let live = entries
            .values()
            .filter(|entry| matches!(entry, Entry::Loaded(value) if value.strong_count() > 0))
            .count();
        let pending = entries
            .values()
            .filter(|entry| matches!(entry, Entry::Loading { .. }))
            .count();
        (live, pending)
    }

    fn request(&self, id: u32) -> Request<V> {
        let mut entries = self.entries.lock();
        match entries.get(&id) {
            Some(Entry::Loaded(value)) => {
                if let Some(value) = value.upgrade() {
                    return Request::Ready(value);
                }
            }
            Some(Entry::Loading { result, .. }) => return Request::Waiting(result.clone()),
            None => {}
        }
        let (sender, receiver) = oneshot::channel::<SharedResult<V>>();
        let result = async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(Arc::new(anyhow!("Resource producer retired"))))
        }
        .boxed()
        .shared();
        let generation = Arc::new(());
        entries.insert(
            id,
            Entry::Loading {
                result,
                generation: generation.clone(),
            },
        );
        Request::Produce(Producer {
            entries: self.entries.clone(),
            id,
            generation,
            sender: Some(sender),
        })
    }

    pub async fn get_or_load<F, Fut>(&self, id: u32, load: F) -> Result<Arc<V>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V>>,
    {
        let result = match self.request(id) {
            Request::Ready(value) => return Ok(value),
            Request::Waiting(result) => result.await,
            Request::Produce(mut producer) => {
                let result = load().await.map(Arc::new).map_err(Arc::new);
                producer.finish(result.clone());
                result
            }
        };
        result.map_err(|error| anyhow!("{error:#}"))
    }
}

#[cfg(test)]
mod tests;
