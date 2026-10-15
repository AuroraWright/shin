//! Exact raw-word comparisons against3001c/d2240/c0574/55548, with original
//! runner reexecution required. This test does not claim shader/pixel parity.
use super::super::{
    Task,
    model_tests::{self, evidence},
};
use super::*;
use crate::adv::higurashi::reconstruction_mode::overlay::DrawPacket;
use serde::Deserialize;
use shin_core::format::font::read_font_metrics;
use std::{collections::HashMap, io::BufReader};
#[path = "pixel_tests.rs"]
mod pixel_tests;

#[derive(Deserialize)]
struct Manifest {
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    index: usize,
    name: String,
    unlock: u32,
    append: u32,
    capture: u32,
    phase: u32,
    scroll: f32,
    selected: i32,
    playing: u32,
    elapsed: f32,
    close: f32,
    layout: u32,
    flashes: Vec<(usize, f32)>,
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
        let w = words(line);
        if op == "D" {
            assert_eq!(w[0] as usize, originals.len());
            originals.push(Original {
                metadata: vec![],
                draws: vec![],
                end: vec![],
                retired: vec![],
            });
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
            "E" => original.end = w,
            "X" => original.retired = w,
            "U" => assert_eq!(w.len(), 2 + w[1] as usize / 4),
            "B" => assert_eq!(w[3], 0),
            "Z" => assert!(w.len() == 1 && w[0] <= 2),
            _ => panic!("Unknown original Music packet transport"),
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
    m: &Model,
    profile: &Profile,
    font: impl FontMetrics + Clone,
) -> (Vec<Expected>, Layout) {
    let layout = completion_layout(font, &m.completion_text).unwrap();
    let mut out = vec![];
    let mut scroll = 0;
    for draw in packets(
        m,
        profile.phase != 0,
        m.scrollbar,
        layout.size,
        profile.layout != 0,
    ) {
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
    (out, layout)
}
#[test]
#[ignore = "305 actual original Music parent/font/scrollbar/First profiles; requires SHIN_HIGURASHI_MUSIC_RENDER_NATIVE, MUSIC_FONT and MUSIC_SCENARIO; CPU only"]
fn higurashi_music_parent_packets_match_actual_native_305_profiles() {
    let output = model_tests::reexecute("render", "SHIN_HIGURASHI_MUSIC_RENDER_NATIVE");
    let (originals, names) = parse(&output);
    let manifest: Manifest = serde_json::from_slice(
        &std::fs::read(evidence().join("music-gallery-render-capture/profiles.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(originals.len(), 305);
    assert_eq!(manifest.profiles.len(), 305);
    let scenario = model_tests::scenario();
    assert_eq!(scenario.music_box_info.len(), 158);
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(model_tests::required("SHIN_HIGURASHI_MUSIC_FONT")).unwrap(),
        ))
        .unwrap(),
    );
    let mut count = 0;
    for (i, (p, n)) in manifest.profiles.iter().zip(&originals).enumerate() {
        assert_eq!(i, p.index);
        let ctx = format!("Music profile{i} {}", p.name);
        let mut m = Model::new(
            &scenario.music_box_info,
            &model_tests::unlocks(p.unlock),
            p.append != 0,
        )
        .unwrap();
        m.scroll.fast_forward_to(p.scroll);
        m.selected = p.selected;
        m.playing = p.playing as i32;
        m.elapsed = p.elapsed;
        m.close_flash.fast_forward_to(p.close);
        for &(index, value) in &p.flashes {
            m.rows[index].flash.fast_forward_to(value);
        }
        m.scrollbar = Scrollbar {
            position: p.scroll as i32,
            visible: 818,
            first: 0,
            end: m.document_height,
        };
        let (actual, layout) = expected(&m, p, font.clone());
        assert_eq!(n.metadata.len(), 14);
        assert_eq!(
            n.metadata[..2],
            [u32::from(p.phase != 0), m.document_height as u32],
            "{ctx} resources/document"
        );
        assert_eq!(
            n.metadata[2..4],
            layout.size.to_array().map(f32::to_bits),
            "{ctx} native measured font40 label"
        );
        assert_eq!(
            n.metadata[4] as usize,
            layout.glyphs.len(),
            "{ctx} retained glyphs"
        );
        assert_eq!(
            n.metadata[6..],
            [1, 818f32.to_bits(), 0, 0, p.scroll as u32, 818, 0, 7268],
            "{ctx} native scrollbar storage"
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
                1,
                1
            ]
        );
        assert_eq!(
            n.retired,
            [
                i as u32,
                layout.glyphs.len() as u32,
                u32::from(p.phase > 0),
                p.capture,
                1
            ]
        );
        count += actual.len();
    }
    assert_eq!(count, 24029);
    println!(
        "Music:305 actual profiles/24029 exact raw parent+font+scrollbar+First packets; no pixels/GPU claim"
    );
}
