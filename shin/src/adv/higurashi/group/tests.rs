use super::*;
use shin_core::time::Ticks;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "executes older native group stable sort and two-pass traversal; no GPU"]
fn higurashi_native_group_priority_id_pass_and_stencil_order() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut random = 0x6e340u32;
    let mut visited = 0;
    for case in 0..384 {
        let count = case % 33;
        input.push_str(&format!("G {count}"));
        let mut layers = Vec::new();
        let mut ids = Vec::new();
        for i in 0..count {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let id = match case % 8 {
                0 => (count - i) as i32,
                1 => 7,
                2 => random as i32,
                _ => (random % 1024) as i32,
            };
            let priority = match case % 8 {
                0 | 1 => 1000.0,
                2 => ((random % 4001) as f32 - 2000.0) / 4.0,
                3 => i as f32 * f32::EPSILON * 0.5,
                4 => (i % 3) as f32 * (-0.125),
                _ => (random % 7) as f32,
            };
            let mut layer = HigurashiUserLayer::null([1, 2, 3, 4]);
            // Read the live tween value, not its integer target.
            layer
                .properties
                .control(5, priority, Ticks::ZERO, 0)
                .unwrap();
            if case % 8 == 5 {
                layer
                    .properties
                    .control(5, priority + 20.0, Ticks::from_f32(10.0), 0)
                    .unwrap();
                layer.properties.advance(Ticks::from_f32(2.5));
            }
            let live = layer.properties.values()[5];
            input.push_str(&format!(" {id} {:08x}", live.to_bits()));
            layers.push(layer);
            ids.push(id);
        }
        input.push('\n');
        let children = ordered(
            layers
                .iter()
                .enumerate()
                .map(|(i, layer)| Child {
                    owner_id: ids[i],
                    slot: i as u8,
                    layer,
                })
                .collect(),
        );
        let mut words = vec![count as u32];
        words.extend(children.iter().map(|c| u32::from(c.slot)));
        for draw in render_plan(children).unwrap() {
            words.extend([
                u32::from(draw.child.slot),
                u32::from(draw.stencil),
                match draw.pass {
                    PassKind::Opaque => 1,
                    PassKind::Transparent => 0,
                },
            ]);
        }
        visited += count * 2;
        expected.push(words);
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_GROUP_ORDER").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let native = String::from_utf8(result.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (case, (actual, expected)) in native.lines().zip(expected.iter()).enumerate() {
        let words: Vec<u32> = actual
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(&words, expected, "group case{case}");
    }
    println!(
        "Higurashi native group: 384 priority/ID permutations and {visited} exact pass/stencil visits match"
    );
}
