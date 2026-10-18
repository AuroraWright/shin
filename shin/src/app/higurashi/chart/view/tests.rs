//! Independent actual48de8/SimpleText packets and literal native shader replay.
use super::super::{catalog::Catalog, model::Model};
use super::*;
use glam::vec2;
use serde::Deserialize;
use shin_core::format::{
    font::read_font_metrics, rom::RomReader, scenario::higurashi::HigurashiScenario,
    text::read_sjis_string,
};
use shin_core::primitives::stateless_reader::StatelessFile;
use std::{
    collections::HashMap,
    io::{BufReader, Read},
    path::PathBuf,
};
use crate::app::higurashi::native_shader_replay::{self as replay, ShaderSet};
#[derive(Deserialize)]
struct Manifest {
    profiles: Vec<Profile>,
    pixel_profiles: Vec<usize>,
}
#[derive(Deserialize)]
struct Profile {
    index: usize,
    name: String,
    group: i32,
    mask: i32,
    route: Vec<u32>,
    unlocks: Vec<u32>,
    ready: bool,
    progress: u32,
    frame: u32,
    selected: i32,
    tooltip: i32,
    x: u32,
    y: u32,
    scroll: u32,
    flashes: [u32; 3],
    layout: u32,
}
struct Native {
    program: u32,
    blend: u32,
    source: u32,
    texture: u32,
    count: u32,
    start: u32,
    matrix: [u32; 16],
    u1: Vec<u32>,
    u2: [u32; 4],
    vertices: Vec<u32>,
}
struct Original {
    state: Vec<u32>,
    draws: Vec<Native>,
    children: u32,
}
fn directory() -> PathBuf {
    std::env::var_os("SHIN_HIGURASHI_CHART_RENDER_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../test-fixtures/higurashi/chart-render-native-corpus")
        })
}
fn words(line: &str) -> Vec<u32> {
    line.split_whitespace()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect()
}
fn corpus() -> (Manifest, Vec<Original>, HashMap<(u32, u32), String>) {
    let dir = directory();
    let manifest =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut originals: Vec<Original> = Vec::new();
    let mut names = HashMap::new();
    for line in std::fs::read_to_string(dir.join("output.txt"))
        .unwrap()
        .lines()
    {
        match line.split_whitespace().next().unwrap() {
            "R" => assert_eq!(words(line)[0] as usize, originals.len()),
            "M" => originals.push(Original {
                state: words(line),
                draws: Vec::new(),
                children: 0,
            }),
            "P" => {
                let w = words(line);
                let u1len = if w[0] == 4 { 16 } else { 4 };
                let end = 23 + u1len;
                assert_eq!(w.len(), end + 4 + w[6] as usize);
                originals.last_mut().unwrap().draws.push(Native {
                    program: w[0],
                    blend: w[1],
                    source: w[2],
                    texture: w[3],
                    count: w[4],
                    start: w[5],
                    matrix: w[7..23].try_into().unwrap(),
                    u1: w[23..end].to_vec(),
                    u2: w[end..end + 4].try_into().unwrap(),
                    vertices: w[end + 4..].to_vec(),
                });
            }
            "E" => {
                let w = words(line);
                let o = originals.last_mut().unwrap();
                assert_eq!(w[0] as usize, o.draws.len());
                o.children = w[1];
            }
            "T" => {
                let mut w = line.split_whitespace().skip(1);
                names.insert(
                    (
                        w.next().unwrap().parse().unwrap(),
                        w.next().unwrap().parse().unwrap(),
                    ),
                    w.next().unwrap().to_owned(),
                );
            }
            _ => panic!("Unknown Chart renderer protocol"),
        }
    }
    (manifest, originals, names)
}
fn catalog() -> Arc<Catalog> {
    let dir = std::env::var_os("SHIN_HIGURASHI_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../higurashi"));
    let mut rom = RomReader::new(
        StatelessFile::new(std::fs::File::open(dir.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let s = HigurashiScenario::new(bytes.into()).unwrap();
    Arc::new(Catalog::from_scenario(&s).unwrap())
}
fn model(p: &Profile, n: &Original, cat: Arc<Catalog>) -> (Model, Scrollbar, [glam::Vec2; 2]) {
    let mut m = Model::new(cat, p.group, p.mask, &p.unlocks, &p.route).unwrap();
    m.progress = f32::from_bits(p.progress);
    m.frame = p.frame;
    let w = &n.state;
    assert_eq!(w[0], m.initial_group as u32);
    assert_eq!(w[1], p.progress);
    assert_eq!(w[3], m.current_group as u32);
    assert_eq!(w[5], u32::from(m.title));
    assert_eq!(w[6], p.frame);
    assert_eq!(w[9] as usize, m.nodes.len());
    let mut at = 10;
    for node in &m.nodes {
        assert_eq!(
            &w[at..at + 7],
            &[
                node.group as u32,
                node.index as u32,
                node.x as u32,
                node.y as u32,
                node.source_id,
                u32::from(node.current),
                node.record as u32
            ]
        );
        at += 7;
    }
    let group = m.current_group as usize;
    if p.selected >= 0 {
        m.groups[group].selected = p.selected;
    }
    for (i, g) in m.groups.iter_mut().enumerate() {
        assert_eq!(
            &w[at..at + 6],
            &[
                g.min_x as u32,
                g.max_x as u32,
                g.min_y as u32,
                g.max_y as u32,
                g.origin_x as u32,
                g.origin_y as u32
            ]
        );
        assert_eq!(w[at + 6], g.selected as u32);
        let values = if i == group {
            [p.x, p.y, p.scroll]
        } else {
            [
                g.x.value().to_bits(),
                g.y.value().to_bits(),
                g.scroll.value().to_bits(),
            ]
        };
        assert_eq!(&w[at + 7..at + 10], &values);
        g.x.fast_forward_to(f32::from_bits(values[0]));
        g.y.fast_forward_to(f32::from_bits(values[1]));
        g.scroll.fast_forward_to(f32::from_bits(values[2]));
        at += 10;
    }
    for (i, t) in m.flashes.iter_mut().enumerate() {
        assert_eq!(w[at + i], p.flashes[i]);
        t.fast_forward_to(f32::from_bits(p.flashes[i]));
    }
    at += 3;
    let bar = Scrollbar {
        position: w[at + 4] as i32,
        visible: w[at + 5] as i32,
        first: w[at + 6] as i32,
        end: w[at + 7] as i32,
    };
    assert_eq!(w[at], 1);
    assert_eq!(w[at + 1], 819f32.to_bits());
    at += 8;
    let sizes = [
        vec2(f32::from_bits(w[at]), f32::from_bits(w[at + 1])),
        vec2(f32::from_bits(w[at + 2]), f32::from_bits(w[at + 3])),
    ];
    assert_eq!(at + 4, w.len());
    m.tooltip = w[7] as i32;
    if m.tooltip >= 0 {
        let r = &m.catalog.records[m.nodes[m.tooltip as usize].record];
        m.tooltip_text = read_sjis_string(
            &mut std::io::Cursor::new(r.label.as_ref().unwrap().c_string_bytes()),
            None,
        )
        .unwrap();
    }
    assert_eq!(
        m.tooltip,
        if p.tooltip == -2 {
            m.groups[group].selected
        } else {
            p.tooltip
        }
    );
    (m, bar, sizes)
}
struct Expected {
    program: u32,
    source: u32,
    texture: String,
    point: u32,
    matrix: [u32; 16],
    vertices: Vec<u32>,
    u1: Vec<u32>,
    u2: [u32; 4],
    count: u32,
    start: u32,
}
fn expected(
    m: &Model,
    p: &Profile,
    bar: Scrollbar,
    font: impl FontMetrics + Clone,
) -> Vec<Expected> {
    let layouts = [
        completion_layout(font.clone(), &m.completion_text).unwrap(),
        label_layout(font, &m.tooltip_text).unwrap(),
    ];
    let sizes = std::array::from_fn(|i| layouts[i].size);
    let mut out = Vec::new();
    let mut scroll = 0;
    for draw in packets(m, p.ready, bar, sizes, p.layout != 0) {
        match draw {
            Draw::Quad(q) => {
                let [l, t, r, b] = q.rect;
                let [u, v, s, w] = q.uv;
                let stride = if q.texture.is_empty() { 4 } else { 6 };
                let mut vertices = Vec::new();
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
                    if stride == 6 {
                        vertices.extend(
                            [
                                if i & 1 == 0 { u } else { s },
                                if i & 2 == 0 { v } else { w },
                            ]
                            .map(f32::to_bits),
                        );
                    }
                }
                out.push(Expected {
                    program: if stride == 4 {
                        1
                    } else if q.flash > 0. {
                        5
                    } else {
                        2
                    },
                    source: if stride == 4 { 3 } else { 0 },
                    texture: q.texture,
                    point: 0,
                    matrix: q.matrix.to_cols_array().map(f32::to_bits),
                    vertices,
                    u1: vec![
                        1f32.to_bits(),
                        1f32.to_bits(),
                        1f32.to_bits(),
                        q.flash.to_bits(),
                    ],
                    u2: [0; 4],
                    count: 4,
                    start: 0,
                });
            }
            Draw::Scrollbar { matrix, vertices } => {
                let mut words = Vec::new();
                for v in vertices {
                    let pos = v.position;
                    let tex = v.texture_position;
                    let c = v.color;
                    words.extend(pos.to_array().map(f32::to_bits));
                    words.push(c.0);
                    words.extend(tex.to_array().map(f32::to_bits));
                }
                out.push(Expected {
                    program: 2,
                    source: 1,
                    texture: "scroll".to_owned(),
                    point: 0,
                    matrix: matrix.to_cols_array().map(f32::to_bits),
                    vertices: words,
                    u1: vec![],
                    u2: [0; 4],
                    count: 8,
                    start: scroll * 8,
                });
                scroll += 1;
            }
            Draw::Text { index, matrix } => {
                for border in [true, false] {
                    if border && index != 0 {
                        continue;
                    }
                    for (i, g) in layouts[index].glyphs.iter().enumerate() {
                        let mut words = Vec::new();
                        for v in g.vertices {
                            let pos = v.position;
                            words.extend(pos.to_array().map(f32::to_bits));
                            words.push(v.color.to_bits());
                        }
                        let u1 = if border {
                            g.border
                                .iter()
                                .flat_map(|v| v.to_array().map(f32::to_bits))
                                .collect()
                        } else {
                            vec![1f32.to_bits(); 4]
                        };
                        out.push(Expected {
                            program: if border { 4 } else { 3 },
                            source: 2,
                            texture: String::new(),
                            point: g.point as u32,
                            matrix: matrix.to_cols_array().map(f32::to_bits),
                            vertices: words,
                            u1,
                            u2: if border {
                                [0, 0, 0, (240. * f32::from_bits(0x3b808081)).to_bits()]
                            } else {
                                [1f32.to_bits(); 4]
                            },
                            count: 4,
                            start: i as u32 * 4,
                        });
                    }
                }
            }
        }
    }
    out
}
fn compare(
    i: usize,
    p: &Profile,
    n: &Original,
    m: &Model,
    bar: Scrollbar,
    sizes: [glam::Vec2; 2],
    names: &HashMap<(u32, u32), String>,
    font: impl FontMetrics + Clone,
) -> usize {
    let layouts = [
        completion_layout(font.clone(), &m.completion_text).unwrap(),
        label_layout(font.clone(), &m.tooltip_text).unwrap(),
    ];
    for k in 0..2 {
        assert_eq!(
            layouts[k].size.to_array().map(f32::to_bits),
            sizes[k].to_array().map(f32::to_bits),
            "profile{i} label{k} measured size"
        );
    }
    let actual = expected(m, p, bar, font);
    assert_eq!(
        actual.len(),
        n.draws.len(),
        "profile{i} {} draw count",
        p.name
    );
    assert_eq!(n.children, 1);
    for (j, (a, b)) in actual.iter().zip(&n.draws).enumerate() {
        assert_eq!(
            [a.program, 2, a.source, a.count, a.start],
            [b.program, b.blend, b.source, b.count, b.start],
            "profile{i} draw{j} header"
        );
        if a.source < 2 {
            assert_eq!(
                a.texture,
                names[&(b.source, b.texture)],
                "profile{i} draw{j} name"
            );
        }
        if a.source == 2 {
            assert_eq!(a.point, b.texture, "profile{i} draw{j} glyph");
        }
        assert_eq!(a.matrix, b.matrix, "profile{i} draw{j} matrix");
        assert_eq!(a.vertices, b.vertices, "profile{i} draw{j} vertices");
        if matches!(a.program, 3 | 4 | 5) {
            assert_eq!(a.u1, b.u1, "profile{i} draw{j} uniform1");
        }
        if a.program == 3 || a.program == 4 {
            assert_eq!(a.u2, b.u2, "profile{i} draw{j} uniform2");
        }
    }
    actual.len()
}
#[test]
#[ignore = "102 original4c200/4687c/48de8/font/scrollbar profiles; SHIN_HIGURASHI_FONT actualNewRodin required; noGPU/audio"]
fn higurashi_chart_parent_packets_match_actual_native_renderer() {
    let (manifest, originals, names) = corpus();
    assert_eq!(originals.len(), 102);
    let cat = catalog();
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
        ))
        .unwrap(),
    );
    let mut count = 0;
    for (i, (p, n)) in manifest.profiles.iter().zip(&originals).enumerate() {
        assert_eq!(i, p.index);
        let (m, bar, sizes) = model(p, n, cat.clone());
        count += compare(i, p, n, &m, bar, sizes, &names, font.clone());
    }
    assert_eq!(count, 13679);
    println!("Chart:102 profiles/13,679 exact actual native parent/font/scrollbar packets");
}
fn clear(ctx: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    let mut pass = ctx.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(ctx.depth_stencil),
        "Chart reference nonblack background",
    );
    pass.clear(
        Some(shin_core::primitives::color::UnormColor::from_rgba(
            31, 63, 127, 193,
        )),
        Some(0),
        Some(1.),
    );
}
#[test]
#[ignore = "66 real chart/systex/NewRodin720p/1080p frames against original literal programs1/2/3/4/5; SHIN_HIGURASHI_CHART_SHADERS required; noaudio/window/saves"]
fn higurashi_chart_real_assets_pixels_match_literal_native_shaders() {
    let (manifest, originals, names) = corpus();
    let cat = catalog();
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/chart.txa").unwrap();
    let system: Arc<CharacterTextures> = server.load_sync("/systex.txa").unwrap();
    for (&(s, _), n) in &names {
        if s == 0 {
            atlas.get(n);
        } else {
            system.get(n);
        }
    }
    let points = originals
        .iter()
        .flat_map(|o| &o.draws)
        .filter(|n| n.source == 2)
        .map(|n| n.texture)
        .collect::<Vec<_>>();
    let glyphs = points
        .iter()
        .copied()
        .zip(
            font.clone().load_glyphs(
                device.clone(),
                queue.clone(),
                &points
                    .iter()
                    .map(|&v| char::from_u32(v).unwrap())
                    .collect::<Vec<_>>(),
            ),
        )
        .collect::<HashMap<_, _>>();
    let mut view = View::new(font.clone());
    let mut renderer = replay::Renderer::new(&device, ShaderSet::Chart);
    let mut frames = 0;
    for &i in &manifest.pixel_profiles {
        let p = &manifest.profiles[i];
        let n = &originals[i];
        let (m, bar, sizes) = model(p, n, cat.clone());
        compare(i, p, n, &m, bar, sizes, &names, font.clone());
        view.synchronize(&server, &m).unwrap();
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    let mut pass=ctx.begin_pass(shin_render::shaders::types::texture::TextureTarget{view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(ctx.depth_stencil),"Chart retained production parent");
                    view.render(
                        &m,
                        bar,
                        if p.ready { Some(&atlas) } else { None },
                        &system,
                        p.layout != 0,
                        &mut pass,
                    )
                    .unwrap();
                },
            );
            let original = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    for d in &n.draws {
                        let source = match d.source {
                            0 => Some(atlas.get(&names[&(0, d.texture)]).as_source()),
                            1 => Some(system.get(&names[&(1, d.texture)]).as_source()),
                            2 => Some(glyphs[&d.texture].as_texture_source().unwrap()),
                            3 => None,
                            _ => unreachable!(),
                        };
                        let uniforms = match d.program {
                            1 | 2 => vec![d.matrix.to_vec()],
                            3 | 4 => vec![d.matrix.to_vec(), d.u1.clone(), d.u2.to_vec()],
                            5 => vec![d.matrix.to_vec(), d.u1.clone()],
                            _ => unreachable!(),
                        };
                        let draw = replay::Draw {
                            program: d.program,
                            blend: d.blend,
                            primitive: 1,
                            depth: [7, 0],
                            stencil: [7, 0, 0, 0, 255],
                            uniforms,
                            vertices: d.vertices.clone(),
                            count: d.count,
                            start: 0,
                        };
                        renderer.draw(ctx, target, &draw, source, None);
                    }
                },
            );
            assert_eq!(
                actual, original,
                "profile{i} {} {extent:?} literal native RGBA",
                p.name
            );
            if p.ready {
                assert!(
                    actual.chunks_exact(4).any(|v| v != [31, 63, 127, 193]),
                    "Chart visible original artwork"
                );
            }
            frames += 1;
        }
    }
    assert_eq!(frames, 66);
    println!(
        "Chart:66 exact full720p/1080p literal original1/2/3/4/5 frames; actualchart/systex/NewRodin; one retained View/text replacement; noaudio"
    );
}
