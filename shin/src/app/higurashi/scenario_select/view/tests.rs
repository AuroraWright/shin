use super::super::{
    Model,
    model_tests::{full_data, settings},
};
use super::*;
use std::collections::HashMap;
// Shared global programs1/2/5 are exact primary matches. Only the independent
// literal GLSL replay is reused, never Config's production plans.
#[path = "../../config/view_tests/gpu.rs"]
mod gpu;
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/scenario-select-render-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/scenario-select-render-capture/output.txt"
));
struct Frame {
    state: ViewState,
    swapped: bool,
    original: Vec<Vec<u32>>,
}
fn corpus() -> (Vec<Frame>, HashMap<u32, String>) {
    let mut headers = Vec::new();
    let mut originals: Vec<Vec<Vec<u32>>> = vec![];
    let mut names = HashMap::new();
    for line in OUTPUT.lines() {
        let mut w = line.split_whitespace();
        match w.next().unwrap() {
            "D" => {
                let h = w.map(|x| x.parse::<u32>().unwrap()).collect::<Vec<_>>();
                assert_eq!(h[0] as usize, originals.len());
                headers.push(h);
                originals.push(vec![]);
            }
            "P" => originals
                .last_mut()
                .unwrap()
                .push(w.map(|x| x.parse().unwrap()).collect()),
            "E" => {
                assert_eq!(
                    w.next().unwrap().parse::<usize>().unwrap(),
                    originals.last().unwrap().len()
                );
                assert_eq!(w.next(), Some("1"));
            }
            "T" => {
                names.insert(
                    w.next().unwrap().parse().unwrap(),
                    w.next().unwrap().to_owned(),
                );
            }
            _ => panic!("Original renderer opcode"),
        }
    }
    let frames = INPUT
        .lines()
        .zip(headers)
        .zip(originals)
        .map(|((input, header), original)| {
            let mut w = input.split_whitespace();
            assert_eq!(w.next(), Some("D"));
            fn n(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
                w.next().unwrap().parse().unwrap()
            }
            fn bits(w: &mut std::str::SplitWhitespace<'_>) -> f32 {
                f32::from_bits(u32::from_str_radix(w.next().unwrap(), 16).unwrap())
            }
            let kind = n(&mut w);
            let selected = n(&mut w);
            let profile = n(&mut w);
            let append = n(&mut w) != 0;
            let ready = n(&mut w) != 0;
            let close = bits(&mut w);
            let blackout = bits(&mut w);
            let swapped = n(&mut w) != 0;
            let count = n(&mut w) as usize;
            let mut model = Model::new(
                kind,
                selected as i32,
                append,
                &full_data(0, 3),
                &settings(profile),
                Some(221),
                true,
            )
            .unwrap();
            assert_eq!(model.count, count);
            assert_eq!(model.mask, header[10]);
            model.ready = ready;
            model.close_flash.fast_forward_to(close);
            model.blackout.fast_forward_to(blackout);
            for i in 0..count {
                model.opacity[i].fast_forward_to(bits(&mut w));
                model.highlight[i].fast_forward_to(bits(&mut w));
                model.hot[i].fast_forward_to(bits(&mut w));
            }
            assert!(w.next().is_none());
            Frame {
                state: model.view_state(),
                swapped,
                original,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 252);
    (frames, names)
}
fn words(p: &Packet, names: &HashMap<u32, String>) -> Vec<u32> {
    let program = if p.texture.is_empty() {
        1
    } else if p.flash > 0. {
        5
    } else {
        2
    };
    let texture = if program == 1 {
        u32::MAX
    } else {
        *names
            .iter()
            .find(|(_, v)| v.as_str() == p.texture.as_str())
            .unwrap()
            .0
    };
    let mut w = vec![program, if p.opaque { 1 } else { 2 }, texture];
    w.extend(p.matrix.to_cols_array().map(f32::to_bits));
    w.extend(if program == 5 {
        [
            1f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            p.flash.to_bits(),
        ]
    } else {
        [0; 4]
    });
    w.push(if program == 1 { 16 } else { 24 });
    for (x, y, u, v) in [
        (p.rect[0], p.rect[1], p.uv[0], p.uv[1]),
        (p.rect[2], p.rect[1], p.uv[2], p.uv[1]),
        (p.rect[0], p.rect[3], p.uv[0], p.uv[3]),
        (p.rect[2], p.rect[3], p.uv[2], p.uv[3]),
    ] {
        w.extend([x.to_bits(), y.to_bits(), 0, p.color]);
        if program != 1 {
            w.extend([u.to_bits(), v.to_bits()]);
        }
    }
    w
}
#[test]
fn reference_updated_scenario_select_original252_frames6352_packets() {
    let (frames, names) = corpus();
    let mut count = 0;
    for (i, f) in frames.iter().enumerate() {
        let plan = packets(&f.state, f.swapped);
        assert_eq!(plan.len(), f.original.len(), "Original packetcount{i}");
        for (j, (p, n)) in plan.iter().zip(&f.original).enumerate() {
            let got = words(p, &names);
            assert_eq!(got[..3], n[..3], "ScenarioSelect program/blend/name{i}/{j}");
            assert_eq!(got[3..19], n[3..19], "ScenarioSelect rawprojection{i}/{j}");
            assert_eq!(
                got[19..],
                n[19..],
                "ScenarioSelect raw flash/BGRA/geometry/UV{i}/{j}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 6352);
}
#[test]
#[ignore = "requires the original native ScenarioSelect renderer, updated executable, font and scenario; CPU only"]
fn reference_updated_scenario_select_renderer_frozen_original_reexecution() {
    let exe = std::env::var("SHIN_HIGURASHI_SCENARIO_SELECT_RENDER_CAPTURE").unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let font = std::env::var("SHIN_HIGURASHI_REFERENCE_FONT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    let snr = std::env::var("SHIN_HIGURASHI_REFERENCE_SNR")
        .unwrap_or_else(|_| "/tmp/higurashi-scenario-patch.rom.snr".into());
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg(font)
        .arg(snr)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(INPUT.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, OUTPUT.as_bytes());
}
fn clear(ctx: &mut crate::render::PreRenderContext, target: &wgpu::TextureView) {
    let mut pass = ctx.begin_pass(
        shin_render::shaders::types::texture::TextureTarget {
            view: target,
            kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
        },
        Some(ctx.depth_stencil),
        "ScenarioSelect reference initialclear",
    );
    pass.clear(
        Some(UnormColor::from_rgba(31, 63, 127, 193)),
        Some(0),
        Some(1.),
    );
}
#[test]
#[ignore = "72 real ScenarioSelect720p/1080p parentframes against original packets/literal shaders; SHIN_HIGURASHI_CONFIG_SHADERS required; no audio"]
fn higurashi_scenario_select_real_assets_pixels_match_literal_native_shaders() {
    let (frames, names) = corpus();
    let (device, queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/snrsel.txa").unwrap();
    for name in names.values() {
        atlas.get(name);
    }
    let mut reference = gpu::Renderer::new(&device);
    let view = View::new();
    let mut indices = vec![];
    let mut base = 0;
    for count in [9, 6, 6, 9, 5, 10] {
        let length = 16 + count * 2 + 11;
        indices.extend([
            base,
            base + 15,
            base + 16,
            base + 16 + count * 2 - 1,
            base + length - 3,
            base + length - 1,
        ]);
        base += length;
    }
    assert_eq!(base, frames.len());
    let mut tested = 0;
    for index in indices {
        let f = &frames[index];
        for extent in [(1280, 720), (1920, 1080)] {
            let actual = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    let mut pass=ctx.begin_pass(shin_render::shaders::types::texture::TextureTarget {view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(ctx.depth_stencil),"ScenarioSelect production parent");
                    view.render(&f.state, Some(&atlas), f.swapped, &mut pass);
                },
            );
            let expected = crate::adv::higurashi::reference_render_to_texture(
                &device,
                &queue,
                extent,
                |ctx, target| {
                    clear(ctx, target);
                    for w in &f.original {
                        assert_eq!(w.len(), 24 + w[23] as usize);
                        let p = w[0];
                        let draw = gpu::Draw {
                            program: p,
                            blend: w[1],
                            primitive: 1,
                            depth: [7, 0],
                            stencil: [7, 0, 0, 0, 0xffff],
                            uniforms: if p == 5 {
                                vec![w[3..19].to_vec(), w[19..23].to_vec()]
                            } else {
                                vec![w[3..19].to_vec()]
                            },
                            vertices: w[24..].to_vec(),
                            count: 4,
                            start: 0,
                        };
                        let source = (p != 1).then(|| atlas.get(&names[&w[2]]).as_source());
                        reference.draw(ctx, target, &draw, source, None);
                    }
                },
            );
            let differing = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
            let max = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert_eq!(
                (differing, max),
                (0, 0),
                "ScenarioSelect parent{i}/{extent:?} literalnativeRGBA",
                i = index
            );
            tested += 1;
        }
    }
    assert_eq!(tested, 72);
}
