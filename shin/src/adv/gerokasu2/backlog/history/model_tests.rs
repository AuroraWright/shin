use super::*;
#[test]
fn history_future_choice_and_distinct_current_origin() {
    let mut b = Builder::new(3);
    b.message(1, true, 0, "@rFirst", false, false);
    b.checkpoint();
    b.message(2, true, -1, "@rCurrent", false, false);
    b.message(3, true, 0, "@rFuture", false, false);
    b.stop();
    assert_eq!(b.current_row(), Some(1));
    assert_eq!(b.chapters, [1]);
    let g = geometry(
        &b.entries,
        b.current_row(),
        &[(54., 1), (54., 0), (54., 1), (54., 1)],
    )
    .unwrap()
    .unwrap();
    assert_eq!(g.selected, 1);
    assert_eq!(
        g.rows.iter().map(|r| (r.top, r.height)).collect::<Vec<_>>(),
        [(-240., 232.), (0., 90.), (98., 90.), (196., 232.)]
    );
    assert_eq!(future_choice(0, 2, 2, 2), Some(1));
    assert_eq!(future_choice(-1, 2, 2, 2), None);
    assert_eq!(future_choice(0, 3, 2, -1), None);
}

#[test]
#[ignore = "original Ger complete LogBuilder VM/worker/FNT6 measurement/First; CPU only"]
fn ger_native_backlog_history_rows_routes_and_initial_geometry() {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };
    fn w(v: &mut Vec<u8>, n: u32) {
        v.extend(n.to_le_bytes());
    }
    fn num(v: &mut Vec<u8>, n: i32) {
        if (0..64).contains(&n) {
            v.push(n as u8);
        } else if (-64..0).contains(&n) {
            v.push(n as u8 & 0x7f);
        } else {
            v.push(0xf0);
            v.extend(n.to_le_bytes());
        }
    }
    fn string(v: &mut Vec<u8>, s: &str) {
        assert!(s.len() < 254);
        v.extend(((s.len() + 1) as u16).to_le_bytes());
        v.extend(s.as_bytes());
        v.push(0);
    }
    fn message(v: &mut Vec<u8>, id: u32, arg: i32, text: &str) {
        v.push(0x86);
        w(v, (id.wrapping_add(1) & 0xffffff) | 0x1000000);
        num(v, arg);
        string(v, text);
    }
    fn words(r: &mut impl Read, n: usize) -> Vec<u32> {
        (0..n)
            .map(|_| {
                let mut b = [0; 4];
                r.read_exact(&mut b).unwrap();
                u32::from_le_bytes(b)
            })
            .collect()
    }
    let root = std::path::PathBuf::from(
        std::env::var_os("SHIN_GEROKASU2_ROOT").unwrap_or_else(|| std::env::var_os("PWD").unwrap()),
    );
    let binary = std::env::var_os("SHIN_GEROKASU2_BACKLOG_HISTORY_ORACLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join("reverse-engineering/gerokasu2/backlog-history/native-history")
        });
    let mut child = Command::new(binary)
        .args([
            root.join("game-data/gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut profiles = 0;
    let mut compared = 0;
    for active in [0, 1] {
        for saved in 0..13 {
            for disallow in [false, true] {
                for read in [false, true] {
                    for remembered in [0, 1, 2, 3] {
                        for visibility in [0, 1, 3] {
                            let mut code = vec![0; 16];
                            for (level, label) in [(0, "章"), (1, "見出し")] {
                                code.push(0xa0);
                                num(&mut code, level);
                                string(&mut code, label);
                            }
                            code.push(0xa4); // RESUMESET
                            message(&mut code, 1, 0, "名前@r「一行。」@vfirst.");
                            code.push(0xa1); // AUTOSAVE
                            message(&mut code, 2, -1, "@r二行目@vsecond.");
                            code.push(0x8d);
                            code.extend(1u16.to_le_bytes());
                            code.extend(1u16.to_le_bytes());
                            code.extend(7u16.to_le_bytes());
                            num(&mut code, visibility);
                            string(&mut code, "選択");
                            let choices = "*12.右\0左\0\0";
                            code.extend((choices.len() as u16).to_le_bytes());
                            code.extend(choices.as_bytes());
                            message(&mut code, u32::MAX, 0, "@r無番号");
                            code.push(0xa2);
                            num(&mut code, 7); // EVBEGIN
                            message(&mut code, 3, 0, "@rイベント本文");
                            code.push(0xa3); // EVEND
                            message(&mut code, 4, 0, "@r次");
                            code.push(0xa5); // RESUME
                            let route = [0x10u8];
                            let mask = if read { u32::MAX } else { 0 };
                            let header = [
                                0x31494842,
                                code.len() as u32,
                                16,
                                active,
                                0,
                                1,
                                saved,
                                99,
                                1,
                                0,
                                1,
                                1,
                                u32::from(disallow),
                            ];
                            let mut request = vec![];
                            for n in header {
                                w(&mut request, n);
                            }
                            request.extend(code);
                            request.extend(route);
                            w(&mut request, mask);
                            request.push(remembered);
                            input.write_all(&request).unwrap();
                            input.flush().unwrap();
                            let mut b = Builder::new(saved);
                            if !b.done {
                                b.save_info(0, "章").unwrap();
                                b.save_info(1, "見出し").unwrap();
                                b.resume_set();
                            }
                            if !b.done {
                                b.message(1, true, 0, "名前@r「一行。」@vfirst.", disallow, read);
                            }
                            if !b.done {
                                b.checkpoint();
                                b.message(2, true, -1, "@r二行目@vsecond.", disallow, read);
                            }
                            if !b.done {
                                let historical = b.begin_interactive();
                                let value = if historical {
                                    Some(1)
                                } else {
                                    future_choice(0, remembered, 2, visibility)
                                };
                                b.choice(value, "選択", &["*12.右".into(), "左".into()])
                                    .unwrap();
                            }
                            if !b.done {
                                b.message(u32::MAX, true, 0, "@r無番号", disallow, read);
                            }
                            if !b.done {
                                b.event_begin(7);
                                b.message(3, true, 0, "@rイベント本文", disallow, read);
                            }
                            if !b.done {
                                b.event_end();
                                b.message(4, true, 0, "@r次", disallow, read);
                            }
                            b.stop();
                            let h = words(&mut output, 13);
                            assert_eq!(h[0], 0x314f4842);
                            assert_eq!(
                                h[1],
                                u32::from(!b.entries.is_empty()),
                                "native history eligibility profile{profiles}"
                            );
                            assert_eq!(
                                h[2] as usize,
                                b.entries.len(),
                                "native history row count profile{profiles}"
                            );
                            assert_eq!((h[9], h[10]), (saved, 99), "native source positions");
                            let mut routes = vec![0; h[11] as usize];
                            output.read_exact(&mut routes).unwrap();
                            let mut measured = vec![];
                            let mut actual = vec![];
                            for (index, e) in b.entries.iter().enumerate() {
                                let v = words(&mut output, 8);
                                let mut text = vec![0; v[7] as usize];
                                output.read_exact(&mut text).unwrap();
                                assert_eq!(
                                    String::from_utf8(text).unwrap(),
                                    e.display_text(),
                                    "native history text profile{profiles}"
                                );
                                assert_eq!(
                                    (v[0], v[2], v[3], v[4]),
                                    (
                                        u32::from(e.kind()),
                                        e.boundary,
                                        index as u32,
                                        e.message_arg() as u32
                                    ),
                                    "native history row profile{profiles}"
                                );
                                let voices = u8::from(e.display_text().contains("@v"));
                                assert_eq!(v[1], u32::from(voices), "native history voice count");
                                measured.push((f32::from_bits(v[6]) - 36., voices));
                                actual.push(v);
                                compared += 1;
                            }
                            let markers = words(&mut output, 1)[0] as usize;
                            assert_eq!(
                                words(&mut output, markers),
                                b.chapters.iter().map(|&i| i as u32).collect::<Vec<_>>()
                            );
                            if let Some(g) =
                                geometry(&b.entries, b.current_row(), &measured).unwrap()
                            {
                                assert_eq!(
                                    (h[3] as usize, h[4] as usize, h[5] as usize, h[6] as usize),
                                    (g.selected, g.selected, 0, g.rows.len()),
                                    "native history initial selection"
                                );
                                assert_eq!(
                                    (h[7], h[8], h[12]),
                                    (
                                        g.minimum.to_bits(),
                                        g.bottom.to_bits(),
                                        g.initial_scroll.to_bits()
                                    ),
                                    "native history document/First"
                                );
                                for (r, v) in g.rows.iter().zip(actual) {
                                    assert_eq!(
                                        (v[5], v[6]),
                                        (r.top.to_bits(), r.height.to_bits()),
                                        "native history row geometry"
                                    );
                                }
                            }
                            profiles += 1;
                        }
                    }
                }
            }
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GER_BACKLOG_HISTORY_MATCH profiles={profiles} rows={compared} originalVM/LogBuilder/CanShow/copyback/measurement/First"
    );
}
