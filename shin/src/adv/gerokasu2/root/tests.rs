use super::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original root copy/message alias/stencil with prepared owners;CPU only"]
fn gerokasu2_native_root_copy_retains_message_and_copies_screen() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/root");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-root");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-root.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let raw = fs::read(root.join("game-data/gerokasu2/uncompressed_executable")).unwrap();
    let word = |at: usize| u32::from_le_bytes(raw[256 + at..260 + at].try_into().unwrap());
    assert_eq!(word(0xf5dc), 0x2a1f03e1); // Screen root storage ID0
    assert_eq!(word(0xf74c), 0x52800021); // Message root storage ID1
    let pointer = |at: usize| u64::from_le_bytes(raw[256 + at..264 + at].try_into().unwrap());
    assert_eq!(pointer(0x1518b0 + 24), 0x14f600); // Message derives NewDrawableLayer
    let mut request = Vec::new();
    let mut expected = Vec::new();
    for copies in 1..=8u32 {
        for mask in [0, 1, 3, 0x80000000, u32::MAX] {
            for delay in [0.0, 1.0, -1.0, 15.25f32] {
                for strong in [0, 1, 97] {
                    request.extend(
                        [copies, mask, delay.to_bits(), strong]
                            .into_iter()
                            .flat_map(u32::to_le_bytes),
                    );
                    for generation in 1..=copies {
                        expected.push([
                            generation,
                            2,
                            generation + 1,
                            generation,
                            1,
                            1,
                            generation,
                            0,
                            1,
                            delay.to_bits(),
                            mask,
                            3,
                        ]);
                    }
                }
            }
        }
    }
    let copy_count = expected.len();
    for allowed in [0, 1, 2, 255u32] {
        for delta in [0.0, 1.0, -1.0, 15.25, f32::NAN, f32::INFINITY] {
            request.extend(
                [9, allowed, delta.to_bits(), 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes),
            );
            expected.push([
                3,
                1,
                allowed,
                delta.to_bits(),
                2,
                allowed & 1,
                delta.to_bits(),
                3,
                allowed & 1,
                delta.to_bits(),
                0,
                0,
            ]);
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout.len(), expected.len() * 52);
    for (row, (actual, expected)) in output
        .stdout
        .chunks_exact(52)
        .zip(expected.iter())
        .enumerate()
    {
        assert_eq!(
            &actual[..4],
            if row < copy_count { b"GRC1" } else { b"GRU1" }
        );
        assert_eq!(
            actual[4..]
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .collect::<Vec<_>>(),
            expected,
            "root copy{row}"
        );
    }
    println!(
        "GEROKASU2_ROOT_NATIVE snapshots={} words={} message_alias_screen_copy_stencil3_slots0_1_NewDrawable_RTTI_updates24=verified",
        expected.len(),
        expected.len() * 12
    );
}
