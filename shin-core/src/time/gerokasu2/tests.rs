use super::*;
use crate::time::Tweener;

fn easing_words(easing: Easing) -> (u32, i32) {
    match easing {
        Easing::Linear => (0, 0),
        Easing::SineIn => (1, 0),
        Easing::SineOut => (2, 0),
        Easing::SineInOut => (3, 0),
        Easing::Jump => (4, 0),
        Easing::Power(p) => (5, p),
    }
}
fn words(t: &GerTweener) -> Vec<u32> {
    let (ease, param) = easing_words(t.easing);
    vec![
        t.from.to_bits(),
        t.to.to_bits(),
        ease,
        param as u32,
        t.elapsed.to_bits(),
        t.total.to_bits(),
        t.ignore_wait as u32,
        t.prohibit as u32,
        t.value().to_bits(),
        t.target_value().to_bits(),
        t.running() as u32,
        t.waiting() as u32,
        t.queue.len() as u32,
    ]
}

#[test]
fn gerokasu2_public_tweener_policy_preserves_legacy_and_queue_copy() {
    let mut ger = Tweener::new_gerokasu2(0.0);
    let mut legacy = Tweener::new(0.0);
    let linear = Tween::linear(Ticks::from_f32(10.0));
    for t in [&mut ger, &mut legacy] {
        t.enqueue(100.0, linear);
        t.update(Ticks::from_f32(2.0));
    }
    ger.gerokasu2_control(50.0, linear.duration, 128, Easing::Linear, false);
    // The relative control is resolved when it starts, from the previous target.
    assert_eq!(ger.target_value(), 150.0);
    let mut copy = ger.clone();
    copy.update(Ticks::from_f32(13.0));
    assert_eq!(copy.value(), 125.0);
    assert_eq!(ger.value(), 20.0);
    ger.enqueue_now(40.0, linear);
    ger.update(Ticks::from_f32(5.0));
    assert_eq!(ger.value(), 30.0);
    assert_eq!(legacy.value(), 20.0);
    // Native keeps both fast-forward bits together as an ordinary queued record.
    ger.gerokasu2_control(80.0, linear.duration, 0x300, Easing::Linear, false);
    assert_eq!(ger.value(), 30.0);
    assert_eq!(ger.target_value(), 80.0);
    ger.fast_forward_to(1.0e20);
    legacy.fast_forward_to(1.0e20);
    for t in [&mut ger, &mut legacy] {
        t.enqueue(1.0, Tween::IMMEDIATE);
    }
    assert_eq!(ger.target_value(), 1.0);
    assert_eq!(ger.value(), 0.0); // FMA retains the rounded subtraction at completion.
    assert_eq!(legacy.value(), 1.0);
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Tweener>();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original Gerokasu2 property queues; no GPU/audio"]
fn gerokasu2_reference_native_tween_queues() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/tweens");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-tweens");
    let build = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-tweens.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let mut request = String::new();
    let mut expected = Vec::new();
    let mut labels = Vec::new();
    let mut tween = GerTweener::new(0.0);
    let mut record = |command: String, t: &GerTweener| {
        request.push_str(&command);
        request.push('\n');
        expected.push(words(t));
        labels.push(command);
    };
    let curves = [
        Easing::Linear,
        Easing::SineIn,
        Easing::SineOut,
        Easing::SineInOut,
        Easing::Jump,
        Easing::Power(0),
        Easing::Power(2),
        Easing::Power(-2),
        Easing::Power(i32::MIN),
        Easing::Power(i32::MAX),
    ];
    for (case, &base) in [0.0, -0.0, 37.0, -1000.0, 16777216.0, 1.0e20]
        .iter()
        .enumerate()
    {
        for &ease in &curves {
            for ff in [0, 0x100, 0x200, 0x300] {
                for extra in [0, 64, 128, 192, 4096, 4096 | 128, 0x800, 0xe000, 0xfffe0000] {
                    tween.reset(base);
                    record(format!("N {:08x}", base.to_bits()), &tween);
                    for (value, duration, flags, ignore) in [
                        (base + 100.0, 10.0, 0, false),
                        (25.0, 8.0, 128, false),
                        (-17.0, if case % 2 == 0 { -1.0 } else { 0.0 }, 0, true),
                        (60.0, 12.0, 4096, false),
                        (41.0, 4.0, ff | extra, case % 2 != 0),
                    ] {
                        let c = Control {
                            value,
                            duration,
                            flags,
                            easing: ease,
                            ignore_wait: ignore,
                        };
                        tween.control(c);
                        let (id, p) = easing_words(ease);
                        record(
                            format!(
                                "C {:08x} {:08x} {:08x} {:08x} {:08x}",
                                value.to_bits(),
                                duration.to_bits(),
                                flags | id,
                                p as u32,
                                ignore as u32
                            ),
                            &tween,
                        );
                    }
                    for delta in [
                        2.0,
                        0.0,
                        -1.0,
                        f32::NAN,
                        f32::NEG_INFINITY,
                        3.0,
                        5.0,
                        8.0,
                        19.0,
                    ] {
                        tween.advance(delta);
                        record(format!("T {:08x}", delta.to_bits()), &tween);
                        tween.forward_unless_prohibited();
                        record("F".into(), &tween);
                    }
                }
            }
        }
    }
    // Cross multiple native deque blocks and retire allocations in normal clocks.
    tween.reset(-0.0);
    record("N 80000000".into(), &tween);
    for i in 0..700 {
        let value = (i % 17) as f32 - 8.0;
        let flags = if i % 3 == 0 { 128 } else { 0 };
        tween.control(Control {
            value,
            duration: 1.0,
            flags,
            easing: Easing::Linear,
            ignore_wait: false,
        });
        record(
            format!(
                "C {:08x} 3f800000 {flags:08x} 00000000 00000000",
                value.to_bits()
            ),
            &tween,
        );
    }
    for _ in 0..280 {
        tween.advance(2.5);
        record("T 40200000".into(), &tween);
    }
    tween.forward_unless_prohibited();
    record("F".into(), &tween);
    let input = folder.join("request.txt");
    fs::write(&input, &request).unwrap();
    let output = Command::new(&binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::from(fs::File::open(&input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = trace.lines().collect();
    assert_eq!(rows.len(), expected.len());
    let mut failures = Vec::new();
    for (row, (line, values)) in rows.iter().zip(&expected).enumerate() {
        let native: Vec<_> = line
            .split_whitespace()
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        assert_eq!(native.len(), 13);
        for (field, (&n, &p)) in native.iter().zip(values).enumerate() {
            if n != p && !(field == 8 && f32::from_bits(n).is_nan() && f32::from_bits(p).is_nan()) {
                failures.push(format!(
                    "row={row} field={field} command={} native={n:08x} port={p:08x}",
                    labels[row]
                ));
            }
        }
    }
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "mismatches={}\n{}\n",
            failures.len(),
            failures
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    assert!(
        failures.is_empty(),
        "{} mismatches: {:?}",
        failures.len(),
        failures.iter().take(6).collect::<Vec<_>>()
    );
    println!(
        "GEROKASU2_TWEENS_NATIVE snapshots={} words={} deque_controls=700 heap=0",
        expected.len(),
        expected.len() * 13
    );
}
