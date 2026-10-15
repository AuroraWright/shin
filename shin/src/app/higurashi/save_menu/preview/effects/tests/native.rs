//! Compare the production packet/resource owners against the independent
//! original-side matrix (including nested lower captures), not Rust formulas.
use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub struct Leaf {
    pub kind: i32,
    pub id: i32,
    pub params: [i32; 8],
    pub values: [i32; 65],
}
pub struct Case {
    pub extent: (u32, u32),
    pub groups: [[i32; 65]; 2],
    pub leaves: Vec<Leaf>,
}
fn number(tokens: &mut std::str::SplitWhitespace<'_>) -> u32 {
    let value = tokens.next().unwrap().parse::<i64>().unwrap();
    assert!((i32::MIN as i64..=u32::MAX as i64).contains(&value));
    value as u32
}
fn numbers<const N: usize>(tokens: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| number(tokens))
}
pub fn input_case(line: &str) -> Case {
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some("G"));
    let width = number(&mut words);
    let height = number(&mut words);
    let count = number(&mut words);
    let groups =
        std::array::from_fn(|_| std::array::from_fn(|_| words.next().unwrap().parse().unwrap()));
    let mut leaves = vec![];
    for _ in 0..count {
        let kind = words.next().unwrap().parse().unwrap();
        let id = words.next().unwrap().parse().unwrap();
        let params = std::array::from_fn(|_| words.next().unwrap().parse().unwrap());
        let values = std::array::from_fn(|_| words.next().unwrap().parse().unwrap());
        leaves.push(Leaf {
            kind,
            id,
            params,
            values,
        });
    }
    assert!(words.next().is_none());
    Case {
        extent: (width, height),
        groups,
        leaves,
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Draw(Box<[u32; 113]>),
    Owner(#[allow(dead_code, reason = "Retain complete native trace payload for evidence diagnostics.")] Box<[u32; 73]>),
    Resource([u32; 4]),
    Target([u32; 4]),
    Viewport([u32; 2]),
    Restore([u32; 4]),
    Clear([u32; 5]),
    Bind(#[allow(dead_code, reason = "Retain complete native trace payload for evidence diagnostics.")] [u32; 3], Option<[u32; 5]>),
    Stencil(#[allow(dead_code, reason = "Retain complete native trace payload for evidence diagnostics.")] [u32; 5]),
    Render([u32; 4]),
    Lower([u32; 3], Vec<u32>),
    SamplerFilter([u32; 2]),
    SamplerWrap([u32; 3]),
    Drop([u32; 2]),
    Keep(#[allow(dead_code, reason = "Retain complete native trace payload for evidence diagnostics.")] [u32; 2]),
    Copy([u32; 2]),
    InitialClear([u32; 2]),
    Flush(#[allow(dead_code, reason = "Retain complete native trace payload for evidence diagnostics.")] u32),
    Depth([u32; 2]),
}
pub struct Original {
    pub events: Vec<Event>,
    pub draws: Vec<[u32; 113]>,
    pub owners: Vec<[u32; 73]>,
    pub resources: Vec<[u32; 4]>,
    pub targets: Vec<[u32; 4]>,
    pub viewports: Vec<[u32; 2]>,
    pub binds: Vec<(u32, Option<[u32; 5]>)>,
    pub summary: [u32; 11],
}
pub fn output(line: &str, index: usize, case: &Case) -> Original {
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some("G"));
    assert_eq!(
        numbers::<4>(&mut words),
        [
            index as u32,
            case.extent.0,
            case.extent.1,
            case.leaves.len() as u32
        ]
    );
    let mut result = Original {
        events: vec![],
        draws: vec![],
        owners: vec![],
        resources: vec![],
        targets: vec![],
        viewports: vec![],
        binds: vec![],
        summary: [0; 11],
    };
    loop {
        match words.next().unwrap() {
            "D" => {
                let value = numbers(&mut words);
                result.draws.push(value);
                result.events.push(Event::Draw(Box::new(value)));
            }
            "O" => {
                let value = numbers(&mut words);
                result.owners.push(value);
                result.events.push(Event::Owner(Box::new(value)));
            }
            "N" => {
                let value = numbers(&mut words);
                result.resources.push(value);
                result.events.push(Event::Resource(value));
            }
            "T" => {
                let value = numbers(&mut words);
                result.targets.push(value);
                result.events.push(Event::Target(value));
            }
            "V" => {
                let value = numbers(&mut words);
                result.viewports.push(value);
                result.events.push(Event::Viewport(value));
            }
            "B" => {
                let head = numbers::<3>(&mut words);
                let descriptor = (head[2] != 0).then(|| numbers::<5>(&mut words));
                result.binds.push((head[1], descriptor));
                result.events.push(Event::Bind(head, descriptor));
            }
            "L" => {
                let head = numbers::<3>(&mut words);
                let lower = (0..head[2]).map(|_| number(&mut words)).collect();
                result.events.push(Event::Lower(head, lower));
            }
            "Q" => result.events.push(Event::Render(numbers(&mut words))),
            "Z" => result.events.push(Event::Restore(numbers(&mut words))),
            "C" => result.events.push(Event::Clear(numbers(&mut words))),
            "S" => result.events.push(Event::Stencil(numbers(&mut words))),
            "M" => result
                .events
                .push(Event::SamplerFilter(numbers(&mut words))),
            "W" => result.events.push(Event::SamplerWrap(numbers(&mut words))),
            "F" => result.events.push(Event::Drop(numbers(&mut words))),
            "K" => result.events.push(Event::Keep(numbers(&mut words))),
            "X" => result.events.push(Event::Copy(numbers(&mut words))),
            "I" => result.events.push(Event::InitialClear(numbers(&mut words))),
            "H" => result.events.push(Event::Flush(number(&mut words))),
            "Y" => result.events.push(Event::Depth(numbers(&mut words))),
            "E" => {
                result.summary = numbers(&mut words);
                break;
            }
            token => panic!("Unknown original effect event{token}"),
        }
    }
    assert!(words.next().is_none());
    result
}
fn float_words(actual: impl IntoIterator<Item = f32>, expected: &[u32], context: &str) {
    let actual = actual.into_iter().collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (component, (&actual, &word)) in actual.iter().zip(expected).enumerate() {
        let expected = f32::from_bits(word);
        assert!(
            actual == expected || (actual - expected).abs() <= 0.000001 * expected.abs().max(1.),
            "{context}/{component}:Rust{actual}({:x}),original{expected}({word:x})",
            actual.to_bits()
        );
    }
}
fn compare(packet: &Packet, draw: &[u32; 113], context: &str) {
    assert_eq!(
        (packet.program, draw[4], draw[9], draw[10], draw[11]),
        (draw[3], 1, 1, 4, 64),
        "{context}:program/blend/quad"
    );
    assert_eq!(
        draw[16],
        if packet.stencil { 4 } else { 7 },
        "{context}:strict/disabled stencil"
    );
    float_words(packet.projection.to_cols_array(), &draw[25..41], context);
    let expected_sizes = match packet.program {
        23 => [64, 32, 16, 16],
        24 => [64, 16, 32, 0],
        _ => [64, 16, 16, 0],
    };
    // Native GPU state keeps previous uploads in unused slots. Only Blur
    // consumes slot3; ordinary and Raster consume slots0..2.
    let consumed_slots = if packet.program == 23 { 4 } else { 3 };
    assert_eq!(
        &draw[21..21 + consumed_slots],
        &expected_sizes[..consumed_slots],
        "{context}:actual uniform upload sizes"
    );
    if packet.program == 23 {
        float_words(
            packet.distances.into_iter().flat_map(|v| v.to_array()),
            &draw[41..49],
            context,
        );
        float_words(packet.color.to_array(), &draw[57..61], context);
        float_words(packet.weights.to_array(), &draw[73..77], context);
    } else {
        float_words(packet.color.to_array(), &draw[41..45], context);
        float_words(packet.param0.to_array(), &draw[57..61], context);
        if packet.program == 24 {
            float_words(packet.param1.to_array(), &draw[61..65], context);
        }
    }
    float_words(
        packet.vertices.into_iter().flat_map(|v| {
            let position = v.position;
            let texture_position = v.texture_position;
            position
                .to_array()
                .into_iter()
                .chain(texture_position.to_array())
        }),
        &draw[89..105],
        context,
    );
}

pub fn read_corpus() -> (Vec<Case>, Vec<Original>) {
    let input = std::fs::read_to_string(
        std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_EFFECTS_INPUT").unwrap(),
    )
    .unwrap();
    let cases = input.lines().map(input_case).collect::<Vec<_>>();
    assert!(!cases.is_empty());
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_EFFECTS").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_EFFECTS_MODEL_CAPTURE") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("input.txt"), copy).unwrap();
        std::fs::write(dir.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output_text = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output_text.lines().count(), cases.len());
    let native = cases
        .iter()
        .zip(output_text.lines())
        .enumerate()
        .map(|(index, (case, line))| output(line, index, case))
        .collect();
    (cases, native)
}

#[test]
#[ignore = "actual updated factories/lower captures/render/fourOS/zeroMT/resources/retirement matrix for saved5/6/10; CPU only, no audio/windows/user saves"]
fn higurashi_saved_effect_models_match_original_resources_and_packets() {
    let (cases, originals) = read_corpus();
    let mut effects = 0;
    let mut packets = 0;
    let mut properties_count = 0;
    for (index, (case, native)) in cases.iter().zip(&originals).enumerate() {
        assert_eq!(native.summary[2], 16 + 4 * case.leaves.len() as u32);
        assert_eq!(native.summary[3], 0);
        assert_eq!(native.summary[4], native.summary[5]);
        assert_eq!(native.summary[10], 0);
        assert_eq!(native.resources.len() as u32, native.summary[4]);
        assert_eq!(native.owners.len(), case.leaves.len());
        let mut expected_resources = vec![
            (1, 1920, 1080),
            (0, case.extent.0, case.extent.1),
            (1, case.extent.0, case.extent.1),
        ];
        // Native O snapshots are emitted in physical input/clone order. IDs
        // may alias, so an ID search cannot identify the source owner.
        for (leaf, owner) in case.leaves.iter().zip(&native.owners) {
            assert_eq!(
                (owner[0] as i32, owner[1] as i32, owner[2], owner[3]),
                (leaf.id, leaf.kind, 4, 0)
            );
            let seeds = owner[4..8].try_into().unwrap();
            let mut props = HigurashiProperties::from_seeds(seeds);
            for (property, value) in leaf.values.into_iter().enumerate() {
                props
                    .tweener_mut(property)
                    .unwrap()
                    .fast_forward_to(value as f32);
            }
            let original_values: [u32; 65] = owner[8..].try_into().unwrap();
            assert_eq!(
                props.values().map(f32::to_bits),
                original_values,
                "original restored65 targets profile{index}/owner{}",
                leaf.id
            );
            properties_count += 65;
            if leaf.kind == 1 {
                continue;
            }
            effects += 1;
            let view = Prepared::new(leaf.kind).unwrap().clone_refresh();
            assert_eq!(view.phases(), [0.; 2]);
            let plan = view.capture_plan(&props);
            let resource = view.resource_plan();
            assert_eq!(plan.resources, resource);
            for _ in 0..resource.color_count {
                expected_resources.push((0, resource.extent.0, resource.extent.1));
            }
            for _ in 0..resource.depth_count {
                expected_resources.push((1, resource.extent.0, resource.extent.1));
            }
            let relevant = native
                .draws
                .iter()
                .filter(|d| d[0] as i32 == leaf.id && d[1] as i32 == leaf.kind && d[3] != 1)
                .collect::<Vec<_>>();
            let mut expected_draws = 0;
            for event in &native.events {
                match event {
                    Event::Render([id, kind, native_pass, _])
                        if *id as i32 == leaf.id && *kind as i32 == leaf.kind =>
                    {
                        let pass = if *native_pass == 1 {
                            PassKind::Opaque
                        } else {
                            assert_eq!(*native_pass, 0);
                            PassKind::Transparent
                        };
                        expected_draws += usize::from(view.packet(&props, pass).unwrap().is_some());
                    }
                    Event::Lower([id, kind, _], _) if *id as i32 == leaf.id && *kind == 5 => {
                        expected_draws += 1
                    }
                    _ => {}
                }
            }
            assert_eq!(
                relevant.len(),
                expected_draws,
                "Original effect draw/skip profile{index}/owner{}",
                leaf.id
            );
            for draw in relevant {
                let packet = if draw[2] == 2 {
                    view.blur_stage0(&props).unwrap()
                } else {
                    view.packet(
                        &props,
                        if draw[2] == 1 {
                            PassKind::Opaque
                        } else {
                            PassKind::Transparent
                        },
                    )
                    .unwrap()
                    .unwrap()
                };
                compare(
                    &packet,
                    draw,
                    &format!("original effect profile{index}/owner{}", leaf.id),
                );
                packets += 1;
                if packet.native_sampler.is_some() {
                    assert!(
                        native.binds.iter().any(|&(id, descriptor)| id == draw[13]
                            && descriptor == packet.native_sampler),
                        "original Mosaic custom descriptor"
                    );
                }
                if draw[2] == 2 {
                    assert_eq!((draw[7], draw[8]), resource.extent);
                }
            }
            // Native target allocation remains the fixed constructor extent;
            // Mosaic explicitly shrinks only the active lower capture viewport.
            if leaf.kind == 6 {
                assert!(
                    native
                        .viewports
                        .contains(&[plan.viewport.0, plan.viewport.1])
                );
            } else {
                assert!(native.targets.iter().any(|t| (t[2], t[3]) == plan.viewport));
            }
        }
        // Root/screen postprocessing snapshot copies belong to the framebuffer,
        // outside these effect constructors; preserve and count those events.
        let copied = native
            .events
            .iter()
            .filter_map(|event| {
                if let Event::Copy([_, target]) = event {
                    Some(*target)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(copied.len() as u32, native.summary[1]);
        let mut actual_resources = native
            .resources
            .iter()
            .filter(|n| !copied.contains(&n[0]))
            .map(|n| (n[1], n[2], n[3]))
            .collect::<Vec<_>>();
        expected_resources.sort();
        actual_resources.sort();
        assert_eq!(
            expected_resources, actual_resources,
            "original native allocation profile{index}"
        );
    }
    eprintln!(
        "Saved effect model:{} original profiles,{effects} actual effect owners,{packets} native full packets,{properties_count} restored propertywords; exact native allocations/captureviewports/sampler/fourOS/zeroMT/retirement and fresh phases; CPU only",
        cases.len()
    );
}
