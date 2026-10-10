//! Native Top renderer packets, including native parent composition, are the
//! independent geometry/UV/color/matrix oracle for the production child.
use super::*;
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn draw_native_top(layer: &MurdererAccusationView, pass: &mut RenderPass, captured: &str) {
    use shin_render::shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    };
    use shin_render::{DrawPrimitive, RenderProgramWithArguments};
    let inner = layer.inner_ref();
    for line in captured.lines().filter(|line| line.starts_with("Q ")) {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f.len(), 61);
        let scalar = |i: usize| f[i].parse::<f32>().unwrap();
        let texture = match f[1] {
            "different_space_2d" => &inner.textures._different_space_2d,
            "Ber_Body1" => &inner.textures._ber_body1,
            "Ber_Body2" => &inner.textures._ber_body2,
            "Ber_Body3" => &inner.textures._ber_body3,
            "Ber_Akuwarai2" => &inner.textures._ber_akuwarai2,
            "Ber_AkuwaraiA1" => &inner.textures._ber_akuwaraia1,
            "plate" => &inner.textures._plate,
            "topmenu" => &inner.textures._topmenu,
            name => panic!("Unexpected native Top texture {name}"),
        };
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

pub(crate) fn reference_top_draw(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
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
    let binary = std::env::temp_dir().join(format!("shin-murderer-top-{}", std::process::id()));
    let result = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-top-draw.c"))
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
        for art in [false, true] {
            for feedback in 0..5 {
                let (focus, flash) = match feedback {
                    0 => ([1., 0., 0., 0., 0., 0.], [0.; 6]),
                    1 => ([0.; 6], [0.; 6]),
                    2 => ([1.; 6], [0.; 6]),
                    3 => ([0.2, 0.4, 0.6, 0.8, 1., 0.], [0., 0.2, 0.7, 1., 0.4, 0.5]),
                    _ => (
                        [0.125, 0.333333, 0.7, 0.999, 0.001, 0.875],
                        [0.125, 0., 0.333, 0.7, 0.999, 1.],
                    ),
                };
                let focus: [f32; 6] = focus;
                let flash: [f32; 6] = flash;
                request += &format!("{} ", u8::from(art));
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
                cases.push((profile, art, feedback, focus, flash, values, parent));
            }
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
            "shin-murderer-top-native-1.03.txt"
        } else {
            "shin-murderer-top-native-original.txt"
        }),
        &native,
    )
    .unwrap();
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Top native child comparison"),
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
    for ((profile, art, feedback, focus, flash, values, parent), native) in cases.iter().zip(frames)
    {
        let packet_count = 17 + flash.iter().filter(|v| **v > 0.).count();
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
            model.reference_set_top_drawing(*art, *focus, *flash);
        }
        layer.pre_render(context, parent);
        let render = |context: &mut PreRenderContext, captured: bool| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Top native frame",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            if captured {
                draw_native_top(&layer, &mut pass, native);
            } else {
                layer.render(&mut pass, parent, 4, PassKind::Opaque);
            }
            drop(pass);
            super::super::TileLayer::readback_reference_texture(context, &target)
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
            "Top profile{profile} art{art} feedback{feedback}: {visible} visible pixels, maxRGBAerror{worst}"
        );
        assert_eq!(
            worst, 0,
            "Top profile{profile} art{art} feedback{feedback}: native child frame mismatch"
        );
    }
    for at in [0x8cd70usize, 0x9b250, 0xc6d00] {
        let mut bytes = std::fs::read(&executable).unwrap();
        bytes[256 + at - if updated { 0xa0 } else { 0 }] ^= 1;
        let path = std::env::temp_dir().join(format!("shin-top-mutant-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let result = Command::new(&binary).arg(&path).output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_file(binary).unwrap();
    println!(
        "Top unchanged native constructor+palette+child+parent:60frames/1140packets/124416000pixels,6transforms x2art x5feedback profiles, maxRGBAerror{worst_all};3whole-image mutants rejected"
    );
}
