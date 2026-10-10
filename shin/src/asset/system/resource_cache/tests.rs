use super::*;
use futures::executor::block_on;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn simultaneous_ids_share_one_producer_without_blocking_other_resources() {
    block_on(async {
        let cache = ResourceCache::<usize>::default();
        let count = AtomicUsize::new(0);
        let (send, recv) = oneshot::channel();
        let mut producer = Box::pin(cache.get_or_load(42, || async {
            count.fetch_add(1, Ordering::SeqCst);
            recv.await.unwrap();
            Ok(100)
        }));
        assert!(futures::poll!(&mut producer).is_pending());
        let mut waiter = Box::pin(cache.get_or_load(42, || async {
            panic!("A second alias must not decode/upload again")
        }));
        assert!(futures::poll!(&mut waiter).is_pending());
        assert_eq!(
            *cache.get_or_load(43, || async { Ok(200) }).await.unwrap(),
            200
        );
        send.send(()).unwrap();
        let first = producer.await.unwrap();
        let second = waiter.await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let weak = Arc::downgrade(&first);
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(
            weak.upgrade().is_none(),
            "Cache must not retain unused GPU assets"
        );
        assert_eq!(
            *cache
                .get_or_load(42, || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(300)
                })
                .await
                .unwrap(),
            300
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn failed_load_wakes_alias_waiters_and_allows_retry() {
    block_on(async {
        let cache = ResourceCache::<usize>::default();
        let (send, recv) = oneshot::channel();
        let mut producer = Box::pin(cache.get_or_load(1, || async {
            recv.await.unwrap();
            Err(anyhow!("disk read failed"))
        }));
        assert!(futures::poll!(&mut producer).is_pending());
        let mut waiter = Box::pin(cache.get_or_load(1, || async { panic!("duplicate") }));
        assert!(futures::poll!(&mut waiter).is_pending());
        send.send(()).unwrap();
        assert!(
            producer
                .await
                .unwrap_err()
                .to_string()
                .contains("disk read failed")
        );
        let retry = cache.get_or_load(1, || async { Ok(10) }).await.unwrap();
        assert!(
            waiter
                .await
                .unwrap_err()
                .to_string()
                .contains("disk read failed")
        );
        let retained = cache
            .get_or_load(1, || async { panic!("retry was evicted") })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&retry, &retained));
    });
}

#[test]
fn cancelled_producer_unblocks_waiters_without_retiring_a_replacement() {
    block_on(async {
        let cache = ResourceCache::<usize>::default();
        let mut producer = Box::pin(cache.get_or_load(1, || futures::future::pending()));
        assert!(futures::poll!(&mut producer).is_pending());
        let mut waiter = Box::pin(cache.get_or_load(1, || async { panic!("duplicate") }));
        assert!(futures::poll!(&mut waiter).is_pending());
        drop(producer);
        let retry = cache.get_or_load(1, || async { Ok(10) }).await.unwrap();
        assert!(waiter.await.unwrap_err().to_string().contains("cancelled"));
        let retained = cache
            .get_or_load(1, || async { panic!("retry was evicted") })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&retry, &retained));
        let separate_device = ResourceCache::<usize>::default();
        assert_eq!(
            *separate_device
                .get_or_load(1, || async { Ok(20) })
                .await
                .unwrap(),
            20
        );
    });
}

#[test]
fn pending_resource_fans_out_across_worker_threads() {
    let cache = Arc::new(ResourceCache::<usize>::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let (release, receiver) = oneshot::channel();
    let (ready, ready_receiver) = std::sync::mpsc::channel();
    let producer_cache = cache.clone();
    let producer_ready = ready.clone();
    let producer_calls = calls.clone();
    let producer = std::thread::spawn(move || {
        block_on(producer_cache.get_or_load(99, || async {
            producer_calls.fetch_add(1, Ordering::SeqCst);
            producer_ready.send(()).unwrap();
            receiver.await.unwrap();
            Ok(777)
        }))
        .unwrap()
    });
    ready_receiver.recv().unwrap();
    let waiters: Vec<_> = (0..4)
        .map(|_| {
            let cache = cache.clone();
            let ready = ready.clone();
            let calls = calls.clone();
            std::thread::spawn(move || {
                block_on(async {
                    let mut future = Box::pin(cache.get_or_load(99, || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        panic!("Pending aliases must share the worker")
                    }));
                    assert!(futures::poll!(&mut future).is_pending());
                    ready.send(()).unwrap();
                    future.await.unwrap()
                })
            })
        })
        .collect();
    for _ in 0..4 {
        ready_receiver.recv().unwrap();
    }
    release.send(()).unwrap();
    let value = producer.join().unwrap();
    for waiter in waiters {
        assert!(Arc::ptr_eq(&value, &waiter.join().unwrap()));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let weak = Arc::downgrade(&value);
    drop(value);
    assert!(weak.upgrade().is_none());
}
