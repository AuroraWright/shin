use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
    sync::Arc,
};

struct Draw {
    program: u32,
    texture: u32,
    count: usize,
    matrix: [u32; 16],
    extra: [u32; 16],
    vertices: Vec<u32>,
}
struct Packet {
    shell: Vec<Draw>,
    content_count: usize,
    stages: Vec<u32>,
}
#[derive(Clone, Copy)]
struct Case {
    state: State,
    ready: bool,
}
fn cases() -> Vec<Case> {
    let mut result = Vec::new();
    for slide in [0.0, 0.125, 0.5, 0.999, 1.0] {
        for flash in [0.0, 0.125, 0.7, 1.0] {
            for kind in 0..4 {
                for voices in [0, 1, 255] {
                    for navigation in [false, true] {
                        result.push(Case {
                            state: State {
                                slide,
                                close_flash: flash,
                                selected: RowMetadata {
                                    kind,
                                    voice_count: voices,
                                    top: 0.0,
                                    height: 112.0,
                                },
                                navigation,
                            },
                            ready: true,
                        });
                    }
                }
            }
        }
    }
    for slide in [0.0, 0.5, 1.0] {
        result.push(Case {
            state: State {
                slide,
                close_flash: 0.7,
                selected: RowMetadata {
                    kind: 0,
                    voice_count: 1,
                    top: 0.0,
                    height: 112.0,
                },
                navigation: true,
            },
            ready: false,
        });
    }
    result
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn native(cases: &[Case]) -> Vec<Packet> {
    let mut input = String::new();
    for case in cases {
        let s = case.state;
        writeln!(
            input,
            "{} {} {} {} {} {}",
            s.slide,
            s.close_flash,
            s.selected.kind,
            s.selected.voice_count,
            u8::from(s.navigation),
            u8::from(case.ready)
        )
        .unwrap();
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_SHELL").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_SHELL_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("native.txt", &output.stdout),
            ("native.log", &output.stderr),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
        }
    }
    let text = String::from_utf8(output.stdout).unwrap();
    text.lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("R"));
            let mut shell = Vec::new();
            let mut content_count = 0;
            let mut stages = Vec::new();
            while let Some(tag) = w.next() {
                match tag {
                    "B" => {
                        let stage = word(&mut w);
                        assert_eq!(stage, stages.len() as u32);
                        stages.push(stage);
                    }
                    "Q" => {
                        assert!(stages.len() < 2);
                        let program = word(&mut w);
                        let texture = word(&mut w);
                        let count = word(&mut w) as usize;
                        let matrix = std::array::from_fn(|_| word(&mut w));
                        let extra = std::array::from_fn(|_| word(&mut w));
                        let _: [u32; 4] = std::array::from_fn(|_| word(&mut w));
                        let vertices = (0..count * 6).map(|_| word(&mut w)).collect();
                        if stages.is_empty() {
                            content_count += 1;
                        } else {
                            shell.push(Draw {
                                program,
                                texture,
                                count,
                                matrix,
                                extra,
                                vertices,
                            });
                        }
                    }
                    "E" => {
                        assert_eq!(word(&mut w) as usize, content_count + shell.len());
                        assert!(w.next().is_none());
                        break;
                    }
                    _ => panic!("shell token"),
                }
            }
            Packet {
                shell,
                content_count,
                stages,
            }
        })
        .collect()
}
fn check(cases: &[Case], packets: &[Packet]) -> (usize, usize) {
    assert_eq!(cases.len(), packets.len());
    let mut words = 0;
    let mut draws = 0;
    for (case, packet) in cases.iter().zip(packets) {
        assert_eq!(packet.stages, [0, 1]);
        if !case.ready {
            assert!(packet.shell.is_empty() && packet.content_count == 0);
            continue;
        }
        assert_eq!(
            packet.content_count,
            9 + 6
                + 2
                + usize::from(
                    case.state.selected.kind == 0 && case.state.selected.voice_count != 0
                )
        );
        let expected = parts(case.state);
        assert_eq!(packet.shell.len(), expected.len());
        for (draw, part) in packet.shell.iter().zip(expected) {
            let texture = match part.name {
                "obi" => 15,
                "caption" => 16,
                "close" => 17,
                "help_logview" => 18,
                "key" => 19,
                _ => panic!("shell artwork"),
            };
            assert_eq!(draw.texture, texture);
            assert_eq!(draw.program, if part.flash > 0.0 { 5 } else { 2 });
            assert_eq!(draw.count, 4);
            assert_eq!(draw.matrix, NATIVE_UI_PROJECTION.to_cols_array().map(f32::to_bits));
            let vertices = part.vertices();
            let bits: &[u32] = bytemuck::cast_slice(&vertices);
            assert_eq!(draw.vertices, bits);
            words += 40;
            if part.flash > 0.0 {
                assert_eq!(
                    draw.extra[..4],
                    vec4(1.0, 1.0, 1.0, part.flash).to_array().map(f32::to_bits)
                );
                words += 4;
            }
            draws += 1;
        }
    }
    (words, draws)
}
#[test]
#[ignore = "original77254/79410/27560 full-shell packets and child boundaries; no GPU/window/audio/saves"]
fn higurashi_browser_shell_matches_native_packets_and_child_order() {
    let cases = cases();
    let packets = native(&cases);
    let (words, draws) = check(&cases, &packets);
    eprintln!(
        "Browser shell:{} cases,{draws} exact shell draws,{words} exact words; child0/shell/child1 and unchanged A/B icons",
        cases.len()
    );
}
fn replay(pass: &mut RenderPass, packet: &Packet, atlas: &CharacterTextures) {
    for d in &packet.shell {
        let vertices: Vec<PosColTexVertex> = d
            .vertices
            .chunks_exact(6)
            .map(|v| PosColTexVertex {
                position: glam::vec3(
                    f32::from_bits(v[0]),
                    f32::from_bits(v[1]),
                    f32::from_bits(v[2]),
                ),
                color: UnormColor(v[3]),
                texture_position: glam::vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
            })
            .collect();
        let name = match d.texture {
            15 => "obi",
            16 => "caption",
            17 => "close",
            18 => "help_logview",
            19 => "key",
            _ => panic!("atlas"),
        };
        let texture = atlas.get(name).as_source();
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &d.matrix.map(f32::from_bits),
        ));
        let vertices = VertexSource::VertexData {
            vertices: &vertices,
        };
        let program = if d.program == 5 {
            RenderProgramWithArguments::Button {
                vertices,
                texture,
                transform,
                flash1: Vec4::ZERO,
                flash2: Vec4::from_array(std::array::from_fn(|i| f32::from_bits(d.extra[i]))),
            }
        } else {
            RenderProgramWithArguments::Sprite {
                vertices,
                sprite: texture,
                transform,
            }
        };
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(program, DrawPrimitive::TrianglesStrip),
        );
    }
}
#[test]
#[ignore = "actual shell/help/key archive against original-packet replay on offscreen Metal720p/1080p; no window/audio/saves"]
fn higurashi_browser_shell_matches_native_pixels_at_720p_and_1080p() {
    use crate::adv::higurashi::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
    let (device, queue, assets) = gpu_assets();
    let atlas: Arc<CharacterTextures> = assets.load_sync("/logview.txa").unwrap();
    for name in ["obi", "caption", "close", "help_logview", "key"] {
        assert!(atlas.contains(name));
    }
    let all = cases();
    let cases: Vec<_> = [
        0, 1, 7, 18, 24, 49, 96, 121, 239, 264, 288, 313, 335, 360, 383, 408, 432, 457, 479, 482,
    ]
    .into_iter()
    .map(|i| all[i])
    .collect();
    let packets = native(&cases);
    check(&cases, &packets);
    let mut pixels = 0;
    let mut hashes = String::new();
    for (i, (case, packet)) in cases.iter().zip(&packets).enumerate() {
        for size in [(1280, 720), (1920, 1080)] {
            let clear = UnormColor(0xff503020);
            let expected = render_prepared_frame(
                &device,
                &queue,
                size,
                clear,
                |_| {},
                |pass| replay(pass, packet, &atlas),
            );
            let actual = render_prepared_frame(
                &device,
                &queue,
                size,
                clear,
                |_| {},
                |pass| {
                    if case.ready {
                        render(pass, &atlas, case.state);
                    }
                },
            );
            assert_eq!(
                actual.iter().zip(&expected).position(|(a, b)| a != b),
                None,
                "shell frame{i} {size:?}"
            );
            pixels += size.0 as usize * size.1 as usize;
            let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ *b as u64).wrapping_mul(0x100000001b3)
            });
            writeln!(hashes, "{i} {} {} {hash:016x}", size.0, size.1).unwrap();
        }
    }
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_SHELL_CAPTURE") {
        std::fs::write(std::path::Path::new(&dir).join("frames.txt"), hashes).unwrap();
    }
    eprintln!(
        "Browser shell:{} byte-identical720p/1080p frames,{pixels} compared pixels",
        cases.len() * 2
    );
}
