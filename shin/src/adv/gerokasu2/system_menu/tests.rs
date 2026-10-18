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
fn native(kind: &str, input: &str) -> String {
    let r = root();
    let d = r.join("reverse-engineering/gerokasu2/system-menu");
    fs::create_dir_all(&d).unwrap();
    let path = d.join(format!("{kind}-{}-input.txt", input.len()));
    fs::write(&path, input).unwrap();
    let bin = std::env::var_os(format!(
        "SHIN_GEROKASU2_SYSTEM_MENU_{}",
        kind.to_uppercase()
    ))
    .map(PathBuf::from)
    .unwrap_or_else(|| d.join(format!("native-system-menu-{kind}")));
    let o = Command::new(bin)
        .arg(r.join("gerokasu2/uncompressed_executable"))
        .arg(r.join("test-fixtures/gerokasu2/system.fnt"))
        .arg(r.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
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
struct Counts {
    enter: u32,
    leave: u32,
    dispatch: u32,
    title: u32,
}
fn packet(m: &mut Controller, c: &mut Counts) -> Vec<u32> {
    for e in m.take_events() {
        match e {
            Event::EnterModal => c.enter += 1,
            Event::LeaveModal => c.leave += 1,
            Event::ReturnTitle => c.title += 1,
            Event::ConfirmTitle => c.dispatch = 5,
            Event::OpenChild(x) => {
                c.dispatch = match x {
                    Child::Save => 1,
                    Child::Load => 2,
                    Child::Config => 3,
                    Child::Manual => 4,
                }
            }
        }
    }
    let mut v = vec![m.phase as u32, m.selected as u32, m.pointer_row as u32];
    for r in &m.rows {
        v.push(r.item as u32);
        for t in [&r.slide, &r.flash] {
            v.extend([t.value().to_bits(), t.busy() as u32]);
        }
    }
    for t in [&m.detail_slide, &m.detail_fade] {
        v.extend([t.value().to_bits(), t.busy() as u32]);
    }
    let mut cues = [0; 3];
    for s in m.take_sounds() {
        cues[["move", "decide", "cancel"]
            .iter()
            .position(|x| *x == s)
            .unwrap()] += 1;
    }
    v.extend([
        m.disabled,
        c.enter,
        c.leave,
        cues[0],
        cues[1],
        cues[2],
        c.dispatch,
        c.title,
        m.play_time.len() as u32,
    ]);
    v.extend(m.play_time.bytes().map(u32::from));
    m.disabled = 0;
    v
}
fn input(p: u32, r: u32) -> Input {
    Input {
        cancel: p & 0x20000080 != 0,
        confirm: p & (1 << 28) != 0,
        up: r & (1 << 24) != 0,
        down: r & (1 << 25) != 0,
        left: r & (1 << 26) != 0,
        right: r & (1 << 27) != 0,
    }
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native SystemMenu probe"]
fn ger_native_system_menu_constructor_controller_pointer_tasks_child_return_and_clock() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut push = |line: String, result: i32, m: &mut Controller, c: &mut Counts| {
        let op = line.chars().next().unwrap();
        script.push_str(&line);
        script.push('\n');
        if op != 'X' {
            expected.push((op, result, packet(m, c)));
        }
    };
    for profile in 0..10 {
        let mut m = Controller::new();
        let mut c = Counts::default();
        push("N".into(), -1, &mut m, &mut c);
        for dt in [0., 0.25, 1., 2.5, 14.25, 0.] {
            m.update(dt, true, false, Input::default(), 3600.);
            push(format!("F {dt} 1 0 0 0 3600"), 0, &mut m, &mut c);
        }
        for r in [1 << 24, 1 << 25, 1 << 26, 1 << 27, (1 << 24) | (1 << 27)] {
            for permission in [false, true] {
                for captured in [false, true] {
                    m.update(0., permission, captured, input(0, r), 0.);
                    push(
                        format!("F 0 {} {} 0 {r:x} 0", permission as u8, captured as u8),
                        0,
                        &mut m,
                        &mut c,
                    );
                }
            }
        }
        for (x, y) in [
            (24., 72.),
            (488., 72.),
            (24., 184.),
            (24., 192.),
            (487.9, 303.9),
            (488., 303.),
            (100., 552.),
            (100., 664.),
            (0., 0.),
            (1900., 1000.),
        ] {
            let b = m.probe([x, y]);
            push(format!("T {x} {y}"), b as i32, &mut m, &mut c);
            m.press();
            push("P".into(), -1, &mut m, &mut c);
            m.cancel();
            push("K".into(), -1, &mut m, &mut c);
            m.reset();
            push("Z".into(), -1, &mut m, &mut c);
        }
        let row = profile % 5;
        m.selected = row;
        push(format!("S {row}"), -1, &mut m, &mut c);
        m.update(0., true, false, input(1 << 28, 0), 59. * 3600.);
        push("F 0 1 0 10000000 0 212400".into(), 0, &mut m, &mut c);
        for dt in [0., 0.5, 1., 5.5, 0.] {
            m.update(dt, true, false, Input::default(), 61. * 3600.);
            push(format!("F {dt} 1 0 0 0 219600"), 0, &mut m, &mut c);
        }
        for dt in [1., 20., 0.] {
            m.update(dt, false, false, Input::default(), 60_000. * 3600.);
            push(format!("F {dt} 0 0 0 0 216000000"), 0, &mut m, &mut c);
        }
        if row < 4 {
            m.child_returned(1);
            push("C 1 0".into(), -1, &mut m, &mut c);
        } else if profile < 5 {
            m.title_answer(0, 1);
            push("C 0 1".into(), -1, &mut m, &mut c);
        } else {
            m.title_answer(1, 0);
            push("C 1 0".into(), -1, &mut m, &mut c);
            push("X".into(), -1, &mut m, &mut c);
            continue;
        }
        for dt in [0., 1., 20., 0.] {
            m.update(dt, true, false, Input::default(), 3600.);
            push(format!("F {dt} 1 0 0 0 3600"), 0, &mut m, &mut c);
        }
        m.update(
            0.,
            true,
            false,
            input(if profile % 2 == 0 { 1 << 29 } else { 1 << 7 }, 0),
            0.,
        );
        push(
            format!(
                "F 0 1 0 {:x} 0 0",
                if profile % 2 == 0 { 1u32 << 29 } else { 1 << 7 }
            ),
            0,
            &mut m,
            &mut c,
        );
        for dt in [0., 0.1, 1., 17., 0., 0.] {
            m.update(dt, true, false, Input::default(), 0.);
            push(format!("F {dt} 1 0 0 0 0"), 0, &mut m, &mut c);
        }
        push("X".into(), -1, &mut m, &mut c);
    }
    let output = native("model", &script);
    let actual = output
        .lines()
        .filter(|s| s.starts_with("M "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (line, (op, result, expected))) in actual.into_iter().zip(expected).enumerate() {
        let mut w = line.split_whitespace();
        assert_eq!(w.next(), Some("M"));
        assert_eq!(w.next().unwrap(), op.to_string());
        assert_eq!(w.next().unwrap().parse::<i32>().unwrap(), result);
        let actual = w.map(|s| s.parse::<u32>().unwrap()).collect::<Vec<_>>();
        if actual != expected {
            let diff = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect::<Vec<_>>();
            panic!("row{i}{op} differences{diff:?}\ngot{actual:?}\nwant{expected:?}");
        }
    }
}
#[test]
fn fixed_items_and_priority() {
    let mut m = Controller::new();
    assert_eq!(m.rows.clone().map(|r| r.item), [0, 1, 2, 3, 4]);
    m.update(30., true, false, Input::default(), 0.);
    m.update(
        0.,
        true,
        false,
        Input {
            up: true,
            ..Default::default()
        },
        0.,
    );
    assert_eq!(m.selected, 4);
    m.update(
        0.,
        true,
        false,
        Input {
            cancel: true,
            confirm: true,
            ..Default::default()
        },
        0.,
    );
    assert_eq!(m.phase, 2);
    assert_eq!(m.take_sounds(), ["move", "cancel"]);
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native SystemMenu render probe"]
fn ger_native_system_menu_complete_dual_texture_shadow_detail_and_text_packets() {
    let mut script = String::new();
    let mut expected = vec![];
    for phase in [0, 1, 2] {
        for selected in 0..5 {
            for (slide, flash, detail, fade, widths) in [
                (0f32, 0f32, 1f32, 0f32, [0f32, 150.25f32]),
                (0.25, 0.5, 0.75, 0.25, [264.5, 150.25]),
                (1., 0., 0., 1., [264.5, 150.25]),
                (0.9, 0.7, 0.33, 0.6, [0., 0.]),
            ] {
                let mut m = Controller::new();
                m.phase = phase;
                m.selected = selected;
                for r in &mut m.rows {
                    r.slide.snap(slide);
                    r.flash.snap(flash);
                }
                m.detail_slide.snap(detail);
                m.detail_fade.snap(fade);
                script.push_str(&format!(
                    "D {phase} {selected} {:x} {:x} {:x} {:x} {} {}\n",
                    slide.to_bits(),
                    flash.to_bits(),
                    detail.to_bits(),
                    fade.to_bits(),
                    widths[0],
                    widths[1]
                ));
                let mut p = artwork::packets(&m);
                let d = artwork::details(&m, widths);
                p.extend(d.iter().map(|d| d.panel.clone()));
                expected.push((p, d));
            }
        }
    }
    let output = native("render", &script);
    let names = output
        .lines()
        .filter_map(|l| l.strip_prefix("T "))
        .map(|l| {
            let (i, s) = l.split_once(' ').unwrap();
            (i.parse::<u32>().unwrap(), s)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let (mut case, mut packet, mut text, mut total) = (0usize, 0usize, 0usize, 0usize);
    for line in output.lines() {
        if let Some(s) = line.strip_prefix("D ") {
            case = s.parse().unwrap();
            packet = 0;
            text = 0;
        } else if let Some(s) = line.strip_prefix("P ") {
            let v = s
                .split_whitespace()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let q = &expected[case].0[packet];
            assert_eq!(v[0], q.program, "program case{case}packet{packet}");
            assert_eq!(v[1], q.blend);
            assert_eq!(names[&v[2]], q.texture);
            assert_eq!(
                &v[8..24],
                q.transform.map(f32::to_bits),
                "matrix case{case}packet{packet}"
            );
            let n = v[32] as usize;
            assert_eq!(
                &v[33..33 + n],
                q.native_vertices(),
                "vertices case{case}packet{packet}"
            );
            assert_eq!(
                v[33 + n],
                q.texture2
                    .map(|name| names
                        .iter()
                        .find(|(_, s)| **s == name)
                        .unwrap()
                        .0
                        .to_owned())
                    .unwrap_or(u32::MAX)
            );
            if q.program == 7 {
                assert_eq!(&v[24..28], q.mix.map(f32::to_bits));
                assert_eq!(&v[28..32], q.flash.map(f32::to_bits));
            }
            packet += 1;
            total += 1;
        } else if let Some(s) = line.strip_prefix("Q ") {
            let v = s
                .split_whitespace()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let q = &expected[case].1[text].text;
            assert_eq!(
                v[0],
                if q.kind == artwork::TextKind::SceneTitle {
                    1
                } else {
                    0
                }
            );
            assert_eq!(v[1], q.color);
            assert_eq!(v[2], q.outline);
            assert_eq!(
                &v[3..],
                q.transform.map(f32::to_bits),
                "text matrix case{case}text{text}"
            );
            text += 1;
        } else if line.starts_with("E ") {
            assert_eq!(packet, expected[case].0.len());
            assert_eq!(text, expected[case].1.len());
        }
    }
    println!(
        "GER_SYSTEM_MENU_RENDER profiles={} packets={total}",
        expected.len()
    );
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native SystemMenu gesture probe"]
fn ger_native_system_menu_leftward_gesture_threshold() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut push = |line: String, result: i32, m: &mut Controller, c: &mut Counts| {
        let op = line.chars().next().unwrap();
        script.push_str(&line);
        script.push('\n');
        if op != 'X' {
            expected.push((op, result, packet(m, c)));
        }
    };
    for velocity in [
        [0., 0.],
        [-20., 0.],
        [-20.001, 0.],
        [20.001, 0.],
        [-100., 0.],
        [-100., 60.],
        [-100., 57.],
        [-100., -57.],
        [-100., -60.],
        [0., 100.],
        [0., -100.],
    ] {
        let mut m = Controller::new();
        let mut c = Counts::default();
        push("N".into(), -1, &mut m, &mut c);
        m.update(20., true, false, Input::default(), 0.);
        push("F 20 1 0 0 0 0".into(), 0, &mut m, &mut c);
        m.probe([100., 72.]);
        push("T 100 72".into(), 1, &mut m, &mut c);
        m.press();
        push("P".into(), -1, &mut m, &mut c);
        m.gesture(velocity);
        push(
            format!("G {} {}", velocity[0], velocity[1]),
            -1,
            &mut m,
            &mut c,
        );
        for dt in [0.25, 1., 17., 0.] {
            m.update(dt, true, false, Input::default(), 0.);
            push(format!("F {dt} 1 0 0 0 0"), 0, &mut m, &mut c);
        }
        push("X".into(), -1, &mut m, &mut c);
    }
    let got = native("model", &script);
    let lines = got
        .lines()
        .filter(|s| s.starts_with("M "))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), expected.len());
    for (i, (line, (op, result, expected))) in lines.into_iter().zip(expected).enumerate() {
        let mut w = line.split_whitespace();
        w.next();
        assert_eq!(w.next().unwrap(), op.to_string());
        assert_eq!(w.next().unwrap().parse::<i32>().unwrap(), result);
        assert_eq!(
            w.map(|s| s.parse::<u32>().unwrap()).collect::<Vec<_>>(),
            expected,
            "gesture row{i}{op}"
        );
    }
}
