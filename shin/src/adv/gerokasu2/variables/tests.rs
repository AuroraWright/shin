use super::*;
use crate::adv::gerokasu2::{layers::Gerokasu2Layers, responses, save_commands::SaveSession};
use shin_core::{format::scenario::gerokasu2::Gerokasu2Scenario, vm::gerokasu2::Gerokasu2Scripter};
use std::sync::Arc;
fn vm() -> Gerokasu2Scripter {
    let mut d = b"SNR ".to_vec();
    for w in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        d.extend(w.to_le_bytes());
    }
    let mut t = Vec::new();
    for i in 0..10 {
        d.extend((0x4c + t.len() as u32).to_le_bytes());
        if i < 7 {
            t.extend(4u32.to_le_bytes());
        }
        t.extend(0u32.to_le_bytes());
    }
    d.extend(t);
    let start = d.len() as u32;
    d[32..36].copy_from_slice(&start.to_le_bytes());
    d.extend([0, 0, 0]);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut vm = Gerokasu2Scripter::new(Arc::new(Gerokasu2Scenario::new(d.into()).unwrap()), 0, 42);
    vm.run(shin_core::vm::command::CommandResult::None, 32).unwrap();
    vm
}
fn snapshot(s: &SaveSession, value: i32) -> Vec<u8> {
    let t = s.timeline;
    let pending = s.variables.pending();
    let committed = &s.variables.committed().0;
    let route = &s.progress.active_entry().responses;
    let mut out = b"GVAR".to_vec();
    for w in [
        value as u32,
        t.boundary,
        t.previous_message,
        t.current_message,
        t.last_skip,
        t.resume_position,
        t.event as u32,
        s.variables.commits_enabled as u32,
        s.response_bit_position as u32,
        pending.len() as u32,
        committed.len() as u32,
        route.len() as u32,
    ] {
        out.extend(w.to_le_bytes());
    }
    for &(slot, value) in pending {
        out.extend(slot.to_le_bytes());
        out.extend(value.to_le_bytes());
    }
    for value in committed {
        out.extend(value.to_le_bytes());
    }
    out.extend(route);
    out
}
#[test]
fn ger_variables_invalid_commit_preserves_pending_and_disabled_commit() {
    let mut v = GerVariables::default();
    v.set(1, 65537);
    v.set(-1, 2);
    assert!(v.commit().is_err());
    assert_eq!(v.pending().len(), 2);
    assert!(v.committed().0.is_empty());
    v.commits_enabled = false;
    v.commit().unwrap();
    assert_eq!(v.get(1), 65537);
    assert_eq!(v.get(-1), 2);
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original event/bookmark,variable live/replay,commit prefix and route bodies;CPU only"]
fn ger_native_event_variables_commit_and_interleaved_response_stream() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/gameplay");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-event-variables");
    let c = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-event-variables.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let vm = vm();
    let layers = Gerokasu2Layers::default();
    let mut request = Vec::new();
    let mut expected = Vec::new();
    fn record(
        request: &mut Vec<u8>,
        expected: &mut Vec<Vec<u8>>,
        row: [u32; 8],
        s: &SaveSession,
        value: i32,
    ) {
        request.extend(row.into_iter().flat_map(u32::to_le_bytes));
        expected.push(snapshot(s, value));
    }
    for boundary in [0, 99, 100, u32::MAX] {
        for enabled in [false, true] {
            for cursor in [0, 1, 7, 8, 15, 31, 8191] {
                for (size, count, seed) in [(0, 0, 0), (3, 2, 255), (1024, 64, 65535)] {
                    let mut s = SaveSession::fresh(0, 42);
                    s.timeline.boundary = boundary;
                    s.timeline.event = -1;
                    s.timeline.previous_message = 7;
                    s.timeline.current_message = 9;
                    s.timeline.last_skip = 11;
                    s.timeline.resume_position = 13;
                    s.variables.commits_enabled = enabled;
                    s.response_bit_position = cursor;
                    s.progress.active_entry_mut().responses =
                        (0..size).map(|i| (i * 37 + seed) as u8).collect();
                    s.variables.replace_committed(PersistData(
                        (0..count).map(|i| (i * 641 + seed) as i16).collect(),
                    ));
                    let mut value = 0;
                    record(
                        &mut request,
                        &mut expected,
                        [
                            0,
                            boundary,
                            u32::MAX,
                            enabled as u32,
                            cursor as u32,
                            size,
                            count,
                            seed,
                        ],
                        &s,
                        value,
                    );
                    for (slot, set) in [
                        (0, 0),
                        (1, 1),
                        (64, -1),
                        (1, 65537),
                        (63, i32::MIN),
                        (127, i32::MAX),
                        (2, -32768),
                        (3, 65536),
                    ] {
                        s.variables.set(slot, set);
                        record(
                            &mut request,
                            &mut expected,
                            [1, slot as u32, set as u32, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                        value = s.persistent_get(slot);
                        record(
                            &mut request,
                            &mut expected,
                            [2, slot as u32, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                    }
                    s.event_begin(&vm, &layers, 0).unwrap();
                    record(
                        &mut request,
                        &mut expected,
                        [4, 0, 0, 0, 0, 0, 0, 0],
                        &s,
                        value,
                    );
                    for slot in [0, 1, 2, 3, 63, 64, 127, -1, 65535] {
                        value = s.persistent_get(slot);
                        record(
                            &mut request,
                            &mut expected,
                            [2, slot as u32, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                    }
                    s.event_end(&vm, &layers).unwrap();
                    record(
                        &mut request,
                        &mut expected,
                        [5, 0, 0, 0, 0, 0, 0, 0],
                        &s,
                        value,
                    );
                    s.resume_set(&vm, &layers).unwrap();
                    record(
                        &mut request,
                        &mut expected,
                        [6, 0, 0, 0, 0, 0, 0, 0],
                        &s,
                        value,
                    );
                    for (b, target) in [(0, 1), (1, 1), (2, 1), (u32::MAX, u32::MAX)] {
                        s.timeline.boundary = b;
                        s.variables.set_replay(5, 42, b, target);
                        record(
                            &mut request,
                            &mut expected,
                            [9, 5, 42, b, target, 0, 0, 0],
                            &s,
                            value,
                        );
                    }
                    s.variables.commit().unwrap();
                    record(
                        &mut request,
                        &mut expected,
                        [3, 0, 0, 0, 0, 0, 0, 0],
                        &s,
                        value,
                    );
                    s.response_bit_position = cursor;
                    record(
                        &mut request,
                        &mut expected,
                        [7, cursor as u32, 0, 0, 0, 0, 0, 0],
                        &s,
                        value,
                    );
                    for _ in 0..8 {
                        value = s.persistent_replay();
                        record(
                            &mut request,
                            &mut expected,
                            [8, 0, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                    }
                    for quiz in [0, 1, 63, 64, -1, i32::MIN] {
                        responses::write_bits(
                            &mut s.progress.active_entry_mut().responses,
                            &mut s.response_bit_position,
                            6,
                            quiz as u32,
                        );
                        record(
                            &mut request,
                            &mut expected,
                            [10, quiz as u32, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                        let position = s.response_bit_position - 6;
                        s.response_bit_position = position;
                        record(
                            &mut request,
                            &mut expected,
                            [7, position as u32, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                        value = responses::read_quiz(
                            &mut s.progress.active_entry_mut().responses,
                            &mut s.response_bit_position,
                        );
                        record(
                            &mut request,
                            &mut expected,
                            [11, 0, 0, 0, 0, 0, 0, 0],
                            &s,
                            value,
                        );
                    }
                }
            }
        }
    }
    let input = folder.join("event-variables-request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("event-variables-native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("event-variables-native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut at = 0;
    for (i, e) in expected.iter().enumerate() {
        let a = &output.stdout[at..at + e.len()];
        let diff: Vec<_> = a
            .iter()
            .zip(e)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(8)
            .collect();
        assert!(
            diff.is_empty(),
            "event/variables snapshot{i} byte/native/Rust {diff:?}"
        );
        at += e.len();
    }
    assert_eq!(at, output.stdout.len());
    println!(
        "GEROKASU2_EVENT_VARIABLES_MODEL_NATIVE snapshots={} compared_bytes={} live_replay_pending_signed16_commit_event_bookmark_route=verified",
        expected.len(),
        at
    );
}
