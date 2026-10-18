#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original19CE0 phase0 plus bank-ID enumeration/transition-target unwrapping; prepared planes/RTTI/clone/job, CPU only"]
fn ger_native_reconstruction_cache_enumerates_plane_bank_targets_and_excludes_interactive() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/reconstruction");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-reconstruction-enumeration");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-reconstruction-enumeration.c"))
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
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for seed in [0u32, 42, 0xdead_beef, u32::MAX] {
        input.write_all(&seed.to_le_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 4 * 68);
    // For each plane: bank2 direct,bank7 transition TARGET,bank19 Card excluded.
    // Deliberately eligible source nodes40..43 never enter the cache.
    let expected = [8u32, 9, 11, 12, 14, 15, 17, 18];
    for snapshot in output.stdout.chunks_exact(68) {
        assert_eq!(&snapshot[..4], b"GRE1");
        let words: Vec<_> = snapshot[4..]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(&words[..8], &expected);
        assert_eq!(&words[8..], &expected);
    }
}
