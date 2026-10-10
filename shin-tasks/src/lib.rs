use std::{fmt, fmt::Debug, io, num::NonZeroUsize};

use futures_lite::FutureExt as _;
use tracing::debug;
use wasm_thread as thread;

mod retirement;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod retirement_tests;

pub struct AsyncTask<T> {
    task: async_task::Task<T>,
    scope: Option<std::sync::Arc<retirement::Scope>>,
}

/// Tracks loader work after its completed output has left the task.
/// Dropping this handle does not join resources; retire their owner explicitly
/// when discarding a result or releasing a published decoder.
#[derive(Clone)]
pub struct LoaderRetirement {
    scope: std::sync::Arc<retirement::Scope>,
}

impl LoaderRetirement {
    /// Whether all registered work has retired. This does not cancel work.
    pub fn is_finished(&self) -> bool {
        self.scope.is_finished()
    }

    /// Drop an owner, including any cleanup work its drop schedules,
    /// and wait for registered work to retire. Call outside executor polls/workers.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retire<T>(self, owner: T) {
        assert!(
            retirement::current().is_none(),
            "Retire a loader owner from its caller"
        );
        {
            let _work = self.scope.register();
            let _entered = self.scope.enter();
            drop(owner);
        }
        self.scope.wait();
    }
}

impl<T> AsyncTask<T> {
    pub fn retirement_handle(&self) -> Option<LoaderRetirement> {
        self.scope.as_ref().map(|scope| LoaderRetirement {
            scope: scope.clone(),
        })
    }
    pub fn detach(self) {
        self.task.detach()
    }

    /// Cancel and wait until the future and its owned fields are retired.
    /// Dropping a task requests cancellation but can leave an active poll alive.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_and_wait(self) -> Option<T> {
        futures_lite::future::block_on(self.task.cancel())
    }

    /// Discard a loader's result, then drain its registered child work.
    /// The caller must be outside an executor poll/worker. In particular, the
    /// I/O executor remains free while compute jobs await decoder input/output.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_and_retire(self) {
        let retirement = self.retirement_handle();
        let result = futures_lite::future::block_on(self.task.cancel());
        if let Some(retirement) = retirement {
            retirement.retire(result);
        } else {
            drop(result);
        }
    }

    /// Wait for readiness without canceling a task that misses the deadline.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll_with_timeout(&mut self, timeout: std::time::Duration) -> Option<T> {
        struct ThreadWake(std::thread::Thread);
        impl std::task::Wake for ThreadWake {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = std::task::Waker::from(std::sync::Arc::new(ThreadWake(std::thread::current())));
        let started = std::time::Instant::now();
        loop {
            if let std::task::Poll::Ready(result) =
                self.task.poll(&mut std::task::Context::from_waker(&waker))
            {
                return Some(result);
            }
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return None;
            };
            if remaining.is_zero() {
                return None;
            }
            std::thread::park_timeout(remaining);
        }
    }
    // Browser main threads cannot park; retain asynchronous publication there.
    #[cfg(target_arch = "wasm32")]
    pub fn poll_with_timeout(&mut self, _timeout: std::time::Duration) -> Option<T> {
        self.poll_naive()
    }

    pub fn poll_naive(&mut self) -> Option<T> {
        // this is slightly inefficient as it will end up registering the waker, even though we don't need it
        // however, we would need to write our own task primitive if we want different behavior
        match self
            .task
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        {
            std::task::Poll::Ready(result) => Some(result),
            std::task::Poll::Pending => None,
        }
    }
}

impl<T> Future for AsyncTask<T> {
    type Output = T;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.get_mut().task.poll(cx)
    }
}

impl<T> Debug for AsyncTask<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.task.fmt(f)
    }
}

pub struct ComputeTask<T> {
    receiver: oneshot::Receiver<T>,
}

/// A compute producer whose owned child work can be drained by its caller.
/// Ordinary drop only closes the receiver; it never joins on an executor.
pub struct LoaderComputeTask<T> {
    task: ComputeTask<T>,
    retirement: LoaderRetirement,
}

impl<T> LoaderComputeTask<T> {
    /// Observe a queued output without consuming or changing its ownership.
    #[cfg(any(test, feature = "reference-tests"))]
    pub fn reference_result_is_ready(&self) -> bool {
        self.task.receiver.has_message()
    }

    pub fn poll_naive(&mut self) -> Option<T> {
        self.task.poll_naive()
    }

    pub fn retirement_handle(&self) -> LoaderRetirement {
        self.retirement.clone()
    }

    /// Close the result receiver, then wait for the producer and its children.
    /// Unlike an async future, a running compute closure cannot be cancelled.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_and_retire(self) {
        self.retirement.retire(self.task);
    }
}

impl<T> Future for LoaderComputeTask<T> {
    type Output = T;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<T> {
        std::pin::Pin::new(&mut self.get_mut().task).poll(cx)
    }
}

impl<T> ComputeTask<T> {
    pub fn poll_naive(&mut self) -> Option<T> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(oneshot::TryRecvError::Empty) => None,
            Err(oneshot::TryRecvError::Disconnected) => {
                panic!("Either a completed task was polled again, or the sender was dropped");
            }
        }
    }
}

impl<T> Future for ComputeTask<T> {
    type Output = T;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.get_mut().receiver.poll(cx) {
            std::task::Poll::Ready(Ok(result)) => std::task::Poll::Ready(result),
            std::task::Poll::Ready(Err(_)) => {
                panic!("Either a completed task was polled again, or the sender was dropped");
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

fn available_parallelism() -> usize {
    thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
}

pub fn create_task_pools() {
    let total_threads = available_parallelism();

    create_rayon_pool(total_threads);

    create_async_io_pool(1);
}

fn create_rayon_pool(threads: usize) {
    debug!("Creating rayon thread pool with {} threads", threads);

    rayon_core::ThreadPoolBuilder::new()
        .num_threads(threads)
        // spawn rayon threads using wasm_thread
        .spawn_handler(|thread| -> io::Result<()> {
            let mut b = thread::Builder::new();
            if let Some(name) = thread.name() {
                b = b.name(name.to_owned());
            }
            if let Some(stack_size) = thread.stack_size() {
                b = b.stack_size(stack_size);
            }
            b.spawn(|| thread.run())?;
            Ok(())
        })
        .build_global()
        .expect("Failed to build global rayon thread pool");
}

fn create_async_io_pool(threads: usize) {
    async_io::EXECUTOR
        .set(async_executor::Executor::new())
        .unwrap();

    let executor = async_io::EXECUTOR.get().unwrap();

    for _ in 0..threads {
        thread::Builder::new()
            .name("async-io".to_owned())
            .spawn(|| {
                futures_lite::future::block_on(executor.run(std::future::pending::<()>()));
            })
            .expect("Failed to spawn async-io thread");
    }
}

pub mod compute {
    use std::sync::Arc;

    use super::{ComputeTask, LoaderComputeTask, LoaderRetirement};

    pub fn spawn<T, F>(f: F) -> ComputeTask<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        spawn_with_scope(f, super::retirement::current())
    }

    /// Start a compute loader with its own producer/child retirement scope.
    pub fn spawn_loader<T, F>(f: F) -> LoaderComputeTask<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let scope = Arc::new(super::retirement::Scope::default());
        LoaderComputeTask {
            task: spawn_with_scope(f, Some(scope.clone())),
            retirement: LoaderRetirement { scope },
        }
    }

    fn spawn_with_scope<T, F>(f: F, scope: Option<Arc<super::retirement::Scope>>) -> ComputeTask<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let work = scope.as_ref().map(|scope| scope.register());

        rayon_core::spawn(move || {
            let _work = work;
            let _entered = scope.as_ref().map(|scope| scope.enter());
            // A cancelled awaiter rejects the output. Retire that output before
            // releasing the work guard, including any owned decoder tasks.
            let _ = sender.send(f());
        });

        ComputeTask { receiver }
    }

    pub fn spawn_and_forget<F>(f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let scope = super::retirement::current();
        let work = scope.as_ref().map(|scope| scope.register());
        rayon_core::spawn(move || {
            let _work = work;
            let _entered = scope.as_ref().map(|scope| scope.enter());
            f();
        });
    }
}

pub mod async_io {
    use std::sync::{Arc, OnceLock};

    use super::AsyncTask;

    pub(crate) static EXECUTOR: OnceLock<async_executor::Executor<'static>> = OnceLock::new();

    pub fn spawn<T, F>(f: F) -> AsyncTask<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        spawn_with_scope(f, super::retirement::current())
    }

    /// Start a loader whose child async/compute work can be drained on retirement.
    pub fn spawn_loader<T, F>(f: F) -> AsyncTask<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        spawn_with_scope(f, Some(Arc::new(super::retirement::Scope::default())))
    }

    fn spawn_with_scope<T, F>(f: F, scope: Option<Arc<super::retirement::Scope>>) -> AsyncTask<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let executor = EXECUTOR.get().expect("async-io executor not initialized");
        let task = if let Some(scope) = &scope {
            executor.spawn(super::retirement::ScopedFuture::new(f, scope.clone()))
        } else {
            executor.spawn(f)
        };

        AsyncTask { task, scope }
    }
}

/// Blocks the current thread on a future.
///
/// # Examples
///
/// ```
/// let val = shin_tasks::block_on(async {
///     1 + 2
/// });
///
/// assert_eq!(val, 3);
/// ```
pub fn block_on<F: Future>(future: F) -> F::Output {
    futures_lite::future::block_on(future)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod timeout_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn timeout_preserves_pending_task_and_completion_wakes_waiter() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (runnable, task) = async_task::spawn(
            async move { receiver.recv().unwrap() },
            |runnable: async_task::Runnable| {
                std::thread::spawn(move || runnable.run());
            },
        );
        runnable.schedule();
        let mut task = AsyncTask { task, scope: None };
        let start = Instant::now();
        assert_eq!(task.poll_with_timeout(Duration::from_millis(5)), None);
        assert!(start.elapsed() >= Duration::from_millis(5));
        // A timeout leaves ownership intact; the same task publishes later.
        sender.send(42).unwrap();
        assert_eq!(task.poll_with_timeout(Duration::from_secs(1)), Some(42));
    }

    #[test]
    fn ready_task_is_polled_even_with_zero_wait_budget() {
        let (runnable, task) = async_task::spawn(async { 7 }, |runnable: async_task::Runnable| {
            runnable.run();
        });
        runnable.run();
        let mut task = AsyncTask { task, scope: None };
        assert_eq!(task.poll_with_timeout(Duration::ZERO), Some(7));
    }

    #[test]
    fn cancellation_waits_for_active_poll_and_retires_its_resources() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct Owned(Arc<AtomicBool>);
        impl Drop for Owned {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let retired = Arc::new(AtomicBool::new(false));
        let owned = Owned(retired.clone());
        let (entered, entry) = std::sync::mpsc::channel();
        let (resume, gate) = std::sync::mpsc::channel();
        let (runnable, task) = async_task::spawn(
            async move {
                let _owned = owned;
                entered.send(()).unwrap();
                gate.recv().unwrap();
                std::future::pending::<()>().await;
            },
            |runnable: async_task::Runnable| {
                std::thread::spawn(move || runnable.run());
            },
        );
        runnable.schedule();
        entry.recv_timeout(Duration::from_secs(1)).unwrap();
        let (finished, completion) = std::sync::mpsc::channel();
        let canceler = std::thread::spawn(move || {
            assert!(AsyncTask { task, scope: None }.cancel_and_wait().is_none());
            finished.send(()).unwrap();
        });
        assert!(completion.recv_timeout(Duration::from_millis(5)).is_err());
        assert!(!retired.load(Ordering::Acquire));
        resume.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(retired.load(Ordering::Acquire));
        canceler.join().unwrap();
    }

    #[test]
    fn cancellation_retains_output_completed_before_request() {
        let (runnable, task) =
            async_task::spawn(async { vec![42] }, |runnable: async_task::Runnable| {
                runnable.run();
            });
        runnable.run();
        assert_eq!(
            AsyncTask { task, scope: None }.cancel_and_wait(),
            Some(vec![42])
        );
    }
}
