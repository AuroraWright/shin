use super::*;
use crate::layer::{PageLayer, ScreenLayer, user::TileLayer};
use shin_core::vm::command::types::PlaneId;
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use std::{
    fs,
    process::{Command, Stdio},
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod populated_slot_tests;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod retained_murderer_tests;

enum Root {
    Group(LayerGroup),
    Page(PageLayer),
    Screen(ScreenLayer),
}
impl Root {
    fn new(kind: usize, planes: usize) -> Self {
        match kind {
            0 => Self::Group(LayerGroup::new(None)),
            1 => Self::Page(PageLayer::new(planes)),
            2 => Self::Screen(ScreenLayer::new(planes)),
            _ => unreachable!(),
        }
    }
    fn group_mut(&mut self, i: usize) -> &mut LayerGroup {
        match self {
            Self::Group(group) => group,
            Self::Page(page) => page.get_plane_mut(PlaneId::new(i as u8)),
            Self::Screen(screen) => screen.page_layer_mut().get_plane_mut(PlaneId::new(i as u8)),
        }
    }
    fn render_clone(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Group(group) => Self::Group(group.render_clone(ctx)),
            Self::Page(page) => Self::Page(page.render_clone(ctx)),
            Self::Screen(screen) => Self::Screen(screen.render_clone(ctx)),
        }
    }
    fn collect<'a>(&'a self, nodes: &mut Vec<Node<'a>>, planes: usize) {
        match self {
            Self::Group(group) => collect_group(group, nodes),
            Self::Page(page) => collect_page(page, nodes, planes),
            Self::Screen(screen) => {
                nodes.push(Node::new(
                    2,
                    screen.properties(),
                    screen.reference_dissolve_seed(),
                    screen.reference_effect_phases(),
                    None,
                ));
                for page in screen.reference_pages() {
                    collect_page(page, nodes, planes);
                }
            }
        }
    }
}
struct Node<'a> {
    kind: usize,
    properties: NodeProperties<'a>,
    seed: u32,
    phases: [f32; 3],
    geometry: [u32; 8],
}
enum NodeProperties<'a> {
    Borrowed(&'a LayerProperties),
    Leaf(ReferenceLayerLeaf<'a, UserLayer>),
}
impl std::ops::Deref for NodeProperties<'_> {
    type Target = LayerProperties;
    fn deref(&self) -> &LayerProperties {
        match self {
            Self::Borrowed(p) => p,
            Self::Leaf(layer) => layer.properties(),
        }
    }
}
impl<'a> Node<'a> {
    fn new(
        kind: usize,
        properties: &'a LayerProperties,
        seed: u32,
        phases: [f32; 3],
        tile: Option<&TileLayer>,
    ) -> Self {
        Self {
            kind,
            properties: NodeProperties::Borrowed(properties),
            seed,
            phases,
            geometry: tile
                .map(TileLayer::reference_geometry_words)
                .unwrap_or([0; 8]),
        }
    }
}
fn collect_group<'a>(group: &'a LayerGroup, nodes: &mut Vec<Node<'a>>) {
    nodes.push(Node::new(
        0,
        group.properties(),
        group.reference_dissolve_seed(),
        group.reference_effect_phases(),
        None,
    ));
    for layer in group.reference_owner_leaves() {
        let UserLayer::Tile(tile) = &*layer else {
            unreachable!()
        };
        let seed = tile.reference_dissolve_seed();
        let phases = tile.reference_effect_phases();
        let geometry = tile.reference_geometry_words();
        nodes.push(Node {
            kind: 3,
            properties: NodeProperties::Leaf(layer),
            seed,
            phases,
            geometry,
        });
    }
}
fn collect_page<'a>(page: &'a PageLayer, nodes: &mut Vec<Node<'a>>, planes: usize) {
    nodes.push(Node::new(
        1,
        page.properties(),
        page.reference_dissolve_seed(),
        page.reference_effect_phases(),
        None,
    ));
    for i in 0..planes {
        collect_group(page.get_plane(PlaneId::new(i as u8)), nodes);
    }
}
fn snapshot(op: char, rng: usize, roots: &[Option<Root>; 2], planes: usize) -> Vec<String> {
    let mut nodes = Vec::new();
    let mut counts = [0; 2];
    for (i, root) in roots.iter().enumerate() {
        if let Some(root) = root {
            let before = nodes.len();
            root.collect(&mut nodes, planes);
            counts[i] = nodes.len() - before;
        }
    }
    let mut words = vec![
        "G".into(),
        op.to_string(),
        rng.to_string(),
        counts[0].to_string(),
        counts[1].to_string(),
    ];
    for node in &nodes {
        let state = node.properties.reference_copy_words();
        words.extend([
            node.kind.to_string(),
            usize::from(state.is_some()).to_string(),
            format!("{:08x}", node.seed),
        ]);
        words.extend(node.phases.map(|v| format!("{:08x}", v.to_bits())));
        words.extend(node.geometry.map(|v| format!("{v:08x}")));
        words.extend(
            state
                .unwrap_or(vec![0; 108])
                .iter()
                .map(|v| format!("{v:08x}")),
        );
    }
    for a in &nodes {
        for b in &nodes {
            let p = a.properties.initialized_identity();
            words.push(
                usize::from(p.is_some() && p == b.properties.initialized_identity()).to_string(),
            );
        }
    }
    words
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Metal only for actual production RenderCloneCtx;real native populated group/page/screen copies;no audio"]
fn reference_native_populated_layer_copy_and_retirement() {
    run_native_populated_layer_copy(0);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Metal for production RenderCloneCtx; native PAGEBACK,pending copy/replacement/retirement;no audio"]
fn reference_native_pending_page_copy_and_retirement() {
    run_native_populated_layer_copy(1);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Metal for production RenderCloneCtx;real nested ScreenTransitions/default wipes and final owners;no audio"]
fn reference_native_active_screen_copy_and_retirement() {
    run_native_populated_layer_copy(2);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "requires Metal; actual populated captured ScreenTransition copies and final native owner release;no audio"]
fn reference_native_captured_screen_copy_and_retirement() {
    run_native_populated_layer_copy(3);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run_native_populated_layer_copy(workload: usize) {
    let pending = workload == 1;
    let active = workload >= 2;
    let captured = workload == 3;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = std::env::var_os(if captured {
        "SHIN_CAPTURED_SCREEN_COPY_OUTPUT"
    } else if active {
        "SHIN_ACTIVE_SCREEN_COPY_OUTPUT"
    } else if pending {
        "SHIN_PENDING_COPY_OUTPUT"
    } else {
        "SHIN_POPULATED_COPY_OUTPUT"
    })
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|| {
        std::env::temp_dir().join(if captured {
            "shin-native-captured-screen-copy-results"
        } else if active {
            "shin-native-active-screen-copy-results"
        } else if pending {
            "shin-native-pending-copy-results"
        } else {
            "shin-native-populated-copy-results"
        })
    });
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("oracle");
    let compiled = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join(if active {
            "reverse-engineering/native-active-screen-copy.c"
        } else {
            "reverse-engineering/native-populated-layer-copy.c"
        }))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
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
    let resize = shin_render::resize::SurfaceResizeSource::new(
        shin_render::resize::ViewportParams::both(winit::dpi::PhysicalSize::new(16, 16)),
    );
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    let configurations = if active {
        vec![(2, 1), (2, 2)]
    } else {
        vec![(0, 1), (1, 2), (1, 4), (2, 2), (2, 4)]
    };
    for (kind, planes) in configurations {
        if pending && kind != 2 {
            continue;
        }
        let total = if kind == 0 {
            3
        } else {
            1 + usize::from(kind == 2) + 3 * planes
        };
        let mut template = Root::new(kind, planes);
        for g in 0..if kind == 0 { 1 } else { planes } {
            for t in 0..2 {
                template.group_mut(g).add_layer(
                    LayerbankId::new(if t == 0 { 1 } else { 5 }),
                    TileLayer::new(FloatColor4::WHITE, glam::Vec4::ZERO).into(),
                );
            }
        }
        let mut nodes = Vec::new();
        template.collect(&mut nodes, planes);
        let parent_mask = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind != 3)
            .map(|(i, _)| 1 << i)
            .sum::<usize>();
        let child_mask = ((1 << total) - 1) ^ parent_mask;
        let first_tile = nodes.iter().position(|n| n.kind == 3).unwrap();
        let all = (1 << total) - 1;
        let cases = if active {
            vec![(0, 0), (all, 0), (all, 1)]
        } else if pending {
            vec![(0, 0), (all, 0), (parent_mask, 0), (all, 1)]
        } else if kind == 0 {
            vec![(0, 0), (all, 0), (parent_mask, 0), (0, 1), (all, 1)]
        } else {
            vec![
                (0, 0),
                (all, 0),
                (parent_mask, 0),
                (child_mask, 0),
                (all, 1),
                (all, 2),
                (all, 3),
                (0, 3),
            ]
        };
        let cases: Vec<_> = cases
            .into_iter()
            .flat_map(|(mask, partition)| {
                (0..if active {
                    6
                } else if pending {
                    4
                } else {
                    1
                })
                    .flat_map(move |mode| {
                        (0..if pending || active { 2 } else { 1 })
                            .map(move |drop_first| (mask, partition, mode, drop_first))
                    })
            })
            .collect();
        for (mask, partition, mode, drop_first) in cases {
            profiles += 1;
            crate::engine_random::with_reference_seed(5489, || {
                let mut roots: [Option<Root>; 2] = [None, None];
                let mut captures_created = 0;
                let mut commands = vec![format!("N {kind} {planes} {mask:x} {partition}")];
                let pending_count = |fresh| if fresh { 1 + planes } else { 1 + 3 * planes };
                let fresh = mode % 2 == 1;
                let last_node = if active {
                    let page_nodes = 1 + 3 * planes;
                    let depth = 1 + mode / 3;
                    commands.extend([
                        "B 0 0".into(),
                        format!("A 0 1 {:08x}", 30.0f32.to_bits()),
                        format!("V 0 {:08x}", 7.5f32.to_bits()),
                    ]);
                    if depth == 2 {
                        commands.extend([
                            "B 0 0".into(),
                            format!("A 0 1 {:08x}", 45.0f32.to_bits()),
                            format!("V 0 {:08x}", 1.25f32.to_bits()),
                        ]);
                    }
                    let extra = match mode % 3 {
                        0 => 0,
                        1 => {
                            commands.push("B 0 0".into());
                            page_nodes
                        }
                        2 => {
                            commands.push("B 0 1".into());
                            1 + planes
                        }
                        _ => unreachable!(),
                    };
                    1 + (depth + 1) * page_nodes + extra - 1
                } else if pending {
                    commands.push(format!("B 0 {}", usize::from(fresh)));
                    commands.push(format!(
                        "M 0 {} {:08x}",
                        total + pending_count(fresh) - 1,
                        555.0f32.to_bits()
                    ));
                    if mode >= 2 {
                        commands.push(format!("B 0 {}", usize::from(!fresh)));
                    }
                    total + pending_count(if mode >= 2 { !fresh } else { fresh }) - 1
                } else {
                    total - 1
                };
                commands.extend([
                    "C".into(),
                    format!("M 1 {first_tile} {:08x}", 999.0f32.to_bits()),
                    format!("M 0 {last_node} {:08x}", 777.0f32.to_bits()),
                    format!("D {drop_first}"),
                    format!(
                        "M {} {last_node} {:08x}",
                        1 - drop_first,
                        333.0f32.to_bits()
                    ),
                    format!("D {}", 1 - drop_first),
                ]);
                if captured {
                    let index = commands.iter().position(|c| c == "C").unwrap();
                    commands.insert(index, "P 0".into());
                }
                if active {
                    commands.insert(
                        commands.len() - 2,
                        format!("V {} {:08x}", 1 - drop_first, 2.5f32.to_bits()),
                    );
                    let end = commands.len() - 1;
                    commands.splice(
                        end..end,
                        [
                            format!("B {} 1", 1 - drop_first),
                            format!("A {} 0 00000000", 1 - drop_first),
                        ],
                    );
                } else if !pending {
                    // Preserve the already validated ordinary graph request exactly.
                    commands[2] = format!("M 1 {} {:08x}", total - 1, 999.0f32.to_bits());
                    commands[3] = format!("M 0 {first_tile} {:08x}", 777.0f32.to_bits());
                    commands[5] = format!("M 1 {first_tile} {:08x}", 333.0f32.to_bits());
                }
                for command in commands {
                    let fields: Vec<_> = command.split_whitespace().collect();
                    let op = fields[0].chars().next().unwrap();
                    let before = crate::engine_random::seed_call_count();
                    match op {
                        'N' => {
                            let mut graph = Root::new(kind, planes);
                            for g in 0..if kind == 0 { 1 } else { planes } {
                                for t in 0..2 {
                                    let i = (g * 2 + t) as f32;
                                    graph.group_mut(g).add_layer(
                                        LayerbankId::new(if t == 0 { 1 } else { 5 }),
                                        TileLayer::new(
                                            FloatColor4::from_vec4(glam::Vec4::new(
                                                0.125 * i,
                                                0.25,
                                                0.5,
                                                1.0,
                                            )),
                                            glam::Vec4::new(-32.0 * i, 16.0 * i, 96.0, 48.0),
                                        )
                                        .into(),
                                    );
                                }
                            }
                            let mut nodes = Vec::new();
                            graph.collect(&mut nodes, planes);
                            for (n, node) in nodes.iter().enumerate() {
                                if mask & (1 << n) != 0 {
                                    use num_traits::FromPrimitive;
                                    let mut properties = node.properties.share();
                                    for i in 0..90 {
                                        properties
                                            .property_tweener_mut(
                                                LayerProperty::from_u32(i).unwrap(),
                                            )
                                            .fast_forward_to((n * 128 + i as usize) as f32);
                                    }
                                }
                            }
                            let mut groups = Vec::new();
                            for n in nodes.iter().filter(|n| n.kind == 3) {
                                groups.push(&n.properties);
                            }
                            let shared: Vec<_> = groups
                                .iter()
                                .enumerate()
                                .map(|(i, p)| {
                                    if matches!(partition, 1 | 3) && i == 0
                                        || partition == 2 && i % 2 == 0
                                    {
                                        Some(p.share())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            drop(nodes);
                            for g in 0..if kind == 0 { 1 } else { planes } {
                                for t in 0..2 {
                                    let source = match partition {
                                        1 => Some(0),
                                        2 => Some(g * 2),
                                        3 if t == 0 => Some(0),
                                        _ => None,
                                    };
                                    if let Some(source) = source {
                                        graph
                                            .group_mut(g)
                                            .get_layer_mut(LayerbankId::new(if t == 0 {
                                                1
                                            } else {
                                                5
                                            }))
                                            .unwrap()
                                            .set_properties(
                                                shared[source].as_ref().unwrap().share(),
                                            );
                                    }
                                }
                            }
                            roots[0] = Some(graph);
                        }
                        'C' => roots[1] = Some(roots[0].as_ref().unwrap().render_clone(&mut ctx)),
                        'B' => {
                            let Root::Screen(screen) =
                                roots[fields[1].parse::<usize>().unwrap()].as_mut().unwrap()
                            else {
                                unreachable!()
                            };
                            screen.pageback(&mut ctx, fields[2] == "1");
                        }
                        'A' | 'V' => {
                            let Root::Screen(screen) =
                                roots[fields[1].parse::<usize>().unwrap()].as_mut().unwrap()
                            else {
                                unreachable!()
                            };
                            let bits = u32::from_str_radix(fields.last().unwrap(), 16).unwrap();
                            if op == 'A' {
                                screen.apply_transition((fields[2] == "1").then(|| {
                                    crate::wiper::DefaultWiper::new(
                                        shin_core::time::Ticks::from_f32(f32::from_bits(bits)),
                                    )
                                    .into()
                                }));
                            } else {
                                screen.reference_advance_transition_clocks(f32::from_bits(bits));
                            }
                        }
                        'P' => {
                            let Root::Screen(screen) =
                                roots[fields[1].parse::<usize>().unwrap()].as_mut().unwrap()
                            else {
                                unreachable!()
                            };
                            captures_created +=
                                screen.reference_prepare_transition_textures(&device, &resize);
                        }
                        'M' => {
                            let g: usize = fields[1].parse().unwrap();
                            let n: usize = fields[2].parse().unwrap();
                            let mut nodes = Vec::new();
                            roots[g].as_ref().unwrap().collect(&mut nodes, planes);
                            let mut properties = nodes[n].properties.share();
                            properties
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
                    if active {
                        let states: Vec<_> = roots
                            .iter()
                            .map(|root| match root {
                                Some(Root::Screen(screen)) => screen.reference_transition_words(),
                                None => Vec::new(),
                                _ => unreachable!(),
                            })
                            .collect();
                        let mut words = vec![
                            "T".into(),
                            op.to_string(),
                            states[0].len().to_string(),
                            states[1].len().to_string(),
                        ];
                        let live: usize = states
                            .iter()
                            .flatten()
                            .map(|w| (w[5] + w[6]) as usize)
                            .sum();
                        words.extend(
                            states
                                .into_iter()
                                .flatten()
                                .flatten()
                                .map(|v| format!("{v:08x}")),
                        );
                        expected.push(words);
                        if captured {
                            expected.push(vec![
                                "H".into(),
                                op.to_string(),
                                captures_created.to_string(),
                                captures_created.saturating_sub(live).to_string(),
                                live.to_string(),
                            ]);
                        }
                    }
                    input.push_str(&command);
                    input.push('\n');
                }
            });
        }
    }
    let request = folder.join("request.txt");
    fs::write(&request, input).unwrap();
    fs::write(
        folder.join("port.trace"),
        expected
            .iter()
            .map(|x| x.join(" "))
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
        let output = Command::new(&binary)
            .env(
                "SHIN_ACTIVE_SCREEN_CAPTURE",
                if captured { "1" } else { "0" },
            )
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
        let trace = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = trace.lines().collect();
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
