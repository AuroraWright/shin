use super::*;
#[test]
fn config_reset_hidden_settings_and_close_persistence_are_owned_until_acknowledged() {
    let mut s = Settings::default();
    s.v19 = 0;
    let mut m = ConfigController::new(s);
    m.update(10.);
    m.reset();
    m.update(7.);
    assert_eq!(m.take_requests(), []);
    m.update(0.);
    assert_eq!(m.take_requests(), [Request::ResetConfirmation]);
    m.reset_response(false, 0);
    assert_eq!(m.settings.v19, u32::MAX);
    assert_eq!(m.phase, Phase::ResetDialog);
    m.reset_response(true, 0);
    m.close();
    m.update(7.);
    m.update(0.);
    assert_eq!(m.take_requests(), [Request::Save]);
    m.update(100.);
    assert_eq!(m.phase, Phase::WaitingSave);
    m.save_failed();
    assert_eq!(m.phase, Phase::Active);
    m.close();
    m.update(7.);
    m.update(0.);
    assert_eq!(m.take_requests(), [Request::Save]);
    m.save_finished();
    m.update(10.);
    assert_eq!(m.take_requests(), [Request::Close]);
    m.update(100.);
    assert!(m.take_requests().is_empty());
}
#[test]
fn config_pointer_edges_scroll_selection_and_uniform_controller_priority() {
    let mut m = ConfigController::new(Settings::default());
    m.update(10.);
    assert!(!m.probe(f32::NAN, 60.));
    assert!(m.probe(1874., 46.));
    assert_eq!(m.target, Target::None);
    assert!(m.probe(1766., 46.));
    assert_eq!(m.target, Target::Close);
    m.input(Input {
        up: true,
        down: true,
        reset: true,
        close: true,
        right: true,
        ..Default::default()
    });
    assert_eq!(m.phase, Phase::ResetFlash);
    m.phase = Phase::Active;
    m.select(12);
    for _ in 0..4 {
        m.update(0.);
    }
    assert_eq!(m.scroll(), 544.);
    assert_eq!(m.selected, 12);
    m.input(Input {
        left: true,
        right: true,
        ..Default::default()
    });
    assert_eq!(m.value(12), 0);
}
#[test]
#[ignore = "requires unchanged Ger executable, FNT6 fonts and native arm64 clang"]
fn ger_native_config_schema_widgets_and_controller() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let local = root.join("reverse-engineering/gerokasu2/config");
    std::fs::create_dir_all(&local).unwrap();
    let binary = local.join("native-config-model-test");
    let compile = Command::new("clang")
        .args(["-O2", "-arch", "arm64"])
        .arg(root.join("reverse-engineering/gerokasu2/native-config-model.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut source = String::new();
    let mut expected = vec![];
    let mut m = ConfigController::new(Settings::default());
    m.update(10.);
    expected.push((
        m.selected,
        (0..13).map(|i| m.value(i)).collect::<Vec<_>>(),
        0,
        0,
    ));
    for row in 0..13 {
        let values: &[u8] = if matches!(ROWS[row].control, Control::Slider(_)) {
            &[0, 1, 4, 5, 50, 95, 99, 100]
        } else {
            &[0, 1]
        };
        for &value in values {
            for input in [
                Input::default(),
                Input {
                    left: true,
                    ..Default::default()
                },
                Input {
                    right: true,
                    ..Default::default()
                },
                Input {
                    left: true,
                    right: true,
                    ..Default::default()
                },
                Input {
                    down: true,
                    reset: true,
                    ..Default::default()
                },
                Input {
                    up: true,
                    close: true,
                    ..Default::default()
                },
                Input {
                    reset: true,
                    close: true,
                    ..Default::default()
                },
                Input {
                    close: true,
                    ..Default::default()
                },
            ] {
                let commands = [
                    format!("C\n"),
                    format!("S {row}\n"),
                    format!("F\n"),
                    format!("V {row} {value}\n"),
                    format!("C\n"),
                    format!(
                        "I 1 0 {:x} {:x}\n",
                        (u32::from(input.reset) << 7) | (u32::from(input.close) << 29),
                        (u32::from(input.up) << 24)
                            | (u32::from(input.down) << 25)
                            | (u32::from(input.left) << 26)
                            | (u32::from(input.right) << 27)
                    ),
                ];
                for (index, cmd) in commands.into_iter().enumerate() {
                    let mut action = 0;
                    let mut cues = 0;
                    match index {
                        0 | 4 => m.phase = Phase::Active,
                        1 => m.select(row),
                        2 => m.scroll.snap(0.),
                        3 => {
                            set(&mut m.settings, ROWS[row].field, value);
                            if ROWS[row].field == 18 && value == 1 {
                                action = 3;
                            }
                        }
                        5 => {
                            m.input(input);
                            action = match m.phase {
                                Phase::ResetFlash => 1,
                                Phase::CloseFlash => 2,
                                Phase::NoticeDialog => 3,
                                _ => 0,
                            };
                            cues = m.take_sounds().len();
                        }
                        _ => {}
                    }
                    source.push_str(&cmd);
                    expected.push((
                        m.selected,
                        (0..13).map(|i| m.value(i)).collect::<Vec<_>>(),
                        action,
                        cues,
                    ));
                }
            }
        }
    }
    let input_path = local.join("controller-input.txt");
    std::fs::write(&input_path, &source).unwrap();
    let mut child = Command::new(&binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(local.join("controller-output.txt")).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = std::fs::read_to_string(local.join("controller-output.txt")).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (line, (selected, values, action, cues)) in lines.into_iter().zip(expected.iter()) {
        let words: Vec<_> = line.split_whitespace().collect();
        let number = |i: usize| words[i].parse::<usize>().unwrap();
        assert_eq!(number(1), *selected, "{line}");
        assert_eq!(number(3), 1340);
        assert_eq!(number(4), 13);
        let mut at = 5;
        for (i, row) in ROWS.iter().enumerate() {
            let slider = matches!(row.control, Control::Slider(_));
            assert_eq!(number(at), usize::from(slider));
            assert_eq!(number(at + 1), row.y as usize);
            assert_eq!(number(at + 2), row.title as usize);
            assert_eq!(number(at + 4), values[i] as usize, "{line}");
            at += 5;
            if let Control::Buttons(labels) = row.control {
                assert_eq!(number(at), 2);
                assert_eq!([number(at + 1), number(at + 2)], labels.map(usize::from));
                at += 3;
            }
        }
        assert_eq!(words[at], "S");
        at += 25;
        assert_eq!(words[at], "E");
        assert_eq!(number(at + 1), *action, "{line}");
        assert_eq!(number(at + 2), *cues, "{line}");
    }
    eprintln!(
        "GER_CONFIG_NATIVE_COMPARED snapshots={} rows={} controller_profiles={}",
        expected.len(),
        expected.len() * 13,
        (expected.len() - 1) / 6
    );
}
#[test]
#[ignore = "requires unchanged Ger executable, FNT6 fonts and native arm64 clang"]
fn ger_native_config_original_parent_row_artwork() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let local = root.join("reverse-engineering/gerokasu2/config");
    std::fs::create_dir_all(&local).unwrap();
    let binary = local.join("native-config-render-test");
    let compile = Command::new("clang")
        .args(["-O2", "-arch", "arm64"])
        .arg(root.join("reverse-engineering/gerokasu2/native-config-render.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut quads = 0;
    for selected in 0..13 {
        let mut child = Command::new(&binary)
            .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
            .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{selected}\n").as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::write(
            local.join(format!("render-output-{selected}.txt")),
            &output.stdout,
        )
        .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = text
            .lines()
            .filter(|l| matches!(l.split_whitespace().nth(1), Some("2" | "3" | "5")))
            .collect();
        let mut m = ConfigController::new(Settings::default());
        m.update(10.);
        m.select(selected);
        m.scroll.snap(0.);
        let expected = artwork::quads(&m, crate::cli::Game::Gerokasu2);
        assert_eq!(lines.len(), expected.len());
        for (line, q) in lines.into_iter().zip(&expected) {
            let words: Vec<_> = line.split_whitespace().collect();
            assert_eq!(words[3], q.texture);
            let bits = |i: usize| words[21 + i].parse::<u32>().unwrap();
            assert_eq!(
                [bits(0), bits(1), bits(18), bits(19)],
                q.rect.map(f32::to_bits),
                "rect {} selected{selected}",
                q.texture
            );
            assert_eq!(
                [bits(4), bits(5), bits(22), bits(23)],
                q.uv.map(f32::to_bits),
                "uv {} selected{selected}",
                q.texture
            );
        }
        quads += expected.len();
    }
    eprintln!(
        "GER_CONFIG_NATIVE_ARTWORK profiles=13 quads={quads} exact_rect_uv_words={}",
        quads * 8
    );
}

#[test]
#[ignore = "requires unchanged Ger executable, FNT6 fonts and native arm64 clang"]
fn ger_native_config_pointer_edges() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let local = root.join("reverse-engineering/gerokasu2/config");
    std::fs::create_dir_all(&local).unwrap();
    let binary = local.join("native-config-pointer-test");
    let compile = Command::new("clang")
        .args(["-O2", "-arch", "arm64"])
        .arg(root.join("reverse-engineering/gerokasu2/native-config-model.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(compile.status.success());
    let mut source = String::new();
    let mut expected = vec![];
    let mut m = ConfigController::new(Settings::default());
    m.update(10.);
    for y in [
        45., 46., 153., 154., 159., 160., 177., 178., 257., 258., 337., 338., 423., 424., 503.,
        504., 601., 602., 681., 682., 757., 758., 837., 838., 847., 848., 933., 934., 973., 974.,
        991., 992.,
    ] {
        for x in [
            159., 160., 199., 200., 1001., 1002., 1049., 1050., 1253., 1254., 1477., 1478., 1489.,
            1490., 1713., 1714., 1759., 1760., 1765., 1766., 1873., 1874.,
        ] {
            source.push_str(&format!("H {x} {y}\n"));
            let accepted = m.probe(x, y);
            expected.push((m.target, accepted));
        }
    }
    let mut child = Command::new(&binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(local.join("pointer-output.txt")).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = std::fs::read_to_string(local.join("pointer-output.txt")).unwrap();
    for (line, (target, accepted)) in actual.lines().skip(1).zip(&expected) {
        let words: Vec<_> = line.split_whitespace().collect();
        let at = words.iter().rposition(|&w| w == "H").unwrap();
        let n = |i: usize| words[at + i].parse::<i32>().unwrap();
        assert_eq!(n(4) != 0, *accepted, "{line}");
        if !accepted {
            continue;
        }
        let expected = match target {
            Target::Close => (-1, -1, 1),
            Target::Reset => (-1, -1, 2),
            Target::Heading(i) => (-1, *i as i32, -1),
            Target::Row(i, _) => ((*i + 1) as i32, -1, -1),
            Target::Content => (0, -1, -1),
            Target::None => (-1, -1, 0),
        };
        assert_eq!((n(1), n(2), n(3)), expected, "{line}");
    }
    eprintln!("GER_CONFIG_NATIVE_POINTER cases={}", expected.len());
}
#[test]
#[ignore = "requires unchanged Ger executable, FNT6 fonts and native arm64 clang"]
fn ger_native_config_original_open_close_clocks() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let local = root.join("reverse-engineering/gerokasu2/config");
    std::fs::create_dir_all(&local).unwrap();
    let binary = local.join("native-config-clock-test");
    let compile = Command::new("clang")
        .args(["-O2", "-arch", "arm64"])
        .arg(root.join("reverse-engineering/gerokasu2/native-config-model.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(compile.status.success());
    let mut source = String::new();
    let mut expected = vec![];
    let mut m = ConfigController::new(Settings::default());
    for _ in 0..8 {
        m.phase = Phase::Opening;
        for dt in [0., 0.25, 1., 2.5, 0.1, 0.3, 0.6, 9., 1000.] {
            if m.phase == Phase::Active {
                m.phase = Phase::Opening;
            }
            source.push_str(&format!("O {dt}\n"));
            m.update(dt);
            expected.push(m.progress.to_bits());
        }
        m.phase = Phase::Closing;
        for dt in [0., 0.25, 1., 2.5, 0.1, 0.3, 0.6, 9., 1000.] {
            if m.phase == Phase::Closed {
                m.phase = Phase::Closing;
            }
            source.push_str(&format!("Z {dt}\n"));
            m.update(dt);
            expected.push(m.progress.to_bits());
        }
    }
    let mut child = Command::new(&binary)
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(local.join("clock-output.txt")).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = std::fs::read_to_string(local.join("clock-output.txt")).unwrap();
    let mut count = 0;
    for (line, expected) in actual.lines().skip(1).zip(expected) {
        let actual = line
            .split_whitespace()
            .last()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        assert_eq!(actual, expected, "{line}");
        count += 1;
    }
    assert_eq!(count, 144);
    eprintln!("GER_CONFIG_NATIVE_CLOCK samples={count}");
}
#[test]
fn gerokasu_config_adds_sliding_bottom_band() {
    use crate::cli::Game;
    let mut m = ConfigController::new(Settings::default());
    m.progress = 0.5;
    let base = artwork::quads(&m, Game::Gerokasu2);
    let ger = artwork::quads(&m, Game::Gerokasu);
    assert!(base.iter().all(|q| q.texture != "bottom"));
    assert_eq!(ger.len(), base.len() + 1);
    let i = ger.iter().position(|q| q.texture == "bottom").unwrap();
    assert_eq!(
        (ger[i - 1].texture, ger[i + 1].texture),
        ("default", "help_config")
    );
    assert_eq!(ger[i].rect, [0., 1036., 1920., 1124.]);
    assert_eq!(ger[i].uv, [0., 0., 1., 1.]);
    assert_eq!((ger[i].alpha, ger[i].space), (1., artwork::Space::Canvas));
}
