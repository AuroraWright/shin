//! NEW retained production Screen/SceneTextures comparison to the frozen full
//! native178-action resource/frame corpus. Physical keyed publication, direct
//! b919c transfer and19614..196cc target setters are explicit fixture seams;
//! update, composition, history, capture, outer draw and ownership are production.
//! No normal WIPE command prefix, asynchronous restoration or global allocation
//! ID equality is inferred from those narrow transports.
use super::frame_pixels::{self, Event, Operation};
use crate::{
    adv::higurashi::{
        layers::HigurashiLayers, reference_gpu_assets, reference_render_to_texture,
        scene::HigurashiScene, scene_history::ScreenHistory,
        scene_textures::HigurashiSceneTextures,
    },
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms},
        user::higurashi::{HigurashiLayerKind, Prepared},
    },
    render::PreRenderContext,
};
use glam::{Vec2, Vec3, Vec4};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
};
use shin_render::{
    DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments, RenderRequestBuilder,
    render_texture::RenderTexture,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};
use std::collections::{BTreeMap, BTreeSet};

fn set_property(owner: &mut HigurashiProperties, index: u32, bits: u32) {
    owner
        .tweener_mut(index as usize)
        .unwrap()
        .fast_forward_to(f32::from_bits(bits));
}
fn publish_capture(
    context: &mut PreRenderContext,
    target: &wgpu::TextureView,
    source: &RenderTexture,
) {
    // Publication transports a copied texture at exactly its physical extent.
    // It does not prepare or sample production's current owner a second time.
    let projection = multiply_transforms(
        HigurashiTransform::default().compute_native_final_transform(),
        glam::Mat4::from_translation(Vec3::new(-640., -720., 0.)),
    );
    let vertices = [(0., 0.), (1280., 0.), (0., 720.), (1280., 720.)].map(|(x, y)| PosTexVertex {
        position: Vec2::new(x, y),
        texture_position: Vec2::new(x / 1280., y / 720.),
    });
    let mut pass = context.begin_pass(
        TextureTarget {
            kind: TextureTargetKind::Screen,
            view: target,
        },
        None,
        "Copied current texture observation",
    );
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::HigurashiPostprocess {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            texture: source.as_texture_source(),
            transform: HigurashiTransform::convert_clip_depth(projection),
            color: FloatColor4::WHITE,
            operation: LayerFragmentShader::Default,
            param: Vec4::ZERO,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
fn differences(a: &[u8], b: &[u8]) -> (usize, u8) {
    assert_eq!(a.len(), b.len());
    (
        a.iter().zip(b).filter(|(a, b)| a != b).count(),
        a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap(),
    )
}

#[test]
#[ignore = "Full original178-action Screen sequence + literal shaders; production retainedGPU owner, pure offscreen720p/1080p, no audio/window/user saves"]
fn higurashi_live_scene_history_full_sequences_match_original_pixels_and_capture_identity() {
    let corpus = frame_pixels::corpus();
    let (device, queue, server) = reference_gpu_assets();
    let scenario = shin_tasks::block_on(server.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut scene: Option<HigurashiScene> = None;
    let mut screens: Option<HigurashiSceneTextures> = None;
    let mut weak = None;
    let mut extent = [0, 0];
    let mut captures: [Option<RenderTexture>; 4] = std::array::from_fn(|_| None);
    let mut native_to_owned = BTreeMap::<u32, u64>::new();
    let mut native_resources = BTreeMap::<u32, u32>::new();
    let mut history_ids = BTreeSet::new();
    let mut depth_id = 0;
    let mut pair_allocated = 0u64;
    let mut pair_retired = 0u64;
    let mut aux = [0u32; 2];
    let mut actual_frames = Vec::new();
    let mut summaries = vec![];
    let mut observed = 0usize;
    let mut fresh_seed_calls = 0usize;
    let mut tile_seed_words = 0usize;
    for row in &corpus.rows {
        if let Operation::New(new_extent) = row.operation {
            assert!(scene.is_none() && screens.is_none());
            extent = new_extent;
            native_to_owned.clear();
            native_resources.clear();
            pair_allocated = 0;
            pair_retired = 0;
            aux = [0; 2];
            let sequence = corpus
                .manifest
                .sequences
                .iter()
                .find(|s| s.first_row == row.index)
                .unwrap();
            history_ids = corpus.rows[sequence.first_row..=sequence.last_row]
                .iter()
                .flat_map(|r| [r.screen[2], r.screen[3]])
                .filter(|&id| id != 0)
                .collect();
            depth_id = row
                .events
                .iter()
                .find_map(|e| match e {
                    Event::Resource(v) if v[1] == 1 => Some(v[0]),
                    _ => None,
                })
                .unwrap();
            let mut calls = 0;
            let next = HigurashiScene::new(HigurashiLayers::new(|| {
                calls += 1;
                [0; 4]
            }));
            assert_eq!(calls, 4);
            fresh_seed_calls += calls;
            weak = Some(next.history().weak());
            screens =
                Some(HigurashiSceneTextures::for_scene(&device, &queue, extent, &next).unwrap());
            scene = Some(next);
            assert!(screens.as_ref().unwrap().modal_capture_ready());
        } else {
            match row.operation {
                Operation::Tile {
                    bank,
                    slot,
                    id,
                    params,
                } => {
                    let p = (
                        params[0], params[1], params[2], params[3], params[4], params[5],
                        params[6], params[7],
                    );
                    let prepared = shin_tasks::block_on(Prepared::load(
                        &server,
                        &scenario,
                        HigurashiLayerKind::Tile,
                        p,
                        0,
                    ))
                    .unwrap();
                    let mut owner = prepared.construct(None, Some([0; 4])).unwrap().unwrap();
                    tile_seed_words += 4;
                    owner.set_owner_id(id);
                    set_property(&mut owner.properties, 0, (-640f32).to_bits());
                    set_property(&mut owner.properties, 1, 720f32.to_bits());
                    scene
                        .as_mut()
                        .unwrap()
                        .layers_mut()
                        .unwrap()
                        .reference_publish_physical(bank as u8, slot as u8, owner)
                        .unwrap();
                }
                Operation::Property { which, index, bits } => set_property(
                    scene
                        .as_mut()
                        .unwrap()
                        .layers_mut()
                        .unwrap()
                        .reference_physical_properties_mut(which)
                        .unwrap(),
                    index,
                    bits,
                ),
                Operation::Update {
                    enabled,
                    paused,
                    delta,
                } => scene.as_mut().unwrap().advance(
                    Ticks::from_f32(f32::from_bits(delta)),
                    enabled,
                    paused,
                    None,
                ),
                Operation::Prepare => {
                    let before = scene.as_ref().unwrap().history().reference_snapshot();
                    reference_render_to_texture(
                        &device,
                        &queue,
                        (extent[0], extent[1]),
                        |ctx, _| {
                            screens
                                .as_mut()
                                .unwrap()
                                .prepare_scene(ctx, scene.as_ref().unwrap())
                                .unwrap()
                        },
                    );
                    assert_eq!(
                        before,
                        scene.as_ref().unwrap().history().reference_snapshot(),
                        "R must not allocate/swap/release history pairs"
                    );
                }
                Operation::Wipe(duration) => scene
                    .as_mut()
                    .unwrap()
                    .reference_native_wipe(duration)
                    .unwrap(),
                Operation::Restore(targets) => {
                    let before = scene.as_ref().unwrap().history().reference_snapshot();
                    for (which, values) in targets.into_iter().enumerate() {
                        let p = scene
                            .as_mut()
                            .unwrap()
                            .layers_mut()
                            .unwrap()
                            .reference_physical_properties_mut(which as u32)
                            .unwrap();
                        for (index, value) in values.into_iter().enumerate() {
                            set_property(p, index as u32, (value as f32).to_bits());
                        }
                    }
                    assert_eq!(
                        before,
                        scene.as_ref().unwrap().history().reference_snapshot(),
                        "19614..196cc target setters retain current/previous"
                    );
                }
                Operation::Draw {
                    extent: output,
                    pass,
                    stencil,
                } => {
                    let before = scene.as_ref().unwrap().history().reference_snapshot();
                    let pixels = reference_render_to_texture(
                        &device,
                        &queue,
                        (output[0], output[1]),
                        |ctx, target| {
                            let mut draw = ctx.begin_pass(
                                TextureTarget {
                                    kind: TextureTargetKind::Screen,
                                    view: target,
                                },
                                Some(ctx.depth_stencil),
                                "Native outer Screen observation",
                            );
                            draw.clear(
                                Some(UnormColor::from_rgba(0x57, 0x35, 0x21, 255)),
                                Some(0),
                                Some(0.),
                            );
                            if pass == 1 {
                                screens.as_ref().unwrap().render_screen(
                                    &mut draw,
                                    scene.as_ref().unwrap(),
                                    stencil as u8,
                                )
                            }
                        },
                    );
                    assert_eq!(
                        before,
                        scene.as_ref().unwrap().history().reference_snapshot(),
                        "output resize/outerdraw must retain physical pair"
                    );
                    actual_frames.push(pixels);
                }
                Operation::Capture(index) => {
                    let before = scene.as_ref().unwrap().history().reference_snapshot();
                    let pixels = reference_render_to_texture(
                        &device,
                        &queue,
                        (extent[0], extent[1]),
                        |ctx, target| {
                            let copied = screens
                                .as_ref()
                                .unwrap()
                                .reference_capture_current(ctx)
                                .unwrap();
                            publish_capture(ctx, target, &copied);
                            captures[index as usize] = Some(copied);
                        },
                    );
                    assert_eq!(
                        before,
                        scene.as_ref().unwrap().history().reference_snapshot(),
                        "b9508 capture must retain selected current without prepare"
                    );
                    actual_frames.push(pixels);
                }
                Operation::Dispose => {
                    for index in (0..4).rev() {
                        drop(captures[index].take());
                    }
                    drop(scene.take());
                    drop(screens.take());
                    assert!(
                        ScreenHistory::from_weak(weak.as_ref().unwrap()).is_none(),
                        "SceneTextures Weak must not retain a disposed Screen"
                    );
                }
                Operation::New(_) => unreachable!(),
            }
        }
        for event in &row.events {
            match event {
                Event::Resource(v) => {
                    assert!(native_resources.insert(v[0], 1).is_none());
                    if history_ids.contains(&v[0]) || v[0] == depth_id {
                        pair_allocated += 1
                    }
                }
                Event::Keep(v) => *native_resources.get_mut(&v[0]).unwrap() = v[1],
                Event::Drop(v) => {
                    *native_resources.get_mut(&v[0]).unwrap() = v[1];
                    if v[1] == 0 && (history_ids.contains(&v[0]) || v[0] == depth_id) {
                        pair_retired += 1
                    }
                }
                Event::Draw(d) if d[3] == 21 => aux = [d[13], d[14]],
                _ => {}
            }
        }
        if let Some(scene) = &scene {
            let (actual_extent, current, previous, allocated, retired, pending) =
                scene.history().reference_snapshot();
            assert_eq!(actual_extent, extent);
            assert_eq!(pending, 0);
            assert_eq!(
                (allocated, retired),
                (pair_allocated, pair_retired),
                "row{} pair/depth allocation/retirement",
                row.index
            );
            for (native, owned) in [(row.screen[2], Some(current)), (row.screen[3], previous)] {
                if let Some(owned) = owned {
                    assert_ne!(native, 0);
                    if let Some(previous) = native_to_owned.insert(native, owned) {
                        assert_eq!(
                            previous, owned,
                            "native image identity changed row{}",
                            row.index
                        )
                    }
                } else {
                    assert_eq!(native, 0)
                }
            }
            assert_eq!(
                native_to_owned
                    .values()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .len(),
                native_to_owned.len(),
                "distinct native pair images cannot alias one owned image"
            );
            assert_eq!(
                scene.layers().screen_properties().values()[39].to_bits(),
                row.screen[9]
            );
            assert_eq!(scene.transition().is_some(), row.screen[4] != 0);
            if let Some(transition) = scene.transition() {
                assert_eq!(
                    transition.progress().to_bits(),
                    (f32::from_bits(row.screen[5]) / f32::from_bits(row.screen[6])).to_bits()
                )
            }
            for bank in 0..2 {
                let owners = scene
                    .layers()
                    .reference_group_entries(bank)
                    .into_iter()
                    .map(|(key, _, owner)| (key, owner.owner_id() as u32))
                    .collect::<Vec<_>>();
                assert_eq!(owners, row.groups[bank], "row{} bank{bank}", row.index)
            }
            let outgoing = scene.transition().map_or_else(Vec::new, |t| {
                t.previous
                    .group_children()
                    .into_iter()
                    .map(|c| (i32::from(c.slot), c.owner_id as u32))
                    .collect()
            });
            assert_eq!(outgoing, row.groups[2], "outgoing row{}", row.index);
            scene
                .history()
                .with_graphics(|graphics| {
                    assert_eq!(
                        graphics.outgoing.is_some(),
                        aux[0] != 0 && native_resources[&aux[0]] != 0
                    );
                    assert_eq!(
                        graphics.incoming.is_some(),
                        aux[1] != 0 && native_resources[&aux[1]] != 0
                    )
                })
                .unwrap();
        }
        if corpus
            .manifest
            .observations
            .get(observed)
            .is_some_and(|o| o.row == row.index)
        {
            let o = &corpus.manifest.observations[observed];
            assert_eq!(observed, o.index);
            assert_eq!(actual_frames.len(), observed + 1);
            let original = reference_render_to_texture(
                &device,
                &queue,
                (o.extent[0], o.extent[1]),
                |ctx, target| frame_pixels::gpu::replay(ctx, target, &corpus, o),
            );
            let actual = &actual_frames[observed];
            let delta = differences(actual, &original);
            if delta != (0, 0) {
                let base = std::env::temp_dir()
                    .join(format!("higurashi-live-history-mismatch-{}", o.index));
                std::fs::write(base.with_extension("actual.rgba"), actual).unwrap();
                std::fs::write(base.with_extension("native.rgba"), &original).unwrap();
                eprintln!(
                    "mismatch {}; sample actual{:?}/original{:?}; root{:?}",
                    o.name,
                    &actual[..16],
                    &original[..16],
                    scene
                        .as_ref()
                        .unwrap()
                        .layers()
                        .screen_properties()
                        .drawable_params()
                );
            }
            assert_eq!(delta, (0, 0), "{} row{}", o.name, o.row);
            let colors = actual
                .chunks_exact(4)
                .map(|p| <[u8; 4]>::try_from(p).unwrap())
                .collect::<BTreeSet<_>>();
            if o.require_visible_pattern {
                assert!(colors.len() >= 3, "{}", o.name);
                assert!(actual.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]))
            }
            summaries.push(serde_json::json!({"index":o.index,"name":o.name,"kind":o.kind,"row":o.row,"extent":o.extent,"different_channels":delta.0,"max_difference":delta.1,"colors":colors.len()}));
            observed += 1;
        }
    }
    assert_eq!((observed, fresh_seed_calls, tile_seed_words), (54, 8, 96));
    assert!(scene.is_none() && screens.is_none());
    for c in &corpus.manifest.comparisons {
        let delta = differences(&actual_frames[c.baseline], &actual_frames[c.effect]);
        if c.equal {
            assert_eq!(delta, (0, 0), "{}", c.reason)
        } else {
            assert!(delta.0 > 0, "{}", c.reason)
        }
    }
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_AFTERIMAGE_INTEGRATION_SUMMARY") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"frames":summaries,"actions":178,"comparisons":34,"pair_identity":"bijective native-ID mapping, separate pair/depth allocations and retirements checked every operation","scope":"Production update/history/SceneTextures composition/current capture/outer copy; physical keyed publication, direct b919c transfer and property-restoration setters are checked fixture seams","audio":"none"})).unwrap()).unwrap()
    }
    println!(
        "live_afterimage_scene_integration=178actions/54exactObservations/40outer/14currentCaptures/34controls;pairIdentityEveryOperation;storedExtent;WeakRetirement;productionComposition;noAudio"
    );
}
