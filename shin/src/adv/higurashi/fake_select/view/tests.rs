//! Actual widget state and original draw packets, captured before Rust runs.
use super::*;
use crate::adv::higurashi::{
    message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame},
    message_playback::MessageVoice,
};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    DepthStencilState, StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    shaders::types::vertices::TextVertex,
};
use std::{collections::HashMap, fmt::Write as _, path::PathBuf};

struct Draw {
    program: u32,
    texture: u32,
    blend: u32,
    stencil: [u32; 5],
    matrix: [u32; 16],
    extra: [u32; 16],
    color: [u32; 16],
    vertices: Vec<u32>,
}
fn word<'a>(words: &mut impl Iterator<Item = &'a str>) -> u32 {
    words.next().unwrap().parse().unwrap()
}
fn words<'a, const N: usize>(input: &mut impl Iterator<Item = &'a str>) -> [u32; N] {
    std::array::from_fn(|_| word(input))
}
fn bits<'a>(words: &mut impl Iterator<Item = &'a str>) -> u32 {
    u32::from_str_radix(words.next().unwrap(), 16).unwrap()
}
fn vector(raw: &[u32]) -> Vec4 {
    Vec4::from_array(std::array::from_fn(|i| f32::from_bits(raw[i])))
}
fn native_color(value: u32) -> u32 {
    (value & 0xff00ff00) | ((value & 255) << 16) | ((value >> 16) & 255)
}
#[derive(Default)]
struct Voice {
    status: i32,
    events: Vec<[u32; 3]>,
}
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        self.status & 2 != 0
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        panic!("FakeSelect uses explicit voice commands without MSGSET metadata")
    }
    fn stop(&mut self) {
        panic!("FakeSelect does not stop the ADV voice")
    }
}
impl VoiceCommands for Voice {
    fn play_command(&mut self, name: &str, gain: f32, flags: i32) {
        assert_eq!(flags, 0);
        let code = super::super::VOICES
            .iter()
            .position(|value| *value == name)
            .unwrap();
        self.events.push([1, code as u32, gain.to_bits()]);
    }
    fn status_bits(&self) -> i32 {
        self.status
    }
}
fn parent(screen: &Screen) -> Mat4 {
    multiply(
        multiply(
            screen
                .properties
                .composed_transform(&HigurashiTransform::default())
                .compute_native_final_transform(),
            Mat4::from_translation(vec3(-640., -720., 0.)),
        ),
        Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
    )
}
fn check_packets(
    draws: &[Draw],
    screen: &Screen,
    jitter: Vec2,
    stencil: u8,
    kind: PassKind,
) -> usize {
    if kind == PassKind::Opaque {
        assert!(draws.is_empty());
        return 0;
    }
    let mut remaining = draws.iter();
    let mut glyphs = 0;
    for (index, row) in screen.model.rows.iter().enumerate() {
        let matrix = multiply(
            parent(screen),
            Mat4::from_translation(vec3(jitter.x + row.x.value(), jitter.y + row.y, 0.)),
        );
        let alpha = row.alpha.value();
        for (top, bottom, uv_top, uv_bottom) in [
            (-8., 0., 0., 0.1),
            (0., row.height, 0.1, 0.9),
            (row.height, row.height + 8., 0.9, 1.),
        ] {
            let draw = remaining.next().unwrap();
            assert_eq!((draw.program, draw.texture, draw.blend), (2, 0, 2));
            assert_eq!(
                draw.matrix,
                matrix.to_cols_array().map(f32::to_bits),
                "FakeSelect sprite matrix"
            );
            let vertices = build_quad_vertices(|corner| PosColTexVertex {
                position: vec3(
                    if corner.x == 0. { 360. } else { 1560. },
                    if corner.y == 0. { top } else { bottom },
                    0.,
                ),
                texture_position: vec2(corner.x, if corner.y == 0. { uv_top } else { uv_bottom }),
                color: FloatColor4::from_rgba(1., 1., 1., alpha).into_unorm(),
            });
            let mut native = draw.vertices.clone();
            for vertex in native.chunks_exact_mut(6) {
                vertex[3] = native_color(vertex[3]);
            }
            assert_eq!(native, bytemuck::cast_slice::<_, u32>(&vertices));
        }
        let glyph = &screen.glyphs[index];
        let text = multiply(
            multiply(matrix, Mat4::from_translation(vec3(0., 8., 0.))),
            multiply(
                Mat4::from_translation(vec3(960., 0., 0.)),
                multiply(
                    Mat4::from_scale(vec3(1., 1., 0.)),
                    Mat4::from_translation(vec3(
                        -glyph.layout.size.x * 0.5,
                        -(glyph.layout.size.y * 0.),
                        0.,
                    )),
                ),
            ),
        );
        let fill = vec4(
            row.focus.value(),
            row.focus.value(),
            row.focus.value(),
            alpha,
        );
        if alpha > 0. {
            for border in [true, false] {
                for glyph in &glyph.layout.glyphs {
                    let draw = remaining.next().unwrap();
                    assert_eq!(
                        (draw.program, draw.texture, draw.blend),
                        (if border { 4 } else { 3 }, glyph.point as u32, 2)
                    );
                    assert_eq!(
                        draw.matrix,
                        text.to_cols_array().map(f32::to_bits),
                        "FakeSelect glyph matrix"
                    );
                    assert_eq!(
                        draw.vertices,
                        bytemuck::cast_slice::<_, u32>(&glyph.vertices)
                    );
                    let extra: Vec<_> = if border {
                        glyph
                            .border
                            .iter()
                            .flat_map(|value| value.to_array())
                            .collect()
                    } else {
                        fill.to_array().into_iter().chain([0.; 12]).collect()
                    };
                    assert_eq!(
                        draw.extra,
                        extra
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>()
                            .as_slice()
                    );
                    let color = if border {
                        vec4(0., 0., 0., alpha)
                    } else {
                        fill
                    };
                    assert_eq!(
                        draw.color,
                        color
                            .to_array()
                            .into_iter()
                            .chain([0.; 12])
                            .map(f32::to_bits)
                            .collect::<Vec<_>>()
                            .as_slice()
                    );
                    glyphs += 1;
                }
            }
        }
    }
    assert!(remaining.next().is_none());
    for draw in draws {
        assert_eq!(
            draw.stencil,
            [6, 0, 0, 2, (u32::from(stencil) << 16) | 0xffff]
        );
    }
    glyphs
}
fn replay(
    pass: &mut RenderPass,
    draws: &[Draw],
    screen: &Screen,
    handles: &HashMap<u32, crate::asset::font::GpuFontGlyphHandle>,
) {
    for draw in draws {
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline: StencilPipelineState {
                        function: StencilFunction::GreaterOrEqual,
                        pass_operation: StencilOperation::Replace,
                        ..Default::default()
                    },
                    stencil_reference: (draw.stencil[4] >> 16) as u8,
                },
                ..Default::default()
            });
        let transform = HigurashiTransform::convert_clip_depth(Mat4::from_cols_array(
            &draw.matrix.map(f32::from_bits),
        ));
        if draw.program == 2 {
            let vertices: Vec<_> = draw
                .vertices
                .chunks_exact(6)
                .map(|raw| PosColTexVertex {
                    position: vec3(
                        f32::from_bits(raw[0]),
                        f32::from_bits(raw[1]),
                        f32::from_bits(raw[2]),
                    ),
                    color: UnormColor(native_color(raw[3])),
                    texture_position: vec2(f32::from_bits(raw[4]), f32::from_bits(raw[5])),
                })
                .collect();
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: screen.textures.focused_selection.as_source(),
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices: [TextVertex; 4] = std::array::from_fn(|index| TextVertex {
                position: vector(&draw.vertices[5 * index..]),
                color: f32::from_bits(draw.vertices[5 * index + 4]),
            });
            let vertices = VertexSource::VertexData {
                vertices: &vertices,
            };
            let glyph = handles[&draw.texture].as_texture_source().unwrap();
            let program = if draw.program == 3 {
                RenderProgramWithArguments::Font {
                    vertices,
                    glyph,
                    transform,
                    color1: FloatColor4::from_vec4(vector(&draw.extra)),
                    color2: FloatColor4::from_vec4(vector(&draw.color)),
                }
            } else {
                assert_eq!(draw.program, 4);
                RenderProgramWithArguments::FontBorder {
                    vertices,
                    glyph,
                    transform,
                    distances: std::array::from_fn(|i| {
                        vec2(
                            f32::from_bits(draw.extra[2 * i]),
                            f32::from_bits(draw.extra[2 * i + 1]),
                        )
                    }),
                    color: FloatColor4::from_vec4(vector(&draw.color)),
                }
            };
            pass.run(builder.build(program, DrawPrimitive::TrianglesStrip));
        }
    }
}

fn check_replay_and_forward(server: &AssetServer) {
    use crate::adv::higurashi::{
        autosave::{ProgressEntry, SaveProgress},
        checkpoints::CheckpointMemory,
        progress::ProgressMemory,
        read_history::MessageReadHistory,
        replay::ReplaySession,
    };
    use shin_core::format::{save::PersistData, scenario::higurashi::HigurashiScenario};
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let audit: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/higurashi/layerwait-authored-audit.json"
    )))
    .unwrap();
    let mut code = Vec::new();
    let mut commands = 0;
    for name in ["TROPHY", "LOGSET", "DEBUGOUT", "UNLOCK", "FAKESELECT"] {
        for row in audit["first_occurrences"][name].as_array().unwrap() {
            let start = usize::from_str_radix(row["start"].as_str().unwrap(), 16).unwrap();
            let end = usize::from_str_radix(row["end"].as_str().unwrap(), 16).unwrap();
            code.extend_from_slice(&authored.raw()[start..end]);
            commands += 1;
        }
    }
    let mut raw = authored.raw().to_vec();
    let entry = authored.header.code_offset as usize;
    raw[entry..entry + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let history = MessageReadHistory::new();
    for forward in [None, Some(false), Some(true)] {
        let entry = ProgressEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: 0,
            replay_target: 0,
            response_data: vec![0x12, 0x34, 0x56],
        };
        let progress = ProgressMemory::new(
            PersistData(vec![0; 64]),
            SaveProgress::new(1, 0x12345678, [entry.clone(), entry]).unwrap(),
            vec![0x45, 0x67],
        )
        .with_trailing_values(vec![0x1234, 0x5678]);
        let fork = CheckpointMemory::new(vec![])
            .fork_for_restore(0, 0, &progress)
            .unwrap();
        let mut replay = ReplaySession::new(scenario.clone(), fork).unwrap();
        let data = replay.progress.progress().clone();
        let timeline = replay.read.timeline();
        let state = replay.read.state();
        let descriptors = replay.read.descriptors().clone();
        for _ in 0..commands {
            replay.prepare_command(&history, 100_000).unwrap();
            assert!(replay.poll_command(forward).unwrap());
            replay.retire_command();
            assert_eq!(replay.progress.progress(), &data);
            assert_eq!(replay.read.timeline(), timeline);
            let actual = replay.read.state();
            assert_eq!(
                (actual.flags, actual.fast_forward_blocked, actual.is_read),
                (state.flags, state.fast_forward_blocked, state.is_read),
            );
            assert_eq!(replay.read.descriptors(), &descriptors);
            assert!(replay.memory.entries().is_empty());
            assert_eq!(replay.progress.responses().1, 0);
            assert_eq!(replay.progress.trailing_values().unwrap(), [0x1234, 0x5678]);
            assert_eq!(replay.progress.bytes(), [0x45, 0x67]);
        }
    }
    eprintln!(
        "Command-tail:{commands} bounded authored commands through actual ReplaySession decode/prepare/poll/retire, replay and both forward policies preserve progress/read/descriptors/checkpoints and response cursor"
    );
}

#[test]
#[ignore = "frozen original FakeSelect factory/real font/update/FF/voice gates/render packets;offscreen Metal720p+1080p;no OS audio/window/save"]
fn higurashi_fake_select_matches_original_widget_states_packets_and_pixels() {
    let reference = PathBuf::from(std::env::var("SHIN_HIGURASHI_COMMAND_TAIL_REFERENCE").unwrap());
    let input = std::fs::read_to_string(reference.join("fakeselect-input.txt")).unwrap();
    let output = std::fs::read_to_string(reference.join("fakeselect-output.txt")).unwrap();
    assert_eq!(input.lines().count(), output.lines().count());
    let (device, queue, server) = gpu_assets();
    check_replay_and_forward(&server);
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    let textures: Arc<HigurashiMessageTextures> = server.load_sync("/msgtex.txa").unwrap();
    let points = super::super::LABELS
        .iter()
        .flat_map(|label| label.chars())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let handles = points
        .iter()
        .map(|point| *point as u32)
        .zip(
            font.clone()
                .load_glyphs(device.clone(), queue.clone(), &points),
        )
        .collect::<HashMap<_, _>>();
    let mut screen: Option<Screen> = None;
    let mut voice = Voice::default();
    let (
        mut profile,
        mut sequence,
        mut owners,
        mut frames,
        mut packets,
        mut glyph_packets,
        mut pixel_frames,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let mut pixel_hashes = String::new();
    for (request, observation) in input.lines().zip(output.lines()) {
        let mut input = request.split_whitespace();
        let mut native = observation.split_whitespace();
        let op = input.next().unwrap();
        assert_eq!(native.next(), Some(op));
        if op == "N" {
            profile = word(&mut input);
            sequence = 0;
        } else {
            sequence += 1;
        }
        assert_eq!(word(&mut native), profile);
        assert_eq!(word(&mut native), sequence);
        let mut events = Vec::new();
        match op {
            "N" => {
                assert!(screen.is_none());
                let seed = word(&mut input);
                screen = Some(
                    Screen::new(&server, font.clone(), textures.clone(), [1, 2, 3, 4], seed)
                        .unwrap(),
                );
                voice = Voice::default();
                owners += 1;
            }
            "U" => {
                let screen = screen.as_mut().unwrap();
                let before = screen.model.phase;
                screen.model.advance(
                    Ticks::from_f32(f32::from_bits(bits(&mut input))),
                    &mut voice,
                );
                events.append(&mut voice.events);
                for cue in screen.model.take_cues() {
                    let code = ["select", "move", "seldecide"]
                        .iter()
                        .position(|name| *name == cue)
                        .unwrap();
                    events.push([0, code as u32, 1f32.to_bits()]);
                }
                if before == 3 && screen.model.complete() {
                    events.push([2, 0, 0]);
                }
            }
            "F" => screen.as_mut().unwrap().model.fast_forward(),
            "C" => {
                let property = word(&mut input) as usize;
                let value = f32::from_bits(bits(&mut input));
                screen
                    .as_mut()
                    .unwrap()
                    .properties
                    .control(property, value, Ticks::ZERO, 0)
                    .unwrap();
            }
            "B" => {
                let count = word(&mut input);
                let entries: Vec<_> = (0..count).map(|_| word(&mut input)).collect();
                voice.status = if entries.contains(&0) {
                    3
                } else if entries.contains(&1) {
                    2
                } else {
                    0
                };
            }
            "I" => {} // Actual no-op input is checked by the original probe;
            // actual desktop routing is separately exercised through Gameplay.
            "D" => {
                drop(screen.take().unwrap());
                assert_eq!(native.next(), Some("X"));
                assert_eq!(word(&mut native), 0);
                assert!(native.next().is_none() && input.next().is_none());
                continue;
            }
            "R" => {
                let kind = if word(&mut input) == 0 {
                    PassKind::Transparent
                } else {
                    PassKind::Opaque
                };
                let stencil = word(&mut input) as u8;
                let mut draws = Vec::new();
                while native.clone().next() == Some("Q") {
                    assert_eq!(native.next(), Some("Q"));
                    let program = word(&mut native);
                    let texture = word(&mut native);
                    let blend = word(&mut native);
                    let count = word(&mut native);
                    draws.push(Draw {
                        program,
                        texture,
                        blend,
                        stencil: words(&mut native),
                        matrix: words(&mut native),
                        extra: words(&mut native),
                        color: words(&mut native),
                        vertices: (0..count).map(|_| word(&mut native)).collect(),
                    });
                }
                assert_eq!(native.next(), Some("E"));
                assert_eq!(word(&mut native) as usize, draws.len());
                let screen = screen.as_mut().unwrap();
                let before = screen.model.seed;
                let jitter = if kind == PassKind::Transparent {
                    screen.model.sample_jitter()
                } else {
                    Vec2::ZERO
                };
                let after = screen.model.seed;
                glyph_packets += check_packets(&draws, screen, jitter, stencil, kind);
                // Replay original vertices/uniforms against actual renderer.
                // Each resolution starts at the same pre-draw seed; both
                // represent the same native draw rather than extra frames.
                for size in [(1280, 720), (1920, 1080)] {
                    screen.model.seed = before;
                    let expected = render_prepared_frame(
                        &device,
                        &queue,
                        size,
                        UnormColor(0xff503020),
                        |_| {},
                        |pass| replay(pass, &draws, screen, &handles),
                    );
                    let actual = render_prepared_frame(
                        &device,
                        &queue,
                        size,
                        UnormColor(0xff503020),
                        |_| {},
                        |pass| screen.render(pass, stencil, kind).unwrap(),
                    );
                    assert_eq!(
                        actual.iter().zip(&expected).position(|(a, b)| a != b),
                        None,
                        "FakeSelect profile{profile} action{sequence} {size:?}"
                    );
                    assert_eq!(screen.model.seed, after);
                    let hash = actual.iter().fold(0xcbf29ce484222325u64, |h, byte| {
                        (h ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
                    });
                    writeln!(
                        pixel_hashes,
                        "{profile} {sequence} {} {} {hash:016x}",
                        size.0, size.1
                    )
                    .unwrap();
                    pixel_frames += 1;
                }
                packets += draws.len();
                frames += 1;
            }
            _ => panic!("widget input {request}"),
        }
        assert!(input.next().is_none());
        let screen = screen.as_ref().unwrap();
        assert_eq!(native.next(), Some("S"));
        let expected: [u32; 10] = words(&mut native);
        assert_eq!(
            expected,
            [
                screen.model.seed,
                screen.model.selected as u32,
                screen.model.phase,
                screen.model.countdown.to_bits(),
                screen.model.remaining,
                1,
                4,
                u32::from(screen.model.complete()),
                2,
                voice.status as u32
            ],
            "{profile}:{sequence} {request}"
        );
        assert_eq!(word(&mut native), 2);
        for (index, row) in screen.model.rows.iter().enumerate() {
            assert_eq!(native.next(), Some("R"));
            let actual: [u32; 16] = words(&mut native);
            let size = screen.glyphs[index].layout.size;
            assert_eq!(
                actual,
                [
                    index as u32,
                    index as u32,
                    row.y.to_bits(),
                    row.height.to_bits(),
                    1f32.to_bits(),
                    1f32.to_bits(),
                    1f32.to_bits(),
                    1f32.to_bits(),
                    size.x.to_bits(),
                    size.y.to_bits(),
                    row.x.value().to_bits(),
                    u32::from(!row.x.is_idle()),
                    row.alpha.value().to_bits(),
                    u32::from(!row.alpha.is_idle()),
                    row.focus.value().to_bits(),
                    u32::from(!row.focus.is_idle())
                ],
                "row{index} {profile}:{sequence} {request}"
            );
        }
        assert_eq!(native.next(), Some("A"));
        let count = word(&mut native);
        let actual: Vec<[u32; 3]> = (0..count).map(|_| words(&mut native)).collect();
        assert_eq!(actual, events, "events {profile}:{sequence} {request}");
        assert!(native.next().is_none());
    }
    assert!(screen.is_none());
    assert!(owners >= 15 && frames >= 100 && glyph_packets > 1000);
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_COMMAND_TAIL_PIXEL_HASHES") {
        std::fs::write(path, pixel_hashes).unwrap();
    }
    eprintln!(
        "FakeSelect:{owners} native owners/{frames} original render states/{packets} packets/{glyph_packets} glyph packets/{pixel_frames} byte-identical720p+1080p frames;all action states/private tweens/countdown/loading+playing voice gates/cues/callback/jitter/seed-extremes/FF/properties/opaque-input gates pass"
    );
}
