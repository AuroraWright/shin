use super::*;
use shin_core::time::{Easing, Tween, TweenOptions};
use std::{
    fs,
    process::{Command, Stdio},
    sync::{Arc, Weak},
};

fn populate(p: &LayerProperties, length: usize, pattern: usize) {
    let mut data = p.inner.lock();
    data.layer_load_counter1 = 17;
    for _ in 0..123 {
        data.updated_frame.advance();
    }
    for (property, tween) in &mut data.properties {
        let base = property as usize as f32 * 8.0;
        tween.fast_forward_to(base);
        if length == 0 {
            continue;
        }
        tween.enqueue_with_options(
            base + 8.0,
            Tween::linear(Ticks::from_f32(8.0)),
            TweenOptions {
                prohibit_fast_forward: pattern == 0,
                ignore_wait: false,
            },
        );
        for j in 0..length {
            tween.enqueue_with_options(
                base + 8.0 * (j + 2) as f32,
                Tween {
                    duration: Ticks::from_f32(if j % 3 == 1 { 0.0 } else { 8.0 }),
                    easing: Easing::Linear,
                },
                TweenOptions {
                    prohibit_fast_forward: j % 5 == pattern,
                    ignore_wait: j % 7 == 0,
                },
            );
        }
    }
    [
        data.wobbler_x,
        data.wobbler_y,
        data.wobbler_rotation,
        data.wobbler_scale_x,
        data.wobbler_scale_y,
        data.wobbler_alpha,
    ] = std::array::from_fn(|i| {
        Wobbler::reference_with_state(0x12345678 + i as u32, i as i32 + 1, i as f32 * 0.125)
    });
}

fn snapshot(op: char, rng: usize, layers: &[Option<LayerProperties>; 4]) -> Vec<String> {
    let mut words = vec!["P".into(), op.to_string(), rng.to_string()];
    let props = layers
        .each_ref()
        .map(|p| p.as_ref().and_then(LayerProperties::initialized_identity));
    for id in 0..4 {
        words.push(usize::from(layers[id].is_some()).to_string());
        words.push(usize::from(props[id].is_some()).to_string());
    }
    for a in 0..4 {
        for b in 0..4 {
            words.push(usize::from(props[a].is_some() && props[a] == props[b]).to_string());
        }
    }
    for layer in layers {
        let cell = layer.as_ref().map(|p| p.inner.cell());
        let state = cell
            .as_ref()
            .and_then(|cell| cell.state.get())
            .map(|state| state.lock());
        for i in 0..90 {
            if let Some(data) = &state {
                let t = data.properties.values().nth(i).unwrap();
                words.extend([
                    format!("{:08x}", t.value().to_bits()),
                    usize::from(!t.is_idle()).to_string(),
                    usize::from(t.is_waiting()).to_string(),
                ]);
            } else {
                words.extend(["00000000".into(), "0".into(), "0".into()]);
            }
        }
    }
    words
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes real original/1.03 lazy property copies, deque traversal and final-owner destructors; CPU only"]
fn reference_native_property_copy_and_final_owner_retirement() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::var_os("SHIN_PROPERTY_COPY_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("shin-native-property-copy-results"));
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join("reverse-engineering/native-property-copy-retirement.c"))
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
    let mut profiles = Vec::new();
    for length in [3, 205, 410] {
        for pattern in 0..5 {
            for reverse_drop in [false, true] {
                profiles.push((true, length, pattern, reverse_drop));
            }
        }
    }
    profiles.extend([
        (true, 0, 0, false),
        (true, 0, 0, true),
        (false, 0, 0, false),
        (false, 0, 0, true),
    ]);
    for &(initialized, length, pattern, reverse_drop) in &profiles {
        let mut layers: [Option<LayerProperties>; 4] = std::array::from_fn(|_| None);
        let mut owners: Vec<Weak<PropertyOwner>> = Vec::new();
        let mut cells: Vec<Weak<PropertyCell>> = Vec::new();
        let mut commands = vec![
            format!("N {} {length} {pattern}", usize::from(initialized)),
            "C 0 1".into(),
            "A 0 2".into(),
            "S 1 3".into(),
            "T 1 2".into(),
            "F 0".into(),
        ];
        if !initialized {
            commands.extend(["I 0".into(), "T 2 3".into()]);
        }
        commands.extend(["T 1 1632".into(), "F 3".into(), "T 2 8".into()]);
        let drops = if reverse_drop {
            [3, 2, 1, 0]
        } else {
            [0, 1, 2, 3]
        };
        commands.push(format!("D {}", drops[0]));
        commands.push(format!("D {}", drops[1]));
        // Continue copied/shared animation after the original holder is gone.
        if !reverse_drop {
            commands.extend(["T 3 1648".into(), "F 2".into()]);
        } else {
            commands.extend(["T 0 1648".into(), "F 1".into()]);
        }
        commands.push(format!("D {}", drops[2]));
        commands.push(format!("D {}", drops[3]));
        for command in commands {
            let fields: Vec<_> = command.split_whitespace().collect();
            let op = fields[0].chars().next().unwrap();
            let before_rng = crate::engine_random::seed_call_count();
            match op {
                'N' => {
                    let p = LayerProperties::new();
                    if initialized {
                        populate(&p, length, pattern);
                    }
                    layers[0] = Some(p);
                }
                'C' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    let b = fields[2].parse::<usize>().unwrap();
                    let p = layers[a].as_ref().unwrap().clone();
                    if initialized {
                        let src = layers[a].as_ref().unwrap().inner.lock();
                        let dst = p.inner.lock();
                        assert_eq!(src.layer_load_counter1, dst.layer_load_counter1);
                        assert_eq!(src.updated_frame, dst.updated_frame);
                        assert_eq!(format!("{:?}", &*src), format!("{:?}", &*dst));
                    }
                    layers[b] = Some(p);
                }
                'A' | 'S' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    let b = fields[2].parse::<usize>().unwrap();
                    layers[b] = Some(if op == 'A' {
                        layers[a].as_ref().unwrap().retain_for_clone()
                    } else {
                        layers[a].as_ref().unwrap().share()
                    });
                }
                'I' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    drop(layers[a].as_ref().unwrap().inner.lock());
                }
                'T' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    let dt = fields[2].parse::<f32>().unwrap();
                    let cell = layers[a].as_ref().unwrap().inner.cell();
                    if let Some(data) = cell.state.get() {
                        for (_, t) in &mut data.lock().properties {
                            t.update(Ticks::from_f32(dt));
                        }
                    }
                }
                'F' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    layers[a].as_mut().unwrap().fast_forward();
                }
                'D' => {
                    let a = fields[1].parse::<usize>().unwrap();
                    layers[a] = None;
                }
                _ => unreachable!(),
            }
            for p in layers.iter().flatten() {
                owners.push(Arc::downgrade(&p.inner));
                cells.push(Arc::downgrade(&p.inner.cell()));
            }
            for owner in &owners {
                let should_live = layers
                    .iter()
                    .flatten()
                    .any(|p| std::ptr::eq(owner.as_ptr(), Arc::as_ptr(&p.inner)));
                assert_eq!(
                    owner.upgrade().is_some(),
                    should_live,
                    "owner release after {command}"
                );
            }
            for cell in &cells {
                let should_live = layers
                    .iter()
                    .flatten()
                    .any(|p| std::ptr::eq(cell.as_ptr(), Arc::as_ptr(&p.inner.cell())));
                assert_eq!(
                    cell.upgrade().is_some(),
                    should_live,
                    "property release after {command}"
                );
            }
            let rng = crate::engine_random::seed_call_count() - before_rng;
            expected.push(snapshot(op, rng, &layers));
            request.push_str(&command);
            request.push('\n');
        }
        assert!(owners.iter().all(|p| p.upgrade().is_none()));
        assert!(cells.iter().all(|p| p.upgrade().is_none()));
    }
    let input = folder.join("request.txt");
    fs::write(&input, &request).unwrap();
    let port = expected
        .iter()
        .map(|words| words.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(folder.join("port.trace"), port).unwrap();
    let mut comparisons = Vec::new();
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let run = |mutant: Option<&str>| {
            let mut cmd = Command::new(&binary);
            cmd.arg(root.join("game-data/umineko").join(executable))
                .stdin(Stdio::from(fs::File::open(&input).unwrap()));
            if let Some(m) = mutant {
                cmd.env("SHIN_PROPERTY_COPY_MUTANT", m);
            }
            cmd.output().unwrap()
        };
        let output = run(None);
        fs::write(folder.join(format!("{version}.trace")), &output.stdout).unwrap();
        fs::write(folder.join(format!("{version}.stderr")), &output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trace = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = trace.lines().collect();
        assert_eq!(rows.len(), expected.len());
        for (row, (line, port)) in rows.iter().zip(&expected).enumerate() {
            let words: Vec<_> = line.split_whitespace().collect();
            assert_eq!(words.len(), port.len() + 1);
            assert_eq!(
                &words[..port.len()],
                port.iter().map(String::as_str).collect::<Vec<_>>(),
                "{version} row {row}"
            );
            if port[3..11].iter().step_by(2).all(|v| v == "0") {
                assert_eq!(words.last(), Some(&"0"));
            }
        }
        for mutant in ["copy", "release", "lazy", "queue", "rng"] {
            let output = run(Some(mutant));
            fs::write(
                folder.join(format!("{version}-{mutant}.trace")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                folder.join(format!("{version}-{mutant}.stderr")),
                &output.stderr,
            )
            .unwrap();
            let different = String::from_utf8_lossy(&output.stdout)
                .lines()
                .zip(&expected)
                .any(|(line, port)| {
                    line.split_whitespace()
                        .take(port.len())
                        .ne(port.iter().map(String::as_str))
                });
            assert!(
                !output.status.success() || different,
                "{version}: undetected mutant {mutant}"
            );
            comparisons.push(format!("{version} mutant {mutant} rejected"));
        }
        comparisons.push(format!(
            "{version}: {} profiles, {} rows, zero mismatches; all final owners released",
            profiles.len(),
            expected.len()
        ));
    }
    fs::write(folder.join("comparison.txt"), comparisons.join("\n") + "\n").unwrap();
}
