use super::*;

fn active(availability: TitleAvailability, item: i32) -> TitleController {
    let mut owner = TitleController::new(availability, item);
    owner.assets_ready();
    for _ in 0..5 {
        assert_eq!(owner.update(100.0), None);
    }
    assert_eq!(owner.phase(), TitlePhase::Active);
    owner
}

#[test]
fn title_new_continue_are_gated_until_fade_and_publish_once() {
    for continuation in [false, true] {
        let mut owner = TitleController::new(
            TitleAvailability {
                continue_available: continuation,
                extra_unlocked: false,
            },
            -1,
        );
        assert!(!owner.confirm_selected());
        assert_eq!(owner.selected(), Some(usize::from(continuation)));
        owner.assets_ready();
        owner.update(60.0);
        assert!(!owner.confirm_selected());
        for _ in 0..4 {
            owner.update(100.0);
        }
        assert!(owner.confirm_selected());
        assert!(!owner.input(TitleInput {
            down: true,
            ..Default::default()
        }));
        owner.update(100.0);
        assert_eq!(owner.phase(), TitlePhase::Fading);
        for _ in 0..120 {
            assert_eq!(owner.update(119.0), None);
        }
        assert_eq!(
            owner.update(1.0),
            Some(if continuation {
                TitleAction::Continue
            } else {
                TitleAction::NewGame { init_value: 0 }
            })
        );
        assert_eq!(owner.update(120.0), None);
        owner.recover_handoff();
        assert_eq!(owner.phase(), TitlePhase::Entering);
        owner.update(100.0);
        assert!(owner.confirm_selected());
    }
}

#[test]
fn title_extra_page_back_child_recovery_and_input_priority() {
    let mut owner = active(
        TitleAvailability {
            continue_available: true,
            extra_unlocked: true,
        },
        4,
    );
    assert_eq!(owner.rows()[owner.selected().unwrap()], TitleItem::Extra);
    owner.input(TitleInput {
        confirm: true,
        back: true,
        down: true,
        up: true,
    });
    owner.update(100.0);
    assert_eq!(
        owner.rows(),
        &[
            TitleItem::CgGallery,
            TitleItem::MusicGallery,
            TitleItem::BustupGallery,
            TitleItem::Back
        ]
    );
    owner.update(100.0);
    owner.input(TitleInput {
        back: true,
        down: true,
        ..Default::default()
    });
    owner.update(100.0);
    assert_eq!(owner.rows()[owner.selected().unwrap()], TitleItem::Extra);
    owner.update(100.0);
    owner.select(2);
    owner.confirm_selected();
    assert_eq!(owner.update(100.0), Some(TitleAction::Load));
    assert_eq!(owner.phase(), TitlePhase::Child);
    assert_eq!(owner.update(100.0), None);
    owner.recover_child();
    owner.update(100.0);
    assert_eq!(owner.selected(), Some(2));
    assert!(owner.point(f32::NAN, 464.0).is_none());
}

#[test]
fn title_retained_children_dim_and_restore_on_parent_frames_after_final_callback() {
    for item in [TitleItem::Load, TitleItem::Config, TitleItem::Manual] {
        let mut owner = active(TitleAvailability::default(), 0);
        owner.select(owner.rows().iter().position(|row| *row == item).unwrap());
        owner.confirm_selected();
        assert_eq!(owner.blackout(), 0.);
        assert!(owner.update(100.).is_some());
        assert_eq!(owner.phase(), TitlePhase::Child);
        assert_eq!(owner.blackout().to_bits(), 0.1f32.to_bits());
        for _ in 0..4 {
            owner.advance_retained_parent_clock();
        }
        assert_eq!(owner.blackout(), 0.5);
        assert!(!owner.input(TitleInput {
            confirm: true,
            ..Default::default()
        }));
        owner.recover_child();
        assert_eq!(owner.phase(), TitlePhase::Entering);
        // Root callbacks run before this frame's parent clock, so a final
        // close callback already renders the first fade-back sample that frame.
        owner.advance_retained_parent_clock();
        assert_eq!(owner.blackout().to_bits(), 0.4f32.to_bits());
        for _ in 0..4 {
            owner.update(0.);
        }
        assert_eq!(owner.blackout(), 0.);
        assert_eq!(owner.row_alpha(0), 0.);
        owner.update(100.);
        assert_eq!(owner.phase(), TitlePhase::Entering);
        owner.update(0.);
        assert_eq!(owner.phase(), TitlePhase::Active);
        assert_eq!(owner.rows()[owner.selected().unwrap()], item);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod native {
    use super::*;
    use std::{
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };

    fn native(source: &str, input: &str) -> Vec<Vec<i128>> {
        let manifest = PathBuf::from(option_env!("CARGO_MANIFEST_DIR").unwrap_or("."));
        let root = manifest
            .ancestors()
            .find(|path| {
                path.join("reverse-engineering/gerokasu2/native-title.c")
                    .is_file()
            })
            .expect("Ger title probe source root")
            .to_owned();
        let executable = std::env::var_os("SHIN_GEROKASU2_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("game-data/gerokasu2/uncompressed_executable"));
        let normal = std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"));
        let system = std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("test-fixtures/gerokasu2/system.fnt"));
        let output_dir = root.join("reverse-engineering/gerokasu2/title");
        std::fs::create_dir_all(&output_dir).unwrap();
        let override_binary = match source {
            "native-title-return-rows" => {
                std::env::var_os("SHIN_GEROKASU2_TITLE_RETURN_ROWS_ORACLE")
            }
            "native-title-first" => std::env::var_os("SHIN_GEROKASU2_TITLE_FIRST_ORACLE"),
            _ => None,
        };
        let binary = override_binary
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| output_dir.join(source));
        if override_binary.is_none() {
            assert!(
                Command::new("clang")
                    .args(["-std=c11", "-O2"])
                    .arg(root.join(format!("reverse-engineering/gerokasu2/{source}.c")))
                    .arg("-o")
                    .arg(&binary)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let mut child = Command::new(binary)
            .args([executable, normal, system])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Write on another thread so a large native trace cannot fill stdout
        // while the parent is still serializing its bounded input corpus.
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_owned();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        if source == "native-title-return-rows" || source == "native-title-first" {
            let directory = root.join("reverse-engineering/gerokasu2/title-return");
            std::fs::create_dir_all(&directory).unwrap();
            let name = if source == "native-title-first" {
                "first-output.txt"
            } else {
                "original-output.txt"
            };
            std::fs::write(directory.join(name), &output).unwrap();
        }
        output
            .lines()
            .map(|line| {
                let mut words = line.split_whitespace();
                let tag = words.next().unwrap().as_bytes()[0] as i128;
                std::iter::once(tag)
                    .chain(words.map(|v| v.parse().unwrap()))
                    .collect()
            })
            .collect()
    }

    #[test]
    #[ignore = "executes original Title First/Start callbacks and capture clocks; CPU only"]
    fn ger_native_title_first_capture_completion_and_music_requests() {
        let mut input = String::new();
        let mut profiles = Vec::new();
        for requested in [-2, -1, 0, 8, 9] {
            for delta in [0., 0.25, 1., 2.5, 60., 1000.] {
                let mut model = TitleController::new(TitleAvailability::default(), requested);
                model.assets_ready();
                input.push_str(&format!("{requested} {delta} 300\n"));
                profiles.push((model, delta, requested, 0usize, 0usize, 0u32));
            }
        }
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("reverse-engineering/gerokasu2/title-return");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("first-profiles.txt"), &input).unwrap();
        let output = native("native-title-first", &input);
        for row in &output {
            assert_eq!(row[0], b'F' as i128);
            let case = row[1] as usize - 1;
            let (model, delta, requested, stops, plays, fade) = &mut profiles[case];
            assert_eq!(model.update(*delta), None);
            for event in model.take_music_actions() {
                match event {
                    TitleMusicAction::Stop(ticks) => {
                        *stops += 1;
                        *fade = ticks;
                    }
                    TitleMusicAction::Play(ticks) => {
                        *plays += 1;
                        *fade = ticks;
                    }
                }
            }
            assert_eq!(
                row[3],
                if model.phase() == TitlePhase::Opening {
                    1
                } else {
                    -1
                },
                "Firstcase{case} frame{}",
                row[2]
            );
            assert_eq!(row[4], model.first_elapsed().to_bits() as i128);
            assert_eq!(row[5], 60f32.to_bits() as i128);
            assert_eq!(row[6], *stops as i128);
            assert_eq!(
                row[7], *plays as i128,
                "Firstcase{case} frame{} music",
                row[2]
            );
            assert_eq!(row[8], *fade as i128);
            assert_eq!(row[9], 1);
            let expected_tasks = if model.phase() == TitlePhase::Opening {
                0
            } else if *requested >= 0 || model.phase() == TitlePhase::Starting {
                1
            } else {
                2
            };
            assert_eq!(row[10], expected_tasks);
            assert_eq!(
                row[12],
                if *requested < 0 {
                    0
                } else {
                    0.3f32.to_bits() as i128
                }
            );
            assert_eq!(row[13], 0);
        }
        assert_eq!(output.len(), 9000);
        eprintln!(
            "GER_TITLE_FIRST profiles=30 snapshots=9000 exact_capture_clock_and_music_requests=true resource_ready_contract=0"
        );
    }

    #[test]
    #[ignore = "executes original retained-child callback, DBA20, DD720 and DA170 row clocks; CPU only"]
    fn ger_native_title_return_rows_advance_in_callback_frame_and_poll_previous_clock() {
        let mut protocol = String::new();
        let mut profiles = Vec::new();
        for availability in [
            TitleAvailability::default(),
            TitleAvailability {
                continue_available: true,
                extra_unlocked: false,
            },
            TitleAvailability {
                continue_available: true,
                extra_unlocked: true,
            },
        ] {
            for item in [TitleItem::Load, TitleItem::Config, TitleItem::Manual] {
                for delta in [0., 0.25, 1., 2.5, 20., 1000.] {
                    for stage0 in [1usize, 999] {
                        for stage1 in [1usize, 8] {
                            let mut model = active(availability, 0);
                            model.select(model.rows().iter().position(|i| *i == item).unwrap());
                            model.confirm_selected();
                            protocol.push_str(&format!(
                                "{} {} {stage0} {stage1} {delta} 200\n",
                                item as u8,
                                model.rows().len()
                            ));
                            profiles.push((model, item, delta, stage1));
                        }
                    }
                }
            }
        }
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("reverse-engineering/gerokasu2/title-return");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("profiles.txt"), &protocol).unwrap();
        let output = native("native-title-return-rows", &protocol);
        let mut words = 0usize;
        for row in &output {
            assert_eq!(row[0], b'R' as i128);
            let case = row[1] as usize - 1;
            let frame = row[2] as usize;
            let (model, item, delta, stage1) = &mut profiles[case];
            if frame == 0 {
                assert_eq!(model.update(100.), TitleController::action(*item));
            } else if frame <= *stage1 {
                if frame == *stage1 {
                    model.recover_child();
                }
                model.advance_retained_parent_frame(*delta);
            } else {
                assert_eq!(model.update(*delta), None);
            }
            assert_eq!(row[3], *item as i128);
            assert_eq!(row[4], model.rows().len() as i128);
            assert_eq!(
                row[5],
                model.blackout().to_bits() as i128,
                "case{case} frame{frame} black"
            );
            assert_eq!(
                row[6],
                i128::from(model.phase() == TitlePhase::Entering),
                "case{case} frame{frame} task"
            );
            assert_eq!(row[7], i128::from(frame >= *stage1));
            assert_eq!(
                row[8],
                i128::from(model.phase() == TitlePhase::Active),
                "case{case} frame{frame} input"
            );
            for i in 0..model.rows().len() {
                assert_eq!(
                    row[9 + i * 2],
                    model.row_alpha(i).to_bits() as i128,
                    "case{case} frame{frame} row{i}"
                );
                assert_eq!(
                    row[10 + i * 2],
                    model.row_flash(i).to_bits() as i128,
                    "case{case} frame{frame} flash{i}"
                );
                words += 2;
            }
        }
        assert_eq!(output.len(), profiles.len() * 200);
        eprintln!(
            "GER_TITLE_RETURN_ROWS profiles={} snapshots={} exact_row_words={words} same_frame_advance_previous_clock_completion=true",
            profiles.len(),
            output.len()
        );
    }

    #[test]
    #[ignore = "executes original Ger child dispatch, final callbacks, parent clock and blackout tweens"]
    fn ger_native_title_retained_child_blackout_parent_clock_and_final_callbacks() {
        let mut protocol = String::new();
        let mut profiles = Vec::new();
        for item in [TitleItem::Load, TitleItem::Config, TitleItem::Manual] {
            for delta in [0., 0.25, 1., 2.5, 120., 1000.] {
                for stage0 in [1usize, 3, 99] {
                    for stage1 in [1usize, 3, 5, 8, 99] {
                        protocol
                            .push_str(&format!("{} {stage0} {stage1} {delta} 20\n", item as u8));
                        let mut model = active(TitleAvailability::default(), 0);
                        model.select(model.rows().iter().position(|row| *row == item).unwrap());
                        model.confirm_selected();
                        profiles.push((model, item, stage0, stage1));
                    }
                }
            }
        }
        let output = native("native-title-child-fade", &protocol);
        for row in &output {
            assert_eq!(row[0], b'C' as i128);
            let case = row[1] as usize - 1;
            let frame = row[2] as usize;
            let (model, item, _stage0, stage1) = &mut profiles[case];
            if frame == 0 {
                let action = model.update(100.);
                assert_eq!(action, TitleController::action(*item));
                assert_eq!(model.phase(), TitlePhase::Child);
            } else {
                if frame == *stage1 {
                    model.recover_child();
                }
                // stage0 is deliberately ignored by all three original Title
                // callbacks. Keep row clocks out of this scoped parent check.
                model.advance_retained_parent_clock();
            }
            assert_eq!(row[3], *item as i128);
            assert_eq!(
                row[4],
                model.blackout().to_bits() as i128,
                "childcase{case} frame{frame}"
            );
            assert_eq!(
                row[5],
                i128::from(model.child_blackout.busy()),
                "child busy case{case} frame{frame}"
            );
            assert_eq!(
                row[6],
                i128::from(frame >= *stage1),
                "parent reentry case{case} frame{frame}"
            );
            assert_eq!(row[7], *item as i128);
            assert_eq!(row[8], 1);
        }
        assert_eq!(output.len(), profiles.len() * 20);
        eprintln!(
            "Ger Title child blackout: {} profiles/{} exact parent samples, three actual child factories/callbacks, ignored stage0, queued early close and literal-one clock",
            profiles.len(),
            output.len()
        );
    }

    #[test]
    #[ignore = "executes original Ger title menu/input/dispatch/ADV publication and row tween bodies"]
    fn ger_native_title_rows_input_dispatch_progress_and_motion_match() {
        let mut input = String::new();
        let mut expected = Vec::new();
        let mut menu_cases = 0;
        for continuation in [false, true] {
            for persisted in [-32768, -1, 0, 1, 2, 32767] {
                for requested in -2..=12 {
                    let model = TitleController::new(
                        TitleAvailability {
                            continue_available: continuation,
                            extra_unlocked: persisted != 0,
                        },
                        requested,
                    );
                    input.push_str(&format!(
                        "M {} {persisted} {requested} {}\n",
                        u8::from(continuation),
                        model.selected.unwrap()
                    ));
                    let mut row = vec![
                        b'M' as i128,
                        i128::from(model.extra),
                        model.selected.unwrap() as i128,
                        model.rows.len() as i128,
                    ];
                    for (i, item) in model.rows.iter().enumerate() {
                        row.extend([
                            *item as i128,
                            1546.0f32.to_bits() as i128,
                            (464.0 + i as f32 * 84.0).to_bits() as i128,
                        ]);
                    }
                    expected.push(row);
                    menu_cases += 1;
                }
            }
        }
        assert_eq!(native("native-title", &input), expected);

        let mut input = String::new();
        let mut profiles = Vec::new();
        for continuation in [false, true] {
            for unlocked in [false, true] {
                for page in [0, 8] {
                    let avail = TitleAvailability {
                        continue_available: continuation,
                        extra_unlocked: unlocked,
                    };
                    for selected in 0..active(avail, page).rows.len() {
                        for permission in [false, true] {
                            for blocked in [false, true] {
                                for mask in 0..16u32 {
                                    let mut model = active(avail, page);
                                    model.select(selected);
                                    input.push_str(&format!(
                                        "M {} {} {page} 0\nS {selected}\nI {} {} {} {}\n",
                                        u8::from(continuation),
                                        u8::from(unlocked),
                                        u8::from(permission),
                                        u8::from(blocked),
                                        ((mask & 1) << 28) | ((mask & 2) << 28),
                                        ((mask & 4) << 23) | ((mask & 8) << 21)
                                    ));
                                    if permission && !blocked {
                                        model.input(TitleInput {
                                            confirm: mask & 1 != 0,
                                            back: mask & 2 != 0,
                                            down: mask & 4 != 0,
                                            up: mask & 8 != 0,
                                        });
                                    }
                                    let confirm = permission
                                        && !blocked
                                        && (mask & 1 != 0 || (mask & 2 != 0 && page == 8));
                                    let argument = if mask & 1 != 0 {
                                        selected as i128
                                    } else {
                                        u32::MAX as i128
                                    };
                                    profiles.push((
                                        model.selected.unwrap() as i128,
                                        confirm,
                                        argument,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        let output = native("native-title", &input);
        assert_eq!(output.len(), profiles.len() * 2);
        for (i, (selected, confirm, argument)) in profiles.iter().enumerate() {
            let row = &output[i * 2 + 1];
            assert_eq!(
                &row[..4],
                &[b'I' as i128, *selected, 0, 1],
                "input profile {i}"
            );
            assert_eq!(row[4], if *confirm { 2 } else { 0 }, "input profile {i}");
            if *confirm {
                assert_eq!(row[5], *argument, "input profile {i}");
            }
        }

        let mut input = String::new();
        let mut actions = Vec::new();
        for page in [0, 8] {
            let avail = TitleAvailability {
                continue_available: true,
                extra_unlocked: true,
            };
            let base = active(avail, page);
            for row in 0..base.rows.len() {
                input.push_str(&format!("M 1 1 {page} 0\nD {row}\n"));
                let item = base.rows[row];
                let expected = match item {
                    TitleItem::NewGame => (1, 0),
                    TitleItem::Continue => (1, -1),
                    TitleItem::Load => (4, 2),
                    TitleItem::Config => (4, 3),
                    TitleItem::Manual => (4, 7),
                    TitleItem::CgGallery => (3, 0x14d750),
                    TitleItem::MusicGallery => (3, 0x14c390),
                    TitleItem::BustupGallery => (3, 0x14cdb0),
                    TitleItem::Extra | TitleItem::Back => (0, 0),
                };
                actions.push((expected, item));
            }
        }
        let output = native("native-title", &input);
        for (i, ((action, argument), item)) in actions.iter().enumerate() {
            assert_eq!(
                output[i * 3 + 1],
                vec![b'D' as i128, *action, *argument, *item as i128]
            );
        }

        let output = native(
            "native-title",
            "M 1 2 0 0\nP 19 20 21 22\nA -1 1\nA -1 0\nA 0 0\nA 65535 0\n",
        );
        assert_eq!(output[1][1..], [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            output[2][1..],
            [
                0,
                1,
                1,
                19,
                20,
                21,
                22,
                12,
                8,
                14475489402745689801,
                1555233060020514401
            ]
        );
        assert_eq!(
            output[3][1..],
            [
                0,
                1,
                1,
                0,
                0,
                65244,
                0,
                0,
                0,
                14695981039346656037,
                14695981039346656037
            ]
        );
        assert_eq!(output[4][5], 65535);

        let fade_deltas = [0.0, 0.25, 1.0, 2.5, 120.0, 1000.0];
        let fade_input = fade_deltas
            .iter()
            .map(|v| format!("{v}\n"))
            .collect::<String>();
        let mut fades = fade_deltas.map(|_| {
            let mut model = active(TitleAvailability::default(), 0);
            model.confirm_selected();
            model.update(100.0);
            assert_eq!(model.phase(), TitlePhase::Fading);
            model
        });
        let fade_output = native("native-title-fade", &fade_input);
        for row in &fade_output {
            assert_eq!(row[0], b'F' as i128);
            let case = row[1] as usize - 1;
            let model = &mut fades[case];
            let action = if row[2] == 0 {
                None
            } else {
                model.update(fade_deltas[case])
            };
            assert_eq!(row[3], i128::from(model.phase() != TitlePhase::Handoff));
            assert_eq!(row[4], model.blackout().to_bits() as i128);
            assert_eq!(row[5], i128::from(action.is_some()));
        }
        assert_eq!(fade_output.len(), 6 * 122);
        eprintln!(
            "Ger title native parent fade: 6 deltas/732 exact samples, literal-one clock and publication after previous-frame tween completion"
        );

        let mut input = String::new();
        let mut hit_expected = Vec::new();
        let model = active(TitleAvailability::default(), 0);
        input.push_str("M 0 0 0 0\n");
        for row in 0..model.rows.len() {
            let [x, y, w, h] = button_rect(row);
            for px in [x - 0.125, x, x + w - 0.125, x + w] {
                for py in [y - 0.125, y, y + h - 0.125, y + h] {
                    input.push_str(&format!("H {px} {py}\n"));
                    hit_expected.push(model.point(px, py).map_or(-1, |v| v as i128));
                }
            }
        }
        let output = native("native-title", &input);
        for (i, expected) in hit_expected.iter().enumerate() {
            assert_eq!(output[i + 1], vec![b'H' as i128, *expected]);
        }

        let mut input = String::new();
        let mut motion = Vec::new();
        for avail in [
            TitleAvailability::default(),
            TitleAvailability {
                continue_available: true,
                extra_unlocked: false,
            },
            TitleAvailability {
                continue_available: true,
                extra_unlocked: true,
            },
        ] {
            let rows = active(avail, 0).rows.len();
            for mode in [0, 1] {
                for selected in 0..rows {
                    for delta in [0.25, 1.0, 2.5, 100.0] {
                        input.push_str(&format!("{mode} {rows} {selected} {delta}\n"));
                        let mut model = active(avail, 0);
                        model.select(selected);
                        if mode == 0 {
                            model.transition(TitlePhase::Entering);
                        } else {
                            model.confirm_selected();
                            // Native motion fixture stubs Decide. Keep the same
                            // outgoing rows while comparing its final sample.
                            model.pending = Some(TitleItem::NewGame);
                        }
                        motion.push((model, delta));
                    }
                }
            }
        }
        let output = native("native-title-motion", &input);
        let mut samples = 0;
        for row in output {
            if row[0] == b'C' as i128 {
                continue;
            }
            assert_eq!(row[0], b'T' as i128);
            let (model, delta) = &mut motion[row[1] as usize - 1];
            if row[2] > 0 {
                model.update(*delta);
            }
            let busy = matches!(model.phase, TitlePhase::Entering | TitlePhase::Leaving);
            assert_eq!(
                row[3],
                i128::from(busy),
                "motion case{} frame{} delta{delta}",
                row[1],
                row[2]
            );
            for i in 0..model.rows.len() {
                assert_eq!(
                    row[4 + i * 2],
                    model.row_alpha(i).to_bits() as i128,
                    "motion case{} frame{} row{i}",
                    row[1],
                    row[2]
                );
                assert_eq!(
                    row[5 + i * 2],
                    model.row_flash(i).to_bits() as i128,
                    "motion flash case{} frame{} row{i}",
                    row[1],
                    row[2]
                );
                samples += 2;
            }
        }
        eprintln!(
            "Ger title native: {menu_cases} menu profiles, {} input profiles, {} actions, {} pointer boundaries, {} motion profiles, {samples} exact alpha/flash samples; native ADV scalar/vector/fade gates",
            profiles.len(),
            actions.len(),
            hit_expected.len(),
            motion.len()
        );
    }
}
