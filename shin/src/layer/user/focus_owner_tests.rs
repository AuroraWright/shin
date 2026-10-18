//! Genuine native warmed ray and queued-owner copies compared with desktop.
use super::*;
use crate::{
    asset::{
        effects::EffectsAtlasBorrow,
        system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    },
    layer::LayerGroup,
    update::AdvUpdateContext,
    wiper::DefaultWiper,
};
use shin_core::{
    primitives::update::FrameId,
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{collections::BTreeSet, path::Path};

enum Root {
    Focus(UserLayer),
    Group(LayerGroup),
}
impl Root {
    fn leaves(&self) -> Vec<crate::layer::layer_group::ReferenceLayerLeaf<'_, UserLayer>> {
        match self {
            Self::Focus(l) => vec![crate::layer::layer_group::ReferenceLayerLeaf::borrowed(l)],
            Self::Group(g) => g.reference_owner_leaves(),
        }
    }
    fn target(&mut self) -> crate::layer::owner::LayerWrite<'_, UserLayer> {
        match self {
            Self::Focus(l) => crate::layer::owner::LayerWrite::Direct(l),
            Self::Group(g) => g.get_layer_mut(LayerbankId::new(0)).unwrap(),
        }
    }
    fn copied(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Focus(l) => Self::Focus(l.render_clone(ctx)),
            Self::Group(g) => Self::Group(g.render_clone(ctx)),
        }
    }
    fn forward(&mut self) {
        match self {
            Self::Focus(l) => l.fast_forward(),
            Self::Group(g) => g.fast_forward(),
        }
    }
    fn slots(&self) -> usize {
        match self {
            Self::Focus(_) => 0,
            Self::Group(g) => g.reference_transition_words().len(),
        }
    }
}
fn focus(l: &UserLayer) -> &focus_line_layer::FocusLineLayer {
    let UserLayer::FocusLine(l) = l else {
        panic!("Expected actual FocusLine variant")
    };
    l
}
fn focus_mut(l: &mut UserLayer) -> &mut focus_line_layer::FocusLineLayer {
    let UserLayer::FocusLine(l) = l else {
        panic!("Expected actual FocusLine variant")
    };
    l
}
fn make_focus(textures: &EffectsAtlasBorrow, queued: bool, populated: bool) -> UserLayer {
    let mut l: UserLayer =
        focus_line_layer::FocusLineLayer::new(textures.clone(), -200, 100).into();
    if queued {
        let alpha = &mut focus_mut(&mut l).reference_owner_rays_mut().rays[0].alpha;
        alpha.fast_forward_to(1.0);
        // One active descriptor plus 205 pending entries spans native 204-entry blocks.
        for j in 0..206 {
            alpha.enqueue(
                1.0 - (j + 1) as f32 * 0.001,
                Tween::linear(Ticks::from_f32(10.0)),
            );
        }
    }
    if populated {
        for i in 0..90 {
            let mut t = l
                .properties_mut()
                .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i));
            t.fast_forward_to(100.0 + i as f32);
            t.enqueue(101.0 + i as f32, Tween::linear(Ticks::from_f32(10.0)));
        }
    }
    l
}
fn prepare_root(
    textures: &EffectsAtlasBorrow,
    queued: bool,
    populated: bool,
    graph: usize,
) -> Root {
    if graph == 0 {
        return Root::Focus(make_focus(textures, queued, populated));
    }
    let mut group = LayerGroup::new(None);
    group.add_layer(LayerbankId::new(0), make_focus(textures, queued, populated));
    if graph == 2 {
        let mut next = make_focus(textures, queued, populated);
        if populated {
            next.set_properties(
                group
                    .get_layer(LayerbankId::new(0))
                    .unwrap()
                    .properties()
                    .share(),
            );
        }
        group.add_layer_with_wiper(
            LayerbankId::new(0),
            next,
            Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
        );
    }
    Root::Group(group)
}
fn snapshot(
    id: usize,
    phase: &str,
    roots: &[Option<Root>; 3],
    textures: &EffectsAtlasBorrow,
    last_rng: &mut usize,
) -> String {
    let now = crate::engine_random::seed_call_count();
    let mut words = vec![
        "F".into(),
        id.to_string(),
        phase.into(),
        (now - *last_rng).to_string(),
    ];
    *last_rng = now;
    let mut owners = Vec::new();
    let mut pointers = BTreeSet::new();
    let mut _live = 0;
    let mut slots = 0;
    for root in roots {
        let leaves = root.as_ref().map_or_else(Vec::new, Root::leaves);
        words.extend([
            usize::from(root.is_some()).to_string(),
            leaves.len().to_string(),
        ]);
        if let Some(root) = root {
            slots += root.slots();
        }
        for l in leaves {
            _live += 1;
            let f = focus(&l);
            let rays = f.reference_owner_rays();
            assert_eq!(rays.rays.len(), 40);
            assert!(
                pointers.insert(rays.rays.as_ptr() as usize),
                "Each live layer owns an independent populated ray buffer"
            );
            let props = l.properties();
            let owner = props.initialized_identity();
            owners.push(owner);
            let offset = f.reference_owner_offset();
            words.extend([
                usize::from(owner.is_some()).to_string(),
                format!("{:08x}", f.reference_dissolve_seed()),
                rays.reference_seed().to_string(),
                format!("{:08x}", offset.x.to_bits()),
                format!("{:08x}", offset.y.to_bits()),
                rays.rays.len().to_string(),
            ]);
            for ray in &rays.rays {
                words.extend([
                    format!("{:08x}", ray.angle.to_bits()),
                    format!("{:08x}", ray.alpha.value().to_bits()),
                    usize::from(!ray.alpha.is_idle()).to_string(),
                ]);
            }
            if owner.is_some() {
                for i in 0..90 {
                    let t = props
                        .reference_tweener_snapshot(<LayerProperty as enum_map::Enum>::from_usize(
                            i,
                        ))
                        .unwrap();
                    words.extend([
                        format!("{:08x}", t.value().to_bits()),
                        usize::from(!t.is_idle()).to_string(),
                    ]);
                }
            }
        }
    }
    for a in &owners {
        for b in &owners {
            words.push(usize::from(a.is_some() && a == b).to_string());
        }
    }
    words.push(slots.to_string());
    assert_eq!(
        textures.reference_strong_count(),
        1,
        "Engine atlas owner only; layer and copy handles borrow"
    );
    words.join(" ")
}

#[test]
#[ignore = "supplied ROM,native FocusLine ownership traces and Metal;no audio/window/saves"]
fn reference_native_focusline_clone_group_slot_and_final_retirement() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_FOCUS_OWNER_OUTPUT").unwrap());
    let request = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut total = 0;
    for version in ["original", "1.03"] {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("game-data/umineko/data.rom"))
            .unwrap();
        if version == "1.03" {
            io.try_with_rom(root.join("game-data/umineko/patch.rom"))
                .unwrap();
        }
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let textures: EffectsAtlasBorrow = shin_tasks::block_on(server.effects_atlas()).unwrap();
        let weak = textures.reference_weak();
        assert_eq!(weak.strong_count(), 1);
        let mut actual = Vec::new();
        let mut frame = FrameId::default();
        let mut advance = |root: &mut Root| {
            frame.advance();
            let update = AdvUpdateContext {
                frame_id: frame,
                delta_ticks: Ticks::from_f32(5.0),
                asset_server: &server,
                device: &device,
                queue: &queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            };
            root.target().update(&update);
        };
        for (id, line) in request.lines().enumerate() {
            let p: Vec<usize> = line
                .split_whitespace()
                .map(|w| w.parse().unwrap())
                .collect();
            let (queued, populated, graph, order) = (p[0] != 0, p[1] != 0, p[2], p[3]);
            crate::engine_random::with_reference_seed(5489, || {
                let mut last_rng = crate::engine_random::seed_call_count();
                let mut roots = [
                    Some(prepare_root(&textures, queued, populated, graph)),
                    None,
                    None,
                ];
                actual.push(snapshot(id, "source", &roots, &textures, &mut last_rng));
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut ctx = RenderCloneCtx {
                    device: &device,
                    encoder: &mut encoder,
                };
                for g in 1..3 {
                    roots[g] = Some(roots[g - 1].as_ref().unwrap().copied(&mut ctx));
                }
                queue.submit(Some(encoder.finish()));
                actual.push(snapshot(id, "copies", &roots, &textures, &mut last_rng));
                advance(roots[1].as_mut().unwrap());
                actual.push(snapshot(
                    id,
                    "advance-copy",
                    &roots,
                    &textures,
                    &mut last_rng,
                ));
                focus_mut(&mut roots[1].as_mut().unwrap().target())
                    .reference_owner_rays_mut()
                    .reference_owner_mutate();
                actual.push(snapshot(
                    id,
                    "mutate-copy",
                    &roots,
                    &textures,
                    &mut last_rng,
                ));
                roots[1].as_mut().unwrap().forward();
                actual.push(snapshot(
                    id,
                    "forward-copy",
                    &roots,
                    &textures,
                    &mut last_rng,
                ));
                for i in 0..3 {
                    let g = if order != 0 { 2 - i } else { i };
                    drop(roots[g].take());
                    actual.push(snapshot(id, "drop", &roots, &textures, &mut last_rng));
                    for root in roots.iter_mut().flatten() {
                        advance(root);
                    }
                    actual.push(snapshot(id, "survivors", &roots, &textures, &mut last_rng));
                }
                assert_eq!(weak.strong_count(), 1);
                total += 1;
            });
        }
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            actual.join("\n") + "\n",
        )
        .unwrap();
        let native = std::fs::read_to_string(output.join(format!("{version}.trace"))).unwrap();
        let expected: Vec<_> = native.lines().filter(|l| l.starts_with("F ")).collect();
        assert_eq!(actual.len(), expected.len());
        for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a, b, "{version} native FocusLine row{i}");
        }
        drop(textures);
        assert_eq!(
            weak.strong_count(),
            1,
            "Engine retains atlas after all layers"
        );
        drop(server);
        assert_eq!(weak.strong_count(), 0, "Engine atlas owner retired");
        device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
    }
    println!(
        "FocusLine ownership:{total} cases;actual warmed rays/queued tweens/Group/Slot copies independent;two final desktop atlas fixtures retired"
    );
}
