//! Native Replay renderer packets, including native parent composition, are the
//! independent geometry/UV/color/matrix oracle for the production child.
use super::*;
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn draw_native_replay(layer: &MurdererAccusationView, pass: &mut RenderPass, captured: &str) {
    use shin_render::shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    };
    use shin_render::{DrawPrimitive, RenderProgramWithArguments};
    let inner = layer.inner_ref();
    for line in captured.lines().filter(|line| line.starts_with("Q ")) {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f.len(), 71);
        let scalar = |i: usize| f[i].parse::<f32>().unwrap();
        let texture_for = |name: &str| match name {
            "different_space_2d" => &inner.textures._different_space_2d,
            "back" => &inner.textures._back,
            "thumb_1_s" => &inner.textures._thumb_1_s,
            "thumb_2_s" => &inner.textures._thumb_2_s,
            "thumb_4_s" => &inner.textures._thumb_4_s,
            "thumb_56_s" => &inner.textures._thumb_56_s,
            "thumb_7_s" => &inner.textures._thumb_7_s,
            "thumb_8_s" => &inner.textures._thumb_8_s,
            "thumb_1" => &inner.textures._thumb_1,
            "thumb_2" => &inner.textures._thumb_2,
            "thumb_4" => &inner.textures._thumb_4,
            "thumb_56" => &inner.textures._thumb_56,
            "thumb_7" => &inner.textures._thumb_7,
            "thumb_8" => &inner.textures._thumb_8,
            name => panic!("Unexpected native Replay texture {name}"),
        };
        let texture = texture_for(f[1]);
        assert_eq!(f[3], "0");
        assert_eq!(&f[54..61], &["1", "6", "1", "0", "255", "255", "2"]);
        let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| {
            let o = 4 + 6 * i;
            let argb = f[o + 3].parse::<u32>().unwrap();
            PosColTexVertex {
                position: glam::vec3(scalar(o), scalar(o + 1), scalar(o + 2)),
                color: UnormColor::from_rgba(
                    (argb >> 16) as u8,
                    (argb >> 8) as u8,
                    argb as u8,
                    (argb >> 24) as u8,
                ),
                texture_position: Vec2::new(scalar(o + 4), scalar(o + 5)),
            }
        });
        let mut matrix: [f32; 16] = std::array::from_fn(|i| scalar(28 + i));
        for column in 0..4 {
            matrix[column * 4 + 2] = matrix[column * 4 + 2] * 0.5 + matrix[column * 4 + 3] * 0.5;
        }
        let matrix = Mat4::from_cols_array(&matrix);
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(1, true, true)
            .color_blend_type(match f[53] {
                "1" => ColorBlendType::Opaque,
                "2" => ColorBlendType::Layer1,
                v => panic!("Native blend {v}"),
            });
        let fill_vertices: [PosColVertex; 4] = vertices.map(|v| PosColVertex {
            position: v.position,
            color: v.color,
        });
        let blend_vertices = std::array::from_fn::<_, 4, _>(|i| {
            shin_render::shaders::types::vertices::BlendVertex {
                position: vertices[i].position,
                color: vertices[i].color,
                texture_position: vec4(
                    vertices[i].texture_position.x,
                    vertices[i].texture_position.y,
                    scalar(63 + 2 * i),
                    scalar(64 + 2 * i),
                ),
            }
        });
        let program = match f[2] {
            "1" => RenderProgramWithArguments::Fill {
                vertices: VertexSource::VertexData {
                    vertices: &fill_vertices,
                },
                transform: matrix,
            },
            "2" => RenderProgramWithArguments::Sprite {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                sprite: texture.as_source(),
                transform: matrix,
            },
            "6" => {
                assert_eq!(f[62], "0");
                RenderProgramWithArguments::Blend {
                    vertices: VertexSource::VertexData {
                        vertices: &blend_vertices,
                    },
                    texture1: texture.as_source(),
                    texture2: texture_for(f[61]).as_source(),
                    transform: matrix,
                    blend: vec4(scalar(44), scalar(45), scalar(46), scalar(47)),
                    flash: vec4(scalar(48), scalar(49), scalar(50), scalar(51)),
                }
            }
            "5" => RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: texture.as_source(),
                transform: matrix,
                flash1: vec4(scalar(44), scalar(45), scalar(46), scalar(47)),
                flash2: vec4(scalar(48), scalar(49), scalar(50), scalar(51)),
            },
            v => panic!("Native program {v}"),
        };
        pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
    }
}

pub(crate) fn reference_replay_draw(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
    use num_traits::FromPrimitive;
    use shin_render::shaders::types::texture::TextureTargetKind;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let executable = root.join("umineko_switch_files").join(if updated {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    });
    let binary = std::env::temp_dir().join(format!("shin-murderer-replay-{}", std::process::id()));
    let result = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-replay-draw.c"))
        .arg("-lm")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut cases = Vec::new();
    let mut request = String::new();
    for profile in 0..6 {
        let mut values = std::array::from_fn::<_, 90, _>(|i| {
            LayerProperty::from_usize(i).unwrap().initial_value() as f32
        });
        let parent = match profile {
            0 => TransformParams::default(),
            1 => {
                values[0] = 147.25;
                values[1] = -81.5;
                values[12] = 1230.;
                values[13] = 870.;
                values[18] = 125.;
                TransformParams {
                    transform: Mat4::from_translation(glam::vec3(-93., 57., 0.)),
                    camera_position: glam::vec3(30., -10., 125.),
                    wobble_translation: Vec2::new(13., -7.),
                    unconditionally_inherited_translation: Vec2::new(19., 23.),
                }
            }
            2 => {
                values[0] = -234.5;
                values[1] = 98.;
                values[2] = 2150.;
                values[14] = 830.;
                values[15] = 1310.;
                values[19] = -90.;
                TransformParams {
                    transform: Mat4::from_rotation_z(0.23),
                    camera_position: glam::vec3(-64., 82., 350.),
                    wobble_translation: Vec2::new(-27., 43.),
                    unconditionally_inherited_translation: Vec2::new(31., -41.),
                }
            }
            3 => {
                values[0] = 23.75;
                values[1] = -42.125;
                values[10] = 133.5;
                values[11] = -75.25;
                values[12] = 750.;
                values[13] = 1200.;
                values[14] = 1100.;
                values[15] = 900.;
                TransformParams::default()
            }
            4 => {
                values[26] = 3.;
                values[18] = 25.;
                values[19] = -11.;
                values[20] = 17.5;
                values[21] = -9.25;
                TransformParams::default()
            }
            _ => {
                values[0] = 0.125;
                values[1] = 0.375;
                values[12] = 997.;
                values[13] = 1003.;
                TransformParams::default()
            }
        };
        let feedbacks: [([f32; 6], [f32; 6], f32); 4] = [
            ([1., 0., 0., 0., 0., 0.], [0.; 6], 0.),
            ([0.; 6], [0.; 6], 0.2),
            ([1.; 6], [0.; 6], 0.7),
            (
                [0.125, 0.333333, 0.7, 0.999, 0.001, 0.875],
                [0.125, 0., 0.333, 0.7, 0.999, 1.],
                1.,
            ),
        ];
        let mut states = Vec::new();
        for (focus, flash, back) in feedbacks {
            states.push((0u32, 0usize, 0f32, focus, flash, back));
        }
        for selected in 0..6 {
            let focus = std::array::from_fn(|i| if i == selected { 1. } else { 0. });
            let flash = std::array::from_fn(|i| if i == selected { 0.7 } else { 0. });
            states.push((1, selected, 0., focus, flash, 0.));
            for amount in [0., 0.001, 0.125, 0.333333, 0.7, 0.999, 1.] {
                // Native phase2 keeps drawing the transition while its timer
                // advances. Focus animates independently during the exit.
                let focus = std::array::from_fn(|i| if i == selected { 1. - amount } else { 0. });
                states.push((2, selected, amount, focus, [0.; 6], 0.));
            }
            states.push((3, selected, 1., focus, [0.; 6], 0.));
        }
        for (phase, selected, amount, focus, flash, back) in states {
            request += &format!(
                "{phase} {selected} {:x} {:x} ",
                back.to_bits(),
                amount.to_bits()
            );
            for (f, s) in focus.into_iter().zip(flash) {
                request += &format!("{:x} {:x} ", f.to_bits(), s.to_bits());
            }
            for v in values {
                request += &format!("{:x} ", v.to_bits());
            }
            request += "11 0 0 ".repeat(6).as_str();
            request += "0 0 ";
            for v in parent
                .transform
                .to_cols_array()
                .into_iter()
                .chain(parent.camera_position.to_array())
                .chain(parent.unconditionally_inherited_translation.to_array())
                .chain(parent.wobble_translation.to_array())
            {
                request += &format!("{:x} ", v.to_bits());
            }
            request += "\n";
            cases.push((
                profile, phase, selected, amount, focus, flash, back, values, parent,
            ));
        }
    }
    let mut child = Command::new(&binary)
        .arg(&executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = request.into_bytes();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native = String::from_utf8(output.stdout).unwrap();
    let frames: Vec<_> = native.split("V ").skip(1).collect();
    assert_eq!(frames.len(), cases.len());
    std::fs::write(
        std::env::temp_dir().join(if updated {
            "shin-murderer-replay-native-1.03.txt"
        } else {
            "shin-murderer-replay-native-original.txt"
        }),
        &native,
    )
    .unwrap();
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Replay native child comparison"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut worst_all = 0;
    for ((profile, phase, selected, amount, focus, flash, back, values, parent), native) in
        cases.iter().zip(frames)
    {
        let packet_count = 8 + focus.iter().filter(|v| **v > 0.).count() + usize::from(*phase >= 2);
        assert_eq!(
            native.lines().next().unwrap(),
            format!("{packet_count} 2 2 3 1 1")
        );
        let mut layer = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        layer
            .properties_mut()
            .reference_install_native_state(values, &[(17, 0, 0.); 6], None);
        {
            let mut model = layer.inner_ref().model.lock().unwrap();
            model.wipe = None;
            model.reference_set_replay_drawing(*phase, *selected, *amount, *focus, *flash, *back);
        }
        layer.pre_render(context, parent);
        let render = |context: &mut PreRenderContext, captured: bool| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Replay native frame",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            if captured {
                draw_native_replay(&layer, &mut pass, native);
            } else {
                layer.render(&mut pass, parent, 4, PassKind::Opaque);
            }
            drop(pass);
            crate::layer::user::TileLayer::readback_reference_texture(context, &target)
        };
        let actual = render(context, false);
        let expected = render(context, true);
        let visible = expected
            .chunks_exact(4)
            .filter(|p| p[..3].iter().any(|v| *v != 0))
            .count();
        assert!(
            visible > 10_000,
            "profile{profile}: native draw must be visible"
        );
        let worst = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
            .max()
            .unwrap();
        worst_all = worst_all.max(worst);
        eprintln!(
            "Replay profile{profile} phase{phase} row{selected} amount{amount}: {visible} visible pixels, maxRGBAerror{worst}"
        );
        assert_eq!(
            worst, 0,
            "Replay profile{profile} phase{phase} row{selected} amount{amount}: native child frame mismatch"
        );
    }
    for at in [0x8e710usize, 0x9b250, 0xc7170] {
        let mut bytes = std::fs::read(&executable).unwrap();
        bytes[256 + at - if updated { 0xa0 } else { 0 }] ^= 1;
        let path = std::env::temp_dir().join(format!("shin-replay-mutant-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let result = Command::new(&binary).arg(&path).output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_file(binary).unwrap();
    println!(
        "Replay unchanged native constructor+child+parent:{}frames/{}packets/{}pixels,6transforms x idle/flash/transition/completed profiles, maxRGBAerror{worst_all};3whole-image mutants rejected",
        cases.len(),
        native.lines().filter(|line| line.starts_with("Q ")).count(),
        cases.len() * 1920 * 1080
    );
}
