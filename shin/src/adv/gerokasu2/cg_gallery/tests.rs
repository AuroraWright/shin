use super::*;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};
fn root() -> PathBuf {
    std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("PWD")))
}
fn entries(count: usize, profile: usize) -> Vec<Entry> {
    (0..count)
        .map(|i| {
            let pictures = (0..1 + i % 5)
                .map(|j| (i * 5 + j) as u16)
                .collect::<Vec<_>>();
            let unlocked = pictures
                .iter()
                .map(|&id| {
                    profile == 1 || (profile == 2 && id % 2 == 0) || (profile == 3 && id % 3 == 0)
                })
                .collect();
            Entry {
                name: "thumbnail".into(),
                pictures,
                unlocked,
            }
        })
        .collect()
}
fn native(kind: &str, input: &str) -> String {
    let r = root();
    let dir = r.join("gerokasu2/.local/cg-gallery");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{kind}-{}-input.txt", input.len()));
    fs::write(&path, input).unwrap();
    let variable = format!("SHIN_GEROKASU2_CG_{}", kind.to_uppercase());
    let binary = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join(format!("native-cg-{kind}")));
    let o = Command::new(binary)
        .arg(r.join("gerokasu2/uncompressed_executable"))
        .arg(r.join("gerokasu2/.local/system.fnt"))
        .arg(r.join("gerokasu2/.local/newrodin-medium.fnt"))
        .stdin(Stdio::from(fs::File::open(path).unwrap()))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "native {kind}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
#[derive(Default)]
struct Counters {
    ret: u32,
    plays: u32,
    stops: u32,
    ticks: u32,
    viewers: u32,
    index: u32,
    achievements: u32,
}
fn packet(m: &mut Controller, c: &mut Counters) -> Vec<u32> {
    for a in m.take_actions() {
        match a {
            Action::OpenViewer(i) => {
                c.viewers += 1;
                c.index = i as u32;
            }
            Action::ReturnTitle { requested_id } => c.ret = requested_id as u32,
            Action::PlayBgm { .. } => c.plays += 1,
            Action::StopBgm(t) => {
                c.stops += 1;
                c.ticks = t;
            }
            Action::Achievement(_) => c.achievements += 1,
        }
    }
    let mut v = vec![
        m.pages() as u32,
        m.page as u32,
        m.x as u32,
        m.y as u32,
        u32::from(m.closing),
        m.frame,
        m.spin.to_bits(),
        m.contact as u32,
        m.hover as u32,
    ];
    for i in 0..12 {
        v.extend([
            m.positions[i].value().to_bits(),
            u32::from(m.positions[i].busy()),
            m.focus[i].to_bits(),
            m.hot[i].value().to_bits(),
            u32::from(m.hot[i].busy()),
        ]);
    }
    v.extend([
        m.close_flash.value().to_bits(),
        u32::from(m.close_flash.busy()),
        m.disabled,
    ]);
    m.disabled = 0;
    let mut cues = [0; 4];
    for s in m.take_sounds() {
        cues[["move", "decide", "cancel", "error"]
            .iter()
            .position(|x| *x == s)
            .unwrap()] += 1;
    }
    v.extend(cues);
    v.extend([
        c.ret,
        c.plays,
        c.stops,
        c.ticks,
        c.viewers,
        c.index,
        c.achievements,
    ]);
    v
}
fn buttons(pressed: u32, repeated: u32) -> Input {
    Input {
        cancel: pressed & (1 << 29) != 0,
        confirm: pressed & (1 << 28) != 0,
        previous: repeated & (1 << 10) != 0,
        next: repeated & (1 << 11) != 0,
        up: repeated & (1 << 24) != 0,
        down: repeated & (1 << 25) != 0,
        left: repeated & (1 << 26) != 0,
        right: repeated & (1 << 27) != 0,
    }
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native CG probe"]
fn ger_native_cg_grid_controller_pointer_page_opening_return_and_tweens() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut push = |line: String, result: i32, m: &mut Controller, c: &mut Counters| {
        let op = line.chars().next().unwrap();
        script.push_str(&line);
        script.push('\n');
        if op != 'X' {
            expected.push((op, result, packet(m, c)));
        }
    };
    let mut profiles = 0;
    for count in [1, 2, 5, 12, 13, 25] {
        for profile in 0..4 {
            let mut m = Controller::new(entries(count, profile)).unwrap();
            let mut c = Counters {
                ret: u32::MAX,
                ..Default::default()
            };
            push(format!("N {count} {profile}"), -1, &mut m, &mut c);
            m.assets_ready();
            push("R".into(), -1, &mut m, &mut c);
            for dt in [0., 0.25, 1., 2.5, 55.25, 0., 1., 0., 0.] {
                m.update(dt, true, false, Input::default());
                push(format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
            }
            for repeated in [
                1 << 24,
                1 << 25,
                1 << 26,
                1 << 27,
                (1 << 24) | (1 << 26),
                (1 << 25) | (1 << 27),
            ] {
                for permission in [false, true] {
                    for capture in [false, true] {
                        let result = m.controls(permission, capture, buttons(0, repeated));
                        push(
                            format!("U {} {} 0 {repeated:x}", permission as u8, capture as u8),
                            result as i32,
                            &mut m,
                            &mut c,
                        );
                    }
                }
            }
            for (x, y) in [
                (1766., 46.),
                (1874., 46.),
                (1766., 154.),
                (117., 222.),
                (501., 222.),
                (0., 162.),
                (1900., 980.),
            ] {
                let result = m.probe(x, y);
                push(format!("T {x} {y}"), result as i32, &mut m, &mut c);
                m.press();
                push("P".into(), -1, &mut m, &mut c);
                for dt in [0.25, 1., 4.] {
                    m.update(dt, true, true, Input::default());
                    push(format!("F {dt} 0 0 1"), 0, &mut m, &mut c);
                }
                m.drag();
                push("H".into(), -1, &mut m, &mut c);
                m.reset();
                push("Z".into(), -1, &mut m, &mut c);
            }
            // Real page task both directions, including the incomplete final page.
            for bit in [11, 10] {
                let r = m.controls(true, false, buttons(0, 1 << bit));
                push(format!("U 1 0 0 {:x}", 1 << bit), r as i32, &mut m, &mut c);
                for dt in [0., 0.25, 1., 2.5, 4., 20., 0., 0.25, 1., 30., 0., 0.] {
                    m.update(dt, true, false, Input::default());
                    push(format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
                }
            }
            m.cancel();
            push("K".into(), -1, &mut m, &mut c);
            let r = m.controls(true, false, buttons(1 << 29, 0));
            push("U 1 0 20000000 0".into(), r as i32, &mut m, &mut c);
            for dt in [0., 0.25, 1., 2.5, 4., 0., 0.] {
                m.update(dt, true, false, Input::default());
                push(format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
            }
            push("X".into(), -1, &mut m, &mut c);
            profiles += 1;
        }
    }
    let output = native("model", &script);
    let actual = output
        .lines()
        .filter(|l| l.starts_with("G "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (line, (op, result, words))) in actual.iter().zip(expected.iter()).enumerate() {
        let mut p = line.split_whitespace();
        assert_eq!(p.next(), Some("G"));
        assert_eq!(p.next().unwrap(), op.to_string());
        assert_eq!(
            p.next().unwrap().parse::<i32>().unwrap(),
            *result,
            "CG result sample{i}"
        );
        let got = p.map(|v| v.parse::<u32>().unwrap()).collect::<Vec<_>>();
        assert_eq!(&got, words, "CG state sample{i} op{op}");
    }
    println!("GER_CG_MODEL profiles={profiles} states={}", expected.len());
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native CG draw probe"]
fn ger_native_cg_complete_cell_shell_digit_hint_check_packets() {
    let mut script = String::new();
    let mut expected = vec![];
    for count in [1usize, 2, 13, 25] {
        for profile in [0, 1, 2] {
            for ready in [false, true] {
                for page in 0..count.div_ceil(12) {
                    for focus in [0.5f32, 0.75, 1.] {
                        let mut m = Controller::new(entries(count, profile)).unwrap();
                        if ready {
                            m.assets_ready();
                        }
                        m.page = page;
                        m.x = 0;
                        m.y = 0;
                        m.spin = 0.125;
                        m.focus = [focus; 12];
                        for hot in &mut m.hot {
                            hot.snap(0.25);
                        }
                        m.close_flash.snap(0.5);
                        script.push_str(&format!(
                            "D {count} {profile} {} {page} 0 {:x} {:x} {:x} {:x}\n",
                            ready as u8,
                            m.spin.to_bits(),
                            focus.to_bits(),
                            0.25f32.to_bits(),
                            0.5f32.to_bits()
                        ));
                        expected.push(artwork::packets(&m, crate::cli::Game::Gerokasu2));
                    }
                }
            }
        }
    }
    let output = native("render", &script);
    let names = output
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            let (i, n) = l.split_once(' ').unwrap();
            (i.parse::<u32>().unwrap(), n.to_owned())
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut case = 0;
    let mut packet = 0;
    let mut total = 0;
    for line in output.lines() {
        if line.starts_with("D ") {
            case = line[2..].parse().unwrap();
            packet = 0;
        } else if let Some(p) = line.strip_prefix("P ") {
            let v = p
                .split_whitespace()
                .map(|w| w.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let q = &expected[case][packet];
            assert_eq!(v[1], q.blend, "CG blend case{case} packet{packet}");
            assert_eq!(
                if v[2] == u32::MAX {
                    ""
                } else {
                    names.get(&v[2]).unwrap()
                },
                q.texture,
                "CG texture case{case} packet{packet}"
            );
            assert_eq!(
                &v[8..24],
                &artwork::projection().map(f32::to_bits),
                "CG matrix case{case} packet{packet}"
            );
            assert_eq!(
                &v[33..],
                q.native_vertices(),
                "CG vertices case{case} packet{packet} texture{}",
                q.texture
            );
            if q.flash > 0. {
                assert_eq!(v[0], 6);
                assert_eq!(&v[24..28], &[0u32; 4]);
                assert_eq!(
                    &v[28..32],
                    &[
                        1f32.to_bits(),
                        1f32.to_bits(),
                        1f32.to_bits(),
                        q.flash.to_bits()
                    ]
                );
            }
            packet += 1;
            total += 1;
        } else if line.starts_with("E ") {
            assert_eq!(packet, expected[case].len(), "CG draw count case{case}");
        }
    }
    println!("GER_CG_RENDER profiles={} packets={total}", expected.len());
}
#[test]
fn ger_cg_locked_entries_and_viewer_variants_preserve_progress() {
    let locked = Entry {
        name: "a".into(),
        pictures: vec![0, 1, 2, 3],
        unlocked: vec![false, false, false, false],
    };
    let mut m = Controller::new(vec![locked.clone()]).unwrap();
    m.decide();
    assert_eq!(m.take_sounds(), vec!["error"]);
    assert!(m.take_actions().is_empty());
    assert!(viewer::Viewer::new(locked).is_err());
    let mut v = viewer::Viewer::new(Entry {
        name: "b".into(),
        pictures: vec![4, 5, 6, 7],
        unlocked: vec![false, true, false, true],
    })
    .unwrap();
    assert_eq!(v.variant, 1);
    assert_eq!(v.take_events(), vec![viewer::Event::Load(5)]);
    v.publish([3840., 2160.]);
    v.controls(
        true,
        false,
        viewer::Input {
            confirm: true,
            ..Default::default()
        },
    );
    assert_eq!(v.variant, 3);
    assert_eq!(v.take_events(), vec![viewer::Event::Load(7)]);
    v.publish([1920., 1080.]);
    v.controls(
        true,
        false,
        viewer::Input {
            confirm: true,
            ..Default::default()
        },
    );
    assert!(v.closing);
    assert_eq!(v.variant, -1);
    assert_eq!(v.take_events(), vec![viewer::Event::CaptureForClose]);
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native CG Viewer probe"]
fn ger_native_cg_viewer_bounds_pan_fit_info_inertia_and_gesture() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut snapshot = |op: &str, result: i32, v: &viewer::Viewer, disabled: u32| {
        script.push_str(op);
        script.push('\n');
        if op == "X" {
            return;
        }
        let mut words = vec![v.closing as u32, v.show_info as u32, v.fit as u32];
        words.extend(v.velocity.map(f32::to_bits));
        words.extend(v.offset.map(f32::to_bits));
        for p in &v.placement {
            words.extend([p.value().to_bits(), p.busy() as u32]);
        }
        words.push(disabled);
        expected.push((op.chars().next().unwrap(), result, words));
    };
    let mut profiles = 0;
    for (width, height) in [
        (1920., 1080.),
        (3840., 1080.),
        (1920., 2160.),
        (3840., 2160.),
        (640., 480.),
    ] {
        let mut v = viewer::Viewer::new(Entry {
            name: "a".into(),
            pictures: vec![0],
            unlocked: vec![true],
        })
        .unwrap();
        v.extent = Some([width, height]);
        snapshot(&format!("N {width} {height}"), -1, &v, 0);
        v.apply_bounds(false);
        snapshot("A 0", -1, &v, 0);
        for held in [
            0,
            1 << 24,
            1 << 25,
            1 << 26,
            1 << 27,
            (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27),
        ] {
            for permission in [false, true] {
                for capture in [false, true] {
                    let x = 0.25;
                    let y = -0.5;
                    v.controls(
                        permission,
                        capture,
                        viewer::Input {
                            left: held & (1 << 26) != 0,
                            right: held & (1 << 27) != 0,
                            up: held & (1 << 24) != 0,
                            down: held & (1 << 25) != 0,
                            axes: [x, y],
                            ..Default::default()
                        },
                    );
                    snapshot(
                        &format!(
                            "U {} {} 0 {held:x} {x} {y}",
                            permission as u8, capture as u8
                        ),
                        1,
                        &v,
                        u32::from(!permission),
                    );
                }
            }
        }
        for fit in [false, true, false] {
            v.fit = fit;
            v.offset = [4000., -4000.];
            v.velocity = [40., -20.];
            snapshot(&format!("S {} 4000 -4000 40 -20", fit as u8), -1, &v, 0);
            v.apply_bounds(true);
            snapshot("A 1", -1, &v, 0);
            for dt in [0., 0.25, 1., 3.75, 5., 1.] {
                v.update(dt);
                snapshot(&format!("F {dt}"), 0, &v, 0);
            }
        }
        for pressed in [0x80, 0x40, 0x80, 0x40] {
            v.controls(
                true,
                false,
                viewer::Input {
                    fit: pressed & 0x40 != 0,
                    info: pressed & 0x80 != 0,
                    ..Default::default()
                },
            );
            snapshot(&format!("U 1 0 {pressed:x} 0 0 0"), 1, &v, 0);
            for dt in [5., 5., 0.] {
                v.update(dt);
                snapshot(&format!("F {dt}"), 0, &v, 0);
            }
        }
        v.fit = false;
        v.offset = [0.; 2];
        v.velocity = [0.; 2];
        snapshot("S 0 0 0 0 0", -1, &v, 0);
        v.apply_bounds(false);
        snapshot("A 0", -1, &v, 0);
        v.gesture([40., -20.]);
        snapshot("G 40 -20", -1, &v, 0);
        for dt in [0., 0.25, 1., 2.5] {
            v.update(dt);
            snapshot(&format!("F {dt}"), 0, &v, 0);
        }
        snapshot("X", -1, &v, 0);
        profiles += 1;
    }
    let output = native("viewer", &script);
    let actual = output
        .lines()
        .filter(|l| l.starts_with("V "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (line, (op, result, words))) in actual.iter().zip(expected.iter()).enumerate() {
        let p = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(p[1], op.to_string());
        assert_eq!(p[2].parse::<i32>().unwrap(), *result);
        assert_eq!(
            p[3..]
                .iter()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "CG Viewer state{i} op{op}"
        );
    }
    println!(
        "GER_CG_VIEWER profiles={profiles} states={}",
        expected.len()
    );
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native CG variants probe"]
fn ger_native_cg_viewer_skips_locked_variants_and_ends_at_authored_count() {
    let mut script = String::new();
    let mut expected = vec![];
    for count in [1usize, 2, 4, 31, 32, 35, 64, 65, 127] {
        for profile in 0..4 {
            script.push_str(&format!("{count} {profile}\n"));
            let mut words = vec![count as u32, profile];
            let e = Entry {
                name: "variants".into(),
                pictures: (400..400 + count as u16).collect(),
                unlocked: (0..count)
                    .map(|i| {
                        profile == 1 || (profile == 2 && i % 2 == 0) || (profile == 3 && i % 3 == 0)
                    })
                    .collect(),
            };
            if e.locked() {
                assert!(viewer::Viewer::new(e).is_err());
                words.extend([0, u32::MAX, 0, u32::MAX]);
            } else {
                let mut m = viewer::Viewer::new(e).unwrap();
                let mut requests = 0;
                let mut last = u32::MAX;
                loop {
                    for event in m.take_events() {
                        if let viewer::Event::Load(id) = event {
                            requests += 1;
                            last = id as u32;
                        }
                    }
                    words.extend([(!m.closing) as u32, m.variant as u32, requests, last]);
                    if m.closing {
                        break;
                    }
                    m.controls(
                        true,
                        false,
                        viewer::Input {
                            confirm: true,
                            ..Default::default()
                        },
                    );
                }
            }
            expected.push(words);
        }
    }
    let output = native("variants", &script);
    let actual = output
        .lines()
        .filter(|l| l.starts_with("V "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (line, words)) in actual.iter().zip(expected.iter()).enumerate() {
        let got = line
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(&got, words, "CG variant profile{i}");
    }
    println!("GER_CG_VARIANTS profiles={} groups_max=127", expected.len());
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native CG probe"]
fn ger_native_cg_confirm_waits_for_flash_and_passes_exact_viewer_group() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut push = |op: &str, result: i32, m: &mut Controller, c: &mut Counters| {
        script.push_str(op);
        script.push('\n');
        if op != "X" {
            expected.push((op.chars().next().unwrap(), result, packet(m, c)));
        }
    };
    for count in [1usize, 13] {
        for profile in 0..4 {
            let mut m = Controller::new(entries(count, profile)).unwrap();
            let mut c = Counters {
                ret: u32::MAX,
                ..Default::default()
            };
            push(&format!("N {count} {profile}"), -1, &mut m, &mut c);
            m.assets_ready();
            push("R", -1, &mut m, &mut c);
            for dt in [60., 0., 0.] {
                m.update(dt, true, false, Input::default());
                push(&format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
            }
            let r = m.controls(
                true,
                false,
                Input {
                    confirm: true,
                    ..Default::default()
                },
            );
            push("U 1 0 10000000 0", r as i32, &mut m, &mut c);
            for dt in [7., 0.] {
                m.update(dt, true, false, Input::default());
                push(&format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
            }
            push("X", -1, &mut m, &mut c);
        }
    }
    let output = native("model", &script);
    let actual = output
        .lines()
        .filter(|l| l.starts_with("G "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (line, (op, result, words))) in actual.iter().zip(expected.iter()).enumerate() {
        let p = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(p[1], op.to_string());
        assert_eq!(p[2].parse::<i32>().unwrap(), *result);
        assert_eq!(
            p[3..]
                .iter()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "CG Viewer request state{i} op{op}"
        );
    }
    println!("GER_CG_PLAY owners=8 states={}", expected.len());
}
#[test]
fn gerokasu_cg_chrome_adds_bottom_band() {
    use crate::cli::Game;
    let mut m = Controller::new(entries(13, 1)).unwrap();
    m.assets_ready();
    let base = artwork::packets(&m, Game::Gerokasu2);
    let ger = artwork::packets(&m, Game::Gerokasu);
    assert!(base.iter().all(|p| p.texture != "bottom"));
    assert_eq!(base.iter().find(|p| p.texture == "top").unwrap().blend, 3);
    assert_eq!(ger.iter().find(|p| p.texture == "top").unwrap().blend, 2);
    assert_eq!(ger.len(), base.len() + 1);
    let i = ger.iter().position(|p| p.texture == "bottom").unwrap();
    assert_eq!(
        (ger[i - 1].texture.as_str(), ger[i + 1].texture.as_str()),
        ("close", "numbg")
    );
    assert_eq!(ger[i].rect, [0., 992., 1920., 1080.]);
    assert_eq!(ger[i].uv, [0., 0., 1., 1.]);
    assert_eq!((ger[i].color, ger[i].blend), (u32::MAX, 2));
}
