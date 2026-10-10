use std::{
    sync::{Arc, Once, mpsc},
    time::Duration,
};

use super::{AsyncTask, async_io, compute};

const DEADLINE: Duration = Duration::from_secs(5);

fn pools() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        super::create_rayon_pool(2);
        // The engine has one I/O worker. Cancellation must leave it available.
        super::create_async_io_pool(1);
    });
}

struct Signal(mpsc::Sender<()>);
impl Drop for Signal {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn cancel<T: Send + 'static>(
    task: AsyncTask<T>,
) -> (std::thread::JoinHandle<()>, mpsc::Receiver<()>) {
    let (done, finished) = mpsc::channel();
    let join = std::thread::spawn(move || {
        task.cancel_and_retire();
        done.send(()).unwrap();
    });
    (join, finished)
}

#[test]
fn retirement_waits_for_late_compute_output_and_io_children() {
    pools();
    let path = std::env::temp_dir().join(format!("shin-retirement-{}.bin", std::process::id()));
    std::fs::write(&path, b"real loader input").unwrap();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (started, entry) = mpsc::channel();
    let (resume, gate) = mpsc::channel();
    let (prepared, preparation) = mpsc::channel();
    let (finish, final_gate) = mpsc::channel();
    let (dropped, cancellation) = mpsc::channel();
    let task = async_io::spawn_loader(async move {
        let _cancelled = Signal(dropped);
        compute::spawn(move || {
            started.send(()).unwrap();
            gate.recv_timeout(DEADLINE).unwrap();
            // This child is spawned after its parent's future has been cancelled.
            // Its I/O must run while the caller waits for the CPU job to retire.
            let (data, bytes) = mpsc::channel();
            let retained = owned.clone();
            let child = async_io::spawn(async move {
                let _retained = retained;
                data.send(std::fs::read(path).unwrap()).unwrap();
                std::future::pending::<()>().await;
            });
            assert_eq!(bytes.recv_timeout(DEADLINE).unwrap(), b"real loader input");
            prepared.send(()).unwrap();
            final_gate.recv_timeout(DEADLINE).unwrap();
            // A rejected compute result owns both the resource and pending child.
            (owned, child)
        })
        .await
    });
    entry.recv_timeout(DEADLINE).unwrap();
    let (join, finished) = cancel(task);
    cancellation.recv_timeout(DEADLINE).unwrap();
    let mut probe = async_io::spawn(async { 42 });
    assert_eq!(probe.poll_with_timeout(DEADLINE), Some(42));
    assert!(weak.upgrade().is_some());
    assert!(finished.try_recv().is_err());
    resume.send(()).unwrap();
    preparation.recv_timeout(DEADLINE).unwrap();
    assert!(weak.upgrade().is_some());
    assert!(finished.recv_timeout(Duration::from_millis(5)).is_err());
    finish.send(()).unwrap();
    finished.recv_timeout(DEADLINE).unwrap();
    join.join().unwrap();
    assert!(weak.upgrade().is_none());
    std::fs::remove_file(
        std::env::temp_dir().join(format!("shin-retirement-{}.bin", std::process::id())),
    )
    .unwrap();
}

#[test]
fn retirement_drops_ready_output_before_waiting_for_decoder_tasks() {
    pools();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (started, entry) = mpsc::channel();
    let (dropped, completion) = mpsc::channel();
    let task = async_io::spawn_loader(async move {
        let _ready = Signal(dropped);
        let child = async_io::spawn(async move {
            let _owned = owned;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        child
    });
    entry.recv_timeout(DEADLINE).unwrap();
    completion.recv_timeout(DEADLINE).unwrap();
    // Polling unrelated I/O establishes that the ready loader's future is gone.
    assert_eq!(
        async_io::spawn(async { 7 }).poll_with_timeout(DEADLINE),
        Some(7)
    );
    assert!(weak.upgrade().is_some());
    let (join, finished) = cancel(task);
    finished.recv_timeout(DEADLINE).unwrap();
    join.join().unwrap();
    assert!(weak.upgrade().is_none());
}

#[test]
fn retirement_tracks_nested_detached_compute_after_parent_cancellation() {
    pools();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (started, entry) = mpsc::channel();
    let (resume, gate) = mpsc::channel();
    let (nested, nesting) = mpsc::channel();
    let (finish, final_gate) = mpsc::channel();
    let (dropped, cancellation) = mpsc::channel();
    let task = async_io::spawn_loader(async move {
        let _cancelled = Signal(dropped);
        compute::spawn(move || {
            started.send(()).unwrap();
            gate.recv_timeout(DEADLINE).unwrap();
            compute::spawn_and_forget(move || {
                let _owned = owned;
                nested.send(()).unwrap();
                final_gate.recv_timeout(DEADLINE).unwrap();
            });
        })
        .await;
    });
    entry.recv_timeout(DEADLINE).unwrap();
    let (join, finished) = cancel(task);
    cancellation.recv_timeout(DEADLINE).unwrap();
    resume.send(()).unwrap();
    nesting.recv_timeout(DEADLINE).unwrap();
    assert!(weak.upgrade().is_some());
    assert!(finished.recv_timeout(Duration::from_millis(5)).is_err());
    finish.send(()).unwrap();
    finished.recv_timeout(DEADLINE).unwrap();
    join.join().unwrap();
    assert!(weak.upgrade().is_none());
}

#[test]
fn successful_loader_keeps_published_child_alive_until_owner_drops() {
    pools();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (started, entry) = mpsc::channel();
    let (dropped, retirement) = mpsc::channel();
    let mut task = async_io::spawn_loader(async move {
        async_io::spawn(async move {
            let _owned = owned;
            let _retired = Signal(dropped);
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        })
    });
    entry.recv_timeout(DEADLINE).unwrap();
    let child = task.poll_with_timeout(DEADLINE).unwrap();
    drop(task);
    assert!(weak.upgrade().is_some());
    assert!(retirement.try_recv().is_err());
    drop(child);
    retirement.recv_timeout(DEADLINE).unwrap();
    assert_eq!(
        async_io::spawn(async { 0 }).poll_with_timeout(DEADLINE),
        Some(0)
    );
    assert!(weak.upgrade().is_none());
}

#[test]
fn retirement_handle_drains_output_already_polled_from_loader() {
    pools();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (started, entry) = mpsc::channel();
    let mut task = async_io::spawn_loader(async move {
        async_io::spawn(async move {
            let _owned = owned;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        })
    });
    let retirement = task.retirement_handle().unwrap();
    let observer = retirement.clone();
    entry.recv_timeout(DEADLINE).unwrap();
    let output = task.poll_with_timeout(DEADLINE).unwrap();
    drop(task);
    assert!(!observer.is_finished());
    retirement.retire(output);
    assert!(observer.is_finished());
    assert!(weak.upgrade().is_none());
}

#[test]
fn retirement_includes_cleanup_spawned_by_output_destructor() {
    pools();
    struct Cleanup {
        owned: Option<Arc<()>>,
        entered: Option<mpsc::Sender<()>>,
        gate: Option<mpsc::Receiver<()>>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let owned = self.owned.take().unwrap();
            let entered = self.entered.take().unwrap();
            let gate = self.gate.take().unwrap();
            compute::spawn_and_forget(move || {
                let _owned = owned;
                entered.send(()).unwrap();
                gate.recv_timeout(DEADLINE).unwrap();
            });
        }
    }
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let (entered, entry) = mpsc::channel();
    let (resume, gate) = mpsc::channel();
    let mut task = async_io::spawn_loader(async move {
        Cleanup {
            owned: Some(owned),
            entered: Some(entered),
            gate: Some(gate),
        }
    });
    let retirement = task.retirement_handle().unwrap();
    let observer = retirement.clone();
    let output = task.poll_with_timeout(DEADLINE).unwrap();
    drop(task);
    let (done, finished) = mpsc::channel();
    let join = std::thread::spawn(move || {
        retirement.retire(output);
        done.send(()).unwrap();
    });
    entry.recv_timeout(DEADLINE).unwrap();
    assert!(!observer.is_finished());
    assert!(weak.upgrade().is_some());
    assert!(finished.recv_timeout(Duration::from_millis(5)).is_err());
    resume.send(()).unwrap();
    finished.recv_timeout(DEADLINE).unwrap();
    join.join().unwrap();
    assert!(observer.is_finished());
    assert!(weak.upgrade().is_none());
}

#[test]
fn compute_loader_retires_queued_decoder_output() {
    compute_loader_output_retirement(false);
}

#[test]
fn compute_loader_retirement_follows_published_decoder_output() {
    compute_loader_output_retirement(true);
}

fn compute_loader_output_retirement(published: bool) {
    pools();
    let resource = Arc::new(());
    let weak = Arc::downgrade(&resource);
    let (started, entry) = mpsc::channel();
    let mut task = compute::spawn_loader(move || {
        let child = async_io::spawn(async move {
            let _resource = resource;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        child
    });
    let retirement = task.retirement_handle();
    let observer = retirement.clone();
    entry.recv_timeout(DEADLINE).unwrap();
    let deadline = std::time::Instant::now() + DEADLINE;
    // Establish an actual queued output without consuming its ownership.
    while !task.task.receiver.has_message() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!observer.is_finished());
    assert!(weak.upgrade().is_some());
    if published {
        let output = task.poll_naive().unwrap();
        drop(task);
        assert!(!observer.is_finished());
        retirement.retire(output);
    } else {
        task.cancel_and_retire();
    }
    assert!(observer.is_finished());
    assert!(weak.upgrade().is_none());
    assert_eq!(
        async_io::spawn(async { 42 }).poll_with_timeout(DEADLINE),
        Some(42)
    );
}
