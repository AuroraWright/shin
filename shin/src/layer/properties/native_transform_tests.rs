use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone)]
struct PropertyCase {
    values: [f32; 90],
    wobblers: [(u32, i32, f32); 6],
    advance: Option<f32>,
}

fn port(case: &PropertyCase) -> (TransformParams, ComposeFlags, [u32; 18]) {
    let mut data = LayerPropertiesData::new();
    for (prop, tween) in &mut data.properties {
        tween.fast_forward_to(case.values[prop as usize]);
    }
    let wobblers = case
        .wobblers
        .map(|(seed, mode, time)| Wobbler::reference_with_state(seed, mode, time));
    [
        data.wobbler_x,
        data.wobbler_y,
        data.wobbler_scale_x,
        data.wobbler_scale_y,
        data.wobbler_rotation,
        data.wobbler_alpha,
    ] = wobblers;
    if let Some(delta) = case.advance {
        data.advance(Ticks::from_f32(delta), false);
    }
    let states = [
        &data.wobbler_x,
        &data.wobbler_y,
        &data.wobbler_scale_x,
        &data.wobbler_scale_y,
        &data.wobbler_rotation,
        &data.wobbler_alpha,
    ]
    .map(Wobbler::reference_state);
    let state_words = std::array::from_fn(|i| match i % 3 {
        0 => states[i / 3].0 as u32,
        1 => states[i / 3].1.to_bits(),
        _ => states[i / 3].2.to_bits(),
    });
    (
        data.get_transform_params(),
        data.get_compose_flags(),
        state_words,
    )
}

fn packed(params: TransformParams) -> [f32; 23] {
    let mut values = [0.0; 23];
    values[..16].copy_from_slice(&params.transform.to_cols_array());
    values[16..19].copy_from_slice(&params.camera_position.to_array());
    values[19..21].copy_from_slice(&params.unconditionally_inherited_translation.to_array());
    values[21..23].copy_from_slice(&params.wobble_translation.to_array());
    values
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 property/tween/oscillator/transform getters; no GPU/audio"]
fn reference_native_layer_property_transform_extraction() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::temp_dir().join(format!("shin-property-native-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-property-transform.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let defaults: [f32; 90] = std::array::from_fn(|i| {
        use num_traits::FromPrimitive;
        LayerProperty::from_usize(i).unwrap().initial_value() as f32
    });
    let inactive = [(1, 0, 0.0); 6];
    let mut cases = vec![PropertyCase {
        values: defaults,
        wobblers: inactive,
        advance: None,
    }];
    for flags in [
        -1000.0,
        -7.0,
        -1.0,
        -0.5,
        0.0,
        0.5,
        1.9,
        7.0,
        8.0,
        15.0,
        31.0,
        2_147_483_648.0,
        4_294_967_296.0,
    ] {
        let mut case = cases[0].clone();
        case.values[25] = flags;
        cases.push(case);
    }
    // Isolate every transform input, including finite fractional tween values.
    for index in [
        0, 1, 2, 3, 4, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 26, 85, 86, 87, 88, 89,
    ] {
        for value in [-1111.125, -0.5, 0.0, 0.5, 997.25, 2111.125] {
            let mut case = cases[0].clone();
            case.values[index] = value;
            cases.push(case);
        }
    }
    let mut rng = 0x7054d0u32;
    let mut next = || {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (rng >> 8) as f32 / 16777216.0
    };
    for sample in 0..256 {
        let mut values = defaults;
        for index in [0, 1, 3, 4, 10, 11, 16, 17, 20, 21, 85, 86, 88, 89] {
            values[index] = (next() - 0.5) * 2400.0;
        }
        for index in [12, 13, 14, 15] {
            values[index] = (next() - 0.35) * 3600.0;
        }
        for index in [18, 19] {
            values[index] = (next() - 0.5) * 3000.0;
        }
        for index in [34, 35, 38, 39, 46, 47, 50, 51, 54, 55] {
            values[index] = (next() - 0.2) * 1300.0;
        }
        values[2] = [-1000.0, 0.0, 1.0, 333.0, 1000.0, 2500.0, 10000.0][sample % 7];
        values[87] = (next() - 0.5) * 1000.0;
        values[26] = [-7.9, -0.5, 0.0, 1.9, 2.5, 3.1, 8.0][sample % 7];
        values[25] = [-1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0][sample % 9];
        for mode in 0..=8 {
            let wobblers = std::array::from_fn(|i| {
                let seed = [0, 1, 0xffff_ffff, 0xffff_fffe, 0x1234_5678, 0x8765_4321][i];
                let phase =
                    [0.0, 0.125, 0.25, 0.5, 0.75, 0.999, 1.125, 3.337, 997.719][(sample + i) % 9];
                (seed, (mode + i as i32) % 9, phase)
            });
            cases.push(PropertyCase {
                values,
                wobblers,
                advance: None,
            });
        }
    }
    let mut steps = Vec::new();
    for (index, case) in cases.iter().skip(152).step_by(18).enumerate() {
        for delta in [-1001.0, -50.0, -1.0, 0.0, 1.0, 10.0, 10001.0] {
            let mut step = case.clone();
            step.advance = Some(delta);
            for (i, mode_index) in [32, 36, 48, 52, 44, 40].into_iter().enumerate() {
                let old_mode = step.wobblers[i].1;
                step.values[mode_index] = match index % 3 {
                    0 => old_mode as f32,
                    1 => ((old_mode + 1) % 9) as f32,
                    _ => 0.0,
                };
                step.values[mode_index + 1] = [1.0, 10.0, 0.0, -10.0, 0.2, 333.33][i];
            }
            steps.push(step);
        }
    }
    let advance_cases = steps.len();
    cases.extend(steps);
    let mut request = String::new();
    for case in &cases {
        for value in case.values {
            request.push_str(&format!("{:08x} ", value.to_bits()));
        }
        for (seed, mode, phase) in case.wobblers {
            request.push_str(&format!("{seed:08x} {mode:08x} {:08x} ", phase.to_bits()));
        }
        request.push_str(&format!(
            "{:08x} {:08x}",
            u32::from(case.advance.is_some()),
            case.advance.unwrap_or(0.0).to_bits()
        ));
        request.push('\n');
    }
    let actual: Vec<_> = cases.iter().map(port).collect();
    let mut report = Vec::new();
    let mut mismatches = 0;
    for (updated, name) in [
        (false, "uncompressed_executable"),
        (true, "uncompressed_executable_update"),
    ] {
        let executable = root.join("umineko_switch_files").join(name);
        let mut child = Command::new(&binary)
            .arg(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = request.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = text.lines().collect();
        assert_eq!(rows.len(), cases.len());
        let mut component_counts = [0usize; 23];
        let mut flag_mismatches = 0;
        let mut oscillator_mismatches = 0;
        let mut first = Vec::new();
        for (index, (row, (params, flags, state_words))) in rows.iter().zip(&actual).enumerate() {
            let fields: Vec<_> = row.split_whitespace().collect();
            assert_eq!(fields.len(), 43);
            assert_eq!(fields[0], "P");
            let native_flags = u32::from_str_radix(fields[1], 16).unwrap() & 7;
            if flags.bits() as u32 != native_flags {
                flag_mismatches += 1;
            }
            for (component, (actual, native)) in
                packed(*params).into_iter().zip(&fields[2..25]).enumerate()
            {
                let native = u32::from_str_radix(native, 16).unwrap();
                if actual.to_bits() != native {
                    component_counts[component] += 1;
                    if first.len() < 12 {
                        first.push(format!(
                            "case{index} word{component} port{:08x}/native{native:08x}",
                            actual.to_bits()
                        ));
                    }
                }
            }
            for (component, (&actual, native)) in state_words.iter().zip(&fields[25..]).enumerate()
            {
                let native = u32::from_str_radix(native, 16).unwrap();
                if actual != native {
                    oscillator_mismatches += 1;
                    if first.len() < 12 {
                        first.push(format!(
                            "case{index} oscillator{component} port{actual:08x}/native{native:08x}"
                        ));
                    }
                }
            }
        }
        let raw = std::fs::read(&executable).unwrap();
        let table = if updated { 0x170e08 } else { 0x16fd48 };
        for (i, value) in defaults.into_iter().enumerate() {
            let actual = i32::from_le_bytes(
                raw[256 + table + i * 4..256 + table + i * 4 + 4]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(
                value, actual as f32,
                "Native property default {i} in {name}"
            );
        }
        let mutated = folder.join("mutated");
        let mut rejected = 0;
        for address in [
            0xbb1c0, 0x754d0, 0x74e20, 0x751e0, 0x754a0, 0xa1c30, 0xa1fb0, 0xa1990, 0xa2030,
            0xa2070, 0xa20b0, 0xa20c0, 0xcd810, 0xc7300, 0xc73c0, 0xc7410, 0xbb4a0, 0x11e000,
            0x757d0, 0xc7310,
        ] {
            let location = if address == 0x11e000 && updated {
                0x11f000
            } else {
                address - if updated { 0xa0 } else { 0 }
            };
            let mut changed = raw.clone();
            changed[256 + location] ^= 1;
            std::fs::write(&mutated, changed).unwrap();
            let result = Command::new(&binary).arg(&mutated).output().unwrap();
            assert_eq!(result.status.code(), Some(2), "{name} mutant{location:x}");
            assert!(result.stdout.is_empty());
            rejected += 1;
        }
        let count = component_counts.iter().sum::<usize>();
        mismatches += count + flag_mismatches + oscillator_mismatches;
        eprintln!(
            "Property native {name}: {} cases ({advance_cases} advanced), {count} transform, {flag_mismatches} flag and {oscillator_mismatches} oscillator mismatches; {rejected} guards rejected; components {component_counts:?}; first {first:?}",
            cases.len()
        );
        report.push(format!("{{\"executable\":\"{name}\",\"cases\":{},\"advance_cases\":{advance_cases},\"transform_mismatches\":{count},\"flag_mismatches\":{flag_mismatches},\"oscillator_mismatches\":{oscillator_mismatches},\"components\":{component_counts:?},\"guard_mutants_rejected\":{rejected},\"first\":{first:?}}}", cases.len()));
    }
    if let Some(path) = std::env::var_os("SHIN_NATIVE_PROPERTY_REPORT") {
        std::fs::write(path, format!("[{}]\n", report.join(","))).unwrap();
    }
    std::fs::remove_dir_all(folder).unwrap();
    assert_eq!(
        mismatches, 0,
        "Native property extraction mismatches; see component diagnostics"
    );
}
