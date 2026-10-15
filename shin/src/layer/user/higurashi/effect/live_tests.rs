use super::*;
use crate::layer::user::higurashi::{
    Drawable, HigurashiLayerKind as Kind, HigurashiUserLayer, Prepared as Factory,
};
use shin_core::time::Ticks;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn old_owner(kind: i32) -> Option<HigurashiUserLayer> {
    if kind == 0 {
        return None;
    }
    let seeds = [1, 2, 3, 4].map(|n| 0xabc00000 + n);
    let mut owner = if kind == 1 {
        HigurashiUserLayer::tile(FloatColor4::from_vec4(Vec4::ZERO), Vec4::ZERO, seeds)
    } else {
        Factory::Effect(Kind::from_native(kind).unwrap())
            .construct(None, Some(seeds))
            .unwrap()
            .unwrap()
    };
    for i in 0..65 {
        let v = if i == 9 {
            777
        } else if i % 7 == 0 {
            1234 + i as i32
        } else {
            0
        };
        owner
            .properties
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to(v as f32);
    }
    for (i, o) in owner.properties.oscillators.iter_mut().enumerate() {
        o.phase = f32::from_bits(0x3e800000 + i as u32 * 0x00800000);
    }
    if let Drawable::Effect(effect) = &mut owner.drawable {
        if kind == 5 {
            effect.view.weights =
                std::array::from_fn(|i| f32::from_bits(0x3e000000 + i as u32 * 0x00400000));
        }
    }
    Some(owner)
}
fn snapshot(owner: &HigurashiUserLayer) -> Vec<u32> {
    let mut v = vec![owner.layer_type().native_id() as u32];
    v.extend(owner.properties.oscillators.map(|o| o.seed));
    v.extend(owner.properties.values().map(f32::to_bits));
    v.extend(owner.properties.oscillators.map(|o| o.phase.to_bits()));
    let (phases, weights) = if let Drawable::Effect(e) = &owner.drawable {
        (
            e.view.phases,
            if e.kind() == 5 {
                e.view.weights
            } else {
                [0.; 4]
            },
        )
    } else {
        ([0.; 2], [0.; 4])
    };
    v.extend(phases.map(f32::to_bits));
    v.extend(weights.map(f32::to_bits));
    v
}
fn native(line: &str) -> (Vec<Vec<u32>>, Vec<[u32; 2]>, Vec<[u32; 4]>, [u32; 5]) {
    let tokens: Vec<_> = line.split_whitespace().collect();
    assert_eq!(tokens[0], "E");
    let mut at = 4;
    let mut snapshots = Vec::new();
    let mut samples = Vec::new();
    let mut resources = Vec::new();
    let mut end = [0; 5];
    while at < tokens.len() {
        let token = tokens[at];
        at += 1;
        let count = match token {
            "N" => 4,
            "K" | "F" => 2,
            "I" => 2,
            "S" => 2,
            "P" => 84,
            "E" => 5,
            _ => panic!("unknown native event {token}"),
        };
        let v: Vec<u32> = tokens[at..at + count]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        at += count;
        match token {
            "P" => {
                snapshots.push(v[..76].iter().chain(&v[80..84]).copied().collect());
                resources.push(v[76..80].try_into().unwrap());
            }
            "S" => samples.push(v.try_into().unwrap()),
            "E" => end = v.try_into().unwrap(),
            _ => {}
        }
    }
    (snapshots, samples, resources, end)
}

#[test]
#[ignore = "executes original live effects factory/old owner/update/copy/phase arithmetic; CPU only"]
fn higurashi_live_effects_match_original_construction_clocks_and_copies() {
    let phases: [f32; 16] = [
        0., -0., 0.5, 0.875, -0.5, -1., 999.75, 1000., -1000., 123456.25, -123456.25, 16777216.,
        -16777216., 3.25, 42.5, -42.5,
    ];
    let deltas = [0., -0., 1., -1., 0.25, 50., 1000., -2000.];
    let periods = [
        (0, 0),
        (32, -48),
        (-1, 1),
        (1, 3),
        (1000, 777),
        (-2000, 2000),
        (i32::MIN, i32::MAX),
        (17, -29),
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut relations = Vec::new();
    for kind in [5, 6, 10] {
        for old_kind in [0, 1, 5, 6, 10] {
            for n in 0..phases.len() {
                let phase = [phases[n], phases[(n + 3) % phases.len()]];
                let delta: f32 = deltas[n % deltas.len()];
                let period = periods[n % periods.len()];
                let steps = n % 9;
                input.push_str(&format!(
                    "E {kind} {old_kind} {} {} {} {} {} {steps}\n",
                    phase[0].to_bits(),
                    phase[1].to_bits(),
                    delta.to_bits(),
                    period.0,
                    period.1
                ));
                let mut old = old_owner(old_kind);
                if let Some(HigurashiUserLayer {
                    drawable: Drawable::Effect(e),
                    ..
                }) = &mut old
                {
                    if old_kind == 10 {
                        e.view.phases = phase;
                    }
                }
                let mut states = old.as_ref().map(snapshot).into_iter().collect::<Vec<_>>();
                let fresh = kind == 6 || old.is_none();
                let start = if old.is_some() { 5 } else { 1 };
                let seeds = std::array::from_fn(|i| 0xabc00000 + start + i as u32);
                let mut owner = Factory::Effect(Kind::from_native(kind).unwrap())
                    .construct(old.as_ref(), fresh.then_some(seeds))
                    .unwrap()
                    .unwrap();
                states.push(snapshot(&owner));
                for (index, value) in [(56, period.0), (59, period.1)] {
                    owner
                        .properties
                        .tweener_mut(index)
                        .unwrap()
                        .fast_forward_to(value as f32);
                }
                if let Drawable::Effect(e) = &mut owner.drawable {
                    if kind == 10 {
                        e.view.phases = phase;
                    }
                }
                for _ in 0..steps {
                    owner.advance(Ticks::from_f32(delta), None);
                }
                states.push(snapshot(&owner));
                let copy = owner.native_copy().unwrap();
                states.push(snapshot(&copy));
                if let (Drawable::Effect(a), Drawable::Effect(b)) =
                    (&owner.drawable, &copy.drawable)
                {
                    assert_eq!(std::sync::Arc::ptr_eq(&a.cache, &b.cache), kind != 6);
                }
                expected.push(states);
                relations.push((fresh, kind, old.is_some()));
            }
        }
    }
    let mut raw = Vec::new();
    for n in 0..2048u32 {
        let a = phases[n as usize % phases.len()] * (1. + (n % 17) as f32);
        let b = deltas[(n as usize / phases.len()) % deltas.len()] * (1. + (n % 23) as f32);
        input.push_str(&format!("W {} {}\n", a.to_bits(), b.to_bits()));
        raw.push(wrap_phase(a, b).to_bits());
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_EFFECTS").unwrap())
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
    let captured_input = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), expected.len() + raw.len());
    let mut words = 0;
    for (i, states) in expected.iter().enumerate() {
        let (actual, samples, resources, end) = native(lines[i]);
        assert_eq!(&actual, states, "native live owner profile{i}");
        words += states.iter().map(Vec::len).sum::<usize>();
        let (fresh, kind, old) = relations[i];
        assert_eq!(samples, [[if fresh { 4 } else { 0 }, 0], [0, 0]]);
        assert_eq!(end[1], 0);
        assert_eq!(end[2], end[3]);
        assert_eq!(end[4], 0);
        let current = if old { 1 } else { 0 };
        let copy = resources.last().unwrap();
        assert_eq!(resources[current] == *copy, kind != 6);
        if old && resources[0][0] != 0 {
            assert_ne!(
                resources[0][0], resources[current][0],
                "replacement must get fresh resources"
            );
        }
    }
    // Separate raw helper rows follow all owner profiles.
    for (i, word) in raw.iter().enumerate() {
        assert_eq!(
            lines[expected.len() + i]
                .strip_prefix("W ")
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            *word,
            "native raw phase{i}"
        );
    }
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_LIVE_EFFECTS_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/input.txt"), captured_input).unwrap();
        std::fs::write(format!("{dir}/output.txt"), text).unwrap();
        std::fs::write(format!("{dir}/stderr.txt"), output.stderr).unwrap();
    }
    println!(
        "Live effects:{} profiles/{words} exact owner words/2048 raw phase cases;replacement versus copy resources/entropy/private phases pass",
        expected.len()
    );
}
