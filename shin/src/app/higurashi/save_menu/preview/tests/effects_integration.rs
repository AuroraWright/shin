use super::super::effects::tests::native::{Case, Event, read_corpus};
use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use glam::{Vec2, Vec4};
use shin_render::{
    LayerFragmentShader,
    shaders::types::{
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};
use std::sync::atomic::{AtomicU32, Ordering};

mod gpu;
mod live;

fn canvas(case: &Case) -> (u32, u32) {
    match case.extent {
        (228, 128) => (1280, 720),
        (342, 192) => (1920, 1080),
        other => panic!("Original thumbnail extent {other:?}"),
    }
}
fn hydrated(case: &Case) -> Hydrated {
    let mut records = [[0; 79]; SLOTS];
    for record in &mut records {
        record[0] = -1;
    }
    for (record, leaf) in records.iter_mut().zip(&case.leaves) {
        record[0] = leaf.kind;
        record[1] = leaf.id;
        record[6..14].copy_from_slice(&leaf.params);
        record[14..].copy_from_slice(&leaf.values);
    }
    Hydrated {
        records,
        root: case.groups[0],
        screen: case.groups[1],
        labels: [Arc::from("A"), Arc::from("B")],
        scenario_id: 0,
    }
}
/// Copy RGBA opaquely so alpha differences remain visible in the readback.
fn publish(
    pass: &mut shin_render::render_pass::RenderPass,
    source: shin_render::shaders::types::texture::TextureSource,
) {
    super::super::postprocessing::Packet {
        program: 6,
        operation: LayerFragmentShader::Default,
        projection: glam::Mat4::from_cols_array(&[
            2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
        ]),
        color: Vec4::ONE,
        param: Vec4::ZERO,
        vertices: [Vec2::ZERO, Vec2::new(2., 0.), Vec2::new(0., 2.)].map(|v| PosTexVertex {
            position: v,
            texture_position: v,
        }),
    }
    .render(pass, source);
}

#[test]
#[ignore = "actual saved Scene with native startup/factory/lower-prefix/effect packets and original GLSL replay; fixed original effect resources and both thumbnail sizes; offscreen Metal,no audio/windows/user saves"]
fn higurashi_saved_effect_integrated_captures_match_original_pixels_and_entropy() {
    let (_, prefixes, packets) = integrated_captures(168);
    assert_eq!((prefixes, packets), (194, 544));
}

fn integrated_captures(expected_profiles: usize) -> (Vec<Vec<u8>>, usize, usize) {
    let (cases, originals) = read_corpus();
    assert_eq!(cases.len(), expected_profiles);
    let (device, queue, server) = reference_gpu_assets();
    let authored = scenario(&[0, 0, 0]);
    let mut images = 0;
    let mut native_draws = 0;
    let mut prefixes = 0;
    let mut frames = Vec::new();
    let mut summaries = Vec::new();
    let capture_directory = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_EFFECTS_PIXEL_CAPTURE")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(directory) = &capture_directory {
        std::fs::create_dir_all(directory).unwrap();
    }
    for (index, (case, original)) in cases.iter().zip(&originals).enumerate() {
        let saved = hydrated(case);
        let original_records = saved.records;
        let samples = Arc::new(AtomicU32::new(0xabc00011));
        let seeds: SeedSource = {
            let samples = samples.clone();
            Arc::new(move || samples.fetch_add(1, Ordering::SeqCst))
        };
        let scene = crate::engine_random::with_reference_seed(5489, || {
            let before = crate::engine_random::seed_call_count();
            let scene = shin_tasks::block_on(Scene::load(
                &server,
                &authored,
                &saved,
                [
                    [0xabc00001, 0xabc00002, 0xabc00003, 0xabc00004],
                    [1, 2, 0xabc0000b, 0xabc0000c],
                ],
                &seeds,
                0,
            ))
            .unwrap();
            assert_eq!(crate::engine_random::seed_call_count(), before);
            scene
        });
        assert_eq!(
            samples.load(Ordering::SeqCst),
            0xabc00011 + case.leaves.len() as u32 * 4
        );
        assert_eq!(scene.layers.len(), case.leaves.len());
        for ((slot, id, layer), (leaf, owner)) in scene
            .layers
            .iter()
            .zip(case.leaves.iter().zip(&original.owners))
        {
            assert_eq!(scene.layers[*slot as usize].0, *slot);
            assert_eq!(*id, leaf.id);
            let properties = group::Drawable::properties(layer);
            let expected: [u32; 65] = owner[8..].try_into().unwrap();
            assert_eq!(properties.values().map(f32::to_bits), expected);
            if let Layer::Effect {
                view,
                cache,
                shared_depth,
                ..
            } = layer
            {
                assert_eq!(view.kind(), leaf.kind);
                assert_eq!(view.phases(), [0.; 2]);
                assert!(cache.lock().is_none());
                assert!(shared_depth.lock().is_none());
            }
        }
        let ordered = group::ordered_drawables(
            scene
                .layers
                .iter()
                .map(|(slot, id, layer)| group::DrawableChild {
                    owner_id: *id,
                    slot: *slot,
                    layer,
                })
                .collect(),
        );
        let mut lower = Vec::new();
        let mut expected_prefixes = Vec::new();
        for child in ordered {
            if case.leaves[child.slot as usize].kind != 1 {
                expected_prefixes.push((child.owner_id, lower.clone()));
                lower.clear();
            }
            lower.push(child.owner_id);
        }
        let native_prefixes = original
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Lower(head, ids) => Some((
                    head[0] as i32,
                    ids.iter().map(|id| *id as i32).collect::<Vec<_>>(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            expected_prefixes, native_prefixes,
            "actual original lower ownership profile{index}"
        );
        prefixes += native_prefixes.len();
        let actual = reference_render_to_texture(&device, &queue, case.extent, |ctx, target| {
            let image = scene.capture(ctx, canvas(case)).unwrap();
            assert_eq!(image.dimensions(), case.extent);
            let mut resources = Vec::new();
            let mut shared_depth_owner = None;
            for (_, _, layer) in &scene.layers {
                if let Layer::Effect {
                    cache,
                    shared_depth,
                    ..
                } = layer
                {
                    resources.extend(cache.lock().as_ref().unwrap().actual_resources());
                    let address = Arc::as_ptr(shared_depth);
                    if let Some(previous) = shared_depth_owner {
                        assert_eq!(address, previous, "Scene-local shared depth identity");
                    } else {
                        shared_depth_owner = Some(address);
                        if let Some(depth) = shared_depth.lock().as_ref() {
                            let (width, height) = depth.actual_extent();
                            resources.push((1, width, height));
                        }
                    }
                }
            }
            // Native eagerly creates its shared depth even for Blur-only
            // scenes; production defers that unused allocation. Thumbnail
            // buffers/copies are separate from the fixed effect resources.
            let uses_shared_depth = case.leaves.iter().any(|leaf| matches!(leaf.kind, 6 | 10));
            let mut native_resources = original
                .resources
                .iter()
                .filter_map(|n| {
                    ((n[2], n[3]) != case.extent && (n[0] != 1 || uses_shared_depth))
                        .then_some((n[1], n[2], n[3]))
                })
                .collect::<Vec<_>>();
            resources.sort_unstable();
            native_resources.sort_unstable();
            assert_eq!(
                resources, native_resources,
                "actual GPU effect extents/counts profile{index}"
            );
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Actual saved effect RGBA readback",
            );
            publish(&mut pass, image.as_texture_source());
        });
        let expected = reference_render_to_texture(&device, &queue, case.extent, |ctx, target| {
            gpu::replay(ctx, target, case, original);
        });
        let different = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
        let maximum = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        if let Some(directory) = &capture_directory {
            std::fs::write(directory.join(format!("actual-{index:03}.rgba")), &actual).unwrap();
            std::fs::write(
                directory.join(format!("original-{index:03}.rgba")),
                &expected,
            )
            .unwrap();
        }
        assert!(
            different == 0,
            "original effect pixels profile{index},extent{:?}: {different} channels differ,max{maximum}",
            case.extent
        );
        let colors = actual
            .chunks_exact(4)
            .map(|pixel| <[u8; 4]>::try_from(pixel).unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let visible_pixels = actual
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|&channel| channel != 0))
            .count();
        summaries.push(serde_json::json!({
            "index": index, "extent": [case.extent.0, case.extent.1],
            "distinct_rgba": colors, "visible_pixels": visible_pixels,
            "different_channels": different, "maximum_difference": maximum,
        }));
        assert_eq!(saved.records, original_records);
        images += 1;
        native_draws += original.draws.len();
        frames.push(actual);
    }
    assert_eq!(images, expected_profiles);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    if let Some(directory) = &capture_directory {
        std::fs::write(
            directory.join("summary.json"),
            serde_json::to_vec_pretty(&summaries).unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "Saved effect integration:{images} exact RGBA thumbnail frames at720p/1080p,{prefixes} native lower prefixes,{native_draws} original shader packets;actual GPU fixed960x540/1920x1080 extents/counts/shareddepth;fourOS/zeroMT/unchangedrecords;startup-correct clip sentinel/original GLSL/nearest Mosaic/fullcopy/filter order;no audio/windows/user saves"
    );
    (frames, prefixes, native_draws)
}

#[test]
#[ignore = "visible nonuniform native Tile patterns and matched Mosaic/Raster controls, actual GPU resource extents and original GLSL replay; offscreen Metal,no audio/windows/user saves"]
fn higurashi_saved_effect_patterned_captures_transform_visible_pixels() {
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_EFFECTS_PATTERN_MANIFEST").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let profiles = manifest["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 40);
    let (frames, _, _) = integrated_captures(profiles.len());
    for profile in profiles {
        let index = profile["index"].as_u64().unwrap() as usize;
        if profile["require_visible_pattern"]
            .as_bool()
            .unwrap_or(false)
        {
            let pixels = &frames[index];
            let colors = pixels
                .chunks_exact(4)
                .map(|p| <[u8; 4]>::try_from(p).unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            assert!(
                colors.len() >= 3,
                "visible pattern profile{index}/{}",
                profile["name"]
            );
            assert!(
                pixels
                    .chunks_exact(4)
                    .any(|p| p[..3].iter().any(|&c| c != 0))
            );
        }
    }
    let comparisons = manifest["comparisons"].as_array().unwrap();
    assert_eq!(comparisons.len(), 14);
    let mut changed = 0;
    let mut aliased = 0;
    for comparison in comparisons {
        let baseline = comparison["baseline"].as_u64().unwrap() as usize;
        let effect = comparison["effect"].as_u64().unwrap() as usize;
        let different = frames[baseline]
            .iter()
            .zip(&frames[effect])
            .filter(|(a, b)| a != b)
            .count();
        // The independent original GLSL replay shows that size3/7 sample the
        // same four-color edges as size1 at 228x128. All larger grids and all
        // 342x192 Mosaic controls change pixels, as do every Raster control.
        let original_alias = matches!((baseline, effect), (2, 4) | (2, 6));
        assert_eq!(
            different == 0,
            original_alias,
            "native visible effect control {baseline}->{effect}: {different} differing channels, {}",
            comparison["reason"]
        );
        if original_alias {
            aliased += 1;
        } else {
            changed += 1;
        }
    }
    assert_eq!((changed, aliased), (12, 2));
    eprintln!(
        "Saved patterned effects:40 spatially nonuniform native source profiles;{changed} matched effect controls change pixels,{aliased} original thumbnail aliases; all original shader replay channels and actual GPU resource extents agree"
    );
}
