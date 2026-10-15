use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
};

#[test]
#[ignore = "actual updated acquired-bit a9fe0/a9f2c/ab014,empty/partial/padded/large vectors,duplicate/read bits and full snapshot synchronization;no audio/GPU/windows/user saves"]
fn higurashi_saved_preview_global_acquisition_matches_native_growth_and_snapshots() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut checks = 0;
    for count in [0usize, 1, 20, 255, 256, 257, 600, 2048] {
        for ids in [
            &[0, 0, 1, 17, 17, 244, 314][..],
            &[0, 15, 16, 4095, 4096, 4096, 8191, 8192],
            &[314, 314, 9999, 9999, 32767],
        ] {
            let mut saved = SaveData::default();
            saved.save_vectors.extra_words_0 = (0..count)
                .map(|i| 0xa5a5a5a5u32 ^ ((i as u32).wrapping_mul(0x9e3779b9)))
                .collect();
            let mut state = State {
                snapshot: Snapshot::new(&saved),
                pending: Vec::new(),
            };
            let mut observations = Vec::new();
            input += &format!("B {count} {}", ids.len());
            for &id in ids {
                input += &format!(" {id}");
                let before = state.acquired(id);
                state.acquire(id).unwrap();
                observations.extend([
                    u32::from(before),
                    u32::from(!before),
                    u32::from(state.acquired(id)),
                ]);
            }
            input.push('\n');
            state.synchronize(&mut saved).unwrap();
            let mut words = vec![saved.save_vectors.extra_words_0.len() as u32];
            words.extend_from_slice(&saved.save_vectors.extra_words_0);
            words.push(ids.len() as u32);
            words.extend(observations);
            checks += saved.save_vectors.extra_words_0.len();
            expected.push(words);
        }
    }
    let exe = std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_ACQUIRED_BITS").unwrap();
    let mut child = Command::new(exe)
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let capture = input.clone();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_ACQUIRED_CAPTURE") {
        let path = std::path::Path::new(&path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("input.txt"), capture).unwrap();
        std::fs::write(path.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(path.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), 24);
    for (index, (actual, expected)) in output.lines().zip(expected).enumerate() {
        let mut parts = actual.split_whitespace();
        assert_eq!(parts.next(), Some("B"));
        let actual = parts
            .filter(|&p| p != "C")
            .map(|p| p.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "original acquired bank profile{index}");
    }
    eprintln!(
        "Saved preview global acquisition:24 native owners/{checks} exact bank words/160 read-set-read observations;256-word growth,existing read/acquired bits,padding,duplicates and synchronization into full settings pass"
    );
}

#[test]
fn higurashi_saved_preview_clone_acquisition_is_serialized_with_manual_snapshots() {
    let mut saved = SaveData::default();
    saved.save_vectors.extra_words_0 = vec![0; 20];
    let mut slots = crate::app::higurashi::runtime_saves::Slots::loaded(&saved);
    let mut current = slots.auto.data.clone();
    let date = shin_core::format::save::higurashi::SaveDate {
        year: 2026,
        month: 10,
        day: 13,
        hour: 1,
        minute: 2,
        second: 3,
    };
    let shared = State::shared(Snapshot::new(&saved));
    // A clone has changed globals before a thumbnail exists or is published.
    shared.lock().acquire(17).unwrap();
    // An independent main-thread read-status update must survive synchronization.
    saved.save_vectors.extra_words_0[1] |= 1 << 3;
    let frozen = {
        let mut state = shared.lock();
        state.synchronize(&mut saved).unwrap();
        let transaction = slots.begin_save(&mut saved, 2, &mut current, date).unwrap();
        let snapshot = transaction.snapshot();
        transaction.complete(&mut slots, &mut saved, true);
        snapshot
    };
    let attempted = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let guard = shared.lock();
    let state = shared.clone();
    let started = attempted.clone();
    let finished = done.clone();
    let worker = std::thread::spawn(move || {
        started.store(true, Ordering::Release);
        state.lock().acquire(244).unwrap();
        finished.store(true, Ordering::Release);
    });
    while !attempted.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    assert!(
        !done.load(Ordering::Acquire),
        "clone cannot mutate while main-thread snapshot owns its bank"
    );
    drop(guard);
    worker.join().unwrap();
    shared.lock().synchronize(&mut saved).unwrap();
    let disk = SaveData::decode(&frozen.encode().unwrap()).unwrap();
    assert_eq!(
        disk.save_vectors.extra_words_0[1] & 0xc,
        0xc,
        "frozen snapshot includes prior acquired/read bits"
    );
    assert_eq!(
        disk.save_vectors.extra_words_0[15] & (1 << 8),
        0,
        "later clone never changes frozen disk data"
    );
    assert_ne!(
        saved.save_vectors.extra_words_0[15] & (1 << 8),
        0,
        "later/discarded clone still updates live globals"
    );
    assert_eq!(slots.manual[2].data, current);
    assert!(slots.manual[2].valid);
}
