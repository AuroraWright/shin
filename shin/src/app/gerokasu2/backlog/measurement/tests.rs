use super::*;
use crate::adv::gerokasu2::backlog::history::{Entry, Item};
use shin_core::format::save::gerokasu2::{GameData, Progress};
use std::time::{Duration, Instant};

fn history(count: usize) -> History {
    History {
        entries: (0..count)
            .map(|i| Entry {
                boundary: i as u32 + 1,
                item: Item::Message {
                    message_id: i as u32,
                    message_arg: -1,
                    text: "@r本文".into(),
                },
            })
            .collect(),
        chapters: vec![],
        current_row: Some(count / 2),
        jump_source: GameData {
            date: Default::default(),
            progress: Progress::fresh(0, 42),
        },
    }
}
fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "measurement test did not reach its gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn worker(count: usize, gate: Arc<Gate>) -> Worker {
    crate::init_test_task_pools();
    shin_tasks::block_on(Worker::start(
        &history(count),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| Ok((54.125, 0))),
        Some(gate),
    ))
    .unwrap()
}

#[test]
fn ger_measurement_opens_at_100_and_grows_without_moving_published_rows() {
    let gate = Gate::new(100);
    let mut worker = worker(300, gate.clone());
    wait(|| gate.blocked());
    assert_eq!(worker.reference(), (100, true, false));
    let first = worker.snapshot().unwrap().unwrap();
    assert_eq!(first.allowed, 100..200);
    assert_eq!(first.rows[150].top, 0.);
    assert_eq!(first.rows[99].height, 0.);
    gate.advance(150);
    wait(|| worker.reference().0 == 150 && gate.blocked());
    let next = worker.snapshot().unwrap().unwrap();
    assert_eq!(next.allowed, 75..225);
    for i in first.allowed {
        assert_eq!(first.rows[i], next.rows[i]);
    }
    assert!(worker.snapshot().unwrap().is_none());
    gate.advance(usize::MAX);
    wait(|| !worker.reference().1);
    let final_geometry = worker.snapshot().unwrap().unwrap();
    assert_eq!(final_geometry.allowed, 0..300);
    worker.cancel_and_wait();
}

#[test]
fn ger_measurement_cancelled_factory_retires_before_readiness() {
    crate::init_test_task_pools();
    let gate = Gate::new(99);
    let token = Arc::new(AtomicBool::new(false));
    let cancelled = token.clone();
    let worker_gate = gate.clone();
    let task = shin_tasks::async_io::spawn_loader(async move {
        Worker::start(
            &history(300),
            cancelled,
            Arc::new(|_| Ok((54., 0))),
            Some(worker_gate),
        )
        .await
    });
    let retirement = task.retirement_handle().unwrap();
    wait(|| gate.blocked());
    token.store(true, Ordering::Release);
    let result = shin_tasks::block_on(task);
    assert!(result.is_err());
    retirement.retire(result);
    assert!(!gate.blocked());
}

#[test]
fn ger_measurement_cancel_then_recovery_uses_fresh_token_and_retained_geometry() {
    let gate = Gate::new(100);
    let mut worker = worker(300, gate.clone());
    wait(|| gate.blocked());
    let before = worker.snapshot().unwrap().unwrap();
    let old_token = worker.cancelled.clone();
    worker.cancel_and_wait();
    assert_eq!(worker.reference(), (100, false, true));
    worker.resume();
    gate.advance(150);
    worker.snapshot().unwrap();
    wait(|| worker.reference().0 == 150 && gate.blocked());
    assert!(old_token.load(Ordering::Acquire));
    assert!(!Arc::ptr_eq(&old_token, &worker.cancelled));
    assert_eq!(worker.reference(), (150, true, false));
    let after = worker.snapshot().unwrap().unwrap();
    for i in before.allowed {
        assert_eq!(before.rows[i], after.rows[i]);
    }
    worker.cancel_and_wait();
}

#[test]
fn ger_measurement_cancellation_publishes_inflight_layout_before_stopping() {
    crate::init_test_task_pools();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (inside, resume, layouts) = (entered.clone(), release.clone(), calls.clone());
    let mut worker = shin_tasks::block_on(Worker::start(
        &history(300),
        Arc::new(AtomicBool::new(false)),
        Arc::new(move |_| {
            if layouts.fetch_add(1, Ordering::AcqRel) == 100 {
                inside.store(true, Ordering::Release);
                wait(|| resume.load(Ordering::Acquire));
            }
            Ok((54., 0))
        }),
        None,
    ))
    .unwrap();
    wait(|| entered.load(Ordering::Acquire));
    worker.cancel();
    release.store(true, Ordering::Release);
    worker.cancel_and_wait();
    assert_eq!(worker.reference(), (101, false, true));
    assert_eq!(calls.load(Ordering::Acquire), 101);
}

#[test]
fn ger_measurement_background_failure_is_reported_after_initial_publication() {
    let gate = Gate::new(100);
    crate::init_test_task_pools();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let mut worker = shin_tasks::block_on(Worker::start(
        &history(300),
        Arc::new(AtomicBool::new(false)),
        Arc::new(move |_| {
            anyhow::ensure!(
                calls.fetch_add(1, Ordering::AcqRel) < 100,
                "injected layout error"
            );
            Ok((54., 0))
        }),
        Some(gate.clone()),
    ))
    .unwrap();
    assert!(worker.snapshot().unwrap().is_some());
    gate.advance(usize::MAX);
    wait(|| !worker.reference().1);
    assert!(
        worker
            .snapshot()
            .unwrap_err()
            .to_string()
            .contains("injected layout error")
    );
    worker.cancel_and_wait();
}
