use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn packed(params: &TransformParams) -> [f32; 23] {
    let mut values = [0.0; 23];
    values[..16].copy_from_slice(&params.transform.to_cols_array());
    values[16..19].copy_from_slice(&params.camera_position.to_array());
    values[19..21].copy_from_slice(&params.unconditionally_inherited_translation.to_array());
    values[21..23].copy_from_slice(&params.wobble_translation.to_array());
    values
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes guarded original/1.03 transform composition and projection; no GPU/audio"]
fn reference_native_transform_composition_and_projection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::temp_dir().join(format!("shin-transform-native-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-transform-compose.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut seed = 0x315c47u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    let mut cases = Vec::new();
    for sample in 0..256 {
        let local = TransformParams {
            transform: Mat4::from_scale_rotation_translation(
                vec3(0.25 + next() * 3.0, 0.25 + next() * 3.0, 1.0),
                glam::Quat::from_rotation_z((next() - 0.5) * 4.0),
                vec3(
                    (next() - 0.5) * 1000.0,
                    (next() - 0.5) * 1000.0,
                    [-1000.0, 0.0, 1.0, 333.0, 1000.0, 2500.0, 10000.0][sample % 7],
                ),
            ),
            camera_position: vec3(
                (next() - 0.5) * 400.0,
                (next() - 0.5) * 400.0,
                (next() - 0.5) * 400.0,
            ),
            unconditionally_inherited_translation: Vec2::new(
                (next() - 0.5) * 100.0,
                (next() - 0.5) * 100.0,
            ),
            wobble_translation: Vec2::new((next() - 0.5) * 50.0, (next() - 0.5) * 50.0),
        };
        let parent = TransformParams {
            transform: Mat4::from_scale_rotation_translation(
                vec3(0.2 + next() * 2.0, 0.2 + next() * 2.0, 1.0),
                glam::Quat::from_rotation_z((next() - 0.5) * 4.0),
                vec3((next() - 0.5) * 800.0, (next() - 0.5) * 800.0, 0.0),
            ),
            camera_position: vec3(
                (next() - 0.5) * 400.0,
                (next() - 0.5) * 400.0,
                [-1000.0, 0.0, 250.0, 999.0][sample % 4],
            ),
            unconditionally_inherited_translation: Vec2::new(
                (next() - 0.5) * 100.0,
                (next() - 0.5) * 100.0,
            ),
            wobble_translation: Vec2::new((next() - 0.5) * 50.0, (next() - 0.5) * 50.0),
        };
        for flags in [0u32, 1, 2, 3, 4, 5, 6, 7, 8, 15, 31, 0x40000007, u32::MAX] {
            cases.push((local, parent, flags));
        }
    }
    let mut request = String::new();
    for (local, parent, flags) in &cases {
        request.push_str(&format!("{flags}"));
        for value in packed(local).into_iter().chain(packed(parent)) {
            request.push_str(&format!(" {:08x}", value.to_bits()));
        }
        request.push('\n');
    }
    let mut report = Vec::new();
    let mut total_mismatches = 0;
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
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = text.lines().collect();
        assert_eq!(rows.len(), cases.len());
        let mut transform_mismatches = 0usize;
        let mut projection_mismatches = 0usize;
        let mut first = None;
        for (index, (row, (local, parent, flags))) in rows.iter().zip(&cases).enumerate() {
            let fields: Vec<_> = row.split_whitespace().collect();
            assert_eq!(fields.len(), 40);
            assert_eq!(fields[0], "M");
            let native: Vec<f32> = fields[1..]
                .iter()
                .map(|s| f32::from_bits(u32::from_str_radix(s, 16).unwrap()))
                .collect();
            let mut actual = *local;
            actual.compose_with(parent, ComposeFlags::from_bits_truncate(*flags as i32));
            for (component, (actual, native)) in
                packed(&actual).into_iter().zip(&native[..23]).enumerate()
            {
                if actual.to_bits() != native.to_bits() {
                    transform_mismatches += 1;
                    first.get_or_insert(format!(
                        "case{index} flags{flags:x} transform{component}:port{:08x}/native{:08x}",
                        actual.to_bits(),
                        native.to_bits()
                    ));
                }
            }
            let mut expected: [f32; 16] = native[23..].try_into().unwrap();
            // The native projection has GL Z in[-1,1]. The host backend requires[0,1].
            for column in 0..4 {
                expected[column * 4 + 2] =
                    expected[column * 4 + 2] * 0.5 + expected[column * 4 + 3] * 0.5;
            }
            for (component, (actual, native)) in actual
                .compute_final_transform()
                .to_cols_array()
                .into_iter()
                .zip(expected)
                .enumerate()
            {
                if actual.to_bits() != native.to_bits() {
                    projection_mismatches += 1;
                    first.get_or_insert(format!(
                        "case{index} flags{flags:x} projection{component}:port{:08x}/native{:08x}",
                        actual.to_bits(),
                        native.to_bits()
                    ));
                }
            }
        }
        let raw = std::fs::read(&executable).unwrap();
        let mutated = folder.join("mutated");
        let mut rejected = 0;
        for address in [
            0x71950,
            0x71b70,
            0xa1990,
            0xa2030,
            0xa2070,
            0xa20b0,
            0xa1c60,
            if updated { 0x11f000 } else { 0x11e000 },
        ] {
            let address = if updated && address < 0x11e000 {
                address - 0xa0
            } else {
                address
            };
            let mut bytes = raw.clone();
            bytes[address + 256] ^= 1;
            std::fs::write(&mutated, bytes).unwrap();
            let output = Command::new(&binary).arg(&mutated).output().unwrap();
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            rejected += 1;
        }
        total_mismatches += transform_mismatches + projection_mismatches;
        eprintln!(
            "{name}:{} cases,transform mismatches{transform_mismatches},projection mismatches{projection_mismatches},{rejected}mutants rejected;first{first:?}",
            cases.len()
        );
        let first_json = first
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "null".into());
        report.push(format!(r#"{{"executable":"{name}","cases":{},"transform_mismatches":{transform_mismatches},"projection_mismatches":{projection_mismatches},"guard_mutants_rejected":{rejected},"first":{first_json}}}"#,cases.len()));
    }
    let report_path = std::env::var("SHIN_REFERENCE_TRANSFORM_REPORT")
        .unwrap_or_else(|_| "/tmp/shin-native-transform-comparison.json".into());
    std::fs::write(report_path, format!("[\n{}\n]\n", report.join(",\n"))).unwrap();
    std::fs::remove_dir_all(folder).unwrap();
    assert_eq!(
        total_mismatches, 0,
        "Native composition/projection binary32 mismatch"
    );
}

#[test]
fn children_use_parent_camera_and_always_inherited_offset() {
    let parent = TransformParams {
        transform: Mat4::IDENTITY,
        camera_position: vec3(100.0, -50.0, 500.0),
        unconditionally_inherited_translation: Vec2::new(5.0, -3.0),
        wobble_translation: Vec2::ZERO,
    };
    let mut child = TransformParams {
        transform: Mat4::from_translation(vec3(10.0, 20.0, 1500.0)),
        camera_position: vec3(777.0, 888.0, 10000.0),
        unconditionally_inherited_translation: Vec2::new(99.0, 9.0),
        wobble_translation: Vec2::ZERO,
    };
    child.compose_with(&parent, ComposeFlags::empty());
    assert_eq!(child.transform.w_axis, Vec4::new(-85.0, 67.0, 0.0, 1.0));
    assert_eq!(child.camera_position, vec3(777.0, 888.0, 10000.0));
    assert_eq!(
        child.unconditionally_inherited_translation,
        Vec2::new(99.0, 9.0)
    );
    let mut grandchild = TransformParams {
        transform: Mat4::from_translation(vec3(0.0, 0.0, 1000.0)),
        ..Default::default()
    };
    grandchild.compose_with(&child, ComposeFlags::all());
    // Even flags that ignore camera, ordinary transforms and wobble keep the
    // parent's unconditional offset. The child's stored offset reaches its child.
    assert_eq!(grandchild.transform.w_axis, Vec4::new(99.0, 9.0, 0.0, 1.0));
}
