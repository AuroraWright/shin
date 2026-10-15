use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn request(state: &LayerBanks, delay: usize, cancelled: bool) -> Vec<i64> {
    if cancelled {
        return vec![0, 0, 0, 0];
    }
    let mut entries = Vec::new();
    let mut factories = SLOTS;
    let mut live = 0;
    for plan in state.reconstruction_plan() {
        let slot = plan.slot as usize;
        for bank in 0..2 {
            let code = if bank == 0 {
                0
            } else {
                match plan.back {
                    BackReconstruction::Load => {
                        factories += 1;
                        1
                    }
                    BackReconstruction::CopyFront => 2,
                    BackReconstruction::Empty => 3,
                }
            };
            let record = &state.records[slot][bank];
            let present = code <= 1 && (0..=14).contains(&record.layer_type());
            live += usize::from(present);
            entries.extend([
                slot as i64,
                code,
                if present {
                    record.layer_type() as i64
                } else {
                    -1
                },
            ]);
            entries.extend((6..14).map(|i| if present { record.words[i] as i64 } else { 0 }));
        }
    }
    // A failed readiness check restarts the original scan from the beginning.
    let checks = live + delay * live * (live + 1) / 2;
    let mut output = vec![1, 64, factories as i64, checks as i64];
    output.extend(entries);
    output
}

#[test]
#[ignore = "executes original18db0 layer preparation and readiness with checked factory services"]
fn higurashi_layer_reconstruction_matches_native_worker() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut prepare = |state: &LayerBanks, input: &mut String| {
        for delay in 0..=2 {
            input.push_str(&format!("R {delay} 0\n"));
            expected.push(request(state, delay, false));
        }
        input.push_str("R 0 1\n");
        expected.push(request(state, 0, true));
    };
    for profile in 0..64 {
        let mut state = LayerBanks::default();
        input.push_str("N\n");
        prepare(&state, &mut input);
        for n in 0..32 {
            let id = (profile * 13 + n * 29) % 1024;
            let kind = (profile + n) % 17 - 1;
            let p = (id, -id, i32::MIN, i32::MAX, profile, n, 750, 424);
            input.push_str(&format!(
                "L {id} {kind} 0 {} {} {} {} {} {} {} {}\n",
                p.0, p.1, p.2, p.3, p.4, p.5, p.6, p.7
            ));
            state.load(id, kind, 0, p).unwrap();
            match (profile + n) % 4 {
                0 => {}
                1 => {
                    input.push_str(&format!("B {id}\nC {id} 0 123 0 0 0 0 0 0 0\n"));
                    state.start_back(id).unwrap();
                    state.control(id, 0, 123, 0).unwrap();
                }
                2 => {
                    input.push_str(&format!("B {id}\nL {id} 2 1 424 750 -1 9 8 7 6 5\n"));
                    state.start_back(id).unwrap();
                    state.load(id, 2, 1, (424, 750, -1, 9, 8, 7, 6, 5)).unwrap();
                }
                _ => {
                    input.push_str(&format!("B {id}\nU {id} 0\n"));
                    state.start_back(id).unwrap();
                    state.unload(id).unwrap();
                }
            }
        }
        prepare(&state, &mut input);
        input.push_str("S 900 0\nB -4\nC -4 64 -2147483648 0 0 0 0 0 0 0\nW\n");
        state.select(900, 0);
        state.start_back(-4).unwrap();
        state.control(-4, 64, i32::MIN, 0).unwrap();
        state.commit_back();
        prepare(&state, &mut input);
        // Reuse freed slots while retaining untouched back-record words.
        input.push_str("L 1023 0 0 1 2 3 4 5 6 7 8\n");
        state.load(1023, 0, 0, (1, 2, 3, 4, 5, 6, 7, 8)).unwrap();
        prepare(&state, &mut input);
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LAYER_RECONSTRUCTION").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (i, (line, expected)) in text.lines().zip(&expected).enumerate() {
        let actual: Vec<i64> = line
            .strip_prefix("R ")
            .unwrap()
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(&actual, expected, "native preparation {i}");
    }
    println!(
        "Higurashi layer reconstruction:64 profiles,1024 worker runs; all32 slots/front factories/back copy-load-clear/raw parameters/readiness rescans/cancellation pass"
    );
}

#[test]
#[ignore = "offscreen Metal layer reconstruction with real PIC assets; no OS audio"]
fn higurashi_layer_reconstruction_publishes_fresh_targets_and_resources() {
    let (_, _, assets) = crate::adv::higurashi::message_glyphs::tests::gpu::gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut state = LayerBanks::default();
    let p = |id| (id, 0, 0, 0, 0, 0, 0, 0);
    for (id, kind, asset) in [(7, 2, 424), (9, 2, 750), (11, 0, 0), (13, 1, 1000)] {
        state.load(id, kind, 0, p(asset)).unwrap();
    }
    state.start_back(7).unwrap();
    state.control(7, 0, 321, 0).unwrap();
    state.start_back(9).unwrap();
    state.load(9, 2, 0, p(424)).unwrap();
    state.start_back(11).unwrap();
    state.unload(11).unwrap();
    // All target words survive independently of native allocator membership.
    state.records[31][0] = state.records[0][0].clone();
    for (i, record) in state.records.iter_mut().enumerate() {
        for (bank, record) in record.iter_mut().enumerate() {
            for k in 0..65 {
                record.words[PROPERTIES_WORD + k] = if k == 64 {
                    i32::MAX
                } else {
                    (i * 500 + bank * 100 + k) as i32
                };
            }
        }
    }
    state.root[0] = i32::MIN;
    state.screen[1][64] = i32::MAX;
    state.page[5] = 777;
    let before = format!("{state:?}");
    let mut outgoing = HigurashiLayers::new(|| [100; 4]);
    for slot in 0..2 {
        let mut owner = HigurashiUserLayer::null([200 + slot as u32; 4]);
        owner
            .properties
            .control(0, 99.0, Ticks::from_f32(50.0), 0x200)
            .unwrap();
        owner
            .properties
            .control(0, 500.0, Ticks::from_f32(60.0), 0)
            .unwrap();
        for o in &mut owner.properties.oscillators {
            o.phase = 12.25 + slot as f32;
        }
        outgoing.owners[slot][0] = Some(owner);
    }
    for (i, p) in [
        &mut outgoing.root,
        &mut outgoing.screen[0],
        &mut outgoing.page,
    ]
    .into_iter()
    .enumerate()
    {
        p.oscillators[0].phase = 7.5 + i as f32;
        p.control(0, 100.0, Ticks::from_f32(100.0), 0).unwrap();
    }
    let outgoing_before = format!("{:?}", outgoing.owners);
    let mut seed = 0;
    let mut layers = shin_tasks::block_on(outgoing.reconstruct(&state, &assets, &scenario, || {
        seed += 1;
        [seed; 4]
    }))
    .unwrap();
    assert_eq!(
        seed, 4,
        "only front owners without an old owner and separate back loads consume entropy"
    );
    assert_eq!(format!("{:?}", outgoing.owners), outgoing_before);
    assert_eq!(format!("{:?}", layers.state), before);
    assert_eq!(format!("{state:?}"), before);
    assert!(layers.pending_front_ids.is_none());
    assert_eq!(layers.root.values(), state.root.map(|v| v as f32));
    for i in 0..2 {
        assert_eq!(layers.screen[i].values(), state.screen[i].map(|v| v as f32));
    }
    assert_eq!(layers.page.values(), state.page.map(|v| v as f32));
    assert_eq!(layers.root.oscillators[0].phase, 7.5);
    assert_eq!(layers.screen[0].oscillators[0].phase, 8.5);
    assert_eq!(layers.page.oscillators[0].phase, 9.5);
    assert!(layers.owners[0][1].is_some());
    assert!(layers.owners[1][1].is_some());
    assert!(layers.owners[2][1].is_none());
    assert!(layers.owners[3][1].is_none());
    assert!(layers.owners[31][0].is_some());
    assert_eq!(
        layers.owners[0][0]
            .as_ref()
            .unwrap()
            .properties
            .oscillators
            .map(|o| o.seed),
        layers.owners[0][1]
            .as_ref()
            .unwrap()
            .properties
            .oscillators
            .map(|o| o.seed)
    );
    assert_ne!(
        layers.owners[1][0]
            .as_ref()
            .unwrap()
            .properties
            .oscillators
            .map(|o| o.seed),
        layers.owners[1][1]
            .as_ref()
            .unwrap()
            .properties
            .oscillators
            .map(|o| o.seed)
    );
    for (slot, owners) in layers.owners.iter_mut().enumerate() {
        for (bank, owner) in owners.iter_mut().enumerate() {
            if let Some(owner) = owner {
                let expected: [f32; 65] = std::array::from_fn(|i| {
                    state.records[slot][bank].words[PROPERTIES_WORD + i] as f32
                });
                assert_eq!(owner.properties.values(), expected);
                let phase = match (slot, bank) {
                    (0, _) => 12.25,
                    (1, 0) => 13.25,
                    _ => 0.0,
                };
                assert!(
                    owner
                        .properties
                        .oscillators
                        .iter()
                        .all(|o| o.phase == phase)
                );
                owner.properties.advance(Ticks::from_f32(20.0));
                assert_eq!(owner.properties.values(), expected, "no stale tween queues");
            }
        }
    }
    state.records[31][0].words[0] = 7;
    let missing = shin_tasks::block_on(outgoing.reconstruct(&state, &assets, &scenario, || [1; 4])).unwrap();
    assert_eq!(missing.owners[31][0].as_ref().unwrap().layer_type(), crate::layer::user::higurashi::HigurashiLayerKind::Null);
    assert_eq!(missing.state.records[31][0].layer_type(), 7);
    state.records[31][0].words[6] = 0;
    let mut missing_context_seeds = 0;
    let error = shin_tasks::block_on(outgoing.reconstruct(&state, &assets, &scenario, || {
        missing_context_seeds += 1;
        [1; 4]
    })).err().unwrap();
    assert!(error.to_string().contains("actual settings bank"));
    assert_eq!(missing_context_seeds, 0, "Context validation precedes any constructor entropy");
    println!(
        "Higurashi layer reconstruction:real PIC424/PIC750 and Null/Tile owners, independent banks, unallocated slot31, all65 targets/empty tween queues, inherited front/group clocks and fresh separate back clocks, missing Movie metadata Null fallback and actual media context gate pass"
    );
}
