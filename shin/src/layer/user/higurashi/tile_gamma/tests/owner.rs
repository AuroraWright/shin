//! Actual owner call-site acceptance, separate from the frozen helper tests.
//! Reference material draws use ONLY original packets/literal program1.
//! The gate canary is a controlled depth-sensitive observer, not an original
//! authored TileGamma packet or a claim about native SDK rasterization.
use super::{Case, Original, cases};
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::higurashi::HigurashiTransform,
        user::higurashi::{
            HigurashiUserLayer,
            leaf_clip::gpu_tests::{gpu, initialize},
            tile::Tile,
        },
    },
};
use glam::{Mat4, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    PassKind,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};

pub(super) fn native_draw(original: &Original) -> gpu::Draw {
    let p = &original.packet;
    gpu::Draw {
        program: p[32],
        blend: p[33],
        primitive: 1,
        depth: p[34..36].try_into().unwrap(),
        stencil: p[36..41].try_into().unwrap(),
        uniforms: vec![p[16..32].to_vec()],
        vertices: p[..16].to_vec(),
        count: 4,
        start: 0,
    }
}

pub(super) fn owner(case: &Case) -> HigurashiUserLayer {
    // Native original corpus supplies a prepared owner/property state. The
    // actual desktop constructor is used, then exactly that sampled state is
    // copied; no entropy/factory/command coverage is claimed by this fixture.
    let mut layer = HigurashiUserLayer::tile(case.tile.color, case.tile.rect, [0; 4]);
    layer.properties = case.properties.clone();
    layer
}

pub(super) fn exact(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let differences = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(
        differences, 0,
        "{label}:different channels{differences}/max{maximum}"
    );
}

#[test]
#[ignore = "24 actual Higurashi Tile owner/literal-original frames at720p/1080p; assets+leaf-clip SPIR-V; pure GPU/no audio/windows/saves"]
fn higurashi_tile_gamma_owner_frames_match_original_packets() {
    let cases = cases();
    // Exact native pairs: identity, fullcanvas opaque Gamma, additive Gamma,
    // reverse-subtract Gamma, opaque Gamma with nonzero red/alpha0, and RGB
    // identity with nonzero shader parameter alpha. Geometry is not moved.
    let pairs = [
        [2, 3],
        [130, 131],
        [132, 133],
        [136, 137],
        [194, 195],
        [386, 387],
    ];
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let (mut frames, mut changed, mut identities) = (0, 0, 0);
    for [default, gamma] in pairs {
        let default = &cases[default];
        let gamma = &cases[gamma];
        assert_eq!(default.raw[21], 0.0);
        assert_eq!(gamma.raw[21], 5.0);
        assert_eq!(default.original.as_ref().unwrap().input[8], 0);
        let native_gamma = gamma.original.as_ref().unwrap().input[8] == 5;
        assert_eq!(default.requested_pass, gamma.requested_pass);
        for extent in [(1280, 720), (1920, 1080)] {
            let mut first = None;
            for case in [default, gamma] {
                let layer = owner(case);
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    initialize(ctx, target, 0);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Actual Higurashi Tile owner Gamma/Default/identity",
                    );
                    layer
                        .render(&mut pass, &case.parent, 7, case.requested_pass)
                        .unwrap();
                });
                let expected =
                    reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                        initialize(ctx, target, 0);
                        renderer.draw(
                            ctx,
                            target,
                            &native_draw(case.original.as_ref().unwrap()),
                            None,
                            None,
                        );
                    });
                exact(
                    &actual,
                    &expected,
                    &format!("owner/profile{}/{extent:?}", case.index),
                );
                if case.raw[21] == 0.0 {
                    first = Some(actual);
                } else {
                    let first: Vec<u8> = first.take().unwrap();
                    if native_gamma {
                        assert_ne!(
                            actual, first,
                            "owner/profile{}/{extent:?} lost original Gamma difference",
                            case.index
                        );
                        changed += 1;
                    } else {
                        assert_eq!(
                            actual, first,
                            "owner/profile{}/{extent:?} lost original identity",
                            case.index
                        );
                        identities += 1;
                    }
                }
                frames += 1;
            }
        }
    }
    assert_eq!((frames, changed, identities), (24, 8, 4));
    println!(
        "TileGamma actual owner:24 exact full720p/1080p frames;8 visible Gamma controls/4 identities;original program1 raw packets;no nativeSDK/audio/windows/saves"
    );
}

/// Literal native74d34(0,1280,720,0,-1,1) projection from accepted clip proof.
/// The reference uses these words directly, not Clip.prepare or its matrix.
const CANARY_MATRIX: [u32; 16] = [
    0x3acc_cccd,
    0,
    0,
    0,
    0,
    0xbb36_0b61,
    0,
    0,
    0,
    0,
    0xbf80_0000,
    0,
    0xbf80_0000,
    0x3f80_0000,
    0x8000_0000,
    0x3f80_0000,
];

pub(super) fn canary_draw() -> gpu::Draw {
    // Half-canvas coverage leaves the other half's color/stencil observable.
    // DepthLess with z0.5 passes initial depth1. An accidental native clip
    // clear writes0.5 outside its mask, causing a visible coverage difference.
    let vertices = [
        [0.0f32, 0.0, 0.0],
        [640.0, 0.0, 0.0],
        [0.0, 720.0, 0.0],
        [640.0, 720.0, 0.0],
    ]
    .into_iter()
    .flat_map(|p| [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), 0xff3f_cc7f])
    .collect();
    gpu::Draw {
        program: 1,
        blend: 1,
        primitive: 1,
        depth: [1, 0],
        stencil: [4, 0, 0, 2, 0x1ffff],
        uniforms: vec![CANARY_MATRIX.to_vec()],
        vertices,
        count: 4,
        start: 0,
    }
}

#[test]
#[ignore = "12 actual owner no-op depth-canary frames at720p/1080p; assets+literal program1; pure GPU/no audio/windows/saves"]
fn higurashi_tile_gamma_owner_gates_leave_depth_untouched() {
    let cases = cases();
    let source = &cases[131]; // True pre-Gamma opaque, unsimplifiedGamma.
    assert_eq!(source.original.as_ref().unwrap().input[8], 5);
    assert_eq!(source.requested_pass, PassKind::Opaque);
    let active = [160.0, 90.0, 1120.0, 630.0];
    let partial_nan = [f32::from_bits(0x7fc1_2345), 0.0, 1280.0, 720.0];
    let specs = [
        (
            "invisible-active",
            active,
            Some((19, 0.0)),
            PassKind::Opaque,
        ),
        (
            "zero-scale-active",
            active,
            Some((12, 0.0)),
            PassKind::Opaque,
        ),
        ("wrong-pass-active", active, None, PassKind::Transparent),
        (
            "invisible-partialNaN",
            partial_nan,
            Some((19, 0.0)),
            PassKind::Opaque,
        ),
        (
            "wrong-pass-overflowed-active",
            [-f32::MAX, 90.0, f32::MAX, 630.0],
            None,
            PassKind::Transparent,
        ),
        (
            "wrong-pass-NaN-with-overflow",
            [f32::from_bits(0x7fc1_2345), -f32::MAX, 1280.0, f32::MAX],
            None,
            PassKind::Transparent,
        ),
    ];
    let probe = Tile {
        color: FloatColor4::from_vec4(vec4(0.25, 0.8, 0.5, 1.0)),
        rect: vec4(0.0, 0.0, 640.0, 720.0),
    };
    let probe_properties = HigurashiProperties::from_seeds([0; 4]);
    let probe_matrix = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
        &CANARY_MATRIX.map(f32::from_bits),
    ));
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let (mut frames, mut noops) = (0, 0);
    for (name, endpoints, hidden, requested) in specs {
        let mut layer = owner(source);
        for (index, value) in endpoints.into_iter().enumerate() {
            layer
                .properties
                .tweener_mut(44 + index)
                .unwrap()
                .fast_forward_to(value);
        }
        if let Some((index, value)) = hidden {
            layer
                .properties
                .tweener_mut(index)
                .unwrap()
                .fast_forward_to(value);
        }
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 0);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    ctx.depth_stencil,
                    "Actual owner Gamma guard followed by depth canary",
                );
                layer
                    .render(&mut pass, &source.parent, 7, requested)
                    .unwrap();
                probe.render_clipped(
                    &mut pass,
                    &probe_properties.drawable_params(),
                    probe_matrix,
                    1,
                    PassKind::Opaque,
                    true,
                );
            });
            let expected = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 0);
                renderer.draw(ctx, target, &canary_draw(), None, None);
            });
            exact(&actual, &expected, &format!("{name}/{extent:?}"));
            let pixel = |x: usize, y: usize| {
                let start = (y * extent.0 as usize + x) * 4;
                &actual[start..start + 4]
            };
            let left = pixel(extent.0 as usize / 4, extent.1 as usize / 2);
            let right = pixel(extent.0 as usize * 3 / 4, extent.1 as usize / 2);
            assert_eq!(left, [63, 204, 127, 255], "{name}:canary must be visible");
            assert_ne!(
                left, right,
                "{name}:uncovered background must remain visible"
            );
            assert!(right[0] > 0 && right[1] > 0 && right[2] > 0);
            assert!(right[3] > 0 && right[3] < 255);
            frames += 1;
            noops += 1;
        }
    }
    assert_eq!((frames, noops), (12, 12));
    println!(
        "TileGamma actual owner:12 exact full720p/1080p canary frames;12 visibility/pass noops including infinite extents;no leaked depth/color/stencil effects;no nativeSDK/audio/windows/saves"
    );
}
