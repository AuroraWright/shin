//! Independent original Core/shader/range evidence for older PIC/BUP material.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::higurashi_properties::INITIAL_VALUES,
};
use glam::{Mat4, Vec4};
use shin_render::ColorBlendType;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

pub(crate) mod gpu;

#[derive(Clone)]
struct Case {
    kind: u32,
    pass: PassKind,
    format: u16,
    opaque: usize,
    transparent: usize,
    values: [i32; 65],
}
struct Original {
    draws: Vec<gpu::Draw>,
    descriptors: Vec<[u32; 30]>,
}
fn word(tokens: &mut std::str::SplitWhitespace<'_>) -> u32 {
    tokens.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(tokens: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(tokens))
}
fn original(cases: &[Case]) -> Vec<Original> {
    let mut input = String::new();
    for case in cases {
        write!(
            input,
            "R {} {} {} {} {}",
            case.kind,
            u32::from(case.pass == PassKind::Opaque),
            case.format,
            case.opaque,
            case.transparent
        )
        .unwrap();
        for value in case.values {
            write!(input, " {value}").unwrap();
        }
        input.push('\n');
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BASIC_FRAGMENTS").unwrap())
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
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_BASIC_FRAGMENTS_CAPTURE") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("input.txt"), copy).unwrap();
        std::fs::write(directory.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(directory.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    output
        .lines()
        .zip(cases)
        .enumerate()
        .map(|(index, (line, case))| {
            let mut tokens = line.split_whitespace();
            assert_eq!(tokens.next(), Some("G"));
            assert_eq!(word(&mut tokens), index as u32);
            let mut original = Original {
                draws: vec![],
                descriptors: vec![],
            };
            loop {
                match tokens.next().unwrap() {
                    "R" => assert_eq!(
                        words::<5>(&mut tokens),
                        [
                            case.kind,
                            u32::from(case.pass == PassKind::Opaque),
                            case.format as u32,
                            case.opaque as u32,
                            case.transparent as u32
                        ]
                    ),
                    "M" => assert_eq!(words::<2>(&mut tokens), [5, 1]),
                    "W" => assert_eq!(words::<3>(&mut tokens), [7, 7, 7]),
                    "B" => {
                        assert_eq!(words::<3>(&mut tokens), [0, 501, 101]);
                    }
                    "P" => original.descriptors.push(words(&mut tokens)),
                    "V" => {
                        assert!(word(&mut tokens) <= 1);
                    }
                    "Y" => {
                        assert_eq!(words::<2>(&mut tokens), [7, 0]);
                    }
                    "S" => {
                        let _ = words::<5>(&mut tokens);
                    }
                    "D" => {
                        let d = words::<86>(&mut tokens);
                        assert_eq!(
                            (d[0], d[1]),
                            (case.kind, u32::from(case.pass == PassKind::Opaque))
                        );
                        assert_eq!((d[4], d[7], d[8], d[9], d[10]), (0, 501, 101, 601, 701));
                        assert_eq!(&d[11..13], &[7, 0]);
                        assert_eq!(&d[13..17], &[4, 0, 0, 2]);
                        assert_eq!(d[17], 0x07ffff);
                        assert_eq!(&d[18..21], &[64, 16, 16]);
                        original.draws.push(gpu::Draw {
                            program: d[2],
                            blend: d[3],
                            index_start: d[6],
                            index_count: d[5],
                            matrix: d[22..38].try_into().unwrap(),
                            color: d[38..42].try_into().unwrap(),
                            param: d[54..58].try_into().unwrap(),
                            stencil: (d[17] >> 16) & 255,
                        });
                    }
                    "E" => {
                        let e = words::<5>(&mut tokens);
                        assert_eq!(e[0] as usize, original.descriptors.len());
                        assert_eq!(e[1] as usize, original.draws.len());
                        assert_eq!(e[4], 0);
                        assert!(tokens.next().is_none());
                        break;
                    }
                    other => panic!("Unreviewed original basic event{other}"),
                }
            }
            original
        })
        .collect()
}
fn properties(case: &Case) -> HigurashiProperties {
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    for (index, value) in case.values.into_iter().enumerate() {
        properties
            .tweener_mut(index)
            .unwrap()
            .fast_forward_to(value as f32);
    }
    properties
}
fn native_blend(blend: ColorBlendType) -> u32 {
    match blend {
        ColorBlendType::Opaque => 1,
        ColorBlendType::Layer1 => 2,
        ColorBlendType::Layer2 => 3,
        ColorBlendType::HigurashiLayer3 => 4,
        other => panic!("Non-native older blend{other:?}"),
    }
}
fn compare(case: &Case, original: &Original) {
    let p = properties(case);
    let params = PictureBlockParams::setup(case.pass, &p.drawable_params());
    let ranges = if p.is_visible() {
        params
            .map(|params| {
                block::submissions(case.format, case.opaque, case.transparent, params)
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };
    assert_eq!(ranges.len(), original.draws.len());
    for (range, draw) in ranges.iter().zip(&original.draws) {
        let params = params.unwrap();
        assert_eq!(
            (
                range.index_start as u32,
                range.index_count as u32,
                native_blend(range.blend)
            ),
            (draw.index_start, draw.index_count, draw.blend)
        );
        assert_eq!(
            draw.program,
            [6, 9, 7, 8, 10, 11][params.fragment_shader as usize]
        );
        assert_eq!(
            draw.color,
            params
                .color_multiplier
                .into_vec4()
                .to_array()
                .map(f32::to_bits)
        );
        assert_eq!(
            draw.param,
            params.fragment_shader_param.to_array().map(f32::to_bits)
        );
    }
}
fn values(operation: usize, blend: usize, alpha: i32) -> [i32; 65] {
    let mut p = INITIAL_VALUES;
    p[0] = -640;
    p[1] = 720;
    p[6..10].copy_from_slice(&[750, 1300, 600, alpha]);
    p[20] = blend as i32;
    p[21] = operation as i32;
    p[23..27].copy_from_slice(&match operation {
        0 => [0, 0, 0, 0],
        1 => [800, 1200, 600, 650],
        2 => [900, 250, 700, 650],
        3 => [650, 300, 850, 400],
        4 => [850, 1100, 600, 700],
        5 => [750, 1250, 1750, 123],
        _ => unreachable!(),
    });
    p
}

#[test]
#[ignore = "original PIC/BUP→69f3c→6a04c index/straight-RGBA/shader/blend matrix; CPU only"]
fn higurashi_basic_fragment_submissions_match_original_core() {
    let mut cases = vec![];
    for kind in [2, 3] {
        for format in [0, 1, 2, 3, 0xfffe] {
            for operation in 0..6 {
                for blend in 0..3 {
                    for alpha in [0, 1, 650, 1000] {
                        for pass in [PassKind::Opaque, PassKind::Transparent] {
                            cases.push(Case {
                                kind,
                                pass,
                                format,
                                opaque: 18,
                                transparent: 12,
                                values: values(operation, blend, alpha),
                            });
                        }
                    }
                }
            }
        }
    }
    let originals = original(&cases);
    let mut packets = 0;
    for (case, native) in cases.iter().zip(&originals) {
        compare(case, native);
        packets += native.draws.len();
    }
    eprintln!(
        "Older basic fragments:{} original PIC/BUP profiles,{packets} exact shader/color/param/index/blend packets;allformats/alpha gates/default andfive operations/transparent-first format2;CPU only",
        cases.len()
    );
}

fn clear(ctx: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let mut pass = ctx.begin_pass(
        TextureTarget {
            view: target,
            kind: TextureTargetKind::Screen,
        },
        Some(ctx.depth_stencil),
        "Basic native comparison background",
    );
    pass.clear(
        Some(
            shin_core::primitives::color::FloatColor4::from_vec4(Vec4::new(0.27, 0.44, 0.71, 0.37))
                .into_unorm(),
        ),
        Some(0),
        Some(1.),
    );
}

#[test]
#[ignore = "real supplied PIC/BUP buffers/partial alpha with original literalGLSL Core packets;Metal offscreen,no audio/windows/user saves"]
fn higurashi_basic_fragment_pixels_match_independent_original_glsl() {
    let (device, queue, server) = reference_gpu_assets();
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let info = &scenario.picture_info[0];
    let picture: Arc<Picture> = server
        .load_sync(format!(
            "/picture/{}.pic",
            selected_asset_name(info.name.as_str(), 0)
        ))
        .unwrap();
    let info = &scenario.bustup_info[0];
    let bustup: Arc<HigurashiBustup> = shin_tasks::block_on(server.load_with_args(
        format!("/bustup/{}.bup", selected_asset_name(info.name.as_str(), 0)),
        BustupArgs {
            expression: info.emotion.as_str().to_owned(),
            character_id: CharacterId::from_metadata(info.lipsync_character_id),
            disable_animations: false,
        },
    ))
    .unwrap();
    let picture_block = picture.blocks.iter_placements().next().unwrap().2;
    let bustup_block = bustup
        .body
        .blocks
        .iter()
        .flatten()
        .find(|b| b.transparent_rect_count > 0)
        .unwrap();
    let sources = [(2, picture_block), (3, bustup_block.as_ref())];
    let mut cases = vec![];
    for (kind, source) in &sources {
        for operation in 0..6 {
            for blend in 0..3 {
                for alpha in [650, 1000] {
                    for pass in [PassKind::Opaque, PassKind::Transparent] {
                        cases.push(Case {
                            kind: *kind,
                            pass,
                            format: source.native_format,
                            opaque: source.opaque_rect_count * 6,
                            transparent: source.transparent_rect_count * 6,
                            values: values(operation, blend, alpha),
                        });
                    }
                }
            }
        }
    }
    let originals = original(&cases);
    for (case, native) in cases.iter().zip(&originals) {
        compare(case, native);
    }
    let mut frames = 0;
    let mut legacy_different = 0;
    let mut fill2_different = 0;
    let mut spatial = 0;
    for (pair, (case_pair, original_pair)) in cases
        .chunks_exact(2)
        .zip(originals.chunks_exact(2))
        .enumerate()
    {
        let source = sources[pair / 36].1;
        let draws = original_pair
            .iter()
            .flat_map(|o| o.draws.clone())
            .collect::<Vec<_>>();
        assert!(!draws.is_empty());
        let matrix = Mat4::from_cols_array(&draws[0].matrix.map(f32::from_bits));
        for extent in [(228, 128), (342, 192)] {
            let render = |legacy| {
                reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    clear(ctx, target);
                    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Actual older blocks",
                    );
                    for case in case_pair {
                        let p = properties(case);
                        if let Some(params) =
                            PictureBlockParams::setup(case.pass, &p.drawable_params())
                        {
                            let builder = RenderRequestBuilder::new()
                                .depth_stencil_shorthand(7, false, false);
                            if legacy {
                                super::super::picture_layer::render_block(
                                    source, &mut pass, builder, params, matrix,
                                );
                            } else {
                                block::render(source, &mut pass, builder, params, matrix);
                            }
                        }
                    }
                })
            };
            let actual = render(false);
            let expected = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                clear(ctx, target);
                gpu::replay(ctx, target, source, &draws);
            });
            let different = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
            let maximum = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                different == 0,
                "independent native basic pixels pair{pair}/{extent:?}:channels{different},max{maximum}"
            );
            let legacy = render(true);
            let changed = legacy != expected;
            legacy_different += usize::from(changed);
            fill2_different += usize::from(changed && case_pair[0].values[21] == 3);
            spatial += usize::from(
                actual
                    .chunks_exact(4)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    > 3,
            );
            frames += 1;
        }
    }
    assert_eq!(frames, 144);
    assert!(
        spatial >= 72,
        "real visible source pattern coverage:{spatial}"
    );
    assert!(
        fill2_different >= 4,
        "independent old Fill2 must expose the later shader defect"
    );
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "Older basic GPU:{frames} exact independent originalGLSL frames720p/1080p,{spatial} nonuniform realPIC/BUP frames,{legacy_different} later-path differences,{fill2_different} Fill2 differences;actual indexed buffers/nativeformats/partialalpha/three regular blends;no audio/windows/user saves"
    );
}
