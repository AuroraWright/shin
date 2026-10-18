#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "originalSELECT/QUIZ live prefix; first UI binding entry guarded, CPU only"]
fn ger_native_reconstruction_retained_selection_and_quiz_skip_ui_for_positive_stop() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/reconstruction");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-reconstruction-retained");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-reconstruction-retained.c"))
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
    let mut cases = Vec::new();
    for command in [0u32, 1] {
        for stop in [i32::MIN, -1, 0, 1, 2, i32::MAX] {
            input.write_all(&command.to_le_bytes()).unwrap();
            input.write_all(&stop.to_le_bytes()).unwrap();
            cases.push((command, stop));
        }
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), cases.len() * 16);
    for ((command, stop), snapshot) in cases.iter().zip(output.stdout.chunks_exact(16)) {
        assert_eq!(&snapshot[..4], b"GRT1");
        let words: Vec<_> = snapshot[4..]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(
            words,
            if *stop > 0 { [1, 0, 1] } else { [0, 1, 1] },
            "command{command}/stop{stop}"
        );
    }
}
