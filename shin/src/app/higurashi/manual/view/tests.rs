use super::*;
use std::collections::HashMap;
// Config and Manual select the same updated global programs1/2/5. Reuse
// only the independent literal GLSL renderer, never Config production plans.
use crate::app::higurashi::native_shader_replay::{self as replay, ShaderSet};
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/manual-render-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/manual-render-capture/output.txt"
));
fn raw_packet(p: &Packet, names: &HashMap<String, u32>) -> Vec<u32> {
    let clip = p.texture.is_empty();
    let program = if clip {
        1
    } else if p.flash > 0. {
        5
    } else {
        2
    };
    let mut words = vec![
        program,
        if clip { 0 } else { 2 },
        if clip { u32::MAX } else { names[&p.texture] },
    ];
    words.extend(p.stencil);
    words.extend(p.matrix.to_cols_array().map(f32::to_bits));
    words.extend(if program == 5 {
        [
            1f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            p.flash.to_bits(),
        ]
    } else {
        [0; 4]
    });
    words.push(if clip { 16 } else { 24 });
    for (x, y, u, v) in [
        (p.rect[0], p.rect[1], p.uv[0], p.uv[1]),
        (p.rect[2], p.rect[1], p.uv[2], p.uv[1]),
        (p.rect[0], p.rect[3], p.uv[0], p.uv[3]),
        (p.rect[2], p.rect[3], p.uv[2], p.uv[3]),
    ] {
        words.extend([x.to_bits(), y.to_bits(), 0, p.color]);
        if !clip {
            words.extend([u.to_bits(), v.to_bits()]);
        }
    }
    words
}
#[test]
fn reference_updated_manual_original128_frames1599_packets() {
    let names = OUTPUT
        .lines()
        .filter_map(|s| s.strip_prefix("T "))
        .map(|s| {
            let mut s = s.split_whitespace();
            let id = s.next().unwrap().parse::<u32>().unwrap();
            (s.next().unwrap().to_owned(), id)
        })
        .collect::<HashMap<_, _>>();
    let mut output = OUTPUT.lines();
    let mut count = 0;
    for (index, input) in INPUT.lines().enumerate() {
        let mut w = input.split_whitespace();
        assert_eq!(w.next(), Some("D"));
        let n = w.next().unwrap().parse().unwrap();
        let ready = w.next().unwrap() == "1";
        let progress = f32::from_bits(u32::from_str_radix(w.next().unwrap(), 16).unwrap());
        let scroll = f32::from_bits(u32::from_str_radix(w.next().unwrap(), 16).unwrap());
        let close_flash = f32::from_bits(u32::from_str_radix(w.next().unwrap(), 16).unwrap());
        let swapped = w.next().unwrap() == "1";
        let state = ViewState {
            ready,
            count: n,
            selected: 0,
            progress,
            scroll,
            close_flash,
        };
        assert_eq!(
            output
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            index
        );
        let plan = packets(&state, swapped);
        for (j, p) in plan.iter().enumerate() {
            let native = output
                .next()
                .unwrap()
                .strip_prefix("P ")
                .expect("Original Manual draw")
                .split_whitespace()
                .map(|w| w.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
            let got = raw_packet(p, &names);
            assert_eq!(
                got[..8],
                native[..8],
                "Manual frame{index}/packet{j} program/blend/texture/full stencil"
            );
            assert_eq!(
                got[8..24],
                native[8..24],
                "Manual frame{index}/packet{j} original raw matrix"
            );
            assert_eq!(
                got[24..],
                native[24..],
                "Manual frame{index}/packet{j} consumed flash/geometry/BGRA/UV"
            );
            count += 1;
        }
        let end = output
            .next()
            .unwrap()
            .strip_prefix("E ")
            .expect("Original Manual draw count")
            .split_whitespace()
            .map(|s| s.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(end, [plan.len(), 1, ready.into()]);
    }
    assert_eq!(count, 1599);
    assert_eq!(names.len(), 23);
    assert!(output.all(|s| s.starts_with("T ")));
}
#[test]
#[ignore = "requires SHIN_HIGURASHI_MANUAL_RENDER_CAPTURE and the updated Higurashi executable"]
fn reference_updated_manual_render_frozen_original_reexecution() {
    let Ok(exe) = std::env::var("SHIN_HIGURASHI_MANUAL_RENDER_CAPTURE") else {
        return;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let font = std::env::var("SHIN_HIGURASHI_REFERENCE_FONT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg(font)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(INPUT.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, OUTPUT.as_bytes());
}

fn original_frames() -> (
    Vec<ViewState>,
    Vec<bool>,
    Vec<Vec<Vec<u32>>>,
    HashMap<u32, String>,
) {
    let mut states = Vec::new();
    let mut layouts = Vec::new();
    for row in INPUT.lines() {
        let words = row.split_whitespace().collect::<Vec<_>>();
        let float = |i| f32::from_bits(u32::from_str_radix(words[i], 16).unwrap());
        states.push(ViewState {
            ready: words[2] == "1",
            count: words[1].parse().unwrap(),
            selected: 0,
            progress: float(3),
            scroll: float(4),
            close_flash: float(5),
        });
        layouts.push(words[6] == "1");
    }
    let mut frames = Vec::new();
    let mut names = HashMap::new();
    for row in OUTPUT.lines() {
        let mut words = row.split_whitespace();
        match words.next().unwrap() {
            "D" => {
                assert_eq!(
                    words.next().unwrap().parse::<usize>().unwrap(),
                    frames.len()
                );
                frames.push(Vec::new());
            }
            "P" => frames.last_mut().unwrap().push(
                words
                    .map(|word| word.parse().unwrap())
                    .collect::<Vec<u32>>(),
            ),
            "E" => {
                assert_eq!(
                    words.next().unwrap().parse::<usize>().unwrap(),
                    frames.last().unwrap().len()
                );
            }
            "T" => {
                names.insert(
                    words.next().unwrap().parse().unwrap(),
                    words.next().unwrap().to_owned(),
                );
            }
            value => panic!("Unknown Manual packet row {value}"),
        }
    }
    assert_eq!(frames.len(), 128);
    (states, layouts, frames, names)
}
fn clear(
    context: &mut crate::render::PreRenderContext,
    target: &wgpu::TextureView,
    color: bool,
    stencil: u8,
) {
    let mut pass = context.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(context.depth_stencil),
        "Manual independent initial/full-byte stencil clear",
    );
    pass.clear(
        color.then(|| UnormColor::from_rgba(31, 63, 127, 193)),
        Some(stencil),
        color.then_some(1.),
    );
}
#[test]
#[ignore = "52 real Manual720p/1080p frames against original packets/literal shaders/full-byte stencil; SHIN_HIGURASHI_CONFIG_SHADERS required; no audio"]
fn higurashi_manual_real_assets_pixels_match_literal_native_shaders() {
    let (states, layouts, originals, names) = original_frames();
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/manual.txa").unwrap();
    for name in names.values() {
        atlas.get(name);
    }
    let view = View::new();
    let mut renderer = replay::Renderer::new(&device, ShaderSet::Config);
    let indices = (0..15)
        .map(|i| 6 + i * 8)
        .chain([1, 2, 4, 5, 7, 120, 121, 122, 125, 126, 127])
        .collect::<Vec<_>>();
    let mut pages = std::collections::HashSet::new();
    let mut frames = 0;
    for index in indices {
        let state = &states[index];
        for words in &originals[index] {
            if let Some(name) = names.get(&words[2]) {
                if name.parse::<usize>().is_ok() {
                    pages.insert(name.clone());
                }
            }
        }
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    // A pre-existing high stencil bit detects accidental reuse of
                    // Umineko's masked general clip instead of the native full clear.
                    clear(ctx, target, true, 0x80);
                    let mut pass = ctx.begin_pass(shin_render::shaders::types::texture::TextureTarget {
                    view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },Some(ctx.depth_stencil),"Manual production parent");
                    view.render(state, Some(&atlas), layouts[index], &mut pass);
                },
            );
            let expected = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target, true, 0x80);
                    if state.ready {
                        clear(ctx, target, false, 0);
                    }
                    for words in &originals[index] {
                        let program = words[0];
                        assert_eq!(words.len(), 29 + words[28] as usize);
                        let draw = replay::Draw {
                            program,
                            blend: words[1],
                            primitive: 1,
                            depth: [7, 0],
                            stencil: words[3..8].try_into().unwrap(),
                            uniforms: if program == 5 {
                                vec![words[8..24].to_vec(), words[24..28].to_vec()]
                            } else {
                                vec![words[8..24].to_vec()]
                            },
                            vertices: words[29..].to_vec(),
                            count: 4,
                            start: 0,
                        };
                        let source =
                            (program != 1).then(|| atlas.get(&names[&words[2]]).as_source());
                        renderer.draw(ctx, target, &draw, source, None);
                    }
                },
            );
            assert_eq!(
                actual, expected,
                "Manual profile{index}/{extent:?} original full-frame RGBA"
            );
            if state.ready && state.progress > 0. {
                assert!(
                    actual.chunks_exact(4).any(|p| p != [31, 63, 127, 193]),
                    "Manual profile{index} visible during opening"
                );
            }
            frames += 1;
        }
    }
    assert_eq!(pages.len(), 18);
    assert_eq!(frames, 52);
    println!(
        "Manual parent:52 exact full720p/1080p frames,all18 real pages,literal updated1/2/5 shaders,full-byte stencil over pre-existing highbit,partial slide/scroll/alpha/flash/both key layouts/count1/2/no-atlas/count0;no audio"
    );
}
