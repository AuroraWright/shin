// Included in Quiz's existing render-fixture module to use its GPU host and
// independent decoded-atlas bilinear sampler. Expected draws come from native
// command capture, not LastModel or the production renderer's geometry.
#[derive(Debug, Clone, Copy)]
struct LastPassTest {
    passes: u8, // bit0 opaque, bit1 transparent
    initial_stencil: u8,
    overlap: bool,
    transform: Option<LastTransformTest>,
}
#[derive(Debug, Clone, Copy)]
struct LastTransformTest {
    local: TransformParams,
    parent: TransformParams,
    flags: crate::layer::render_params::ComposeFlags,
    properties: Option<LastPropertyTest>,
}
#[derive(Debug, Clone, Copy)]
struct LastPropertyTest {
    values: [f32; 90],
    wobblers: [(u32, i32, f32); 6],
    advance: Option<f32>,
    queued: Option<(usize, f32, f32)>,
    fast_forward: bool,
}

fn render_captured_last_draws(
    layer: &crate::layer::user::last_layer::LastLayerImpl,
    pass: &mut RenderPass,
    captured: &str,
    kind: PassKind,
) {
    render_captured_draw_packets(|name| layer.reference_texture(name), pass, captured, kind);
}

fn render_captured_draw_packets<'a>(
    texture: impl Fn(&str) -> &'a GpuTexture,
    pass: &mut RenderPass,
    captured: &str,
    kind: PassKind,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::{
        buffer::VertexSource, texture::TextureSampler, vertices::PosColTexVertex,
    };
    use shin_render::{DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder};
    for line in captured.lines().filter(|l| l.starts_with("Q ")) {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f.len(), 61);
        let opaque = f[52] == "1";
        if opaque != (kind == PassKind::Opaque) {
            continue;
        }
        let scalar = |i: usize| f[i].parse::<f32>().unwrap();
        let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| {
            let offset = 4 + i * 6;
            let argb = f[offset + 3].parse::<u32>().unwrap();
            PosColTexVertex {
                position: glam::vec3(scalar(offset), scalar(offset + 1), scalar(offset + 2)),
                color: UnormColor::from_rgba(
                    (argb >> 16) as u8,
                    (argb >> 8) as u8,
                    argb as u8,
                    (argb >> 24) as u8,
                ),
                texture_position: Vec2::new(scalar(offset + 4), scalar(offset + 5)),
            }
        });
        let mut matrix: [f32; 16] = std::array::from_fn(|i| scalar(28 + i));
        for column in 0..4 {
            matrix[column * 4 + 2] = matrix[column * 4 + 2] * 0.5 + matrix[column * 4 + 3] * 0.5;
        }
        let matrix = glam::Mat4::from_cols_array(&matrix);
        let mut texture = texture(f[1]).as_source();
        texture.sampler = match f[3] {
            "0" => TextureSampler::Linear,
            "2" => TextureSampler::LinearMirrorU,
            _ => panic!("Native sampler {}", f[3]),
        };
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(f[54].parse().unwrap(), f[55] == "6", false)
            .color_blend_type(match f[53] {
                "1" => shin_render::ColorBlendType::Opaque,
                "2" => shin_render::ColorBlendType::Layer1,
                _ => panic!("Native blend {}", f[53]),
            });
        let program = match f[2] {
            "2" => RenderProgramWithArguments::Sprite {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                sprite: texture,
                transform: matrix,
            },
            "5" => RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture,
                transform: matrix,
                flash1: glam::vec4(scalar(44), scalar(45), scalar(46), scalar(47)),
                flash2: glam::vec4(scalar(48), scalar(49), scalar(50), scalar(51)),
            },
            _ => panic!("Native program {}", f[2]),
        };
        pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires supplied original/1.03 Last executable/atlas, Metal and Loopback Audio"]
fn reference_last_native_draw_commands_and_full_atlas_frames() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };
    crate::init_test_task_pools();
    let _audio = shin_audio::AudioManager::new_for_tests();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = std::env::temp_dir().join(format!("shin-last-native-draw-{}", std::process::id()));
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-last-draw.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let executable = root.join("umineko_switch_files").join(if updated {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    });
    let mut atlas = None;
    for path in std::env::var_os("SHIN_REFERENCE_PATCH_ROM")
        .into_iter()
        .chain(std::env::var_os("SHIN_REFERENCE_ROM"))
    {
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        if let Ok(file) = rom.find_file("/last.txa") {
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            atlas = Some(shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap());
            break;
        }
    }
    let atlas = atlas.unwrap();
    assert!(
        atlas
            .get_texture("bg")
            .unwrap()
            .as_raw()
            .chunks_exact(4)
            .all(|p| p[3] == 255),
        "Native opaque background capture uses fully opaque authored texels"
    );
    let cases: &[(&str, &[&str])] = &[
        ("initial", &["N"]),
        ("left-hold", &["N", "P 400 900", "H 400 900", "T 1"]),
        (
            "right-hold-peak",
            &["N", "P 1200 900", "H 1200 900", "T 3.5"],
        ),
        (
            "hold-cancel",
            &["N", "P 400 900", "H 400 900", "T 3.5", "X", "T 1"],
        ),
        ("left-flash", &["N", "C 1 1 1", "T 1"]),
        ("left-flash-peak", &["N", "C 1 0 0", "T 3.5"]),
        ("left-flash-end", &["N", "C 1 0 0", "T 7"]),
        ("left-slide-start", &["N", "C 1 0 0", "T 7", "T 0.25"]),
        ("left-slide-middle", &["N", "C 1 0 0", "T 7", "T 10"]),
        ("left-ready", &["N", "C 1 0 0", "T 7", "T 20"]),
        ("left-pulse-low", &["N", "C 1 0 0", "T 7", "T 20", "T 48"]),
        (
            "left-confirm-hold",
            &[
                "N",
                "C 1 0 0",
                "T 7",
                "T 20",
                "P 900 950",
                "H 900 950",
                "T 1",
            ],
        ),
        (
            "left-confirm-first",
            &["N", "C 1 0 0", "T 7", "T 20", "C 1 1 1", "T 1"],
        ),
        (
            "left-confirm-peak",
            &["N", "C 1 0 0", "T 7", "T 20", "C 0 0 1", "T 3.5"],
        ),
        (
            "left-confirm-finished",
            &["N", "C 1 0 0", "T 7", "T 20", "C 0 0 1", "T 7"],
        ),
        (
            "left-back-flash",
            &["N", "C 1 0 0", "T 7", "T 20", "C 0 1 0", "T 1"],
        ),
        (
            "left-return-middle",
            &["N", "C 1 0 0", "T 7", "T 20", "C 0 1 0", "T 7", "T 10"],
        ),
        (
            "left-return-finished",
            &["N", "C 1 0 0", "T 7", "T 20", "C 0 1 0", "T 7", "T 20"],
        ),
        ("right-slide-middle", &["N", "C 0 1 0", "T 7", "T 10"]),
        ("right-ready", &["N", "C 0 1 0", "T 7", "T 20"]),
        (
            "right-back-hold",
            &[
                "N",
                "C 0 1 0",
                "T 7",
                "T 20",
                "P 100 900",
                "H 100 900",
                "T 1",
            ],
        ),
        (
            "right-confirm-peak",
            &["N", "C 0 1 0", "T 7", "T 20", "C 1 1 1", "T 3.5"],
        ),
        (
            "right-confirm-finished",
            &["N", "C 0 1 0", "T 7", "T 20", "C 0 0 1", "T 7"],
        ),
        (
            "right-return-middle",
            &["N", "C 0 1 0", "T 7", "T 20", "C 1 0 0", "T 7", "T 10"],
        ),
    ];
    let pass_cases: &[(&str, &[&str], LastPassTest)] = &[
        (
            "opaque-only-empty",
            &["N"],
            LastPassTest {
                passes: 1,
                initial_stencil: 0,
                overlap: false,
                transform: None,
            },
        ),
        (
            "opaque-only-equal",
            &["N"],
            LastPassTest {
                passes: 1,
                initial_stencil: 4,
                overlap: false,
                transform: None,
            },
        ),
        (
            "opaque-only-higher",
            &["N"],
            LastPassTest {
                passes: 1,
                initial_stencil: 5,
                overlap: false,
                transform: None,
            },
        ),
        (
            "transparent-only-empty",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 2,
                initial_stencil: 0,
                overlap: false,
                transform: None,
            },
        ),
        (
            "transparent-only-equal",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 2,
                initial_stencil: 4,
                overlap: false,
                transform: None,
            },
        ),
        (
            "transparent-only-higher",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 2,
                initial_stencil: 5,
                overlap: false,
                transform: None,
            },
        ),
        (
            "combined-empty",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 3,
                initial_stencil: 0,
                overlap: false,
                transform: None,
            },
        ),
        (
            "combined-equal",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 3,
                initial_stencil: 4,
                overlap: false,
                transform: None,
            },
        ),
        (
            "combined-higher",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 3,
                initial_stencil: 5,
                overlap: false,
                transform: None,
            },
        ),
        (
            "overlap-front-flash",
            &["N", "C 1 0 0", "T 3.5"],
            LastPassTest {
                passes: 3,
                initial_stencil: 0,
                overlap: true,
                transform: None,
            },
        ),
        (
            "overlap-left-door",
            &["N", "C 1 0 0", "T 7", "T 20"],
            LastPassTest {
                passes: 3,
                initial_stencil: 0,
                overlap: true,
                transform: None,
            },
        ),
        (
            "overlap-right-slide",
            &["N", "C 0 1 0", "T 7", "T 10"],
            LastPassTest {
                passes: 3,
                initial_stencil: 0,
                overlap: true,
                transform: None,
            },
        ),
    ];
    use crate::layer::render_params::ComposeFlags;
    let base = TransformParams {
        transform: glam::Mat4::from_translation(glam::vec3(0.0, 0.0, 1000.0)),
        ..Default::default()
    };
    let mut transforms = Vec::new();
    let mut parent = TransformParams::default();
    parent.unconditionally_inherited_translation = Vec2::new(137.0, -83.0);
    let mut local = base;
    local.unconditionally_inherited_translation = Vec2::new(-800.0, 550.0);
    local.camera_position = glam::vec3(500.0, 400.0, 2000.0);
    transforms.push((
        "parent-offset",
        LastTransformTest {
            local,
            parent,
            flags: ComposeFlags::empty(),
            properties: None,
        },
    ));
    parent = TransformParams::default();
    parent.camera_position = glam::vec3(-160.0, 90.0, 400.0);
    transforms.push((
        "parent-camera",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::empty(),
            properties: None,
        },
    ));
    transforms.push((
        "ignore-camera",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::IGNORE_CAMERA_POSITION,
            properties: None,
        },
    ));
    parent.camera_position = glam::vec3(80.0, -40.0, 1000.0);
    transforms.push((
        "collapsed-camera",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::empty(),
            properties: None,
        },
    ));
    parent = TransformParams::default();
    parent.transform = glam::Mat4::from_scale_rotation_translation(
        glam::vec3(0.65, 1.4, 1.0),
        glam::Quat::from_rotation_z(0.23),
        glam::vec3(50.0, -30.0, 0.0),
    );
    parent.wobble_translation = Vec2::new(23.0, -13.0);
    parent.unconditionally_inherited_translation = Vec2::new(57.0, 31.0);
    transforms.push((
        "rotated-scaled",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::empty(),
            properties: None,
        },
    ));
    transforms.push((
        "ignore-parent",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::DONT_INHERIT_TRANSFORM,
            properties: None,
        },
    ));
    parent.transform = glam::Mat4::from_scale(glam::vec3(-0.8, 1.15, 1.0));
    transforms.push((
        "mirrored",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::empty(),
            properties: None,
        },
    ));
    transforms.push((
        "ignore-wobble",
        LastTransformTest {
            local: base,
            parent,
            flags: ComposeFlags::DONT_INHERIT_WOBBLE,
            properties: None,
        },
    ));
    // Exercise the actual wrapper with raw script properties. The oracle now
    // extracts them through native754d0/754a0 rather than supplying a matrix.
    use num_traits::FromPrimitive;
    use shin_core::vm::command::types::LayerProperty;
    let defaults = std::array::from_fn(|i| LayerProperty::from_usize(i).unwrap().initial_value() as f32);
    let property_cases: &[(&str, &[(usize, f32)])] = &[
        ("properties-tint", &[(6, 250.0), (7, 1700.0), (8, 500.0)]),
        ("properties-hidden-alpha", &[(22, 0.0), (9, 0.0)]),
        ("properties-blend", &[(23, 2.0)]),
        ("properties-fragment-clip", &[(24, 5.0), (61, 1.0), (62, -150.0), (63, -100.0), (64, 200.0), (65, 130.0)]),
        ("properties-effects", &[(56, 800.0), (66, 10000.0), (67, 64.0), (68, 500.0), (76, 100.0), (82, 100.0)]),
        ("properties-scale-origin", &[(0, 81.25), (1, -62.75), (10, 133.5), (11, -75.25), (12, 750.0), (13, 1200.0), (14, 1100.0), (15, 900.0)]),
        ("properties-rotation-origin", &[(16, 41.75), (17, -20.5), (18, 25.0), (19, -11.0), (20, 17.5), (21, -9.25)]),
        ("properties-flip", &[(26, 3.0), (3, 48.75), (4, -33.5)]),
        ("properties-camera", &[(2, 1500.0), (85, 500.0), (86, -800.0), (87, 2000.0), (88, -700.0), (89, 400.0)]),
        ("properties-wobblers", &[(32, 6.0), (33, 17.0), (34, 90.0), (36, 1.0), (37, 11.0), (38, 50.0), (44, 3.0), (45, 10.0), (46, 15.0), (48, 1.0), (49, 12.0), (50, 120.0), (52, 4.0), (53, 19.0), (54, 200.0)]),
        ("properties-frozen-clock", &[(32, 1.0), (33, 10.0), (34, 120.0)]),
        ("properties-frozen-fast-forward", &[(44, 1.0), (45, 10.0), (46, 10.0)]),
    ];
    for &(name, overrides) in property_cases {
        let mut properties = LastPropertyTest {
            values: defaults,
            wobblers: [(0xfffffff0, 0, 0.0), (23, 0, 0.0), (17, 0, 0.0), (29, 0, 0.0), (0x80000001, 0, 0.0), (31, 0, 0.0)],
            advance: None,
            queued: None,
            fast_forward: name == "properties-frozen-fast-forward",
        };
        for &(index, value) in overrides { properties.values[index] = value; }
        if name.contains("wobblers") || name.contains("frozen") {
            properties.advance = Some(2.25);
        }
        if name.contains("frozen") { properties.queued = Some((0, 320.0, 100.0)); }
        transforms.push((name, LastTransformTest {
            local: base,
            parent: TransformParams {
                transform: glam::Mat4::from_scale_rotation_translation(
                    glam::vec3(0.85, 1.1, 1.0), glam::Quat::from_rotation_z(0.07), glam::vec3(37.0, -23.0, 0.0)),
                camera_position: glam::vec3(-45.0, 31.0, 200.0),
                unconditionally_inherited_translation: Vec2::new(27.0, -13.0),
                wobble_translation: Vec2::new(11.0, 7.0),
            },
            flags: ComposeFlags::empty(),
            properties: Some(properties),
        }));
    }
    let transformed: Vec<_> = transforms
        .iter()
        .flat_map(|&(name, transform)| {
            let commands: [(&str, &[&str]); 3] = [
                ("front", &["N", "C 1 0 0", "T 3.5"]),
                ("left-door", &["N", "C 1 0 0", "T 7", "T 20"]),
                ("right-slide", &["N", "C 0 1 0", "T 7", "T 10"]),
            ];
            commands.into_iter().map(move |(phase, commands)| {
                (
                    format!("{name}-{phase}"),
                    commands,
                    LastPassTest {
                        passes: 3,
                        initial_stencil: 0,
                        overlap: false,
                        transform: Some(transform),
                    },
                )
            })
        })
        .collect();
    let mut validated_frames = 0;
    for (name, commands, pass_case) in cases
        .iter()
        .map(|&(n, c)| (n, c, None))
        .chain(pass_cases.iter().map(|&(n, c, p)| (n, c, Some(p))))
        .chain(
            transformed
                .iter()
                .map(|(n, c, p)| (n.as_str(), *c, Some(*p))),
        )
    {
        if let Ok(filter) = std::env::var("SHIN_REFERENCE_LAST_CASE_FILTER") {
            if !name.contains(&filter) { continue; }
        }
        validated_frames += 1;
        let mut request = String::new();
        if let Some(t) = pass_case.and_then(|c| c.transform) {
            request.push_str(&format!("A {}", t.flags.bits() as u32));
            for p in [t.local, t.parent] {
                for v in p
                    .transform
                    .to_cols_array()
                    .into_iter()
                    .chain(p.camera_position.to_array())
                    .chain(p.unconditionally_inherited_translation.to_array())
                    .chain(p.wobble_translation.to_array())
                {
                    request.push_str(&format!(" {:08x}", v.to_bits()));
                }
            }
            request.push('\n');
        }
        if let Some(properties) = pass_case.and_then(|c| c.transform).and_then(|t| t.properties) {
            request.push_str("E");
            for value in properties.values { request.push_str(&format!(" {:08x}", value.to_bits())); }
            for (seed, mode, time) in properties.wobblers { request.push_str(&format!(" {seed:08x} {:08x} {:08x}", mode as u32, time.to_bits())); }
            request.push_str(&format!(" {} {:08x}\n", u8::from(properties.advance.is_some()), properties.advance.unwrap_or(0.0).to_bits()));
            if let Some((index, target, duration)) = properties.queued { request.push_str(&format!("Y {index} {:08x} {duration}\n", target.to_bits())); }
        }
        request.push_str(&(commands.join("\n") + "\n"));
        if let Some(properties) = pass_case.and_then(|c| c.transform).and_then(|t| t.properties) {
            if properties.fast_forward { request.push_str("F\n"); }
            request.push_str("S\n");
        }
        request.push_str("V\n");
        let mut child = Command::new(&binary)
            .arg(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let native = String::from_utf8(output.stdout).unwrap();
        let packets: Vec<_> = native
            .lines()
            .filter(|line| line.starts_with("Q "))
            .collect();
        assert!(!packets.is_empty());
        let pixels = render_reference_frame_pixels(
            None,
            Some(None),
            None,
            None,
            None,
            None,
            Some(MurdererFeedback::LastCommands(commands, pass_case, None)),
        );
        if pass_case.is_some_and(|c| c.transform.is_some()) {
            // Render the unchanged native quad/color/UV/uniform/state packets
            // on the same Metal host. Model geometry and CPU transform code are
            // absent from this expected draw path. Existing36 cases retain the
            // independent scalar native-command/decoded-atlas oracle below.
            let expected = render_reference_frame_pixels(
                None,
                Some(None),
                None,
                None,
                None,
                None,
                Some(MurdererFeedback::LastCommands(
                    commands,
                    pass_case,
                    Some(&native),
                )),
            );
            let visible = expected
                .chunks_exact(4)
                .filter(|pixel| pixel[..3].iter().any(|&channel| channel != 0))
                .count();
            if name.starts_with("collapsed-camera-") {
                assert_eq!(
                    visible, 0,
                    "Last {name}: zero camera scale must collapse all quads"
                );
            } else {
                assert!(visible > 10_000, "Last {name}: native reference must contain visible artwork, got {visible} pixels");
            }
            let worst = pixels
                .iter()
                .zip(&expected)
                .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
                .max()
                .unwrap();
            assert!(
                worst <= 2,
                "Last {name}: captured native transformed GPU error {worst}"
            );
            eprintln!("Last transformed {name}: {} native draws,2073600 pixels xRGBA,maxByteError {worst},visiblePixels {visible}",packets.len());
            continue;
        }
        let config = pass_case.unwrap_or(LastPassTest {
            passes: 3,
            initial_stencil: 0,
            overlap: false,
            transform: None,
        });
        let mut expected = vec![[0.0f32; 3]; 1920 * 1080];
        let mut stencil = vec![config.initial_stencil; 1920 * 1080];
        if config.overlap {
            for y in 720..1040 {
                for x in 1320..1800 {
                    if 6 > stencil[y * 1920 + x] {
                        expected[y * 1920 + x] = [0.0, 255.0, 255.0];
                        stencil[y * 1920 + x] = 6;
                    }
                }
            }
        }
        let mut lower_drawn = false;
        for packet in &packets {
            let fields: Vec<_> = packet.split_whitespace().collect();
            assert_eq!(fields.len(), 61);
            let native_pass = fields[52].parse::<u32>().unwrap();
            let blend = fields[53].parse::<u32>().unwrap();
            let reference = fields[54].parse::<u8>().unwrap();
            let stencil_function = fields[55].parse::<u32>().unwrap();
            assert_eq!(reference, 4);
            assert_eq!(fields[56], "7"); // Always depth; depth writes disabled.
            assert_eq!(fields[57], "0");
            assert_eq!(fields[58], "255");
            assert_eq!(fields[59], "255");
            assert_eq!(fields[60], "2"); // Replace stencil on pass.
            if native_pass == 1 {
                assert_eq!(fields[1], "bg");
                assert_eq!(blend, 1);
                assert_eq!(stencil_function, 4);
                if config.passes & 1 == 0 {
                    continue;
                }
            } else {
                assert_eq!(native_pass, 0);
                assert_ne!(fields[1], "bg");
                assert_eq!(blend, 2);
                assert_eq!(stencil_function, 6);
                if config.overlap && !lower_drawn {
                    for (pixel, value) in expected.iter_mut().zip(&mut stencil) {
                        if 3 > *value {
                            *pixel = [255.0, 0.0, 255.0];
                            *value = 3;
                        }
                    }
                    lower_drawn = true;
                }
                if config.passes & 2 == 0 {
                    continue;
                }
            }
            let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
            let image = atlas.get_texture(fields[1]).unwrap();
            let program = fields[2].parse::<u32>().unwrap();
            assert!(program == 2 || program == 5);
            let sampler = fields[3].parse::<u32>().unwrap();
            assert!(sampler == 0 || sampler == 2);
            // The fixture now executes the actual native GL projection. Undo
            // only that known centered projection for the scalar atlas oracle.
            let left = scalar(4) + scalar(40) * 960.0 + 960.0;
            let top = scalar(5) - scalar(41) * 540.0 + 540.0;
            assert_eq!(scalar(28).to_bits(), (1.0f32 / 960.0).to_bits());
            assert_eq!(scalar(33).to_bits(), (-1.0f32 / 540.0).to_bits());
            let width = scalar(10) - scalar(4);
            let height = scalar(17) - scalar(5);
            assert!(width > 0.0 && height > 0.0);
            let color = fields[7].parse::<u32>().unwrap();
            for vertex in 1..4 {
                assert_eq!(fields[7 + vertex * 6].parse::<u32>().unwrap(), color);
            }
            let tint = [
                (color >> 16) as u8 as f32 / 255.0,
                (color >> 8) as u8 as f32 / 255.0,
                color as u8 as f32 / 255.0,
            ];
            let alpha = (color >> 24) as f32 / 255.0;
            for y in top.floor().max(0.0) as usize..(top + height).ceil().min(1080.0) as usize {
                for x in left.floor().max(0.0) as usize..(left + width).ceil().min(1920.0) as usize
                {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    if px < left || px >= left + width || py < top || py >= top + height {
                        continue;
                    }
                    let stored = &mut stencil[y * 1920 + x];
                    let accepted = if stencil_function == 4 {
                        reference > *stored
                    } else {
                        reference >= *stored
                    };
                    if !accepted {
                        continue;
                    }
                    *stored = reference;
                    let mut u = scalar(8) + (scalar(14) - scalar(8)) * (px - left) / width;
                    let v = scalar(9) + (scalar(21) - scalar(9)) * (py - top) / height;
                    if sampler == 2 {
                        u = u.rem_euclid(2.0);
                        if u > 1.0 {
                            u = 2.0 - u;
                        }
                    }
                    let sampled = sample_reference_rgba(image.dimensions(), image.as_raw(), u, v);
                    let coverage = sampled[3] / 255.0 * alpha;
                    for c in 0..3 {
                        let mut source = sampled[c];
                        if program == 5 {
                            source = (scalar(44 + c) * 255.0 - source).mul_add(scalar(47), source);
                            source = (scalar(48 + c) * 255.0 - source).mul_add(scalar(51), source);
                        }
                        let previous = &mut expected[y * 1920 + x][c];
                        *previous = if blend == 1 {
                            (source * tint[c]).round()
                        } else {
                            (source * tint[c] * coverage + *previous * (1.0 - coverage)).round()
                        };
                    }
                }
            }
        }
        let mut worst = 0.0f32;
        for (pixel, expected) in pixels.chunks_exact(4).zip(&expected) {
            for c in 0..3 {
                worst = worst.max((pixel[c] as f32 - expected[c]).abs());
            }
        }
        assert!(
            worst <= 2.0,
            "Last {name}: native commands/atlas full-frame RGB error {worst}"
        );
        eprintln!(
            "Last {name}: {} native draws, 2073600 independent pixels, maxRGBerror {worst}",
            packets.len()
        );
    }
    assert!(validated_frames > 0, "The Last case filter must select a frame");
    if std::env::var_os("SHIN_REFERENCE_LAST_CASE_FILTER").is_none() {
        assert_eq!(validated_frames, 96);
    }
    eprintln!("Last frame coverage: {validated_frames} frames; original36 scalar atlas,24 parent transforms,36 raw-property wrapper cases when unfiltered");
    std::fs::remove_file(binary).unwrap();
}
