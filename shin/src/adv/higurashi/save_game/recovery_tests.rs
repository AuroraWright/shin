//! Actual SaveGame/Question owners with desktop policy; no AudioManager,
//! window, native execution or user progress path is involved.
use super::*;
use enum_map::{Enum, EnumMap};
use shin_core::format::save::higurashi::SaveData;
use shin_input::{ActionsState, RawInputState, inputs::GamepadButton};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use winit::keyboard::KeyCode;

struct Temporary(PathBuf);
impl Temporary {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "higurashi-save-game-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[derive(Clone, Copy, Enum)]
enum Face {
    A,
    B,
}
struct Driver {
    actions: ActionsState<Face>,
    notifications: usize,
}
impl Driver {
    fn new() -> Self {
        Self {
            actions: ActionsState::new(),
            notifications: 0,
        }
    }
    fn step(
        &mut self,
        owner: &mut SaveGame,
        key: Option<KeyCode>,
        permission: bool,
    ) -> Result<Buttons> {
        let mut raw = RawInputState::new();
        if let Some(key) = key {
            raw.keyboard.insert(key);
        }
        // Reuse the approved keyboard bridge once. The component needs only
        // A/B; ActionsState supplies genuine fresh and held snapshots, while
        // Runtime's complete lowering is checked by its separate journey.
        let raw = crate::app::keyboard_as_controller(&raw);
        let states = self.actions.update(
            EnumMap::from_fn(|face| {
                raw.gamepads.is_held(match face {
                    Face::A => GamepadButton::A,
                    Face::B => GamepadButton::B,
                })
            }),
            Duration::from_millis(50),
        );
        let mut buttons = Buttons::default();
        for (face, mask) in [
            (Face::A, (1 << 13) | (1 << 24)),
            (Face::B, (1 << 14) | (1 << 25)),
        ] {
            let state = states[face];
            if state.is_held {
                buttons.held |= mask;
            }
            if state.is_clicked {
                buttons.pressed |= mask;
            }
            if state.is_clicked_or_repeated {
                buttons.repeat |= mask;
            }
            if state.is_clicked_or_rapid_repeated {
                buttons.rapid_repeat |= mask;
            }
        }
        let result = owner.poll(Ticks::from_f32(3.0), permission, buttons, None);
        self.notifications += usize::from(owner.take_notification());
        result.map(|_| buttons)
    }
    fn tap(&mut self, owner: &mut SaveGame, key: KeyCode) {
        self.step(owner, None, true).unwrap();
        self.step(owner, Some(key), true).unwrap();
        self.step(owner, None, true).unwrap();
    }
    fn recovery(&mut self, owner: &mut SaveGame, deadline: Instant) {
        while owner.reference_recovery() != Some((2, 0, false)) {
            assert!(
                Instant::now() < deadline,
                "RetryOnly Question never became ready"
            );
            self.step(owner, None, true).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!owner.complete());
        assert_eq!(self.notifications, 0, "an IO failure is not native success");
        let handles = owner.reference_retirements();
        assert!(!handles.is_empty());
        // Question publication need not wait for its asynchronously requested
        // glyph descendants. Observe their actual retirement independently.
        while !handles.iter().all(|handle| handle.is_finished()) {
            assert!(
                Instant::now() < deadline,
                "published Question descendants did not finish"
            );
            self.step(owner, None, true).unwrap();
            assert_eq!(owner.reference_recovery(), Some((2, 0, false)));
            assert_eq!(self.notifications, 0);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(handles.iter().all(|handle| handle.is_finished()));
    }
}

#[test]
#[ignore = "actual SaveGame/real Question assets/typed atomic IO retry/RetryOnly denied edges;headless Metal AssetServer;no audio/window/user saves"]
fn higurashi_save_game_retry_only_reuses_immutable_write_and_notifies_success_once() {
    let (_device, _queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let temporary = Temporary::new("retry-only");
    let current = temporary.0.join("progress.dat");
    let blocked = temporary.0.join("blocked-destination");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("marker"), b"retained directory").unwrap();
    let original = b"old progress remains byte-identical";
    std::fs::write(&current, original).unwrap();
    let mut saved = SaveData::default();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    let frozen = Arc::new(saved.encode().unwrap());
    let fail = Arc::new(AtomicBool::new(true));
    let attempts = Arc::new(Mutex::new(Vec::<(usize, Vec<u8>)>::new()));
    let write: WriteSnapshot = {
        let fail = fail.clone();
        let attempts = attempts.clone();
        let frozen = frozen.clone();
        let current = current.clone();
        let blocked = blocked.clone();
        Arc::new(move || {
            attempts
                .lock()
                .unwrap()
                .push((Arc::as_ptr(&frozen) as usize, frozen.as_ref().clone()));
            crate::adv::persistence::write_atomic(
                if fail.load(Ordering::SeqCst) {
                    &blocked
                } else {
                    &current
                },
                frozen.as_slice(),
            )
            .map_err(|error| WriteIoFailure::new(error).into())
        })
    };
    let mut owner = SaveGame::new(Arc::new(server), write, 1, 91)
        .with_desktop_recovery(true, RecoveryPolicy::RetryOnly);
    let mut driver = Driver::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    driver.recovery(&mut owner, deadline);
    assert_eq!(attempts.lock().unwrap().len(), 1);
    assert_eq!(std::fs::read(&current).unwrap(), original);

    for (key, mask) in [(KeyCode::Enter, 1 << 24), (KeyCode::Escape, 1 << 25)] {
        driver.step(&mut owner, None, true).unwrap();
        let denied = driver.step(&mut owner, Some(key), false).unwrap();
        assert_ne!(denied.pressed & mask, 0);
        assert_eq!(owner.reference_recovery(), Some((2, 0, false)));
        let held = driver.step(&mut owner, Some(key), true).unwrap();
        assert_eq!(held.pressed & mask, 0);
        assert_eq!(owner.reference_recovery(), Some((2, 0, false)));
        assert!(!owner.complete());
        assert_eq!(attempts.lock().unwrap().len(), 1);
        driver.step(&mut owner, None, true).unwrap();
    }
    driver.tap(&mut owner, KeyCode::Escape);
    assert_eq!(
        owner.reference_recovery(),
        Some((2, 0, false)),
        "RetryOnly B cannot Return or fake completion"
    );
    assert_eq!(driver.notifications, 0);
    assert_eq!(attempts.lock().unwrap().len(), 1);
    driver.tap(&mut owner, KeyCode::Enter);
    driver.recovery(&mut owner, deadline); // The first Retry really fails again.
    assert_eq!(attempts.lock().unwrap().len(), 2);
    assert_eq!(std::fs::read(&current).unwrap(), original);
    fail.store(false, Ordering::SeqCst);
    driver.tap(&mut owner, KeyCode::Enter);
    while !owner.complete() {
        assert!(
            Instant::now() < deadline,
            "accepted Retry/native save notification never completed"
        );
        // Accepted animations and write completion continue under denied input.
        driver.step(&mut owner, None, false).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read(&current).unwrap(), frozen.as_slice());
    let attempts = attempts.lock().unwrap();
    assert_eq!(attempts.len(), 3);
    for (identity, bytes) in attempts.iter() {
        assert_eq!(
            *identity,
            Arc::as_ptr(&frozen) as usize,
            "same immutable closure payload owner"
        );
        assert_eq!(bytes, frozen.as_ref());
    }
    drop(attempts);
    assert_eq!(driver.notifications, 1);
    assert_eq!(
        owner.take_result(),
        None,
        "no false/Return result hidden behind success"
    );
    for _ in 0..8 {
        driver.step(&mut owner, None, true).unwrap();
    }
    assert_eq!(driver.notifications, 1);
    assert_eq!(owner.reference_recovery(), None);
    assert_eq!(
        std::fs::read(blocked.join("marker")).unwrap(),
        b"retained directory"
    );
    assert_eq!(owner.reference_retirements().len(), 0);
    drop(owner);
    let mut files = std::fs::read_dir(&temporary.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(
        files,
        [
            std::ffi::OsString::from("blocked-destination"),
            std::ffi::OsString::from("progress.dat")
        ]
    );
}

#[test]
#[ignore = "actual SaveGame/real Question assets/default typed IO and opted-in untyped IO remain fatal;headless Metal AssetServer;no audio/window/user saves"]
fn higurashi_save_game_default_and_untyped_errors_keep_native_fatal_behavior() {
    let (_device, _queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let server = Arc::new(server);
    let temporary = Temporary::new("fatal-boundaries");
    let blocked = temporary.0.join("blocked-destination");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("marker"), b"retained directory").unwrap();
    for (typed, opted_in) in [(true, false), (false, true)] {
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let bytes = Arc::new(SaveData::default().encode().unwrap());
        let write: WriteSnapshot = {
            let attempts = attempts.clone();
            let blocked = blocked.clone();
            let bytes = bytes.clone();
            Arc::new(move || {
                attempts.lock().unwrap().push(bytes.as_ref().clone());
                let result = crate::adv::persistence::write_atomic(&blocked, bytes.as_slice());
                assert!(result.as_ref().is_err_and(|error| {
                    error.chain().any(|cause| cause.is::<std::io::Error>())
                }));
                if typed {
                    result.map_err(|error| WriteIoFailure::new(error).into())
                } else {
                    result
                }
            })
        };
        let mut owner = SaveGame::new(server.clone(), write, 0, 92);
        if opted_in {
            owner = owner.with_desktop_recovery(true, RecoveryPolicy::RetryOnly);
        }
        let mut driver = Driver::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        let error = loop {
            assert!(
                Instant::now() < deadline,
                "fatal writer did not return its actual error"
            );
            if let Err(error) = driver.step(&mut owner, None, true) {
                break error;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(format!("{error:#}").contains("Replacing progress save"));
        assert_eq!(owner.reference_recovery(), None);
        assert!(owner.write_failed());
        assert!(!owner.complete());
        assert_eq!(driver.notifications, 0);
        assert_eq!(
            attempts.lock().unwrap().as_slice(),
            &[bytes.as_ref().clone()]
        );
        assert!(
            driver.step(&mut owner, None, true).is_err(),
            "fatal result cannot silently restart or succeed"
        );
        assert_eq!(attempts.lock().unwrap().len(), 1);
        assert_eq!(driver.notifications, 0);
        let retirements = owner.reference_retirements();
        assert!(!retirements.is_empty());
        assert!(retirements.iter().all(|handle| handle.is_finished()));
        drop(owner);
        assert!(retirements.iter().all(|handle| handle.is_finished()));
    }
    assert_eq!(
        std::fs::read(blocked.join("marker")).unwrap(),
        b"retained directory"
    );
}
