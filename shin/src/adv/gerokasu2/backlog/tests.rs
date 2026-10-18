use super::*;
#[test]
fn ger_backlog_geometry_update_preserves_external_selection_and_entire_scroll() {
    let rows: Vec<_> = (0..8)
        .map(|i| Row {
            marker: i == 3,
            voiced: false,
            top: 0.,
            height: 0.,
        })
        .collect();
    let mut c = Controller::new(rows.clone(), 4, 4..5, 0., 0., 0.).unwrap();
    assert!(c.select_external(7, 0., false));
    c.advance_scroll();
    // The retained owner can lie beyond bounds supplied by a newer snapshot;
    // geometry publication never clamps or replaces its four-tick tween.
    c.scroll.current = 123.25;
    c.scroll.start = 117.125;
    c.scroll.target = 201.375;
    let before = (
        c.scroll.current.to_bits(),
        c.scroll.start.to_bits(),
        c.scroll.target.to_bits(),
        c.scroll.elapsed.to_bits(),
        c.scroll.busy,
    );
    let mut g = history::Geometry {
        rows: rows
            .iter()
            .map(|r| history::GeometryRow {
                marker: r.marker,
                voice_count: 0,
                top: 0.,
                height: 0.,
            })
            .collect(),
        selected: 4,
        allowed: 3..6,
        initial_scroll: -999.,
        minimum: -98.25,
        maximum: -98.25,
        bottom: 188.,
    };
    g.rows[3].top = -98.25;
    g.rows[3].height = 90.25;
    g.rows[4].height = 90.;
    g.rows[4].voice_count = 1;
    g.rows[5].top = 98.;
    g.rows[5].height = 90.;
    c.update_geometry(&g).unwrap();
    assert_eq!(c.selected(), 7);
    assert_eq!(c.allowed(), 3..6);
    assert!(c.voice_available(4));
    assert_eq!(
        before,
        (
            c.scroll.current.to_bits(),
            c.scroll.start.to_bits(),
            c.scroll.target.to_bits(),
            c.scroll.elapsed.to_bits(),
            c.scroll.busy
        )
    );
    assert_eq!(c.rows()[3].top, -98.25);
    let saved = c.rows().to_vec();
    for invalid in 0..5 {
        let mut bad = g.clone();
        match invalid {
            0 => {
                bad.rows.pop();
            }
            1 => bad.rows[3].marker = false,
            2 => bad.rows[4].height = f32::NAN,
            3 => bad.allowed = 3..3,
            _ => bad.minimum = bad.maximum + 1.,
        }
        assert!(c.update_geometry(&bad).is_err());
        assert_eq!(c.rows(), saved);
        assert_eq!(c.selected(), 7);
        assert_eq!(c.allowed(), 3..6);
        assert_eq!(
            before,
            (
                c.scroll.current.to_bits(),
                c.scroll.start.to_bits(),
                c.scroll.target.to_bits(),
                c.scroll.elapsed.to_bits(),
                c.scroll.busy
            )
        );
    }
}
#[test]
fn ger_backlog_priority_clamping_and_literal_scroll_clock() {
    let rows = (0..20)
        .map(|i| Row {
            marker: i % 5 == 0,
            voiced: i % 2 == 0,
            top: i as f32 * 110.0,
            height: 96.0,
        })
        .collect();
    let mut c = Controller::new(rows, 0, 0..20, 0.0, 0.0, 1354.0).unwrap();
    assert_eq!(
        c.input(Input {
            permission: true,
            repeated: 1 << 24,
            pressed: 1 << 29,
            ..Default::default()
        }),
        [Event::Close]
    );
    assert_eq!(
        c.input(Input {
            permission: true,
            repeated: (1 << 24) | (1 << 25),
            pressed: 0,
            ..Default::default()
        }),
        []
    );
    assert_eq!(
        c.input(Input {
            permission: true,
            repeated: 1 << 11,
            ..Default::default()
        }),
        [Event::Move]
    );
    assert_eq!(c.selected(), 5);
    for _ in 0..4 {
        c.advance_scroll();
    }
    assert_eq!(
        c.input(Input {
            permission: true,
            pressed: (1 << 6) | (1 << 28) | (1 << 7),
            ..Default::default()
        }),
        [Event::ChapterSelector]
    );
    assert!(!c.voice_available(0));
    assert!(c.voice_available(2));
    assert!(!c.voice_available(1));
}

#[test]
fn ger_backlog_external_rows_search_before_clamping_without_reversed_slices() {
    let rows: Vec<_> = (0..16)
        .map(|i| Row {
            marker: [2, 6, 13].contains(&i),
            voiced: false,
            top: i as f32 * 110.,
            height: 96.,
        })
        .collect();
    // Markers outside the allowed range deliberately distinguish searching
    // from the actual external row from pre-clamping before marker navigation.
    for (external, bit, want) in [
        (0, 11, 3),
        (0, 10, 3),
        (15, 10, 11),
        (15, 11, 11),
        (2, 11, 6),
        (2, 10, 3),
        (12, 10, 6),
        (12, 11, 11),
    ] {
        let mut c = Controller::new(rows.clone(), 6, 3..12, 330., 330., 474.).unwrap();
        assert!(c.select_external(external, if external < 3 { 330. } else { 474. }, true));
        assert_eq!(
            c.input(Input {
                permission: true,
                repeated: 1 << bit,
                pressed: 1 << 29,
                ..Default::default()
            }),
            [Event::Move]
        );
        assert_eq!(c.selected(), want, "external{external} bit{bit}");
    }
}

#[test]
#[ignore = "executes bounded original Ger MessageBrowser controls; CPU only"]
fn ger_native_backlog_controls_match() {
    use std::{
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };
    let root = std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("PWD")));
    let native = std::env::var_os("SHIN_GEROKASU2_BACKLOG_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("reverse-engineering/gerokasu2/backlog/native-controls"));
    let mut script = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    fn snapshot(c: &Controller, events: &[Event]) -> Vec<u32> {
        let mut event = 0;
        let mut row = 0;
        let mut moves = 0;
        for e in events {
            match *e {
                Event::Move => moves += 1,
                Event::Close => event = 1,
                Event::ChapterSelector => event = 2,
                Event::ReplayVoice { row: r } => {
                    event = 3;
                    row = r as u32
                }
                Event::QuickLoad { row: r } => {
                    event = 4;
                    row = r as u32
                }
            }
        }
        vec![
            c.selected() as u32,
            c.scroll().to_bits(),
            u32::from(c.scroll_busy()),
            event,
            row,
            moves,
        ]
    }
    for origin in [-300.0, 0.0] {
        for seed in [0, 1, 2] {
            for allowed in [0..32, 2..29] {
                for which in [0, 1, 2] {
                    let mut top = origin;
                    let rows: Vec<_> = (0..32)
                        .map(|i| {
                            let height = [60.0, 96.0, 180.0][(i + seed) % 3];
                            let r = Row {
                                marker: i % 7 == 0,
                                voiced: i % 2 == 0,
                                top,
                                height,
                            };
                            top += height + 14.0;
                            r
                        })
                        .collect();
                    let selected = [
                        allowed.start,
                        (allowed.start + allowed.end) / 2,
                        allowed.end - 1,
                    ][which];
                    let min = rows[allowed.start].top;
                    let max = (rows[allowed.end - 1].top + rows[allowed.end - 1].height
                        - VIEW_HEIGHT)
                        .max(min);
                    let start = (rows[selected].top - 500.0).clamp(min, max);
                    let mut c =
                        Controller::new(rows.clone(), selected, allowed.clone(), start, min, max)
                            .unwrap();
                    script.push_str(&format!(
                        "N 32 {selected} {} {} {start} {min} {max}\n",
                        allowed.start, allowed.end
                    ));
                    for r in rows {
                        script.push_str(&format!(
                            "{} {} {} {}\n",
                            u8::from(r.marker),
                            u8::from(r.voiced),
                            r.top,
                            r.height
                        ));
                    }
                    expected.push(snapshot(&c, &[]));
                    profiles += 1;
                    let masks = [
                        0,
                        1 << 24,
                        1 << 25,
                        1 << 26,
                        1 << 27,
                        1 << 10,
                        1 << 11,
                        (1 << 24) | (1 << 25),
                        (1 << 10) | (1 << 11),
                    ];
                    for step in 0..160 {
                        let repeated = masks[(step * 7 + seed) % masks.len()];
                        let pressed = [
                            0,
                            1 << 29,
                            1 << 6,
                            1 << 28,
                            1 << 7,
                            (1 << 29) | (1 << 28),
                            (1 << 6) | (1 << 28),
                            (1 << 28) | (1 << 7),
                        ][(step * 3 + seed) % 8];
                        let axis = [0.0, 0.25, -0.5, 1.0, -1.0, 2.5][step % 6];
                        let permission = step % 13 != 0;
                        let captured = step % 17 == 0;
                        script.push_str(&format!(
                            "F {} {} {pressed:x} {repeated:x} {axis}\n",
                            u8::from(permission),
                            u8::from(captured)
                        ));
                        let events = c.input(Input {
                            permission,
                            captured,
                            pressed,
                            repeated,
                            axis,
                        });
                        expected.push(snapshot(&c, &events));
                        script.push_str("T\n");
                        c.advance_scroll();
                        expected.push(snapshot(&c, &[]));
                    }
                    for _ in 0..4 {
                        script.push_str("T\n");
                        c.advance_scroll();
                        expected.push(snapshot(&c, &[]));
                    }
                    for axis in [-1000.0, 1000.0, -0.25, -1.0, 0.5, 2.5, -2.5, 0.0] {
                        script.push_str(&format!("F 1 0 0 0 {axis}\n"));
                        let events = c.input(Input {
                            permission: true,
                            axis,
                            ..Default::default()
                        });
                        expected.push(snapshot(&c, &events));
                    }
                }
            }
        }
    }
    // Each control also starts from an idle tween. Mixed frame streams alone
    // can mask a direction behind the native four-tick input gate.
    for selected in [2, 16, 28] {
        for repeated in [
            0,
            1 << 24,
            1 << 25,
            1 << 26,
            1 << 27,
            1 << 10,
            1 << 11,
            (1 << 24) | (1 << 25),
            (1 << 10) | (1 << 11),
        ] {
            for pressed in [
                0,
                1 << 29,
                1 << 6,
                1 << 28,
                1 << 7,
                (1 << 29) | (1 << 6) | (1 << 28) | (1 << 7),
            ] {
                for (permission, captured) in [(true, false), (false, false), (true, true)] {
                    let rows: Vec<_> = (0..32)
                        .map(|i| Row {
                            marker: i % 7 == 0,
                            voiced: i % 2 == 0,
                            top: i as f32 * 110.0,
                            height: 96.0,
                        })
                        .collect();
                    let start = (rows[selected].top - 690.0).max(0.0);
                    let max = rows[31].top + 96.0 - VIEW_HEIGHT;
                    let mut c =
                        Controller::new(rows.clone(), selected, 2..29, start, 0.0, max).unwrap();
                    script.push_str(&format!("N 32 {selected} 2 29 {start} 0 {max}\n"));
                    for r in rows {
                        script.push_str(&format!(
                            "{} {} {} {}\n",
                            u8::from(r.marker),
                            u8::from(r.voiced),
                            r.top,
                            r.height
                        ));
                    }
                    expected.push(snapshot(&c, &[]));
                    profiles += 1;
                    for _ in 0..2 {
                        script.push_str(&format!(
                            "F {} {} {pressed:x} {repeated:x} 0\n",
                            u8::from(permission),
                            u8::from(captured)
                        ));
                        let events = c.input(Input {
                            permission,
                            captured,
                            pressed,
                            repeated,
                            axis: 0.0,
                        });
                        expected.push(snapshot(&c, &events));
                    }
                    for _ in 0..4 {
                        script.push_str("T\n");
                        c.advance_scroll();
                        expected.push(snapshot(&c, &[]));
                    }
                }
            }
        }
    }
    // A Chapter/pointer publication can leave a valid metadata row outside
    // Main's keyboard bounds. Test both sides, including markers that must be
    // inspected before the final clamp and absent-marker fallbacks.
    for marker_rows in [&[2, 6, 13][..], &[][..]] {
        for external in [0, 2, 12, 15] {
            for repeated in [
                0,
                1 << 24,
                1 << 25,
                1 << 26,
                1 << 27,
                1 << 10,
                1 << 11,
                (1 << 10) | (1 << 11),
            ] {
                let rows: Vec<_> = (0..16)
                    .map(|i| Row {
                        marker: marker_rows.contains(&i),
                        voiced: i % 2 == 0,
                        top: i as f32 * 110.,
                        height: 96.,
                    })
                    .collect();
                let mut c = Controller::new(rows.clone(), 6, 3..12, 330., 330., 474.).unwrap();
                script.push_str("N 16 6 3 12 330 330 474\n");
                for row in &rows {
                    script.push_str(&format!(
                        "{} {} {} {}\n",
                        u8::from(row.marker),
                        u8::from(row.voiced),
                        row.top,
                        row.height
                    ));
                }
                expected.push(snapshot(&c, &[]));
                profiles += 1;
                let scroll = if external < 3 { 330. } else { 474. };
                script.push_str(&format!("S {external} {scroll}\n"));
                c.select_external(external, scroll, true);
                expected.push(snapshot(&c, &[]));
                script.push_str(&format!("F 1 0 20000000 {repeated:x} 0\n"));
                let events = c.input(Input {
                    permission: true,
                    pressed: 1 << 29,
                    repeated,
                    ..Default::default()
                });
                expected.push(snapshot(&c, &events));
                for _ in 0..4 {
                    script.push_str("T\n");
                    c.advance_scroll();
                    expected.push(snapshot(&c, &[]));
                }
            }
        }
    }
    let evidence = std::env::var_os("SHIN_GEROKASU2_BACKLOG_EVIDENCE").map(PathBuf::from);
    if let Some(prefix) = &evidence {
        std::fs::write(prefix.with_extension("input"), &script).unwrap();
    }
    let mut child = Command::new(native)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(script.as_bytes());
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Some(prefix) = &evidence {
        std::fs::write(prefix.with_extension("output"), &output.stdout).unwrap();
        std::fs::write(prefix.with_extension("native.log"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = text.lines().filter(|s| s.starts_with("B ")).collect();
    assert_eq!(rows.len(), expected.len());
    for (i, (got, want)) in rows.iter().zip(expected).enumerate() {
        let actual: Vec<u32> = got
            .split_whitespace()
            .skip(2)
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(actual, want, "native backlog observation{i}");
    }
    println!(
        "GER_BACKLOG_MATCH profiles={profiles} observations={} selection/markerpriority/actions/832pxreveal/axisFMA/literalclock",
        rows.len()
    );
}
