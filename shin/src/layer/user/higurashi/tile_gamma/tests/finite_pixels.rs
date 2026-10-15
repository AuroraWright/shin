//! Actual owner versus the newly captured literal original finite-sentinel
//! packets. This is desktop packet replay, not an independent Switch framebuffer.
use super::{finite_sentinel, owner};
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::leaf_clip::gpu_tests::{gpu, initialize},
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

#[test]
#[ignore = "384 finite full-cover owner/original frames at720p/1080p; leaf-clip SPIR-V; pure GPU, no audio/windows/saves"]
fn higurashi_tile_gamma_finite_fullcover_owner_frames_match_original_packets() {
    let cases = finite_sentinel::cases();
    super::check_packets_expected(&cases, (192, 64, 64, 7872));
    let (device, queue, _) = reference_gpu_assets();
    let mut renderer = gpu::Renderer::new(&device);
    let mut frames = 0;
    for case in &cases {
        let Some(original) = &case.original else {
            continue;
        };
        let layer = owner::owner(case);
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 0);
                let mut pass = ctx.begin_pass_preserving_depth_stencil(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::Screen,
                    },
                    ctx.depth_stencil,
                    "Finite full-cover Tile actual owner",
                );
                layer
                    .render(&mut pass, &case.parent, 7, case.requested_pass)
                    .unwrap();
            });
            let expected = reference_render_to_texture(&device, &queue, extent, |ctx, target| {
                initialize(ctx, target, 0);
                renderer.draw(ctx, target, &owner::native_draw(original), None, None);
            });
            owner::exact(
                &actual,
                &expected,
                &format!("finite/profile{}/{extent:?}", case.index),
            );
            frames += 1;
        }
    }
    assert_eq!(frames, 384);
    println!(
        "TileGamma finite full-cover:384 byte-exact actual owner/original720p1080p frames;all192 native draw profiles;no SDK/audio/window/save acceptance"
    );
}
