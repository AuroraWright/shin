//! Exact raw-word comparisons against93e28/94470/d2240/c0574/55548, with original
//! runner reexecution required. This test does not claim shader/pixel parity.
use super::super::{Model, Task};
use super::*;
use crate::adv::higurashi::{browser_controls::Scrollbar, reconstruction_mode::overlay::DrawPacket};
use serde::Deserialize;
use shin_core::{
    format::{font::read_font_metrics, scenario::higurashi::HigurashiScenario},
};
use std::{
    collections::HashMap,
    io::{BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};
#[derive(Deserialize)]
struct Manifest {
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    index: usize,
    name: String,
    mask: u32,
    append: u32,
    capture: u32,
    phase: u32,
    scroll: f32,
    selected: usize,
    frame: u32,
    close: f32,
    layout: u32,
    height: i32,
    viewer: u32,
    #[serde(rename = "volume")]
    _volume: u32,
    opacity: f32,
    backdrop: f32,
    focus: Vec<(usize, f32)>,
    synthetic_scrollbar_height: bool,
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
fn evidence() -> PathBuf {
    root().join("reverse-engineering/higurashi/movie-gallery-render-capture")
}
fn required(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Set {name}; original render comparison cannot be skipped"))
}
fn reexecute() -> String {
    let folder = evidence();
    let input = std::fs::read(folder.join("input.txt")).unwrap();
    let golden = std::fs::read(folder.join("output.txt")).unwrap();
    let stderr = std::fs::read(folder.join("stderr.txt")).unwrap();
    assert_eq!(
        golden,
        std::fs::read(folder.join("output-repeat.txt")).unwrap()
    );
    assert_eq!(
        stderr,
        std::fs::read(folder.join("stderr-repeat.txt")).unwrap()
    );
    for run in 0..2 {
        let mut child = Command::new(required("SHIN_HIGURASHI_MOVIE_GALLERY_RENDER_NATIVE"))
            .arg(root().join("higurashi/uncompressed_executable_update"))
            .arg(required("SHIN_HIGURASHI_MOVIE_GALLERY_FONT"))
            .arg(required("SHIN_HIGURASHI_MOVIE_GALLERY_RENDER_SCENARIO"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let bytes = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(&bytes).unwrap());
        let result = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            result.status.success(),
            "Original Movie render{run}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            result.stdout == golden,
            "Original renderer stdout drift run{run}"
        );
        assert!(
            result.stderr == stderr,
            "Original renderer stderr/service/retirement drift run{run}"
        );
    }
    String::from_utf8(golden).unwrap()
}
struct Native {
    header: Vec<u32>,
    matrix: [u32; 16],
    uniforms: [Vec<u32>; 2],
    vertices: Vec<u32>,
}
struct Original {
    metadata: Vec<u32>,
    draws: Vec<Native>,
    end: Vec<u32>,
    retired: Vec<u32>,
}
fn words(line: &str) -> Vec<u32> {
    line.split_whitespace()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect()
}
fn parse(output: &str) -> (Vec<Original>, HashMap<(u32, u32), String>) {
    let mut originals: Vec<Original> = vec![];
    let mut names = HashMap::new();
    for line in output.lines() {
        let op = line.split_whitespace().next().unwrap();
        if op == "T" {
            let mut t = line.split_whitespace().skip(1);
            names.insert(
                (
                    t.next().unwrap().parse().unwrap(),
                    t.next().unwrap().parse().unwrap(),
                ),
                t.next().unwrap().to_owned(),
            );
            continue;
        }
        if op == "E" {
            let _: serde_json::Value =
                serde_json::from_str(line.split_once(' ').unwrap().1).unwrap();
            continue;
        }
        let w = words(line);
        if op == "H" {
            assert_eq!(w[0] as usize, originals.len());
            originals.push(Original {
                metadata: vec![],
                draws: vec![],
                end: vec![],
                retired: vec![],
            });
            continue;
        }
        if op == "D" {
            assert_eq!(w[0] as usize, originals.len() - 1);
            continue;
        }
        let original = originals.last_mut().unwrap();
        match op {
            "M" => original.metadata = w,
            "P" => {
                assert_eq!(w.len(), 61 + w[9] as usize);
                original.draws.push(Native {
                    header: w[..13].to_vec(),
                    matrix: w[13..29].try_into().unwrap(),
                    uniforms: [w[29..45].to_vec(), w[45..61].to_vec()],
                    vertices: w[61..].to_vec(),
                });
            }
            "F" => original.end = w,
            "X" => original.retired = w,
            "U" => assert_eq!(w.len(), 2 + w[1] as usize / 4),
            "B" => assert_eq!(w[3], 0),
            "Z" => assert!(w.len() == 1 && w[0] <= 2),
            _ => panic!("Unknown original Movie packet transport"),
        }
    }
    (originals, names)
}
struct Expected {
    program: u32,
    blend: u32,
    source: u32,
    name: String,
    point: u32,
    mask: bool,
    count: u32,
    start: u32,
    matrix: [u32; 16],
    uniforms: [Vec<u32>; 2],
    vertices: Vec<u32>,
}
fn expected(
    state: &ViewState,
    profile: &Profile,
    font: impl FontMetrics + Clone,
) -> (Vec<Expected>, Layout) {
    let layout = completion_layout(font, &state.completion).unwrap();
    let mut out = vec![];
    let mut scroll = 0;
    for draw in packets(state, layout.size, profile.layout != 0) {
        match draw {
            Draw::Quad(q) => {
                let [l, t, r, b] = q.rect;
                let [u, v, s, w] = q.uv;
                let mut vertices = vec![];
                for i in 0..4 {
                    vertices.extend(
                        [
                            if i & 1 == 0 { l } else { r },
                            if i & 2 == 0 { t } else { b },
                            0.,
                        ]
                        .map(f32::to_bits),
                    );
                    vertices.push(q.color);
                    vertices.extend(
                        [
                            if i & 1 == 0 { u } else { s },
                            if i & 2 == 0 { v } else { w },
                        ]
                        .map(f32::to_bits),
                    );
                }
                out.push(Expected {
                    program: if q.flash > 0. { 5 } else { 2 },
                    blend: if q.opaque { 1 } else { 2 },
                    source: 0,
                    name: q.texture,
                    point: 0,
                    mask: false,
                    count: 4,
                    start: 0,
                    matrix: q.matrix.to_cols_array().map(f32::to_bits),
                    uniforms: [
                        vec![
                            1f32.to_bits(),
                            1f32.to_bits(),
                            1f32.to_bits(),
                            q.flash.to_bits(),
                        ],
                        vec![],
                    ],
                    vertices,
                });
            }
            Draw::Scrollbar { matrix, vertices } => {
                let mut raw = vec![];
                for v in vertices {
                    let pos = v.position;
                    let uv = v.texture_position;
                    let color = v.color;
                    raw.extend(pos.to_array().map(f32::to_bits));
                    raw.push(color.0);
                    raw.extend(uv.to_array().map(f32::to_bits));
                }
                out.push(Expected {
                    program: 2,
                    blend: 2,
                    source: 1,
                    name: "scroll".into(),
                    point: 0,
                    mask: false,
                    count: 8,
                    start: scroll * 8,
                    matrix: matrix.to_cols_array().map(f32::to_bits),
                    uniforms: [vec![], vec![]],
                    vertices: raw,
                });
                scroll += 1;
            }
            Draw::Completion(matrix) => {
                for border in [true, false] {
                    for (i, glyph) in layout.glyphs.iter().enumerate() {
                        let mut vertices = vec![];
                        for v in glyph.vertices {
                            let pos = v.position;
                            vertices.extend(pos.to_array().map(f32::to_bits));
                            vertices.push(v.color.to_bits());
                        }
                        out.push(Expected {
                            program: if border { 4 } else { 3 },
                            blend: 2,
                            source: 2,
                            name: String::new(),
                            point: glyph.point as u32,
                            mask: false,
                            count: 4,
                            start: i as u32 * 4,
                            matrix: matrix.to_cols_array().map(f32::to_bits),
                            uniforms: [
                                if border {
                                    glyph
                                        .border
                                        .iter()
                                        .flat_map(|v| v.to_array().map(f32::to_bits))
                                        .collect()
                                } else {
                                    vec![1f32.to_bits(); 4]
                                },
                                if border {
                                    vec![0, 0, 0, (240. * f32::from_bits(0x3b808081)).to_bits()]
                                } else {
                                    vec![1f32.to_bits(); 4]
                                },
                            ],
                            vertices,
                        });
                    }
                }
            }
        }
    }
    // This reuses the audited original55380..55548 clock/packet adapter, not
    // fabricated parent geometry. It exercises actual First phase ordering.
    let mut task = Task::first(profile.capture != 0);
    if let Task::First { phase, overlay } = &mut task {
        if profile.phase >= 2 {
            overlay.masked(30, f32::from_bits(0x3e99999a), 0, true);
            *phase = 1;
        }
        let elapsed = match profile.phase {
            3 => 15.,
            4 => 29.5,
            5 | 6 => 30.,
            _ => 0.,
        };
        overlay.advance(shin_core::time::Ticks::from_f32(elapsed));
        let packet = overlay.draw_packet(if overlay.mask_present {
            Some((1920, 1080))
        } else {
            None
        });
        if let Some(packet) = packet {
            let mut vertices = vec![];
            let (program, uniforms) = match packet {
                DrawPacket::Plain {
                    vertices: quad,
                    opacity,
                } => {
                    for v in quad {
                        let pos = v.position;
                        let uv = v.texture_position;
                        vertices.extend(pos.to_array().map(f32::to_bits));
                        vertices.extend(uv.to_array().map(f32::to_bits));
                    }
                    (6, [vec![opacity.to_bits(); 4], vec![]])
                }
                DrawPacket::Masked {
                    vertices: quad,
                    minmax,
                } => {
                    for v in quad {
                        let pos = v.position;
                        let uv = v.texture_position;
                        let mask = v.mask_position;
                        vertices.extend(pos.to_array().map(f32::to_bits));
                        vertices.extend(uv.to_array().map(f32::to_bits));
                        vertices.extend(mask.to_array().map(f32::to_bits));
                    }
                    (
                        18,
                        [
                            vec![minmax.x.to_bits(), minmax.y.to_bits(), 0, 0],
                            vec![1f32.to_bits(); 4],
                        ],
                    )
                }
            };
            let matrix = [
                2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
            ]
            .map(f32::to_bits);
            out.push(Expected {
                program,
                blend: 5,
                source: 3,
                name: String::new(),
                point: 0,
                mask: program == 18,
                count: 4,
                start: 0,
                matrix,
                uniforms,
                vertices,
            });
        }
    }
    if profile.viewer != 0 {
        // Independent checked-frame transport fixture for actual95e90/90d34.
        // These82 private packets are not the parent View implementation.
        let alpha = |v: f32| ((v.max(0.) * 255.) as u32).min(255) << 24;
        let mut vertices = vec![];
        for i in 0..4 {
            vertices.extend(
                [
                    if i & 1 == 0 { 0. } else { 1920. },
                    if i & 2 == 0 { 0. } else { 1080. },
                    0.,
                ]
                .map(f32::to_bits),
            );
            vertices.push(alpha(profile.backdrop));
        }
        out.push(Expected {
            program: 1,
            blend: if profile.backdrop < 1. { 2 } else { 1 },
            source: 5,
            name: String::new(),
            point: 0,
            mask: false,
            count: 4,
            start: 0,
            matrix: projection().to_cols_array().map(f32::to_bits),
            uniforms: [vec![], vec![]],
            vertices,
        });
        // Viewer1 has desired=false; Viewer3 has no retained frame. Actual
        // Viewer2 does submit Sprite even for opacity0, after gain/color.
        if profile.viewer == 2 {
            let mut vertices = vec![];
            for i in 0..4 {
                vertices.extend(
                    [
                        if i & 1 == 0 { 0. } else { 1. },
                        if i & 2 == 0 { 0. } else { 1. },
                        0.,
                    ]
                    .map(f32::to_bits),
                );
                vertices.push(0xffffff | alpha(profile.opacity));
                vertices.extend(
                    [
                        if i & 1 == 0 { 0.125 } else { 0.875 },
                        if i & 2 == 0 { 0.25 } else { 0.75 },
                    ]
                    .map(f32::to_bits),
                );
            }
            out.push(Expected {
                program: 2,
                blend: if profile.opacity < 1. { 2 } else { 1 },
                source: 6,
                name: String::new(),
                point: 0,
                mask: false,
                count: 4,
                start: 0,
                matrix: [
                    2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -1., 0., -1., 1., -0., 1.,
                ]
                .map(f32::to_bits),
                uniforms: [vec![], vec![]],
                vertices,
            });
        }
    }
    (out, layout)
}
fn prepared_state(p: &Profile, scenario: &HigurashiScenario) -> ViewState {
    let mut m = Model::new(&scenario.movie_info, &[p.mask], p.append != 0).unwrap();
    m.x = (p.selected % 4) as i32;
    m.y = (p.selected / 4) as i32;
    m.frame = p.frame;
    m.scroll.fast_forward_to(p.scroll);
    m.close_flash.fast_forward_to(p.close);
    m.document_height = p.height;
    // Original native phase setup advances default selected0's focus only
    // for positive atlas-backed delta; profile then changes selection.
    if p.phase >= 3 {
        m.rows[0].focus = 1.;
    }
    for &(index, value) in &p.focus {
        m.rows[index].focus = value;
    }
    m.scrollbar = Scrollbar {
        position: p.scroll as i32,
        visible: 720,
        first: 0,
        end: p.height,
    };
    m.view_state(p.phase != 0)
}
#[test]
#[ignore = "requires frozen original Movie RENDER_NATIVE/FONT/RENDER_SCENARIO; twice exact replay; CPU only"]
fn higurashi_movie_parent_packets_match_original_230_profiles_9082_packets() {
    let output = reexecute();
    let (originals, names) = parse(&output);
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(evidence().join("profiles.json")).unwrap()).unwrap();
    assert_eq!(originals.len(), 230);
    assert_eq!(manifest.profiles.len(), 230);
    let scenario = HigurashiScenario::new(
        std::fs::read(required("SHIN_HIGURASHI_MOVIE_GALLERY_RENDER_SCENARIO"))
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(scenario.movie_info.len(), 13);
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(required("SHIN_HIGURASHI_MOVIE_GALLERY_FONT")).unwrap(),
        ))
        .unwrap(),
    );
    let mut count = 0;
    let (mut parent_packets, mut private_packets, mut synthetic_profiles) = (0, 0, 0);
    for (i, (p, n)) in manifest.profiles.iter().zip(&originals).enumerate() {
        assert_eq!(i, p.index);
        let ctx = format!("Movie profile{i} {}", p.name);
        let state = prepared_state(p, &scenario);
        let (actual, layout) = expected(&state, p, font.clone());
        assert_eq!(n.metadata.len(), 87);
        assert_eq!(
            n.metadata[..2],
            [u32::from(p.phase != 0), (p.height as f32).to_bits()],
            "{ctx} publication/document"
        );
        assert_eq!(
            n.metadata[2..4],
            layout.size.to_array().map(f32::to_bits),
            "{ctx} native NewRodin measured label"
        );
        assert_eq!(n.metadata[4] as usize, layout.glyphs.len());
        assert_eq!(n.metadata[5], 1);
        assert_eq!(
            n.metadata[6..14],
            [
                1,
                720f32.to_bits(),
                0,
                0,
                p.scroll as i32 as u32,
                720,
                0,
                p.height as u32
            ],
            "{ctx} actual scrollbar storage"
        );
        assert_eq!(n.metadata[14], 12);
        for (index, (row, raw)) in state
            .rows
            .iter()
            .zip(n.metadata[15..].chunks_exact(6))
            .enumerate()
        {
            assert_eq!(raw[0], index as u32);
            assert_eq!(raw[1], u32::from(row.movie_id));
            assert_eq!(
                raw[2] & 65535,
                u32::from(row.append) | (u32::from(row.locked) << 8)
            );
            assert_eq!(
                raw[3..],
                [row.x.to_bits(), row.y.to_bits(), row.focus.to_bits()]
            );
        }
        assert_eq!(
            p.synthetic_scrollbar_height,
            p.height != 720,
            "{ctx} explicit synthetic seam"
        );
        assert_eq!(
            actual.len(),
            n.draws.len(),
            "{ctx} native parent+private packet count"
        );
        for (j, (a, b)) in actual.iter().zip(&n.draws).enumerate() {
            let ctx = format!("{ctx} draw{j}");
            assert_eq!(
                b.header[9] as usize,
                a.vertices.len(),
                "{ctx} vertex upload size"
            );
            assert_eq!(b.header[10], 64, "{ctx} matrix upload size");
            assert_eq!(
                [
                    a.program,
                    a.blend,
                    a.source,
                    if a.mask { 4 } else { 5 },
                    0,
                    1,
                    a.count,
                    a.start
                ],
                [
                    b.header[0],
                    b.header[1],
                    b.header[2],
                    b.header[4],
                    b.header[5],
                    b.header[6],
                    b.header[7],
                    b.header[8]
                ],
                "{ctx} shader/blend/source/primitive/subrange"
            );
            if a.source < 2 {
                assert_eq!(
                    &a.name,
                    &names[&(a.source, b.header[3])],
                    "{ctx} actual atlas name"
                );
            } else {
                assert_eq!(a.point, b.header[3], "{ctx} codepoint/capture handle");
            }
            assert_eq!(a.matrix, b.matrix, "{ctx} native matrix");
            assert_eq!(a.vertices, b.vertices, "{ctx} exact vertices");
            // Original slots retain unused old uploads across program changes.
            // Compare only each program's declared inputs, never stale padding.
            for slot in 0..2 {
                let expected = &a.uniforms[slot];
                if a.program == 2 || expected.is_empty() {
                    continue;
                }
                assert_eq!(
                    b.header[11 + slot] as usize,
                    expected.len() * 4,
                    "{ctx} declared uniform{} upload size",
                    slot + 1
                );
                assert_eq!(
                    &b.uniforms[slot][..expected.len()],
                    expected,
                    "{ctx} declared uniform{}",
                    slot + 1
                );
            }
        }
        assert_eq!(
            n.end,
            [
                actual.len() as u32,
                1,
                if p.capture != 0 && p.phase < 5 { 3 } else { 0 },
                1 + u32::from(p.viewer != 0),
                1
            ],
            "{ctx} native borrowed draw/overlay state"
        );
        assert_eq!(
            n.retired,
            [
                i as u32,
                layout.glyphs.len() as u32,
                u32::from(p.phase > 0),
                p.capture,
                1,
                u32::from(p.viewer != 0),
                u32::from(p.viewer != 0),
                1
            ],
            "{ctx} actual complete owner/font/buffer/Player retirement"
        );
        parent_packets += actual.iter().filter(|a| a.source < 3).count();
        private_packets += actual.iter().filter(|a| a.source >= 3).count();
        synthetic_profiles += usize::from(p.synthetic_scrollbar_height);
        count += actual.len();
    }
    assert_eq!(count, 9082);
    assert_eq!(
        (parent_packets, private_packets, synthetic_profiles),
        (9000, 82, 21)
    );
    println!(
        "Movie:230 original profiles/9082 exact packets; parent9000,private82 transport fixtures;21 synthetic scrollbar profiles; no GPU/audio/app acceptance"
    );
}

#[path = "view_tests/pixel_tests.rs"]
mod pixel_tests;
