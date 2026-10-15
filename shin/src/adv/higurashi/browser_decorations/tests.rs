use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

struct Draw {
    texture: u32,
    matrix: [u32; 16],
    vertices: Vec<u32>,
}
struct Case {
    matrix: Mat4,
    geometry: Vec<Vec<PosColTexVertex>>,
    texture: u32,
    input: String,
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn native(cases: &[Case]) -> Vec<Vec<Draw>> {
    let input: String = cases.iter().map(|c| c.input.as_str()).collect();
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_DECORATION").unwrap())
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_DECORATION_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("input.txt"), input).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("native.txt"),
            &output.stdout,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("native.log"),
            &output.stderr,
        )
        .unwrap();
    }
    let text = String::from_utf8(output.stdout).unwrap();
    text.lines()
        .map(|line| {
            let mut w = line.split_whitespace();
            assert_eq!(w.next(), Some("R"));
            let mut draws = Vec::new();
            while let Some(tag) = w.next() {
                if tag == "E" {
                    assert_eq!(word(&mut w) as usize, draws.len());
                    assert!(w.next().is_none());
                    break;
                }
                assert_eq!(tag, "Q");
                let texture = word(&mut w);
                let count = word(&mut w) as usize;
                let matrix = std::array::from_fn(|_| word(&mut w));
                let vertices = (0..count * 6).map(|_| word(&mut w)).collect();
                draws.push(Draw {
                    texture,
                    matrix,
                    vertices,
                });
            }
            draws
        })
        .collect()
}
fn matrix_input(matrix: Mat4) -> String {
    matrix
        .to_cols_array()
        .iter()
        .map(|v| format!(" {v}"))
        .collect()
}
fn cases() -> Vec<Case> {
    let mut result = Vec::new();
    let projection = parent_transform(1.0);
    let mut matrices = vec![Mat4::IDENTITY];
    matrices.extend([0.0, 0.25, 0.5, 0.999, 1.0].map(parent_transform));
    matrices.push(multiply(
        projection,
        Mat4::from_translation(vec3(1848.0, 162.0, 0.0)),
    ));
    for matrix in matrices {
        for (top, height) in [
            (-111.25, 112.0),
            (162.0, 112.0),
            (1024.5, 46.5),
            (0.25, 12.0),
        ] {
            for color in [
                FloatColor4::WHITE.with_alpha(0.0),
                FloatColor4::WHITE.with_alpha(0.5),
                FloatColor4::WHITE,
                FloatColor4::from_rgba(0.1, 0.6, 0.9, 0.125),
            ] {
                let rect = Vec4::new(147.0, top, 1626.0, height);
                let mut input = format!("F{}", matrix_input(matrix));
                for value in rect.to_array().into_iter().chain(color.into_array()) {
                    write!(input, " {value}").unwrap();
                }
                input.push('\n');
                result.push(Case {
                    matrix,
                    geometry: cursor(rect, color).map(|v| v.to_vec()).to_vec(),
                    texture: 11,
                    input,
                });
            }
        }
        for height in [32.0, 818.0, 818.25] {
            for state in [
                Scrollbar {
                    position: 0,
                    visible: 818,
                    first: 0,
                    end: 2000,
                },
                Scrollbar {
                    position: -400,
                    visible: 818,
                    first: -2048,
                    end: 4096,
                },
                Scrollbar {
                    position: 0,
                    visible: 112,
                    first: 0,
                    end: 112,
                },
                Scrollbar {
                    position: 1337,
                    visible: 818,
                    first: 0,
                    end: 2155,
                },
                Scrollbar {
                    position: -2147483640,
                    visible: 32,
                    first: -2147483647,
                    end: 2147483647,
                },
            ] {
                for (x, y) in [(0.0, 0.0), (0.125, -36.25)] {
                    let input = format!(
                        "S{} {height} {x} {y} {} {} {} {}\n",
                        matrix_input(matrix),
                        state.position,
                        state.visible,
                        state.first,
                        state.end
                    );
                    result.push(Case {
                        matrix: scrollbar_transform(matrix, x, y),
                        geometry: scrollbar(height, state).map(|v| v.to_vec()).to_vec(),
                        texture: 12,
                        input,
                    });
                }
            }
        }
    }
    // Exercise the original parent77254 selection/jump branches with an empty
    // retained glyph owner. Worker/glyph population has its own native oracle.
    for slide in [0.0, 0.25, 0.999, 1.0] {
        for (top, scroll) in [(-112.25, -112.0), (42.5, 64.25)] {
            for selected in [0, 1] {
                for jump in [0.0, 0.125, 0.5, 1.0] {
                    let matrix = parent_transform(slide);
                    let rect = Vec4::new(147.0, top - (scroll - 162.0), 1626.0, 112.0);
                    let mut geometry = Vec::new();
                    if selected == 0 {
                        geometry.extend(
                            cursor(rect, FloatColor4::WHITE.with_alpha(0.5)).map(|v| v.to_vec()),
                        );
                    }
                    if jump > 0.0 {
                        geometry.extend(
                            cursor(rect, FloatColor4::WHITE.with_alpha(jump)).map(|v| v.to_vec()),
                        );
                    }
                    result.push(Case {
                        matrix,
                        geometry,
                        texture: 11,
                        input: format!(
                            "R{} {top} {scroll} 112 {slide} {selected} {jump}\n",
                            matrix_input(Mat4::IDENTITY)
                        ),
                    });
                }
            }
        }
    }
    result
}
fn check(cases: &[Case], packets: &[Vec<Draw>]) -> usize {
    assert_eq!(cases.len(), packets.len());
    let mut words = 0;
    for (i, (case, packet)) in cases.iter().zip(packets).enumerate() {
        assert_eq!(case.geometry.len(), packet.len());
        for (geometry, draw) in case.geometry.iter().zip(packet) {
            assert_eq!(draw.texture, case.texture);
            assert_eq!(
                draw.matrix,
                case.matrix.to_cols_array().map(f32::to_bits),
                "matrix case {i}"
            );
            let bits: &[u32] = bytemuck::cast_slice(geometry);
            assert_eq!(draw.vertices, bits, "vertices case {i}");
            words += bits.len() + 16;
        }
    }
    words
}
#[test]
#[ignore = "executes original7910c/c0574 checked native packets; no GPU/audio/window/saves"]
fn higurashi_browser_cursor_and_scrollbar_match_native_packets() {
    let cases = cases();
    let packets = native(&cases);
    let words = check(&cases, &packets);
    eprintln!(
        "Browser decorations:{} cases,{} packets,{words} exact words",
        cases.len(),
        packets.iter().map(Vec::len).sum::<usize>()
    );
}
#[test]
#[ignore = "actual cursor/scroll assets and native packet replay on offscreen Metal720p/1080p; no audio/window/saves"]
fn higurashi_browser_cursor_and_scrollbar_match_native_pixels() {
    use crate::adv::higurashi::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
    use std::sync::Arc;
    let (device, queue, assets) = gpu_assets();
    let log: Arc<CharacterTextures> = assets.load_sync("/logview.txa").unwrap();
    let system: Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
    assert!(log.contains("cursor") && system.contains("scroll"));
    let all = cases();
    // Onscreen cursor, both scroll extents, opening slide, partial alpha and
    // truncated negative document coordinates, through real archive textures.
    let cases: Vec<_> = [235, 236, 237, 189, 302, 303, 306, 308]
        .into_iter()
        .map(|i| {
            let c = &all[i];
            Case {
                matrix: c.matrix,
                geometry: c.geometry.clone(),
                texture: c.texture,
                input: c.input.clone(),
            }
        })
        .collect();
    let packets = native(&cases);
    check(&cases, &packets);
    let mut pixels = 0;
    let mut hashes = String::new();
    for (i, (case, packet)) in cases.iter().zip(&packets).enumerate() {
        let (atlas, name) = if case.texture == 11 {
            (&*log, "cursor")
        } else {
            (&*system, "scroll")
        };
        for size in [(1280, 720), (1920, 1080)] {
            let clear = UnormColor(0xff503020);
            let expected = render_prepared_frame(
                &device,
                &queue,
                size,
                clear,
                |_| {},
                |pass| {
                    for draw in packet {
                        let geometry: Vec<PosColTexVertex> = draw
                            .vertices
                            .chunks_exact(6)
                            .map(|v| PosColTexVertex {
                                position: vec3(
                                    f32::from_bits(v[0]),
                                    f32::from_bits(v[1]),
                                    f32::from_bits(v[2]),
                                ),
                                color: UnormColor(v[3]),
                                texture_position: vec2(f32::from_bits(v[4]), f32::from_bits(v[5])),
                            })
                            .collect();
                        sprite(
                            pass,
                            atlas,
                            name,
                            &geometry,
                            Mat4::from_cols_array(&draw.matrix.map(f32::from_bits)),
                        );
                    }
                },
            );
            let actual = render_prepared_frame(
                &device,
                &queue,
                size,
                clear,
                |_| {},
                |pass| {
                    for geometry in &case.geometry {
                        sprite(pass, atlas, name, geometry, case.matrix);
                    }
                },
            );
            assert_eq!(
                actual.iter().zip(&expected).position(|(a, b)| a != b),
                None,
                "frame{i} {size:?}"
            );
            assert!(
                actual
                    .chunks_exact(4)
                    .any(|p| p != [0x20, 0x30, 0x50, 0xff]),
                "visible decoration frame {i} {size:?}"
            );
            pixels += size.0 as usize * size.1 as usize;
            let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ *b as u64).wrapping_mul(0x100000001b3)
            });
            writeln!(hashes, "{i} {} {} {hash:016x}", size.0, size.1).unwrap();
        }
    }
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_DECORATION_CAPTURE") {
        std::fs::write(std::path::Path::new(&dir).join("frames.txt"), hashes).unwrap();
    }
    eprintln!(
        "Browser decorations:{} byte-identical720p/1080p frames,{pixels} compared pixels",
        cases.len() * 2
    );
}
