use super::*;
use futures::executor::block_on;
use std::{sync::mpsc, time::Duration};

fn reserve(cache: &AssetCache<u32, usize>, id: u32) -> CacheLoaderHandle<u32, usize> {
    match cache.lookup(id) {
        CacheLookupResult::LoadRequired(producer) => producer,
        _ => panic!("Expected a new producer"),
    }
}

#[test]
fn block_cache_shares_pending_value_and_releases_unused_owners() {
    block_on(async {
        let cache = AssetCache::new();
        let producer = reserve(&cache, 1);
        let mut waiting = Box::pin(producer.get_handle().wait_async());
        assert!(futures::poll!(&mut waiting).is_pending());
        let second = cache
            .lookup(1)
            .try_into_cache_handle()
            .unwrap_or_else(|_| panic!("duplicate"));
        let unrelated = reserve(&cache, 2);
        let value = Arc::new(10);
        cache.finish_load(producer, value.clone());
        let first = waiting.await.unwrap();
        let second = second.wait_async().await.unwrap();
        assert!(Arc::ptr_eq(&first, &value) && Arc::ptr_eq(&second, &value));
        let weak = Arc::downgrade(&value);
        drop(value);
        drop(first);
        drop(second);
        assert!(weak.upgrade().is_none());
        let replacement = reserve(&cache, 1);
        cache.finish_load(replacement, Arc::new(20));
        drop(unrelated);
    });
}

#[test]
fn block_failure_and_cancellation_wake_waiters_and_preserve_retries() {
    for cancel in [false, true] {
        block_on(async {
            let cache = AssetCache::new();
            let producer = reserve(&cache, 1);
            let sync_handle = producer.get_handle();
            let mut waiter = Box::pin(producer.get_handle().wait_async());
            assert!(futures::poll!(&mut waiter).is_pending());
            if cancel {
                drop(producer);
            } else {
                cache.fail_load(producer, Arc::new(anyhow!("bad block payload")));
            }
            let retry = reserve(&cache, 1);
            let value = Arc::new(99);
            cache.finish_load(retry, value.clone());
            let expected = if cancel {
                "cancelled"
            } else {
                "bad block payload"
            };
            assert!(waiter.await.unwrap_err().to_string().contains(expected));
            assert!(
                sync_handle
                    .try_wait_ref()
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
            let current = cache
                .lookup(1)
                .try_into_cache_handle()
                .unwrap_or_else(|_| panic!("old waiter retired retry"));
            assert!(Arc::ptr_eq(&current.wait_async().await.unwrap(), &value));
        });
    }
}

#[test]
fn blocked_threads_and_async_waiters_receive_the_same_failure_during_unwind() {
    let cache = AssetCache::new();
    let producer = reserve(&cache, 5);
    let sync_handle = producer.get_handle();
    let async_handle = producer.get_handle();
    let (ready, ready_recv) = mpsc::channel();
    let (done, done_recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        ready.send(()).unwrap();
        done.send(sync_handle.try_wait_ref().unwrap_err().to_string())
            .unwrap();
    });
    ready_recv.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _producer = producer;
            panic!("injected producer unwind");
        }))
        .is_err()
    );
    assert!(
        done_recv
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .contains("cancelled")
    );
    assert!(
        block_on(async_handle.wait_async())
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    thread.join().unwrap();
    drop(reserve(&cache, 5));
}

#[test]
fn completion_wakers_can_reenter_cache_without_holding_the_publication_lock() {
    struct Reenter {
        cache: Arc<AssetCache<u32, usize>>,
        done: mpsc::Sender<()>,
    }
    impl std::task::Wake for Reenter {
        fn wake(self: Arc<Self>) {
            let other = reserve(&self.cache, 2);
            self.cache.finish_load(other, Arc::new(2));
            self.done.send(()).unwrap();
        }
    }
    let cache = Arc::new(AssetCache::new());
    let producer = reserve(&cache, 1);
    let mut waiter = Box::pin(producer.get_handle().wait_async());
    let (done, done_recv) = mpsc::channel();
    let waker = std::task::Waker::from(Arc::new(Reenter {
        cache: cache.clone(),
        done,
    }));
    let mut context = std::task::Context::from_waker(&waker);
    assert!(std::future::Future::poll(waiter.as_mut(), &mut context).is_pending());
    let thread_cache = cache.clone();
    let thread = std::thread::spawn(move || thread_cache.finish_load(producer, Arc::new(1)));
    done_recv.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(*block_on(waiter).unwrap(), 1);
    thread.join().unwrap();
}
