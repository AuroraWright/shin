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
    let d = r.join("gerokasu2/.local/bustup-gallery");
    fs::create_dir_all(&d).unwrap();
    let path = d.join(format!("{kind}-{}-input.txt", input.len()));
    fs::write(&path, input).unwrap();
    let var = format!("SHIN_GEROKASU2_BUSTUP_{}", kind.to_uppercase());
    let bin = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| d.join(format!("native-bustup-{kind}")));
    let o = Command::new(bin)
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
fn catalog() -> Catalog {
    use catalog::*;
    Catalog {
        backgrounds: (0..4)
            .map(|i| Background {
                primary: 100 + i * 2,
                secondary: (i % 2 != 0).then_some(101 + i * 2),
            })
            .collect(),
        characters: (0..3)
            .map(|c| Character {
                outfits: (0..2 + c % 2)
                    .map(|o| Outfit {
                        poses: (0..2 + (c + o) % 2)
                            .map(|p| Pose {
                                expressions: (0..2 + (c + o + p) % 4)
                                    .map(|e| 1000 * c as u16 + 100 * o as u16 + 10 * p as u16 + e)
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}
fn buttons(p: u32, r: u32) -> Input {
    Input {
        cancel: p & (1 << 29) != 0,
        confirm: p & (1 << 28) != 0,
        up: r & (1 << 24) != 0,
        down: r & (1 << 25) != 0,
        left: r & (1 << 26) != 0,
        right: r & (1 << 27) != 0,
        previous: r & (1 << 10) != 0,
        next: r & (1 << 11) != 0,
    }
}
#[derive(Default)]
struct Counts {
    ret: u32,
    stops: u32,
    plays: u32,
    ticks: u32,
    bup: u32,
    bg: u32,
}
fn packet(m: &mut Controller, c: &mut Counts) -> Vec<u32> {
    for a in m.take_actions() {
        match a {
            Action::LoadBustup { .. } => c.bup += 1,
            Action::LoadBackground { .. } => c.bg += 1,
            Action::StopBgm(t) => {
                c.stops += 1;
                c.ticks = t;
            }
            Action::PlayBgm { .. } => c.plays += 1,
            Action::ReturnTitle { requested_id } => c.ret = requested_id as u32,
        }
    }
    let mut v = m.indices().map(|x| x as u32).to_vec();
    v.extend([
        m.focus as u32,
        m.contact as u32,
        m.selector as u32,
        m.elapsed.to_bits(),
    ]);
    for clock in [
        &m.controls_visibility,
        &m.close_flash,
        &m.confirm_flash,
        &m.panel_offset,
    ]
    .into_iter()
    .chain(m.arrows.iter())
    .chain([&m.scene_x])
    {
        v.extend([clock.value().to_bits(), clock.busy() as u32]);
    }
    v.push(m.disabled);
    m.disabled = 0;
    let mut cues = [0u32; 4];
    for s in m.take_sounds() {
        cues[["move", "decide", "cancel", "error"]
            .iter()
            .position(|x| *x == s)
            .unwrap()] += 1;
    }
    v.extend(cues);
    v.extend([c.ret, c.stops, c.plays, c.ticks, c.bup, c.bg]);
    v
}
fn compare(kind: &str, script: &str, expected: Vec<(char, i32, Vec<u32>)>) {
    let output = native(kind, script);
    compare_output(&output, expected);
}
fn compare_output(output: &str, expected: Vec<(char, i32, Vec<u32>)>) {
    let actual = output
        .lines()
        .filter(|l| l.starts_with("B "))
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (index, (line, (op, result, packet))) in actual.iter().zip(expected).enumerate() {
        let mut words = line.split_whitespace();
        assert_eq!(words.next(), Some("B"));
        assert_eq!(words.next().unwrap(), op.to_string(), "row{index}");
        assert_eq!(
            words.next().unwrap().parse::<i32>().unwrap(),
            result,
            "row{index} {op}"
        );
        let got = words.map(|x| x.parse::<u32>().unwrap()).collect::<Vec<_>>();
        if got != packet {
            let diffs = got
                .iter()
                .zip(&packet)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect::<Vec<_>>();
            panic!("row{index} {op} differences{diffs:?}\ngot{got:?}\nexpected{packet:?}");
        }
    }
}
#[test]
fn hierarchy_remembers_each_character_outfit_and_pose() {
    let mut m = Controller::new(catalog());
    m.focus = 3;
    assert!(m.change(1));
    m.focus = 2;
    assert!(m.change(1));
    m.focus = 3;
    assert!(m.change(2));
    m.focus = 0;
    assert!(m.change(1));
    assert_eq!(m.indices(), [1, 0, 0, 0, 0]);
    assert!(m.change(-1));
    assert_eq!(m.indices(), [0, 0, 1, 2, 0]);
    m.focus = 2;
    assert!(m.change(-1));
    assert_eq!(m.indices(), [0, 0, 0, 1, 0]);
}
#[test]
fn catalog_validates_order_and_wraps_secondary() {
    use catalog::Segment::*;
    let c = Catalog::from_segments([
        Background {
            primary: 65535,
            secondary_offset: 2,
        },
        EndDefinition,
        Bustup(9),
        EndExpressionGroup,
        Bustup(10),
    ])
    .unwrap();
    assert_eq!(c.backgrounds[0].secondary, Some(1));
    assert_eq!(c.characters[0].outfits[0].poses.len(), 2);
    assert!(Catalog::from_segments([Bustup(1)]).is_err());
    assert!(Catalog::from_segments([EndDefinition, EndExpressionGroup]).is_err());
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native Bustup model probe"]
fn ger_native_bustup_controller_pointer_remembered_selection_first_and_return() {
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
    for profile in 0..8 {
        let mut m = Controller::new(catalog());
        let mut c = Counts {
            ret: u32::MAX,
            ..Default::default()
        };
        push("N".into(), -1, &mut m, &mut c);
        m.assets_ready();
        push("R".into(), -1, &mut m, &mut c);
        for dt in [0., 0.25, 1., 2.5, 56.25, 0., 0., 0.] {
            m.update(dt, true, false, Input::default(), ViewerInput::default());
            push(format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
        }
        for row in 0..5 {
            m.focus = row;
            push(format!("S {row}"), -1, &mut m, &mut c);
            for delta in [-10, -1, 0, 1, 10, 1, -1, -10, 1] {
                let b = m.change(delta);
                push(format!("C {delta}"), b as i32, &mut m, &mut c);
                m.replacement_ready();
            }
        }
        for rep in [
            1 << 24,
            1 << 25,
            1 << 26,
            1 << 27,
            1 << 10,
            1 << 11,
            (1 << 24) | (1 << 27),
            (1 << 26) | (1 << 27),
        ] {
            for permission in [false, true] {
                for captured in [false, true] {
                    let b = m.controls(permission, captured, buttons(0, rep));
                    push(
                        format!("U {} {} 0 {rep:x}", permission as u8, captured as u8),
                        b as i32,
                        &mut m,
                        &mut c,
                    );
                    m.replacement_ready();
                }
            }
        }
        for (x, y) in [
            (1766., 46.),
            (1874., 46.),
            (1766., 154.),
            (1636., 46.),
            (1744., 46.),
            (1504., 216.),
            (1648., 216.),
            (1424., 248.),
            (1648., 248.),
            (1520., 360.),
            (1440., 376.),
            (1632., 376.),
            (1520., 472.),
            (1520., 584.),
            (1504., 696.),
            (1424., 728.),
            (1648., 728.),
            (0., 162.),
            (0., 980.),
            (1900., 1100.),
        ] {
            let b = m.probe([x, y]);
            push(format!("T {x} {y}"), b as i32, &mut m, &mut c);
            m.press();
            push("P".into(), -1, &mut m, &mut c);
            if x < 1700. && y > 160. && profile % 2 == 0 {
                m.release();
                push("E".into(), -1, &mut m, &mut c);
                m.replacement_ready();
            } else {
                m.cancel();
                push("K".into(), -1, &mut m, &mut c);
            }
            m.reset();
            push("Z".into(), -1, &mut m, &mut c);
        }
        let b = m.controls(true, false, buttons(1 << 29, 0));
        push("U 1 0 20000000 0".into(), b as i32, &mut m, &mut c);
        for dt in [0., 0.1, 0.5, 1., 3., 30., 0., 0.] {
            m.update(dt, true, false, Input::default(), ViewerInput::default());
            push(format!("F {dt} 0 0 0"), 0, &mut m, &mut c);
        }
        push("X".into(), -1, &mut m, &mut c);
    }
    compare("model", &script, expected);
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native Bustup resource probe"]
fn ger_native_bustup_initial_worker_publication() {
    let got = native("resources", "");
    assert_eq!(
        got,
        "FILE /bupmode.txa\nFILE /change.msk\nINSTALL 0 2:1:2000 5:1:1000 12:1:2000 13:1:2000\nINSTALL 1 2:1:2000 5:1:1001 12:1:2000 13:1:2000\nINSTALL 10 2:1:1000 5:1:1000\nWAIT bup=1 backgrounds=1\nREPLACE 10 inherited=1 ticks=10\nREPLACE 0 inherited=1 ticks=10\nREPLACE 1 inherited=1 ticks=10\n"
    );
    assert_eq!(
        background_properties(1),
        [(5, 1001.), (2, 2000.), (12, 2000.), (13, 2000.)]
    );
    assert_eq!(BUSTUP_PROPERTIES, &[(5, 1000.), (2, 1000.)]);
}

#[test]
#[ignore = "requires supplied original ARM64 executable and native Bustup render probe"]
fn ger_native_bustup_complete_selector_shell_digit_hint_packets() {
    let mut script = String::new();
    let mut expected = vec![];
    for ready in [false, true] {
        for focus in 0..5 {
            for (panel, controls, flash) in [
                (0f32, 1f32, 0f32),
                (87.5, 0.75, 0.25),
                (500., 0., 0.7),
                (-10., 0.1, 0.5),
            ] {
                let mut m = Controller::new(catalog());
                m.ready = ready;
                m.focus = focus;
                m.panel_offset.snap(panel);
                m.controls_visibility.snap(controls);
                for a in &mut m.arrows {
                    a.snap(flash);
                }
                m.close_flash.snap(flash);
                m.confirm_flash.snap(flash);
                script.push_str(&format!(
                    "D {} {focus} {:x} {:x} {:x}\n",
                    ready as u8,
                    panel.to_bits(),
                    controls.to_bits(),
                    flash.to_bits()
                ));
                expected.push(artwork::packets(&m));
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
    let (mut case, mut packet, mut total) = (0usize, 0usize, 0usize);
    for line in output.lines() {
        if let Some(n) = line.strip_prefix("D ") {
            case = n.parse().unwrap();
            packet = 0;
        } else if let Some(p) = line.strip_prefix("P ") {
            let v = p
                .split_whitespace()
                .map(|w| w.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let q = &expected[case][packet];
            assert_eq!(v[1], q.blend, "blend case{case} packet{packet}");
            assert_eq!(names[&v[2]], q.texture, "texture case{case} packet{packet}");
            assert_eq!(
                &v[8..24],
                &artwork::projection().map(f32::to_bits),
                "matrix case{case} packet{packet}"
            );
            assert_eq!(
                &v[33..],
                q.native_vertices(),
                "vertices case{case} packet{packet} texture{}",
                q.texture
            );
            if let Some(f) = q.flash.filter(|f| *f > 0.) {
                assert_eq!(v[0], 6);
                assert_eq!(&v[24..28], &[0u32; 4]);
                assert_eq!(
                    &v[28..32],
                    &[1f32.to_bits(), 1f32.to_bits(), 1f32.to_bits(), f.to_bits()]
                );
            }
            packet += 1;
            total += 1;
        } else if line.starts_with("E ") {
            assert_eq!(packet, expected[case].len(), "draw count case{case}");
        }
    }
    println!(
        "GER_BUSTUP_RENDER profiles={} packets={total}",
        expected.len()
    );
}
#[test]
#[ignore = "requires supplied original ARM64 executable and native Bustup Viewer probe"]
fn ger_native_bustup_full_viewer_tasks_inertia_drag_camera_and_return() {
    let mut script = String::new();
    let mut expected = vec![];
    let mut viewers = vec![];
    let mut push = |line: String, result: i32, m: &mut Controller, c: &mut Counts| {
        let op = line.chars().next().unwrap();
        script.push_str(&line);
        script.push('\n');
        if op != 'X' {
            expected.push((op, result, packet(m, c)));
            let mut v = m.viewer_position.map(f32::to_bits).to_vec();
            v.extend(m.viewer_velocity.map(f32::to_bits));
            for clock in &m.camera {
                v.extend([clock.value().to_bits(), clock.busy() as u32]);
            }
            viewers.push(v);
        }
    };
    for profile in 0..8 {
        let mut m = Controller::new(catalog());
        let mut c = Counts {
            ret: u32::MAX,
            ..Default::default()
        };
        push("N".into(), -1, &mut m, &mut c);
        m.assets_ready();
        push("R".into(), -1, &mut m, &mut c);
        for (dt, pressed, repeat, held, captured, axes) in [
            (60., 0, 0, 0, false, [0.; 2]),
            (0., 0, 0, 0, false, [0.; 2]),
            (0., 1 << 28, 0, 0, false, [0.; 2]),
            (0.1, 0, 0, 0, false, [0.; 2]),
            (4., 0, 0, 0, false, [0.; 2]),
            (0., 0, 0, 0, false, [0.; 2]),
            (29., 0, 0, 0, false, [0.; 2]),
            (1., 0, 0, 0, false, [0.; 2]),
            (29., 0, 0, 0, false, [0.; 2]),
            (0., 0, 0, 0, false, [0.; 2]),
            (0., 0, 0, 0, false, [0.; 2]),
        ] {
            let vi = ViewerInput {
                cancel: pressed & (1 << 29) != 0,
                confirm: pressed & (1 << 28) != 0,
                up: held & 1 != 0,
                down: held & 2 != 0,
                left: held & 4 != 0,
                right: held & 8 != 0,
                axes,
            };
            m.update(dt, true, captured, buttons(pressed, repeat), vi);
            push(
                format!(
                    "F {dt} {pressed:x} {repeat:x} {held:x} {} {} {}",
                    captured as u8, axes[0], axes[1]
                ),
                0,
                &mut m,
                &mut c,
            );
        }
        for permission in [false, true] {
            for captured in [false, true] {
                for held in [0, 1, 2, 4, 8, 15] {
                    let axes = [0.25, -0.125];
                    let vi = ViewerInput {
                        up: held & 1 != 0,
                        down: held & 2 != 0,
                        left: held & 4 != 0,
                        right: held & 8 != 0,
                        axes,
                        ..Default::default()
                    };
                    let b = m.viewer_controls(permission, captured, vi);
                    push(
                        format!(
                            "U {} {} 0 {held:x} {} {}",
                            permission as u8, captured as u8, axes[0], axes[1]
                        ),
                        b as i32,
                        &mut m,
                        &mut c,
                    );
                }
            }
        }
        for (old, new) in [
            ([10., 20.], [100., 90.]),
            ([100., 90.], [-3000., -4000.]),
            ([-3000., -4000.], [5000., 6000.]),
            ([0., 0.], [0.125, 0.75]),
        ] {
            m.viewer_drag(old, new);
            push(
                format!("D {} {} {} {}", old[0], old[1], new[0], new[1]),
                -1,
                &mut m,
                &mut c,
            );
        }
        let velocity = [profile as f32 * 1.125 - 4., profile as f32 * 0.75 - 3.];
        m.viewer_gesture(velocity);
        push(
            format!("G {} {}", velocity[0], velocity[1]),
            -1,
            &mut m,
            &mut c,
        );
        for dt in [0., 0.25, 1., 2.5, 10., 0.] {
            m.update(dt, true, false, Input::default(), ViewerInput::default());
            push(format!("F {dt} 0 0 0 0 0 0"), 0, &mut m, &mut c);
        }
        m.viewer_probe();
        push("P".into(), 1, &mut m, &mut c);
        if profile % 2 == 0 {
            m.viewer_release();
            push("E".into(), -1, &mut m, &mut c);
        } else {
            let b = m.viewer_controls(
                true,
                false,
                ViewerInput {
                    confirm: true,
                    ..Default::default()
                },
            );
            push("U 1 0 10000000 0 0 0".into(), b as i32, &mut m, &mut c);
        }
        for dt in [0., 0.25, 1., 28.75, 0., 0., 0.] {
            m.update(dt, true, false, Input::default(), ViewerInput::default());
            push(format!("F {dt} 0 0 0 0 0 0"), 0, &mut m, &mut c);
        }
        push("X".into(), -1, &mut m, &mut c);
    }
    let output = native("viewer", &script);
    compare_output(&output, expected);
    let actual = output
        .lines()
        .filter_map(|s| s.strip_prefix("V "))
        .map(|s| {
            s.split_whitespace()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), viewers.len());
    for (i, (a, b)) in actual.iter().zip(viewers).enumerate() {
        assert_eq!(a, &b, "Viewer scalar row{i}");
    }
}

#[test]
#[ignore = "requires supplied original ARM64 executable and native Bustup factory probe"]
fn ger_native_bustup_selected_factories_options_and_background_secondary() {
    assert_eq!(
        native("factories", ""),
        "BUSTUP 0 /bustup/M/fixture.bup face -32767 1\nBUSTUP 1111 /bustup/M/fixture.bup face -32767 1\nPICTURE 100 /picture/fixture.pic\nNULL\nPICTURE 102 /picture/fixture.pic\nPICTURE 103 /picture/fixture.pic\nPICTURE 104 /picture/fixture.pic\nNULL\nPICTURE 106 /picture/fixture.pic\nPICTURE 107 /picture/fixture.pic\n"
    );
}
#[test]
fn host_publication_wait_and_explicit_failed_return_recovery() {
    let mut m = Controller::new(catalog());
    assert!(!m.accepts_input());
    assert!(artwork::packets(&m).is_empty());
    m.assets_ready();
    for dt in [60., 0., 0.] {
        m.update(dt, true, false, Input::default(), ViewerInput::default());
    }
    assert!(m.accepts_input());
    m.change(1);
    assert!(m.is_loading());
    assert!(!m.accepts_input());
    m.replacement_ready();
    assert!(m.accepts_input());
    m.controls(
        true,
        false,
        Input {
            cancel: true,
            ..Default::default()
        },
    );
    for dt in [10., 0.] {
        m.update(dt, true, false, Input::default(), ViewerInput::default());
    }
    assert_eq!(m.phase, Phase::Closed);
    m.recover_return();
    assert!(m.accepts_input());
    assert_eq!(m.selected_bustup(), 1000);
    assert!(
        m.take_actions()
            .iter()
            .all(|a| matches!(a, Action::PlayBgm { id: 8, fade: 0 }))
    );
}
