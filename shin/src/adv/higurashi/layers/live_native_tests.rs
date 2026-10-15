//! Integrated live factory/reconstruction comparisons against a distinct
//! executable which runs original20df0/+28/+38/18db0/19578.
use super::*;
use crate::layer::user::higurashi::{HigurashiLayerKind as Kind, Prepared};
use std::{
    collections::BTreeMap,
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone, Debug)]
struct Case {
    op: char,
    kind: i32,
    old: i32,
    back: u32,
    params: [i32; 8],
    load: Option<(i32, u32, u32)>,
}
#[derive(Debug)]
struct Owner {
    kind: i32,
    id: u32,
    seeds: [u32; 4],
    phases: [u32; 4],
    values: [u32; 65],
    busy: [u32; 65],
    derived: Vec<u32>,
}
#[derive(Debug, Default)]
struct Native {
    owners: BTreeMap<char, Option<Owner>>,
    deltas: Vec<[u32; 2]>,
    total: [u32; 2],
    keep: Option<u32>,
}
struct Words<'a>(std::str::SplitWhitespace<'a>);
impl<'a> Words<'a> {
    fn word(&mut self) -> &'a str {
        self.0.next().expect("complete native word")
    }
    fn n(&mut self) -> u32 {
        self.word().parse().expect("native u32")
    }
    fn skip(&mut self, n: usize) {
        for _ in 0..n {
            self.word();
        }
    }
    fn owner(&mut self) -> Option<Owner> {
        let present = self.n();
        let name = self.word();
        if present == 0 {
            assert_eq!(name, "-");
            return None;
        }
        assert_eq!(present, 1);
        self.n();
        let id = self.n();
        self.n();
        let oscillators: [u32; 8] = std::array::from_fn(|_| self.n());
        let mut values = [0; 65];
        let mut busy = [0; 65];
        for i in 0..65 {
            values[i] = self.n();
            self.skip(3);
            busy[i] = self.n();
        }
        assert_eq!(self.word(), "D");
        let kind = self.n() as i32;
        let count = self.n();
        let derived = (0..count).map(|_| self.n()).collect();
        Some(Owner {
            kind,
            id,
            seeds: std::array::from_fn(|i| oscillators[2 * i]),
            phases: std::array::from_fn(|i| oscillators[2 * i + 1]),
            values,
            busy,
            derived,
        })
    }
}
fn native(line: &str, case: &Case) -> Native {
    let mut w = Words(line.split_whitespace());
    assert_eq!(w.word(), case.op.to_string());
    w.skip(match case.op {
        'F' => 3,
        'L' => 6,
        _ => 4,
    });
    let mut out = Native::default();
    while let Some(tag) = w.0.next() {
        match tag {
            "N" => w.skip(4),
            "K" | "F" | "X" | "I" => w.skip(2),
            "A" => w.skip(5),
            "O" | "P" | "C" | "B" => {
                if case.op != 'F' {
                    w.skip(2);
                    assert_eq!(w.word(), "P");
                }
                out.owners.insert(tag.chars().next().unwrap(), w.owner());
            }
            "S" => out.deltas.push([w.n(), w.n()]),
            "J" => {
                if case.op == 'L' {
                    out.keep = Some(w.n());
                    assert_eq!(w.n(), 1);
                } else {
                    assert_eq!(w.n(), 1, "18db0 worker is actually ready");
                    let count = w.n();
                    assert_eq!(count, 64);
                    w.skip(count as usize * 3);
                }
            }
            "R" => {
                assert_eq!(
                    [w.n(), w.n(), w.n()],
                    [1, 1, 0],
                    "actual8e78 completes and restores ADV gate"
                );
            }
            "V" => {
                assert_eq!(w.n(), 0);
            }
            "T" => w.skip(20),
            "M" => w.skip(3),
            "E" => {
                out.total = [w.n(), w.n()];
                w.skip(3);
                assert_eq!(w.n(), 0, "native heap balanced");
                w.skip(1);
            }
            other => panic!("unrecognized native event {other}"),
        }
    }
    out
}
fn params(kind: i32) -> [i32; 8] {
    if kind == 1 {
        [0xf123, 0, 0, 1280, 720, 0, 0, 0]
    } else {
        [0; 8]
    }
}
fn array(p: [i32; 8]) -> UntypedNumberArray {
    (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7])
}
fn fresh(counter: &mut u32) -> [u32; 4] {
    std::array::from_fn(|_| {
        *counter += 1;
        0xabc00000 + *counter
    })
}
fn construct(
    assets: &AssetServer,
    scenario: &HigurashiScenario,
    kind: i32,
    p: [i32; 8],
    old: Option<&HigurashiUserLayer>,
    counter: &mut u32,
) -> Option<HigurashiUserLayer> {
    let prepared = shin_tasks::block_on(Prepared::load(
        assets,
        scenario,
        Kind::from_native(kind).unwrap(),
        array(p),
        0,
    ))
    .unwrap();
    let seeds = (prepared.present() && (!prepared.inherits_base() || old.is_none()))
        .then(|| fresh(counter));
    prepared.construct(old, seeds).unwrap()
}
fn mutate(owner: &mut HigurashiUserLayer) {
    owner.set_owner_id(91);
    for (i, &value) in INITIAL_VALUES.iter().enumerate() {
        owner
            .properties
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to((value + (i % 5) as i32) as f32);
    }
    for (i, o) in owner.properties.oscillators.iter_mut().enumerate() {
        o.phase = f32::from_bits(0x3e800000 + 0x00800000 * i as u32);
    }
    owner
        .properties
        .control(0, 101., Ticks::from_f32(8.), 0)
        .unwrap();
    owner
        .properties
        .control(0, 37., Ticks::from_f32(11.), 0x80)
        .unwrap();
    owner
        .properties
        .tweener_mut(0)
        .unwrap()
        .update(Ticks::from_f32(2.));
}
fn compare(owner: Option<&HigurashiUserLayer>, expected: Option<&Owner>, label: &str) {
    assert_eq!(owner.is_some(), expected.is_some(), "{label}: NULL child");
    let (Some(owner), Some(expected)) = (owner, expected) else {
        return;
    };
    assert_eq!(
        owner.layer_type().native_id(),
        expected.kind,
        "{label}: original RTTI kind"
    );
    assert_eq!(
        owner.owner_id() as u32,
        expected.id,
        "{label}:native owner ID independent of VM record"
    );
    assert_eq!(
        owner.properties.values().map(f32::to_bits),
        expected.values,
        "{label}:65 native current values"
    );
    assert_eq!(
        owner.properties.oscillators.map(|o| o.seed),
        expected.seeds,
        "{label}:four native OS seeds"
    );
    assert_eq!(
        owner.properties.oscillators.map(|o| o.phase.to_bits()),
        expected.phases,
        "{label}:four native oscillator phases"
    );
    assert_eq!(
        std::array::from_fn::<_, 65, _>(|i| u32::from(
            owner.properties.tweener(i).unwrap().is_waiting()
        )),
        expected.busy,
        "{label}:65 native queue/busy states"
    );
    let derived = if let Some(animation) = owner.reference_animation() {
        animation.reference_native_words()
    } else if let Some(effect) = owner.reference_effect() {
        effect.reference_native_words()
    } else {
        Vec::new()
    };
    assert_eq!(
        derived, expected.derived,
        "{label}:original derived clock/vector words"
    );
}

#[test]
#[ignore = "original AArch64 live constructors and integrated real-assets reconstruction; headless GPU, no audio"]
fn higurashi_live_factory_and_reconstruction_match_actual_native_owners() {
    let exe = std::env::var("SHIN_HIGURASHI_LIVE_LAYER_FACTORY_NATIVE")
        .expect("native live factory oracle");
    let (_, _, assets) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut cases = Vec::new();
    for kind in [0, 1, 4, 5, 6, 8, 9, 10] {
        let mut p = params(kind);
        if kind == 4 {
            p[0] = 6;
        } // original ready factory/NULLchild, no missing Sakura art substitution.
        let mut olds = vec![-1, 0, kind];
        olds.sort_unstable();
        olds.dedup();
        for old in olds {
            cases.push(Case {
                op: 'F',
                kind,
                old,
                back: 0,
                params: p,
                load: None,
            });
            for back in 0..3 {
                if kind == 4 && back == 2 {
                    continue;
                } // original CopyFront dereferences NULL; desktop guard is separate.
                cases.push(Case {
                    op: 'R',
                    kind,
                    old,
                    back,
                    params: p,
                    load: None,
                });
            }
        }
    }
    for kind in [0, 1, 4, 5, 6, 8, 9, 10] {
        for (flags, valid, bank) in [
            (0, true, 0),
            (1, true, 0),
            (1, false, 0),
            (2, true, 1),
            (3, true, 1),
            (3, false, 1),
        ] {
            // Invalid Animation reaches native NULL-ID dereference in8e78.
            // Constructor-only validmode0 needs no Sakura texture draw.
            cases.push(Case {
                op: 'L',
                kind,
                old: 0,
                back: bank,
                params: params(kind),
                load: Some((if valid { 0 } else { -1 }, flags, bank)),
            });
        }
    }
    let mut input = String::new();
    for c in &cases {
        input += &format!("{} {} {}", c.op, c.kind, c.old);
        if c.op == 'R' {
            input += &format!(" {}", c.back);
        }
        if let Some((old_vm, flags, bank)) = c.load {
            input += &format!(" {old_vm} {flags} {bank}");
        }
        for p in c.params {
            input += &format!(" {p}");
        }
        input.push('\n');
    }
    let mut process = Command::new(exe)
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = process.stdin.take().unwrap();
    let bytes = input.clone().into_bytes();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes).unwrap());
    let output = process.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_LAYER_FACTORY_CAPTURE") {
        std::fs::create_dir_all(&path).unwrap();
        for (name, bytes) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(std::path::Path::new(&path).join(name), bytes).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), cases.len());
    let mut owners = 0;
    for (i, (case, line)) in cases.iter().zip(text.lines()).enumerate() {
        let expected = native(line, case);
        let label = format!("native profile{i} {case:?}");
        crate::engine_random::with_reference_seed(5489, || {
            let mt_before = crate::engine_random::seed_call_count();
            let mut counter = if case.op != 'F' { 16 } else { 0 };
            let mut old = if case.old < 0 {
                None
            } else {
                construct(
                    &assets,
                    &scenario,
                    case.old,
                    params(case.old),
                    None,
                    &mut counter,
                )
            };
            if let Some(old) = &mut old {
                mutate(old);
            }
            compare(
                old.as_ref(),
                expected.owners[&'O'].as_ref(),
                &format!("{label} old"),
            );
            let before = counter;
            let mt = crate::engine_random::seed_call_count();
            if case.op == 'F' {
                let owner = construct(
                    &assets,
                    &scenario,
                    case.kind,
                    case.params,
                    old.as_ref(),
                    &mut counter,
                );
                compare(owner.as_ref(), expected.owners[&'P'].as_ref(), &label);
                assert_eq!(
                    [
                        counter - before,
                        (crate::engine_random::seed_call_count() - mt) as u32
                    ],
                    expected.deltas[0],
                    "{label}:constructor entropy"
                );
                if let Some(owner) = owner {
                    let copy = owner.native_copy();
                    compare(
                        copy.as_ref(),
                        expected.owners[&'C'].as_ref(),
                        &format!("{label} virtual38"),
                    );
                    assert_eq!(expected.deltas[1], [0, 0]);
                }
            } else if case.op == 'L' {
                let (old_vm, flags, bank) = case.load.unwrap();
                let mut layers = HigurashiLayers::new(|| [0; 4]);
                layers.state.allocate(0).unwrap();
                layers.state.back[0] = bank as u8;
                // Native fixture begins with zeroed runtime records before
                //8a2c, independently of the existing concrete drawable.
                layers.state.records[0][bank as usize].words = [0; RECORD_WORDS];
                layers.state.records[0][bank as usize].words[0] = old_vm;
                layers.owners[0][bank as usize] = old;
                let plan = layers
                    .plan_load(0, case.kind, flags as i32, array(case.params))
                    .unwrap();
                assert_eq!(
                    u32::from(plan.targets[0].1),
                    expected.keep.unwrap(),
                    "{label}:original8a2c keep gate"
                );
                let next_words = *plan.next.records[0][bank as usize].native_words();
                let loaded =
                    shin_tasks::block_on(plan.load_with_setting(&assets, &scenario, 0)).unwrap();
                assert_eq!(
                    counter, before,
                    "{label}:planning/readiness consumes no OS seeds"
                );
                assert_eq!(
                    crate::engine_random::seed_call_count(),
                    mt,
                    "{label}:preparation has no derivedMT"
                );
                layers.publish_load(loaded, || fresh(&mut counter)).unwrap();
                compare(
                    layers.owners[0][bank as usize].as_ref(),
                    expected.owners[&'P'].as_ref(),
                    &format!("{label}:actual8e78 publication"),
                );
                assert_eq!(
                    layers.state.records[0][bank as usize].native_words(),
                    &next_words,
                    "{label}:loaded proposal publishes whole integer record"
                );
                assert_eq!(
                    [
                        counter - before,
                        (crate::engine_random::seed_call_count() - mt) as u32
                    ],
                    expected.deltas[0],
                    "{label}:actual load constructor entropy"
                );
                let children = layers.group_children();
                assert_eq!(children.len(), usize::from(bank == 0));
                if bank == 0 {
                    assert_eq!(
                        children[0].owner_id as u32,
                        expected.owners[&'P'].as_ref().unwrap().id
                    );
                }
            } else {
                let mut outgoing = HigurashiLayers::new(|| [0; 4]);
                outgoing.owners[0][0] = old;
                let mut state = LayerBanks::default();
                for bank in 0..2 {
                    state.records[0][bank].words[0] = case.kind;
                    state.records[0][bank].words[1] = if bank == 0 { 500 } else { 900 };
                    state.records[0][bank].words[2] = -1;
                    state.records[0][bank].words[6..14].copy_from_slice(&case.params);
                    state.records[0][bank].reset_properties();
                }
                state.dirty[0] = [u8::from(case.back != 0), u8::from(case.back == 1)];
                let result =
                    shin_tasks::block_on(
                        outgoing.reconstruct(&state, &assets, &scenario, || fresh(&mut counter)),
                    )
                    .unwrap();
                compare(
                    result.owners[0][0].as_ref(),
                    expected.owners[&'P'].as_ref(),
                    &format!("{label} front"),
                );
                compare(
                    result.owners[0][1].as_ref(),
                    expected.owners[&'B'].as_ref(),
                    &format!("{label} back"),
                );
                assert!(
                    result.owners[1..]
                        .iter()
                        .all(|banks| banks.iter().all(Option::is_none)),
                    "{label}:other31physical slots stay NULL"
                );
                assert_eq!(
                    [
                        counter - before,
                        (crate::engine_random::seed_call_count() - mt) as u32
                    ],
                    expected.deltas[0],
                    "{label}:actual18db0/19578 entropy"
                );
                let children = result.group_children();
                assert_eq!(children.len(), usize::from(expected.owners[&'P'].is_some()));
                if let Some(front) = expected.owners[&'P'].as_ref() {
                    assert_eq!(
                        children[0].owner_id as u32, front.id,
                        "{label}:final native row ID"
                    );
                }
            }
            assert_eq!(
                [
                    counter,
                    (crate::engine_random::seed_call_count() - mt_before) as u32
                ],
                expected.total,
                "{label}:whole original owner stream"
            );
        });
        owners += expected.owners.values().filter(|o| o.is_some()).count();
    }
    println!(
        "Higurashi live factory/reconstruction:{} original profiles/{owners} owner observations; actualproductionPrepared+constructor+virtual38 and all32physical18db0/19578 records,65values/busy,seeds/phases,fullFocus/Rain vectors and effectwords match; NULLSakura factory checked; nativeMovie/temporaries/PIC/BUP transport gaps remain scoped",
        cases.len()
    );
}
