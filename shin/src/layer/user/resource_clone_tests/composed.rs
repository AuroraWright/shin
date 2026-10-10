//! Genuine ROM resources inside production Group/Slot graphs and final owners.
use super::*;
use crate::{
    layer::{Layer, LayerGroup},
    wiper::DefaultWiper,
};
use shin_core::vm::command::types::LayerbankId;

fn state(fixture: usize, phase: &str, roots: &[Option<LayerGroup>; 2], owners: &[Owner]) -> String {
    let mut blocks = BTreeSet::new();
    for o in owners {
        match o {
            Owner::Picture(p) => {
                if let Some(p) = p.upgrade() {
                    for (_, _, b) in p.blocks.iter_placements() {
                        blocks.insert(b as *const _ as usize);
                    }
                }
            }
            Owner::Bustup(b) => {
                if let Some(b) = b.upgrade() {
                    for b in b
                        .base_blocks
                        .iter()
                        .chain(b.face1.iter())
                        .chain(b.face2.iter())
                        .chain(&b.mouth_blocks)
                        .chain(&b.eye_blocks)
                    {
                        blocks.insert(Arc::as_ptr(b) as usize);
                    }
                }
            }
        }
    }
    let mut words = vec![
        "O".into(),
        fixture.to_string(),
        phase.into(),
        blocks.len().to_string(),
    ];
    let mut leaves = [Vec::new(), Vec::new()];
    let mut slots = 0;
    for (g, root) in roots.iter().enumerate() {
        words.push(usize::from(root.is_some()).to_string());
        if let Some(root) = root {
            let entries = root.reference_owner_entry_words();
            words.push(entries.len().to_string());
            for (bank, delay, transition) in entries {
                words.extend([
                    bank.to_string(),
                    format!("{delay:08x}"),
                    usize::from(transition).to_string(),
                ]);
            }
            leaves[g] = root.reference_owner_leaves();
            slots += root.reference_transition_words().len();
        }
    }
    words.extend(leaves.iter().map(|l| l.len().to_string()));
    let all: Vec<_> = leaves.into_iter().flatten().collect();
    for leaf in &all {
        let id = Owner::of(leaf).identity();
        let asset = owners.iter().position(|o| o.identity() == id).unwrap();
        let props = leaf.properties().reference_copy_words();
        words.extend([asset.to_string(), usize::from(props.is_some()).to_string()]);
        for i in 0..90 {
            words.push(format!("{:08x}", props.as_ref().map_or(0, |p| p[i])));
        }
    }
    for a in &all {
        for b in &all {
            let p = a.properties().initialized_identity();
            words.push(
                usize::from(p.is_some() && p == b.properties().initialized_identity()).to_string(),
            );
        }
    }
    words.extend([slots.to_string(), slots.to_string()]);
    words.join(" ")
}

#[test]
#[ignore = "requires supplied ROM,native composed-resource traces and Metal;no audio/window/saves"]
fn reference_native_resource_group_clone_and_final_retirement() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("SHIN_RESOURCE_GROUP_OUTPUT").unwrap());
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let mut observations = 0;
    for version in ["original", "1.03"] {
        let snr = std::fs::read(
            Path::new("/private/tmp/shin-authored-restoration")
                .join(version)
                .join("main.snr"),
        )
        .unwrap();
        let scenario = Scenario::new(snr.into()).unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join("umineko_switch_files/data.rom"))
            .unwrap();
        if version == "1.03" {
            io.try_with_rom(root.join("umineko_switch_files/patch.rom"))
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
        for mode in 0..3 {
            for sharing in [false, true] {
                for order in 0..2 {
                    for forward in [false, true] {
                        let label = format!(
                            "{version}-{mode}-{}-{order}-{}",
                            usize::from(sharing),
                            usize::from(forward)
                        );
                        let text =
                            std::fs::read_to_string(output.join(format!("{label}.trace"))).unwrap();
                        let expected: Vec<_> =
                            text.lines().filter(|l| l.starts_with("O ")).collect();
                        let mut actual = Vec::new();
                        for fixture in 0..2 {
                            let mut roots = [Some(LayerGroup::new(None)), None];
                            let mut owners = Vec::new();
                            let group = roots[0].as_mut().unwrap();
                            for line in text
                                .lines()
                                .filter(|l| l.starts_with(&format!("L {fixture} ")))
                            {
                                let f: Vec<_> = line.split_whitespace().collect();
                                let id = f[4].parse::<usize>().unwrap();
                                let layer: UserLayer = if f[3] == "2" {
                                    let p = shin_tasks::block_on(server.load::<Picture, _>(f[6]))
                                        .unwrap();
                                    PictureLayer::new(p, None).into()
                                } else {
                                    let info = &scenario.info_tables().bustup_info[id];
                                    let b =
                                        shin_tasks::block_on(server.load_with_args::<Bustup, _>(
                                            f[6],
                                            BustupArgs {
                                                expression: info.emotion.to_string(),
                                                character_id: CharacterId::from_metadata(
                                                    info.lipsync_character_id,
                                                ),
                                                disable_animations: false,
                                            },
                                        ))
                                        .unwrap();
                                    super::super::bustup_layer::BustupLayer::new(b, None).into()
                                };
                                owners.push(Owner::of(&layer));
                                group.add_layer(LayerbankId::new((owners.len() - 1) as u8), layer);
                            }
                            let picture_probes: Vec<_> = owners
                                .iter()
                                .filter_map(|o| match o {
                                    Owner::Picture(p) => {
                                        Some(Arc::downgrade(&p.upgrade().unwrap().blocks))
                                    }
                                    _ => None,
                                })
                                .collect();
                            let block_probes: Vec<_> = owners
                                .iter()
                                .flat_map(|o| match o {
                                    Owner::Bustup(b) => {
                                        let b = b.upgrade().unwrap();
                                        b.base_blocks
                                            .iter()
                                            .chain(b.face1.iter())
                                            .chain(b.face2.iter())
                                            .chain(&b.mouth_blocks)
                                            .chain(&b.eye_blocks)
                                            .map(Arc::downgrade)
                                            .collect::<Vec<_>>()
                                    }
                                    _ => Vec::new(),
                                })
                                .collect();
                            if sharing {
                                for n in 0..owners.len() {
                                    let layer =
                                        group.get_layer_mut(LayerbankId::new(n as u8)).unwrap();
                                    for i in 0..90 {
                                        layer
                                            .properties_mut()
                                            .property_tweener_mut(
                                                <LayerProperty as enum_map::Enum>::from_usize(i),
                                            )
                                            .fast_forward_to((100 + n * 128 + i) as f32);
                                    }
                                    for i in 0..3 {
                                        layer
                                            .properties_mut()
                                            .property_tweener_mut(LayerProperty::TranslateX)
                                            .enqueue(
                                                0.25 * (i + 1) as f32,
                                                Tween::linear(Ticks::from_f32(10.0)),
                                            );
                                    }
                                }
                            }
                            let mut encoder = device.create_command_encoder(&Default::default());
                            let mut ctx = RenderCloneCtx {
                                device: &device,
                                encoder: &mut encoder,
                            };
                            if mode != 0 {
                                let mut next = group
                                    .get_layer(LayerbankId::new(1))
                                    .unwrap()
                                    .render_clone(&mut ctx);
                                if sharing {
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
                            if mode == 2 {
                                let leaves = group.reference_owner_leaves();
                                let mut next = leaves[0].render_clone(&mut ctx);
                                if sharing {
                                    next.set_properties(leaves[0].properties().share());
                                }
                                group.add_layer_with_wiper(
                                    LayerbankId::new(0),
                                    next,
                                    Some(DefaultWiper::new(Ticks::from_f32(30.0)).into()),
                                );
                                assert!(
                                    group
                                        .remove_layer(LayerbankId::new(0), Ticks::from_f32(3.0))
                                        .is_none()
                                );
                                let next = group
                                    .get_layer(LayerbankId::new(1))
                                    .unwrap()
                                    .render_clone(&mut ctx);
                                group.add_layer(LayerbankId::new(0), next);
                            }
                            actual.push(state(fixture, "prepared", &roots, &owners));
                            roots[1] = Some(roots[0].as_ref().unwrap().render_clone(&mut ctx));
                            actual.push(state(fixture, "copied", &roots, &owners));
                            roots[1].as_ref().unwrap().reference_owner_leaves()[0]
                                .properties()
                                .share()
                                .property_tweener_mut(LayerProperty::TranslateX)
                                .fast_forward_to(999.0);
                            actual.push(state(fixture, "mutated", &roots, &owners));
                            if forward {
                                roots[1].as_mut().unwrap().fast_forward();
                                actual.push(state(fixture, "forward-copy", &roots, &owners));
                            }
                            roots[order] = None;
                            actual.push(state(
                                fixture,
                                if order == 0 {
                                    "drop-original"
                                } else {
                                    "drop-copy"
                                },
                                &roots,
                                &owners,
                            ));
                            roots[1 - order] = None;
                            actual.push(state(fixture, "final", &roots, &owners));
                            assert!(owners.iter().all(|o| !o.live()));
                            assert!(picture_probes.iter().all(|p| p.strong_count() == 0));
                            assert!(block_probes.iter().all(|p| p.strong_count() == 0));
                            device.poll(wgpu::Maintain::Wait);
                        }
                        std::fs::write(
                            output.join(format!("{label}.port.trace")),
                            actual.join("\n") + "\n",
                        )
                        .unwrap();
                        assert_eq!(actual, expected, "{label}");
                        observations += actual.len();
                    }
                }
            }
        }
    }
    println!(
        "Composed resource owners:96 cases,{observations} native/port observations;all logical resources retired"
    );
}
