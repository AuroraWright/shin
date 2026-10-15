use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn higurashi_scene_defers_live_publication_and_updates_outgoing_owners() {
    use super::super::scene::{CommandStatus, HigurashiScene};
    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    let params = UntypedNumberArray::default();
    let target = layers.state.load(1, 0, 0, params).unwrap()[0].0;
    let mut owner = HigurashiUserLayer::null([1, 2, 3, 4]);
    owner.set_owner_id(1);
    owner
        .properties
        .control(0, 100.0, Ticks::from_f32(128.0), 0)
        .unwrap();
    layers.owners[target.slot as usize][0] = Some(owner);
    let mut scene = HigurashiScene::new(layers);
    let wipe = |duration, flags| HigurashiRuntimeCommand::WIPE {
        args: [0, flags, duration],
        params,
    };
    assert_eq!(
        scene.dispatch(&wipe(64, 1), false).unwrap(),
        Some(CommandStatus::Complete)
    );
    let layers = scene.layers_mut().unwrap();
    layers.state.start_back(1).unwrap();
    let mut back = layers.owners[target.slot as usize][0].clone().unwrap();
    back.properties
        .control(0, 200.0, Ticks::from_f32(128.0), 0x100)
        .unwrap();
    layers.owners[target.slot as usize][1] = Some(back);
    layers.state.control(1, 0, 200, 0).unwrap();
    assert_eq!(
        scene.dispatch(&wipe(64, 1), false).unwrap(),
        Some(CommandStatus::Pending)
    );
    // Constructor integer publication is already visible, but front owners
    // keep their current identity and motion while waiting for the old wipe.
    let front = scene.layers().state.target(1).unwrap();
    assert_eq!(front.bank, 0);
    assert_eq!(scene.layers().state.record(front).property(0), Some(200));
    assert_eq!(scene.layers().group_children()[0].owner_id, 1);
    assert_eq!(
        scene.layers().layer(front).unwrap().properties.values()[0],
        0.0
    );
    assert!(scene.layers_mut().is_err());
    scene.advance(Ticks::from_f32(32.0), false, false, None);
    scene.advance(Ticks::from_f32(32.0), true, true, None);
    assert_eq!(
        scene.layers().layer(front).unwrap().properties.values()[0],
        0.0
    );
    scene.advance(Ticks::from_f32(32.0), true, false, None);
    assert_eq!(
        scene.layers().layer(front).unwrap().properties.values()[0],
        25.0
    );
    assert_eq!(
        scene
            .transition()
            .unwrap()
            .previous
            .layer(front.slot)
            .unwrap()
            .properties
            .values()[0],
        25.0
    );
    assert_eq!(scene.poll_command(false), CommandStatus::Pending);
    scene.advance(Ticks::from_f32(32.0), true, false, None);
    assert!(scene.transition().is_none());
    assert_eq!(scene.poll_command(false), CommandStatus::Complete);
    assert_eq!(
        scene.layers().layer(front).unwrap().properties.values()[0],
        100.0
    );
    assert_eq!(
        scene
            .transition()
            .unwrap()
            .previous
            .layer(front.slot)
            .unwrap()
            .properties
            .values()[0],
        50.0
    );
    scene.advance(Ticks::from_f32(8.0), true, false, None);
    assert_eq!(
        scene.layers().layer(front).unwrap().properties.values()[0],
        112.5
    );
    assert_eq!(
        scene
            .transition()
            .unwrap()
            .previous
            .layer(front.slot)
            .unwrap()
            .properties
            .values()[0],
        56.25
    );
    assert_eq!(
        scene
            .dispatch(&HigurashiRuntimeCommand::WIPEWAIT {}, false)
            .unwrap(),
        Some(CommandStatus::Pending)
    );
    scene.fast_forward_wiper();
    assert_eq!(scene.poll_command(false), CommandStatus::Pending);
    scene.advance(Ticks::ZERO, true, false, None);
    assert_eq!(scene.poll_command(false), CommandStatus::Complete);
    assert!(scene.transition().is_none());
}

fn snapshot(state: &LayerBanks) -> u64 {
    let mut words = Vec::new();
    words.extend(state.free.map(u32::from));
    words.push(state.allocated as u32);
    words.extend(state.logical_to_slot.map(u32::from));
    words.extend(state.slot_to_logical.map(u32::from));
    words.extend(state.selected.map(u32::from));
    words.extend(state.back.map(u32::from));
    for (dirty, records) in state.dirty.iter().zip(&state.records) {
        words.extend(dirty.map(u32::from));
        for record in records {
            words.extend(record.words.map(|v| v as u32));
        }
    }
    words.extend(state.root.map(|v| v as u32));
    for screen in state.screen {
        words.extend(screen.map(|v| v as u32));
    }
    words.push(u32::from(state.screen_back));
    words.extend(state.page.map(|v| v as u32));
    words.push(state.loads_with_init);
    words.push(u32::from(state.back_started));
    words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
}

#[test]
#[ignore = "executes the updated AArch64 executable's layer command constructors"]
fn higurashi_native_layer_banks_commands_and_wipe_commit() {
    let fixture = std::env::var("SHIN_HIGURASHI_NATIVE_LAYER_COMMANDS").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut random = 0x631cdfu32;
    let mut next = || {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        random
    };
    for sequence in 0..32 {
        let mut state = LayerBanks::default();
        input.push_str("N\n");
        expected.push(("N".to_owned(), snapshot(&state), -1, Vec::new()));
        for step in 0..192 {
            let r = next();
            let id = if step % 11 == 0 {
                -4
            } else if step % 19 == 0 {
                -1 - (r % 3) as i32
            } else {
                ((r >> 7) % 40) as i32 + if sequence % 2 == 0 { 0 } else { 984 }
            };
            let mut result = -1;
            let targets;
            let line = match step % 12 {
                0 => {
                    // Clamp/reverse includes all IDs and narrow dense ranges.
                    let (from, to) = if step % 24 == 0 {
                        (-128, 2048)
                    } else {
                        (id + 2, id - 2)
                    };
                    state.select(from, to);
                    targets = Vec::new();
                    format!("S {from} {to}")
                }
                1 | 6 => {
                    targets = state
                        .start_back(id)
                        .unwrap()
                        .into_iter()
                        .map(|t| (t, false))
                        .collect();
                    format!("B {id}")
                }
                2 | 7 | 8 => {
                    let kind = (r % 15) as i32;
                    let flags = (r >> 23) & 15;
                    let p = (
                        (r >> 1) as i32,
                        -640,
                        -720,
                        1280,
                        720,
                        -1,
                        7,
                        (r >> 24) as i32,
                    );
                    targets = state.load(id, kind, flags, p).unwrap();
                    format!(
                        "L {id} {kind} {flags} {} {} {} {} {} {} {} {}",
                        p.0, p.1, p.2, p.3, p.4, p.5, p.6, p.7
                    )
                }
                3 | 9 => {
                    let property = ((r >> 15) % 65) as i32;
                    let value = if step % 24 == 3 { i32::MAX } else { r as i32 };
                    let flags = if (r >> 5) & 1 == 0 {
                        0
                    } else {
                        0x80 | 0x200 | 3
                    };
                    targets = state
                        .control(id, property, value, flags)
                        .unwrap()
                        .into_iter()
                        .map(|t| (t, false))
                        .collect();
                    format!("C {id} {property} {value} 73 {flags} 0 0 0 0 0")
                }
                4 => {
                    targets = state
                        .initialize(id)
                        .unwrap()
                        .into_iter()
                        .map(|t| (t, false))
                        .collect();
                    format!("I {id}")
                }
                5 | 10 => {
                    targets = state
                        .unload(id)
                        .unwrap()
                        .into_iter()
                        .map(|t| (t, false))
                        .collect();
                    format!("U {id} 0")
                }
                _ => {
                    let id = ((r >> 13) % 1024) as u16;
                    result = state.allocate(id).map(i32::from).unwrap_or(-1);
                    targets = Vec::new();
                    format!("A {id}")
                }
            };
            input.push_str(&line);
            input.push('\n');
            expected.push((line, snapshot(&state), result, targets));
            if step % 23 == 10 {
                state.commit_back();
                input.push_str("W\n");
                expected.push(("W".to_owned(), snapshot(&state), -1, Vec::new()));
            }
        }
    }
    let mut child = Command::new(fixture)
        .arg(exe)
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
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    let mut target_count = 0;
    for (row, (actual, (command, hash, result, targets))) in
        native.lines().zip(expected.iter()).enumerate()
    {
        let words: Vec<_> = actual.split_whitespace().collect();
        assert_eq!(
            u64::from_str_radix(words[1], 16).unwrap(),
            *hash,
            "row {row}: {command}"
        );
        assert_eq!(
            words[2].parse::<i32>().unwrap(),
            *result,
            "row {row}: {command}"
        );
        assert_eq!(
            words[3].parse::<usize>().unwrap(),
            targets.len(),
            "row {row}: {command}"
        );
        assert_eq!(words.len(), 4 + targets.len() * 4);
        for (i, (t, keep)) in targets.iter().enumerate() {
            let at = 4 + i * 4;
            let expected = [
                u32::from(t.logical),
                u32::from(t.slot),
                u32::from(t.bank),
                u32::from(*keep),
            ];
            for j in 0..4 {
                assert_eq!(
                    words[at + j].parse::<u32>().unwrap(),
                    expected[j],
                    "row {row}: {command}, target {i}"
                );
            }
        }
        target_count += targets.len();
    }
    println!(
        "Higurashi native layer commands: {} complete state hashes, {target_count} ordered targets",
        expected.len()
    );
}
