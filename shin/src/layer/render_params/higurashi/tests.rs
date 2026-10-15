use super::*;
use std::{
    fmt::Write,
    fs,
    process::{Command, Stdio},
};

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn higurashi_rotation_matches_original_separate_trigonometric_services() {
    // Actual updated74ed0 execution, with the same checked Darwin sinf/cosf
    // service boundary as the retained native-transform/Tile oracles. First
    // angle is native Tilecase74; optimized sin_cos gives bf268fa3 instead.
    for (angle, sine, cosine) in [
        (0x41849012, 0xbf4267bf, 0xbf268fa4),
        (0x00000000, 0x00000000, 0x3f800000),
        (0x80000000, 0x80000000, 0x3f800000),
        (0x40c90fdb, 0x343bbd2e, 0x3f800000),
        (0xc18a1234, 0x3f7ff310, 0xbca2c680),
    ] {
        let words = native_rotation_z(f32::from_bits(angle))
            .to_cols_array()
            .map(f32::to_bits);
        assert_eq!(words[0], cosine, "angle{angle:08x}:native cosine");
        assert_eq!(words[1], sine, "angle{angle:08x}:native sine");
        assert_eq!(words[4], sine ^ 0x80000000, "angle{angle:08x}:native -sine");
        assert_eq!(words[5], cosine, "angle{angle:08x}:native cosine column1");
    }
}

#[test]
fn higurashi_bottom_center_canvas_and_older_flags() {
    let transform = HigurashiTransform::default().compute_final_transform();
    let close =
        |a: Vec3, b: Vec3| assert!((a - b).abs().max_element() < 0.000001, "{a:?} != {b:?}");
    close(
        transform.transform_point3(vec3(-640.0, -720.0, 0.0)),
        vec3(-1.0, 1.0, 0.5),
    );
    close(
        transform.transform_point3(vec3(640.0, 0.0, 0.0)),
        vec3(1.0, -1.0, 0.5),
    );
    let parent = HigurashiTransform {
        transform: Mat4::from_translation(vec3(50.0, 100.0, 0.0)),
        camera: vec3(0.0, 0.0, 1000.0),
        inherited: vec2(20.0, 40.0),
        wobble: vec2(2.0, 3.0),
    };
    let local = HigurashiTransform {
        transform: Mat4::from_translation(vec3(10.0, 20.0, 1000.0)),
        ..Default::default()
    };
    let mut bypass = local;
    bypass.compose_with(&parent, 1);
    assert_eq!(bypass.transform.w_axis.truncate(), vec3(30.0, 60.0, 1000.0));
    assert_eq!(bypass.wobble, parent.wobble);
    let mut normal = local;
    normal.compose_with(&parent, 0);
    assert_eq!(normal.transform.w_axis.truncate(), vec3(75.0, 150.0, 0.0));
    let mut suppress = local;
    suppress.compose_with(&parent, 2);
    assert_eq!(suppress.transform, normal.transform);
    assert_eq!(suppress.wobble, Vec2::ZERO);
    let mut high_bit = local;
    high_bit.compose_with(&parent, 4);
    assert_eq!(
        high_bit, normal,
        "Older bit2 does not suppress parent wobble"
    );
}

#[test]
#[ignore = "Requires the supplied updated executable and compiled native-transform fixture; no GPU/audio/saves"]
fn higurashi_native_property_transform_and_projection() {
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_TRANSFORM").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut seed = 0x23456789u32;
    let mut random = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed % 8001) as f32 - 4000.0
    };
    let mut cases = Vec::new();
    let mut input = String::new();
    let mut add = |mode: char, values: &[f32], expected: HigurashiTransform| {
        write!(input, "{mode}").unwrap();
        for v in values {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        input.push('\n');
        cases.push((mode, expected));
    };
    for i in 0..3072 {
        let mut p = [0.0; PROPERTY_COUNT];
        for v in &mut p {
            *v = random();
        }
        // Mode0 at phase0 yields zero; mode5 returns the signed fractional
        // phase. Both execute the native sampler and complete extraction.
        for index in [27, 30, 33, 36] {
            p[index] = if i < 2048 { 0.0 } else { 5.0 };
        }
        p[22] = (i % 16) as f32;
        if i % 16 == 0 {
            p[22] = -1.0;
        }
        if i % 16 == 1 {
            p[22] = 1.75;
        }
        p[2] = [-1000.0, 0.0, 1000.0, 2000.0, random()][i % 5];
        let mut parent = HigurashiTransform::default();
        parent.transform = Mat4::from_scale(vec3(random() * 0.001, random() * 0.001, 1.0));
        parent.transform.w_axis.x = random();
        parent.transform.w_axis.y = random();
        parent.camera = vec3(random(), random(), [0.0, 1000.0, -1000.0, random()][i % 4]);
        parent.inherited = vec2(random(), random());
        parent.wobble = vec2(random(), random());
        let phases = if i < 2048 {
            [0.0; 3]
        } else {
            [random() * 0.001, random() * 0.001, random() * 0.001]
        };
        let mut expected = HigurashiTransform::from_properties(&p, phases.map(f32::fract));
        expected.compose_with(&parent, HigurashiTransform::compose_flags(&p));
        let mut values = p.to_vec();
        values.extend(parent.words());
        values.extend(phases);
        add('P', &values, expected);
    }
    for _ in 0..512 {
        let values = std::array::from_fn(|_| random());
        add('F', &values, HigurashiTransform::from_words(values));
    }
    let input_path =
        std::env::temp_dir().join(format!("higu-native-transform-{}.txt", std::process::id()));
    fs::write(&input_path, input).unwrap();
    let output = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(input_path).unwrap()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native oracle status {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    for (i, (line, (mode, expected))) in output.lines().zip(cases).enumerate() {
        let mut words = line.split_ascii_whitespace();
        assert_eq!(words.next().unwrap(), mode.to_string());
        let words: Vec<u32> = words.map(|v| u32::from_str_radix(v, 16).unwrap()).collect();
        let expected: Vec<u32> = expected
            .words()
            .into_iter()
            .chain(expected.compute_native_final_transform().to_cols_array())
            .map(f32::to_bits)
            .collect();
        assert_eq!(words.len(), expected.len());
        for (lane, (native, port)) in words.iter().zip(expected).enumerate() {
            assert_eq!(
                *native, port,
                "case {i} {mode}, lane {lane}: native {native:08x}, port {port:08x}"
            );
        }
    }
    println!(
        "Higurashi native transforms: 3072 frozen property/parent cases (2048 zero phase + 1024 nonzero mode5 phase) + 512 raw projection cases; all 23 state and 16 matrix words exact; no GPU/audio/saves"
    );
}
