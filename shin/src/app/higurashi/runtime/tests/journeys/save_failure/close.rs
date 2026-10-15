//! A desktop close request must resolve the retained manual transaction before
//! freezing its own write. Both failures here are actual atomic rename errors.
use super::*;

#[test]
#[ignore = "actual Runtime/manual IO recovery plus pending close/Return rollback/shutdown IO failure/Return to same menu;real assets/offscreen Metal/exact Loopback Audio;temporary Paths only"]
fn higurashi_runtime_close_waits_for_manual_recovery_before_freezing_progress() {
    let saved = initial();
    let initial_file = saved.encode().unwrap();
    let (mut j, storage) = fixture(&saved);
    let viewport = viewport((1280, 720));
    let deadline = Instant::now() + Duration::from_secs(150);
    let blank = RawInputState::new();
    j.boot_title(viewport, deadline);
    j.start_story(viewport, deadline);
    j.until(
        viewport,
        deadline,
        "authored input not ready",
        message_ready,
    );
    let calendar = Arc::new(AtomicUsize::new(0));
    let calls = calendar.clone();
    j.runtime
        .slot
        .as_mut()
        .unwrap()
        .game
        .as_mut()
        .unwrap()
        .set_clock_source(move || {
            assert_eq!(
                calls.fetch_add(1, Ordering::SeqCst),
                0,
                "pending close/recovery must not recapture calendar"
            );
            Ok(NOW)
        });
    let source = SourceState::new(&j.runtime);
    let before = rows(&j.runtime);
    j.tap(KeyCode::KeyM, viewport);
    j.until(viewport, deadline, "SystemMenu Save not ready", |r| {
        game(r).host().unwrap().reference_menu_control() == Some((0, true))
    });
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "manual Save Input not ready",
        save_ready,
    );
    for selected in 1..=TARGET {
        j.tap(KeyCode::ArrowDown, viewport);
        j.until(viewport, deadline, "manual scroll not idle", save_ready);
        assert_eq!(
            game(&j.runtime)
                .reference_save_menu()
                .unwrap()
                .state()
                .selected,
            selected as i32
        );
    }
    confirm(&mut j, viewport, deadline, false);
    wait_failure(&mut j, &storage, viewport, deadline);
    let manual_retirements = game(&j.runtime)
        .reference_save_menu()
        .unwrap()
        .reference_save_retirements();
    let mut pending = before.clone();
    pending.manual[TARGET].valid = true;
    pending.manual[TARGET].data = saved_row(&source.current, NOW.second);
    same_rows(&rows(&j.runtime), &pending);
    assert_eq!(attempts(&storage).len(), 1);
    original_file(&j, &initial_file);

    j.runtime.request_close().unwrap();
    assert!(j.runtime.close_active());
    assert!(!j.runtime.freezes_scene());
    // The first pending-close callback consumes the close-boundary input flush.
    assert!(matches!(j.step(&blank, viewport), Frame::Advanced(_)));
    assert!(j.runtime.shutdown.requested());
    assert!(j.runtime.has_save_task());
    assert_eq!(failure(&j.runtime), Some((2, 0, true)));
    assert_eq!(
        attempts(&storage).len(),
        1,
        "close bypassed unresolved manual recovery"
    );
    assert!(j.runtime.shutdown.writer_retirement().is_none());
    for (code, bit) in [(KeyCode::Enter, 24), (KeyCode::Escape, 25)] {
        assert!(matches!(j.step(&blank, viewport), Frame::Advanced(_)));
        assert!(matches!(
            j.gpu.update_with_permission(
                &mut j.runtime,
                &key(code),
                viewport,
                Duration::from_millis(50),
                false,
            ),
            Frame::Advanced(_)
        ));
        assert_ne!(j.runtime.reference_sampled_buttons.pressed & (1 << bit), 0);
        assert_eq!(
            j.runtime.input.buttons().pressed,
            0,
            "Gameplay quarantines denied input at callback end"
        );
        assert_eq!(failure(&j.runtime), Some((2, 0, true)));
        assert!(matches!(j.step(&key(code), viewport), Frame::Advanced(_)));
        assert_eq!(j.runtime.reference_sampled_buttons.pressed & (1 << bit), 0);
        assert_eq!(failure(&j.runtime), Some((2, 0, true)));
        assert_eq!(attempts(&storage).len(), 1);
        same_rows(&rows(&j.runtime), &pending);
        source.check(&j.runtime);
        assert!(matches!(j.step(&blank, viewport), Frame::Advanced(_)));
    }

    // Fresh B reaches the actual modal while close is requested. Its accepted
    // close continues with outer permission denied; no lower menu handles B.
    assert!(matches!(
        j.step(&key(KeyCode::Escape), viewport),
        Frame::Advanced(_)
    ));
    assert_eq!(failure(&j.runtime), Some((3, -1, true)));
    while failure(&j.runtime).is_some() {
        assert!(
            Instant::now() < deadline,
            "manual Return stayed blocked by pending close"
        );
        assert!(matches!(
            j.gpu.update_with_permission(
                &mut j.runtime,
                &blank,
                viewport,
                Duration::from_millis(50),
                false,
            ),
            Frame::Advanced(_)
        ));
        assert_eq!(attempts(&storage).len(), 1);
        source.check(&j.runtime);
    }
    assert!(
        j.runtime.shutdown.requested(),
        "rollback precedes close snapshot callback"
    );
    assert!(!j.runtime.has_save_task());
    same_rows(&rows(&j.runtime), &before);
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .save_menu_position,
        saved.save_menu_position
    );
    assert!(
        manual_retirements
            .iter()
            .all(shin_tasks::LoaderRetirement::is_finished)
    );
    let frozen_clock = j.runtime.globals().unwrap().unwrap().clock.ticks();

    // The next callback is the separate desktop shutdown writer. Its frozen
    // snapshot must contain the rolled-back row, never the failed provisional row.
    while j.runtime.shutdown.failure_phase() != Some(2) {
        assert!(
            Instant::now() < deadline,
            "actual shutdown IO failure did not become ready"
        );
        assert_eq!(j.step(&blank, viewport), Frame::Closing);
        assert_eq!(
            j.runtime.globals().unwrap().unwrap().clock.ticks(),
            frozen_clock
        );
        same_rows(&rows(&j.runtime), &before);
        assert_eq!(
            game(&j.runtime).host().unwrap().runner.completed_commands() as u64,
            source.commands
        );
    }
    let writes = attempts(&storage);
    assert_eq!(
        writes.len(),
        2,
        "one failed manual attempt and one separate shutdown attempt"
    );
    let shutdown = SaveData::decode(&writes[1]).unwrap();
    assert_eq!(shutdown.manual_save_slots, saved.manual_save_slots);
    assert_eq!(shutdown.save_menu_position, saved.save_menu_position);
    assert!(j.storage.writes.lock().unwrap().is_empty());
    original_file(&j, &initial_file);
    assert_eq!(calendar.load(Ordering::SeqCst), 1);

    // Return from shutdown's real Question restores the same still-owned Save
    // menu. It must neither retry either writer nor acknowledge a failed save.
    j.step(&blank, viewport);
    assert_eq!(j.step(&key(KeyCode::Escape), viewport), Frame::Closing);
    while j.runtime.close_active() {
        assert!(
            Instant::now() < deadline,
            "shutdown Return did not release close ownership"
        );
        assert_eq!(j.step(&blank, viewport), Frame::Closing);
        assert_eq!(
            j.runtime.globals().unwrap().unwrap().clock.ticks(),
            frozen_clock
        );
        assert_eq!(attempts(&storage).len(), 2);
    }
    j.until(
        viewport,
        deadline,
        "same manual Save menu did not resume after close Return",
        save_ready,
    );
    assert_eq!(
        game(&j.runtime)
            .reference_save_menu()
            .unwrap()
            .state()
            .selected,
        TARGET as i32
    );
    assert!(
        game(&j.runtime)
            .reference_save_menu()
            .unwrap()
            .reference_save_retirements()
            .is_empty()
    );
    source.check(&j.runtime);
    same_rows(&rows(&j.runtime), &before);
    assert!(failure(&j.runtime).is_none());
    assert_eq!(attempts(&storage).len(), 2);
    assert_eq!(calendar.load(Ordering::SeqCst), 1);
    original_file(&j, &initial_file);
    assert!(j.storage.writes.lock().unwrap().is_empty());
    pure_failure_frame(&mut j);
    println!(
        "Runtime close waits for actual failed manual Save recovery;denied+held A/B do not choose,fresh B/accepted denied close rolls back all101rows before shutdown snapshot;real shutdown IO failure B returns same authored Save menu/source;2 failed attempts/0 successful writes/1 calendar call;offscreen720p/exact Loopback,temporary Paths only"
    );
}
