//! Frozen original a350/a560 output compared with the actual captured wait.
use super::*;
use crate::layer::user::higurashi::HigurashiUserLayer;
use std::path::PathBuf;

fn fixture(owner: usize) -> HigurashiUserLayer {
    let mut layer = HigurashiUserLayer::null([1, 2, 3, 4]);
    layer.set_owner_id(owner as i32);
    layer
}
fn properties<'a>(
    layers: &'a HigurashiLayers,
    detached: &'a [Option<HigurashiUserLayer>],
    owner: usize,
) -> &'a HigurashiProperties {
    match owner {
        0 => &layers.root,
        1 => &layers.screen[0],
        2 => &layers.screen[1],
        3 => &layers.page,
        _ => {
            &layers
                .owners
                .iter()
                .flatten()
                .flatten()
                .find(|layer| layer.owner_id() == owner as i32)
                .or_else(|| detached[owner].as_ref())
                .unwrap()
                .properties
        }
    }
}
fn properties_mut<'a>(
    layers: &'a mut HigurashiLayers,
    detached: &'a mut [Option<HigurashiUserLayer>],
    owner: usize,
) -> &'a mut HigurashiProperties {
    match owner {
        0 => &mut layers.root,
        1 => &mut layers.screen[0],
        2 => &mut layers.screen[1],
        3 => &mut layers.page,
        _ => {
            &mut layers
                .owners
                .iter_mut()
                .flatten()
                .flatten()
                .find(|layer| layer.owner_id() == owner as i32)
                .or_else(|| detached[owner].as_mut())
                .unwrap()
                .properties
        }
    }
}
fn dec<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> i64 {
    tokens.next().unwrap().parse().unwrap()
}
fn bits<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> u32 {
    u32::from_str_radix(tokens.next().unwrap(), 16).unwrap()
}

#[test]
#[ignore = "frozen original updated LAYERWAIT constructor/live-poll/tween corpus; CPU only, no GPU/audio/window/save"]
fn higurashi_layerwait_matches_original_captured_targets_and_live_tweens() {
    let reference = PathBuf::from(std::env::var("SHIN_HIGURASHI_LAYERWAIT_REFERENCE").unwrap());
    let input = std::fs::read_to_string(reference.join("input.txt")).unwrap();
    let output = std::fs::read_to_string(reference.join("output.txt")).unwrap();
    let requests: Vec<_> = input.lines().collect();
    let observations: Vec<_> = output.lines().collect();
    assert_eq!(requests.len(), observations.len());
    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    let mut detached: Vec<_> = (0..72).map(|owner| Some(fixture(owner))).collect();
    let mut wait: Option<LayerWait> = None;
    let mut profiles = 0;
    let mut captures = 0;
    let mut live_polls = 0;
    let mut native_queries = 0;
    let mut other_polls = 0;
    for (index, (request, observation)) in requests.iter().zip(&observations).enumerate() {
        let mut tokens = request.split_whitespace();
        let op = tokens.next().unwrap();
        let mut result = observation.split_whitespace();
        assert_eq!(result.next(), Some(op), "native operation {index}");
        let _profile = dec(&mut result);
        let _sequence = dec(&mut result);
        match op {
            "N" => {
                let _profile = dec(&mut tokens);
                layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
                detached = (0..72).map(|owner| Some(fixture(owner))).collect();
                wait = None;
                profiles += 1;
            }
            "A" => {
                layers
                    .state
                    .allocate(dec(&mut tokens).try_into().unwrap())
                    .unwrap();
            }
            "S" => {
                let from = dec(&mut tokens) as i32;
                let to = dec(&mut tokens) as i32;
                layers.state.select(from, to);
            }
            "B" => {
                let logical = dec(&mut tokens) as usize;
                layers.state.back[logical] = dec(&mut tokens).try_into().unwrap();
            }
            "K" => layers.state.screen_back = dec(&mut tokens).try_into().unwrap(),
            "L" => {
                let bank = usize::from(dec(&mut tokens) != 0);
                let slot = dec(&mut tokens) as usize;
                let owner = dec(&mut tokens);
                if let Some(old) = layers.owners[slot][bank].take() {
                    let id = old.owner_id() as usize;
                    assert!(detached[id].is_none());
                    detached[id] = Some(old);
                }
                if owner >= 0 {
                    layers.owners[slot][bank] = Some(detached[owner as usize].take().unwrap());
                }
            }
            "E" => {
                let owner = dec(&mut tokens) as usize;
                let property = dec(&mut tokens) as usize;
                let flags = bits(&mut tokens);
                let value = f32::from_bits(bits(&mut tokens));
                let time = f32::from_bits(bits(&mut tokens));
                properties_mut(&mut layers, &mut detached, owner)
                    .control(
                        property,
                        value,
                        shin_core::time::Ticks::from_f32(time),
                        flags,
                    )
                    .unwrap();
            }
            "T" => {
                let owner = dec(&mut tokens) as usize;
                let property = dec(&mut tokens) as usize;
                let delta = f32::from_bits(bits(&mut tokens));
                properties_mut(&mut layers, &mut detached, owner)
                    .tweener_mut(property)
                    .unwrap()
                    .update(shin_core::time::Ticks::from_f32(delta));
            }
            "F" => {
                let owner = dec(&mut tokens) as usize;
                let property = dec(&mut tokens) as usize;
                properties_mut(&mut layers, &mut detached, owner)
                    .tweener_mut(property)
                    .unwrap()
                    .fast_forward_unless_prohibited();
            }
            "Z" => {
                let owner = dec(&mut tokens) as usize;
                let property = dec(&mut tokens) as usize;
                let value = f32::from_bits(bits(&mut tokens));
                properties_mut(&mut layers, &mut detached, owner)
                    .tweener_mut(property)
                    .unwrap()
                    .fast_forward_to(value);
            }
            "W" => {
                let selector = dec(&mut tokens) as i32;
                let count = dec(&mut tokens);
                let properties: Vec<_> =
                    (0..count.max(0)).map(|_| dec(&mut tokens) as i32).collect();
                let captured = LayerWait::capture(&layers, selector, &properties).unwrap();
                let expected_count = dec(&mut result) as usize;
                let expected_targets: Vec<_> = (0..expected_count)
                    .map(|_| super::super::Target {
                        logical: dec(&mut result).try_into().unwrap(),
                        slot: dec(&mut result).try_into().unwrap(),
                        bank: dec(&mut result).try_into().unwrap(),
                    })
                    .collect();
                let screen = dec(&mut result);
                match &captured.target {
                    WaitTarget::Layers(targets) => assert_eq!(
                        *targets, expected_targets,
                        "native captured coordinates {index}"
                    ),
                    _ => assert!(expected_targets.is_empty()),
                }
                if let WaitTarget::Screen(bank) = captured.target {
                    assert_eq!(i64::from(bank), screen);
                } else {
                    assert_eq!(screen, -1);
                }
                wait = Some(captured);
                captures += 1;
            }
            "P" => {
                let mode = dec(&mut tokens);
                let done = dec(&mut result) != 0;
                if mode == 0 {
                    assert_eq!(
                        wait.as_ref().unwrap().complete(&layers),
                        done,
                        "native live poll {index}: {request}"
                    );
                    live_polls += 1;
                } else {
                    // Original a550/a558 are constant-complete callbacks.
                    // Existing replay support already bypasses visual waits.
                    assert!(done);
                    other_polls += 1;
                }
                let count = dec(&mut result);
                for _ in 0..count {
                    let event = (
                        dec(&mut result),
                        dec(&mut result),
                        dec(&mut result),
                        dec(&mut result),
                        dec(&mut result),
                    );
                    match event {
                        (0, _, _, 0, 0) => {} // Checked native owner-lookup boundary.
                        (1, owner, property, value, busy) => {
                            let tween = properties(&layers, &detached, owner as usize)
                                .tweener(property as usize)
                                .unwrap();
                            assert_eq!(
                                (tween.value().to_bits(), tween.is_waiting()),
                                (value as u32, busy != 0),
                                "native queried property {index}: {event:?}"
                            );
                            native_queries += 1;
                        }
                        _ => panic!("Unreviewed native event {event:?}"),
                    }
                }
            }
            "V" => {
                let owner = dec(&mut tokens) as usize;
                let property = dec(&mut tokens) as usize;
                let tween = properties(&layers, &detached, owner)
                    .tweener(property)
                    .unwrap();
                assert_eq!(
                    (tween.value().to_bits(), tween.is_waiting()),
                    (dec(&mut result) as u32, dec(&mut result) != 0),
                    "native explicit property {index}"
                );
            }
            _ => panic!("Unreviewed native operation {op}"),
        }
        assert!(tokens.next().is_none(), "unconsumed input {request}");
        assert!(result.next().is_none(), "unconsumed output {observation}");
    }
    assert!(profiles >= 30 && captures >= 30 && live_polls >= 100 && native_queries >= 100);
    eprintln!(
        "LAYERWAIT original comparison:{profiles}profiles/{captures}constructors/{live_polls}livepolls/{native_queries}queriedproperties/{other_polls}constantcomplete controls/{}snapshots; real captured coordinates/current owners/queued tweens/protected explicit forwarding; bounded native leaf/property services, no GPU/audio/windows/saves",
        requests.len()
    );
}
