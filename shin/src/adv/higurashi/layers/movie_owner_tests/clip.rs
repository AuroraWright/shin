//! Actual short-media retained RGBA through the live Movie clip call site.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::user::higurashi::{
        leaf_clip::{
            call_site_tests,
            gpu_tests::{self, gpu::Renderer},
        },
        movie::{FrameInfo, Packet},
    },
};
use shin_render::{
    PassKind,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};

#[test]
#[ignore = "real short Movie retained RGBA and original clip/material shaders at720p/1080p;offscreen Metal/exact Loopback Audio,no windows/saves"]
fn higurashi_live_movie_authored_clips_match_original_shader_transport() {
    let (device, queue, _) = reference_gpu_assets();
    let scenario = scenario();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let fixtures = Fixtures::new(false);
    let assets = fixtures.assets(&device, &queue);
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let now = clock.clone();
    let services = Services::new(
        assets.clone(),
        manager.clone(),
        [40, 60, 80],
        Some(Arc::new(move || now.load(Ordering::Acquire))),
    );
    let (case, _) = cases()
        .into_iter()
        .find(|(c, _)| {
            c.op == "L"
                && c.movie == 0
                && c.salt == 0
                && c.setting == 0
                && c.bank == Some(0)
                && c.flags == Some(0)
                && c.old == Some(0)
        })
        .unwrap();
    let mut counter = 0;
    let mut layers = setup(&case, &mut counter);
    layers.set_movie_services(services);
    let mut data = settings(0);
    let loaded = shin_tasks::block_on(plan(&layers, &case).load(&assets, &scenario)).unwrap();
    layers
        .publish_load_with_settings(loaded, || seeds(&mut counter), Some(&mut data))
        .unwrap();
    let target = layers.state.target(0).unwrap();
    let live = layers
        .layer(target)
        .unwrap()
        .reference_movie()
        .unwrap()
        .clone();
    let retirement = live.reference_retirement().unwrap();
    tick_until(
        &mut layers,
        |_| live.reference_state().4 == Some((64, 48)),
        "Clip retained Movie RGBA",
    );
    let endpoints = [
        [0., 0., 1280., 720.],
        [4., 2., 33., 28.],
        [33., 28., 4., 2.],
        [22., 0., 22., 720.],
        [1500., 0., 1800., 720.],
    ];
    let originals = gpu_tests::original_clips(&endpoints.map(|p| ('Q', p, false)));
    let mut renderer = Renderer::new(&device);
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for alpha in [1000., 500.] {
            let mut coverage = Vec::new();
            for (case, &endpoints) in endpoints.iter().enumerate() {
                let mut properties = call_site_tests::properties(endpoints);
                properties.tweener_mut(9).unwrap().fast_forward_to(alpha);
                let kind = if alpha == 1000. {
                    PassKind::Opaque
                } else {
                    PassKind::Transparent
                };
                layers.layer_mut(target).unwrap().properties = properties.clone();
                let packet = Packet::new(
                    FrameInfo {
                        size: glam::vec2(64., 48.),
                        origin: glam::Vec2::ZERO,
                        extent: glam::Vec2::ONE,
                    },
                    &properties,
                    &Default::default(),
                    kind,
                )
                .unwrap()
                .unwrap();
                let actual = reference_render_to_texture(&device, &queue, size, |ctx, view| {
                    gpu_tests::initialize(ctx, view, 0);
                    let mut pass = ctx.begin_pass_preserving_depth_stencil(
                        TextureTarget {
                            view,
                            kind: TextureTargetKind::RenderTexture,
                        },
                        ctx.depth_stencil,
                        "Actual Movie authored clip",
                    );
                    layers
                        .layer(target)
                        .unwrap()
                        .render(&mut pass, &Default::default(), 7, kind)
                        .unwrap();
                });
                let original = live.reference_with_rgba(|source, extent| {
                    assert_eq!(extent, (64, 48));
                    reference_render_to_texture(&device, &queue, size, |ctx, view| {
                        gpu_tests::initialize(ctx, view, 0);
                        for draw in &originals[case].draws {
                            renderer.draw(ctx, view, draw, None, None);
                        }
                        let draw = call_site_tests::ordinary(
                            packet.matrix,
                            packet.color,
                            packet.param,
                            call_site_tests::vertices(&packet.vertices),
                            &originals[case],
                        );
                        renderer.draw(ctx, view, &draw, Some(source), None);
                    })
                });
                call_site_tests::exact(
                    &actual,
                    &original,
                    &format!("Movie clip{case}/{alpha}/{size:?}"),
                );
                coverage.push(original);
                frames += 1;
            }
            call_site_tests::coverage(&coverage, &format!("Movie/{alpha}/{size:?}"));
        }
    }
    assert_eq!(frames, 20);
    drop(live);
    drop(layers);
    assert!(retirement.is_finished());
    eprintln!(
        "Live Movie clipping:20 exact720p/1080p authored full/reversed/partial/zero/exterior frames atopaque/transparent gates;real retained short-media RGBA/exact Loopback Audio;native masks/material GLSL,scoped retirement,no windows/saves"
    );
}
