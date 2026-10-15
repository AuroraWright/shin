use super::*;
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Clone)]
struct Case {
    kind: i32,
    append: bool,
    profile: usize,
    params: [i32; 8],
}
fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for append in [false, true] {
        for profile in 0..4 {
            for argument in [i32::MIN, -1].into_iter().chain(0..43).chain([i32::MAX]) {
                let mut params = [0; 8];
                params[0] = argument;
                out.push(Case {
                    kind: 13,
                    append,
                    profile,
                    params,
                });
            }
            for count in [-5, 0, 1, 2, 3, 4, 5, 6, 7] {
                out.push(Case {
                    kind: 14,
                    append,
                    profile,
                    params: [count, 0, 1, 17, 33, 87, 104, 159],
                });
            }
            for ids in [[33, 33, 0, 159, 33, 0, 0], [-1, 0, 31, 32, 63, 127, 159]] {
                let mut params = [7; 8];
                params[1..].copy_from_slice(&ids);
                out.push(Case {
                    kind: 14,
                    append,
                    profile,
                    params,
                });
            }
        }
    }
    out
}
fn properties(profile: usize) -> HigurashiProperties {
    let mut p = crate::layer::higurashi_properties::INITIAL_VALUES;
    p[5] = 1;
    match profile {
        0 => {}
        1 => {
            p[22] = 1;
            p[9] = 375;
            p[6] = 123;
            p[7] = 456;
            p[8] = 789;
            p[20] = 2;
            p[21] = 3;
        }
        2 => {
            p[0] = 75;
            p[1] = -25;
            p[2] = 1200;
            p[12] = 850;
            p[13] = 1100;
            p[14] = 128;
            p[15] = 64;
            p[16] = 125;
            p[9] = 625;
            p[6] = 400;
            p[7] = 700;
            p[8] = 900;
        }
        3 => {
            p[22] = 1;
            p[0] = -40;
            p[1] = 17;
            p[12] = 600;
            p[13] = 1200;
            p[16] = -250;
            p[9] = 0;
            p[6] = 0;
            p[7] = 0;
            p[8] = 0;
        }
        _ => unreachable!(),
    }
    let mut properties =
        HigurashiProperties::from_seeds([0xabc00001, 0xabc00002, 0xabc00003, 0xabc00004]);
    for (i, p) in p.into_iter().enumerate() {
        properties.tweener_mut(i).unwrap().fast_forward_to(p as f32);
    }
    properties
}
#[derive(Clone, Debug)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    stencil: [u32; 2],
    uniforms: [[u32; 16]; 3],
    vertices: Vec<u32>,
}
#[derive(Debug)]
struct Native {
    kind: i32,
    clocks: Vec<u32>,
    rows: Vec<(i32, u32)>,
    draws: Vec<Draw>,
    textures: BTreeMap<u32, String>,
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn native(cases: &[Case]) -> Vec<Native> {
    let mut input = String::new();
    for c in cases {
        write!(input, "N {} {} {}", c.kind, u8::from(c.append), c.profile).unwrap();
        for p in c.params {
            write!(input, " {p}").unwrap();
        }
        input.push('\n');
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_NOTIFICATION").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_NOTIFICATION_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), input).unwrap();
        std::fs::write(dir.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("N"));
            let kind = word(&mut w) as i32;
            assert_eq!(word(&mut w), 4, "Four fresh base OS samples");
            assert_eq!(word(&mut w), 0, "No global MT seed");
            assert_eq!(word(&mut w), 0, "Constructor phase preserved");
            assert_eq!(w.next(), Some("C"));
            let clocks = (0..if kind == 13 { 3 } else { 2 })
                .map(|_| word(&mut w))
                .collect();
            assert_eq!(w.next(), Some("I"));
            let rows = (0..word(&mut w))
                .map(|_| (word(&mut w) as i32, word(&mut w)))
                .collect();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            loop {
                match w.next().unwrap() {
                    "P" => {
                        let program = word(&mut w);
                        draws.push(Draw {
                            program,
                            blend: word(&mut w),
                            texture: word(&mut w),
                            stencil: words(&mut w),
                            uniforms: std::array::from_fn(|_| words(&mut w)),
                            vertices: (0..if program == 1 { 16 } else { 24 })
                                .map(|_| word(&mut w))
                                .collect(),
                        });
                    }
                    "E" => break,
                    x => panic!("Notification packet{x}"),
                }
            }
            assert_eq!(word(&mut w) as usize, draws.len());
            assert_eq!(w.next(), Some("K"));
            let textures = (0..word(&mut w))
                .map(|_| (word(&mut w), w.next().unwrap().to_owned()))
                .collect();
            assert!(w.next().is_none());
            Native {
                kind,
                clocks,
                rows,
                draws,
                textures,
            }
        })
        .collect()
}
fn rgba(v: u32) -> u32 {
    (v & 0xff00ff00) | ((v & 255) << 16) | ((v >> 16) & 255)
}
fn check(index: usize, c: &Case, n: &Native) -> usize {
    assert_eq!(c.kind, n.kind);
    if c.kind == 13 {
        assert_eq!(n.clocks, [-120f32, 0., 1.].map(f32::to_bits));
        assert!(n.rows.is_empty());
    } else {
        assert_eq!(n.clocks, [1f32, 100.].map(f32::to_bits));
        let count = c.params[0].max(0) as usize;
        assert_eq!(
            n.rows,
            c.params[1..1 + count]
                .iter()
                .map(|&id| (id, 0f32.to_bits()))
                .collect::<Vec<_>>()
        );
    }
    let content = Content::from_params(c.kind, &c.params, c.append).unwrap();
    let packets = [PassKind::Opaque, PassKind::Transparent]
        .into_iter()
        .flat_map(|p| content.packets(p))
        .collect::<Vec<_>>();
    assert_eq!(packets.len(), n.draws.len(), "case{index} packet count");
    let matrix = View::matrix(&properties(c.profile), &HigurashiTransform::default())
        .to_cols_array()
        .map(f32::to_bits);
    for (draw, (p, d)) in packets.iter().zip(&n.draws).enumerate() {
        assert_eq!(d.program, if p.texture.is_none() { 1 } else { 2 });
        assert_eq!(d.blend, if p.opaque { 1 } else { 2 });
        assert_eq!(d.stencil, [7, 1]);
        assert_eq!(
            d.uniforms[0], matrix,
            "case{index} draw{draw} native transform"
        );
        assert_eq!(d.uniforms[1..], [[0; 16]; 2]);
        assert_eq!(
            n.textures.get(&d.texture),
            p.texture.as_ref(),
            "case{index} draw{draw} native artwork"
        );
        let vertices: Vec<u32> = if p.texture.is_none() {
            bytemuck::cast_slice(&p.fills()).to_vec()
        } else {
            bytemuck::cast_slice(&p.sprites()).to_vec()
        };
        let mut original = d.vertices.clone();
        for i in (3..original.len()).step_by(if d.program == 1 { 4 } else { 6 }) {
            original[i] = rgba(original[i]);
        }
        assert_eq!(
            vertices, original,
            "case{index} draw{draw} native vertices/UV/color"
        );
    }
    packets.len()
}
#[test]
#[ignore = "actual13/14 updated factories/freshclone/literalrefresh/renderer, including signed parameters/color/alpha/transforms; CPU only,no audio/windows/saves"]
fn higurashi_saved_notification_factories_refresh_and_packets_match_native() {
    let cases = cases();
    assert_eq!(cases.len(), 456);
    let original = native(&cases);
    assert_eq!(original.len(), cases.len());
    let packets = cases
        .iter()
        .zip(&original)
        .enumerate()
        .map(|(i, (c, n))| check(i, c, n))
        .sum::<usize>();
    eprintln!(
        "Saved notifications:{} original13/14 owners/{packets} exact packets;4OS/noMT/no settings writes/no SimpleText/literal-return refresh;all artwork branches,signed/duplicate/0..7 rows and unsigned count7 geometry,alpha/tint/shader invariance;checked atlas/GPU services,no audio/windows/saves",
        cases.len()
    );
}
fn replay(pass: &mut RenderPass, native: &Native, atlas: &CharacterTextures) {
    for d in &native.draws {
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.uniforms[0].map(f32::from_bits),
        ));
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(d.stencil[0] as u8, true, false)
            .color_blend_type(if d.blend == 1 {
                ColorBlendType::Opaque
            } else {
                ColorBlendType::Layer1
            });
        if d.program == 1 {
            let vertices: [PosColVertex; 4] = std::array::from_fn(|i| {
                let v = &d.vertices[i * 4..];
                PosColVertex {
                    position: vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(rgba(v[3])),
                }
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices: [PosColTexVertex; 4] = std::array::from_fn(|i| {
                let v = &d.vertices[i * 6..];
                PosColTexVertex {
                    position: vec3(
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ),
                    color: UnormColor(rgba(v[3])),
                    texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                }
            });
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: atlas.get(&native.textures[&d.texture]).as_source(),
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
#[test]
#[ignore = "real otsuget/kakeraget atlases and original refreshed packet replay at720p/1080p over colored clear; pure offscreen GPU,no audio/windows/saves"]
fn higurashi_saved_notification_real_assets_pixels_match_native_overlay_semantics() {
    let (device, queue, server) = reference_gpu_assets();
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut cases = Vec::new();
    for (argument, append, profile) in [
        (0, false, 0),
        (6, true, 1),
        (7, false, 2),
        (27, false, 3),
        (27, true, 1),
        (31, true, 2),
        (42, true, 0),
    ] {
        let mut params = [0; 8];
        params[0] = argument;
        cases.push(Case {
            kind: 13,
            append,
            profile,
            params,
        });
    }
    for (count, profile) in [(0, 0), (1, 1), (3, 2), (6, 3), (7, 2)] {
        cases.push(Case {
            kind: 14,
            append: true,
            profile,
            params: [count, 0, 1, 17, 33, 87, 104, 159],
        });
    }
    let original = native(&cases);
    let clear = UnormColor::from_rgba(17, 39, 117, 255);
    let mut frames = 0;
    for (i, (c, n)) in cases.iter().zip(&original).enumerate() {
        check(i, c, n);
        let prepared = shin_tasks::block_on(Prepared::load_with_append(
            &server, &scenario, c.kind, &c.params, c.append,
        ))
        .unwrap();
        let view = prepared.clone_refresh().unwrap();
        let properties = properties(c.profile);
        assert_eq!(
            view.content, prepared.content,
            "Refresh retains constructor metadata/clocks"
        );
        for size in [(1280, 720), (1920, 1080)] {
            let actual = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: target,
                    },
                    Some(ctx.depth_stencil),
                    "Refreshed saved notification over colored clear",
                );
                pass.clear(Some(clear), Some(0), Some(1.));
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    view.render(
                        &mut pass,
                        &properties,
                        &HigurashiTransform::default(),
                        7,
                        kind,
                    )
                    .unwrap();
                }
            });
            let expected = reference_render_to_texture(&device, &queue, size, |ctx, target| {
                let mut pass = ctx.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: target,
                    },
                    Some(ctx.depth_stencil),
                    "Original notification packets over colored clear",
                );
                pass.clear(Some(clear), Some(0), Some(1.));
                replay(&mut pass, n, &prepared.atlas);
            });
            assert_eq!(
                actual, expected,
                "case{i} native saved notification pixels{size:?}"
            );
            assert!(
                actual.chunks_exact(4).any(|p| p[..3] == [0, 0, 0]),
                "Native overlay actually replaces the colored clear"
            );
            if c.profile <= 1 {
                assert!(
                    actual.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]),
                    "Constructor overlay remains black despite nondefault propertyalpha/tint/shader"
                );
            } else {
                assert!(
                    actual.chunks_exact(4).any(|p| p[..3] == [17, 39, 117]),
                    "Partial transform exposes the independent clear outside the source overlay"
                );
            }
            frames += 1;
        }
    }
    assert_eq!(frames, 24);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    eprintln!(
        "Saved notifications:{frames} byte-identical real otsuget/kakeraget720p/1080p frames over colored clear;default andalpha0/tint/shader/partial transforms preserve the native constructor overlay; literal-return refresh/no live opening/cues;no audio/windows/saves"
    );
}
