#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "whole original1B180 cache/miss factory8-parameter forwarding and cancellation; prepared factory/jobs/RTTI, masks absent, CPU only"]
fn ger_native_reconstruction_resource_worker_reuses_generations_and_checks_cancel_after_new_values()
{
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/reconstruction");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-reconstruction-resources");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-reconstruction-resources.c"))
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
    for seed in [0u32, 42, 0xdead_beef, u32::MAX] {
        for profile in 0u32..9 {
            input.write_all(&seed.to_le_bytes()).unwrap();
            input.write_all(&profile.to_le_bytes()).unwrap();
            cases.push((seed, profile));
        }
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), cases.len() * 248);
    let heads = [
        [1, 48, 48, 0, 0],
        [1, 40, 40, 21, 0],
        [1, 48, 47, 0, 0],
        [0, 0, 0, 0, 1],
        [0, 1, 1, 0, 1],
        [0, 1, 1, 8, 1],
        // Original has no cancellation check for a cache-hit-only bank loop
        // followed by four absent masks. This fixture proves that boundary.
        [1, 0, 0, 8, 1],
        [1, 41, 41, 70, 0],
        [1, 3, 3, 0, 0],
    ];
    for ((seed, profile), snapshot) in cases.iter().zip(output.stdout.chunks_exact(248)) {
        assert_eq!(&snapshot[..4], b"GRR1");
        let words: Vec<_> = snapshot[4..]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(
            &words[..5],
            &heads[*profile as usize],
            "seed{seed:x}/profile{profile}"
        );
        let mut cache: Vec<Option<u32>> = (0..8)
            .map(|i| match *profile {
                1 | 5 => Some(if i == 1 { 0 } else { i }),
                6 => Some(i),
                7 => {
                    if i == 2 {
                        None
                    } else {
                        Some(if i == 1 { 0 } else { i })
                    }
                }
                _ => None,
            })
            .collect();
        for bank in 0..48 {
            let active = match *profile {
                6 => bank < 8,
                8 => [0, 17, 47].contains(&bank),
                _ => true,
            };
            let visited = match *profile {
                3 => false,
                4 => bank == 0,
                5 => bank < 2,
                _ => active,
            };
            let expected = if !visited {
                u32::MAX
            } else if let Some(index) = super::consume_cache_index(&mut cache, bank % 8) {
                index as u32
            } else if *profile == 2 && bank == 17 {
                u32::MAX
            } else {
                bank + 8
            };
            assert_eq!(
                words[5 + bank as usize],
                expected,
                "seed{seed:x}/profile{profile}/bank{bank}"
            );
        }
        for i in 0..8 {
            // Failed RTTI remains owned by native cache even though the Rust
            // eligibility view cannot use it for a drawable match.
            let expected = if *profile == 7 && i == 2 {
                1
            } else {
                u32::from(cache[i].is_some())
            };
            assert_eq!(words[53 + i], expected);
        }
    }
    println!(
        "Ger native resource worker: {} cases; actual factory parameter guards and native hit/miss/value/cancel branches",
        cases.len()
    );
}
