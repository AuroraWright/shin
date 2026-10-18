//! Genuine native Rain vector/Group/Slot copies and logical desktop retirement.
use super::*;
use crate::{
    asset::{
        effects::EffectsAtlasBorrow,
        system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    },
    layer::LayerGroup,
    wiper::DefaultWiper,
};
use shin_core::{
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{collections::BTreeSet, path::Path};

enum Root {
    Rain(UserLayer),
    Group(LayerGroup),
}
impl Root {
    fn leaves(&self) -> Vec<crate::layer::layer_group::ReferenceLayerLeaf<'_, UserLayer>> {
        match self {
            Self::Rain(l) => vec![crate::layer::layer_group::ReferenceLayerLeaf::borrowed(l)],
            Self::Group(g) => g.reference_owner_leaves(),
        }
    }
    fn target(&mut self) -> crate::layer::owner::LayerWrite<'_, UserLayer> {
        match self {
            Self::Rain(l) => crate::layer::owner::LayerWrite::Direct(l),
            Self::Group(g) => g.get_layer_mut(LayerbankId::new(0)).unwrap(),
        }
    }
    fn copied(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Rain(l) => Self::Rain(l.render_clone(ctx)),
            Self::Group(g) => Self::Group(g.render_clone(ctx)),
        }
    }
    fn forward(&mut self) {
        match self {
            Self::Rain(l) => l.fast_forward(),
            Self::Group(g) => g.fast_forward(),
        }
    }
    fn slots(&self) -> usize {
        match self {
            Self::Rain(_) => 0,
            Self::Group(g) => g.reference_transition_words().len(),
        }
    }
}
fn rain(l: &UserLayer) -> &rain_layer::RainLayer {
    let UserLayer::Rain(l) = l else {
        panic!("Expected actual Rain variant")
    };
    l
}
fn rain_mut(l: &mut UserLayer) -> &mut rain_layer::RainLayer {
    let UserLayer::Rain(l) = l else {
        panic!("Expected actual Rain variant")
    };
    l
}
fn make_rain(
    textures: &EffectsAtlasBorrow,
    mode: i32,
    count: usize,
    identity: usize,
    populated: bool,
) -> UserLayer {
    let mut l: UserLayer = rain_layer::RainLayer::new(textures.clone(), mode, 1500, 100).into();
    rain_mut(&mut l)
        .inner_mut()
        .reference_owner_prepare(count, identity);
    if populated {
        for i in 0..90 {
            let mut t = l
                .properties_mut()
                .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i));
            t.fast_forward_to(100.0 + i as f32);
            for j in 0..3 {
                t.enqueue(
                    100.0 + i as f32 + (j + 1) as f32 * 0.25,
                    Tween::linear(Ticks::from_f32(10.0)),
                );
            }
        }
    }
    l
}
fn prepare_root(
    textures: &EffectsAtlasBorrow,
    mode: i32,
    count: usize,
    populated: bool,
    graph: usize,
) -> Root {
    if graph == 0 {
        return Root::Rain(make_rain(textures, mode, count, 0, populated));
    }
    let mut group = LayerGroup::new(None);
    group.add_layer(
        LayerbankId::new(0),
        make_rain(textures, mode, count, 0, populated),
    );
    let shared = if populated {
        Some(
            group
                .get_layer(LayerbankId::new(0))
                .unwrap()
                .properties()
                .share(),
        )
    } else {
        None
    };
    for i in 1..graph {
        let mut next = make_rain(
            textures,
            mode,
            if count == 0 { 0 } else { count + i },
            i,
            populated,
        );
        if let Some(shared) = &shared {
            next.set_properties(shared.share());
        }
        group.add_layer_with_wiper(
            LayerbankId::new(0),
            next,
            Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
        );
    }
    drop(shared);
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
        "R".into(),
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
            let r = rain(&l);
            let (pointer, data) = r.inner_ref().reference_owner_words();
            if let Some(pointer) = pointer {
                assert!(
                    pointers.insert(pointer),
                    "Every populated particle vector belongs to exactly one layer"
                );
            }
            let props = l.properties();
            let owner = props.initialized_identity();
            owners.push(owner);
            words.extend([
                usize::from(owner.is_some()).to_string(),
                format!("{:08x}", r.reference_dissolve_seed()),
            ]);
            words.extend(data);
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
fn advance(root: &mut Root) {
    let mut l = root.target();
    // Execute the particle model independently of rendering/warm-up. Getter
    // lazily allocates properties as native c9eb0/75d20 does, even with dt0.
    let intensity = l.properties().get_value(LayerProperty::RainIntensity);
    let direction = l.properties().get_value(LayerProperty::RainDirection);
    let speed = l.properties().get_value(LayerProperty::RainSpeed);
    rain_mut(&mut l)
        .inner_mut()
        .reference_owner_advance(intensity, direction, speed);
    for i in 0..90 {
        l.properties_mut()
            .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i))
            .update(Ticks::from_f32(5.0));
    }
}
#[test]
#[ignore = "supplied ROM,native Rain ownership traces and Metal;no audio/window/saves"]
fn reference_native_rain_clone_group_slot_and_final_retirement() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_RAIN_OWNER_OUTPUT").unwrap());
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
        let server = AssetServer::new(
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
        );
        let textures: EffectsAtlasBorrow = shin_tasks::block_on(server.effects_atlas()).unwrap();
        let weak = textures.reference_weak();
        assert_eq!(weak.strong_count(), 1);
        let mut actual = Vec::new();
        for (id, line) in request.lines().enumerate() {
            let p: Vec<usize> = line
                .split_whitespace()
                .map(|w| w.parse().unwrap())
                .collect();
            let (mode, count, populated, graph, order) = (p[0] as i32, p[1], p[2] != 0, p[3], p[4]);
            crate::engine_random::with_reference_seed(5489, || {
                let mut last_rng = crate::engine_random::seed_call_count();
                let mut roots = [
                    Some(prepare_root(&textures, mode, count, populated, graph)),
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
                rain_mut(&mut roots[1].as_mut().unwrap().target())
                    .inner_mut()
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
        let native = std::fs::read_to_string(output.join(format!("{version}.trace"))).unwrap();
        let expected: Vec<_> = native.lines().filter(|l| l.starts_with("R ")).collect();
        assert_eq!(actual.len(), expected.len());
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            actual.join("\n") + "\n",
        )
        .unwrap();
        for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a, b, "{version} native Rain row{i}");
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
        "Rain ownership:{total} cases;all individual/group/active/nested Slot particle vectors independent;both final atlas owners retired"
    );
}
