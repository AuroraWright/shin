//! Background geometry publication; metadata is complete before this worker starts.
use super::model::history::{Geometry, History, Measurement};
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

type Measure = dyn Fn(usize) -> Result<(f32, u8)> + Send + Sync;
struct State {
    measurement: Measurement,
    finished: bool,
    error: Option<String>,
}
/// Compute only owns immutable metadata/metrics and the shared accumulator.
/// No worker borrows the live controller, glyph cache or GPU resources.
pub(super) struct Worker {
    state: Arc<Mutex<State>>,
    measure: Arc<Measure>,
    task: Option<shin_tasks::ComputeTask<()>>,
    cancelled: Arc<AtomicBool>,
    restart: bool,
    revision: usize,
    #[cfg(test)]
    gate: Option<Arc<Gate>>,
}
impl Worker {
    pub async fn start(
        history: &History,
        cancelled: Arc<AtomicBool>,
        measure: Arc<Measure>,
        #[cfg(test)] gate: Option<Arc<Gate>>,
    ) -> Result<Self> {
        let measurement = Measurement::new(&history.entries, history.current_row)
            .ok_or_else(|| anyhow!("Ger measurement requires nonempty history"))?;
        let mut worker = Self {
            state: Arc::new(Mutex::new(State {
                measurement,
                finished: false,
                error: None,
            })),
            measure,
            task: None,
            cancelled,
            restart: false,
            revision: 0,
            #[cfg(test)]
            gate,
        };
        let (send, ready) = oneshot::channel();
        worker.spawn(Some(send));
        ready
            .await
            .map_err(|_| anyhow!("Ger measurement lost readiness"))?
            .map_err(anyhow::Error::msg)?;
        Ok(worker)
    }
    fn spawn(&mut self, mut ready: Option<oneshot::Sender<Result<(), String>>>) {
        let shared = self.state.clone();
        let cancelled = self.cancelled.clone();
        let measure = self.measure.clone();
        #[cfg(test)]
        let gate = self.gate.clone();
        self.state.lock().finished = false;
        // During factory preparation this inherits its loader's retirement
        // scope. A published host explicitly drains its compute handle on Drop.
        self.task = Some(shin_tasks::compute::spawn(move || {
            let result = (|| -> Result<()> {
                loop {
                    #[cfg(test)]
                    if let Some(gate) = gate.as_ref() {
                        let count = shared.lock().measurement.published_count();
                        gate.wait(count, &cancelled);
                    }
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let row = shared.lock().measurement.next_row();
                    let Some(row) = row else { break };
                    let (height, voices) = measure(row)?;
                    let is_ready = {
                        let mut state = shared.lock();
                        state
                            .measurement
                            .publish(row, height, voices)
                            .map_err(anyhow::Error::msg)?;
                        state.measurement.ready(false)
                    };
                    if is_ready && let Some(send) = ready.take() {
                        let _ = send.send(Ok(()));
                    }
                }
                Ok(())
            })();
            let mut state = shared.lock();
            state.finished = true;
            if let Err(error) = result {
                // Cooperative cancellation preserves published geometry. Other
                // failures remain errors even when a later owner cancels.
                state.error = Some(format!("{error:#}"));
            }
            if let Some(send) = ready.take() {
                let result = if let Some(error) = state.error.as_ref() {
                    Err(error.clone())
                } else if cancelled.load(Ordering::Acquire) {
                    Err("Ger measurement cancelled before publication".into())
                } else {
                    Ok(())
                };
                let _ = send.send(result);
            }
        }));
    }
    pub fn snapshot(&mut self) -> Result<Option<Geometry>> {
        if self.task.as_mut().is_some_and(|t| t.poll_naive().is_some()) {
            self.task = None;
        }
        if self.restart && self.task.is_none() {
            self.restart = false;
            let state = self.state.lock();
            let resume = !state.measurement.complete() && state.error.is_none();
            drop(state);
            if resume {
                self.cancelled = Arc::new(AtomicBool::new(false));
                self.spawn(None);
            }
        }
        let state = self.state.lock();
        if let Some(error) = state.error.as_ref() {
            return Err(anyhow!("Ger background measurement failed: {error}"));
        }
        let count = state.measurement.published_count();
        if count == self.revision {
            return Ok(None);
        }
        self.revision = count;
        Ok(Some(state.measurement.geometry()))
    }
    pub fn cancel(&mut self) {
        self.restart = false;
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn resume(&mut self) {
        // Do not clear an old token: the old closure may still be in layout.
        self.restart = true;
    }
    pub fn cancel_and_wait(&mut self) {
        self.cancel();
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            shin_tasks::block_on(task);
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
    #[cfg(test)]
    pub fn reference(&self) -> (usize, bool, bool) {
        let state = self.state.lock();
        (
            state.measurement.published_count(),
            !state.finished,
            self.cancelled.load(Ordering::Acquire),
        )
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Pending factory Drop can run inside the loader executor. Its caller
        // drains inherited compute work; Arc retains rows until it has stopped.
        self.cancel();
    }
}

#[cfg(test)]
mod tests;

/// Per-worker deterministic scheduling boundary, absent from production builds.
#[cfg(test)]
pub(in crate::app::gerokasu2) struct Gate {
    limit: std::sync::atomic::AtomicUsize,
    blocked: AtomicBool,
    mutex: Mutex<()>,
    wake: parking_lot::Condvar,
}
#[cfg(test)]
impl Gate {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit: std::sync::atomic::AtomicUsize::new(limit),
            blocked: AtomicBool::new(false),
            mutex: Mutex::new(()),
            wake: parking_lot::Condvar::new(),
        })
    }
    pub fn advance(&self, limit: usize) {
        let _guard = self.mutex.lock();
        self.limit.store(limit, Ordering::Release);
        self.wake.notify_all();
    }
    pub fn blocked(&self) -> bool {
        self.blocked.load(Ordering::Acquire)
    }
    fn wait(&self, count: usize, cancelled: &AtomicBool) {
        let mut guard = self.mutex.lock();
        while count >= self.limit.load(Ordering::Acquire) && !cancelled.load(Ordering::Acquire) {
            self.blocked.store(true, Ordering::Release);
            self.wake
                .wait_for(&mut guard, std::time::Duration::from_millis(5));
        }
        self.blocked.store(false, Ordering::Release);
    }
}
