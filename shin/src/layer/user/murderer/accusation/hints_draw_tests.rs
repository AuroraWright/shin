//! Native Hints renderer packets, including native parent composition, are the
//! independent geometry/UV/color/matrix oracle for the production child.
use super::*;
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(super) fn draw_native_hints(
    layer: &MurdererAccusationView,
    pass: &mut RenderPass,
    captured: &str,
) {
    use shin_render::shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    };
    use shin_render::{
        DepthFunction, DepthState, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    };
    let unpack = |color: u32| {
        UnormColor::from_rgba(
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            (color >> 24) as u8,
        )
    };
    let inner = layer.inner_ref();
    for line in captured.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.first() == Some(&"C") {
            assert_eq!(f.len(), 4);
            let flags = f[1].parse::<u32>().unwrap();
            let color = unpack(f[2].parse::<u32>().unwrap());
            pass.clear(
                (flags & 1 != 0).then_some(color),
                (flags & 4 != 0).then_some(0),
                (flags & 2 != 0).then(|| f[3].parse::<f32>().unwrap()),
            );
            continue;
        }
        if f.first() != Some(&"Q") {
            continue;
        }
        let scalar = |i: usize| f[i].parse::<f32>().unwrap();
        let count = f[13].parse::<usize>().unwrap();
        assert!(count == 4 || count == 8);
        assert_eq!(f.len(), 38 + 6 * count);
        assert_eq!(f[3], "0");
        assert_eq!(&f[8..13], &["1", "6", "255", "255", "2"]);
        let texture = match f[1] {
            "different_space_2d" => &inner.textures._different_space_2d,
            "left" => &inner.textures.left,
            "hint_menu" => &inner.textures._hint_menu,
            "hint_text" => &inner.textures._hint_text,
            "scroll" => inner.adv_textures.get("scroll"),
            "back" => &inner.textures._back,
            name => panic!("Unexpected native Hints texture {name}"),
        };
        let vertices: Vec<PosColTexVertex> = (0..count)
            .map(|i| {
                let o = 38 + 6 * i;
                let color = f[o + 3].parse::<u32>().unwrap();
                PosColTexVertex {
                    position: glam::vec3(scalar(o), scalar(o + 1), scalar(o + 2)),
                    color: unpack(color),
                    texture_position: Vec2::new(scalar(o + 4), scalar(o + 5)),
                }
            })
            .collect();
        let mut matrix: [f32; 16] = std::array::from_fn(|i| scalar(14 + i));
        for column in 0..4 {
            matrix[column * 4 + 2] = matrix[column * 4 + 2] * 0.5 + matrix[column * 4 + 3] * 0.5;
        }
        let matrix = Mat4::from_cols_array(&matrix);
        let depth = match f[6] {
            "1" => DepthFunction::Less,
            "7" => DepthFunction::Always,
            v => panic!("Native Hints depth{v}"),
        };
        let builder = RenderRequestBuilder::new()
            .depth_stencil(DepthStencilState {
                depth: DepthState {
                    function: depth,
                    write_enable: f[7] == "1",
                },
                ..DepthStencilState::shorthand(1, true, false)
            })
            .color_blend_type(match f[5] {
                "0" => ColorBlendType::NoColor,
                "1" => ColorBlendType::Opaque,
                "2" => ColorBlendType::Layer1,
                v => panic!("Native Hints blend{v}"),
            });
        let fill: Vec<PosColVertex> = vertices
            .iter()
            .map(|v| PosColVertex {
                position: v.position,
                color: v.color,
            })
            .collect();
        let program = match f[2] {
            "1" => RenderProgramWithArguments::Fill {
                vertices: VertexSource::VertexData { vertices: &fill },
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
                flash1: vec4(scalar(30), scalar(31), scalar(32), scalar(33)),
                flash2: vec4(scalar(34), scalar(35), scalar(36), scalar(37)),
            },
            v => panic!("Native Hints program{v}"),
        };
        pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
    }
}

pub(crate) fn reference_hints_draw(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
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
    let binary = std::env::temp_dir().join(format!("shin-murderer-hints-{}", std::process::id()));
    let result = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-hints-draw.c"))
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
        for progress in [0usize, 6, 7, 8, 12, 20] {
            let maximum = ((progress + 1) as f32 * 90. - 720.).max(0.);
            let scrolls = if maximum == 0. {
                vec![0.]
            } else {
                vec![
                    0.,
                    0.125,
                    12.5,
                    89.875,
                    maximum * 0.333333,
                    maximum - 0.125,
                    maximum,
                ]
            };
            for scroll in scrolls {
                for feedback in 0..3 {
                    let focus: [f32; 21] = std::array::from_fn(|i| match feedback {
                        0 => {
                            if i == progress {
                                1.
                            } else {
                                0.
                            }
                        }
                        1 => 0.,
                        _ => [0.125, 0.333333, 0.7, 0.999, 0.001, 0.875][i % 6],
                    });
                    let flash: [f32; 21] = std::array::from_fn(|i| {
                        if feedback == 2 {
                            [0.125, 0., 0.333, 0.7, 0.999, 1.][i % 6]
                        } else {
                            0.
                        }
                    });
                    let back: f32 = match feedback {
                        0 => 0.,
                        1 => 0.7,
                        _ => 1.,
                    };
                    request += &format!("{progress} {:x} {:x} ", scroll.to_bits(), back.to_bits());
                    for i in 0..=progress {
                        request += &format!("{:x} {:x} ", focus[i].to_bits(), flash[i].to_bits());
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
                        profile, progress, scroll, feedback, focus, flash, back, values, parent,
                    ));
                }
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
            "shin-murderer-hints-native-1.03.txt"
        } else {
            "shin-murderer-hints-native-original.txt"
        }),
        &native,
    )
    .unwrap();
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Hints native child comparison"),
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
    for ((profile, progress, scroll, feedback, focus, flash, back, values, parent), native) in
        cases.iter().zip(frames)
    {
        let packet_count = 12 + 2 * (progress + 1) + if *progress >= 8 { 2 } else { 0 };
        assert_eq!(
            native.lines().next().unwrap(),
            format!("{packet_count} 2 2 4 1 1")
        );
        let mut layer = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        layer
            .properties_mut()
            .reference_install_native_state(values, &[(17, 0, 0.); 6], None);
        {
            let mut model = layer.inner_ref().model.lock().unwrap();
            model.wipe = None;
            model.reference_set_hints_drawing(*progress as i32, *scroll, *focus, *flash, *back);
        }
        if *profile == 0 && *progress == 20 && *feedback == 1 && (*scroll == 0. || *scroll == 1170.)
        {
            use crate::app::AppAction as A;
            use shin_input::{Action, RawInputState};
            let key = if *scroll == 0. {
                winit::keyboard::KeyCode::KeyK
            } else {
                winit::keyboard::KeyCode::KeyL
            };
            {
                let mut model = layer.inner_ref().model.lock().unwrap();
                model.reference_set_hints_drawing(
                    20,
                    if *scroll == 0. { 1170. } else { 0. },
                    *focus,
                    *flash,
                    *back,
                );
            }
            let mut raw = RawInputState::new();
            raw.keyboard.insert(key);
            let lowered = A::lower(&raw);
            let input = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                is_clicked: lowered[action],
                is_clicked_or_repeated: lowered[action],
                is_held: lowered[action],
                ..Default::default()
            });
            for _ in 0..1000 {
                assert!(layer.handle_controller_input_with_scroll(
                    &input,
                    crate::app::text_scroll_axis(&raw)
                ));
            }
            assert_eq!(
                layer.inner_ref().model.lock().unwrap().hint_scroll,
                *scroll,
                "Physical {key:?} reaches the native scroll bound through the real view"
            );
            eprintln!(
                "Hint list physical {key:?}: scroll{scroll}; following full-frame comparison checks clipping, rows and scrollbar"
            );
        }
        layer.pre_render(context, parent);
        let render = |context: &mut PreRenderContext, captured: bool| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Hints native frame",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            if captured {
                draw_native_hints(&layer, &mut pass, native);
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
            "Hints profile{profile} progress{progress} scroll{scroll} feedback{feedback}: {visible} visible pixels, maxRGBAerror{worst}"
        );
        assert_eq!(
            worst, 0,
            "Hints profile{profile} progress{progress} scroll{scroll} feedback{feedback}: native child frame mismatch"
        );
    }
    for at in [0x91b20usize, 0x9b250, 0xeee50] {
        let mut bytes = std::fs::read(&executable).unwrap();
        let at = if updated {
            if at == 0xeee50 { 0xef580 } else { at - 0xa0 }
        } else {
            at
        };
        bytes[256 + at] ^= 1;
        let path = std::env::temp_dir().join(format!("shin-hints-mutant-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let result = Command::new(&binary).arg(&path).output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_file(binary).unwrap();
    println!(
        "Hints unchanged native constructor+child+scrollbar+parent:{}frames/{}packets/{}pixels,6transforms x progress/scroll/focus/flash profiles, maxRGBAerror{worst_all};3whole-image mutants rejected",
        cases.len(),
        native.lines().filter(|line| line.starts_with("Q ")).count(),
        cases.len() * 1920 * 1080
    );
}
