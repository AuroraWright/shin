//! Desktop IO recovery around actual Runtime/manual transactions. The native
//! component failure path stays fallible; this exercises Runtime's opt-in UI.
use super::*;
mod close;
use crate::app::higurashi::{
    gameplay::Gameplay, runtime_game_data::RuntimeGameData, runtime_saves::Slots,
};
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::sync::atomic::AtomicUsize;

const NOW: SaveDate = SaveDate {
    year: 2026,
    month: 10,
    day: 13,
    hour: 18,
    minute: 19,
    second: 20,
};
const TARGET: usize = 2;

struct FaultPaths {
    base: Arc<ObservedPaths>,
    fail: AtomicBool,
    directory: PathBuf,
    attempts: StdMutex<Vec<Vec<u8>>>,
}
impl Storage for FaultPaths {
    fn trial_token(&self) -> bool {
        self.base.trial_token()
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.base.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        self.base.trial_exists()
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.attempts.lock().unwrap().push(bytes.to_vec());
        if self.fail.load(Ordering::SeqCst) {
            // A real atomic rename error, not a manufactured anyhow error or
            // a fake successful writer. The original progress file is separate.
            let result = Paths {
                current: Some(self.directory.clone()),
                trial: None,
            }
            .write(bytes);
            assert!(
                result
                    .as_ref()
                    .is_err_and(|e| { e.chain().any(|cause| cause.is::<std::io::Error>()) })
            );
            result
        } else {
            self.base.write(bytes)
        }
    }
}
fn initial() -> SaveData {
    let mut data = SaveData::default();
    data.play_seconds = 17;
    data.settings.common.v3_sysvol = 37;
    data.save_menu_position = 0;
    let row = GameData {
        date: NOW,
        extra_word: 5,
        primary_entry: Some(GameDataEntry {
            scenario_id: 2084,
            random_seed: 0x8765_4321,
            save_position: 17,
            replay_target: 17,
            response_data: vec![0xf1, 0x55, 0x23, 0xee],
        }),
        entry: GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 0,
            replay_target: 0,
            response_data: vec![],
        },
        trailing_values: vec![7, 1023],
        byte_variables: vec![2, 255],
    };
    data.manual_save_slots[TARGET] = Some(row.clone());
    data.manual_save_slots[99] = Some(row);
    data.save_vectors.seen_messages = vec![1, 0, 0x8000_0000];
    data.save_vectors.select_seen_choices = vec![0x4000];
    data.save_vectors.select_last_choices = vec![3, 9, 15];
    data
}
fn fixture(initial: &SaveData) -> (Journey, Arc<FaultPaths>) {
    let temporary = TemporaryProgress(std::env::temp_dir().join(format!(
        "higurashi-runtime-save-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let paths = Paths {
        current: Some(temporary.0.join("progress.dat")),
        trial: None,
    };
    paths.write(&initial.encode().unwrap()).unwrap();
    let base = Arc::new(ObservedPaths {
        paths,
        reads: StdMutex::new(Vec::new()),
        writes: StdMutex::new(Vec::new()),
    });
    let directory = temporary.0.join("blocked-destination");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("retained-marker"), b"not progress").unwrap();
    let storage = Arc::new(FaultPaths {
        base: base.clone(),
        fail: AtomicBool::new(true),
        directory,
        attempts: StdMutex::new(Vec::new()),
    });
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut runtime = Runtime::new(Arc::new(server), manager, sounds, storage.clone());
    runtime.set_focused(true);
    (
        Journey {
            runtime,
            gpu: Graphics::new(device, queue),
            storage: base,
            _temporary: temporary,
        },
        storage,
    )
}
fn game(runtime: &Runtime) -> &Gameplay {
    runtime.slot().unwrap().game.as_ref().unwrap()
}
fn rows(runtime: &Runtime) -> Slots {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone()
}
fn same_rows(actual: &Slots, expected: &Slots) {
    for (index, (a, b)) in std::iter::once(&actual.auto)
        .chain(actual.manual.iter())
        .zip(std::iter::once(&expected.auto).chain(expected.manual.iter()))
        .enumerate()
    {
        assert_eq!(a.valid, b.valid, "complete row{index} validity");
        assert_eq!(a.data, b.data, "complete row{index} payload");
    }
}
fn save_ready(runtime: &Runtime) -> bool {
    game(runtime)
        .reference_save_menu()
        .is_some_and(|m| m.reference_input_ready())
}
fn failure(runtime: &Runtime) -> Option<(i32, i32, bool)> {
    game(runtime)
        .reference_save_menu()?
        .reference_save_failure()
}
fn message_ready(runtime: &Runtime) -> bool {
    runtime
        .slot()
        .and_then(|s| s.game.as_ref())
        .and_then(Gameplay::host)
        .is_some_and(|h| {
            let m = &h.runner.frame().message.playback;
            !h.has_modes()
                && h.modal_snapshot().is_none()
                && h.runner.reconstruction_phase().is_none()
                && m.wait.is_some()
                && m.interested_in_input()
        })
}
struct SourceState {
    current: RuntimeGameData,
    commands: u64,
    bindings: (usize, usize, usize, usize, usize),
    common: usize,
    manager: usize,
}
impl SourceState {
    fn new(runtime: &Runtime) -> Self {
        let game = game(runtime);
        let globals = runtime.globals().unwrap().unwrap();
        Self {
            current: game.runtime_game_data().unwrap(),
            commands: game.host().unwrap().runner.completed_commands() as u64,
            bindings: game.reference_bindings(),
            common: Arc::as_ptr(globals.common) as usize,
            manager: Arc::as_ptr(globals.manager) as usize,
        }
    }
    fn check(&self, runtime: &Runtime) {
        let game = game(runtime);
        assert_eq!(game.runtime_game_data().unwrap(), self.current);
        assert_eq!(
            game.host().unwrap().runner.completed_commands() as u64,
            self.commands
        );
        assert_eq!(game.reference_bindings(), self.bindings);
        let globals = runtime.globals().unwrap().unwrap();
        assert_eq!(Arc::as_ptr(globals.common) as usize, self.common);
        assert_eq!(Arc::as_ptr(globals.manager) as usize, self.manager);
        assert_eq!(globals.data.play_seconds, globals.clock.saved_seconds());
        assert!(
            runtime.failure().is_none(),
            "IO recovery must not become shutdown failure"
        );
    }
}
fn confirm(j: &mut Journey, viewport: InputViewport, deadline: Instant, delete: bool) {
    assert!(save_ready(&j.runtime));
    j.tap(
        if delete {
            KeyCode::KeyM
        } else {
            KeyCode::Enter
        },
        viewport,
    );
    j.until(
        viewport,
        deadline,
        "native transaction question not ready",
        |r| game(r).reference_save_menu().unwrap().reference_question() == Some((2, 1)),
    );
    j.tap(KeyCode::ArrowLeft, viewport);
    assert_eq!(
        game(&j.runtime)
            .reference_save_menu()
            .unwrap()
            .reference_question(),
        Some((2, 0))
    );
    j.tap(KeyCode::Enter, viewport);
}
fn wait_failure(j: &mut Journey, storage: &FaultPaths, viewport: InputViewport, deadline: Instant) {
    j.until(
        viewport,
        deadline,
        "actual IO failure question not ready",
        |r| failure(r) == Some((2, 0, true)),
    );
    let handles = game(&j.runtime)
        .reference_save_menu()
        .unwrap()
        .reference_save_retirements();
    assert!(!handles.is_empty());
    let attempted = attempts(storage).len();
    let written = j.storage.writes.lock().unwrap().len();
    // First publication and Question readiness are separate from retirement
    // of any glyph descendants belonging to that published Question.
    while !handles.iter().all(|handle| handle.is_finished()) {
        assert!(
            Instant::now() < deadline,
            "published failure Question descendants did not finish"
        );
        j.step(&RawInputState::new(), viewport);
        assert_eq!(failure(&j.runtime), Some((2, 0, true)));
        assert_eq!(attempts(storage).len(), attempted);
        assert_eq!(j.storage.writes.lock().unwrap().len(), written);
    }
    assert!(
        handles.iter().all(|h| h.is_finished()),
        "failed writer must have retired its worker scope"
    );
}
fn original_file(j: &Journey, expected: &[u8]) {
    assert_eq!(
        j.storage.paths.read(Source::Current).unwrap().unwrap(),
        expected
    );
}
fn attempts(storage: &FaultPaths) -> Vec<Vec<u8>> {
    storage.attempts.lock().unwrap().clone()
}
fn saved_row(source: &RuntimeGameData, second: u8) -> RuntimeGameData {
    let mut row = source.clone();
    row.date = SaveDate { second, ..NOW };
    row
}
fn pure_failure_frame(j: &mut Journey) {
    let state = failure(&j.runtime);
    let frames = j.runtime.frames;
    let clock = j.runtime.globals().unwrap().unwrap().clock.ticks();
    let pixels = j.gpu.pixels(&j.runtime, (1280, 720), false);
    let capture = j.gpu.pixels(&j.runtime, (1280, 720), true);
    assert_eq!(pixels.len(), capture.len());
    assert_eq!(
        pixels
            .iter()
            .zip(capture)
            .filter(|(a, b)| **a != *b)
            .count(),
        0
    );
    assert_eq!(failure(&j.runtime), state);
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), clock);
}

#[test]
#[ignore = "actual Runtime/manual Save+Delete real atomic IO errors/retained Retry-or-Return/raw permission quarantine/frozen snapshots;real assets/offscreen Metal/exact Loopback Audio;temporary Paths only"]
fn higurashi_runtime_manual_save_delete_retry_and_return_preserve_frozen_transactions() {
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
        "authored first message input not ready",
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
            let call = calls.fetch_add(1, Ordering::SeqCst);
            assert!(call < 2, "retry/delete must not recapture calendar");
            Ok(SaveDate {
                second: NOW.second + call as u8,
                ..NOW
            })
        });
    let source = SourceState::new(&j.runtime);
    let before = rows(&j.runtime);
    j.tap(KeyCode::KeyM, viewport);
    j.until(viewport, deadline, "SystemMenu Save row not ready", |r| {
        game(r).host().unwrap().reference_menu_control() == Some((0, true))
    });
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "retained Save menu not ready",
        save_ready,
    );
    assert_eq!(
        game(&j.runtime)
            .reference_save_menu()
            .unwrap()
            .state()
            .selected,
        0
    );
    for selected in 1..=TARGET {
        j.tap(KeyCode::ArrowDown, viewport);
        j.until(
            viewport,
            deadline,
            "manual row navigation stayed busy",
            save_ready,
        );
        assert_eq!(
            game(&j.runtime)
                .reference_save_menu()
                .unwrap()
                .state()
                .selected,
            selected as i32
        );
    }

    // The first failure retains the mutation until the user chooses Return.
    confirm(&mut j, viewport, deadline, false);
    wait_failure(&mut j, &storage, viewport, deadline);
    assert_eq!(calendar.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&storage).len(), 1);
    assert!(j.storage.writes.lock().unwrap().is_empty());
    original_file(&j, &initial_file);
    source.check(&j.runtime);
    let mut first_pending = before.clone();
    first_pending.manual[TARGET].valid = true;
    first_pending.manual[TARGET].data = saved_row(&source.current, NOW.second);
    same_rows(&rows(&j.runtime), &first_pending);
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .save_menu_position,
        TARGET as u8
    );
    pure_failure_frame(&mut j);

    // Denied *fresh* keys still exist in the focused raw input bridge. Granting
    // permission while held cannot replay either A or B as a new response.
    for (code, bit) in [(KeyCode::Enter, 24), (KeyCode::Escape, 25)] {
        j.step(&blank, viewport);
        j.gpu.update_with_permission(
            &mut j.runtime,
            &key(code),
            viewport,
            Duration::from_millis(50),
            false,
        );
        assert_ne!(j.runtime.reference_sampled_buttons.pressed & (1 << bit), 0);
        assert_eq!(
            j.runtime.input.buttons().pressed,
            0,
            "Gameplay quarantines denied input at callback end"
        );
        assert_eq!(failure(&j.runtime), Some((2, 0, true)));
        j.step(&key(code), viewport);
        assert_eq!(j.runtime.reference_sampled_buttons.pressed & (1 << bit), 0);
        assert_eq!(failure(&j.runtime), Some((2, 0, true)));
        assert_eq!(attempts(&storage).len(), 1);
        source.check(&j.runtime);
        same_rows(&rows(&j.runtime), &first_pending);
        j.step(&blank, viewport);
    }
    j.tap(KeyCode::Escape, viewport); // Actual B selects Return, never success.
    j.until(
        viewport,
        deadline,
        "Return did not resume the same Save menu",
        save_ready,
    );
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
    assert_eq!(calendar.load(Ordering::SeqCst), 1);
    assert_eq!(attempts(&storage).len(), 1);
    assert_eq!(
        game(&j.runtime)
            .reference_save_menu()
            .unwrap()
            .reference_current()
            .unwrap()
            .date,
        NOW
    );
    source.check(&j.runtime);
    original_file(&j, &initial_file);

    // Explicit fresh Save captures date21 once. Both failed and successful
    // retries must reuse exactly those immutable encoded bytes.
    confirm(&mut j, viewport, deadline, false);
    wait_failure(&mut j, &storage, viewport, deadline);
    let second = attempts(&storage)[1].clone();
    assert_ne!(second, attempts(&storage)[0]);
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    let mut second_pending = before.clone();
    second_pending.manual[TARGET].valid = true;
    second_pending.manual[TARGET].data = saved_row(&source.current, NOW.second + 1);
    same_rows(&rows(&j.runtime), &second_pending);
    j.tap(KeyCode::Enter, viewport); // Retry still encounters the real IO error.
    wait_failure(&mut j, &storage, viewport, deadline);
    assert_eq!(attempts(&storage).len(), 3);
    assert_eq!(attempts(&storage)[2], second);
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    source.check(&j.runtime);
    same_rows(&rows(&j.runtime), &second_pending);
    original_file(&j, &initial_file);
    storage.fail.store(false, Ordering::SeqCst);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "successful retry did not complete native Save notice",
        save_ready,
    );
    let saved_attempts = attempts(&storage);
    assert_eq!(saved_attempts.len(), 4);
    assert_eq!(saved_attempts[3], second);
    assert_eq!(*j.storage.writes.lock().unwrap(), [second.clone()]);
    original_file(&j, &second);
    let saved_rows = second_pending;
    same_rows(&rows(&j.runtime), &saved_rows);
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .save_menu_position,
        TARGET as u8
    );
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    source.check(&j.runtime);
    assert!(SaveData::decode(&second).unwrap().manual_save_slots[TARGET].is_some());

    // Delete has no date capture and changes only validity. Its Return restores
    // the complete dormant payload, then a fresh Delete can retry successfully.
    storage.fail.store(true, Ordering::SeqCst);
    confirm(&mut j, viewport, deadline, true);
    wait_failure(&mut j, &storage, viewport, deadline);
    assert_eq!(attempts(&storage).len(), 5);
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    original_file(&j, &second);
    let mut delete_pending = saved_rows.clone();
    delete_pending.manual[TARGET].valid = false;
    same_rows(&rows(&j.runtime), &delete_pending);
    j.tap(KeyCode::ArrowRight, viewport);
    assert_eq!(failure(&j.runtime), Some((2, 1, true)));
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "Delete Return did not resume Save Input",
        save_ready,
    );
    same_rows(&rows(&j.runtime), &saved_rows);
    assert_eq!(
        j.runtime
            .globals()
            .unwrap()
            .unwrap()
            .data
            .save_menu_position,
        TARGET as u8
    );
    source.check(&j.runtime);
    confirm(&mut j, viewport, deadline, true);
    wait_failure(&mut j, &storage, viewport, deadline);
    let deletion = attempts(&storage)[5].clone();
    storage.fail.store(false, Ordering::SeqCst);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "Delete retry did not complete native Save notice",
        save_ready,
    );
    assert_eq!(attempts(&storage).len(), 7);
    assert_eq!(attempts(&storage)[6], deletion);
    assert_eq!(
        *j.storage.writes.lock().unwrap(),
        [second.clone(), deletion.clone()]
    );
    original_file(&j, &deletion);
    let deleted = delete_pending;
    same_rows(&rows(&j.runtime), &deleted);
    assert!(SaveData::decode(&deletion).unwrap().manual_save_slots[TARGET].is_none());
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    source.check(&j.runtime);
    j.tap(KeyCode::Escape, viewport);
    j.until(
        viewport,
        deadline,
        "Save Back did not restore actual SystemMenu",
        |r| {
            game(r).reference_save_menu().is_none()
                && game(r).host().unwrap().reference_menu_control() == Some((0, true))
        },
    );
    source.check(&j.runtime);
    j.tap(KeyCode::Escape, viewport);
    j.until(
        viewport,
        deadline,
        "SystemMenu Back did not restore authored message",
        message_ready,
    );
    source.check(&j.runtime);
    same_rows(&rows(&j.runtime), &deleted);
    assert_eq!(
        std::fs::read(storage.directory.join("retained-marker")).unwrap(),
        b"not progress"
    );
    assert_eq!(attempts(&storage).len(), 7);
    assert_eq!(calendar.load(Ordering::SeqCst), 2);
    let mut files = std::fs::read_dir(&j._temporary.0)
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
    println!(
        "Runtime actual Save/Delete:7 real atomic attempts/2 writes;SaveReturn,failedRetry,exact frozenRetry,DeleteReturn+Retry;full101 owners/VM pause/calendar2;denied fresh A/B+held grants;temporary progress only"
    );
}
