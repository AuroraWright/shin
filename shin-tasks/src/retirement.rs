//! Loader-owned work is drained by its caller, never by an executor-side drop.
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex},
    task::{Context, Poll},
};

thread_local! {
    static CURRENT: RefCell<Option<Arc<Scope>>> = const { RefCell::new(None) };
}

#[derive(Default)]
pub(crate) struct Scope {
    pending: Mutex<usize>,
    empty: Condvar,
}

pub(crate) fn current() -> Option<Arc<Scope>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

pub(crate) struct Enter(Option<Arc<Scope>>);
impl Drop for Enter {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

impl Scope {
    pub(crate) fn is_finished(&self) -> bool {
        *self.pending.lock().unwrap() == 0
    }
    pub(crate) fn enter(self: &Arc<Self>) -> Enter {
        Enter(CURRENT.with(|slot| slot.replace(Some(self.clone()))))
    }

    pub(crate) fn register(self: &Arc<Self>) -> Work {
        let mut count = self.pending.lock().unwrap();
        *count = count.checked_add(1).expect("Loader work count overflow");
        Work(self.clone())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn wait(&self) {
        assert!(
            current().is_none(),
            "Drain loader work from its caller, outside an executor poll/worker"
        );
        let mut count = self.pending.lock().unwrap();
        while *count != 0 {
            count = self.empty.wait(count).unwrap();
        }
    }
}

pub(crate) struct Work(Arc<Scope>);
impl Drop for Work {
    fn drop(&mut self) {
        let mut count = self.0.pending.lock().unwrap();
        *count = count.checked_sub(1).expect("Unregistered loader work");
        if *count == 0 {
            self.0.empty.notify_all();
        }
    }
}

pub(crate) struct ScopedFuture<F> {
    future: Option<Pin<Box<F>>>,
    scope: Arc<Scope>,
    work: Option<Work>,
}

impl<F> ScopedFuture<F> {
    pub(crate) fn new(future: F, scope: Arc<Scope>) -> Self {
        let work = scope.register();
        Self {
            future: Some(Box::pin(future)),
            scope,
            work: Some(work),
        }
    }
}

impl<F: Future> Future for ScopedFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let _entered = this.scope.enter();
        this.future.as_mut().unwrap().as_mut().poll(cx)
    }
}

impl<F> Drop for ScopedFuture<F> {
    fn drop(&mut self) {
        // Drops can schedule further cleanup. Keep their scope active and the
        // current work registered until all owned future fields are destroyed.
        let _entered = self.scope.enter();
        drop(self.future.take());
        drop(self.work.take());
    }
}
