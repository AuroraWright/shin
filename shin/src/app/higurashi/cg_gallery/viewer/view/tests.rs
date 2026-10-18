use super::super::{Metadata, model_tests::Fixture};
use super::*;
use glam::{Mat4, Vec2};
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/cg-gallery-viewer-render-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/cg-gallery-viewer-render-capture/output.txt"
));
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Packet(Vec<u32>),
    Picture(Vec<u32>),
}
fn quad(program: u32, blend: u32, texture: u32, matrix: Mat4, rect: [f32; 4], color: u32) -> Event {
    let mut w = vec![program, blend, texture];
    w.extend(matrix.to_cols_array().map(f32::to_bits));
    w.extend([0; 4]);
    w.push(24 - 8 * u32::from(program == 1));
    for (x, y, u, v) in [
        (rect[0], rect[1], 0f32, 0f32),
        (rect[2], rect[1], 1., 0.),
        (rect[0], rect[3], 0., 1.),
        (rect[2], rect[3], 1., 1.),
    ] {
        w.extend([x.to_bits(), y.to_bits(), 0, color]);
        if program != 1 {
            w.extend([u.to_bits(), v.to_bits()]);
        }
    }
    Event::Packet(w)
}
#[test]
fn reference_updated_cg_viewer_original480_frames1472_packets384_picture_passes() {
    let mut original: Vec<Vec<Event>> = vec![];
    let mut ends = vec![];
    for l in OUTPUT.lines() {
        let mut w = l.split_whitespace();
        let tag = w.next().unwrap();
        if tag == "T" {
            assert_eq!(l, "T 0 check");
            continue;
        }
        let n = w.map(|s| s.parse::<u32>().unwrap()).collect::<Vec<_>>();
        match tag {
            "D" => {
                assert_eq!(n[0] as usize, original.len());
                original.push(vec![]);
            }
            "P" => original.last_mut().unwrap().push(Event::Packet(n)),
            "Q" => original.last_mut().unwrap().push(Event::Picture(n)),
            "E" => ends.push(n),
            "Y" => assert_eq!(n, [7, 0]),
            "S" => assert!(n == [4, 0, 0, 2, 131071] || n == [7, 0, 0, 0, 65535]),
            "T" => {
                assert_eq!(l, "T 0 check");
            }
            _ => panic!("Viewer original packet opcode"),
        }
    }
    let (mut packets, mut draws) = (0, 0);
    for (index, (line, expected)) in INPUT.lines().zip(original).enumerate() {
        let mut w = line.split_whitespace();
        assert_eq!(w.next(), Some("D"));
        fn u(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            w.next().unwrap().parse().unwrap()
        }
        fn i(w: &mut std::str::SplitWhitespace<'_>) -> i16 {
            w.next().unwrap().parse().unwrap()
        }
        fn f(w: &mut std::str::SplitWhitespace<'_>) -> f32 {
            w.next().unwrap().parse().unwrap()
        }
        let group = u(&mut w);
        let profile = u(&mut w);
        let width = u(&mut w);
        let height = u(&mut w);
        assert_eq!(height, width * 9 / 16);
        let phase = u(&mut w);
        let fit = u(&mut w) != 0;
        let info = u(&mut w) != 0;
        let metadata = Metadata {
            origin: [i(&mut w), i(&mut w)],
            extent: [u(&mut w) as u16, u(&mut w) as u16],
        };
        let offset = Vec2::new(f(&mut w), f(&mut w));
        let delta = u(&mut w);
        let mut fixture = Fixture::new(group, profile, metadata);
        if phase != 0 {
            fixture.worker();
            if phase >= 2 {
                fixture.tick(1.);
                if phase >= 3 {
                    fixture.tick(30.);
                    fixture.tick(1.);
                    fixture.tick(1.);
                }
            }
        }
        if phase >= 2 {
            let m = fixture.model_mut();
            m.fit = fit;
            m.offset = offset;
            m.apply_bounds(true);
            if delta != 0 {
                fixture.tick(delta as f32);
            }
        }
        fixture.model_mut().show_info = info;
        if phase == 4 {
            fixture.direct(|m, p| {
                m.close(p)?;
                Ok(-1)
            });
            if delta != 0 {
                fixture.tick(delta as f32);
            }
        }
        let m = fixture.model();
        let mut got = vec![];
        for op in plan(m) {
            match op {
                Op::Black => {
                    packets += 1;
                    got.push(quad(
                        1,
                        1,
                        u32::MAX,
                        NATIVE_UI_PROJECTION,
                        [0., 0., 1920., 1080.],
                        0xff000000,
                    ));
                }
                Op::Picture(pass) => {
                    draws += 1;
                    let mut q = vec![
                        m.picture.as_ref().unwrap().identity as u32,
                        1,
                        u32::from(pass == PassKind::Opaque),
                    ];
                    q.extend(Mat4::IDENTITY.to_cols_array().map(f32::to_bits));
                    q.extend([0; 7]);
                    got.push(Event::Picture(q));
                }
                Op::Badge { rect, color } => {
                    packets += 1;
                    got.push(quad(2, 2, 0, NATIVE_UI_PROJECTION, rect, color));
                }
                Op::Overlay { opacity } => {
                    packets += 1;
                    let mut w = vec![6, 5, 1000 + if phase == 4 { 2 } else { 1 }];
                    let matrix = Mat4::from_cols_array(&[
                        2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
                    ]);
                    w.extend(matrix.to_cols_array().map(f32::to_bits));
                    w.extend([opacity.to_bits(); 4]);
                    w.push(16);
                    for (x, y) in [(0f32, 0f32), (1., 0.), (0., 1.), (1., 1.)] {
                        w.extend([x.to_bits(), y.to_bits(), x.to_bits(), y.to_bits()]);
                    }
                    got.push(Event::Packet(w));
                }
            }
        }
        assert_eq!(
            got, expected,
            "Viewer original composition/default-root-call/full packet words {index}"
        );
        assert_eq!(ends[index][2], 1);
    }
    assert_eq!((ends.len(), packets, draws), (480, 1472, 384));
}
#[test]
#[ignore = "requires explicit frozen original runner and supplied assets; no GPU/audio/window/save"]
fn reference_updated_cg_viewer_renderer_frozen_original_reexecution() {
    let exe = std::env::var("SHIN_HIGURASHI_CG_GALLERY_VIEWER_RENDER_CAPTURE")
        .expect("explicit original runner is required");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("game-data/higurashi/uncompressed_executable_update"))
        .arg("/tmp/higurashi-newrodin.fnt")
        .arg("/tmp/higurashi-scenario-patch.rom.snr")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(INPUT.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, OUTPUT.as_bytes());
}
