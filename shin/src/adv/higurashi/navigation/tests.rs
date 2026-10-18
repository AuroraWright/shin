use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
#[ignore = "original2858/29c4 event constructors,checkpoint ordering,unsigned boundary wrap and native live completion"]
fn higurashi_event_boundaries_complete_like_native_without_external_reply() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for end in [false, true] {
        for flags in [0, 7, 0x80000000, u32::MAX] {
            for boundary in [0, 1, 200, u32::MAX] {
                input.push_str(&format!("E {} {flags:x} {boundary}\n", u8::from(end)));
                expected.push(vec![
                    1u32,
                    boundary.wrapping_add(1),
                    if end { u32::MAX } else { flags },
                    1,
                ]);
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_NAVIGATION").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual: Vec<Vec<u32>> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            l.strip_prefix("E ")
                .unwrap()
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(actual, expected);
    eprintln!(
        "Native event completion:32 original event constructors/128 exact values;one checkpoint,boundary wrap/event flags and immediate live completion pass"
    );
}

fn observation(
    lookups: u32,
    searches: u32,
    target: u32,
    succeeds: bool,
    mode: u32,
    modes: u32,
    temporary: u32,
    opened: bool,
    cue: Option<u32>,
) -> Vec<i64> {
    let mut row = vec![
        i64::from(lookups),
        i64::from(searches),
        i64::from(succeeds),
        i64::from(succeeds),
        i64::from(target),
        i64::from(target),
        if succeeds { i64::from(mode) } else { 0 },
        i64::from(modes),
        i64::from(temporary),
        i64::from(opened),
        if opened { 1 } else { -1 },
        if opened { 2 } else { 0 },
        i64::from(cue.is_some()),
    ];
    if let Some(cue) = cue {
        row.push(i64::from(cue));
    }
    row
}
#[test]
#[ignore = "original14974/14140/14414/17d1c/17f04 decisions and actual chapter-question/font constructor ownership"]
fn higurashi_navigation_matches_native_rewind_chapter_opening_and_callbacks() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let targets = [(0, 0), (7, 8), (u32::MAX, 0x80000000)];
    for flags in [0, 0x80000000, u32::MAX] {
        for modes in [0, 1, 2, 3, 0x80, u32::MAX] {
            for forced in [false, true] {
                for read_only in [false, true] {
                    for is_read in [0, 1, 255] {
                        for blocked in [0, 1, 255] {
                            for count in [0, 1, 2, u32::MAX] {
                                for has_text in [false, true] {
                                    for restore in [false, true] {
                                        for (previous, current) in targets {
                                            input.push_str(&format!("P {flags:x} {modes:x} {} {} {is_read} {blocked} {count} {} {previous} {current} {}\n",
                u8::from(forced),u8::from(read_only),u8::from(has_text),u8::from(restore)));
                                            let timeline = MessageTimeline {
                                                normal_messages: count,
                                                previous_message: previous,
                                                current_message: current,
                                                ..Default::default()
                                            };
                                            let decision = previous_message(
                                                ReadState {
                                                    flags,
                                                    fast_forward_blocked: blocked,
                                                    is_read,
                                                },
                                                timeline,
                                                ReadPolicy {
                                                    modes,
                                                    forced,
                                                    read_only,
                                                },
                                                has_text,
                                            );
                                            expected.push(match decision {
                                                PreviousMessage::Suppressed => observation(
                                                    0, 0, 0, false, 0, modes, 7, false, None,
                                                ),
                                                PreviousMessage::Unavailable => observation(
                                                    0,
                                                    0,
                                                    0,
                                                    false,
                                                    0,
                                                    modes,
                                                    7,
                                                    false,
                                                    Some(3),
                                                ),
                                                PreviousMessage::Restore(target) => observation(
                                                    1,
                                                    0,
                                                    target,
                                                    restore,
                                                    1,
                                                    modes,
                                                    7,
                                                    false,
                                                    Some(if restore { 2 } else { 3 }),
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let rewind_cases = expected.len();
    for kind in [Chapter::Previous, Chapter::Next] {
        for disabled in [false, true] {
            for stage in [0, 1, u32::MAX] {
                for button in [-2, -1, 0, 1] {
                    for count in [0, 1, 2, u32::MAX] {
                        for (previous, current) in targets {
                            for restore in [false, true] {
                                for search in [false, true] {
                                    input.push_str(&format!("C {} {} {stage} {button} {count} {previous} {current} {} {}\n",
            u8::from(kind == Chapter::Next),u8::from(disabled),u8::from(restore),u8::from(search)));
                                    let accepted =
                                        accepts_chapter(disabled, Callback { stage, button });
                                    expected.push(if !accepted {
                                        observation(0, 0, 0, false, 0, 0, 7, false, None)
                                    } else if kind == Chapter::Previous {
                                        let target = previous_chapter(MessageTimeline {
                                            quick_return_messages: count,
                                            previous_chapter: previous,
                                            current_chapter: current,
                                            ..Default::default()
                                        });
                                        observation(
                                            1,
                                            0,
                                            target,
                                            restore,
                                            0,
                                            0,
                                            if restore { 0 } else { 7 },
                                            false,
                                            None,
                                        )
                                    } else {
                                        observation(
                                            0,
                                            1,
                                            0,
                                            search,
                                            0,
                                            0,
                                            if search { 0 } else { 7 },
                                            false,
                                            None,
                                        )
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let callback_cases = expected.len() - rewind_cases;
    for kind in [Chapter::Previous, Chapter::Next] {
        for modes in [0, 1, 2, u32::MAX] {
            for temporary in [0, 7, 255] {
                for available in [false, true] {
                    input.push_str(&format!(
                        "O {} {modes:x} {temporary} {}\n",
                        u8::from(kind == Chapter::Next),
                        u8::from(available)
                    ));
                    let opens = kind == Chapter::Previous || available;
                    let mut row = observation(
                        0,
                        u32::from(kind == Chapter::Next),
                        0,
                        false,
                        0,
                        if opens { 0 } else { modes },
                        temporary,
                        opens,
                        Some(if opens { 2 } else { 3 }),
                    );
                    row[3] = i64::from(kind == Chapter::Next && available); // Discarded preflight VM.
                    expected.push(row);
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_NAVIGATION").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut writer = child.stdin.take().unwrap();
    let write = std::thread::spawn(move || writer.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    write.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let actual: Vec<Vec<i64>> = text
        .lines()
        .map(|line| {
            line.split_whitespace()
                .skip(2)
                .map(|v| v.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(actual.len(), expected.len());
    for (n, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "Native navigation observation{n}");
    }
    eprintln!(
        "Native navigation:{rewind_cases} rewind/{callback_cases} callback/48 opening cases;{} exact values,skip/read/force/count/text-target gates,lookup failures,stage1 Yes/disabled predicate,previous/current chapter target,second next-search,mode0/1,cues/input reset and actual question/font ownership pass",
        expected.iter().map(Vec::len).sum::<usize>()
    );
}
