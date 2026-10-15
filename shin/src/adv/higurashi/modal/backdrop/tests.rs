use super::*;
use crate::adv::higurashi::{
    frame::tests::gpu::render_to_texture, message_glyphs::tests::gpu::gpu_assets,
};
use glam::{Vec2, Vec3};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

struct Native {
    root: bool,
    packet: Option<Packet>,
    words: Vec<u32>,
}
fn native(cases: &[(bool, f32)]) -> Vec<Native> {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MODAL_BACKDROP").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = cases
        .iter()
        .map(|(has, f)| format!("{} {:x}\n", u32::from(*has), f.to_bits()))
        .collect::<String>();
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
    assert_eq!(text.lines().count(), cases.len());
    text.lines()
        .map(|line| {
            assert!(line.starts_with("R "));
            let words: Vec<u32> = line
                .split_whitespace()
                .skip(1)
                .map(|w| w.parse().unwrap())
                .collect();
            assert_eq!(words.len(), 44);
            let packet = (words[1] == 1).then(|| {
                assert_eq!(words[3], 2);
                Packet {
                    projection: Mat4::from_cols_array(&std::array::from_fn(|i| {
                        f32::from_bits(words[4 + i])
                    })),
                    blend: match words[2] {
                        1 => ColorBlendType::Opaque,
                        5 => ColorBlendType::LayerPremultiplied1,
                        _ => panic!("native blend"),
                    },
                    vertices: std::array::from_fn(|v| PosColTexVertex {
                        position: Vec3::from_array(std::array::from_fn(|i| {
                            f32::from_bits(words[20 + v * 6 + i])
                        })),
                        // Native ARGB and desktop RGBA are identical for greyRGB.
                        color: UnormColor(words[23 + v * 6]),
                        texture_position: Vec2::new(
                            f32::from_bits(words[24 + v * 6]),
                            f32::from_bits(words[25 + v * 6]),
                        ),
                    }),
                }
            });
            Native {
                root: words[0] == 1,
                packet,
                words,
            }
        })
        .collect()
}
fn words(p: &Plan) -> Vec<u32> {
    let mut words = vec![u32::from(p.render_root), u32::from(p.backdrop.is_some())];
    words.extend(if let Some(packet) = &p.backdrop {
        [
            if packet.blend == ColorBlendType::Opaque {
                1
            } else {
                5
            },
            2,
        ]
    } else {
        [0, 0]
    });
    words.extend(
        super::super::capture::packets()[0]
            .projection
            .to_cols_array()
            .map(f32::to_bits),
    );
    if let Some(packet) = &p.backdrop {
        for v in &packet.vertices {
            let pos = v.position;
            let uv = v.texture_position;
            let color = v.color;
            words.extend(pos.to_array().map(f32::to_bits));
            words.push(color.0);
            words.extend(uv.to_array().map(f32::to_bits));
        }
    } else {
        words.extend([0; 24]);
    }
    words
}

#[test]
#[ignore = "original1382c root suppression and packed Sprite words, no GPU/audio/saves"]
fn higurashi_modal_backdrop_packets_match_native_quantization_and_root_gates() {
    let mut fades = vec![
        -3.0,
        -0.25,
        -0.0,
        0.0,
        f32::from_bits(1),
        0.0001,
        1.0,
        1.0001,
        1.5,
        2.0,
        10.0,
    ];
    for i in 0..=1020 {
        let f = i as f32 / 510.0;
        fades.extend([
            f32::from_bits(f.to_bits().saturating_sub(1)),
            f,
            f32::from_bits(f.to_bits() + 1),
        ]);
    }
    let cases: Vec<_> = [false, true]
        .into_iter()
        .flat_map(|has| fades.iter().map(move |fade| (has, *fade)))
        .collect();
    for ((has, fade), n) in cases.iter().zip(native(&cases)) {
        assert_eq!(
            words(&plan(*has, *fade)),
            n.words,
            "native snapshot{has} fade{fade:?}"
        );
    }
    println!(
        "Higurashi modal backdrop:{} exact native cases; missing snapshot, signed/near-zero/over-one fades, all adjacent tint quantization boundaries, root suppression, blend1/5 and original packed Sprite quads pass",
        cases.len()
    );
}

fn replay_native(pass: &mut RenderPass, packet: &Packet, source: TextureSource) {
    let clip = Mat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
    ]);
    // Submit the captured uniforms/vertices/blend independently of Packet::render.
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(packet.blend)
            .build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &packet.vertices,
                    },
                    sprite: source,
                    transform: clip * packet.projection,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

fn copy(pass: &mut RenderPass, source: TextureSource) {
    let vertices = build_quad_vertices(|uv| PosColTexVertex {
        position: vec3(uv.x, uv.y, 0.0),
        color: UnormColor::WHITE,
        texture_position: uv,
    });
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: source,
            transform: crate::render::normalized_projection_matrix(),
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

#[test]
#[ignore = "real host/root/message/backdrop composition against original1382c packets on Metal/exact Loopback Audio"]
fn higurashi_host_modal_backdrop_composes_native_packets_at_720p_and_1080p() {
    use crate::adv::higurashi::{
        audio::{AudioFramePolicy, HigurashiAudio},
        host::HigurashiHost,
        replay::tests::scenario,
        runner::{HigurashiRunner, Policy},
        scene_textures::HigurashiSceneTextures,
    };
    use shin_core::{format::scenario::higurashi::HigurashiScenario, time::Ticks};
    use shin_render::{
        gpu_texture::GpuTexture,
        render_texture::RenderTexture,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::texture::{TextureTarget, TextureTargetKind},
    };
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for pattern in 0..2 {
            let runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                scenario(&[0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0, 0, 0]),
                0,
                42,
                || [1; 4],
            ))
            .unwrap();
            let audio = HigurashiAudio::new(
                Arc::new(shin_audio::AudioManager::new_for_tests()),
                assets.clone(),
                authored.clone(),
            );
            let mut host = HigurashiHost::new(runner, audio);
            let policy = Policy {
                animations_enabled: true,
                paused: false,
                fast_skip: false,
                suppress_transient_audio: false,
                read_flags: 0,
                text_speed: 100,
                outline_mode: 0,
            };
            shin_tasks::block_on(
                host.runner
                    .pump(policy, 8, 10000, || [1; 4], &mut host.audio),
            )
            .unwrap();
            assert!(host.runner.frame().message.glyphs().is_some());
            host.runner
                .advance(Ticks::from_f32(100.0), policy, &mut host.audio, None);
            let raw: Vec<u8> = (0..540u32)
                .flat_map(|y| {
                    (0..960u32).flat_map(move |x| {
                        if pattern == 0 {
                            [64, 128, 192, 255]
                        } else {
                            [
                                ((x * 11 + y * 7) % 256) as u8,
                                ((x * 13 + y * 3) % 256) as u8,
                                ((x * 3 + y * 19) % 256) as u8,
                                ((x + y * 5) % 256) as u8,
                            ]
                        }
                    })
                })
                .collect();
            let gpu = GpuTexture::new_static_with_data(
                &device,
                &queue,
                None,
                winit::dpi::PhysicalSize::new(960, 540),
                shin_render::TEXTURE_FORMAT,
                &raw,
            );
            let mut snapshot = None;
            render_to_texture(&device, &queue, size, |ctx, _| {
                let resize = SurfaceResizeSource::new(ViewportParams::both(
                    winit::dpi::PhysicalSize::new(960, 540),
                ));
                let mut texture = RenderTexture::new(
                    device.clone(),
                    resize.handle(),
                    "Modal backdrop fixture snapshot".into(),
                );
                let mut pass = ctx.begin_pass(
                    texture.as_texture_target(),
                    None,
                    "Modal backdrop fixture source",
                );
                copy(&mut pass, gpu.as_source());
                drop(pass);
                snapshot = Some(texture);
            });
            let render = |host: &HigurashiHost, reference: bool| {
                let n = native(&[(host.modal_snapshot().is_some(), host.modal_progress())])
                    .pop()
                    .unwrap();
                render_to_texture(&device, &queue, size, |ctx, view| {
                    let mut screens = HigurashiSceneTextures::new(ctx);
                    screens
                        .prepare_frame_with_overlay(ctx, host.runner.frame(), None)
                        .unwrap();
                    let mut pass = ctx.begin_pass(
                        TextureTarget {
                            view,
                            kind: TextureTargetKind::RenderTexture,
                        },
                        Some(ctx.depth_stencil),
                        "Higurashi host/native modal frame",
                    );
                    pass.clear(
                        Some(UnormColor::from_rgba(19, 43, 71, 97)),
                        Some(0),
                        Some(1.0),
                    );
                    if reference {
                        if n.root {
                            pass.clear(None, Some(0), Some(1.0));
                            // Root is the shared retained ADV cache; this
                            // fixture independently compares backdrop packets.
                            screens.render_cached_frame(&mut pass, host.runner.frame(), 1);
                        }
                        if let Some(packet) = n.packet {
                            replay_native(
                                &mut pass,
                                &packet,
                                host.modal_snapshot().unwrap().as_texture_source(),
                            );
                        }
                    } else {
                        host.render_adv(&mut pass, &screens).unwrap();
                    }
                })
            };
            let verify = |host: &HigurashiHost| {
                let actual = render(host, false);
                let expected = render(host, true);
                let mismatches = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                assert_eq!(
                    mismatches,
                    0,
                    "host native backdrop{size:?} pattern{pattern} fade{} first={:?}",
                    host.modal_progress(),
                    actual
                        .iter()
                        .zip(&expected)
                        .position(|(a, b)| a != b)
                        .map(|i| (i, actual[i], expected[i]))
                );
            };
            verify(&host);
            frames += 1;
            host.enter_modal(|_| Ok(snapshot.take().unwrap())).unwrap();
            let audio_policy = AudioFramePolicy {
                focus_mode_active: false,
                voice_focus_enabled: false,
                music_rate: 1.0,
                effects_update_enabled: true,
            };
            for ticks in [0.0, 3.0, 3.0, 3.0, 1.0] {
                shin_tasks::block_on(host.update(
                    Ticks::from_f32(ticks),
                    policy,
                    audio_policy,
                    8,
                    10000,
                    || [1; 4],
                ))
                .unwrap();
                verify(&host);
                frames += 1;
            }
            assert_eq!(host.modal_progress(), 1.0);
            let opaque = render(&host, false);
            if pattern == 0 {
                for p in opaque.chunks_exact(4) {
                    assert_eq!(p, &[32, 64, 96, 255]);
                }
            }
            host.exit_modal();
            // Preserve VM/glyph owners while independently advancing the close.
            let paused = Policy {
                paused: true,
                ..policy
            };
            for ticks in [3.0, 3.0, 3.0, 1.0] {
                shin_tasks::block_on(host.update(
                    Ticks::from_f32(ticks),
                    paused,
                    audio_policy,
                    8,
                    10000,
                    || [1; 4],
                ))
                .unwrap();
                verify(&host);
                frames += 1;
            }
            assert!(host.modal_snapshot().is_none());
        }
    }
    println!(
        "Higurashi modal host rendering:{frames} exact whole RGBA frames at720p/1080p; actual retained message/root, mixed-alpha960x540 snapshots, native packed tint/blend/root suppression and fade/retirement pass; exact Loopback Audio; optional hint/indicator/mode drawing remains a caller service"
    );
}
