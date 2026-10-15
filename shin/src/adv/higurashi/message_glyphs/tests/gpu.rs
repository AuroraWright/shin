use super::*;
use crate::{
    asset::system::{AssetLoadContext, locate_assets_for_game},
    cli::Game,
};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};

// No window, audio or save initialization. Each reference pass replays native
// vertex/uniform packets through the same desktop shaders and glyph textures.
pub(in crate::adv::higurashi) fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    render: impl FnOnce(&mut RenderPass),
) -> Vec<u8> {
    render_prepared_frame(device, queue, size, UnormColor::BLACK, |_| {}, render)
}

pub(in crate::adv::higurashi) fn render_prepared_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    clear: UnormColor,
    prepare: impl FnOnce(&mut crate::render::PreRenderContext),
    render: impl FnOnce(&mut RenderPass),
) -> Vec<u8> {
    let samplers = TextureSamplerStore::new(device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        size.0, size.1,
    )));
    let make = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi message offscreen evidence"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let target = make(
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = make(
        shin_render::DEPTH_STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let view = target.create_view(&Default::default());
    let depth = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = crate::render::PreRenderContext {
        device,
        queue,
        breakup_texture: None,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    prepare(&mut context);
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Higurashi message glyphs",
        );
        pass.clear(Some(clear), Some(0), Some(1.0));
        render(&mut pass);
    }
    crate::layer::user::readback_test_texture(&mut context, &target)
}

pub(in crate::adv::higurashi) fn replay_native(
    pass: &mut RenderPass,
    packet: &Packet,
    message: &HigurashiMessageGlyphs,
) {
    replay_native_at_stencil(pass, packet, message, 9);
}

pub(in crate::adv::higurashi) fn replay_native_at_stencil(
    pass: &mut RenderPass,
    packet: &Packet,
    message: &HigurashiMessageGlyphs,
    stencil: u8,
) {
    for draw in &packet.draws {
        let glyph = &message.glyphs[draw.glyph - 1];
        let raw = &packet.vertices[draw.start / 4];
        let vertices: [TextVertex; 4] = std::array::from_fn(|i| TextVertex {
            position: glam::Vec4::from_array(std::array::from_fn(|j| {
                f32::from_bits(raw[5 * i + j])
            })),
            color: f32::from_bits(raw[5 * i + 4]),
        });
        let matrix = Mat4::from_cols_array(&draw.matrix.map(f32::from_bits));
        // Native OpenGL clip Z -> desktop0..1. XY/native uniforms stay intact.
        let clip = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
        ]);
        let transform = clip * matrix;
        let color = |values: &[u32]| {
            FloatColor4::from_rgba(
                f32::from_bits(values[0]),
                f32::from_bits(values[1]),
                f32::from_bits(values[2]),
                f32::from_bits(values[3]),
            )
        };
        let vertices = VertexSource::VertexData {
            vertices: &vertices,
        };
        let texture = glyph.handle.as_texture_source().unwrap();
        let program = if draw.program == 4 {
            RenderProgramWithArguments::FontBorder {
                vertices,
                glyph: texture,
                transform,
                distances: std::array::from_fn(|i| {
                    vec2(
                        f32::from_bits(draw.extra[2 * i]),
                        f32::from_bits(draw.extra[2 * i + 1]),
                    )
                }),
                color: color(&draw.color),
            }
        } else {
            RenderProgramWithArguments::Font {
                vertices,
                glyph: texture,
                transform,
                color1: color(&draw.extra),
                color2: color(&draw.color),
            }
        };
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .depth_stencil_shorthand(stencil, true, true)
                .build(program, DrawPrimitive::TrianglesStrip),
        );
    }
}

pub(in crate::adv::higurashi) fn gpu_assets() -> (wgpu::Device, wgpu::Queue, AssetServer) {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let root = std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let assets = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, Some(&root))
            .unwrap()
            .into(),
        AssetLoadContext {
            game: Game::Higurashi,
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    (device, queue, assets)
}

#[test]
#[ignore = "drives live older message clocks and compares updated native packet replay on offscreen Metal"]
fn higurashi_message_owner_drives_live_progress_and_command_waits() {
    use crate::adv::higurashi::{
        message_owner::HigurashiMessageOwner, message_playback::MessageVoice, scene::CommandStatus,
    };
    use shin_core::{
        format::scenario::instruction_elements::MessageId, time::Ticks,
        vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
    };
    struct Voice;
    impl MessageVoice for Voice {
        fn is_playing(&self) -> bool {
            false
        }
        fn position_micros(&self) -> u64 {
            0
        }
        fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
            false
        }
        fn stop(&mut self) {}
    }
    let (device, queue, assets) = gpu_assets();
    let mut frames = 0;
    let capture = std::path::PathBuf::from(
        std::env::var("SHIN_HIGURASHI_MESSAGE_PLAYBACK_CAPTURES").unwrap(),
    );
    std::fs::create_dir_all(&capture).unwrap();
    for style in 0..5 {
        let text = "名前@r@bかな.@<漢字@>、続く。";
        let hex = text
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let operations = [
            "D",
            "U 3f000000 0 0 0 1 100 0",
            "D",
            "U 3f800000 0 0 0 1 100 0",
            "D",
            "U 40a00000 0 0 0 1 100 0",
            "D",
            "C",
            "D",
            "U 00000000 0 0 0 1 100 0",
            "W -1",
            "D",
            "C",
            "W -1",
            "D",
            "X 1",
            "D",
            "U 41200000 0 0 0 1 100 0",
            "D",
        ];
        let input = format!(
            "N 0 {style} 0 -1 2 -1 123 0 50 50 0 {hex}\n{}\n",
            operations.join("\n")
        );
        let mut child =
            Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_MESSAGE_PLAYBACK").unwrap())
                .args([
                    std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                    std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let packets = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("P "))
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .join("\n");
        let mut packets = parse_packets(&packets).into_iter();
        let mut owner = shin_tasks::block_on(HigurashiMessageOwner::load(&assets)).unwrap();
        let mut voice = Voice;
        let dispatch = |owner: &mut HigurashiMessageOwner, command, skip| {
            owner
                .dispatch(&command, &device, &queue, skip, 0, 50, 0, &voice)
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            dispatch(
                &mut owner,
                Cmd::MSGINIT {
                    style,
                    updated_options: Some([-1, 2])
                },
                false
            ),
            CommandStatus::Complete
        );
        assert_eq!(
            dispatch(
                &mut owner,
                Cmd::MSGSET {
                    msg_id: MessageId(123),
                    auto_wait: 0,
                    argument: -1,
                    text: text.into()
                },
                true
            ),
            CommandStatus::Complete
        );
        let mut drawn = 0;
        for operation in operations {
            let words: Vec<_> = operation.split_whitespace().collect();
            match words[0] {
                "D" => {
                    let packet = packets.next().unwrap();
                    for size in [(1280, 720), (1920, 1080)] {
                        let actual = render_frame(&device, &queue, size, |pass| {
                            // This fixture captures glyph packets only. Full
                            // window/cursor/owner submission has its own test.
                            if let Some(glyphs) = owner.glyphs() {
                                glyphs
                                    .render(
                                        pass,
                                        &HigurashiTransform::default(),
                                        7,
                                        owner.playback.natural_slide,
                                        owner.playback.modal_slide,
                                        owner.playback.height,
                                        &owner.playback.progress(),
                                    )
                                    .unwrap();
                            }
                        });
                        let expected = render_frame(&device, &queue, size, |pass| {
                            if let Some(glyphs) = owner.glyphs() {
                                replay_native(pass, &packet, glyphs);
                            } else {
                                assert!(packet.draws.is_empty());
                            }
                        });
                        assert_eq!(
                            actual, expected,
                            "message owner style{style} draw{drawn} at{size:?}"
                        );
                        std::fs::write(
                            capture.join(format!(
                                "style{style}-frame{drawn}-{}x{}.rgba",
                                size.0, size.1
                            )),
                            actual,
                        )
                        .unwrap();
                        frames += 1;
                    }
                    drawn += 1;
                }
                "U" => owner.advance(
                    Ticks::from_f32(f32::from_bits(u32::from_str_radix(words[1], 16).unwrap())),
                    false,
                    false,
                    &mut voice,
                ),
                "C" => {
                    assert!(owner.click(&mut voice));
                }
                "W" => {
                    if owner.command_pending() {
                        owner.poll_command(&voice);
                    } else {
                        owner
                            .dispatch(
                                &Cmd::MSGWAIT { signal: -1 },
                                &device,
                                &queue,
                                false,
                                0,
                                50,
                                0,
                                &voice,
                            )
                            .unwrap();
                    }
                }
                "X" => {
                    owner.poll_command(&voice);
                    assert!(!owner.command_pending());
                    assert_eq!(
                        owner
                            .dispatch(
                                &Cmd::MSGCLOSE { wait: 1 },
                                &device,
                                &queue,
                                false,
                                0,
                                50,
                                0,
                                &voice
                            )
                            .unwrap(),
                        Some(CommandStatus::Pending)
                    );
                }
                _ => unreachable!(),
            }
        }
        assert!(packets.next().is_none());
        assert_eq!(owner.poll_command(&voice), CommandStatus::Complete);
        assert!(owner.glyphs().is_none());
        // An auto-wait command must retain ownership, even for empty text,
        // until the native final key wait is released.
        let status = owner
            .dispatch(
                &Cmd::MSGSET {
                    msg_id: MessageId(124),
                    auto_wait: 1,
                    argument: -1,
                    text: "@r".into(),
                },
                &device,
                &queue,
                true,
                0,
                50,
                0,
                &voice,
            )
            .unwrap();
        assert_eq!(status, Some(CommandStatus::Pending));
        assert!(
            owner
                .dispatch(&Cmd::MSGSIGNAL {}, &device, &queue, false, 0, 50, 0, &voice)
                .is_err()
        );
        assert!(owner.command_pending());
        owner.advance(Ticks::ZERO, false, false, &mut voice);
        assert!(owner.click(&mut voice));
        assert_eq!(owner.poll_command(&voice), CommandStatus::Complete);
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(frames, 90);
    println!(
        "Higurashi live message owner:90 whole RGBA frames match native update/draw packet replay at720p/1080p; MSGINIT options, MSGSET, partial per-glyph fades, clicks, MSGWAIT and animated MSGCLOSE pass"
    );
}

#[test]
#[ignore = "loads supplied newrodin and compares native packet replay on offscreen Metal"]
fn higurashi_message_glyphs_match_native_packets_at_720p_and_1080p() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let root = std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let assets = AssetServer::new(
        locate_assets_for_game(Game::Higurashi, Some(&root))
            .unwrap()
            .into(),
        AssetLoadContext {
            game: Game::Higurashi,
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let font = shin_tasks::block_on(HigurashiMessageGlyphs::load_font(&assets)).unwrap();
    let mut frames = 0;
    let capture =
        std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_MESSAGE_CAPTURES").unwrap());
    std::fs::create_dir_all(&capture).unwrap();
    for (case, (style, outline, options, text)) in [
        (0, 0, [0, 0], "名前@r@bかな.@<漢字@>、続く。"),
        (1, 1, [0, 0], "@r@c$F00.赤@c$0F0.緑@c$00F.青"),
        (2, 0, [0, 0], "@r隠れた本文"),
        (3, 1, [-1, 2], "@r@z10.小@z200.大@z.戻"),
        (
            4,
            2,
            [0, 0],
            "@r……もやがかったような意識のすぐそばで、いびき交じりの寝息が聞こえてくる。",
        ),
        (0, 0, [0, 0], "@r"),
    ]
    .into_iter()
    .enumerate()
    {
        let message = HigurashiMessageGlyphs::prepare(
            &device,
            &queue,
            font.clone(),
            style,
            options,
            50,
            outline,
            text,
        );
        let packets = native(
            &[0.0, 0.5, 1.0]
                .map(|p| row(style, outline, options, p, text))
                .concat(),
        );
        for (progress, packet) in [0.0, 0.5, 1.0].into_iter().zip(packets) {
            for size in [(1280, 720), (1920, 1080)] {
                let values = vec![progress; message.glyphs.len()];
                let output = render_frame(&device, &queue, size, |pass| {
                    message
                        .render(
                            pass,
                            &HigurashiTransform::default(),
                            7,
                            1.0,
                            1.0,
                            357.0,
                            &values,
                        )
                        .unwrap();
                });
                let expected = render_frame(&device, &queue, size, |pass| {
                    replay_native(pass, &packet, &message)
                });
                assert_eq!(
                    output, expected,
                    "message case{case} progress{progress} at{size:?}"
                );
                let visible = output
                    .chunks_exact(4)
                    .filter(|p| p[0..3] != [0, 0, 0])
                    .count();
                assert_eq!(
                    visible > 0,
                    !packet.draws.is_empty(),
                    "message visibility case{case}"
                );
                if progress == 1.0 {
                    std::fs::write(
                        capture.join(format!("case{case}-{}x{}.rgba", size.0, size.1)),
                        &output,
                    )
                    .unwrap();
                }
                frames += 1;
            }
        }
        // The older renderer suppresses text until both slide factors are one.
        for (natural, modal) in [(0.5, 1.0), (1.0, 0.5)] {
            let output = render_frame(&device, &queue, (1280, 720), |pass| {
                message
                    .render(
                        pass,
                        &HigurashiTransform::default(),
                        7,
                        natural,
                        modal,
                        357.0,
                        &vec![1.0; message.glyphs.len()],
                    )
                    .unwrap();
            });
            assert!(output.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
        }
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(frames, 36);
    println!(
        "Higurashi message GPU:36 whole RGBA frames match native packet replay at720p/1080p; five styles, ruby, colors, font scales, outline modes, progress and empty text pass"
    );
}
