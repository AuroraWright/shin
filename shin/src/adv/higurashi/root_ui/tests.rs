use super::*;
use crate::adv::higurashi::{message_glyphs::tests::gpu::render_frame, reference_gpu_assets};
use glam::Mat4;
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(crate) struct NativeDraw {
    texture: u32,
    matrix: [u32; 16],
    vertices: [u32; 24],
}
pub(crate) fn native(states: &[(bool, State)]) -> Vec<Vec<NativeDraw>> {
    let mut input = String::new();
    for (ready, s) in states {
        let vm = s.vm.unwrap_or(ReadState {
            flags: 0,
            fast_forward_blocked: 0,
            is_read: 0,
        });
        input += &format!(
            "{} {} {:x} {} {} {:x} {} {} {:x} {:x} {} {:x}\n",
            u8::from(*ready),
            u8::from(s.vm.is_some()),
            vm.flags,
            vm.fast_forward_blocked,
            vm.is_read,
            s.modes,
            u8::from(s.forced),
            u8::from(s.read_only),
            s.elapsed_seconds.to_bits(),
            s.navi_slide.to_bits(),
            s.pressed,
            s.hint.to_bits()
        );
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_ROOT_UI").unwrap())
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
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut words = text.split_whitespace();
    let mut results = Vec::new();
    while let Some(op) = words.next() {
        assert_eq!(op, "R");
        let mut draws = Vec::new();
        loop {
            let op = words.next().unwrap();
            if op == "E" {
                assert_eq!(words.next().unwrap().parse::<usize>().unwrap(), draws.len());
                assert_eq!(words.next(), Some("4"));
                break;
            }
            assert_eq!(op, "P");
            let mut number = || words.next().unwrap().parse::<u32>().unwrap();
            assert_eq!(number(), 2);
            let texture = number();
            assert!(texture < 3);
            assert_eq!(number(), 0);
            assert_eq!(number(), 24);
            let matrix = std::array::from_fn(|_| number());
            for _ in 0..32 {
                number();
            }
            let vertices = std::array::from_fn(|_| number());
            draws.push(NativeDraw {
                texture,
                matrix,
                vertices,
            });
        }
        results.push(draws);
    }
    assert_eq!(results.len(), states.len());
    results
}
fn words(packet: &Packet) -> [u32; 24] {
    std::array::from_fn(|i| {
        let v = packet.vertices[i / 6];
        match i % 6 {
            0 => v.position.x.to_bits(),
            1 => v.position.y.to_bits(),
            2 => v.position.z.to_bits(),
            3 => v.color.0,
            4 => v.texture_position.x.to_bits(),
            5 => v.texture_position.y.to_bits(),
            _ => unreachable!(),
        }
    })
}
fn rgba(bgra: u32) -> u32 {
    // Same native BGRA8 -> shared RGBA8 conversion verified by Tile/PIC/BUP.
    (bgra & 0xff00ff00) | ((bgra & 255) << 16) | ((bgra >> 16) & 255)
}
pub(crate) fn replay(draws: &[NativeDraw], atlas: &CharacterTextures, pass: &mut RenderPass) {
    let clip = Mat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
    ]);
    for draw in draws {
        let vertices = std::array::from_fn::<_, 4, _>(|i| PosColTexVertex {
            position: vec3(
                f32::from_bits(draw.vertices[i * 6]),
                f32::from_bits(draw.vertices[i * 6 + 1]),
                f32::from_bits(draw.vertices[i * 6 + 2]),
            ),
            color: UnormColor(rgba(draw.vertices[i * 6 + 3])),
            texture_position: vec2(
                f32::from_bits(draw.vertices[i * 6 + 4]),
                f32::from_bits(draw.vertices[i * 6 + 5]),
            ),
        });
        let texture = atlas
            .get(["adv", "navi", "skip"][draw.texture as usize])
            .as_source();
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: texture,
                        transform: clip * Mat4::from_cols_array(&draw.matrix.map(f32::from_bits)),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
fn cases() -> Vec<(bool, State)> {
    let mut result = Vec::new();
    for vm in [
        None,
        Some(ReadState {
            flags: 0x1234,
            fast_forward_blocked: 0,
            is_read: 0,
        }),
        Some(ReadState {
            flags: u32::MAX,
            fast_forward_blocked: 0,
            is_read: 0,
        }),
        Some(ReadState {
            flags: u32::MAX,
            fast_forward_blocked: 0,
            is_read: 255,
        }),
        Some(ReadState {
            flags: u32::MAX,
            fast_forward_blocked: 255,
            is_read: 255,
        }),
    ] {
        for modes in [0, 1, 2, 3, 0x83] {
            for forced in [false, true] {
                for read_only in [false, true] {
                    for elapsed_seconds in [0.0, 0.1, 0.25, 0.37, 1.0, 10000.0] {
                        result.push((
                            true,
                            State {
                                vm,
                                modes,
                                forced,
                                read_only,
                                elapsed_seconds,
                                navi_slide: 1.0,
                                pressed: -1,
                                hint: 0.37,
                            },
                        ));
                    }
                }
            }
        }
    }
    for navi_slide in [0.0, -0.0, 0.0001, 0.1, 0.37, 0.7, 1.0, 1.2] {
        for pressed in [-1, 0, 1, 2, 3, 4, 5] {
            for vm in [
                None,
                Some(ReadState {
                    flags: 0,
                    fast_forward_blocked: 0,
                    is_read: 0,
                }),
            ] {
                result.push((
                    true,
                    State {
                        navi_slide,
                        pressed,
                        vm,
                        ..Default::default()
                    },
                ));
            }
        }
    }
    for ready in [false, true] {
        for hint in [0.0, -0.0, -0.1, 0.01, 0.37, 1.0, 1.2, f32::NAN] {
            result.push((
                ready,
                State {
                    modes: 3,
                    navi_slide: 1.0,
                    pressed: 4,
                    hint,
                    ..Default::default()
                },
            ));
        }
    }
    result
}
#[test]
#[ignore = "original complete1382c root UI packets, actual helpers and ordered child boundaries"]
fn higurashi_root_ui_packets_match_updated_executable() {
    let cases = cases();
    let native = native(&cases);
    let mut draws = 0;
    for (i, ((ready, state), expected)) in cases.iter().zip(native).enumerate() {
        let actual = if *ready { packets(*state) } else { Vec::new() };
        assert_eq!(actual.len(), expected.len(), "profile{i}");
        for (j, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(
                actual.texture as u32, expected.texture,
                "profile{i} draw{j}"
            );
            let mut expected_words = expected.vertices;
            for i in 0..4 {
                expected_words[i * 6 + 3] = rgba(expected_words[i * 6 + 3]);
            }
            assert_eq!(words(actual), expected_words, "profile{i} draw{j}");
            draws += 1;
        }
    }
    eprintln!(
        "Root UI:{} native profiles/{draws} exact24-word sprite packets;auto/skip read-policy colors,pulse,slide/pressed UV,disabled backlog,event hint,missing atlas and ordered child boundaries pass",
        cases.len()
    );
}
#[test]
#[ignore = "native root UI sprite replay with actual adv.txa at720p/1080p;offscreen Metal/exact Loopback Audio"]
fn higurashi_root_ui_pixels_match_native_at_720p_and_1080p() {
    let states = (0..30)
        .map(|i| {
            (
                true,
                State {
                    vm: (i % 5 != 0).then_some(ReadState {
                        flags: if i % 3 == 0 { 0 } else { u32::MAX },
                        fast_forward_blocked: u8::from(i % 4 == 0),
                        is_read: u8::from(i % 2 == 0),
                    }),
                    modes: [0, 1, 2, 3][i % 4],
                    forced: i % 2 == 0,
                    read_only: i % 3 == 0,
                    elapsed_seconds: [0.0, 0.1, 0.37, 1.0, 100.0][i % 5],
                    navi_slide: [0.0, 0.1, 0.37, 0.7, 1.0][i % 5],
                    pressed: (i % 7) as i32 - 1,
                    hint: [0.0, 0.37, 1.0][i % 3],
                },
            )
        })
        .collect::<Vec<_>>();
    let native = native(&states);
    let (device, queue, assets) = reference_gpu_assets();
    let atlas = assets.load_sync::<CharacterTextures>("/adv.txa").unwrap();
    for (i, ((_, state), expected)) in states.iter().zip(native).enumerate() {
        for size in [(1280, 720), (1920, 1080)] {
            let actual = render_frame(&device, &queue, size, |pass| render(*state, &atlas, pass));
            let expected = render_frame(&device, &queue, size, |pass| {
                replay(&expected, &atlas, pass)
            });
            let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
            assert_eq!(mismatch, None, "profile{i} at{size:?}");
        }
    }
    eprintln!(
        "Root UI:60 whole RGBA frames match native auto/skip indicators,read-only pulses,navi slides/pressed/disabled rows and event hint packets at720p/1080p"
    );
}
