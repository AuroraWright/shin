//! Native Accusation renderer packets, including native parent composition, are the
//! independent geometry/UV/color/matrix oracle for the production child.
use super::*;
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn draw_native_accusation(layer: &MurdererAccusationView, pass: &mut RenderPass, captured: &str) {
    use shin_render::shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    };
    use shin_render::{DrawPrimitive, RenderProgramWithArguments};
    let inner = layer.inner_ref();
    let mut last_pass = None;
    for line in captured.lines().filter(|line| line.starts_with("Q ")) {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f.len(), 61);
        let scalar = |i: usize| f[i].parse::<f32>().unwrap();
        let texture = match f[1] {
            "different_space_1a" => &inner.textures.background,
            "But_BodyA" => &inner.textures._but_bodya,
            "But_BodyB" => &inner.textures._but_bodyb,
            "But_FingerB1" => &inner.textures._but_fingerb1,
            "But_FingerB2" => &inner.textures._but_fingerb2,
            "But_Majime3" => &inner.textures._but_majime3,
            "But_FutekiB2" => &inner.textures._but_futekib2,
            "But_KomaruB1" => &inner.textures._but_komarub1,
            "left" => &inner.textures.left,
            "sentaku" => &inner.textures.sentaku,
            "tokutei" => &inner.textures.tokutei,
            "line" => &inner.textures.line,
            "non" => &inner.textures.non,
            "murderer" => &inner.textures.murderer,
            "back" => &inner.textures._back,
            name => {
                let names = [
                    "kla0", "nat0", "jes0", "eva0", "hid0", "geo0", "rud0", "kir0", "but0", "ros0",
                    "mar1", "nan0", "gen0", "sha0", "kan0", "goh0", "kum0",
                ];
                inner.textures.suspects()[names
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap_or_else(|| panic!("Unexpected native Accusation texture{name}"))]
            }
        };
        // Native parent clears depth between both child passes.
        if last_pass != Some(f[52]) {
            pass.clear(None, None, Some(1.));
            last_pass = Some(f[52]);
        }
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

struct Case {
    profile: usize,
    state: usize,
    mask: u32,
    feedback: usize,
    elapsed: Option<f32>,
    focus: [f32; 18],
    flash: [f32; 18],
    back: f32,
    values: [f32; 90],
    parent: TransformParams,
}
fn capture(
    binary: &std::path::Path,
    executable: &std::path::Path,
    input: &str,
    option: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(binary);
    command.arg(executable);
    if let Some(option) = option {
        command.arg(option);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    output
}
pub(crate) fn reference_accusation_draw(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
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
    let scratch =
        std::env::temp_dir().join(format!("shin-accusation-reference-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let binary = scratch.join("native-accusation");
    let result = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-accusation-draw.c"))
        .arg("-lm")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success() && result.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let prepared = shin_tasks::block_on(MurdererAccusationView::prepare(server, 20));
    let mut cases = Vec::new();
    let mut request = String::new();
    let masks: Vec<u32> = [0, 0x1ffff, 0x1c0, 0x15555, 0xaaaa]
        .into_iter()
        .chain((0..17).map(|i| 1 << i))
        .collect();
    for profile in 0..7 {
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
            6 => TransformParams {
                // Exactly on the far plane: native Less rejects every quad.
                // Always would draw, so this also guards packet depth replay.
                transform: Mat4::from_translation(glam::vec3(0., 0., -1.)),
                ..TransformParams::default()
            },
            _ => {
                values[0] = 0.125;
                values[1] = 0.375;
                values[12] = 997.;
                values[13] = 1003.;
                TransformParams::default()
            }
        };
        let states = (0..3)
            .flat_map(|state| {
                masks
                    .iter()
                    .copied()
                    .flat_map(move |mask| (0..3).map(move |feedback| (state, mask, feedback, None)))
            })
            .chain(
                [0., 3.75, 15., 22.5, 30.]
                    .into_iter()
                    .map(|elapsed| (1, 0x1c0, 2, Some(elapsed))),
            );
        for (state, mask, feedback, elapsed) in states {
            let focus: [f32; 18] = std::array::from_fn(|row| match feedback {
                0 => 0.,
                1 => {
                    if row == 17 {
                        0.
                    } else {
                        0.125 + (row % 4) as f32 * 0.125
                    }
                }
                _ => 1.,
            });
            let flash: [f32; 18] = std::array::from_fn(|row| match feedback {
                0 => 0.,
                1 => {
                    if row % 2 != 0 {
                        0.2
                    } else {
                        0.
                    }
                }
                _ => 0.7,
            });
            let back: f32 = [0., 0.7, 1.][feedback];
            let white = elapsed.map_or(0., |elapsed| (1.0f32 - elapsed / 30.).max(0.));
            request += &format!(
                "{state} {mask:x} {:x} {:x} ",
                back.to_bits(),
                white.to_bits()
            );
            for (focus, flash) in focus.into_iter().zip(flash) {
                request += &format!("{:x} {:x} ", focus.to_bits(), flash.to_bits());
            }
            for value in values {
                request += &format!("{:x} ", value.to_bits());
            }
            request += &"11 0 0 ".repeat(6);
            request += "0 0 ";
            for value in parent
                .transform
                .to_cols_array()
                .into_iter()
                .chain(parent.camera_position.to_array())
                .chain(parent.unconditionally_inherited_translation.to_array())
                .chain(parent.wobble_translation.to_array())
            {
                request += &format!("{:x} ", value.to_bits());
            }
            request += "\n";
            cases.push(Case {
                profile,
                state,
                mask,
                feedback,
                elapsed,
                focus,
                flash,
                back,
                values,
                parent,
            });
        }
    }
    std::fs::write(scratch.join("input.txt"), &request).unwrap();
    let output = capture(&binary, &executable, &request, None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native = String::from_utf8(output.stdout).unwrap();
    std::fs::write(scratch.join("packets.txt"), &native).unwrap();
    let frames: Vec<_> = native.split("V ").skip(1).collect();
    assert_eq!(frames.len(), cases.len());
    for mutant in [
        "--mutant-renderer",
        "--mutant-constructor",
        "--mutant-parent",
    ] {
        let output = capture(
            &binary,
            &executable,
            &format!("{}\n", request.lines().next().unwrap()),
            Some(mutant),
        );
        assert!(
            !output.status.success()
                || String::from_utf8_lossy(&output.stdout) != format!("V {}", frames[0]),
            "Native entrypoint mutant must reject or change captured packets"
        );
    }
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native Accusation framebuffer comparison"),
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
    let mut packets = 0;
    for (case, native) in cases.iter().zip(frames) {
        let mut layer = prepared.construct(false);
        layer.properties_mut().reference_install_native_state(
            &case.values,
            &[(17, 0, 0.); 6],
            None,
        );
        layer
            .inner_ref()
            .model
            .lock()
            .unwrap()
            .reference_set_accusation_drawing(
                case.state,
                case.mask,
                case.elapsed,
                case.focus,
                case.flash,
                case.back,
            );
        layer.pre_render(context, &case.parent);
        let draw = |context: &mut PreRenderContext, captured: bool| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Accusation native frame",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            if captured {
                draw_native_accusation(&layer, &mut pass, native);
            } else {
                layer.render(&mut pass, &case.parent, 4, PassKind::Opaque);
            }
            drop(pass);
            crate::layer::user::TileLayer::readback_reference_texture(context, &target)
        };
        let actual = draw(context, false);
        let expected = draw(context, true);
        if case.profile == 6 {
            assert!(
                expected.chunks_exact(4).all(|p| p == [0, 0, 0, 255]),
                "Native Less must reject quads at cleared far-plane depth"
            );
        }
        packets += native.lines().filter(|line| line.starts_with("Q ")).count();
        let worst = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
            .max()
            .unwrap();
        let different = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
        eprintln!(
            "Accusation profile{} state{} mask{:x} feedback{} elapsed{:?}: different{different} maxRGBAerror{worst}",
            case.profile, case.state, case.mask, case.feedback, case.elapsed
        );
        if worst != 0 {
            std::fs::write(scratch.join("actual.rgba"), &actual).unwrap();
            std::fs::write(scratch.join("expected.rgba"), &expected).unwrap();
        }
        assert_eq!(
            worst,
            0,
            "Native Accusation full-frame mismatch; artifacts at{}",
            scratch.display()
        );
    }
    println!(
        "Accusation: {} strict1920x1080frames, {packets} native packets, {} byte-identical pixels. Seven parent/property transforms including far-plane depth rejection, all17single-selected/all/none/correct/alternating masks, three feedback profiles, all3Battler states and white-flash boundaries. Shared Metal replay does not prove native NVN/scheduling.",
        cases.len(),
        cases.len() * 1920 * 1080
    );
    // Preserve inputs and native packets outside the compiled-artifact directory.
    let folder = std::path::Path::new("/tmp").join(if updated {
        "shin-accusation-native-1.03"
    } else {
        "shin-accusation-native-original"
    });
    std::fs::create_dir_all(&folder).unwrap();
    for name in ["input.txt", "packets.txt"] {
        std::fs::copy(scratch.join(name), folder.join(name)).unwrap();
    }
    std::fs::remove_dir_all(scratch).unwrap();
}
