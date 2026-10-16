//! Full native TaskGroup/zero-button Question timing against the shared owner.
use super::*;
use crate::adv::backlog::rollback::Confirmation;
use shin_core::time::Ticks;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
#[ignore = "executes bounded original SaveGame/TaskGroup/notification bodies; CPU only"]
fn ger_native_save_write_notification_and_coordinator_match() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let binary = std::env::var_os("SHIN_GEROKASU2_SAVE_WRITE_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("gerokasu2/.local/save-write/native-save-write"));
    let mut script = String::new();
    let mut expected: Vec<Vec<i64>> = Vec::new();
    let mut profiles = 0;
    for flags in [0, 1, 2, 3, 0xffff_fffe, 0xffff_ffff] {
        for dt in [0.0, 0.25, 0.5, 1.0, 2.5, 12.0, 1000.0] {
            for permission in [0, 1] {
                for delay in [0, 3, 17] {
                    profiles += 1;
                    script.push_str(&format!("N {flags}\n"));
                    let mut owner = Controller::new(flags);
                    let mut question: Option<Confirmation> = None;
                    let (
                        mut panels,
                        mut requests,
                        mut completed,
                        mut intermediate,
                        mut final_count,
                    ) = (0, 0, 0, 0, 0);
                    let mut status = 0;
                    let mut requested_at = None;
                    let mut ready_sent = false;
                    let snapshot = |owner: &Controller,
                                    question: &Option<Confirmation>,
                                    panels,
                                    requests,
                                    completed,
                                    status,
                                    intermediate,
                                    final_count| {
                        let q = question.as_ref();
                        vec![
                            i64::from(owner.flags()),
                            match owner.phase() {
                                Phase::Start => 0,
                                Phase::Waiting => i32::MAX as i64,
                                Phase::Completing => 2,
                                Phase::Done => -1,
                            },
                            panels,
                            requests,
                            completed,
                            i64::from(q.is_some()),
                            status,
                            i64::from(q.is_some_and(|q| q.is_open())),
                            i64::from(q.is_some_and(|q| q.is_done())),
                            q.map_or(0, |q| i64::from(q.scale.value().to_bits())),
                            q.map_or(0, |q| i64::from(q.opacity.value().to_bits())),
                            i64::from(owner.is_done()),
                            intermediate,
                            final_count,
                        ]
                    };
                    expected.push(snapshot(
                        &owner,
                        &question,
                        panels,
                        requests,
                        completed,
                        status,
                        intermediate,
                        final_count,
                    ));
                    for frame in 0..180 {
                        if !ready_sent && requested_at.is_some_and(|at| frame > at + delay) {
                            script.push_str("I 2\n");
                            status = 2;
                            owner.io_finished();
                            ready_sent = true;
                            expected.push(snapshot(
                                &owner,
                                &question,
                                panels,
                                requests,
                                completed,
                                status,
                                intermediate,
                                final_count,
                            ));
                        }
                        script.push_str(&format!("F {dt} {permission} 30000000 ffffffff\n"));
                        owner.update();
                        for event in owner.take_events() {
                            match event {
                                Event::ShowWait => {
                                    panels += 1;
                                    question = Some(Confirmation::notification());
                                }
                                Event::Completed { success } => {
                                    assert!(success);
                                    completed += 1;
                                }
                                Event::RequestWrite => unreachable!(),
                            }
                        }
                        if question.as_ref().is_some_and(|q| q.is_done()) {
                            question = None;
                        }
                        if let Some(q) = question.as_mut() {
                            q.update_notification(Ticks::from_f32(dt), || owner.poll_write());
                            if q.take_intermediate_outcome().is_some() {
                                intermediate += 1;
                                owner.question_response(false);
                            }
                            if q.take_outcome().is_some() {
                                final_count += 1;
                                owner.question_response(true);
                            }
                        }
                        for event in owner.take_events() {
                            assert_eq!(event, Event::RequestWrite);
                            requests += 1;
                            status = 1;
                            requested_at = Some(frame);
                        }
                        expected.push(snapshot(
                            &owner,
                            &question,
                            panels,
                            requests,
                            completed,
                            status,
                            intermediate,
                            final_count,
                        ));
                    }
                    script.push_str("X\n");
                }
            }
        }
    }
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
            root.join("gerokasu2/.local/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(script.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut rows = text
        .lines()
        .filter(|s| s.starts_with("W ") || s.starts_with("C "));
    let mut maximum = 0f32;
    for (i, expected) in expected.iter().enumerate() {
        let line = rows.next().unwrap();
        let fields: Vec<i64> = line
            .split_whitespace()
            .skip(2)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(fields.len(), 12, "{line}");
        let callbacks = rows.next().unwrap();
        let callbacks: Vec<i64> = callbacks
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        let mut actual = fields[..7].to_vec();
        actual.extend([i64::from(fields[7] == 2), i64::from(fields[7] == -1)]);
        actual.extend_from_slice(&fields[9..12]);
        actual.extend(callbacks);
        assert_eq!(actual.len(), expected.len());
        for (word, (&a, &e)) in actual.iter().zip(expected).enumerate() {
            if word == 9 || word == 10 {
                let d = (f32::from_bits(a as u32) - f32::from_bits(e as u32)).abs();
                maximum = maximum.max(d);
                assert!(d <= 0.000003, "sample{i} field{word} native{a} Rust{e}");
            } else {
                assert_eq!(a, e, "sample{i} field{word}");
            }
        }
    }
    assert!(rows.next().is_none());
    println!(
        "GER_SAVE_WRITE_SHARED profiles={profiles} observations={} exactflags/state/IOstart/callbacks/done tween_max_abs={maximum}",
        expected.len()
    );
}
