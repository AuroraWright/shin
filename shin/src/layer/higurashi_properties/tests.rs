use super::*;
use std::{
    fmt::Write,
    fs,
    process::{Command, Stdio},
};

#[test]
fn higurashi_property_owner_retains_seeds_and_resets_clocks() {
    let before = crate::engine_random::seed_call_count();
    let mut p = HigurashiProperties::from_seeds([0, 1, 0x80000000, u32::MAX]);
    assert!(p.tweener_mut(65).is_none());
    assert!(p.control(65, 1.0, Ticks::ZERO, 0).is_err());
    assert!(p.control(0, 1.0, Ticks::ZERO, 4).is_err());
    assert_eq!(p.values(), INITIAL_VALUES.map(|v| v as f32));
    p.tweener_mut(28).unwrap().fast_forward_to(100.0);
    p.tweener_mut(29).unwrap().fast_forward_to(20.0);
    p.tweener_mut(27).unwrap().fast_forward_to(2.0);
    p.advance(Ticks::from_f32(5.0));
    assert_eq!(p.oscillators[0].phase, 0.25);
    p.tweener_mut(27).unwrap().fast_forward_to(5.0);
    p.advance(Ticks::ZERO);
    assert_eq!(
        p.oscillators[0].phase, 0.25,
        "Changing waveform keeps the clock"
    );
    assert_eq!(p.samples()[0], 0.25);
    p.tweener_mut(29).unwrap().fast_forward_to(0.0);
    p.advance(Ticks::from_f32(200.0));
    assert_eq!(
        p.oscillators[0].phase, 0.25,
        "Zero period freezes active clock"
    );
    p.tweener_mut(28).unwrap().fast_forward_to(0.0);
    p.advance(Ticks::ZERO);
    assert_eq!(
        p.oscillators[0].phase, 0.0,
        "Zero amplitude resets even with zero period/delta"
    );
    p.oscillators[1].phase = 99.0;
    p.reset();
    assert_eq!(p.values(), INITIAL_VALUES.map(|v| v as f32));
    assert_eq!(p.oscillators.map(|s| s.seed), [0, 1, 0x80000000, u32::MAX]);
    assert_eq!(p.oscillators.map(|s| s.phase), [0.0; 4]);
    assert_eq!(
        crate::engine_random::seed_call_count(),
        before,
        "Older explicit seeds do not touch Umineko random state"
    );
}

#[test]
#[ignore = "Requires supplied updated executable and native-properties fixture; no GPU/audio/saves"]
fn higurashi_native_properties_clocks_waveforms_and_draw_params() {
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_PROPERTIES").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let phases: [f32; 20] = [
        -2000.0, -1001.0, -1000.0, -999.0, -1.75, -1.0, -0.75, -0.0, 0.0, 0.25, 0.5, 0.75, 0.999,
        1.0, 1.25, 2.5, 999.5, 999.99, 1000.5, 1001.0,
    ];
    let modes: [f32; 10] = [-1.0, 0.0, 1.0, 1.9, 2.0, 3.0, 4.0, 5.0, 6.0, 100.0];
    let seeds = [0, 1, 0x7fffffff, 0x80000000, 0xffffffff, 123456789];
    let mut input = String::from("D\nR\n");
    // Device/asset sizing is separate from the 1280x720 logical projection.
    // These native getters are constants, not a handheld/docked mode query.
    let mut expected = vec![vec![
        1920,
        1080,
        (1280.0f32 / 1920.0).to_bits(),
        (720.0f32 / 1080.0).to_bits(),
    ]];
    let mut sampler_count = 0;
    for mode in modes {
        for seed in seeds {
            for phase in phases {
                writeln!(
                    input,
                    "S {:08x} {seed:08x} {:08x}",
                    mode.to_bits(),
                    phase.to_bits()
                )
                .unwrap();
                expected.push(vec![
                    HigurashiOscillator { seed, phase }.sample(mode).to_bits(),
                ]);
                sampler_count += 1;
            }
        }
    }
    let mut seed = 0x67891234u32;
    let mut random = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed
    };
    for i in 0..1536 {
        let mut p =
            HigurashiProperties::from_seeds(std::array::from_fn(|j| seeds[(i + j) % seeds.len()]));
        for j in 0..PROPERTY_COUNT {
            p.tweener_mut(j)
                .unwrap()
                .fast_forward_to((random() % 4001) as f32 - 2000.0);
        }
        for j in 0..4 {
            p.oscillators[j].phase = phases[(i + j * 3) % phases.len()];
            p.tweener_mut(27 + 3 * j)
                .unwrap()
                .fast_forward_to(modes[(i + j) % modes.len()]);
            p.tweener_mut(28 + 3 * j)
                .unwrap()
                .fast_forward_to([0.0, 0.25, 1000.0, -2.0][(i + j) % 4]);
            p.tweener_mut(29 + 3 * j)
                .unwrap()
                .fast_forward_to([0.0, 0.25, -60.0, 10.0, 1000.0][(i + j) % 5]);
        }
        p.tweener_mut(19)
            .unwrap()
            .fast_forward_to([0.0, 0.5, 1.0, -1.0][i % 4]);
        p.tweener_mut(20)
            .unwrap()
            .fast_forward_to((i % 6) as f32 - 1.0);
        p.tweener_mut(21)
            .unwrap()
            .fast_forward_to((i % 9) as f32 - 1.0);
        p.tweener_mut(22).unwrap().fast_forward_to((i % 8) as f32);
        p.tweener_mut(9)
            .unwrap()
            .fast_forward_to([0.0, 0.5, 1.0, 1000.0, 2000.0, -1000.0][i % 6]);
        let delta = Ticks::from_f32([-60.0, 0.0, 0.125, 60.0, 2000.0][i % 5]);
        let steps = i % 7;
        input.push('U');
        for v in p.values() {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        for s in p.oscillators {
            write!(input, " {:08x}", s.seed).unwrap();
        }
        for s in p.oscillators {
            write!(input, " {:08x}", s.phase.to_bits()).unwrap();
        }
        writeln!(input, " {:08x} {steps}", delta.as_f32().to_bits()).unwrap();
        for _ in 0..steps {
            p.advance(delta);
        }
        let mut words = p.oscillators.map(|s| s.phase).to_vec();
        words.extend(p.samples());
        let t = p.composed_transform(&HigurashiTransform::default());
        words.extend(t.transform.to_cols_array());
        words.extend(t.camera.to_array());
        words.extend(t.inherited.to_array());
        words.extend(t.wobble.to_array());
        words.extend(t.compute_native_final_transform().to_cols_array());
        let d = p.drawable_params();
        words.extend(d.color_multiplier.into_vec4().to_array());
        words.extend(d.shader_param.to_array());
        let mut words: Vec<u32> = words.into_iter().map(f32::to_bits).collect();
        words.extend([
            d.blend_type as u32,
            d.fragment_shader as u32,
            u32::from(p.is_visible()),
        ]);
        expected.push(words);
    }
    let path =
        std::env::temp_dir().join(format!("higu-native-properties-{}.txt", std::process::id()));
    fs::write(&path, input).unwrap();
    let out = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(path).unwrap()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Native property oracle {:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let mut lines = text.lines();
    let defaults: Vec<i32> = lines
        .next()
        .unwrap()
        .split_ascii_whitespace()
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    assert_eq!(defaults, INITIAL_VALUES);
    let lines: Vec<_> = lines.collect();
    assert_eq!(lines.len(), expected.len());
    for (i, (line, expected)) in lines.into_iter().zip(expected).enumerate() {
        let actual: Vec<u32> = line
            .split_ascii_whitespace()
            .skip(1)
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        assert_eq!(actual.len(), expected.len(), "Case {i} output size");
        for (lane, (native, port)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(
                *native, port,
                "Case {i}, lane {lane}: native {native:08x}, port {port:08x}"
            );
        }
    }
    println!(
        "Higurashi native properties: 65 defaults + 4 sizing words + {sampler_count} waveform samples + 1536 full clock/property/draw cases exact; no GPU/audio/saves"
    );
}

#[test]
#[ignore = "Requires updated executable/native-tweens fixture; no GPU/audio/saves"]
fn higurashi_native_tween_control_queues_and_curves() {
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_TWEENS").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut record = |p: &HigurashiProperties, line: String| {
        input.push_str(&line);
        input.push('\n');
        let t = p.tweener(0).unwrap();
        expected.push((t.value().to_bits(), u32::from(t.is_waiting())));
    };
    let durations = [-1.0f32, 0.0, 0.1, 0.25, 1.0, 2.5, 10.0, 30.0];
    for i in 0..1024 {
        let base = (i % 17) as f32 * 13.25 - 100.0;
        let mut p = HigurashiProperties::from_seeds([1, 2, 3, 4]);
        p.tweener_mut(0).unwrap().fast_forward_to(base);
        record(&p, format!("N {:08x}", base.to_bits()));
        for j in 0..5 {
            let value = ((i * 37 + j * 13) % 401) as f32 * 0.125 - 20.0;
            let time = durations[(i + j * 3) % durations.len()];
            let flags = ((i + j) % 4) as u32
                | ((((i >> (j % 3)) & 15) as u32) << 6)
                | if i % 7 == 0 { 0x400 } else { 0 };
            p.control(0, value, Ticks::from_f32(time), flags).unwrap();
            record(
                &p,
                format!("E {flags:x} {:08x} {:08x}", value.to_bits(), time.to_bits()),
            );
            if j == 1 {
                p.advance(Ticks::from_f32(0.125));
                record(&p, format!("T {:08x}", 0.125f32.to_bits()));
            }
        }
        for delta in [-1.0f32, 0.0, -0.0, f32::NAN, 0.1, 0.25, 1.0, 10.0, 100.0] {
            p.advance(Ticks::from_f32(delta));
            record(&p, format!("T {:08x}", delta.to_bits()));
            p.fast_forward();
            record(&p, "F".to_owned());
        }
        p.tweener_mut(0).unwrap().fast_forward_to(base);
        record(&p, format!("Z {:08x}", base.to_bits()));
    }
    // An immediate queued control reached at an exact time boundary becomes
    // the active completed endpoint; its successor waits for another update.
    let mut p = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    record(&p, "N 00000000".to_owned());
    for (value, time) in [(100.0f32, 10.0f32), (200.0, 0.0), (300.0, 10.0)] {
        p.control(0, value, Ticks::from_f32(time), 0).unwrap();
        record(
            &p,
            format!("E 0 {:08x} {:08x}", value.to_bits(), time.to_bits()),
        );
    }
    for delta in [10.0f32, 1.0, 9.0] {
        p.advance(Ticks::from_f32(delta));
        record(&p, format!("T {:08x}", delta.to_bits()));
    }
    p.fast_forward();
    record(&p, "F".to_owned());
    let path = std::env::temp_dir().join(format!("higu-native-tweens-{}.txt", std::process::id()));
    fs::write(&path, input).unwrap();
    let out = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(path).unwrap()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Native tween oracle {:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let output = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), expected.len());
    let mut mismatches = Vec::new();
    for (i, (line, expected)) in lines.iter().zip(expected).enumerate() {
        let parts: Vec<_> = line.split_ascii_whitespace().collect();
        assert_eq!(parts.len(), 4);
        let actual = (
            u32::from_str_radix(parts[2], 16).unwrap(),
            u32::from_str_radix(parts[3], 16).unwrap(),
        );
        if actual != expected && mismatches.len() < 10 {
            mismatches.push(format!(
                "row{i} {line}: port {:08x} {}",
                expected.0, expected.1
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    println!(
        "Higurashi native tweens: 1025 control sequences, {} exact value/busy rows; four curves, relative/speed/replacement/protection, upper bit10, signed/NaN delta; no GPU/audio/saves",
        lines.len()
    );
}

#[test]
#[ignore = "Requires updated executable/native-tweens fixture; no GPU/audio/saves"]
fn higurashi_native_queued_properties_feed_oscillator_clocks() {
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_TWEENS").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut record = |p: &HigurashiProperties, line: String| {
        input.push_str(&line);
        input.push('\n');
        let mut words = p.values().map(f32::to_bits).to_vec();
        words.extend(p.oscillators.map(|s| s.phase.to_bits()));
        words.extend((0..65).map(|i| u32::from(p.tweener(i).unwrap().is_waiting())));
        expected.push(words);
    };
    for i in 0..64 {
        let mut p = HigurashiProperties::from_seeds([0, 1, 0x80000000, u32::MAX]);
        record(&p, "M 0 1 80000000 ffffffff".to_owned());
        for (j, index) in [0, 28, 29, 31, 32, 34, 35, 37, 38, 28, 29]
            .into_iter()
            .enumerate()
        {
            let value = ((i * 37 + j * 13) % 81) as f32 * 0.125 - 5.0;
            let time = [0.0f32, 1.0, 2.5, 10.0][(i + j) % 4];
            let flags = ((i + j) % 4) as u32 | ((((i >> (j % 3)) & 15) as u32) << 6);
            p.control(index, value, Ticks::from_f32(time), flags)
                .unwrap();
            record(
                &p,
                format!(
                    "C {index} {flags:x} {:08x} {:08x}",
                    value.to_bits(),
                    time.to_bits()
                ),
            );
            if j % 3 == 2 {
                p.advance(Ticks::from_f32(0.25));
                record(&p, format!("T {:08x}", 0.25f32.to_bits()));
            }
        }
        p.fast_forward();
        record(&p, "F".to_owned());
        for delta in [-1.0f32, -0.0, 0.125, 1.75, 60.0, 100.0] {
            p.advance(Ticks::from_f32(delta));
            record(&p, format!("T {:08x}", delta.to_bits()));
        }
        p.fast_forward();
        record(&p, "F".to_owned());
    }
    let path = std::env::temp_dir().join(format!(
        "higu-native-queued-properties-{}.txt",
        std::process::id()
    ));
    fs::write(&path, input).unwrap();
    let out = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(path).unwrap()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Native queued properties {:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let output = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (i, (line, expected)) in lines.iter().zip(expected).enumerate() {
        let actual: Vec<_> = line
            .split_ascii_whitespace()
            .skip(1)
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        assert_eq!(actual.len(), 134);
        for (lane, (native, port)) in actual.into_iter().zip(expected).enumerate() {
            assert_eq!(
                native, port,
                "row{i} lane{lane}: native {native:08x}, port {port:08x}"
            );
        }
    }
    println!(
        "Higurashi queued properties: 64 full-owner sequences, {} rows, all65 values/busy states +4 clocks exact; no GPU/audio/saves",
        lines.len()
    );
}
