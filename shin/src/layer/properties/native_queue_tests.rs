use super::*;
use shin_core::time::{Easing, Tween, TweenOptions};
use std::{
    fs,
    process::{Command, Stdio},
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 all-property fast-forward and queued clocks; no GPU/audio"]
fn reference_native_property_queue_dispatch() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::temp_dir().join("shin-native-property-queues-results");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join("reverse-engineering/native-property-queues.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let mut request = String::new();
    let mut expected = Vec::new();
    for pattern in 0..4 {
        let mut data = LayerPropertiesData::new();
        for (property, tweener) in &mut data.properties {
            let base = property as usize as f32 * 8.0;
            tweener.fast_forward_to(base);
            for (target, duration, protected) in [
                (base + 8.0, 8.0, pattern == 0 || pattern == 3),
                (base + 16.0, 8.0, pattern == 1),
                (base + 24.0, 0.0, true),
                (base + 32.0, 8.0, pattern == 2),
            ] {
                tweener.enqueue_with_options(
                    target,
                    Tween {
                        duration: Ticks::from_f32(duration),
                        easing: Easing::Linear,
                    },
                    TweenOptions {
                        prohibit_fast_forward: protected,
                        ignore_wait: false,
                    },
                );
            }
        }
        for instruction in [
            format!("N {pattern}"),
            "T 2".into(),
            "T -1".into(),
            "T NaN".into(),
            "T -inf".into(),
            "T -0".into(),
            "T 0".into(),
            "F".into(),
            "F".into(),
            "T 4".into(),
            "F".into(),
            "T 8".into(),
            "F".into(),
            "T 16".into(),
            "T inf".into(),
        ] {
            match instruction.as_bytes()[0] {
                b'T' => {
                    let delta = instruction[2..].parse::<f32>().unwrap();
                    for (_, tweener) in &mut data.properties {
                        tweener.update(Ticks::from_f32(delta));
                    }
                }
                b'F' => data.fast_forward(),
                _ => {}
            }
            request.push_str(&instruction);
            request.push('\n');
            expected.push(
                data.properties
                    .values()
                    .map(|t| (t.value().to_bits(), u32::from(!t.is_idle())))
                    .collect::<Vec<_>>(),
            );
        }
    }
    let input = folder.join("request.txt");
    fs::write(&input, request).unwrap();
    let mut mismatches = Vec::new();
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let output = Command::new(&binary)
            .arg(root.join("umineko_switch_files").join(executable))
            .stdin(Stdio::from(fs::File::open(&input).unwrap()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(folder.join(format!("{version}.trace")), &output.stdout).unwrap();
        let trace = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = trace.lines().collect();
        assert_eq!(rows.len(), expected.len());
        for (row, (line, values)) in rows.into_iter().zip(&expected).enumerate() {
            let words: Vec<_> = line.split_whitespace().collect();
            assert_eq!(words.len(), 182);
            assert_eq!(words[0], "Q");
            for (property, &(value, busy)) in values.iter().enumerate() {
                let native = (
                    u32::from_str_radix(words[2 + property * 2], 16).unwrap(),
                    words[3 + property * 2].parse::<u32>().unwrap(),
                );
                if native != (value, busy) {
                    mismatches.push(format!(
                        "{version} row{row} property{property}: native={native:?} port={:?}",
                        (value, busy)
                    ));
                }
            }
        }
    }
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "mismatches={}\n{}",
            mismatches.len(),
            mismatches
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    assert!(
        mismatches.is_empty(),
        "{} mismatches: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(4)]
    );
}
