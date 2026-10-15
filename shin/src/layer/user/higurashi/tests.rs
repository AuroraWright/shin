use super::*;
use crate::layer::{higurashi_properties::INITIAL_VALUES, render_params::multiply_transforms};
use glam::{Mat4, vec2, vec3};
use std::{
    fmt::Write,
    fs,
    process::{Command, Stdio},
};

#[test]
#[ignore = "Requires updated executable/native-tile fixture; no framebuffer/audio/saves"]
fn higurashi_native_tile_color_rectangle_and_projection() {
    use shin_core::primitives::color::FloatColor4;
    use shin_render::LayerFragmentShader;
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_TILE").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut state = 0xb2ca746du32;
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state % 8001) as f32 - 4000.0
    };
    let mut input = String::new();
    let mut cases = Vec::new();
    let mut draws = 0;
    for i in 0..2048 {
        let mut values = INITIAL_VALUES.map(|v| v as f32);
        for index in [
            0, 1, 2, 3, 4, 10, 11, 14, 15, 16, 17, 18, 60, 61, 62, 63, 64,
        ] {
            values[index] = random();
        }
        values[12] = random();
        values[13] = random();
        for index in 6..10 {
            values[index] = random();
        }
        if i % 3 == 0 {
            values[9] = 1000.0;
        }
        values[19] = if i % 19 == 0 { 0.0 } else { 1.0 };
        values[20] = (i % 5) as f32 - 1.0;
        values[21] = (i % 7) as f32;
        values[22] = (i % 8) as f32;
        for index in 23..27 {
            values[index] = random();
        }
        if i % 11 == 0 {
            values[26] = 0.0;
        }
        if i % 13 == 0 {
            values[23..26].fill(0.0);
        }
        // CPU Gamma branches straight to return without loading its color.
        // Only the independently implemented simplification-to-default branch
        // is compared here. Unsimplified Gamma is an explicit renderer error.
        if values[21] == 5.0 {
            values[23..26].fill(1000.0);
        }
        let seeds = [0, 123456789, 0xdeadbeef, 0xfffffff0];
        let phases: [f32; 4] = std::array::from_fn(|_| random() * 0.01);
        let mut properties = HigurashiProperties::from_seeds(seeds);
        for j in 0..4 {
            values[27 + 3 * j] = ((i + j) % 6) as f32;
            values[28 + 3 * j] = random();
            properties.oscillators[j].phase = phases[j];
        }
        for (j, v) in values.iter().enumerate() {
            properties.tweener_mut(j).unwrap().fast_forward_to(*v);
        }
        let parent = HigurashiTransform {
            transform: Mat4::from_scale_rotation_translation(
                vec3(random() * 0.001, random() * 0.001, 1.0),
                glam::Quat::from_rotation_z(random() * 0.001),
                vec3(random(), random(), 0.0),
            ),
            camera: vec3(random(), random(), random()),
            inherited: vec2(random(), random()),
            wobble: vec2(random(), random()),
        };
        let color = if i % 5 == 0 {
            FloatColor4::WHITE
        } else {
            FloatColor4::from_vec4(glam::Vec4::from_array(std::array::from_fn(|_| {
                random() * 0.001
            })))
        };
        let tile = tile::Tile {
            color,
            rect: glam::vec4(random(), random(), random(), random()),
        };
        let opaque = i & 1 != 0;
        write!(input, "T").unwrap();
        for v in values {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        for seed in seeds {
            write!(input, " {seed:08x}").unwrap();
        }
        for v in phases
            .into_iter()
            .chain(parent.transform.to_cols_array())
            .chain(parent.camera.to_array())
            .chain(parent.inherited.to_array())
            .chain(parent.wobble.to_array())
            .chain(color.into_array())
            .chain(tile.rect.to_array())
        {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        writeln!(input, " {}", u32::from(opaque)).unwrap();
        let params = properties.drawable_params();
        let target_opaque = tile.pass_kind(&params) == PassKind::Opaque;
        let submitted = properties.is_visible() && opaque == target_opaque;
        let mut expected = vec![u32::from(submitted); 3];
        if submitted {
            assert_ne!(
                params.fragment_shader.simplify(params.shader_param),
                LayerFragmentShader::Gamma
            );
            draws += 1;
            for vertex in tile.vertices(&params) {
                let position = vertex.position;
                expected.extend(position.to_array().map(f32::to_bits));
                // Native vertex bytes are BGRA; the shared GPU vertex uses RGBA.
                let rgba = vertex.color.0;
                expected.push((rgba & 0xff00ff00) | (rgba & 255) << 16 | ((rgba >> 16) & 255));
            }
            expected.extend(
                properties
                    .composed_transform(&parent)
                    .compute_native_tile_transform()
                    .to_cols_array()
                    .map(f32::to_bits),
            );
            expected.push(match tile.color_blend_type(&params) {
                shin_render::ColorBlendType::Opaque => 1,
                shin_render::ColorBlendType::Layer1 => 2,
                shin_render::ColorBlendType::Layer2 => 3,
                shin_render::ColorBlendType::HigurashiLayer3 => 4,
                value => panic!("Unexpected Higurashi Tile blend {value:?}"),
            });
            expected.push(1);
        }
        cases.push(expected);
    }
    let path = std::env::temp_dir().join(format!("higu-native-tile-{}.txt", std::process::id()));
    fs::write(&path, input).unwrap();
    let output = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(&path).unwrap()))
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "native status {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    let mut words = 0;
    for (i, (line, expected)) in output.lines().zip(cases).enumerate() {
        let mut native = line.split_ascii_whitespace();
        assert_eq!(native.next(), Some("T"));
        let native: Vec<u32> = native
            .map(|w| u32::from_str_radix(w, 16).unwrap())
            .collect();
        assert_eq!(native.len(), expected.len(), "case{i}");
        for (lane, (a, b)) in native.iter().zip(&expected).enumerate() {
            assert_eq!(a, b, "case{i} lane{lane}: native{a:08x} port{b:08x}");
        }
        words += expected.len();
    }
    println!(
        "Higurashi native Tile: 2048 cases, {draws} draw packets, {words} exact words; tint clamp, CPU shader/packed color, logical rectangle, projection and pass selection; no framebuffer/audio/saves"
    );
}

#[test]
#[ignore = "Requires updated executable/native-draw fixture; no framebuffer/audio/saves"]
fn higurashi_native_picture_bustup_draw_packets() {
    let native = std::env::var("SHIN_HIGURASHI_NATIVE_DRAW").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut state = 0x53aa682du32;
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state % 8001) as f32 - 4000.0
    };
    let mut input = String::new();
    let mut cases = Vec::new();
    let mut packet_count = 0;
    for i in 0..2048 {
        let op = if i & 1 == 0 { 'P' } else { 'B' };
        let mut values = INITIAL_VALUES.map(|v| v as f32);
        for index in [0, 1, 3, 4, 10, 11, 14, 15, 17, 18, 60, 61, 62, 63, 64] {
            values[index] = random();
        }
        values[2] = [0.0, 1000.0, 2000.0, -1000.0][i % 4];
        values[12] = random();
        values[13] = random();
        values[16] = random();
        values[19] = if i % 17 == 0 { 0.0 } else { 1.0 };
        for index in 6..10 {
            values[index] = random();
        }
        if i % 3 == 0 {
            values[9] = 1000.0;
        }
        if i % 13 == 0 {
            values[12] = 0.0;
        }
        values[20] = (i % 5) as f32 - 1.0;
        values[21] = (i % 8) as f32 - 1.0;
        values[22] = (i % 16) as f32;
        for index in 23..27 {
            values[index] = if i % 3 == 0 { 1000.0 } else { random() };
        }
        // Include the shader simplification boundaries as well as general values.
        if i % 7 == 0 {
            values[26] = 0.0;
        }
        if i % 11 == 0 {
            values[23..26].fill(0.0);
        }
        let seeds = [0, 123456789, 0xdeadbeef, 0xffffffff];
        let phases: [f32; 4] = std::array::from_fn(|_| random() * 0.01);
        let mut properties = HigurashiProperties::from_seeds(seeds);
        for j in 0..4 {
            values[27 + 3 * j] = ((i + j) % 8) as f32 - 1.0;
            values[28 + 3 * j] = random();
            properties.oscillators[j].phase = phases[j];
        }
        for (j, value) in values.iter().enumerate() {
            properties.tweener_mut(j).unwrap().fast_forward_to(*value);
        }
        let parent = HigurashiTransform {
            transform: Mat4::from_scale_rotation_translation(
                vec3(random() * 0.001, random() * 0.001, 1.0),
                glam::Quat::from_rotation_z(random() * 0.001),
                vec3(random(), random(), 0.0),
            ),
            camera: vec3(random(), random(), random()),
            inherited: vec2(random(), random()),
            wobble: vec2(random(), random()),
        };
        let origin = (random() as i16 * 7, random() as i16 * 7);
        let placement = vec2(random(), random());
        let mouth = (i % 3) as u32;
        let opaque = (i / 2) & 1 != 0;
        let mask = if i % 5 == 0 { 0x3ff } else { i as u32 & 0x3ff };
        write!(input, "{op}").unwrap();
        for v in values {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        for seed in seeds {
            write!(input, " {seed:08x}").unwrap();
        }
        for v in phases
            .into_iter()
            .chain(parent.transform.to_cols_array())
            .chain(parent.camera.to_array())
            .chain(parent.inherited.to_array())
            .chain(parent.wobble.to_array())
        {
            write!(input, " {:08x}", v.to_bits()).unwrap();
        }
        writeln!(
            input,
            " {} {} {:08x} {:08x} {} {} {:x}",
            origin.0,
            origin.1,
            placement.x.to_bits(),
            placement.y.to_bits(),
            mouth,
            u32::from(opaque),
            mask
        )
        .unwrap();

        let mut expected = Vec::new();
        let params = PictureBlockParams::setup(
            if opaque {
                PassKind::Opaque
            } else {
                PassKind::Transparent
            },
            &properties.drawable_params(),
        );
        let submitted = properties.is_visible() && params.is_some();
        let mut count = 0;
        if submitted {
            let params = params.unwrap();
            let transform = properties
                .composed_transform(&parent)
                .compute_native_asset_transform(origin);
            let mut packet = |id: u32, transform: Mat4| {
                count += 1;
                expected.push(id);
                expected.extend(transform.to_cols_array().map(f32::to_bits));
                expected.push(u32::from(opaque));
                expected.push(u32::from(
                    params.blend_type == shin_render::LayerBlendType::Type1
                        && params.color_multiplier.a >= 1.0,
                ));
                expected.extend(
                    params
                        .color_multiplier
                        .into_vec4()
                        .to_array()
                        .map(f32::to_bits),
                );
                expected.push(params.blend_type as u32);
                expected.push(params.fragment_shader as u32);
                expected.extend(params.fragment_shader_param.to_array().map(f32::to_bits));
            };
            if op == 'P' {
                packet(
                    42,
                    multiply_transforms(transform, Mat4::from_translation(vec3(0.0, 0.0, 0.0))),
                );
                packet(
                    42,
                    multiply_transforms(transform, Mat4::from_translation(placement.extend(0.0))),
                );
            } else {
                for id in [1, 3, 4, 5, 6 + mouth, 9] {
                    if mask & (1 << id) != 0 {
                        packet(id, transform);
                    }
                }
            }
        }
        packet_count += count;
        expected.extend([count, u32::from(submitted), u32::from(submitted)]);
        cases.push((op, expected));
    }
    let path = std::env::temp_dir().join(format!("higu-native-draw-{}.txt", std::process::id()));
    fs::write(&path, input).unwrap();
    let output = Command::new(native)
        .arg(exe)
        .stdin(Stdio::from(fs::File::open(&path).unwrap()))
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "native status {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    let mut words = 0;
    for (i, (line, (op, expected))) in output.lines().zip(cases).enumerate() {
        let mut native = line.split_ascii_whitespace();
        assert_eq!(native.next().unwrap(), op.to_string());
        let native: Vec<u32> = native
            .map(|v| u32::from_str_radix(v, 16).unwrap())
            .collect();
        assert_eq!(native.len(), expected.len(), "case {i} {op}");
        for (lane, (a, b)) in native.iter().zip(&expected).enumerate() {
            assert_eq!(
                a, b,
                "case {i} {op}, word {lane}: native {a:08x}, port {b:08x}"
            );
        }
        words += expected.len();
    }
    println!(
        "Higurashi native draw: 2048 PIC/BUP cases, {packet_count} block packets, {words} exact words; matrices, pass/color/blend/shader, sparse traversal/order; no framebuffer/audio/saves"
    );
}
