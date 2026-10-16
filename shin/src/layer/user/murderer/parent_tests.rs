use crate::layer::{LayerProperties, render_params::TransformParams};
use shin_core::vm::command::types::LayerProperty;

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[ignore = "executes supplied original/1.03 AArch64 Murderer parent callbacks"]
fn reference_native_murderer_parent_properties_capture_and_passes() {
    fn gpu_matrix(words: &[u32]) -> [u32; 16] {
        let mut matrix: [f32; 16] = std::array::from_fn(|i| f32::from_bits(words[i]));
        for column in 0..4 {
            let z = column * 4 + 2;
            matrix[z] = matrix[z] * 0.5 + matrix[z + 1] * 0.5;
            if matrix[z] == 0.0 {
                matrix[z] = 0.0;
            }
        }
        matrix.map(f32::to_bits)
    }
    fn canonical_depth(mut matrix: [u32; 16]) -> [u32; 16] {
        for column in 0..4 {
            let z = column * 4 + 2;
            if f32::from_bits(matrix[z]) == 0.0 {
                matrix[z] = 0;
            }
        }
        matrix
    }
    use num_traits::FromPrimitive;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = std::env::temp_dir().join(format!("shin-murderer-parent-{}", std::process::id()));
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-parent.c"))
        .arg("-lm")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let mut request = String::new();
    let mut cases = Vec::new();
    for flags in 0..8 {
        for profile in 0..3 {
            let mut values = std::array::from_fn::<_, 90, _>(|i| {
                LayerProperty::from_usize(i).unwrap().initial_value() as f32
            });
            values[25] = flags as f32;
            let parent = match profile {
                0 => TransformParams::default(),
                1 => {
                    values[0] = 147.25;
                    values[1] = -81.5;
                    values[12] = 1230.;
                    values[13] = 870.;
                    values[18] = 125.;
                    TransformParams {
                        transform: glam::Mat4::from_translation(glam::vec3(-93., 57., 0.)),
                        camera_position: glam::vec3(30., -10., 125.),
                        wobble_translation: glam::vec2(13., -7.),
                        unconditionally_inherited_translation: glam::vec2(19., 23.),
                    }
                }
                _ => {
                    values[0] = -234.5;
                    values[1] = 98.;
                    values[2] = 2150.;
                    values[14] = 830.;
                    values[15] = 1310.;
                    values[19] = -90.;
                    values[32] = 7.;
                    values[35] = 17.;
                    values[36] = 7.;
                    values[39] = -13.;
                    TransformParams {
                        transform: glam::Mat4::from_rotation_z(0.23),
                        camera_position: glam::vec3(-64., 82., 350.),
                        wobble_translation: glam::vec2(-27., 43.),
                        unconditionally_inherited_translation: glam::vec2(31., -41.),
                    }
                }
            };
            // All these non-transform properties are ignored by the real parent.
            values[6] = 230.;
            values[7] = 810.;
            values[8] = 0.;
            values[9] = 170.;
            values[22] = 0.;
            values[23] = 3.;
            values[24] = 4.;
            values[61] = 2.;
            values[62] = 400.;
            values[63] = 300.;
            values[64] = 450.;
            values[65] = 350.;
            values[66] = 9000.;
            values[67] = 17.;
            values[68] = 1000.;
            let wobblers = [(17, 0, 0.); 6];
            let mut props = LayerProperties::new();
            props.reference_install_native_state(&values, &wobblers, None);
            let matrix = props
                .get_composed_transform_params(&parent)
                .compute_top_left_transform()
                .to_cols_array()
                .map(f32::to_bits);
            for child in 0..2 {
                for wipe in 0..2 {
                    for passes in 1..=3 {
                        let delta = [0., 2., 120.][profile];
                        request +=
                            &format!("{child} {wipe} {passes} 4 {:x} 0 ", f32::to_bits(delta));
                        for v in values {
                            request += &format!("{:x} ", v.to_bits());
                        }
                        for (seed, mode, time) in wobblers {
                            request += &format!("{seed:x} {mode:x} {:x} ", f32::to_bits(time));
                        }
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
                        cases.push((child, wipe, passes, delta, matrix, values[0]));
                    }
                }
            }
        }
    }
    for name in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut child = Command::new(&binary)
            .arg(root.join("umineko_switch_files").join(name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = request.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let result = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8(result.stdout).unwrap();
        assert_eq!(output.lines().count(), cases.len());
        for (line, &(has_child, has_wipe, passes, delta, expected_matrix, value)) in
            output.lines().zip(&cases)
        {
            let words: Vec<u32> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            assert_eq!(
                &words[..9],
                &[
                    2,
                    2,
                    has_child,
                    has_child * 2,
                    3,
                    1,
                    1,
                    has_wipe * (passes & 1),
                    (1 - has_wipe) * (passes & 1)
                ],
                "{name}: {line}"
            );
            assert_eq!(words[9], (delta / 60.).fract().to_bits());
            assert_eq!(
                words[10],
                value.to_bits(),
                "Parent update must leave property clock/value unchanged"
            );
            assert_eq!(words[11], if has_child != 0 { delta.to_bits() } else { 0 });
            let mut offset = 12;
            for pass in [1, 0].into_iter().take((has_child * 2) as usize) {
                assert_eq!(words[offset], pass);
                // Adapt only GL [-1,1] depth to wgpu [0,1], as in the existing
                // native draw replay. XY/W words retain exact float bits.
                assert_eq!(
                    gpu_matrix(&words[offset + 1..offset + 17]),
                    canonical_depth(expected_matrix),
                    "{name}: child matrix"
                );
                offset += 17;
            }
            assert_eq!(
                &words[offset..offset + 9],
                &[
                    6,
                    0xff000000,
                    1f32.to_bits(),
                    2,
                    0xff000000,
                    1f32.to_bits(),
                    2,
                    0xff000000,
                    1f32.to_bits()
                ]
            );
            offset += 9;
            if words[8] != 0 {
                assert_eq!(&words[offset..offset + 9], &[1, 2, 7, 0, 6, 4, 255, 255, 2]);
                offset += 9;
                assert_eq!(
                    gpu_matrix(&words[offset..offset + 16]),
                    canonical_depth(
                        crate::render::top_left_projection_matrix()
                            .to_cols_array()
                            .map(f32::to_bits)
                    )
                );
                offset += 16;
                let vertices = &words[offset..offset + 24];
                for (vertex, (x, y, u, v)) in vertices.chunks_exact(6).zip([
                    (0f32, 0f32, 0f32, 0f32),
                    (1920., 0., 1., 0.),
                    (0., 1080., 0., 1.),
                    (1920., 1080., 1., 1.),
                ]) {
                    assert_eq!(
                        vertex,
                        &[
                            x.to_bits(),
                            y.to_bits(),
                            0,
                            0xffffffff,
                            u.to_bits(),
                            v.to_bits()
                        ]
                    );
                }
                offset += 24;
            }
            assert_eq!(offset, words.len());
        }
        // The whole-image guard rejects a mutant before native execution.
        let mut bytes = std::fs::read(root.join("umineko_switch_files").join(name)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let mutant = binary.with_extension("mutant");
        std::fs::write(&mutant, bytes).unwrap();
        let rejected = Command::new(&binary).arg(&mutant).output().unwrap();
        std::fs::remove_file(mutant).unwrap();
        assert_eq!(rejected.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("Unreviewed exact parent image")
        );
        println!(
            "{name}: {} native parent capture/update/presentation cases; exact property matrices, pass gates, clocks, clears, white opaque quad and stencil pass",
            cases.len()
        );
    }
    std::fs::remove_file(binary).unwrap();
}

pub(crate) fn reference_parent_properties_and_capture(
    context: &mut crate::render::PreRenderContext,
    server: &std::sync::Arc<crate::asset::system::AssetServer>,
) {
    use super::accusation::MurdererAccusationView;
    use crate::{
        layer::{DrawableLayer, Layer},
        update::AdvUpdatable,
    };
    use shin_core::{
        primitives::{color::UnormColor, update::FrameId},
        time::{Ticks, Tween},
    };
    use shin_render::{
        ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{
            buffer::VertexSource,
            texture::{TextureTarget, TextureTargetKind},
            vertices::PosColTexVertex,
        },
    };
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Murderer parent capture oracle"),
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
    let draw = |context: &mut crate::render::PreRenderContext,
                layer: &MurdererAccusationView,
                native: bool,
                kind: PassKind,
                parent: &TransformParams,
                stencil: u8| {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Murderer parent presentation",
            );
            pass.clear(
                Some(UnormColor::from_rgba(73, 31, 101, 255)),
                Some(stencil),
                Some(1.0),
            );
            if native {
                if kind == PassKind::Opaque {
                    // Exact packet from executing native9b4a0: program2, opaque
                    // blend1, >=stencil6/ref4, full viewport, white and UV0..1.
                    // Only map native GL depth into the wgpu depth interval.
                    let vertices = [
                        (0., 0., 0., 0.),
                        (1920., 0., 1., 0.),
                        (0., 1080., 0., 1.),
                        (1920., 1080., 1., 1.),
                    ]
                    .map(|(x, y, u, v)| PosColTexVertex {
                        position: glam::vec3(x, y, 0.),
                        color: UnormColor::from_rgba(255, 255, 255, 255),
                        texture_position: glam::vec2(u, v),
                    });
                    let mut matrix = [
                        982026377, 0, 0, 0, 0, 3136469462, 0, 0, 0, 0, 3212836864, 0, 3212836864,
                        1065353216, 2147483648, 1065353216,
                    ]
                    .map(f32::from_bits);
                    for column in 0..4 {
                        matrix[column * 4 + 2] =
                            matrix[column * 4 + 2] * 0.5 + matrix[column * 4 + 3] * 0.5;
                    }
                    layer
                        .reference_with_capture(|source| {
                            pass.run(
                                RenderRequestBuilder::new()
                                    .depth_stencil_shorthand(4, true, false)
                                    .color_blend_type(ColorBlendType::Opaque)
                                    .build(
                                        RenderProgramWithArguments::Sprite {
                                            vertices: VertexSource::VertexData {
                                                vertices: &vertices,
                                            },
                                            sprite: source.as_texture_source(),
                                            transform: glam::Mat4::from_cols_array(&matrix),
                                        },
                                        DrawPrimitive::TrianglesStrip,
                                    ),
                            );
                        })
                        .unwrap();
                }
            } else {
                layer.render(&mut pass, parent, 4, kind);
            }
        }
        crate::layer::user::TileLayer::readback_reference_texture(context, &target)
    };
    let mut frames = 0;
    for menu in 0..7 {
        let mut layer = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        if menu != 0 {
            layer.reference_open_menu(menu - 1);
        }
        let parent = TransformParams::default();
        layer.pre_render(context, &parent);
        let baseline = draw(context, &layer, false, PassKind::Opaque, &parent, 0);
        assert!(baseline.chunks_exact(4).any(|p| p[..3] != [73, 31, 101]));
        assert!(baseline.chunks_exact(4).all(|p| p[3] == 255));
        assert_eq!(
            baseline,
            draw(context, &layer, true, PassKind::Opaque, &parent, 0)
        );
        frames += 2;
        for (property, value) in [
            (LayerProperty::ShowLayer, 0),
            (LayerProperty::MulColorRed, 0),
            (LayerProperty::MulColorGreen, 230),
            (LayerProperty::MulColorBlue, 810),
            (LayerProperty::MulColorAlpha, 0),
            (LayerProperty::BlendType, 3),
            (LayerProperty::FragmentShader, 4),
            (LayerProperty::ClipMode, 2),
            (LayerProperty::ClipFromX, 400),
            (LayerProperty::ClipFromY, 300),
            (LayerProperty::ClipToX, 450),
            (LayerProperty::ClipToY, 350),
            (LayerProperty::BlurRadius, 9000),
            (LayerProperty::MosaicSize, 17),
            (LayerProperty::DissolveIntensity, 1000),
        ] {
            layer
                .properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value as f32);
        }
        layer.pre_render(context, &parent);
        assert_eq!(
            baseline,
            draw(context, &layer, false, PassKind::Opaque, &parent, 0),
            "Menu{menu}: generic shading/clipping/effects are ignored"
        );
        frames += 1;
        assert_eq!(
            baseline,
            draw(context, &layer, false, PassKind::Opaque, &parent, 4),
            "Native >=stencil accepts equal references"
        );
        frames += 1;
        let hidden = draw(context, &layer, false, PassKind::Opaque, &parent, 5);
        frames += 1;
        assert!(hidden.chunks_exact(4).all(|p| p == [73, 31, 101, 255]));
        let transparent = draw(context, &layer, false, PassKind::Transparent, &parent, 0);
        frames += 1;
        assert_eq!(transparent, hidden, "Parent has no transparent-pass draw");
        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(120.);
        layer.pre_render(context, &parent);
        let shifted = draw(context, &layer, false, PassKind::Opaque, &parent, 0);
        frames += 1;
        assert_ne!(
            shifted, baseline,
            "Menu{menu}: transform applies during child capture"
        );
        let different_parent = TransformParams {
            transform: glam::Mat4::from_translation(glam::vec3(-243., 97., 0.)),
            ..Default::default()
        };
        assert_eq!(
            shifted,
            draw(
                context,
                &layer,
                false,
                PassKind::Opaque,
                &different_parent,
                0
            ),
            "Presentation must not transform its viewport again"
        );
        frames += 1;
        assert_eq!(
            shifted,
            draw(context, &layer, true, PassKind::Opaque, &parent, 0)
        );
        frames += 1;
        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .enqueue(360., Tween::linear(Ticks::from_f32(60.)));
        let property_frame = layer.properties().reference_initialized_frame();
        let mut frame = FrameId::default();
        frame.advance();
        layer.update(&crate::update::AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(30.),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        assert_eq!(
            layer.properties().get_value(LayerProperty::TranslateX),
            120.
        );
        layer.fast_forward();
        assert_eq!(
            layer.properties().get_value(LayerProperty::TranslateX),
            120.
        );
        assert!(
            !layer
                .properties()
                .property_tweener(LayerProperty::TranslateX)
                .is_idle()
        );
        assert_eq!(layer.reference_effect_phases(), [0.; 3]);
        assert_eq!(
            layer.properties().reference_initialized_frame(),
            property_frame
        );
    }
    super::accusation::reference_retained_wipe_ownership(context, server);
    println!(
        "Murderer native parent: {frames} real1920x1080 frames across all7menus; opaque capture/presentation, stencil, ignored properties, active transforms, frozen property clocks and retained Default/Glass transitions pass"
    );
}
