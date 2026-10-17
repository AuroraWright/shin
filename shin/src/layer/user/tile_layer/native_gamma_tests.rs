//! Compare production material gates, Gamma, geometry and G12 to complete
//! original SDK uploads. These are frozen native results, not model fixtures.
use super::*;
use glam::Vec2;
use serde_json::Value;
use std::{fs, path::PathBuf};

fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures/umineko-sdk-audit-20261015/actual-sdk/tile-state/whole-tile/domains")
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn words(value: &Value, field: &str) -> Vec<u32> {
    value[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect()
}
fn vector(words: &[u32]) -> Vec4 {
    Vec4::from_array(std::array::from_fn(|i| f32::from_bits(words[i])))
}
fn input(
    case: &Value,
) -> (
    TileLayerImpl,
    DrawableParams,
    TransformParams,
    DrawableClipParams,
    PassKind,
) {
    let owner = words(case, "owner");
    let parent = words(case, "parent");
    let material = words(case, "material");
    let clip = words(case, "clip");
    (
        TileLayerImpl::new(
            FloatColor4::from_vec4(vector(&owner[23..27])),
            vector(&owner[27..31]),
        ),
        DrawableParams {
            color_multiplier: FloatColor4::from_vec4(vector(&material)),
            blend_type: [
                LayerBlendType::Type1,
                LayerBlendType::Type2,
                LayerBlendType::Type3,
            ][material[4] as usize],
            fragment_shader: [
                LayerFragmentShader::Default,
                LayerFragmentShader::Mono,
                LayerFragmentShader::Fill,
                LayerFragmentShader::Fill2,
                LayerFragmentShader::Negative,
                LayerFragmentShader::Gamma,
            ][material[5] as usize],
            shader_param: vector(&material[6..10]),
        },
        TransformParams {
            transform: Mat4::from_cols_array(&std::array::from_fn(|i| f32::from_bits(parent[i]))),
            wobble_translation: Vec2::new(f32::from_bits(parent[21]), f32::from_bits(parent[22])),
            ..Default::default()
        },
        DrawableClipParams {
            mode: [
                crate::layer::render_params::DrawableClipMode::None,
                crate::layer::render_params::DrawableClipMode::Clip,
                crate::layer::render_params::DrawableClipMode::ClipIgnoreTransform,
            ][clip[0] as usize],
            area: vector(&clip[1..5]),
        },
        if case["requested_pass"] == 1 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        },
    )
}

#[test]
fn umineko_tile_domains_match_original_sdk_uploads() {
    let corpus: Value =
        serde_json::from_slice(&fs::read(directory().join("corpus.json")).unwrap()).unwrap();
    let raw = fs::read(directory().join("probe-initial/O0-0-plain.bin.streams")).unwrap();
    assert_eq!(raw.len(), 102_992);
    assert_eq!(
        raw.iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100_0000_01b3)),
        0x9ad3_f0ce_9752_baf8
    );
    let mut at = 0;
    let mut drawn = 0;
    for (index, case) in corpus["cases"].as_array().unwrap().iter().enumerate() {
        let h: [u32; 12] = std::array::from_fn(|i| word(&raw, at + i * 4));
        at += 48;
        assert_eq!(h[0], index as u32);
        let commands: Vec<u32> = (0..h[6] as usize).map(|i| word(&raw, at + i * 4)).collect();
        at += h[6] as usize * 4;
        let upload = &raw[at..at + h[7] as usize];
        at += h[7] as usize;
        let (tile, drawable, transform, clip, requested) = input(case);
        let result = tile.material(&drawable, requested);
        assert_eq!(result.is_some(), case["draw"] == 1, "gate {}", case["name"]);
        let Some((tinted, shader, blend)) = result else {
            continue;
        };
        drawn += 1;
        // Read the real blend state before Tile's DrawArrays. Active clip
        // cleanup can bind a different state later in the same command stream.
        let tile_draw = commands
            .iter()
            .enumerate()
            .filter_map(|(i, word)| (*word == 0xa003_0e32).then_some(i))
            .nth(if case["mask"] == 3 { 2 } else { 0 })
            .unwrap();
        let prefix = &commands[..tile_draw];
        let enabled = *prefix
            .iter()
            .rev()
            .find(|&&word| matches!(word, 0x8000_04bb | 0x8001_04bb))
            .unwrap();
        let native_blend = if enabled == 0x8000_04bb {
            ColorBlendType::Opaque
        } else {
            let offset = prefix
                .iter()
                .rposition(|&word| word == 0x2006_0781)
                .unwrap();
            match &prefix[offset + 1..offset + 7] {
                [1, 5, 6, 1, 8, 2] => ColorBlendType::Layer1,
                [1, 5, 2, 1, 8, 2] => ColorBlendType::Layer2,
                [3, 5, 2, 1, 8, 2] => ColorBlendType::Layer3,
                unknown => panic!("unexpected native blend {unknown:?}"),
            }
        };
        assert_eq!(blend, native_blend, "blend {}", case["name"]);
        assert_eq!(
            shader as u32,
            case["selected_shader"].as_u64().unwrap() as u32
        );
        let matrix = transform.compute_final_transform();
        assert_eq!(
            matrix.w_axis.x.to_bits(),
            word(upload, h[9] as usize + 48),
            "G12 {}",
            case["name"]
        );
        // GL/WGPU differ only in the depth row. Preserve every other word,
        // including copied NaN payloads and signs, rather than just G12.
        for (lane, value) in matrix.to_cols_array().into_iter().enumerate() {
            if lane % 4 != 2 {
                assert_eq!(
                    value.to_bits(),
                    word(upload, h[9] as usize + lane * 4),
                    "matrix{lane} {}",
                    case["name"]
                );
            }
        }
        if case["mask"] == 3 {
            for (i, vertex) in clip.mask_vertices().iter().enumerate() {
                let actual = [
                    vertex.position.x.to_bits(),
                    vertex.position.y.to_bits(),
                    vertex.position.z.to_bits(),
                    vertex.color.0,
                ];
                let expected: [u32; 4] =
                    std::array::from_fn(|lane| word(upload, 64 + i * 16 + lane * 4));
                assert_eq!(actual, expected, "mask{i} {}", case["name"]);
            }
        }
        let vertices = tile.vertices(tinted, shader, drawable.shader_param, matrix);
        for (i, vertex) in vertices.iter().enumerate() {
            let offset = h[10] as usize + i * 16;
            let actual = [
                vertex.position.x.to_bits(),
                vertex.position.y.to_bits(),
                vertex.position.z.to_bits(),
                // Original vertices use BGRA; WGPU PosCol uses RGBA.
                (vertex.color.0 & 0xff00_ff00)
                    | ((vertex.color.0 & 255) << 16)
                    | ((vertex.color.0 >> 16) & 255),
            ];
            let expected: [u32; 4] = std::array::from_fn(|lane| word(upload, offset + lane * 4));
            assert_eq!(actual, expected, "vertex{i} {}", case["name"]);
        }
    }
    assert_eq!(drawn, 92);
    assert_eq!(at, raw.len());
}

#[test]
#[ignore = "Focused offscreen 720p/1080p Tile Gamma and clip-order checks; no audio, window or saves"]
fn umineko_tile_gamma_and_clip_order_pixels() {
    use crate::{
        adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
        layer::{DrawableLayer, Layer, user::higurashi::leaf_clip::gpu_tests::initialize},
    };
    use shin_core::vm::command::types::LayerProperty::*;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let set = |tile: &mut TileLayer, p, value| {
        tile.properties_mut()
            .property_tweener_mut(p)
            .fast_forward_to(value);
    };
    let make = |alpha: f32, shader: f32, clip: f32| {
        let mut tile = TileLayer::new(
            FloatColor4 {
                a: alpha,
                ..FloatColor4::WHITE
            },
            Vec4::new(-960., -540., 1920., 1080.),
        );
        for (p, v) in [
            (ClipMode, clip),
            (ClipFromX, -100.),
            (ClipFromY, -50.),
            (ClipToX, 100.),
            (ClipToY, 50.),
            (FragmentShader, shader),
            (ShaderParamX, 500.),
            (ShaderParamY, 250.),
            (ShaderParamZ, 750.),
        ] {
            set(&mut tile, p, v);
        }
        tile
    };
    let (device, queue, _) = reference_gpu_assets();
    let mut frames = 0;
    for extent in [(1280, 720), (1920, 1080)] {
        let pixel = |frame: &[u8], x: usize, y: usize| -> [u8; 4] {
            frame[(y * extent.0 as usize + x) * 4..][..4]
                .try_into()
                .unwrap()
        };
        let center = (extent.0 as usize / 2, extent.1 as usize / 2);
        let outside = (extent.0 as usize / 4, extent.1 as usize / 4);
        for scenario in 0..10 {
            let frame = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                let seeded = if scenario >= 3 && scenario != 6 {
                    0x40
                } else {
                    0
                };
                initialize(ctx, target, seeded);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    ctx.depth_stencil,
                    "Umineko Tile Gamma / pass and clip ordering",
                );
                let mut tile = make(
                    if scenario == 4 { 0. } else { 1. },
                    if scenario <= 2 { 5. } else { 0. },
                    if scenario <= 2 { 0. } else { 1. },
                );
                if scenario == 1 {
                    // Gamma xyz=1 is a true Default simplifier, even W=NaN.
                    for p in [ShaderParamX, ShaderParamY, ShaderParamZ] {
                        set(&mut tile, p, 1000.);
                    }
                    set(&mut tile, ShaderParamW, f32::NAN);
                }
                if scenario == 2 {
                    set(&mut tile, FragmentShader, 0.);
                }
                if scenario == 5 {
                    // Transparent pre-Gamma alpha keeps the transparent blend.
                    tile = make(0.5, 5., 1.);
                }
                if scenario == 7 {
                    set(&mut tile, ClipToX, -100.);
                }
                if scenario == 8 {
                    set(&mut tile, ClipFromY, f32::NAN);
                }
                if scenario == 9 {
                    set(&mut tile, MulColorAlpha, 500.);
                }
                let requested = if matches!(scenario, 3 | 5 | 9) {
                    PassKind::Transparent
                } else {
                    PassKind::Opaque
                };
                tile.render(&mut pass, &TransformParams::default(), 1, requested);
                if scenario >= 3 {
                    let follower = make(1., 0., 0.);
                    follower.render(&mut pass, &TransformParams::default(), 2, PassKind::Opaque);
                }
            });
            let c = pixel(&frame, center.0, center.1);
            let o = pixel(&frame, outside.0, outside.1);
            match scenario {
                0 => {
                    assert_eq!(c, [255, 0, 0, 0], "opaque Gamma {extent:?}");
                    assert_eq!(o, c);
                }
                1 | 2 => {
                    assert_eq!(c, [255; 4], "Default / simplified Gamma {extent:?}");
                    assert_eq!(o, c);
                }
                3 | 4 => {
                    assert_eq!(
                        c,
                        [46, 82, 135, 105],
                        "rejected pass/alpha touched clip {extent:?}/{scenario}"
                    );
                    assert_eq!(o, c);
                }
                _ => {
                    assert_eq!(c, [255; 4], "accepted clip cleanup {extent:?}/{scenario}");
                    assert_eq!(o, c);
                }
            }
            frames += 1;
        }
    }
    assert_eq!(frames, 20);
}
