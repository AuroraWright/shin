use super::*;

#[test]
fn ger_checkpoint_archive_predecessor_excludes_newest_and_prunes_selected() {
    let mut archive = Archive::default();
    for boundary in [0, 200, 400, 600] {
        assert!(archive.capture(boundary, || Ok(boundary)).unwrap());
    }
    assert!(
        !archive
            .capture(600, || panic!("existing bucket must not capture"))
            .unwrap()
    );
    for (target, selected, retained) in [
        (0, 0, 0),
        (200, 0, 0),
        (201, 200, 1),
        (599, 400, 2),
        (600, 400, 2),
        (999, 400, 2),
    ] {
        let fork = archive.fork_for_restore(target);
        assert_eq!(fork.restored.unwrap().state, selected);
        assert_eq!(fork.archive.entries().len(), retained);
        assert_eq!(archive.entries().len(), 4);
    }
    assert_eq!(archive.clone_newest().restored.unwrap().state, 600);
    assert!(
        Archive::<u32>::default()
            .fork_for_restore(999)
            .restored
            .is_none()
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original Ger E500/E820 archive/progress/cursor restore; CPU only"]
fn ger_native_checkpoint_archive_clone_restore_unsigned_predecessor_and_pruning() {
    use std::{
        fs,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/replay");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-checkpoint-fork");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-checkpoint-fork.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
            root.join("gerokasu2/.local/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut cases = 0;
    let mut bytes = 0;
    let archives: &[&[u32]] = &[
        &[],
        &[0],
        &[0, 200],
        &[0, 200, 400, 600],
        &[7, 7, 7, 7],
        &[u32::MAX, 0, 200],
        &[0, 0x80000000, u32::MAX],
        &[400, 1, 700, 0],
    ];
    for boundaries in archives {
        let archive = Archive {
            entries: boundaries
                .iter()
                .enumerate()
                .map(|(id, &boundary)| Entry {
                    boundary,
                    state: id as u32,
                })
                .collect(),
        };
        for op in [0, 1] {
            for active in [0, 1] {
                for source in [0, 777, u32::MAX] {
                    for saved in [
                        0,
                        1,
                        7,
                        199,
                        200,
                        201,
                        399,
                        400,
                        600,
                        601,
                        0x80000000,
                        u32::MAX,
                    ] {
                        let target = saved.wrapping_add(37);
                        let fork = if op == 0 {
                            archive.clone_newest()
                        } else {
                            archive.fork_for_restore(saved)
                        };
                        let restored = fork.restored.as_ref();
                        let id = restored.map_or(u32::MAX, |e| e.state);
                        let row = [
                            op,
                            active,
                            source,
                            saved,
                            target,
                            boundaries.len() as u32,
                            0,
                            0,
                        ];
                        input
                            .write_all(
                                &row.into_iter()
                                    .chain(boundaries.iter().copied())
                                    .flat_map(u32::to_le_bytes)
                                    .collect::<Vec<_>>(),
                            )
                            .unwrap();
                        input.flush().unwrap();
                        let mut expected = b"GCF1".to_vec();
                        for word in [
                            id,
                            restored.map_or(0, |e| e.boundary),
                            if op == 1 {
                                saved
                            } else if restored.is_some() {
                                source
                            } else {
                                0
                            },
                            if op == 1 {
                                target
                            } else if restored.is_some() {
                                source
                            } else {
                                0
                            },
                            if id == u32::MAX { 0 } else { id * 13 },
                            if id == u32::MAX { 0 } else { id & 1 },
                            fork.archive.entries().len() as u32,
                            if restored.is_some() { 3 } else { 0 },
                        ] {
                            expected.extend(word.to_le_bytes());
                        }
                        for e in fork.archive.entries() {
                            expected.extend(e.state.to_le_bytes());
                        }
                        if restored.is_some() {
                            for word in [id, id + 7, u32::MAX] {
                                expected.extend(word.to_le_bytes());
                            }
                        }
                        let mut actual = vec![0; expected.len()];
                        output.read_exact(&mut actual).unwrap();
                        assert_eq!(
                            actual, expected,
                            "fork case{cases},row{row:?},boundaries{boundaries:?}"
                        );
                        cases += 1;
                        bytes += actual.len();
                    }
                }
            }
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_CHECKPOINT_FORK_NATIVE_MATCH cases={cases} bytes={bytes} original_unsigned_lookup_prune_progress_cursor_extras=verified prepared_DFE0_resource_teardown=bounded"
    );
}
