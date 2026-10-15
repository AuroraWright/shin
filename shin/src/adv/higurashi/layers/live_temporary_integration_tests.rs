//! Actual live11..14 factories through retained layer publication/reconstruction.
//! Native packets/objects come from the frozen owner oracle; no wrapper/Choice
//! attachment or audio/window/save transport is synthesized by these checks.
use super::*;
use crate::layer::user::higurashi::{HigurashiLayerKind as Kind, Prepared};
use shin_core::format::save::higurashi::SaveData;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

mod gpu;
mod pixels;
const SEEDS: [u32; 4] = [0xabc00001, 0xabc00002, 0xabc00003, 0xabc00004];
#[derive(Clone, Debug)]
struct Case {
    kind: i32,
    old: bool,
    append: bool,
    route: bool,
    pattern: u32,
    params: [i32; 8],
}
#[derive(Debug)]
struct Snapshot {
    kind: i32,
    entropy: [u32; 2],
    base: Vec<u32>,
    private: Vec<u32>,
    bits: Vec<u32>,
}
#[derive(Clone, Debug)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    mask: [u32; 2],
    uniforms: Vec<u32>,
    vertices: Vec<u32>,
}
#[derive(Debug)]
struct Capture {
    draws: Vec<Draw>,
    names: std::collections::BTreeMap<u32, String>,
}
#[derive(Debug)]
enum Original {
    State(Snapshot),
    Render(Capture),
    Drop,
}
fn number(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words(w: &mut std::str::SplitWhitespace<'_>, n: usize) -> Vec<u32> {
    (0..n).map(|_| number(w)).collect()
}
fn parse(line: &str) -> Original {
    let mut w = line.split_whitespace();
    match w.next().unwrap() {
        "X" => {
            number(&mut w);
            assert!(w.next().is_none());
            Original::Drop
        }
        "O" => {
            w.next().unwrap();
            number(&mut w);
            let kind = number(&mut w) as i32;
            let entropy = [number(&mut w), number(&mut w)];
            assert_eq!(w.next(), Some("H"));
            let base = words(&mut w, 138);
            assert_eq!(w.next(), Some("D"));
            let mut private = words(
                &mut w,
                match kind {
                    11 => 4,
                    12 => 7,
                    13 => 9,
                    14 => 6,
                    _ => panic!("Kind{kind}"),
                },
            );
            let (count, stride) = match kind {
                11 => (private[3], 7),
                12 => (private[6], 5),
                13 => (0, 0),
                14 => (private[5], 3),
                _ => unreachable!(),
            };
            if kind == 11 {
                private.extend(words(&mut w, 16));
            }
            private.extend(words(&mut w, count as usize * stride));
            assert_eq!(w.next(), Some("G"));
            let bits = words(&mut w, 20);
            assert_eq!(w.next(), Some("S"));
            let cues = number(&mut w) as usize;
            words(&mut w, cues);
            assert_eq!(w.next(), Some("B"));
            words(&mut w, 2);
            assert!(w.next().is_none());
            Original::State(Snapshot {
                kind,
                entropy,
                base,
                private,
                bits,
            })
        }
        "R" => {
            number(&mut w);
            let mut draws = vec![];
            loop {
                match w.next().unwrap() {
                    "P" => {
                        let program = number(&mut w);
                        let blend = number(&mut w);
                        let texture = number(&mut w);
                        let mask = [number(&mut w), number(&mut w)];
                        let uniforms = words(&mut w, 48);
                        let vertices = words(&mut w, if program == 1 { 16 } else { 24 });
                        draws.push(Draw {
                            program,
                            blend,
                            texture,
                            mask,
                            uniforms,
                            vertices,
                        });
                    }
                    "E" => {
                        assert_eq!(number(&mut w) as usize, draws.len());
                        break;
                    }
                    v => panic!("Packet token{v}"),
                }
            }
            assert_eq!(w.next(), Some("K"));
            let n = number(&mut w);
            let names = (0..n)
                .map(|_| (number(&mut w), w.next().unwrap().to_owned()))
                .collect();
            assert!(w.next().is_none());
            Original::Render(Capture { draws, names })
        }
        v => panic!("Original event{v}"),
    }
}
fn native(input: String, label: &str) -> Vec<Original> {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_TEMPORARY").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_TEMPORARY_INTEGRATION_CAPTURE") {
        let path = std::path::Path::new(&path).join(label);
        std::fs::create_dir_all(&path).unwrap();
        for (name, data) in [
            ("input.txt", copy.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(path.join(name), data).unwrap();
        }
    }
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(parse)
        .collect()
}
fn input_new(input: &mut String, c: &Case) {
    write!(
        input,
        "N {} {} {} {} {}",
        c.kind,
        if c.old { 0 } else { 99 },
        u32::from(c.append),
        u32::from(c.route),
        c.pattern
    )
    .unwrap();
    for p in c.params {
        write!(input, " {p}").unwrap();
    }
    input.push('\n');
}
fn array(p: [i32; 8]) -> UntypedNumberArray {
    (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7])
}
fn params(kind: i32) -> [i32; 8] {
    match kind {
        11 => [7, 0, 0, 0, 0, 0, 0, 0],
        12 => [0, 3, 2, 3, 2, 0, 0, 0],
        13 => [7, 0, 0, 0, 0, 0, 0, 0],
        14 => [3, 0, 2, 33, 0, 0, 0, 0],
        _ => unreachable!(),
    }
}
fn initial_bits(pattern: u32) -> Vec<u32> {
    let mut b = vec![0; 20];
    for id in 0..315 {
        let status = if pattern == 4 {
            id % 4
        } else {
            pattern as usize
        };
        for lane in 0..2 {
            if status & (1 << lane) != 0 {
                let bit = 2 * id + lane;
                b[bit / 32] |= 1 << (bit % 32);
            }
        }
    }
    b
}
fn settings(c: &Case) -> SaveData {
    let mut s = SaveData::default();
    s.save_vectors.extra_words_0 = initial_bits(c.pattern);
    s.settings.common.v14_showroutenavi = c.route;
    s
}
fn expected_bits(c: &Case) -> Vec<u32> {
    let mut b = initial_bits(c.pattern);
    if c.kind == 12 {
        for &id in &c.params[2..2 + c.params[1] as usize] {
            let bit = 2 * id as usize;
            b[bit / 32] |= 1 << (bit % 32);
        }
    }
    b
}
fn base(properties: &HigurashiProperties) -> Vec<u32> {
    let mut w = properties.values().map(f32::to_bits).to_vec();
    for o in properties.oscillators {
        w.extend([o.seed, o.phase.to_bits()]);
    }
    for i in 0..65 {
        w.push(u32::from(properties.tweener(i).unwrap().is_waiting()));
    }
    w
}
fn compare(layer: &HigurashiUserLayer, original: &Snapshot, label: &str) {
    assert_eq!(
        layer.layer_type().native_id(),
        original.kind,
        "{label}:native kind"
    );
    assert_eq!(
        base(&layer.properties),
        original.base,
        "{label}:all65 values/busy and oscillator words"
    );
    assert_eq!(
        layer
            .reference_temporary()
            .unwrap()
            .reference_native_words(),
        original.private,
        "{label}:native private state"
    );
}
fn snapshot(row: Original) -> Snapshot {
    let Original::State(s) = row else {
        panic!("Expected original snapshot")
    };
    s
}
fn assert_bits(settings: &SaveData, original: &Snapshot, c: &Case, label: &str) {
    assert_eq!(
        settings.save_vectors.extra_words_0, original.bits,
        "{label}:actual acquired/read/padding bank"
    );
    assert_eq!(settings.save_vectors.extra_words_0, expected_bits(c));
    for (after, before) in settings
        .save_vectors
        .extra_words_0
        .iter()
        .zip(initial_bits(c.pattern))
    {
        assert_eq!(
            after & 0xaaaa_aaaa,
            before & 0xaaaa_aaaa,
            "{label}:read bits untouched"
        );
    }
    assert_eq!(
        settings.save_vectors.extra_words_0.len(),
        20,
        "Existing bitmap padding retained"
    );
}
fn old_layer() -> HigurashiUserLayer {
    let mut p = HigurashiUserLayer::null(SEEDS);
    p.set_owner_id(91);
    p.properties
        .control(0, 100., Ticks::from_f32(10.), 0)
        .unwrap();
    p
}
fn scene() -> HigurashiLayers {
    HigurashiLayers::new(|| [0x4312; 4])
}

#[test]
#[ignore = "real11..14 assets through actual retained publication/reconstruction, original CPU snapshots; pure GPU asset transport,no audio/windows/user saves"]
fn higurashi_live_temporary_publication_and_reconstruction_match_original() {
    let (_, _, assets) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let append = assets.contains("/append.txt");
    let profiles = (11..=14)
        .flat_map(|kind| {
            [
                (0, true, 0),
                (1, true, 0),
                (1, false, 0),
                (2, true, 1),
                (3, true, 1),
                (3, false, 1),
            ]
            .into_iter()
            .map(move |(flags, valid, bank)| {
                (
                    Case {
                        kind,
                        old: flags & 1 != 0 && valid,
                        append,
                        route: flags & 1 != 0,
                        pattern: if kind == 12 {
                            if valid { 2 } else { 4 }
                        } else {
                            (kind - 11) as u32
                        },
                        params: params(kind),
                    },
                    flags,
                    valid,
                    bank,
                )
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(profiles.len(), 24);
    let mut input = String::new();
    for (c, _, _, _) in &profiles {
        input_new(&mut input, c);
        input.push_str("P 0\nU 0 7.25\nC 0 1\nU 1 9.5\nF 1\nV 1\nD 0\nD 1\n");
    }
    let original = native(input, "publication");
    let mut original = original.into_iter();
    let mt = crate::engine_random::seed_call_count();
    let mut owners = 0;
    for (index, (c, flags, valid, bank)) in profiles.iter().enumerate() {
        let label = format!("publication{index} {c:?} flags{flags}/bank{bank}");
        let mut layers = scene();
        let logical = 37;
        let slot = layers.state.allocate(logical).unwrap();
        let target = Target {
            logical,
            slot,
            bank: *bank,
        };
        layers.state.back[logical as usize] = *bank;
        let record = &mut layers.state.records[slot as usize][*bank as usize];
        record.words[0] = if *valid { 0 } else { -1 };
        record.words[1] = 91;
        record.words[2] = 137;
        record.reset_properties();
        record.words[PROPERTIES_WORD] = 19;
        layers.owners[slot as usize][*bank as usize] = Some(old_layer());
        let old_properties = base(&layers.layer(target).unwrap().properties);
        let mut settings = settings(c);
        let untouched = settings.clone();
        let mut os = 0;
        let plan = layers
            .plan_load(logical as i32, c.kind, *flags, array(c.params))
            .unwrap();
        assert!(!plan.immediate());
        assert_eq!(
            base(&layers.layer(target).unwrap().properties),
            old_properties,
            "{label}:plan keeps old owner"
        );
        let loaded = shin_tasks::block_on(plan.load_with_setting(&assets, &scenario, 0)).unwrap();
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::to_value(&untouched).unwrap(),
            "{label}:ready assets have no global mutation"
        );
        assert_eq!(os, 0);
        assert_eq!(crate::engine_random::seed_call_count(), mt);
        // The outgoing owner advances during readiness; its state is sampled at publication.
        layers
            .layer_mut(target)
            .unwrap()
            .advance(Ticks::from_f32(2.5), None);
        layers
            .publish_load_with_settings(
                loaded,
                || {
                    os += 4;
                    SEEDS
                },
                Some(&mut settings),
            )
            .unwrap();
        let layer = layers.layer(target).unwrap();
        let start = snapshot(original.next().unwrap());
        assert_eq!(start.entropy, [if c.old { 0 } else { 4 }, 0]);
        assert_eq!(os, start.entropy[0]);
        compare(layer, &start, &label);
        assert_bits(&settings, &start, c, &label);
        assert_eq!(
            layer.owner_id(),
            if c.old { 91 } else { logical as i32 },
            "{label}:native8e78 retained/fresh owner ID"
        );
        let record = layers.state.record(target);
        assert_eq!(record.layer_type(), c.kind);
        assert_eq!(&record.words[6..14], &c.params);
        assert_eq!(record.words[1], if c.old { 91 } else { logical as i32 });
        assert_eq!(record.words[2], if c.old { 137 } else { -1 });
        assert_eq!(record.property(0), Some(if c.old { 19 } else { 0 }));
        assert_eq!(
            layers.state.dirty[slot as usize],
            if *bank == 1 { [1, 1] } else { [0, 0] }
        );
        let frozen = base(&layer.properties);
        let mut wrapper = layer.reference_temporary().unwrap().retain();
        wrapper.poll_phase();
        compare(layer, &snapshot(original.next().unwrap()), &label);
        layers
            .layer_mut(target)
            .unwrap()
            .advance(Ticks::from_f32(7.25), None);
        let layer = layers.layer(target).unwrap();
        compare(layer, &snapshot(original.next().unwrap()), &label);
        assert_eq!(base(&layer.properties), frozen);
        let before = layer
            .reference_temporary()
            .unwrap()
            .reference_native_words();
        let mut copy = layer.native_copy().unwrap();
        let cloned = snapshot(original.next().unwrap());
        assert_eq!(cloned.entropy, [0, 0]);
        compare(&copy, &cloned, &label);
        copy.advance(Ticks::from_f32(9.5), None);
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        assert_eq!(
            layer
                .reference_temporary()
                .unwrap()
                .reference_native_words(),
            before,
            "{label}:independent private copy"
        );
        copy.fast_forward();
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        assert_eq!(
            base(&copy.properties),
            frozen,
            "{label}:fast-forward leaves inherited base frozen"
        );
        copy.reconstruction_refresh().unwrap();
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        assert_bits(&settings, &start, c, &label);
        assert!(matches!(original.next(), Some(Original::Drop)));
        assert!(matches!(original.next(), Some(Original::Drop)));
        owners += 2;
    }
    assert!(original.next().is_none());
    assert_eq!(crate::engine_random::seed_call_count(), mt);

    // All preparation is asset-only; front clone and separate back clone occur
    // in the current main-poll settings context. Each case visits all32 slots.
    let profiles = (11..=14)
        .flat_map(|kind| {
            [false, true].into_iter().flat_map(move |old| {
                (0..3).map(move |back| {
                    (
                        Case {
                            kind,
                            old,
                            append,
                            route: old,
                            pattern: 2,
                            params: params(kind),
                        },
                        back,
                    )
                })
            })
        })
        .collect::<Vec<_>>();
    let mut input = String::new();
    for (c, back) in &profiles {
        input_new(&mut input, c);
        if *back == 1 {
            input.push_str("C 0 1\nV 0\nV 1\nD 0\nD 1\n");
        } else {
            input.push_str("V 0\nD 0\n");
        }
        if *back == 2 {
            let mut b = c.clone();
            b.old = false;
            if b.kind == 12 {
                b.pattern = 3;
            }
            input_new(&mut input, &b);
            input.push_str("V 0\nD 0\n");
        }
    }
    let mut original = native(input, "reconstruction").into_iter();
    let mut reconstructed = 0;
    for (index, (c, back)) in profiles.iter().enumerate() {
        let label = format!("reconstruction{index} {c:?}/back{back}");
        let mut outgoing = scene();
        let mut state = LayerBanks::default();
        let logical = 53;
        let slot = state.allocate(logical).unwrap();
        let target = Target {
            logical,
            slot,
            bank: 0,
        };
        if c.old {
            let mut old = old_layer();
            old.advance(Ticks::from_f32(2.5), None);
            outgoing.owners[slot as usize][0] = Some(old);
        }
        for bank in 0..2 {
            let record = &mut state.records[slot as usize][bank];
            record.words[0] = c.kind;
            record.words[1] = 71 + bank as i32;
            record.words[2] = -1;
            record.words[6..14].copy_from_slice(&c.params);
            record.reset_properties();
            record.words[PROPERTIES_WORD] = 17 + bank as i32;
            record.words[PROPERTIES_WORD + 9] = 625;
        }
        state.dirty[slot as usize] = match back {
            0 => [0, 0],
            1 => [1, 0],
            2 => [1, 1],
            _ => unreachable!(),
        };
        let mut settings = settings(c);
        let before = settings.clone();
        let ready = shin_tasks::block_on(HigurashiLayers::prepare_reconstruction(
            &state, &assets, &scenario, 0,
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert_eq!(crate::engine_random::seed_call_count(), mt);
        let mut os = 0;
        let restored = outgoing
            .construct_reconstruction(
                ready,
                || {
                    os += 4;
                    SEEDS
                },
                Some(&mut settings),
            )
            .unwrap();
        let fresh = snapshot(original.next().unwrap());
        if *back == 1 {
            let copied = snapshot(original.next().unwrap());
            assert_eq!(copied.entropy, [0, 0]);
        }
        let refreshed = snapshot(original.next().unwrap());
        let back_refreshed = (*back == 1).then(|| snapshot(original.next().unwrap()));
        assert!(matches!(original.next(), Some(Original::Drop)));
        if *back == 1 {
            assert!(matches!(original.next(), Some(Original::Drop)));
        }
        assert_eq!(
            os,
            (if c.old { 0 } else { 4 }) + if *back == 2 { 4 } else { 0 }
        );
        assert_eq!(fresh.entropy, [if c.old { 0 } else { 4 }, 0]);
        let front = restored.layer(target).unwrap();
        assert_eq!(
            front
                .reference_temporary()
                .unwrap()
                .reference_native_words(),
            refreshed.private,
            "{label}"
        );
        assert_eq!(front.owner_id(), 71);
        assert_eq!(&base(&front.properties)[65..73], &fresh.base[65..73]);
        assert_eq!(front.properties.values()[0], 17.);
        assert_eq!(front.properties.values()[9], 625.);
        assert!(base(&front.properties)[73..].iter().all(|&w| w == 0));
        assert_bits(&settings, &fresh, c, &label);
        for slot in 1..SLOTS {
            assert!(
                restored.owners[slot].iter().all(Option::is_none),
                "{label}:all32 slots visited"
            );
        }
        match back {
            0 => assert!(restored.owners[0][1].is_none()),
            1 => {
                let b = restored.owners[0][1].as_ref().unwrap();
                assert_eq!(b.owner_id(), 72);
                assert_eq!(
                    b.reference_temporary().unwrap().reference_native_words(),
                    back_refreshed.as_ref().unwrap().private,
                    "{label}:original independent back copy and refresh"
                );
                assert_eq!(&base(&b.properties)[65..73], &fresh.base[65..73]);
                assert_eq!(b.properties.values()[0], 18.);
            }
            2 => {
                let _initial = snapshot(original.next().unwrap());
                let b_native = snapshot(original.next().unwrap());
                assert!(matches!(original.next(), Some(Original::Drop)));
                let b = restored.owners[0][1].as_ref().unwrap();
                assert_eq!(b.owner_id(), 72);
                assert_eq!(
                    b.reference_temporary().unwrap().reference_native_words(),
                    b_native.private,
                    "{label}"
                );
                assert_eq!(&base(&b.properties)[65..73], &b_native.base[65..73]);
            }
            _ => unreachable!(),
        }
        let mut front_copy = front.native_copy().unwrap();
        let before = front
            .reference_temporary()
            .unwrap()
            .reference_native_words();
        front_copy.advance(Ticks::from_f32(11.25), None);
        assert_eq!(
            front
                .reference_temporary()
                .unwrap()
                .reference_native_words(),
            before
        );
        assert_eq!(front_copy.properties.values(), front.properties.values());
        assert_eq!(settings.save_vectors.extra_words_0, expected_bits(c));
        reconstructed += 1;
    }
    assert!(original.next().is_none());
    assert_eq!(crate::engine_random::seed_call_count(), mt);
    eprintln!(
        "Live temporary integrated real-assets:24 publication profiles/{owners} independent owners and{reconstructed} reconstruction profiles; native private/base words, actual settings/order, ID/banks/flags, readiness/no entropy, private-only advancement and independent copy/refresh match. Command wrappers/Choice child remain outside this check."
    );
}
