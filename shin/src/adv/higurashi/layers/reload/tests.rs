use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run_native(variable: &str, protocol: String) -> String {
    let mut child = Command::new(std::env::var(variable).unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
#[ignore = "executes original LayerReloader constructor/worker/poll/destructor with checked factories and groups"]
fn higurashi_layer_reload_matches_native_planning_publication_and_cancellation() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    for profile in 0..96 {
        for (delay, cancel) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 2)] {
            protocol += &format!("{delay} {cancel} ");
            let mut layers = HigurashiLayers::new(|| [9; 4]);
            for slot in 0..SLOTS {
                let front = if profile == 0 {
                    2
                } else {
                    (profile + slot) as i32 % 18 - 2
                };
                let back = if profile == 0 {
                    3
                } else {
                    (profile + slot * 7) as i32 % 18 - 2
                };
                let dirty = if profile == 0 {
                    [1, 1]
                } else {
                    [
                        ((profile + slot) % 4) as u8,
                        ((profile / 4 + slot) % 4) as u8,
                    ]
                };
                protocol += &format!("{front} {back} {} {} ", dirty[0], dirty[1]);
                layers.state.dirty[slot] = dirty;
                for bank in 0..2 {
                    let record = &mut layers.state.records[slot][bank];
                    record.words[0] = if bank == 0 { front } else { back };
                    for word in 1..79 {
                        record.words[word] = (slot * 100 + bank * 10 + word) as i32;
                    }
                    let mut owner = HigurashiUserLayer::null([slot as u32 + 1; 4]);
                    owner
                        .properties
                        .tweener_mut(0)
                        .unwrap()
                        .fast_forward_to((1000 + slot * 2 + bank) as f32);
                    owner
                        .properties
                        .tweener_mut(1)
                        .unwrap()
                        .control(901.0, Ticks::from_f32(60.0), 0)
                        .unwrap();
                    owner.properties.advance(Ticks::from_f32(3.0));
                    layers.owners[slot][bank] = Some(owner);
                }
            }
            protocol.push('\n');
            let plan = layers.state.reload_plan();
            let live = plan
                .jobs
                .iter()
                .filter(|j| j.kind != ReloadKind::CopyFront)
                .count();
            let completes = cancel == 0 || (cancel == 2 && live == 0);
            let cancelled_at_start = cancel == 1;
            let count = if cancelled_at_start {
                0
            } else {
                plan.jobs.len()
            };
            let checks = if cancelled_at_start {
                0
            } else if cancel == 2 && live != 0 {
                1
            } else {
                live + delay * live * (live + 1) / 2
            };
            let sleeps = if cancel == 2 && live != 0 {
                1
            } else {
                delay * live
            };
            let mut fields = vec![
                u32::from(completes) as i64,
                count as i64,
                if cancelled_at_start { 0 } else { live as i64 },
                checks as i64,
                sleeps as i64,
            ];
            if !cancelled_at_start {
                for job in &plan.jobs {
                    let copy = job.kind == ReloadKind::CopyFront;
                    fields.extend([
                        job.slot as i64,
                        match job.kind {
                            ReloadKind::Front => 0,
                            ReloadKind::Back => 1,
                            ReloadKind::CopyFront => 2,
                        },
                        if copy {
                            -1
                        } else {
                            job.record.layer_type() as i64
                        },
                    ]);
                    fields
                        .extend((6..14).map(|i| if copy { 0 } else { job.record.words[i] as i64 }));
                }
            }
            if completes {
                let state_before = format!("{:?}", layers.state);
                let old = layers.owners.clone();
                let mut loaded = LoadedReload {
                    jobs: plan
                        .jobs
                        .into_iter()
                        .map(|job| {
                            let owner = (job.kind != ReloadKind::CopyFront)
                                .then(|| HigurashiUserLayer::null([0; 4]));
                            (job, owner)
                        })
                        .collect(),
                };
                layers
                    .publish_reload(&mut loaded, || panic!("All targets have outgoing owners"))
                    .unwrap();
                assert_eq!(format!("{:?}", layers.state), state_before);
                for slot in 0..SLOTS {
                    for bank in 0..2 {
                        let copy = bank == 1
                            && layers.state.dirty[slot][0] != 0
                            && layers.state.dirty[slot][1] == 0
                            && layers.state.records[slot][1].layer_type() & !1 == 2;
                        let kind =
                            layers.state.records[slot][if copy { 0 } else { bank }].layer_type();
                        let owner = layers.owners[slot][bank].as_ref().unwrap();
                        let old = old[slot][if copy { 0 } else { bank }].as_ref().unwrap();
                        assert_eq!(owner.properties.values(), old.properties.values());
                        assert_eq!(
                            format!("{:?}", owner.properties),
                            format!("{:?}", old.properties),
                            "Queued clocks/seeds slot{slot} bank{bank}"
                        );
                        fields.extend([
                            owner.properties.values()[0] as i64,
                            kind as i64,
                            if kind == 3 {
                                layers.state.records[slot][bank].words[2] as i64
                            } else {
                                0
                            },
                        ]);
                    }
                }
            }
            expected.push(fields);
        }
    }
    let output = run_native("SHIN_HIGURASHI_NATIVE_LAYER_RELOAD", protocol);
    assert_eq!(output.lines().count(), expected.len());
    for (case, (line, expected)) in output.lines().zip(expected).enumerate() {
        let fields: Vec<i64> = line
            .split_whitespace()
            .filter(|s| *s != "R" && *s != "P")
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(fields, expected, "LayerReloader case{case}");
    }
    println!(
        "Higurashi LayerReloader:480 exact native constructor/worker/publication/cancellation cases, all32 slots, both dirty bytes, max64 jobs; live queues/seeds and VM records retained"
    );
}

#[test]
#[ignore = "executes original PIC/BUP variant path builders over authored names and all setting bytes"]
fn higurashi_asset_variant_names_match_native_paths() {
    use crate::layer::user::higurashi::selected_asset_name;
    let scenario = super::super::super::replay::tests::scenario(&[0]);
    let mut names: Vec<(u32, String)> = scenario
        .picture_info
        .iter()
        .map(|i| (2, i.name.as_str().to_owned()))
        .chain(
            scenario
                .bustup_info
                .iter()
                .map(|i| (3, i.name.as_str().to_owned())),
        )
        .collect();
    let authored_count = names.len();
    for kind in [2, 3] {
        for name in [
            "",
            "?",
            "??",
            "?a?b?",
            "ABCDEFGHIJKLMN?OPQRSTUV",
            "日本?語?",
            "A",
        ] {
            names.push((kind, name.to_owned()));
        }
    }
    let mut protocol = String::new();
    let mut expected = Vec::new();
    for (index, (kind, name)) in names.into_iter().enumerate() {
        // Authored names exercise both values; special names also cover all256.
        let settings: Vec<u8> = if index >= authored_count {
            (0..=255).collect()
        } else {
            vec![0, 1, 255]
        };
        for setting in settings {
            let hex = if name.is_empty() {
                "-".to_owned()
            } else {
                name.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
            };
            protocol += &format!("{kind} {setting} {hex}\n");
            expected.push(format!(
                "/{}/{}.{}",
                if kind == 2 { "picture" } else { "bustup" },
                selected_asset_name(&name, setting),
                if kind == 2 { "pic" } else { "bup" }
            ));
        }
    }
    let output = run_native("SHIN_HIGURASHI_NATIVE_ASSET_VARIANT", protocol);
    assert_eq!(output.lines().count(), expected.len());
    for (case, (line, path)) in output.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("V"));
        let size: usize = fields.next().unwrap().parse().unwrap();
        let bytes: Vec<u8> = fields.map(|s| s.parse().unwrap()).collect();
        assert_eq!(size, bytes.len());
        assert_eq!(
            String::from_utf8(bytes).unwrap().to_ascii_lowercase(),
            *path,
            "Variant case{case}"
        );
    }
    println!(
        "Higurashi asset variants:{} exact native PIC/BUP paths, authored names and settings bytes0..255",
        expected.len()
    );
}
