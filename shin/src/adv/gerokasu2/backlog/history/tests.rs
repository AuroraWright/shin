use super::*;
use shin_core::format::{
    font::gerokasu2::Gerokasu2Font,
    save::gerokasu2::{Progress, ProgressEntry, SaveDate},
};
use std::io::{Read, Write};
fn scenario() -> Arc<Gerokasu2Scenario> {
    let mut d = b"SNR ".to_vec();
    for w in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        d.extend(w.to_le_bytes());
    }
    let mut tables = vec![];
    for i in 0..10 {
        d.extend((0x4c + tables.len() as u32).to_le_bytes());
        if i < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    d.extend(tables);
    let start = d.len() as u32;
    d[32..36].copy_from_slice(&start.to_le_bytes());
    fn number(d: &mut Vec<u8>, n: i32) {
        if (-64..64).contains(&n) {
            d.push(n as u8 & 0x7f);
        } else {
            assert!((-2048..2048).contains(&n));
            d.extend([0x80 | ((n >> 8) & 15) as u8, n as u8]);
        }
    }
    fn text(d: &mut Vec<u8>, s: &str) {
        d.extend(((s.len() + 1) as u16).to_le_bytes());
        d.extend(s.as_bytes());
        d.push(0);
    }
    fn msg(d: &mut Vec<u8>, id: u32, arg: i32, s: &str) {
        d.push(0x86);
        d.extend(((id + 1) | 0x1000000).to_le_bytes());
        number(d, arg);
        text(d, s);
    }
    for (level, label) in [(0, "章"), (1, "見出し")] {
        d.push(0xa0);
        number(&mut d, level);
        text(&mut d, label);
    }
    d.push(0xa4);
    d.push(0x81);
    d.extend(3u16.to_le_bytes());
    number(&mut d, 0);
    msg(&mut d, 1, 0, "名前@r「一行。」@o50.@vfirst.@o-1.@vsecond.");
    d.push(0x82);
    number(&mut d, 0);
    number(&mut d, -123);
    d.push(0x81);
    d.extend(4u16.to_le_bytes());
    number(&mut d, 0);
    d.push(0xa1);
    d.push(0x8d);
    d.extend(1u16.to_le_bytes());
    d.extend(1u16.to_le_bytes());
    d.extend(7u16.to_le_bytes());
    number(&mut d, 3);
    text(&mut d, "選択");
    let choices = "*12.右\0左\0\0";
    d.extend((choices.len() as u16).to_le_bytes());
    d.extend(choices.as_bytes());
    msg(&mut d, 2, -1, "@r@bふりがな.@<本文@>と@{太字@}。@x100.");
    d.push(0xe0);
    d.extend(8u16.to_le_bytes());
    number(&mut d, 0);
    msg(&mut d, 3, 0, "@r続き");
    d.push(0xa5);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(d.into()).unwrap())
}
#[test]
fn ger_history_cancellation_and_zero_source_never_execute_or_mutate_globals() {
    let scenario = scenario();
    let globals = SaveData::default();
    let source = GameData {
        date: SaveDate::default(),
        progress: Progress::fresh(0, 1),
    };
    assert!(History::collect(scenario.clone(), source.clone(), &globals, 1, || true).is_err());
    let h = History::collect(scenario, source.clone(), &globals, 1, || false).unwrap();
    assert!(h.entries.is_empty());
    assert_eq!(h.jump_source, source);
    assert!(h.geometry(&[]).unwrap().is_none());
}
#[test]
#[ignore = "original Ger complete history worker and supplied FNT6; CPU only"]
fn ger_native_backlog_history_adapter_routes_layout_and_first_match() {
    use std::process::{Command, Stdio};
    fn words(r: &mut impl Read, n: usize) -> Vec<u32> {
        (0..n)
            .map(|_| {
                let mut b = [0; 4];
                r.read_exact(&mut b).unwrap();
                u32::from_le_bytes(b)
            })
            .collect()
    }
    fn w(v: &mut Vec<u8>, n: u32) {
        v.extend(n.to_le_bytes());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let normal = Gerokasu2Font::new(
        std::fs::read(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .unwrap()
            .into(),
    )
    .unwrap();
    let bold = Gerokasu2Font::new(
        std::fs::read(root.join("test-fixtures/gerokasu2/system.fnt"))
            .unwrap()
            .into(),
    )
    .unwrap();
    let binary = std::env::var_os("SHIN_GEROKASU2_BACKLOG_HISTORY_ORACLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join("reverse-engineering/gerokasu2/backlog-history/native-history")
        });
    let mut native = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = native.stdin.take().unwrap();
    let mut output = native.stdout.take().unwrap();
    let scenario = scenario();
    let mut cases = 0;
    let mut rows = 0;
    for active in [0, 1] {
        for saved in [0, 1, 2, 3, 4, 5, 7, 100] {
            for remembered in [1, 2] {
                let mut globals = SaveData::default();
                globals.persist_data.set(0, -111);
                globals.settings.v11_disallowskipunread = true;
                globals.save_vectors.seen_messages = vec![u32::MAX];
                globals.save_vectors.select_last_choices = vec![remembered];
                let before = globals.encode().unwrap();
                let mut route = vec![];
                let mut cursor = 0;
                super::super::super::responses::write_persistent(&mut route, &mut cursor, -7);
                super::super::super::responses::write_persistent(&mut route, &mut cursor, -9);
                super::super::super::responses::write_bits(&mut route, &mut cursor, 4, 1);
                super::super::super::responses::write_bits(&mut route, &mut cursor, 6, 9);
                let mut entries = [
                    ProgressEntry {
                        scenario_id: 0x777,
                        random_seed: 0x888,
                        save_position: 0x999,
                        replay_target: 0xaaa,
                        responses: vec![],
                    },
                    ProgressEntry {
                        scenario_id: 0x777,
                        random_seed: 0x888,
                        save_position: 0x999,
                        replay_target: 0xaaa,
                        responses: vec![],
                    },
                ];
                entries[active] = ProgressEntry {
                    scenario_id: 0,
                    random_seed: 1,
                    save_position: saved,
                    replay_target: 99,
                    responses: route.clone(),
                };
                let source = GameData {
                    date: SaveDate::default(),
                    progress: Progress::new(active, entries).unwrap(),
                };
                let history =
                    History::collect(scenario.clone(), source.clone(), &globals, 1000, || false)
                        .unwrap();
                assert_eq!(globals.encode().unwrap(), before);
                let raw = scenario.raw();
                let mut request = vec![];
                for n in [
                    0x31494842,
                    raw.len() as u32,
                    scenario.header.code_offset,
                    active as u32,
                    0,
                    1,
                    saved,
                    99,
                    route.len() as u32,
                    globals.persist_data.0.len() as u32,
                    1,
                    1,
                    1,
                ] {
                    w(&mut request, n);
                }
                request.extend(raw);
                request.extend(route);
                for &p in &globals.persist_data.0 {
                    request.extend(p.to_le_bytes());
                }
                w(&mut request, u32::MAX);
                request.push(remembered);
                input.write_all(&request).unwrap();
                input.flush().unwrap();
                let h = words(&mut output, 13);
                assert_eq!(h[0], 0x314f4842);
                assert_eq!(h[1], u32::from(!history.entries.is_empty()));
                assert_eq!(h[2] as usize, history.entries.len());
                let mut route = vec![0; h[11] as usize];
                output.read_exact(&mut route).unwrap();
                assert_eq!(
                    route,
                    history.jump_source.progress.active_entry().responses,
                    "native history extended responses case{cases}"
                );
                assert_eq!((h[9], h[10]), (saved, 99));
                assert_eq!(
                    history.jump_source.progress.entries[1 - active],
                    source.progress.entries[1 - active]
                );
                let mut measured = vec![];
                let mut native_rows = vec![];
                for (i, e) in history.entries.iter().enumerate() {
                    let v = words(&mut output, 8);
                    let mut text = vec![0; v[7] as usize];
                    output.read_exact(&mut text).unwrap();
                    assert_eq!(String::from_utf8(text).unwrap(), e.display_text());
                    assert_eq!(
                        (v[0], v[2], v[3], v[4]),
                        (
                            e.kind() as u32,
                            e.boundary,
                            i as u32,
                            e.message_arg() as u32
                        )
                    );
                    let l = e.layout(&normal, &bold, 999);
                    measured.push((l.size.y, l.voice_count));
                    native_rows.push(v);
                    rows += 1;
                }
                let count = words(&mut output, 1)[0] as usize;
                assert_eq!(
                    words(&mut output, count),
                    history
                        .chapters
                        .iter()
                        .map(|&n| n as u32)
                        .collect::<Vec<_>>()
                );
                if let Some(g) = history.geometry(&measured).unwrap() {
                    assert_eq!(
                        (h[3], h[4], h[5], h[6]),
                        (g.selected as u32, g.selected as u32, 0, g.rows.len() as u32)
                    );
                    assert_eq!(
                        (h[7], h[8], h[12]),
                        (
                            g.minimum.to_bits(),
                            g.bottom.to_bits(),
                            g.initial_scroll.to_bits()
                        ),
                        "native history layout/First case{cases}"
                    );
                    for (r, v) in g.rows.iter().zip(native_rows) {
                        assert_eq!(
                            (v[1], v[5], v[6]),
                            (
                                u32::from(r.voice_count),
                                r.top.to_bits(),
                                r.height.to_bits()
                            ),
                            "native history layout row case{cases}"
                        );
                    }
                }
                cases += 1;
            }
        }
    }
    drop(input);
    assert!(native.wait().unwrap().success());
    println!(
        "GER_BACKLOG_HISTORY_ADAPTER_MATCH cases={cases} rows={rows} actualGerVM/routes/privateGlobals/FNT6layout/First"
    );
}

#[test]
fn ger_history_title_voices_are_measured_but_marker_rows_do_not_replay() {
    #[derive(Clone, Copy)]
    struct Metrics;
    impl shin_core::layout::font::TextFontMetrics for Metrics {
        fn ascent(&self) -> f32 {
            40.
        }
        fn descent(&self) -> f32 {
            4.
        }
        fn line_height(&self) -> f32 {
            44.
        }
        fn glyph_advance(&self, _: char) -> Option<f32> {
            Some(44.)
        }
    }
    for item in [
        Item::Choice {
            title: "題名@vtitle.@o25.".into(),
            text: "本文@vbody.".into(),
            response: 0,
        },
        Item::Checkpoint {
            title: "題名@vtitle.@o25.".into(),
            text: "本文@vbody.".into(),
        },
    ] {
        let row = Entry { boundary: 1, item };
        let layout = row.layout(Metrics, Metrics, 999);
        assert_eq!(
            layout.voice_count, 2,
            "measurement parses the formatted title and body"
        );
        assert!(layout.voices.is_empty(), "native83030 rejects marker rows");
        assert_eq!(row.voices(), layout.voices);
    }
}
