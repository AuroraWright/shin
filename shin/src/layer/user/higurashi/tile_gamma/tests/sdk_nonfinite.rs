//! Complete original nonfinite CPU packets and actual-owner literal-shader
//! replay. Reference vertices/colors/states are immutable native captures.
use super::*;
use shin_core::time::Ticks;

fn pinned(raw: &[u8], bytes: usize, hash: u64) {
    assert_eq!(raw.len(), bytes);
    let actual = raw.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    });
    assert_eq!(actual, hash, "frozen original nonfinite capture drift");
}
fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn u64_at(raw: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}

fn directory() -> PathBuf {
    root().join("reverse-engineering/higurashi/tile-gamma-active-clip-20261014/allocation-closure/whole-tile-nonfinite")
}
fn case_words(words: &[u32; 174], index: usize) -> Case {
    let values = &words[5..109];
    let from = &words[109..174];
    let mut properties = HigurashiProperties::from_seeds(values[65..69].try_into().unwrap());
    for lane in 0..65 {
        let tween = properties.tweener_mut(lane).unwrap();
        tween.fast_forward_to(f32::from_bits(from[lane]));
        if from[lane] != values[lane] {
            // Use actual public tween controls, preserving from=0/to=Inf
            // versus from=to=Inf, both sampled at elapsed/duration=1.
            tween
                .control(f32::from_bits(values[lane]), Ticks::from_f32(1.), 0)
                .unwrap();
            tween.update(Ticks::from_f32(1.));
        }
    }
    for lane in 0..4 {
        properties.oscillators[lane].phase = f32::from_bits(values[69 + lane]);
    }
    let tile = Tile {
        color: FloatColor4::from_vec4(Vec4::from_array(std::array::from_fn(|i| {
            f32::from_bits(values[96 + i])
        }))),
        rect: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(values[100 + i]))),
    };
    Case {
        index,
        raw: std::array::from_fn(|i| f32::from_bits(values[i])),
        properties,
        parent: HigurashiTransform {
            transform: Mat4::from_cols_array(&std::array::from_fn(|i| {
                f32::from_bits(values[73 + i])
            })),
            camera: Vec3::from_array(std::array::from_fn(|i| f32::from_bits(values[89 + i]))),
            inherited: Vec2::from_array(std::array::from_fn(|i| f32::from_bits(values[92 + i]))),
            wobble: Vec2::from_array(std::array::from_fn(|i| f32::from_bits(values[94 + i]))),
        },
        tile,
        requested_pass: if words[4] == 1 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        },
        original: None,
    }
}

#[test]
fn higurashi_tile_gamma_sdk_nonfinite_packets_match_original() {
    let directory = root().join("reverse-engineering/higurashi/tile-gamma-active-clip-20261014/allocation-closure/whole-tile-nonfinite");
    let input = fs::read(directory.join("input.bin")).unwrap();
    let packets = fs::read(directory.join("probe-nonfinite/O0-plain/packets.bin")).unwrap();
    let getters = fs::read(directory.join("probe-nonfinite/O0-plain/getters.bin")).unwrap();
    pinned(&input, 12_383_232, 0x13b1_f959_5517_2425);
    pinned(&packets, 18_063_568, 0x0cd0_76b0_434c_d30c);
    pinned(&getters, 3_131_392, 0x051a_f850_ff2d_9685);
    assert_eq!(u64_at(&packets, 0), 0x3130_304d_4f44_4754);
    assert_eq!(u64_at(&packets, 8), 17_792);
    let mut at = 16;
    let (mut drawn, mut skipped, mut regressions) = (0, 0, 0);
    let mut clip_classes = [0usize; 3];
    let (mut tile_words, mut mask_words) = (0, 0);
    for index in 0..17_792 {
        let words: [u32; 174] = std::array::from_fn(|i| u32_at(&input, index * 696 + i * 4));
        let facts: [u64; 24] = std::array::from_fn(|i| u64_at(&packets, at + i * 8));
        assert_eq!(facts[0], 0x3130_3053_4143_4754);
        assert_eq!(facts[1], index as u64);
        let metadata: [u64; 5] = std::array::from_fn(|i| u64::from(words[i]));
        assert_eq!(facts[2..7], metadata);
        let case = case_words(&words, index);
        let properties = &case.properties;
        let tile = &case.tile;
        let p = properties.drawable_params();
        let requested = if words[4] == 1 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        };
        let visible = properties.is_visible();
        let submitted = visible && tile.pass_kind(&p) == requested;
        assert_eq!(
            submitted,
            facts[23] == 1,
            "original pass/visibility profile{index}"
        );
        let alpha = tile.color.a * p.color_multiplier.a;
        if visible && alpha.is_nan() {
            assert_eq!(tile.pass_kind(&p), PassKind::Transparent);
            // The previous finite-only clamp converted NaN to1. Pin the
            // regression to original pass outcomes, not an inferred new rule.
            let old_alpha = alpha.min(1.).max(0.);
            let old_opaque = p.blend_type == shin_render::LayerBlendType::Type1 && old_alpha >= 1.;
            regressions += usize::from(old_opaque);
        }
        let native_clip: [u32; 4] =
            std::array::from_fn(|lane| u32_at(&getters, index * 176 + 48 + lane * 16));
        let clip = Clip::from_properties(&properties);
        assert_eq!(
            clip.0.map(f32::to_bits),
            native_clip,
            "native clip getter profile{index}"
        );
        assert_eq!(clip.active(), facts[22] == 1);
        for lane in clip.0 {
            clip_classes[if lane.is_nan() {
                0
            } else if lane.is_infinite() {
                2
            } else {
                1
            }] += 1;
        }
        if submitted {
            let endpoints = properties.values()[44..48].try_into().unwrap();
            let material = if clip.active() {
                let active = ActiveClip::new(tile, &p, endpoints).unwrap();
                assert_eq!(active.clip.0.map(f32::to_bits), native_clip);
                active.material
            } else {
                NoMask::new(tile, &p, endpoints, clip).unwrap().0
            };
            assert_eq!(material.pass_kind(), requested);
            assert_eq!(
                u64::from(native_blend(material.color_blend_type())),
                facts[13]
            );
            let upload_at = at + 192 + 128;
            let upload = &packets[upload_at..upload_at + facts[9] as usize];
            let mut actual = vertex_words(&material.vertices());
            for lane in (3..16).step_by(4) {
                actual[lane] = bgra(actual[lane]);
            }
            let native: Vec<_> = (0..16)
                .map(|i| u32_at(upload, facts[10] as usize + i * 4))
                .collect();
            assert_eq!(actual, native, "Tile vertices profile{index}");
            let mut matrix = properties
                .composed_transform(&case.parent)
                .compute_native_tile_transform()
                .to_cols_array()
                .map(f32::to_bits);
            if words[3] != 0 {
                for lane in (1..16).step_by(4) {
                    matrix[lane] ^= 0x8000_0000;
                }
            }
            let matrix_offset = facts[11] as usize + if words[3] != 0 { 64 } else { 0 };
            let native: Vec<_> = (0..16)
                .map(|i| u32_at(upload, matrix_offset + i * 4))
                .collect();
            assert_eq!(
                matrix.as_slice(),
                native,
                "Tile bound matrix profile{index}"
            );
            tile_words += 32;
            if clip.active() {
                // The bounded original-command decoder separately verifies
                // these source upload offsets against the actual bound draws.
                let native: Vec<_> = (0..16).map(|i| u32_at(upload, 128 + i * 4)).collect();
                assert_eq!(
                    vertex_words(&clip.vertices()),
                    native,
                    "mask vertices profile{index}"
                );
                let mut matrix = Clip::native_projection().to_cols_array().map(f32::to_bits);
                if words[3] != 0 {
                    for lane in (1..16).step_by(4) {
                        matrix[lane] ^= 0x8000_0000;
                    }
                }
                let offset = 64 + if words[3] != 0 { 128 } else { 0 };
                let native: Vec<_> = (0..16).map(|i| u32_at(upload, offset + i * 4)).collect();
                assert_eq!(matrix.as_slice(), native, "mask matrix profile{index}");
                mask_words += 32;
            }
        }
        drawn += usize::from(submitted);
        skipped += usize::from(!submitted);
        at += 192 + 128 + facts[9] as usize + facts[8] as usize * 4;
    }
    assert_eq!(at, packets.len());
    assert_eq!((drawn, skipped), (8464, 9328));
    assert_eq!(clip_classes, [33760, 36288, 1120]);
    assert_eq!(tile_words, 8464 * 32);
    assert_eq!(mask_words, 2752 * 32);
    assert!(
        regressions > 0,
        "original NaN-alpha regression must be covered"
    );
    println!(
        "TileGamma nonfinite:17792 exact original owner pass/getter profiles;8464 draws/9328 skips;{regressions} NaN-alpha clamp regressions;clip lanes33760NaN/36288finite/1120infinite;complete Tile and mask vertices/matrices match"
    );
}

fn vertex_words(vertices: &[shin_render::shaders::types::vertices::PosColVertex; 4]) -> Vec<u32> {
    bytemuck::cast_slice::<_, u8>(vertices)
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
#[ignore = "8064 actual owner/original nonfinite frames at720p/1080p; literal GLSL and depth probes; no audio/window/save"]
fn higurashi_tile_gamma_sdk_nonfinite_owner_pixels_match_original() {
    use crate::{
        adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
        layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let raw = fs::read(
        directory()
            .parent()
            .unwrap()
            .join("whole-tile-nonfinite-integration/gpu-cases.json"),
    )
    .unwrap();
    pinned(&raw, 2_874_569, 0x9b77_da23_b4c7_20bb);
    let rows: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2016);
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let extents = [(1280, 720), (1920, 1080)];
    let backgrounds = extents.map(|extent| {
        reference_render_to_texture(&device, &queue, extent, |ctx, target| {
            initialize(ctx, target, 0)
        })
    });
    let probe = owner::canary_draw();
    let (mut frames, mut visible, mut changed_probes) = (0, 0, 0);
    let mut coverage = std::collections::BTreeMap::<String, [usize; 4]>::new();
    let mut patterns = std::collections::BTreeSet::new();
    for (ordinal, row) in rows.iter().enumerate() {
        let index = row["index"].as_u64().unwrap() as usize;
        let words: Vec<u32> = row["input"]
            .as_array()
            .unwrap()
            .iter()
            .chain(row["from_values"].as_array().unwrap())
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        let case = case_words(&words.try_into().unwrap(), index);
        let layer = owner::owner(&case);
        let draws: Vec<_> = row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(sdk_active::draw)
            .collect();
        let category = row["selection_category"].as_str().unwrap();
        let count = coverage.entry(category.to_owned()).or_default();
        count[usize::from(row["emitted"].as_bool().unwrap())] += 1;
        patterns.insert(row["pattern"].as_u64().unwrap());
        for (extent_index, extent) in extents.into_iter().enumerate() {
            let mut natural = None;
            for with_probe in [false, true] {
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    initialize(ctx, target, 0);
                    {
                        let mut pass = ctx.begin_pass_preserving_depth_stencil(
                            TextureTarget {
                                view: target,
                                kind: TextureTargetKind::Screen,
                            },
                            ctx.depth_stencil,
                            "Nonfinite TileGamma actual owner",
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
                        for draw in &draws {
                            renderer.draw(ctx, target, draw, None, None);
                        }
                        if with_probe {
                            renderer.draw(ctx, target, &probe, None, None);
                        }
                    });
                owner::exact(
                    &actual,
                    &expected,
                    &format!(
                        "nonfinite/profile{index}/{extent:?}/probe{with_probe}/{}",
                        row["selection_name"]
                    ),
                );
                if with_probe {
                    let changed = usize::from(actual != natural.take().unwrap());
                    changed_probes += changed;
                    count[3] += changed;
                } else {
                    let changed = usize::from(actual != backgrounds[extent_index]);
                    visible += changed;
                    count[2] += changed;
                    natural = Some(actual);
                }
                frames += 1;
            }
        }
        if ordinal % 128 == 127 {
            println!(
                "NONFINITE checked_cases={}/2016 frames={frames}",
                ordinal + 1
            );
        }
    }
    assert_eq!(patterns.len(), 556);
    assert_eq!(frames, 8064);
    assert!(
        visible > 0 && changed_probes > 0,
        "natural/probe controls must be observable"
    );
    for category in [
        "control",
        "clip",
        "color",
        "tint",
        "shader",
        "rectangle",
        "clip-interaction",
        "material-interaction",
    ] {
        let count = coverage.get(category).unwrap();
        assert!(
            count[0] > 0 && count[1] > 0 && count[2] > 0 && count[3] > 0,
            "{category}:missing draw/skip/natural/probe control {count:?}"
        );
    }
    println!(
        "TileGamma nonfinite:8064 exact owner/original720p1080p frames;2016 states/all556 patterns/all521 unique literal outputs/opposite-pass controls;{visible} visible natural frames/{changed_probes} observable probe frames;coverage={coverage:?};local Metal GLSL replay, no native Switch framebuffer claim"
    );
}
