//! Original full EEC0 capture, independent of the earlier oversized archive
//! fixture which suppressed construction. Live resource teardown is excluded.
use super::*;
use shin_core::format::scenario::gerokasu2::Gerokasu2Scenario;

const SCHEDULE: [(u32, usize); 14] = [
    (0, 1),
    (0, 1),
    (99, 1),
    (100, 1),
    (199, 1),
    (200, 2),
    (201, 2),
    (399, 2),
    (400, 3),
    (400, 3),
    (999, 4),
    (999, 5),
    (999, 5),
    (999, 5),
];

fn checkpoint_vm() -> Gerokasu2Scripter {
    let mut data = b"SNR ".to_vec();
    for word in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        data.extend(word.to_le_bytes());
    }
    let mut tables = Vec::new();
    for index in 0..10 {
        data.extend((0x4c + tables.len() as u32).to_le_bytes());
        if index < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    data.extend(tables);
    let start = data.len() as u32;
    data[32..36].copy_from_slice(&start.to_le_bytes());
    data.extend([0, 0, 0]);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut vm = Gerokasu2Scripter::new(
        Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()),
        0,
        42,
    );
    vm.run(shin_core::vm::command::CommandResult::None, 32)
        .unwrap();
    vm
}

fn session_schedule() -> Vec<(u32, usize)> {
    let vm = checkpoint_vm();
    let layers = Gerokasu2Layers::default();
    let mut save = SaveSession::fresh(0, 42);
    SCHEDULE
        .into_iter()
        .map(|(boundary, expected)| {
            save.timeline.boundary = boundary;
            save.message(&vm, &layers).unwrap();
            let actual = save.checkpoints.len();
            assert_eq!(actual, expected, "pre-constructor boundary {boundary}");
            (boundary, actual)
        })
        .collect()
}

#[test]
fn ger_checkpoint_sparse_schedule_uses_200_boundary_buckets_and_single_gap_capture() {
    assert_eq!(session_schedule(), SCHEDULE);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original full Ger EEC0 capture/sparse schedule and save key; CPU only"]
fn ger_native_checkpoint_full_capture_sparse_schedule_and_save_key() {
    use std::{fs, process::Command};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/checkpoint");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-checkpoint");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-checkpoint.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let output = String::from_utf8(run.stdout).unwrap();
    assert!(output.starts_with("KEY 144965dd\n"));
    assert_eq!(
        output
            .lines()
            .filter(|line| line.starts_with("CAP "))
            .count(),
        9
    );
    let native: Vec<(u32, usize)> = output
        .lines()
        .filter_map(|line| line.strip_prefix("SCHEDULE "))
        .map(|line| {
            let mut words = line.split_whitespace();
            let boundary = words.next().unwrap().parse().unwrap();
            let count = words.next().unwrap().parse().unwrap();
            assert!(words.next().is_none());
            (boundary, count)
        })
        .collect();
    assert_eq!(native, session_schedule());
    println!(
        "GEROKASU2_CHECKPOINT_NATIVE payloads=9 schedules={} arithmetic_PC_seed_scene_extra_vector_cursor_system_permission=verified live_resources_teardown=excluded",
        native.len()
    );
}
