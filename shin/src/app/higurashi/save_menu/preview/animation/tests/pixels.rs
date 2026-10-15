use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    app::higurashi::save_menu::preview::{
        postprocessing::tests::gpu::NativeShader, resources::thumbnail_size,
    },
    layer::higurashi_properties::INITIAL_VALUES,
};
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

mod regular_blend;

struct ShaderCase {
    kind: i32,
    params: [i32; 8],
    values: [i32; 65],
}
fn shader_cases() -> Vec<ShaderCase> {
    let mut descriptors = vec![(8, [640, 360, 0, 0, 0, 0, 0, 0])];
    descriptors.extend((0..4).map(|mode| (9, [mode, 100, 1500, 0, 0, 0, 0, 0])));
    let mut cases = Vec::new();
    for (kind, params) in descriptors {
        for operation in 0..8 {
            for param in [[700, 200, 1600, 650], [1000, 1000, 1000, 0]] {
                let mut values = INITIAL_VALUES;
                values[6..10].copy_from_slice(&[700, 1700, 800, 650]);
                values[21] = operation;
                values[23..27].copy_from_slice(&param);
                cases.push(ShaderCase {
                    kind,
                    params,
                    values,
                });
            }
        }
    }
    cases
}
fn original_shader_packets(cases: &[ShaderCase]) -> Vec<Native> {
    let mut input = String::new();
    for case in cases {
        // The production clone below resets MT to5489, whose first raw sample
        // is3499211612. The original fixture supplies that one checked sample.
        write!(input, "G {} 3499211612", case.kind).unwrap();
        for value in case.params.into_iter().chain(case.values) {
            write!(input, " {value}").unwrap();
        }
        input.push('\n');
    }
    let mut child = Command::new(
        std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_ANIMATION_SHADERS").unwrap(),
    )
    .args([
        std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
        std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_ANIMATION_SHADER_CAPTURE") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("input.txt"), copy).unwrap();
        std::fs::write(dir.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    output.lines().map(parse).collect()
}
fn native_vertices(words: [u32; 16]) -> [PosTexVertex; 4] {
    std::array::from_fn(|i| PosTexVertex {
        position: vec2(
            f32::from_bits(words[4 * i]),
            f32::from_bits(words[4 * i + 1]),
        ),
        texture_position: vec2(
            f32::from_bits(words[4 * i + 2]),
            f32::from_bits(words[4 * i + 3]),
        ),
    })
}
fn compare_shader_packet(packet: &Packet, draw: &Draw, context: &str) {
    assert_eq!(
        draw.program,
        [6, 9, 7, 8, 10, 11][packet.shader as usize],
        "{context}:actual native ordinary program"
    );
    assert_eq!(draw.blend, native_blend(packet.blend), "{context}:blend");
    // The common comparer also verifies texture, transform, RGBA, parameters
    // and quad geometry. Its default-program assertion is scoped to the older
    // four-profile test, so use an otherwise identical local copy here.
    let default = Draw {
        program: 6,
        blend: draw.blend,
        texture: draw.texture,
        matrix: draw.matrix,
        color: draw.color,
        param: draw.param,
        vertices: draw.vertices,
    };
    equal_draw(packet, &default, context);
}

#[test]
#[ignore = "real updated warmed FocusLine/Rain packets and supplied effects art through six extracted original GLSL programs at both thumbnail extents; no audio/windows/user saves"]
fn higurashi_saved_animation_real_art_pixels_match_original_leaf_shaders() {
    let cases = shader_cases();
    assert_eq!(cases.len(), 80);
    let originals = original_shader_packets(&cases);
    let (device, queue, assets) = reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let shaders = (0..6)
        .map(|operation| NativeShader::new(&device, Some(operation), 2))
        .collect::<Vec<_>>();
    let mut native_packets = 0;
    let mut images = 0;
    for (index, (case, original)) in cases.iter().zip(originals).enumerate() {
        assert!(original.present);
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
        let initial = snapshot(&view);
        for expected in &original.states[..3] {
            equal_state(&initial, expected, &format!("warmed shader case{index}"));
        }
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
            compare_shader_packet(packet, draw, &format!("native shader case{index}"));
        }
        equal_state(
            &snapshot(&view),
            &original.states[3],
            &format!("retained after render{index}"),
        );
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
                    "Saved warmed animation real art",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                view.render(
                    &mut pass,
                    &properties,
                    &HigurashiTransform::default(),
                    0,
                    PassKind::Opaque,
                )
                .unwrap();
                view.render(
                    &mut pass,
                    &properties,
                    &HigurashiTransform::default(),
                    0,
                    PassKind::Transparent,
                )
                .unwrap();
            });
            let expected = reference_render_to_texture(&device, &queue, size, |context, target| {
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        None,
                        "Original particle black clear",
                    );
                    pass.clear(Some(UnormColor::BLACK), None, None);
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
                "Original warmed particle pixels case{index},kind{},canvas{canvas:?},operation{},max channel delta{largest}",
                case.kind, case.values[21]
            );
            images += 1;
        }
    }
    eprintln!(
        "Saved animation real art:80 native warmed shader profiles,{native_packets} original leaf packets,{images} exact pixel comparisons across720p/1080p thumbnail extents; ordinary RGBA/blend2 and extracted original GLSL, no audio/windows/user saves"
    );
}

#[test]
#[ignore = "real Higurashi effects atlas inventory and visible native Sakura missing-art boundary, headless GPU only; no audio/windows/user saves"]
fn higurashi_saved_sakura_missing_supplied_art_is_a_checked_draw_error() {
    let (device, queue, assets) = reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let prepared = shin_tasks::block_on(Prepared::load(&assets, &scenario, 4, &[0; 8])).unwrap();
    let atlas = prepared.atlas.as_ref().unwrap();
    for name in ["focusline", "goldsnow", "hanabira", "rain", "snow"] {
        assert!(atlas.contains(name));
    }
    assert!(!atlas.contains("Sakura"));
    let view = View {
        state: State::new(4, &[0; 8], 1).unwrap(),
        atlas: prepared.atlas.clone(),
    };
    let properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    assert_eq!(
        view.packets(
            &properties,
            &HigurashiTransform::default(),
            PassKind::Transparent
        )
        .unwrap()
        .len(),
        1,
        "The original rawMT1 constructor has one visible warmed Sakura cell"
    );
    reference_render_to_texture(&device, &queue, (228, 128), |context, target| {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Sakura missing supplied art boundary",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
        let error = view
            .render(
                &mut pass,
                &properties,
                &HigurashiTransform::default(),
                0,
                PassKind::Transparent,
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires 'Sakura' in /effects.txa"),
            "{error:#}"
        );
    });
}
