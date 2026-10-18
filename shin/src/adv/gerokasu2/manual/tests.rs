use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn root() -> PathBuf {
    std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("PWD")))
}
fn native(binary: &str, input: &str) -> String {
    let root = root();
    let tool = std::env::var_os(format!("SHIN_GEROKASU2_MANUAL_{}", binary.to_uppercase()))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join(format!(
                "reverse-engineering/gerokasu2/manual/native-manual-{binary}"
            ))
        });
    let mut child = Command::new(tool)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&bytes).unwrap();
    });
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stderr));
    String::from_utf8(result.stdout).unwrap()
}
fn input(pressed: u32, repeat: u32) -> Input {
    Input {
        left: repeat & (1 << 26) != 0,
        right: repeat & (1 << 27) != 0,
        previous: repeat & (1 << 10) != 0,
        next: repeat & (1 << 11) != 0,
        close: pressed & (1 << 29) != 0,
    }
}
fn kind(k: Kind) -> u32 {
    match k {
        Kind::First => 0,
        Kind::Open => 1,
        Kind::Input => 2,
        Kind::CloseWait => 3,
        Kind::Close => 4,
    }
}
fn snapshot(m: &mut Controller, op: char, result: i32) -> String {
    let mut fields = vec![
        u32::from(m.complete).to_string(),
        m.progress.to_bits().to_string(),
        m.contact.to_string(),
        u32::from(m.swipe).to_string(),
        u32::from(m.closing).to_string(),
        m.count.to_string(),
        (m.selected as u32).to_string(),
        m.scroll().to_bits().to_string(),
        u32::from(m.scroll_busy()).to_string(),
        m.close_flash().to_bits().to_string(),
        u32::from(m.flash_busy()).to_string(),
        m.disabled.to_string(),
    ];
    m.disabled = 0;
    let cues = m.take_sounds();
    for cue in ["move", "decide", "cancel"] {
        fields.push(cues.iter().filter(|s| **s == cue).count().to_string());
    }
    let actions = m.take_actions();
    fields.push(actions.len().to_string());
    for Action::ReturnSource(stage) in actions {
        fields.push(stage.to_string());
    }
    fields.push(m.tasks.len().to_string());
    for t in &m.tasks {
        fields.extend([kind(t.kind).to_string(), u32::from(t.done).to_string()]);
    }
    format!("M {op} {result} {}", fields.join(" "))
}
fn compare_model(protocol: &str) {
    let native = native("model", protocol);
    let mut model = None;
    let mut available = 0;
    let mut capture = false;
    let mut expected = vec![];
    for line in protocol.lines() {
        let mut words = line.split_whitespace();
        let op = words.next().unwrap().chars().next().unwrap();
        let args: Vec<_> = words.collect();
        let mut result = -1;
        match op {
            'N' => {
                available = args[0].parse().unwrap();
                capture = false;
                model = Some(Controller::new());
            }
            'X' => {
                model = None;
                expected.push("X".to_owned());
                continue;
            }
            _ => {
                let m = model.as_mut().unwrap();
                match op {
                    'R' => m.assets_ready(available),
                    'F' => {
                        m.update(
                            args[1].parse().unwrap(),
                            args[0] == "1",
                            capture,
                            Input::default(),
                        );
                        result = 0;
                    }
                    'U' => {
                        capture = args[1] == "1";
                        result = i32::from(m.poll_controls(
                            args[0] == "1",
                            args[1] == "1",
                            input(
                                u32::from_str_radix(args[2], 16).unwrap(),
                                u32::from_str_radix(args[3], 16).unwrap(),
                            ),
                        ));
                    }
                    'V' => {
                        result =
                            i32::from(m.probe(args[0].parse().unwrap(), args[1].parse().unwrap()))
                    }
                    'P' => m.press(),
                    'E' => m.release(),
                    'Z' => m.reset(),
                    'C' => m.cancel(),
                    'G' => m.drag(args[0].parse().unwrap(), args[2].parse().unwrap()),
                    'I' => m.gesture(args[0].parse().unwrap(), args[1].parse().unwrap()),
                    _ => panic!("opcode"),
                }
            }
        }
        expected.push(snapshot(model.as_mut().unwrap(), op, result));
    }
    let lines: Vec<_> = native.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (i, (actual, expected)) in lines.iter().zip(expected).enumerate() {
        let prefix = if *actual == "X" {
            actual.to_string()
        } else {
            let mut fields: Vec<_> = actual.split_whitespace().collect();
            fields.truncate(fields.len() - 5);
            fields.join(" ")
        };
        assert_eq!(
            prefix,
            expected,
            "operation {i}: {}",
            protocol.lines().nth(i).unwrap()
        );
    }
}
#[test]
fn manual_lifecycle_waits_for_publication_flash_and_parent_final_stage() {
    let mut m = Controller::new();
    m.update(100., true, false, Input::default());
    assert_eq!(m.phase, Phase::Loading);
    m.assets_ready(15);
    m.update(1., true, false, Input::default());
    assert_eq!(m.phase, Phase::Opening);
    assert_eq!(m.progress, 0.);
    m.update(10., true, false, Input::default());
    assert_eq!(m.phase, Phase::Active);
    m.update(
        0.,
        true,
        false,
        Input {
            close: true,
            ..Default::default()
        },
    );
    assert_eq!(m.phase, Phase::CloseFlash);
    m.update(7., false, false, Input::default());
    assert!(m.take_actions().is_empty());
    m.update(0., false, false, Input::default());
    assert_eq!(m.take_actions(), [Action::ReturnSource(0)]);
    assert_eq!(m.phase, Phase::Closing);
    m.update(10., false, false, Input::default());
    assert_eq!(m.take_actions(), [Action::ReturnSource(1)]);
    assert_eq!(m.phase, Phase::Closed);
}
#[test]
fn manual_pointer_close_bounds_and_numbered_page_geometry() {
    let mut m = Controller::new();
    m.assets_ready(15);
    for (x, y, contact) in [
        (1765.99, 46., 0),
        (1766., 46., 1),
        (1874., 46., 0),
        (1800., 154., 0),
        (-100., 162., -1),
        (4000., 979.99, -1),
        (100., 980., 0),
    ] {
        assert!(m.probe(x, y));
        assert_eq!(m.contact, contact);
    }
    let p = artwork::packets_for(true, 15, 1., 0., 0., false, crate::cli::Game::Gerokasu2);
    assert_eq!(p[2].texture, "001");
    assert_eq!(p[2].rect, [240., 166., 1680., 976.]);
    assert!(p.iter().all(|p| p.texture != "obi"));
    assert_eq!(
        p.iter()
            .filter(|p| p.texture.parse::<usize>().is_ok())
            .count(),
        2
    );
}
#[test]
#[ignore = "requires original Ger executable/fonts and compiled CPU Manual probe"]
fn ger_native_manual_controller_pointer_lifecycle_and_exact_tweens() {
    let mut p = String::new();
    for count in [0, 1, 15, 999] {
        p.push_str(&format!(
            "N {count}\nF 1 0\nF 0 2\nR\nF 1 1\nF 1 2.5\nF 1 7.5\nF 1 0\n"
        ));
        for repeat in [
            0,
            1 << 26,
            1 << 27,
            1 << 10,
            1 << 11,
            (1 << 26) | (1 << 27),
            (1 << 27) | (1 << 10),
            (1 << 10) | (1 << 11),
            1 << 7,
            1 << 5,
        ] {
            for permission in 0..2 {
                for captured in 0..2 {
                    p.push_str(&format!(
                        "U {permission} {captured} 0 {repeat:x}\nF 1 3.25\nF 1 20\n"
                    ));
                }
            }
        }
        for (x, y) in [
            (1765., 46.),
            (1766., 46.),
            (1873., 153.),
            (1874., 153.),
            (1800., 154.),
            (100., 161.),
            (-500., 162.),
            (3000., 979.),
            (0., 980.),
        ] {
            p.push_str(&format!("V {x} {y}\nP\nF 0 1.25\nC\nZ\n"));
        }
        p.push_str("V 400 400\nG 400 400 300 450\nG 300 450 150 450\nZ\nF 1 2.5\nF 1 15\n");
        for (x, y) in [
            (-20., 0.),
            (-20.001, 0.),
            (20., 0.),
            (20.001, 0.),
            (-100., 0.),
            (100., 0.),
            (0., 100.),
            (100., 100.),
            (-100., 10.),
            (100., -10.),
        ] {
            p.push_str(&format!(
                "V 400 400\nG 400 400 0 400\nI {x} {y}\nZ\nF 1 0.25\nF 1 1.5\nF 1 10000\n"
            ));
        }
        p.push_str(
            "U 1 0 20000000 0\nF 1 1\nF 1 2.5\nF 1 3.5\nF 1 0\nF 1 2.5\nF 1 7.5\nF 1 0\nX\n",
        );
    }
    p.push_str("N 15\nR\nF 1 1\nF 1 10\nV 1800 60\nP\nF 1 1.75\nE\nF 0 1.75\nF 0 3.5\nF 1 0\nF 1 10\nX\nN 15\nX\n");
    compare_model(&p);
}
#[test]
#[ignore = "requires original Ger executable/fonts and compiled CPU Manual render probe"]
fn ger_native_manual_complete_page_shell_hint_vertex_and_matrix_packets() {
    let mut profiles = vec![];
    for ready in [false, true] {
        for progress in [0., 0.125, 0.5, 0.9, 1.] {
            for scroll in [0., 100., 750., 1499.5, 1500., 21000.] {
                for flash in [0., 0.35, 0.7] {
                    for swapped in [false, true] {
                        profiles.push((ready, 15, progress, scroll, flash, swapped));
                    }
                }
            }
        }
    }
    for count in [0, 1, 2, 18] {
        profiles.push((true, count, 1., 0., 0., false));
    }
    let mut protocol = String::new();
    for &(ready, count, progress, scroll, flash, swapped) in &profiles {
        protocol.push_str(&format!(
            "D {count} {} {:x} {:x} {:x} {}\n",
            u32::from(ready),
            f32::to_bits(progress),
            f32::to_bits(scroll),
            f32::to_bits(flash),
            u32::from(swapped)
        ));
    }
    let native = native("render", &protocol);
    let names: std::collections::HashMap<u32, &str> = native
        .lines()
        .filter(|l| l.starts_with("T "))
        .map(|l| {
            let mut w = l.split_whitespace();
            w.next();
            (w.next().unwrap().parse().unwrap(), w.next().unwrap())
        })
        .collect();
    let mut lines = native.lines().filter(|l| !l.starts_with("T "));
    assert_eq!(lines.next().unwrap(), "B 2 1 15 1 1 5 6 8 2");
    assert_eq!(lines.next().unwrap(), "B 3 1 15 1 1 5 2 8 2");
    let mut packets = 0;
    for (i, &(ready, count, progress, scroll, flash, swapped)) in profiles.iter().enumerate() {
        let header = lines.next().unwrap();
        assert!(header.starts_with(&format!("D {i} ")));
        let expected = artwork::packets_for(
            ready,
            count,
            progress,
            scroll,
            flash,
            swapped,
            crate::cli::Game::Gerokasu2,
        );
        for (j, p) in expected.iter().enumerate() {
            let line = lines.next().unwrap();
            assert!(line.starts_with("P "), "missing packet {i}/{j}: {line}");
            let w: Vec<u32> = line
                .split_whitespace()
                .skip(1)
                .map(|v| v.parse().unwrap())
                .collect();
            let program = if p.texture.is_empty() {
                1
            } else if p.flash > 0. {
                6
            } else {
                2
            };
            assert_eq!(w[0], program, "program profile{i}/packet{j}");
            assert_eq!(w[1], p.blend, "blend profile{i}/packet{j}");
            if p.texture.is_empty() {
                assert_eq!(w[2], u32::MAX);
            } else {
                assert_eq!(names[&w[2]], p.texture);
            }
            assert_eq!(&w[3..8], p.stencil);
            assert_eq!(
                &w[8..24],
                p.matrix.map(f32::to_bits),
                "matrix profile{i}/packet{j}"
            );
            let mut uniforms = [0u32; 8];
            if p.flash > 0. {
                uniforms[4..].copy_from_slice(&[
                    1.0f32.to_bits(),
                    1.0f32.to_bits(),
                    1.0f32.to_bits(),
                    p.flash.to_bits(),
                ]);
            }
            assert_eq!(&w[24..32], uniforms);
            let vertices = p.native_vertices();
            assert_eq!(w[32] as usize, vertices.len());
            assert_eq!(&w[33..], vertices, "vertices profile{i}/packet{j}");
            packets += 1;
        }
        assert_eq!(
            lines.next().unwrap(),
            format!("E {} 1 {}", expected.len(), u32::from(ready))
        );
    }
    assert!(lines.next().is_none());
    eprintln!(
        "GER_MANUAL_RENDER_COMPARE profiles={} packets={packets}",
        profiles.len()
    );
}
#[test]
fn gerokasu_manual_adds_sliding_bottom_band() {
    use crate::cli::Game;
    let base = artwork::packets_for(true, 3, 0.5, 0., 0., false, Game::Gerokasu2);
    let ger = artwork::packets_for(true, 3, 0.5, 0., 0., false, Game::Gerokasu);
    assert!(base.iter().all(|p| p.texture != "bottom"));
    assert_eq!(base.iter().find(|p| p.texture == "top").unwrap().blend, 3);
    assert_eq!(ger.iter().find(|p| p.texture == "top").unwrap().blend, 2);
    assert_eq!(ger.len(), base.len() + 1);
    let i = ger.iter().position(|p| p.texture == "bottom").unwrap();
    assert_eq!(ger[i - 1].texture, "close");
    assert_eq!(ger[i].rect, [0., 1036., 1920., 1124.]);
    assert_eq!(ger[i].uv, [0., 0., 1., 1.]);
    assert_eq!((ger[i].color, ger[i].blend), (0xd8ffffff, 2));
}
