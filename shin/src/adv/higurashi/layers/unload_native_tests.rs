//! Production captured-command/bank adapters against NEW original output.
//! Native leaves are bounded identity/update/copy services. Exact complete
//! destruction order is covered independently by temporal::tail_tests; this
//! test does not substitute Null owners for codec/widget destructor evidence.
use super::*;

const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-layer-unload-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-layer-unload-capture/output.txt"
));
const PROFILES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-layer-unload-capture/profiles.json"
));

#[derive(Debug)]
struct Native {
    targets: Vec<Target>,
    hash: u64,
    groups: [Vec<(i32, u32, i32)>; 2],
}
fn number<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> u32 {
    tokens.next().unwrap().parse().unwrap()
}
fn native(index: usize, op: &str, line: &str) -> Native {
    let mut tokens = line.split_whitespace();
    assert_eq!(tokens.next(), Some("O"));
    assert_eq!(number(&mut tokens) as usize, index);
    assert_eq!(tokens.next(), Some(op));
    let mut out = Native {
        targets: vec![],
        hash: 0,
        groups: [vec![], vec![]],
    };
    while let Some(event) = tokens.next() {
        match event {
            "F" | "K" | "G" | "a" | "W" => {
                let _ = tokens.next().unwrap();
            }
            "U" | "C" => {
                let _ = (tokens.next().unwrap(), tokens.next().unwrap());
            }
            "w" => {
                for _ in 0..32 {
                    let _ = number(&mut tokens);
                }
            }
            "B" => {
                for _ in 0..number(&mut tokens) {
                    out.targets.push(Target {
                        logical: number(&mut tokens).try_into().unwrap(),
                        slot: number(&mut tokens).try_into().unwrap(),
                        bank: number(&mut tokens).try_into().unwrap(),
                    });
                }
            }
            "H" => out.hash = u64::from_str_radix(tokens.next().unwrap(), 16).unwrap(),
            "V" => {
                let bank = number(&mut tokens) as usize;
                for _ in 0..number(&mut tokens) {
                    let key = tokens.next().unwrap().parse().unwrap();
                    let remaining = number(&mut tokens);
                    let cookie = number(&mut tokens);
                    // The explicitly bounded copy callback gives its new
                    // instrumented identity source_cookie+10000. Native base
                    // owner IDs are separately preserved by actual native_copy.
                    out.groups[bank].push((key, remaining, (cookie % 10000) as i32));
                    for _ in 0..3 {
                        let _ = number(&mut tokens);
                    }
                }
            }
            unknown => panic!("Unreviewed native live-unload event {unknown}"),
        }
    }
    out
}
fn owner(label: i32) -> HigurashiUserLayer {
    let mut owner = HigurashiUserLayer::null([1, 2, 3, 4]);
    owner.set_owner_id(label);
    owner
}
fn snapshot(layers: &HigurashiLayers, bank: usize) -> Vec<(i32, u32, i32)> {
    layers
        .reference_group_entries(bank)
        .into_iter()
        .map(|(key, remaining, owner)| (key, remaining, owner.owner_id()))
        .collect()
}
fn previous_snapshot(previous: &HigurashiPreviousFront) -> Vec<(i32, u32, i32)> {
    previous
        .retiring
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.key,
                entry.remaining.to_bits(),
                entry.owner.as_ref().unwrap().owner_id(),
            )
        })
        .chain(
            previous
                .owners
                .iter()
                .enumerate()
                .filter_map(|(slot, owner)| Some((slot as i32, 1, owner.as_ref()?.owner_id()))),
        )
        .collect()
}

#[test]
fn higurashi_live_unload_matches_260_original_constructor_callback_and_bank_snapshots() {
    let profiles: serde_json::Value = serde_json::from_str(PROFILES).unwrap();
    let compare_count = profiles
        .as_array()
        .unwrap()
        .iter()
        .filter(|profile| profile["compare"].as_bool().unwrap_or(true))
        .map(|profile| profile["end"].as_u64().unwrap() as usize)
        .max()
        .unwrap();
    assert_eq!(compare_count, 260);
    let input: Vec<_> = INPUT.lines().collect();
    let output: Vec<_> = OUTPUT.lines().collect();
    assert_eq!(output.len(), input.len() + 1);
    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    let mut command: Option<LayerUnload> = None;
    let mut previous: Option<HigurashiPreviousFront> = None;
    let mut captured = 0;
    let mut completed = 0;
    let mut wipes = 0;
    let mut maximum = 0;
    for (index, request) in input.iter().take(compare_count).enumerate() {
        let mut tokens = request.split_whitespace();
        let op = tokens.next().unwrap();
        let native = native(index, op, output[index]);
        match op {
            "G" => {
                previous = None;
                layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
                command = None;
                for slot in 0..number(&mut tokens) as usize {
                    assert_eq!(layers.state.allocate((slot * 2) as u16), Some(slot as u8));
                    for bank in 0..2 {
                        layers.state.records[slot][bank].words[0] = 1;
                        layers.owners[slot][bank] = Some(owner((100 + bank * 100 + slot) as i32));
                    }
                }
            }
            "S" => {
                let from = tokens.next().unwrap().parse().unwrap();
                let to = tokens.next().unwrap().parse().unwrap();
                layers.state.select(from, to);
            }
            "B" => {
                let logical = number(&mut tokens) as usize;
                layers.state.back[logical] = number(&mut tokens).try_into().unwrap();
            }
            "C" => {
                let selector = tokens.next().unwrap().parse().unwrap();
                let delay = tokens.next().unwrap().parse().unwrap();
                command = Some(LayerUnload::capture(&mut layers.state, selector, delay).unwrap());
                captured += 1;
            }
            "P" => {
                let command = command.as_mut().unwrap();
                command.complete(&mut layers).unwrap();
                let once = [snapshot(&layers, 0), snapshot(&layers, 1)];
                command.complete(&mut layers).unwrap();
                assert_eq!(
                    once,
                    [snapshot(&layers, 0), snapshot(&layers, 1)],
                    "desktop completed callback repeats only as a no-op"
                );
                completed += 1;
            }
            "A" => {
                assert!(
                    layers
                        .state
                        .allocate(number(&mut tokens).try_into().unwrap())
                        .is_some()
                );
            }
            "I" => {
                let bank = number(&mut tokens) as usize;
                let slot = number(&mut tokens) as usize;
                let label = number(&mut tokens) as i32;
                assert_eq!(
                    number(&mut tokens),
                    0,
                    "actualMovieNULL copy is the separate native-only profile"
                );
                layers.owners[slot][bank] = Some(owner(label));
            }
            "U" => {
                let bank = number(&mut tokens) as usize;
                let delta = f32::from_bits(number(&mut tokens));
                layers.advance_group(bank, Ticks::from_f32(delta), None);
            }
            "K" => layers.fast_forward_group(number(&mut tokens) as usize),
            "W" => {
                previous = Some(layers.commit_back());
                wipes += 1;
            }
            unknown => panic!("Unreviewed production live-unload action {unknown}"),
        }
        assert!(tokens.next().is_none());
        assert_eq!(
            replay_snapshot(&layers.state),
            native.hash,
            "original complete integer snapshot {index}/{op}"
        );
        assert_eq!(
            command
                .as_ref()
                .map_or(&[][..], LayerUnload::reference_targets),
            native.targets.as_slice(),
            "constructor-captured physical coordinates {index}/{op}"
        );
        let actual = if op == "W" {
            // Original oracle stops before the separately tracedb919c swap.
            // Actual productioncommit_back publishes it: oldFRONT is outgoing,
            // preparedBACK is incoming, and the next stagingBACK is empty.
            assert!(snapshot(&layers, 1).is_empty());
            [
                previous_snapshot(previous.as_ref().unwrap()),
                snapshot(&layers, 0),
            ]
        } else {
            [snapshot(&layers, 0), snapshot(&layers, 1)]
        };
        assert_eq!(
            actual, native.groups,
            "actual integrated keyed/retiring bank state {index}/{op}"
        );
        maximum = maximum.max(actual.iter().map(Vec::len).max().unwrap());
    }
    assert_eq!(captured, completed);
    assert_eq!(wipes, 4);
    assert_eq!(maximum, 64);
    eprintln!(
        "Live unload:26 original profiles/260 complete VM+bank snapshots;captured physical targets,immediate once-only callback,signed delay,slot reuse,64 owners and4 actual productionWIPE swaps;NULLMovie copy separately native-proven,exact destructor order reused from generic tail oracle;no GPU/audio/windows/saves"
    );
}

#[test]
fn higurashi_complete_group_clear_matches_original_reverse_owner_retirement() {
    use std::{cell::RefCell, rc::Rc};
    struct ObservedOwner(i32, Rc<RefCell<Vec<i32>>>);
    impl Drop for ObservedOwner {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let input: Vec<_> = INPUT.lines().collect();
    let output: Vec<_> = OUTPUT.lines().collect();
    let mut checked = 0;
    let mut retired = 0;
    for (index, action) in input
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with("Z "))
    {
        let bank: usize = action.split_whitespace().nth(1).unwrap().parse().unwrap();
        let preceding_op = input[index - 1].split_whitespace().next().unwrap();
        let before = native(index - 1, preceding_op, output[index - 1]);
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut keyed: Vec<Option<ObservedOwner>> = (0..SLOTS).map(|_| None).collect();
        let mut negative = Vec::new();
        for &(key, remaining, id) in &before.groups[bank] {
            let owner = ObservedOwner(id, events.clone());
            if key < 0 {
                negative.push(super::super::temporal::Entry {
                    key,
                    remaining: f32::from_bits(remaining),
                    owner: Some(owner),
                });
            } else {
                keyed[key as usize] = Some(owner);
            }
        }
        let mut group = super::super::temporal::Group::new(negative);
        clear_group_reverse(keyed.iter_mut(), &mut group);
        let mut expected = Vec::new();
        let words: Vec<_> = output[index].split_whitespace().collect();
        for pair in words.windows(2).filter(|pair| pair[0] == "F") {
            expected.push(pair[1].parse::<i32>().unwrap());
        }
        assert!(
            !expected.is_empty(),
            "original clear must release observable complete owners"
        );
        assert_eq!(
            *events.borrow(),
            expected,
            "actual6b944 complete reverse vector clear at row{index}"
        );
        assert!(keyed.iter().all(Option::is_none) && group.entries().is_empty());
        retired += expected.len();
        checked += 1;
    }
    assert_eq!((checked, retired), (3, 13));
    eprintln!(
        "Complete group clear:3 actual6b944 mixed/keyed/negative vectors;13 exact complete-owner retirements through the production outgoing clear adapter,including active-suffix then reversed negative-prefix order;synthetic leaves,no GPU/audio/window/save"
    );
}
