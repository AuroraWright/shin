use super::*;
use shin_core::time::TweenOptions;

fn queued(pattern: usize) -> Tweener {
    let mut tween = Tweener::new(0.0);
    for index in 1..=3 {
        tween.enqueue_with_options(
            index as f32,
            Tween::linear(Ticks::from_f32(1.0)),
            TweenOptions {
                prohibit_fast_forward: pattern == index,
                ignore_wait: false,
            },
        );
    }
    tween.update(Ticks::from_f32(0.25));
    tween
}

fn model(kind: usize, pattern: usize) -> MurdererModel {
    let mut model = MurdererModel::new(20);
    if kind != 0 {
        assert!(model.open_menu(kind - 1));
    }
    model.top_wait = queued(pattern);
    model.back_feedback = queued(pattern);
    model.hint_scroll_motion = queued(pattern);
    model.purple_list_motion = queued(pattern);
    model.purple_text_scroll = 1.0;
    for group in [
        &mut model.top_row_focus[..],
        &mut model.top_row_flash[..],
        &mut model.replay_focus[..],
        &mut model.replay_row_flash[..],
        &mut model.hint_focus[..],
        &mut model.hint_row_flash[..],
        &mut model.purple_person_focus[..],
        &mut model.purple_person_flash[..],
        &mut model.purple_chapter_focus[..],
        &mut model.purple_chapter_flash[..],
        &mut model.purple_heading_flash[..],
        &mut model.accusation_row_focus[..],
        &mut model.accusation_row_flash[..],
    ] {
        for tween in group {
            *tween = queued(pattern);
        }
    }
    if matches!(model.menu, MurdererMenu::Replay | MurdererMenu::Hints) {
        model.menu_focus = 0;
        model.result = Some(if kind == 1 { 0 } else { 6 });
        model.response_completion = Some(ResponseCompletion {
            phase: ResponsePhase::Flash,
            flash: queued(pattern),
            timer: queued(pattern),
        });
    }
    if model.menu == MurdererMenu::Accusation {
        model.result = Some(27);
        model.completion = Some(AccusationCompletion::WhiteFlashWait {
            remaining: 45.0,
            elapsed: 7.5,
            correct: true,
        });
    }
    model.take_sounds();
    model
}

fn snapshot(model: &MurdererModel) -> Vec<u32> {
    let mut words = Vec::new();
    let mut add =
        |tween: &Tweener| words.extend([tween.value().to_bits(), u32::from(!tween.is_idle())]);
    match model.menu {
        MurdererMenu::Top => {
            add(&model.top_wait);
            for row in 0..6 {
                add(&model.top_row_focus[row]);
                add(&model.top_row_flash[row]);
            }
        }
        MurdererMenu::Replay => {
            add(&model.back_feedback);
            add(&model.response_completion.as_ref().unwrap().timer);
            for row in 0..6 {
                add(if row == 0 {
                    &model.response_completion.as_ref().unwrap().flash
                } else {
                    &model.replay_row_flash[row]
                });
                add(&model.replay_focus[row]);
            }
        }
        MurdererMenu::Rules => add(&model.back_feedback),
        MurdererMenu::Hints => {
            add(&model.response_completion.as_ref().unwrap().timer);
            for row in 0..21 {
                add(&model.hint_focus[row]);
                add(&model.hint_row_flash[row]);
            }
            add(&model.hint_scroll_motion);
            add(&model.back_feedback);
        }
        MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter => {
            add(&model.back_feedback);
            add(&model.purple_list_motion);
            add(&Tweener::new(model.purple_text_scroll));
            if model.menu == MurdererMenu::PurpleByPerson {
                for row in 0..17 {
                    add(&model.purple_person_focus[row]);
                    add(&model.purple_person_flash[row]);
                }
            } else {
                let mut statement = 0;
                for entry in CHAPTERS {
                    if let Some(heading) = entry.heading() {
                        // Heading focus has no queued transition in this fixture.
                        add(&Tweener::new(1.0));
                        add(&model.purple_heading_flash[heading]);
                    } else {
                        add(&model.purple_chapter_focus[statement]);
                        add(&model.purple_chapter_flash[statement]);
                        statement += 1;
                    }
                }
            }
        }
        MurdererMenu::Accusation => {
            // Native +28 is the zero-valued hold, +1a8 is the white fade.
            // The port stores them together in WhiteFlashWait.
            let remaining = match model.completion {
                Some(AccusationCompletion::WhiteFlashWait { remaining, .. }) => remaining,
                _ => panic!(),
            };
            words.extend([0, u32::from(remaining > 0.0)]);
            for row in 0..18 {
                words.extend([
                    model.accusation_row_focus[row].value().to_bits(),
                    u32::from(!model.accusation_row_focus[row].is_idle()),
                    model.accusation_row_flash[row].value().to_bits(),
                    u32::from(!model.accusation_row_flash[row].is_idle()),
                ]);
            }
            words.extend([
                model.white_flash_opacity().to_bits(),
                0,
                model.back_feedback.value().to_bits(),
                u32::from(!model.back_feedback.is_idle()),
            ]);
        }
    }
    words.insert(0, (words.len() / 2) as u32);
    words
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[ignore = "executes supplied original/1.03 parent and child fast-forward callbacks"]
fn reference_native_murderer_fast_forward() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = std::env::temp_dir().join(format!("shin-murderer-forward-{}", std::process::id()));
    let compile = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-fast-forward.c"))
        .args(["-lm", "-o"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut request = String::new();
    let mut expected = Vec::new();
    for kind in 0..7 {
        for pattern in 0..4 {
            for repeat in 1..=3 {
                let mut model = model(kind, pattern);
                let inactive = model.hint_focus[0].value().to_bits();
                for _ in 0..repeat {
                    model.fast_forward();
                }
                assert!(model.take_sounds().is_empty());
                if matches!(kind, 1 | 5 | 6) {
                    assert!(model.poll_result().is_none());
                }
                if kind != 5 {
                    assert_eq!(
                        model.hint_focus[0].value().to_bits(),
                        inactive,
                        "Only active child tween state is forwarded"
                    );
                }
                expected.push(snapshot(&model));
                request += &format!("0 {kind} {pattern} 0 {repeat} 0\n");
            }
        }
    }
    for active in -1..7 {
        for staged in -1..7 {
            for wipe in 0..=1 {
                for repeat in 1..=3 {
                    for cleanup in 0..=1 {
                        let chosen = if staged >= 0 { staged } else { active };
                        let mut events = Vec::new();
                        if staged >= 0 && active >= 0 {
                            events.push(10 + active as u32);
                        }
                        for _ in 0..repeat {
                            if chosen >= 0 {
                                events.push(20 + chosen as u32);
                            }
                            if wipe != 0 {
                                events.push(50);
                            }
                        }
                        if cleanup != 0 {
                            if chosen >= 0 {
                                events.push(40 + chosen as u32);
                            }
                            if wipe != 0 {
                                events.extend([51, 52]);
                            }
                        }
                        let mut words = vec![
                            (chosen + 1) as u32,
                            0,
                            wipe,
                            wipe,
                            if wipe != 0 {
                                60f32.to_bits()
                            } else {
                                7.5f32.to_bits()
                            },
                            0.25f32.to_bits(),
                            53,
                            1,
                            wipe * (1 - cleanup),
                            wipe * (1 - cleanup),
                            events.len() as u32,
                        ];
                        words.extend(events);
                        expected.push(words);
                        request += &format!("1 {active} {staged} {wipe} {repeat} {cleanup}\n");
                        // The model has a live child at all times. Native UI
                        // stages Top on return, and submenus from the live Top.
                        if active >= 0 && (staged <= 0 || active == 0) {
                            let mut model = MurdererModel::new(20);
                            if active > 0 {
                                assert!(model.open_menu((active - 1) as usize));
                            }
                            if staged == 0 {
                                model.pending_top = true;
                            }
                            if staged > 0 {
                                model.pending_menu = Some((staged - 1) as usize);
                                model.top_phase = TopPhase::Staged;
                            }
                            if wipe != 0 {
                                model.capture_wipe(MurdererWipeKind::Default, 60.0);
                            }
                            model.take_sounds();
                            for _ in 0..repeat {
                                model.fast_forward();
                            }
                            assert_eq!(
                                model.menu,
                                [
                                    MurdererMenu::Top,
                                    MurdererMenu::Replay,
                                    MurdererMenu::Rules,
                                    MurdererMenu::PurpleByPerson,
                                    MurdererMenu::PurpleByChapter,
                                    MurdererMenu::Hints,
                                    MurdererMenu::Accusation
                                ][chosen as usize]
                            );
                            assert!(model.pending_menu.is_none() && !model.pending_top);
                            assert_eq!(model.wipe.is_some(), wipe != 0);
                            if let Some(wipe) = &model.wipe {
                                assert_eq!(wipe.elapsed, 60.0);
                            }
                            assert!(model.take_sounds().is_empty());
                            if cleanup != 0 {
                                model.update_with_wipe_permission(0.0, false);
                                assert!(model.wipe.is_none());
                            }
                        }
                    }
                }
            }
        }
    }
    for image in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut native = Command::new(&binary)
            .arg(root.join("game-data/umineko").join(image))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = native.stdin.take().unwrap();
        let request = request.clone();
        let writer = std::thread::spawn(move || input.write_all(request.as_bytes()).unwrap());
        let output = native.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{image}: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.lines().count(), expected.len());
        for (index, (line, expected)) in output.lines().zip(&expected).enumerate() {
            let actual: Vec<u32> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            assert_eq!(&actual, expected, "{image}: case{index}");
        }
        let mut mutated = std::fs::read(root.join("game-data/umineko").join(image)).unwrap();
        let end = mutated.len() - 1;
        mutated[end] ^= 1;
        let mutant = binary.with_extension("mutant");
        std::fs::write(&mutant, mutated).unwrap();
        let rejected = Command::new(&binary).arg(&mutant).output().unwrap();
        std::fs::remove_file(mutant).unwrap();
        assert_eq!(rejected.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&rejected.stderr)
                .contains("Unreviewed exact fast-forward image")
        );
        println!(
            "{image}: 84 exact native child/port tween cases and768 native parent staged-child/forward/retirement traces; protected queues, repeated calls, delayed frozen/wiper release and untouched parent properties/response/phase pass"
        );
    }
    std::fs::remove_file(binary).unwrap();
}

#[test]
fn fast_forward_preserves_response_phases_until_update() {
    let mut replay = MurdererModel::new(20);
    replay.open_menu(0);
    assert!(replay.replay_chapter(0));
    replay.take_sounds();
    replay.fast_forward();
    assert!(replay.poll_result().is_none());
    assert!(replay.take_sounds().is_empty());
    replay.update(0.0);
    assert!(replay.poll_result().is_none());
    replay.fast_forward();
    assert!(replay.poll_result().is_none());
    replay.update(0.0);
    assert_eq!(replay.poll_result(), Some(0));
    let mut hints = MurdererModel::new(20);
    hints.open_menu(4);
    assert!(hints.request_hint(20));
    hints.take_sounds();
    hints.fast_forward();
    assert!(hints.poll_result().is_none());
    hints.update(0.0);
    assert_eq!(hints.poll_result(), Some(26));
    assert!(hints.take_sounds().is_empty());
    for correct in [false, true] {
        let mut accusation = MurdererModel::new(20);
        accusation.open_menu(5);
        accusation.suspects = std::array::from_fn(|i| correct && SUSPECT_FLAGS[i] & 0x40 != 0);
        assert!(accusation.activate_accusation_entry(SUBMIT_ENTRY));
        accusation.take_sounds();
        accusation.fast_forward();
        assert!(accusation.poll_result().is_none());
        assert!(matches!(
            accusation.completion,
            Some(AccusationCompletion::SubmitFlash { .. })
        ));
        accusation.update(0.0);
        assert_eq!(accusation.white_flash_opacity(), 1.0);
        accusation.fast_forward();
        assert_eq!(accusation.white_flash_opacity(), 0.0);
        assert!(accusation.poll_result().is_none());
        assert!(accusation.take_sounds().is_empty());
        accusation.update(0.0);
        if !correct {
            assert_eq!(accusation.take_sounds(), ["ahaha"]);
            assert!(accusation.poll_result().is_none());
            accusation.fast_forward();
            assert!(accusation.poll_result().is_none());
            accusation.update(0.0);
        }
        assert_eq!(
            accusation.poll_result(),
            Some(if correct { 27 } else { 48 })
        );
    }
}

#[test]
fn glass_fast_forward_retains_source_and_physics_until_update() {
    let mut model = MurdererModel::new(20);
    model.capture_wipe(MurdererWipeKind::Glass, 60.0);
    let before = model
        .wipe
        .as_ref()
        .unwrap()
        .glass
        .as_ref()
        .unwrap()
        .reference_motion();
    let source = model.wipe.as_ref().unwrap().source.menu;
    model.fast_forward();
    model.fast_forward();
    let wipe = model.wipe.as_ref().unwrap();
    assert_eq!(wipe.elapsed, 60.0);
    assert_eq!(wipe.source.menu, source);
    assert_eq!(wipe.glass.as_ref().unwrap().reference_motion(), before);
    assert!(model.menu_input_busy());
    model.update_with_wipe_permission(0.0, false);
    assert!(model.wipe.is_none());
}
