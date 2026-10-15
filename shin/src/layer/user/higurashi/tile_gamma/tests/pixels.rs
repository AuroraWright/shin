//! Independent literal original program1 versus the proposed packed-color
//! transport. Native SDK execution/pixels and authored reachability are not
//! claimed. Only raw frozen original packets feed the reference renderer.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

fn original_draw(original: &Original) -> gpu::Draw {
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

fn exact(actual: &[u8], expected: &[u8], context: &str) {
    assert_eq!(actual.len(), expected.len());
    let changed = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let maximum = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(
        changed, 0,
        "{context}:different channels{changed}/max{maximum}"
    );
}

#[test]
#[ignore = "52 full720p/1080p literal-original program1 comparisons; requires SHIN_HIGURASHI_LEAF_CLIP_SHADERS; pure GPU, no audio/windows/saves"]
fn higurashi_tile_gamma_fullcanvas_pixels_match_original_packets() {
    let cases = cases();
    check_packets(&cases);
    // Parameter family1 is unsimplified Gamma. Rectangle0 is a full logical
    // canvas; rectangle4 crosses the lower edge, giving opaque Gamma red with
    // emitted alpha0. Rectangle3 is a tiny authored-coordinate quad whose
    // pre-Gamma alpha0 is intentionally retained. No geometry is repositioned.
    let mut pairs = Vec::new();
    for rectangle in [0, 3, 4] {
        for blend in 0..4 {
            let base = 128 + rectangle * 16 + blend * 4;
            let selected: Vec<_> = cases[base..base + 4]
                .iter()
                .filter(|case| case.original.is_some())
                .collect();
            assert_eq!(selected.len(), 2);
            assert!(!is_gamma(selected[0]) && is_gamma(selected[1]));
            assert_eq!(selected[0].requested_pass, selected[1].requested_pass);
            pairs.push((rectangle, blend, selected));
        }
    }
    assert_eq!(pairs.len(), 12);
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let (mut frames, mut changed_pairs) = (0, 0);
    for (rectangle, blend, pair) in pairs {
        for extent in [(1280, 720), (1920, 1080)] {
            let mut baseline = None;
            for case in &pair {
                let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                    // Existing independent fixture background is nonblack and
                    // has alpha0.41. Its DS clears are fixture initialization,
                    // not a substitution for any native clip clear.
                    initialize(ctx, target, 0);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view: target,
                            kind: TextureTargetKind::Screen,
                        },
                        ctx.depth_stencil,
                        "Proposed fullcanvas Tile Gamma",
                    );
                    let matrix = case
                        .properties
                        .composed_transform(&case.parent)
                        .compute_tile_transform();
                    if is_gamma(case) {
                        gamma(case).render(&mut pass, matrix, 7, case.requested_pass);
                    } else {
                        case.tile.render(
                            &mut pass,
                            &case.properties.drawable_params(),
                            matrix,
                            7,
                            case.requested_pass,
                        );
                    }
                });
                let expected =
                    reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                        initialize(ctx, target, 0);
                        renderer.draw(
                            ctx,
                            target,
                            &original_draw(case.original.as_ref().unwrap()),
                            None,
                            None,
                        );
                    });
                exact(
                    &actual,
                    &expected,
                    &format!("profile{}/{extent:?}", case.index),
                );
                if is_gamma(case) {
                    let baseline: Vec<u8> = baseline.take().unwrap();
                    let differences = actual.iter().zip(&baseline).filter(|(a, b)| a != b).count();
                    if rectangle != 3 {
                        assert!(
                            differences > 0,
                            "rect{rectangle}/blend{blend}/{extent:?} lost its Default/Gamma control"
                        );
                        changed_pairs += 1;
                    }
                } else {
                    baseline = Some(actual);
                }
                frames += 1;
            }
        }
    }
    // A true pre-Gamma opaque pair with alpha0 Gamma must still obey the
    // native strict-Greater stencil test. Equality rejects BOTH materials.
    for case in [&cases[130], &cases[131]] {
        assert!(case.original.is_some());
        assert_eq!(case.requested_pass, PassKind::Opaque);
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 7);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    ctx.depth_stencil,
                    "Tile Gamma strict-stencil rejection",
                );
                let matrix = case
                    .properties
                    .composed_transform(&case.parent)
                    .compute_tile_transform();
                if is_gamma(case) {
                    gamma(case).render(&mut pass, matrix, 7, case.requested_pass);
                } else {
                    case.tile.render(
                        &mut pass,
                        &case.properties.drawable_params(),
                        matrix,
                        7,
                        case.requested_pass,
                    );
                }
            });
            let expected = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 7);
                renderer.draw(
                    ctx,
                    target,
                    &original_draw(case.original.as_ref().unwrap()),
                    None,
                    None,
                );
            });
            exact(
                &actual,
                &expected,
                &format!("strict-equal/profile{}/{extent:?}", case.index),
            );
            assert!(actual.chunks_exact(4).all(|rgba| rgba == &actual[..4]));
            assert!(actual[0] > 0 && actual[1] > 0 && actual[2] > 0);
            assert!(actual[3] > 0 && actual[3] < 255);
            frames += 1;
        }
    }
    assert_eq!((frames, changed_pairs), (52, 16));
    println!(
        "TileGamma:52 byte-exact full720p/1080p reference frames;16 visible Default/Gamma differential controls;native preGamma routing+strictstencil;no SDK/audio/windows/saves"
    );
}
