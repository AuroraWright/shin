use super::*;
use crate::wiper::DefaultWiper;
use shin_core::time::Ticks;
use shin_render::resize::{SurfaceResizeSource, ViewportParams};

fn tile(index: usize) -> TileLayer {
    let i = index as f32;
    TileLayer::new(
        FloatColor4::from_vec4(glam::vec4(0.125 * i, 0.25, 0.5, 1.0)),
        glam::vec4(-32.0 * i, 16.0 * i, 96.0, 48.0),
    )
}
fn groups<'a>(root: &'a Root, planes: usize) -> Vec<&'a LayerGroup> {
    match root {
        Root::Group(group) => vec![group],
        Root::Page(page) => (0..planes)
            .map(|i| page.get_plane(PlaneId::new(i as u8)))
            .collect(),
        Root::Screen(screen) => (0..planes)
            .map(|i| screen.page_layer().get_plane(PlaneId::new(i as u8)))
            .collect(),
    }
}
fn states(roots: &[Option<Root>; 2], planes: usize) -> Vec<Vec<[u32; 7]>> {
    roots
        .iter()
        .map(|root| {
            root.as_ref()
                .map(|root| {
                    groups(root, planes)
                        .into_iter()
                        .flat_map(LayerGroup::reference_transition_words)
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}
fn initialize(kind: usize, planes: usize, empty: bool, mask: usize, partition: usize) -> Root {
    let mut root = Root::new(kind, planes);
    for g in 0..if kind == 0 { 1 } else { planes } {
        for t in usize::from(empty)..2 {
            root.group_mut(g).add_layer(
                LayerbankId::new(if t == 0 { 1 } else { 5 }),
                tile(g * 2 + t).into(),
            );
        }
    }
    let mut nodes = Vec::new();
    root.collect(&mut nodes, planes);
    for (n, node) in nodes.iter().enumerate() {
        if mask & (1 << n) != 0 {
            use num_traits::FromPrimitive;
            let mut props = node.properties.share();
            for i in 0..90 {
                props
                    .property_tweener_mut(LayerProperty::from_u32(i).unwrap())
                    .fast_forward_to((n * 128 + i as usize) as f32);
            }
        }
    }
    let shared = (partition == 1).then(|| {
        nodes
            .iter()
            .find(|n| n.kind == 3)
            .unwrap()
            .properties
            .share()
    });
    drop(nodes);
    if let Some(shared) = shared {
        for g in 0..if kind == 0 { 1 } else { planes } {
            for t in usize::from(empty)..2 {
                root.group_mut(g)
                    .get_layer_mut(LayerbankId::new(if t == 0 { 1 } else { 5 }))
                    .unwrap()
                    .set_properties(shared.share());
            }
        }
    }
    root
}

#[test]
#[ignore = "requires Metal;actual native populated SlotLayer construction,clone and final destruction;no audio"]
fn reference_native_populated_slot_copy_and_retirement() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::var_os("SHIN_POPULATED_SLOT_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("shin-native-populated-slot-results"));
    fs::create_dir_all(&folder).unwrap();
    let oracle = folder.join("oracle");
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join("reverse-engineering/native-populated-slot-retirement.c"))
        .arg("-o")
        .arg(&oracle)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, _queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ctx = RenderCloneCtx {
        device: &device,
        encoder: &mut encoder,
    };
    let resize =
        SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)));
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for (kind, planes) in [(0, 1), (1, 2), (2, 2)] {
        for mode in 0..3 {
            let empty = mode == 0;
            let group_count = if kind == 0 { 1 } else { planes };
            let template = initialize(kind, planes, empty, 0, 0);
            let mut nodes = Vec::new();
            template.collect(&mut nodes, planes);
            let all = (1usize << nodes.len()) - 1;
            for (mask, partition) in [(0, 0), (all, 0), (all, 1)] {
                if empty && kind == 0 && partition == 1 {
                    continue;
                }
                for sharing in 0..if empty { 1 } else { 2 } {
                    for captured in [false, true] {
                        for drop_first in 0..2 {
                            profiles += 1;
                            crate::engine_random::with_reference_seed(5489, || {
                                let mut roots: [Option<Root>; 2] = [None, None];
                                let mut captures_created = 0usize;
                                let mut commands = vec![format!(
                                    "N {kind} {planes} {mask:x} {partition} {}",
                                    usize::from(empty)
                                )];
                                for g in 0..group_count {
                                    commands.push(format!(
                                        "R 0 {g} 1 {sharing} 2 {:08x}",
                                        30.0f32.to_bits()
                                    ));
                                }
                                commands.push(format!("V 0 {:08x}", 7.5f32.to_bits()));
                                if mode == 2 {
                                    for g in 0..group_count {
                                        commands.push(format!(
                                            "R 0 {g} 1 {sharing} 3 {:08x}",
                                            45.0f32.to_bits()
                                        ));
                                    }
                                    commands.push(format!("V 0 {:08x}", 1.25f32.to_bits()));
                                }
                                if captured {
                                    commands.push("P 0".into());
                                }
                                let source_index = kind + 1;
                                let target_index = source_index + mode;
                                commands.extend([
                                    "C".into(),
                                    format!("M 1 {target_index} {:08x}", 999.0f32.to_bits()),
                                    format!("M 0 {source_index} {:08x}", 777.0f32.to_bits()),
                                    format!("D {drop_first}"),
                                    format!("V {} {:08x}", 1 - drop_first, 2.5f32.to_bits()),
                                    format!(
                                        "M {} {target_index} {:08x}",
                                        1 - drop_first,
                                        333.0f32.to_bits()
                                    ),
                                ]);
                                for g in 0..group_count {
                                    commands
                                        .push(format!("R {} {g} 0 0 7 00000000", 1 - drop_first));
                                }
                                commands.push(format!("D {}", 1 - drop_first));
                                for command in commands {
                                    let fields: Vec<_> = command.split_whitespace().collect();
                                    let op = fields[0].chars().next().unwrap();
                                    let before = crate::engine_random::seed_call_count();
                                    match op {
                                        'N' => {
                                            roots[0] = Some(initialize(
                                                kind, planes, empty, mask, partition,
                                            ))
                                        }
                                        'R' => {
                                            let g = fields[1].parse::<usize>().unwrap();
                                            let plane = fields[2].parse::<usize>().unwrap();
                                            let generation = fields[5].parse::<usize>().unwrap();
                                            let duration = f32::from_bits(
                                                u32::from_str_radix(fields[6], 16).unwrap(),
                                            );
                                            let group = roots[g].as_mut().unwrap().group_mut(plane);
                                            let mut next = tile(plane * 10 + generation);
                                            if fields[4] == "1" {
                                                if let Some(old) =
                                                    group.get_layer(LayerbankId::new(1))
                                                {
                                                    next.set_properties(old.properties().share());
                                                }
                                            }
                                            group.add_layer_with_wiper(
                                                LayerbankId::new(1),
                                                next.into(),
                                                (fields[3] == "1").then(|| {
                                                    DefaultWiper::new(Ticks::from_f32(duration))
                                                        .into()
                                                }),
                                            );
                                        }
                                        'C' => {
                                            roots[1] = Some(
                                                roots[0].as_ref().unwrap().render_clone(&mut ctx),
                                            )
                                        }
                                        'V' => {
                                            let g = fields[1].parse::<usize>().unwrap();
                                            let delta = f32::from_bits(
                                                u32::from_str_radix(fields[2], 16).unwrap(),
                                            );
                                            for p in 0..group_count {
                                                roots[g]
                                                    .as_mut()
                                                    .unwrap()
                                                    .group_mut(p)
                                                    .reference_advance_transition_clocks(delta);
                                            }
                                        }
                                        'P' => {
                                            let g = fields[1].parse::<usize>().unwrap();
                                            for p in 0..group_count {
                                                captures_created += roots[g]
                                                    .as_mut()
                                                    .unwrap()
                                                    .group_mut(p)
                                                    .reference_prepare_transition_textures(
                                                        &device, &resize,
                                                    );
                                            }
                                        }
                                        'M' => {
                                            let g = fields[1].parse::<usize>().unwrap();
                                            let n = fields[2].parse::<usize>().unwrap();
                                            let mut nodes = Vec::new();
                                            roots[g].as_ref().unwrap().collect(&mut nodes, planes);
                                            nodes[n]
                                                .properties
                                                .share()
                                                .property_tweener_mut(LayerProperty::TranslateX)
                                                .fast_forward_to(f32::from_bits(
                                                    u32::from_str_radix(fields[3], 16).unwrap(),
                                                ));
                                        }
                                        'D' => roots[fields[1].parse::<usize>().unwrap()] = None,
                                        _ => unreachable!(),
                                    }
                                    expected.push(snapshot(
                                        op,
                                        crate::engine_random::seed_call_count() - before,
                                        &roots,
                                        planes,
                                    ));
                                    let states = states(&roots, planes);
                                    let live: usize = states
                                        .iter()
                                        .flatten()
                                        .map(|w| (w[5] + w[6]) as usize)
                                        .sum();
                                    let mut words = vec![
                                        "T".into(),
                                        op.to_string(),
                                        states[0].len().to_string(),
                                        states[1].len().to_string(),
                                    ];
                                    words.extend(
                                        states
                                            .into_iter()
                                            .flatten()
                                            .flatten()
                                            .map(|v| format!("{v:08x}")),
                                    );
                                    expected.push(words);
                                    expected.push(vec![
                                        "H".into(),
                                        op.to_string(),
                                        captures_created.to_string(),
                                        (captures_created - live).to_string(),
                                        live.to_string(),
                                    ]);
                                    input.push_str(&command);
                                    input.push('\n');
                                }
                            });
                        }
                    }
                }
            }
        }
    }
    let request = folder.join("request.txt");
    fs::write(&request, input).unwrap();
    fs::write(
        folder.join("port.trace"),
        expected
            .iter()
            .map(|w| w.join(" "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let mut mismatches = Vec::new();
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        let output = Command::new(&oracle)
            .arg(root.join("game-data/umineko").join(executable))
            .stdin(Stdio::from(fs::File::open(&request).unwrap()))
            .output()
            .unwrap();
        fs::write(folder.join(format!("{version}.trace")), &output.stdout).unwrap();
        fs::write(folder.join(format!("{version}.stderr")), &output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rows = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = rows.lines().collect();
        assert_eq!(rows.len(), expected.len());
        for (row, (line, port)) in rows.iter().zip(&expected).enumerate() {
            let words: Vec<_> = line.split_whitespace().collect();
            assert_eq!(words.len(), port.len() + 1);
            for (field, (native, port)) in words.iter().zip(port).enumerate() {
                if *native != port {
                    mismatches.push(format!(
                        "{version} row{row} field{field}:native={native},port={port}"
                    ));
                }
            }
            if words[0] == "G" && words[3] == "0" && words[4] == "0" {
                assert_eq!(words.last(), Some(&"0"));
            }
        }
    }
    fs::write(
        folder.join("comparison.txt"),
        format!(
            "profiles={profiles}\nrows={}\nmismatches={}\n{}\n",
            expected.len(),
            mismatches.len(),
            mismatches
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    assert!(
        mismatches.is_empty(),
        "{} mismatches: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(8)]
    );
}
