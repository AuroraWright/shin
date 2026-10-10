//! Real interactive controllers in native-shaped direct/active/nested graphs.
use super::*;
use crate::{
    layer::{Layer, LayerGroup},
    wiper::DefaultWiper,
};
use shin_core::vm::command::types::LayerbankId;
use std::collections::BTreeSet;

fn prepare(
    textures: &Textures,
    mask: usize,
    populated: bool,
    queued: usize,
    completed: bool,
) -> UserLayer {
    let mut layer = textures.new_layer(mask as i32);
    if completed {
        match &mut layer {
            UserLayer::Quiz(l) => {
                l.restore_completed_answer((usize::BITS - 1 - mask.leading_zeros()) as i32)
            }
            UserLayer::Cake(l) => l.restore_completed_answer(3),
            UserLayer::Last(l) => l.restore_completed_answer(3),
            _ => unreachable!(),
        }
    }
    clocks(&layer, "selection", 0, 0.0);
    clocks(&layer, "populate", queued, 0.0);
    if populated {
        for i in 0..90 {
            clock_command(
                &mut layer
                    .properties_mut()
                    .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(i)),
                "populate",
                i,
                3,
                0.0,
            );
        }
    }
    layer
}
fn graph(
    textures: &Textures,
    mask: usize,
    populated: bool,
    queued: usize,
    completed: bool,
    graph: usize,
) -> LayerGroup {
    let mut group = LayerGroup::new(None);
    group.add_layer(
        LayerbankId::new(0),
        prepare(textures, mask, populated, queued, completed),
    );
    for _ in 1..graph {
        let mut next = prepare(textures, mask, false, queued, completed);
        if populated {
            next.set_properties(group.reference_owner_leaves()[0].properties().share());
        }
        group.add_layer_with_wiper(
            LayerbankId::new(0),
            next,
            Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
        );
    }
    group
}
fn target(group: &mut LayerGroup) -> &mut UserLayer {
    group.get_layer_mut(LayerbankId::new(0)).unwrap()
}
fn snapshot(
    id: usize,
    phase: &str,
    roots: &[Option<LayerGroup>; 3],
    textures: &Textures,
    probes: &mut Vec<(usize, Box<dyn Fn() -> usize>)>,
) -> String {
    let mut current = BTreeSet::new();
    let count: usize = roots
        .iter()
        .flatten()
        .map(|g| g.reference_owner_leaves().len())
        .sum();
    assert_eq!(textures.count(), count + 1);
    let mut words = vec![
        "J".into(),
        id.to_string(),
        phase.into(),
        usize::from(count != 0).to_string(),
    ];
    let mut properties = Vec::new();
    let mut slots = 0;
    for root in roots {
        let leaves = root
            .as_ref()
            .map_or_else(Vec::new, |g| g.reference_owner_leaves());
        words.extend([
            usize::from(root.is_some()).to_string(),
            leaves.len().to_string(),
        ]);
        if let Some(g) = root {
            slots += g.reference_transition_words().len();
        }
        for l in leaves {
            let probe = owner(l);
            assert!(
                current.insert(probe.0),
                "Every mutable controller is distinct"
            );
            if !probes.iter().any(|p| p.0 == probe.0 && p.1() != 0) {
                probes.push(probe);
            }
            let p = l.properties();
            let identity = p.initialized_identity();
            properties.push(identity);
            words.extend([
                selected(l).to_string(),
                usize::from(complete(l)).to_string(),
                usize::from(identity.is_some()).to_string(),
            ]);
            if identity.is_some() {
                for i in 0..90 {
                    let t = p
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
            let c = clocks(l, "observe", 0, 0.0);
            words.push(c.len().to_string());
            for (bits, busy) in c {
                words.extend([format!("{bits:08x}"), usize::from(busy).to_string()]);
            }
        }
    }
    for (identity, weak) in probes {
        assert_eq!(
            weak(),
            usize::from(current.contains(identity)),
            "Only actual graph owners retain each controller"
        );
    }
    for a in &properties {
        for b in &properties {
            words.push(usize::from(a.is_some() && a == b).to_string());
        }
    }
    words.push(slots.to_string());
    words.join(" ")
}
#[test]
#[ignore = "supplied ROM,native composed traces and offscreen Metal;no audio/window/saves"]
fn reference_native_plain_group_slot_clone_and_final_retirement() {
    crate::init_test_task_pools();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output =
        std::path::PathBuf::from(std::env::var_os("SHIN_PLAIN_GROUP_OWNER_OUTPUT").unwrap());
    let request = std::fs::read_to_string(output.join("request.txt")).unwrap();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let mut cases = 0;
    for version in ["original", "1.03"] {
        let native = std::fs::read_to_string(output.join(format!("{version}.trace"))).unwrap();
        let expected: Vec<_> = native.lines().filter(|l| l.starts_with("J ")).collect();
        let mut rows = Vec::new();
        for kind in 0..3 {
            let mut io = LayeredAssetIo::new();
            io.try_with_rom(workspace.join("umineko_switch_files/data.rom"))
                .unwrap();
            if version == "1.03" {
                io.try_with_rom(workspace.join("umineko_switch_files/patch.rom"))
                    .unwrap();
            }
            let server = AssetServer::new(
                io.into(),
                AssetLoadContext {
                    wgpu_device: device.clone(),
                    wgpu_queue: queue.clone(),
                    bustup_cache: AssetCache::new(),
                    picture_cache: Default::default(),
                    font_cache: Default::default(),
                },
            );
            let textures = match kind {
                0 => Textures::Quiz(shin_tasks::block_on(server.load("/quiz.txa")).unwrap()),
                1 => Textures::Cake(shin_tasks::block_on(server.load("/cake.txa")).unwrap()),
                2 => Textures::Last(shin_tasks::block_on(server.load("/last.txa")).unwrap()),
                _ => unreachable!(),
            };
            drop(server);
            let atlas = textures.probe();
            assert_eq!(atlas(), 1);
            for (id, line) in request.lines().enumerate() {
                let fields: Vec<usize> = line
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                if fields[0] != kind {
                    continue;
                }
                let (mask, populated, queued, completed, mode, order) = (
                    fields[1],
                    fields[2] != 0,
                    fields[3],
                    fields[4] != 0,
                    fields[5],
                    fields[6],
                );
                let mut roots = [
                    Some(graph(&textures, mask, populated, queued, completed, mode)),
                    None,
                    None,
                ];
                let mut probes = Vec::new();
                rows.push(snapshot(id, "source", &roots, &textures, &mut probes));
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut ctx = RenderCloneCtx {
                    device: &device,
                    encoder: &mut encoder,
                };
                for g in 1..3 {
                    roots[g] = Some(roots[g - 1].as_ref().unwrap().render_clone(&mut ctx));
                }
                queue.submit(Some(encoder.finish()));
                rows.push(snapshot(id, "copies", &roots, &textures, &mut probes));
                advance(target(roots[1].as_mut().unwrap()), 5.0);
                rows.push(snapshot(id, "advance-copy", &roots, &textures, &mut probes));
                let l = target(roots[1].as_mut().unwrap());
                clocks(l, "mutate", 0, 0.0);
                l.properties_mut()
                    .property_tweener_mut(<LayerProperty as enum_map::Enum>::from_usize(0))
                    .fast_forward_to(-123.0);
                rows.push(snapshot(id, "mutate-copy", &roots, &textures, &mut probes));
                roots[1].as_mut().unwrap().fast_forward();
                rows.push(snapshot(id, "forward-copy", &roots, &textures, &mut probes));
                for i in 0..3 {
                    drop(roots[if order != 0 { 2 - i } else { i }].take());
                    rows.push(snapshot(id, "drop", &roots, &textures, &mut probes));
                    for g in roots.iter_mut().flatten() {
                        advance(target(g), 10.0);
                    }
                    rows.push(snapshot(id, "survivors", &roots, &textures, &mut probes));
                }
                assert!(probes.iter().all(|(_, w)| w() == 0));
                assert_eq!(atlas(), 1);
                cases += 1;
            }
            drop(textures);
            assert_eq!(atlas(), 0, "Final decoded fixture atlas retired");
            device.poll(wgpu::Maintain::Wait);
        }
        std::fs::write(
            output.join(format!("{version}.port.trace")),
            rows.join("\n") + "\n",
        )
        .unwrap();
        assert_eq!(rows.len(), expected.len());
        for (i, (a, b)) in rows.iter().zip(expected).enumerate() {
            assert_eq!(a, b, "{version} composed interactive native row{i}");
        }
    }
    println!(
        "Interactive Group/Slot ownership:{cases} cases;exact native rows;all per-case mutable models and six final fixture atlas owners retired"
    );
}
