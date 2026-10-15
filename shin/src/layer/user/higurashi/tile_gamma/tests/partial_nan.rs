//! All expectations come from the frozen untouched/observed original CPU
//! captures. No-mask returns before any SDK call. Pixel checks replay literal
//! program1 packets on desktop Metal; they are not Switch framebuffer proof.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

fn pinned(bytes: &[u8], length: usize, expected: u64) {
    assert_eq!(bytes.len(), length);
    let fnv = bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    assert_eq!(fnv, expected, "frozen partial-NaN corpus drift");
}

fn group<const N: usize>(tokens: &[&str], at: &mut usize, label: &str) -> [u32; N] {
    assert_eq!(tokens[*at], label);
    let words = std::array::from_fn(|i| {
        let word = tokens[*at + 1 + i];
        assert_eq!(word.len(), 8);
        u32::from_str_radix(word, 16).unwrap()
    });
    *at += N + 1;
    words
}

fn scalar_lanes(values: [u32; 4]) -> [u32; 16] {
    std::array::from_fn(|i| if i % 4 == 0 { values[i / 4] } else { 0 })
}

fn cases() -> Vec<Case> {
    let base = root().join("reverse-engineering/higurashi/tile-gamma-partial-nan-capture");
    let input = fs::read(base.join("input.txt")).unwrap();
    pinned(&input, 902_400, 0x2799_b1f5_c57a_f015);
    let output = fs::read(base.join("output.txt")).unwrap();
    pinned(&output, 607_570, 0x45ec_f0cc_3974_df0a);
    for name in ["output-repeat.txt", "output-O0.txt", "output-O3.txt"] {
        assert_eq!(output, fs::read(base.join(name)).unwrap(), "{name}");
    }
    let input = String::from_utf8(input).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert_eq!(input.lines().count(), 960);
    assert_eq!(output.lines().count(), 960);
    let mut cases = Vec::new();
    let mut cache = [0; 9];
    for (index, (input, output)) in input.lines().zip(output.lines()).enumerate() {
        let mut case = read_input(input, index);
        let fields: Vec<_> = output.split_ascii_whitespace().collect();
        assert_eq!(fields[0], "T");
        assert_eq!(fields[1].parse::<usize>().unwrap(), index);
        assert!(matches!(fields[2], "0" | "1"));
        let mut at = 3;
        let clip = group::<4>(&fields, &mut at, "C");
        assert_eq!(
            Clip::from_properties(&case.properties).0.map(f32::to_bits),
            clip
        );
        assert!(!Clip::from_properties(&case.properties).active());
        assert_eq!(group::<4>(&fields, &mut at, "N"), [CANONICAL_NAN_BITS; 4]);
        let after_cache = group::<9>(&fields, &mut at, "F");
        assert_eq!(
            group::<2>(&fields, &mut at, "E"),
            [0, if index < 3 { 0x10 } else { 0x0800_0011 }]
        );
        if fields[2] == "1" {
            let packet = group::<41>(&fields, &mut at, "P");
            let material = group::<9>(&fields, &mut at, "I");
            let before = group::<16>(&fields, &mut at, "B");
            let after = group::<16>(&fields, &mut at, "A");
            assert_eq!(group::<16>(&fields, &mut at, "Q"), scalar_lanes(clip));
            let expected_lanes = [
                (case.tile.rect.y + case.tile.rect.w).to_bits(),
                case.tile.rect.y.to_bits(),
                clip[2],
                clip[3],
            ];
            assert_eq!(before, scalar_lanes(expected_lanes));
            if is_gamma(&case) {
                assert_eq!(after, before);
            }
            cache = [1, packet[33], 7, 0, 7, 0, 0, 0, 0xffff];
            case.original = Some(Original {
                packet,
                input: material,
                before,
                after,
            });
        }
        assert_eq!(at, fields.len(), "profile{index}:trailing words");
        assert_eq!(after_cache, cache);
        cases.push(case);
    }
    cases
}

fn check(cases: &[Case]) {
    let (mut draws, mut gamma, mut words) = (0, 0, 0);
    let mut alphas = std::collections::BTreeMap::new();
    let mut masks = std::collections::BTreeSet::new();
    for case in cases {
        let p = case.properties.drawable_params();
        let endpoints: [f32; 4] = case.properties.values()[44..48].try_into().unwrap();
        let mask = endpoints
            .iter()
            .enumerate()
            .fold(0, |mask, (i, v)| mask | (u8::from(v.is_nan()) << i));
        masks.insert(mask);
        assert_eq!(
            case.original.is_some(),
            case.properties.is_visible() && case.requested_pass == case.tile.pass_kind(&p)
        );
        // Validate every profile, including the 480 original pass skips.
        let mut forced = p;
        forced.fragment_shader = LayerFragmentShader::Gamma;
        let prepared = NoMask::new(
            &case.tile,
            &forced,
            endpoints,
            Clip::from_endpoints(endpoints),
        )
        .unwrap();
        assert_eq!(prepared.0.pass_kind(), case.tile.pass_kind(&p));
        assert_eq!(
            prepared.0.color_blend_type(),
            case.tile.color_blend_type(&p)
        );
        let Some(original) = &case.original else {
            continue;
        };
        let vertices = if is_gamma(case) {
            gamma += 1;
            *alphas
                .entry(prepared.0.vertices()[0].color.0 >> 24)
                .or_insert(0) += 1;
            assert_eq!(original.input[8], 5);
            prepared.0.vertices()
        } else {
            assert_eq!(original.input[8], 0);
            case.tile.vertices(&p)
        };
        let mut packet = Vec::new();
        for vertex in vertices {
            let position = vertex.position;
            packet.extend(position.to_array().map(f32::to_bits));
            packet.push(bgra(vertex.color.0));
        }
        packet.extend(
            case.properties
                .composed_transform(&case.parent)
                .compute_native_tile_transform()
                .to_cols_array()
                .map(f32::to_bits),
        );
        packet.extend([
            1,
            native_blend(case.tile.color_blend_type(&p)),
            7,
            0,
            4,
            0,
            0,
            2,
            0x7ffff,
        ]);
        assert_eq!(packet, original.packet, "profile{}", case.index);
        let clamped = (case.tile.color.into_vec4() * p.color_multiplier.into_vec4())
            .min(Vec4::ONE)
            .max(Vec4::ZERO);
        assert_eq!(original.input[..4], clamped.to_array().map(f32::to_bits));
        assert_eq!(
            original.input[4..8],
            p.shader_param.to_array().map(f32::to_bits)
        );
        draws += 1;
        words += packet.len();
    }
    assert_eq!((draws, gamma, words), (480, 240, 19_680));
    assert_eq!(masks, (1..=15).collect());
    assert_eq!(alphas, [(0, 192), (191, 24), (255, 24)].into());
    println!(
        "TileGamma partial NaN:960 profiles/480 packets/19680 exact words/240 Gamma;15 masks/two quiet payloads;alpha0x192/191x24/255x24"
    );
}

#[test]
fn higurashi_tile_gamma_partial_nan_packets_match_original() {
    check(&cases());
}

#[test]
fn higurashi_tile_gamma_no_mask_requires_inactive_matching_clip() {
    let source = &super::cases()[131];
    let p = source.properties.drawable_params();
    let qnan = f32::from_bits(0x7fc1_2345);
    let valid = [qnan, 0., 1280., 720.];
    assert!(NoMask::new(&source.tile, &p, valid, Clip::from_endpoints(valid)).is_ok());
    // A supplied canonical sentinel cannot replace the actual partial result.
    assert!(
        NoMask::new(
            &source.tile,
            &p,
            valid,
            Clip([f32::from_bits(CANONICAL_NAN_BITS); 4])
        )
        .is_err()
    );
    for endpoints in [
        [f32::from_bits(0x7f81_2345), 0., 1280., 720.],
        [f32::from_bits(0xff81_2345), 0., 1280., 720.],
        [qnan, 0., 1280., f32::INFINITY],
        [qnan, -f32::MAX, 1280., f32::MAX],
        [f32::NEG_INFINITY, 0., f32::INFINITY, 720.],
    ] {
        assert!(
            NoMask::new(&source.tile, &p, endpoints, Clip::from_endpoints(endpoints)).is_ok(),
            "{endpoints:?}"
        );
    }
    let active = [1., 0., 1280., 720.];
    assert!(NoMask::new(&source.tile, &p, active, Clip::from_endpoints(active)).is_err());
    let mut invalid_shader = p;
    invalid_shader.fragment_shader = LayerFragmentShader::Default;
    assert!(
        NoMask::new(
            &source.tile,
            &invalid_shader,
            valid,
            Clip::from_endpoints(valid)
        )
        .is_err()
    );
}

#[test]
#[ignore = "960 original partial-NaN owner frames, natural + depth probes at720p/1080p; literal Fill SPIR-V; no audio/windows/saves"]
fn higurashi_tile_gamma_partial_nan_owner_pixels_match_original() {
    let cases = cases();
    check(&cases);
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let probe = owner::canary_draw();
    let mut frames = 0;
    let mut visible_pairs = [0; 2];
    let mut masks = std::collections::BTreeSet::new();
    // Signed payloads produce identical packets; CPU proves both. Render all
    // masks/families/blends/Default+Gamma for one payload, retaining geometry.
    for pair in cases.chunks_exact(2) {
        let [default, gamma] = pair else {
            unreachable!()
        };
        if !default.raw[44..48]
            .iter()
            .any(|v| v.to_bits() == 0x7fc1_2345)
            || default.original.is_none()
        {
            continue;
        }
        assert!(gamma.original.is_some());
        let mask = default.raw[44..48]
            .iter()
            .enumerate()
            .fold(0, |mask, (i, v)| mask | (u8::from(v.is_nan()) << i));
        masks.insert(mask);
        for extent in [(1280, 720), (1920, 1080)] {
            for with_probe in [false, true] {
                let mut baseline = None;
                for case in [default, gamma] {
                    let layer = owner::owner(case);
                    let actual =
                        reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                            initialize(ctx, target, 0);
                            {
                                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                                    TextureTarget {
                                        view: target,
                                        kind: TextureTargetKind::Screen,
                                    },
                                    ctx.depth_stencil,
                                    "Partial NaN actual owner",
                                );
                                layer
                                    .render(&mut pass, &case.parent, 7, case.requested_pass)
                                    .unwrap();
                            }
                            if with_probe {
                                renderer.draw(ctx, target, &probe, None, None);
                            }
                        });
                    let expected =
                        reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                            initialize(ctx, target, 0);
                            renderer.draw(
                                ctx,
                                target,
                                &owner::native_draw(case.original.as_ref().unwrap()),
                                None,
                                None,
                            );
                            if with_probe {
                                renderer.draw(ctx, target, &probe, None, None);
                            }
                        });
                    owner::exact(
                        &actual,
                        &expected,
                        &format!(
                            "partialNaN/profile{}/{extent:?}/probe{with_probe}",
                            case.index
                        ),
                    );
                    if is_gamma(case) {
                        visible_pairs[usize::from(with_probe)] +=
                            usize::from(actual != baseline.take().unwrap());
                    } else {
                        baseline = Some(actual);
                    }
                    frames += 1;
                }
            }
        }
    }
    assert_eq!(frames, 960);
    assert_eq!(masks, (1..=15).collect());
    assert!(
        visible_pairs.into_iter().all(|count| count > 0),
        "Default/Gamma controls must be observable with and without depth probe"
    );
    println!(
        "TileGamma partial NaN:960 byte-exact owner/original720p1080p frames, natural + depth probes;{visible_pairs:?} visible Default/Gamma pairs;all15 masks/two finite families/four blends"
    );
}
