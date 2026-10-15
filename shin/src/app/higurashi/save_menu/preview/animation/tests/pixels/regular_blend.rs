use super::*;

#[test]
#[ignore = "original warmed Focus/Rain blend4 packets and literal GLSL over a nonblack partial-alpha background at both thumbnail extents; pure GPU, no audio/windows/user saves"]
fn higurashi_saved_animation_type3_pixels_match_original_reverse_subtract() {
    // Keep the frozen80 ordinary-shader profiles intact. These ten new cases
    // isolate older Type3 for Focus and all four actual supplied Rain modes.
    let mut cases = shader_cases()
        .into_iter()
        .filter(|case| {
            matches!(case.values[21], 0 | 4) && case.values[23..27] == [700, 200, 1600, 650]
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 10);
    for case in &mut cases {
        case.values[20] = 2;
    }
    let originals = original_shader_packets(&cases);
    let (device, queue, assets) = reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let shaders = (0..6)
        .map(|operation| NativeShader::new(&device, Some(operation), 4))
        .collect::<Vec<_>>();
    let background_bytes = [173u8, 127, 91, 113];
    let background = UnormColor::from_rgba(
        background_bytes[0],
        background_bytes[1],
        background_bytes[2],
        background_bytes[3],
    );
    assert!(background_bytes[..3].iter().all(|&channel| channel > 0));
    assert!((1..255).contains(&background_bytes[3]));
    let mut images = 0;
    let mut native_packets = 0;
    let mut changed_rgb_pixels = 0;
    for (index, (case, original)) in cases.iter().zip(originals).enumerate() {
        assert!(original.present && !original.draws.is_empty());
        assert_eq!(
            (original.os, original.mt, original.scopes),
            (4, 1, [original.draws.len() as u32, 1, 1])
        );
        let prepared =
            shin_tasks::block_on(Prepared::load(&assets, &scenario, case.kind, &case.params))
                .unwrap();
        let view = crate::engine_random::with_reference_seed(5489, || {
            prepared.clone_refresh().unwrap().unwrap()
        });
        let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
        for (property, value) in case.values.into_iter().enumerate() {
            properties
                .tweener_mut(property)
                .unwrap()
                .fast_forward_to(value as f32);
        }
        let packets = view
            .packets(
                &properties,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(packets.len(), original.draws.len());
        for (packet, draw) in packets.iter().zip(&original.draws) {
            assert_eq!(packet.blend, ColorBlendType::HigurashiLayer3);
            assert_eq!(draw.blend, 4);
            compare_shader_packet(packet, draw, &format!("native Type3 case{index}"));
        }
        native_packets += original.draws.len();
        for canvas in [(1280, 720), (1920, 1080)] {
            let size = thumbnail_size(canvas);
            let actual = reference_render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Higurashi Type3 actual particle background",
                );
                pass.clear(Some(background), Some(0), Some(1.));
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    view.render(
                        &mut pass,
                        &properties,
                        &HigurashiTransform::default(),
                        0,
                        kind,
                    )
                    .unwrap();
                }
            });
            let expected = reference_render_to_texture(&device, &queue, size, |context, target| {
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        None,
                        "Original Type3 particle partial-alpha background",
                    );
                    pass.clear(Some(background), None, None);
                }
                for draw in &original.draws {
                    let operation = [6, 9, 7, 8, 10, 11]
                        .iter()
                        .position(|&program| program == draw.program)
                        .unwrap();
                    let texture = [
                        "Sakura",
                        "focusline",
                        "rain",
                        "hanabira",
                        "snow",
                        "goldsnow",
                    ][draw.texture as usize];
                    shaders[operation].draw_quad(
                        context,
                        prepared.atlas.as_ref().unwrap().get(texture).as_source(),
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        Mat4::from_cols_array(&draw.matrix.map(f32::from_bits)),
                        Vec4::from_array(draw.color.map(f32::from_bits)),
                        Vec4::from_array(draw.param.map(f32::from_bits)),
                        &native_vertices(draw.vertices),
                    );
                }
            });
            let differences = actual.iter().zip(&expected).filter(|(a, e)| a != e).count();
            let largest = actual
                .iter()
                .zip(&expected)
                .map(|(a, e)| a.abs_diff(*e))
                .max()
                .unwrap();
            assert_eq!(
                differences, 0,
                "Original particle Type3 case{index},kind{},canvas{canvas:?},operation{},max channel delta{largest}",
                case.kind, case.values[21]
            );
            let changed = expected
                .chunks_exact(4)
                .filter(|rgba| rgba[..3] != background_bytes[..3])
                .count();
            assert!(
                changed > 0,
                "Type3 case{index},canvas{canvas:?} needs visible RGB changes"
            );
            changed_rgb_pixels += changed;
            images += 1;
        }
    }
    assert_eq!(images, 20);
    eprintln!(
        "Saved animation Type3:10 original warmed profiles,{native_packets} exact leaf packets,{images} byte-identical frames,{changed_rgb_pixels} changed RGB pixels over nonblack partial-alpha background; native ReverseSubtract/SrcAlpha and extracted GLSL, no audio/windows/user saves"
    );
}
