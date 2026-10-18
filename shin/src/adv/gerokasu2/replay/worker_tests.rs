//! Bounded original worker prefix evidence. Poll, refork and VM destruction are
//! prepared services; this oracle excludes live resource hydration/publication.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original1B180 entry/poll/refork/cancel prefix; CPU only"]
fn ger_native_replay_worker_threshold_refork_cancellation_and_mutation_guards() {
    use std::{fs, process::Command};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/replay-worker");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-replay-worker");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-replay-worker.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = |mutant: Option<&str>| {
        let mut command = Command::new(&binary);
        command
            .arg(root.join("gerokasu2/uncompressed_executable"))
            .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
            .env_remove("SHIN_GER_REPLAY_WORKER_MUTANT");
        if let Some(mutant) = mutant {
            command.env("SHIN_GER_REPLAY_WORKER_MUTANT", mutant);
        }
        command.output().unwrap()
    };
    let native = run(None);
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let output = String::from_utf8(native.stdout).unwrap();
    assert_eq!(output.lines().count(), 22);
    assert!(
        output.contains(
            "GWORK crossing0 ready polls=3 forks=0 deletes=0 boundary=3 saved=1 active=0"
        )
    );
    assert!(output.contains(
        "GWORK event_cancel0 cancelled polls=1 forks=1 deletes=1 boundary=4 saved=4 active=1"
    ));
    for mutant in [
        "completed_early_ready",
        "omit_event_restart",
        "omit_preasset_cancel",
        "wrong_last_skip",
    ] {
        let output = run(Some(mutant));
        assert!(!output.status.success(), "worker mutant survived: {mutant}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("worker"),
            "worker mutant did not fail its checked contract: {mutant}"
        );
    }
    println!(
        "GEROKASU2_REPLAY_WORKER_NATIVE cases=22 mutants=4 original_entry_complete_poll_blocked_refork_cancel_prefix=verified hydration=excluded prepared_callback_contracts=explicit"
    );
}
