//! Independent original parent packets and literal1/2/5 shader pixel replay.
//! Model/asset/First child ownership comparisons have separate fixtures.
use super::{
    Control, Heading, RowState, ViewState,
    view::{self, Draw, View},
};
use crate::adv::higurashi::browser_controls::Scrollbar;
use serde::Deserialize;
use shin_render::shaders::types::vertices::PosColTexVertex;
use std::{collections::HashMap, path::PathBuf};

mod gpu;
#[derive(Deserialize)]
struct Manifest {
    profiles: Vec<Profile>,
    pixel_profiles: Vec<usize>,
}
#[derive(Deserialize)]
struct Profile {
    index: usize,
    name: String,
    ready: bool,
    progress: u32,
    scroll: u32,
    frame: u32,
    close_flash: u32,
    reset_flash: u32,
    layout: bool,
}
struct Original {
    state: ViewState,
    draws: Vec<Native>,
    children: u32,
}
struct Native {
    program: u32,
    blend: u32,
    source: u32,
    texture: u32,
    count: u32,
    start: u32,
    matrix: [u32; 16],
    flash: [u32; 4],
    vertices: Vec<u32>,
}
fn directory() -> PathBuf {
    std::env::var_os("SHIN_HIGURASHI_CONFIG_RENDER_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../reverse-engineering/higurashi/config-render-native-corpus")
        })
}
fn words(line: &str) -> Vec<u32> {
    line.split_whitespace()
        .skip(1)
        .map(|w| w.parse().unwrap())
        .collect()
}
fn state(profile: &Profile, w: Vec<u32>) -> ViewState {
    let mut at = 0;
    let mut next = || {
        let v = w[at];
        at += 1;
        v
    };
    let count = next();
    let content_height = next() as i32;
    let mut rows = Vec::new();
    for _ in 0..count {
        let slider = next();
        let y = next() as i32;
        let title_band = next();
        let subtype = next();
        let value = next() as i32;
        let focused = next() != 0;
        let cursor = next() as i32;
        let count = next();
        let labels = (0..count).map(|_| next()).collect();
        rows.push(RowState {
            y,
            title_band,
            focused,
            kind: if slider != 0 {
                assert_eq!(count, 0);
                Control::Slider { subtype, value }
            } else {
                Control::Buttons {
                    labels,
                    value,
                    cursor,
                }
            },
        });
    }
    let count = next();
    let headings = (0..count)
        .map(|_| Heading {
            band: next(),
            y: next() as i32,
            bottom: next() as i32,
        })
        .collect();
    let count = next();
    let dividers = (0..count).map(|_| next() as i32).collect();
    assert_eq!(next(), 1);
    assert_eq!(next(), 770f32.to_bits());
    assert_eq!(next(), 0);
    assert_eq!(next(), 0);
    let scrollbar = Scrollbar {
        position: next() as i32,
        visible: next() as i32,
        first: next() as i32,
        end: next() as i32,
    };
    drop(next);
    assert_eq!(at, w.len());
    ViewState {
        ready: profile.ready,
        progress: f32::from_bits(profile.progress),
        scroll: f32::from_bits(profile.scroll),
        content_height,
        frame: profile.frame,
        close_flash: f32::from_bits(profile.close_flash),
        reset_flash: f32::from_bits(profile.reset_flash),
        scrollbar,
        rows,
        headings,
        dividers,
    }
}
fn corpus() -> (Manifest, Vec<Original>, HashMap<(u32, u32), String>) {
    let path = directory();
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let output = std::fs::read_to_string(path.join("output.txt")).unwrap();
    let mut originals = Vec::new();
    let mut names = HashMap::new();
    let mut current = None;
    for line in output.lines() {
        match line.split_whitespace().next().unwrap() {
            "R" => {
                let i: usize = line[2..].parse().unwrap();
                assert_eq!(i, originals.len());
                assert!(current.is_none());
                current = Some(i);
            }
            "M" => {
                let i = current.unwrap();
                originals.push(Original {
                    state: state(&manifest.profiles[i], words(line)),
                    draws: Vec::new(),
                    children: 0,
                });
            }
            "P" => {
                let w = words(line);
                let size = w[6] as usize;
                assert_eq!(w.len(), 27 + size);
                originals.last_mut().unwrap().draws.push(Native {
                    program: w[0],
                    blend: w[1],
                    source: w[2],
                    texture: w[3],
                    count: w[4],
                    start: w[5],
                    matrix: w[7..23].try_into().unwrap(),
                    flash: w[23..27].try_into().unwrap(),
                    vertices: w[27..].to_vec(),
                });
            }
            "E" => {
                let w = words(line);
                let o = originals.last_mut().unwrap();
                assert_eq!(w[0] as usize, o.draws.len());
                o.children = w[1];
                current = None;
            }
            "T" => {
                let mut w = line.split_whitespace().skip(1);
                let source = w.next().unwrap().parse().unwrap();
                let index = w.next().unwrap().parse().unwrap();
                names.insert((source, index), w.next().unwrap().to_owned());
                assert!(w.next().is_none());
            }
            value => panic!("Unknown Config native output {value}"),
        }
    }
    assert!(current.is_none());
    assert_eq!(originals.len(), manifest.profiles.len());
    (manifest, originals, names)
}
fn flatten(vertices: &[PosColTexVertex]) -> Vec<u32> {
    vertices
        .iter()
        .flat_map(|v| {
            let p = v.position;
            let uv = v.texture_position;
            let color = v.color;
            [
                p.x.to_bits(),
                p.y.to_bits(),
                p.z.to_bits(),
                color.0,
                uv.x.to_bits(),
                uv.y.to_bits(),
            ]
        })
        .collect()
}
fn compare(
    index: usize,
    profile: &Profile,
    original: &Original,
    names: &HashMap<(u32, u32), String>,
) -> usize {
    let actual = view::packets(&original.state, profile.layout);
    assert_eq!(
        actual.len(),
        original.draws.len(),
        "profile{index} {} count",
        profile.name
    );
    assert_eq!(
        original.children, 1,
        "native empty/mixed private scope draw remains checked boundary"
    );
    let mut scrollbar = 0;
    for (draw_index, (a, n)) in actual.iter().zip(&original.draws).enumerate() {
        let (program, source, texture, matrix, vertices, flash, count, start) = match a {
            Draw::Quad(q) => {
                let stride = if q.texture.is_empty() { 4 } else { 6 };
                let [l, t, r, b] = q.rect;
                let [u, v, s, w] = q.uv;
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
                (
                    if q.texture.is_empty() {
                        1
                    } else if q.flash > 0. {
                        5
                    } else {
                        2
                    },
                    if q.texture.is_empty() { 2 } else { 0 },
                    q.texture.as_str(),
                    q.matrix,
                    vertices,
                    if q.flash > 0. {
                        [
                            1f32.to_bits(),
                            1f32.to_bits(),
                            1f32.to_bits(),
                            q.flash.to_bits(),
                        ]
                    } else {
                        [0; 4]
                    },
                    4,
                    0,
                )
            }
            Draw::Scrollbar { matrix, vertices } => {
                let start = scrollbar * 8;
                scrollbar += 1;
                (2, 1, "scroll", *matrix, flatten(vertices), [0; 4], 8, start)
            }
        };
        assert_eq!(
            [program, 2, source, count, start],
            [n.program, n.blend, n.source, n.count, n.start],
            "profile{index} draw{draw_index} header"
        );
        if source != 2 {
            assert_eq!(
                texture,
                names[&(n.source, n.texture)],
                "profile{index} draw{draw_index} name"
            );
        }
        assert_eq!(
            matrix.to_cols_array().map(f32::to_bits),
            n.matrix,
            "profile{index} draw{draw_index} {texture} matrix"
        );
        assert_eq!(
            vertices, n.vertices,
            "profile{index} draw{draw_index} {texture} vertices"
        );
        assert_eq!(
            flash, n.flash,
            "profile{index} draw{draw_index} {texture} meaningful uniform"
        );
    }
    assert_eq!(scrollbar, if profile.ready { 2 } else { 0 });
    actual.len()
}
#[test]
#[ignore = "408 original constructor/schema/full50c58/row/c0574 packets, allflags/append/ready/selection/pulse/header flash/layout; noGPU/audio/windows/saves"]
fn higurashi_config_parent_packets_match_actual_native_renderer() {
    let (manifest, originals, names) = corpus();
    assert_eq!(originals.len(), 408);
    let mut draws = 0;
    for (i, (profile, original)) in manifest.profiles.iter().zip(&originals).enumerate() {
        assert_eq!(profile.index, i);
        draws += compare(i, profile, original, &names);
    }
    assert_eq!(draws, 38420);
    println!(
        "Config parent:408 profiles/38,420 exact original packets; flat older rows, all4flags/append branches, no-atlas child boundary, title/slider/button pulse/header flash/scrollbar/hints pass"
    );
}

fn clear(context: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    let mut pass = context.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(context.depth_stencil),
        "Config independent clear",
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
#[ignore = "24 real config/systex full720p/1080p frames against literal updated programs1/2/5 rawWGPU, noaudio/window/saves; SHIN_HIGURASHI_CONFIG_SHADERS required"]
fn higurashi_config_real_assets_pixels_match_literal_native_shaders() {
    let (manifest, originals, names) = corpus();
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let atlas: std::sync::Arc<crate::adv::character_view::CharacterTextures> =
        server.load_sync("/config.txa").unwrap();
    let system: std::sync::Arc<crate::adv::character_view::CharacterTextures> =
        server.load_sync("/systex.txa").unwrap();
    for (&(source, _), name) in &names {
        if source == 0 {
            atlas.get(name);
        } else {
            system.get(name);
        }
    }
    let view = View::new();
    let mut renderer = gpu::Renderer::new(&device);
    let mut frames = 0;
    for &index in &manifest.pixel_profiles {
        let profile = &manifest.profiles[index];
        let original = &originals[index];
        compare(index, profile, original, &names);
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    let mut pass=ctx.begin_pass(shin_render::shaders::types::texture::TextureTarget{view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(ctx.depth_stencil),"Config production parent");
                    view.render(
                        &original.state,
                        Some(&atlas),
                        &system,
                        profile.layout,
                        &mut pass,
                    )
                    .unwrap();
                },
            );
            let expected = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    for n in &original.draws {
                        let texture = match n.source {
                            0 => Some(atlas.get(&names[&(0, n.texture)]).as_source()),
                            1 => Some(system.get(&names[&(1, n.texture)]).as_source()),
                            2 => None,
                            _ => unreachable!(),
                        };
                        let draw = gpu::Draw {
                            program: n.program,
                            blend: n.blend,
                            primitive: 1,
                            depth: [7, 0],
                            stencil: [7, 0, 0, 0, 255],
                            uniforms: if n.program == 5 {
                                vec![n.matrix.to_vec(), n.flash.to_vec()]
                            } else {
                                vec![n.matrix.to_vec()]
                            },
                            vertices: n.vertices.clone(),
                            count: n.count,
                            start: 0,
                        };
                        renderer.draw(ctx, target, &draw, texture, None);
                    }
                },
            );
            assert_eq!(
                actual, expected,
                "profile{index} {} {extent:?} literal shader RGBA",
                profile.name
            );
            if profile.ready {
                assert!(
                    actual.chunks_exact(4).any(|p| p != [31, 63, 127, 193]),
                    "Config visible artwork"
                );
            }
            frames += 1;
        }
    }
    assert_eq!(frames, 24);
    println!(
        "Config parent:24 exact full720p/1080p real config/systex frames, literal older1/2/5 shaders, partial-alpha nonblack background, all4flags/append branches/partial opening/scrolled append and no-atlas pass; no audio"
    );
}
