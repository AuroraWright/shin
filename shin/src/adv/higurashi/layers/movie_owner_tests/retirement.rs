//! Real codec/PCM ownership remains live in the negative-key vector until
//! exact timer/skip retirement, while VM records and keyed waits clear first.
use super::*;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

#[test]
#[ignore = "real short H264/AAC delayed owners and scoped retirement; offscreen Metal720p/1080p, the default audio output, no windows/saves"]
fn higurashi_live_movie_delayed_unload_keeps_render_update_and_retires_workers() {
    let (device, queue, _) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = scenario();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frames = 0;
    for audio in [false, true] {
        for skip in [false, true] {
            let fixtures = Fixtures::new(audio);
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
                .find(|c| {
                    let c = &c.0;
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
            let loaded =
                shin_tasks::block_on(plan(&layers, &case).load(&assets, &scenario)).unwrap();
            layers
                .publish_load_with_settings(loaded, || seeds(&mut counter), Some(&mut data))
                .unwrap();
            let target = layers.state.target(0).unwrap();
            tick_until(
                &mut layers,
                |layers| {
                    layers
                        .layer(target)
                        .unwrap()
                        .reference_movie()
                        .unwrap()
                        .reference_state()
                        .4
                        == Some((64, 48))
                },
                "Delayed Movie readiness",
            );
            let owner = layers.layer(target).unwrap().reference_movie().unwrap();
            let identity = owner.reference_state().7;
            let retirement = owner.reference_retirement().unwrap();
            let lifetime = owner.reference_lifetime();
            assert_eq!(lifetime.strong_count(), 1);
            let render = |layers: &HigurashiLayers, size| {
                crate::adv::higurashi::reference_render_to_texture(
                    &device,
                    &queue,
                    size,
                    |ctx, view| {
                        let mut pass = ctx.begin_pass(
                            TextureTarget {
                                view,
                                kind: TextureTargetKind::Screen,
                            },
                            Some(ctx.depth_stencil),
                            "Delayed Movie physical vector",
                        );
                        pass.clear(
                            Some(shin_core::primitives::color::UnormColor::BLACK),
                            Some(0),
                            Some(1.),
                        );
                        layers
                            .render_front_group(&mut pass, &Default::default())
                            .unwrap();
                    },
                )
            };
            let sizes = [(1280, 720), (1920, 1080)];
            let before = sizes.map(|size| render(&layers, size));
            let wait = super::super::movie_wait::MovieWait::capture(&layers, 0, 2).unwrap();
            let mut unload = super::super::LayerUnload::capture(&mut layers.state, 0, 2).unwrap();
            assert!(layers.state.target(0).is_none());
            assert!(
                layers.layer(target).is_some(),
                "constructor edits metadata before callback"
            );
            unload.complete(&mut layers).unwrap();
            assert!(layers.layer(target).is_none());
            assert!(
                wait.complete(&layers),
                "MOVIEWAIT keyed lookup excludes retiring owners"
            );
            assert_eq!(layers.group_children().len(), 1);
            assert_eq!(layers.reference_group_entries(0)[0].0, -1);
            assert_eq!(
                layers.group_children()[0]
                    .layer
                    .reference_movie()
                    .unwrap()
                    .reference_state()
                    .7,
                identity
            );
            for (size, before) in sizes.into_iter().zip(before) {
                let after = render(&layers, size);
                assert_eq!(after, before, "same live Movie renders after keyed removal");
                assert!(
                    after
                        .chunks_exact(4)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        >= 3
                );
                frames += 2;
            }
            // Advance the actual retained base clocks without holding another
            // strong LiveOwner, so complete expiration must join real workers.
            layers.retiring[0].entries_mut()[0]
                .owner
                .as_mut()
                .unwrap()
                .properties
                .control(0, 64., Ticks::from_f32(2.), 0)
                .unwrap();
            layers.advance(Ticks::from_f32(1.), None);
            let entries = layers.reference_group_entries(0);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].1, 1f32.to_bits());
            assert_eq!(entries[0].2.properties.values()[0], 32.);
            assert_eq!(
                entries[0].2.reference_movie().unwrap().reference_state().7,
                identity
            );
            assert_eq!(
                lifetime.strong_count(),
                1,
                "retiring drawable retains the same complete Movie owner"
            );
            if skip {
                layers.fast_forward_screen();
            } else {
                layers.advance(Ticks::from_f32(1.), None);
            }
            assert!(layers.group_children().is_empty());
            assert_eq!(
                lifetime.strong_count(),
                0,
                "complete Movie owner retires at timer/skip boundary"
            );
            assert!(
                retirement.is_finished(),
                "timer/skip retires complete codec/AAC scoped owner"
            );
            assert!(layers.visual_fault().is_none());
        }
    }
    assert_eq!(frames, 16);
    eprintln!(
        "Live delayed Movie:4 actual silent/AAC timer/skip owners;16 byte-identical720p/1080p before/retiring frames; constructor/keyed wait separation,same media identity,retained clock update,complete scoped codec/PCM retirement; the default audio output,no window/save"
    );
}
