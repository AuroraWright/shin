//! Live command/bank owners compared to original stepped effect scenes.
use super::*;
use crate::adv::higurashi::{
    layers::HigurashiLayers, scene::HigurashiScene, scene_textures::HigurashiSceneTextures,
};
use shin_core::{time::Ticks, vm::higurashi::command::HigurashiRuntimeCommand as C};

#[test]
#[ignore = "actual live layer commands, moving native effects and original GLSL full-scene replay; offscreen Metal,no audio/window/save"]
fn higurashi_live_effect_scenes_match_original_pixels_and_motion() {
    let (cases, originals) = read_corpus();
    assert_eq!(cases.len(), 80);
    let (device, queue, server) = reference_gpu_assets();
    let authored = scenario(&[0, 0, 0]);
    let mut frames = Vec::new();
    let mut packets = 0;
    let mut summaries = Vec::new();
    let capture = std::env::var("SHIN_HIGURASHI_LIVE_EFFECT_PIXELS")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(dir) = &capture {
        std::fs::create_dir_all(dir).unwrap();
    }
    for (index, (case, original)) in cases.iter().zip(&originals).enumerate() {
        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        for (leaf_index, leaf) in case.leaves.iter().enumerate() {
            let p = leaf.params;
            shin_tasks::block_on(layers.dispatch(
                &C::LAYERLOAD {
                    args: [leaf.id, leaf.kind, 0],
                    params: (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]),
                },
                &server,
                &authored,
                || [1, 2, 3, 4],
            ))
            .unwrap();
            for (property, &value) in leaf.values.iter().enumerate() {
                shin_tasks::block_on(layers.dispatch(
                    &C::LAYERCTRL {
                        args: [leaf.id, property as i32],
                        params: (value, 0, 0, 0, 0, 0, 0, 0),
                    },
                    &server,
                    &authored,
                    || panic!("CTRL consumes no constructor entropy"),
                ))
                .unwrap();
            }
            assert_eq!(layers.group_children().len(), leaf_index + 1);
        }
        for (group, id) in [(-1, 0), (-2, 1)] {
            for (property, &value) in case.groups[id].iter().enumerate() {
                shin_tasks::block_on(layers.dispatch(
                    &C::LAYERCTRL {
                        args: [group, property as i32],
                        params: (value, 0, 0, 0, 0, 0, 0, 0),
                    },
                    &server,
                    &authored,
                    || panic!("CTRL consumes no constructor entropy"),
                ))
                .unwrap();
            }
        }
        let steps = case.leaves[0].params[7];
        let delta = f32::from_bits(case.leaves[0].params[6] as u32);
        for leaf in &case.leaves {
            assert_eq!(
                (leaf.params[6], leaf.params[7]),
                (case.leaves[0].params[6], steps)
            );
        }
        for _ in 0..steps {
            layers.advance(Ticks::from_f32(delta), None);
        }
        let scene = HigurashiScene::new(layers);
        let actual = reference_render_to_texture(&device, &queue, case.extent, |ctx, target| {
            let mut textures = HigurashiSceneTextures::new(ctx);
            textures.prepare_scene(ctx, &scene).unwrap();
            let mut pass = ctx.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                Some(ctx.depth_stencil),
                "Live effects output",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
            textures.render_scene(&mut pass, &scene);
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
        let colors = actual
            .chunks_exact(4)
            .map(|pixel| <[u8; 4]>::try_from(pixel).unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let visible = actual
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|&channel| channel != 0))
            .count();
        if let Some(dir) = &capture {
            std::fs::write(dir.join(format!("actual-{index:03}.rgba")), &actual).unwrap();
            std::fs::write(dir.join(format!("original-{index:03}.rgba")), &expected).unwrap();
        }
        assert_eq!(
            different, 0,
            "live scene{index} extent{:?} max{maximum}",
            case.extent
        );
        summaries.push(serde_json::json!({"index":index,"extent":[case.extent.0,case.extent.1],"steps":steps,"distinct_rgba":colors,"visible_pixels":visible,"different_channels":different,"maximum_difference":maximum}));
        frames.push(actual);
        packets += original.draws.len();
    }
    let mut changed_pairs = 0;
    let mut changed_channels = 0;
    for pair in frames.chunks_exact(2) {
        let changed = pair[0].iter().zip(&pair[1]).filter(|(a, b)| a != b).count();
        changed_channels += changed;
        changed_pairs += usize::from(changed > 0);
    }
    assert!(
        changed_pairs >= 8,
        "moving Raster phases must visibly change patterned scenes"
    );
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    if let Some(dir) = &capture {
        std::fs::write(dir.join("summary.json"), serde_json::to_vec_pretty(&serde_json::json!({"profiles":cases.len(),"native_packets":packets,"changed_pairs":changed_pairs,"changed_channels":changed_channels,"frames":summaries})).unwrap()).unwrap();
    }
    println!(
        "Live effects:80 exact full scene frames,{packets} original packets,{changed_pairs} moving pairs/{changed_channels} changed channels;actual runtime commands/fixed captures/two canvases/no audio/window/save"
    );
}
