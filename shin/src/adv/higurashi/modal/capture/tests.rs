use super::*;
use crate::adv::higurashi::{
    frame::tests::gpu::render_to_texture, message_glyphs::tests::gpu::gpu_assets,
};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    LayerFragmentShader, LayerShaderOutputKind,
    gpu_texture::GpuTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::{
    borrow::Cow,
    io::Write,
    process::{Command, Stdio},
};
use wgpu::util::DeviceExt;

fn native_packets() -> [Packet; 2] {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MODAL_CAPTURE").unwrap())
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
        .write_all(b"1 1\n1280 720\n1920 1080\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lines: Vec<_> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(lines.len(), 9);
    for case in 0..3 {
        assert_eq!(lines[case * 3], lines[0]);
        assert_eq!(lines[case * 3 + 1], lines[1]);
        assert_eq!(lines[case * 3 + 2], "E 2 1");
    }
    std::array::from_fn(|i| {
        let words: Vec<u32> = lines[i]
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse().unwrap())
            .collect();
        assert_eq!(&words[..4], &[i as u32, i as u32 + 1, 960, 540]);
        assert_eq!(words.len(), 52);
        let floats: Vec<f32> = words[4..].iter().copied().map(f32::from_bits).collect();
        Packet {
            projection: Mat4::from_cols_array(floats[..16].try_into().unwrap()),
            distances: [
                Vec4::from_slice(&floats[16..20]),
                Vec4::from_slice(&floats[20..24]),
            ],
            color: Vec4::from_slice(&floats[24..28]),
            weights: Vec4::from_slice(&floats[28..32]),
            vertices: std::array::from_fn(|v| PosTexVertex {
                position: Vec2::from_slice(&floats[32 + v * 4..34 + v * 4]),
                texture_position: Vec2::from_slice(&floats[34 + v * 4..36 + v * 4]),
            }),
        }
    })
}

fn words(p: &Packet) -> Vec<u32> {
    let mut f = p.projection.to_cols_array().to_vec();
    for v in p.distances {
        f.extend(v.to_array());
    }
    f.extend(p.color.to_array());
    f.extend(p.weights.to_array());
    for v in &p.vertices {
        let position = v.position;
        let texture_position = v.texture_position;
        f.extend(position.to_array());
        f.extend(texture_position.to_array());
    }
    f.into_iter().map(f32::to_bits).collect()
}

struct NativeShader {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl NativeShader {
    fn new(device: &wgpu::Device) -> Self {
        let dir =
            std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_NATIVE_BLUR_SHADERS").unwrap());
        let module = |stage| {
            let raw = std::fs::read(dir.join(format!("native-modal-blur.{stage}.spv"))).unwrap();
            let words = raw
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Original updated program23 GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(words)),
            })
        };
        let vertex = module("vert");
        let fragment = module("frag");
        let mut entries: Vec<_> = (0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original program23 GLSL reference"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shin_render::TEXTURE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
    }
    fn draw(
        &self,
        ctx: &mut PreRenderContext,
        source: TextureSource,
        target: TextureTarget,
        packet: &Packet,
    ) {
        // GLSL compiler adds the clip-depth bridge; original projection bytes
        // and the four original uniform blocks are supplied independently.
        let values = [
            packet.projection.to_cols_array().to_vec(),
            packet.distances.iter().flat_map(|v| v.to_array()).collect(),
            packet.color.to_array().to_vec(),
            packet.weights.to_array().to_vec(),
        ];
        let buffers: Vec<_> = values
            .iter()
            .map(|v| {
                ctx.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(v),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect();
        let vertices = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&packet.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::TextureView(source.view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::Sampler(ctx.sampler_store.get(source.sampler)),
        });
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            multiview_mask: None,
            label: Some("Original program23 GPU pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
                view: target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..4, 0..1);
    }
}

fn copy(pass: &mut RenderPass, source: TextureSource) {
    let vertices = build_quad_vertices(|uv| PosTexVertex {
        position: uv,
        texture_position: uv,
    });
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::Layer {
            output_kind: LayerShaderOutputKind::Layer,
            fragment_shader: LayerFragmentShader::Default,
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            texture: source,
            transform: crate::render::normalized_projection_matrix(),
            color_multiplier: FloatColor4::WHITE,
            fragment_shader_param: Vec4::ZERO,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

#[test]
#[ignore = "offscreen Metal comparison with original executable program23 GLSL and native capture packets"]
fn higurashi_modal_blur_pixels_match_original_glsl_and_packets() {
    let native = native_packets();
    for (a, b) in packets().iter().zip(&native) {
        assert_eq!(words(a), words(b));
    }
    let (device, queue, _) = gpu_assets();
    let shader = NativeShader::new(&device);
    let mut frames = 0;
    for size in [(1, 1), (17, 13), (1280, 720), (1920, 1080)] {
        for pattern in 0..6 {
            let pixels: Vec<u8> = (0..size.1)
                .flat_map(|y| {
                    (0..size.0).flat_map(move |x| match pattern {
                        0 => [31, 97, 213, 127],
                        1 => [
                            ((x * 17) % 256) as u8,
                            ((y * 29) % 256) as u8,
                            ((x + y) % 256) as u8,
                            255,
                        ],
                        2 => [
                            if x % 11 < 3 { 255 } else { 0 },
                            0,
                            0,
                            if x % 7 < 2 { 255 } else { 0 },
                        ],
                        3 => [0, if y % 13 < 4 { 255 } else { 0 }, 0, 255],
                        4 => {
                            [if x < 5 || y < 5 || x + 5 >= size.0 || y + 5 >= size.1 {
                                255
                            } else {
                                0
                            }; 4]
                        }
                        _ => [
                            ((x * 43 + y * 71) % 256) as u8,
                            ((x * 11 + y * 5) % 256) as u8,
                            ((x * 7 + y * 17) % 256) as u8,
                            ((x * 31 + y * 29) % 256) as u8,
                        ],
                    })
                })
                .collect();
            let texture = GpuTexture::new_static_with_data(
                &device,
                &queue,
                None,
                winit::dpi::PhysicalSize::new(size.0, size.1),
                shin_render::TEXTURE_FORMAT,
                &pixels,
            );
            for stages in [1, 2] {
                let render = |reference: bool| {
                    render_to_texture(&device, &queue, EXTENT, |ctx, view| {
                        // Source has its actual dimensions, independent of output.
                        let resize = SurfaceResizeSource::new(ViewportParams::both(
                            winit::dpi::PhysicalSize::new(size.0, size.1),
                        ));
                        let mut source = RenderTexture::new(
                            device.clone(),
                            resize.handle(),
                            "Blur fixture source extent".into(),
                        );
                        {
                            let mut pass = ctx.begin_pass(
                                source.as_texture_target(),
                                None,
                                "Blur fixture upload",
                            );
                            copy(&mut pass, texture.as_source());
                        }
                        if reference {
                            let mut first = ctx.new_render_texture("Native blur first".into());
                            shader.draw(
                                ctx,
                                source.as_texture_source(),
                                first.as_texture_target(),
                                &native[0],
                            );
                            if stages == 2 {
                                let mut second =
                                    ctx.new_render_texture("Native blur second".into());
                                shader.draw(
                                    ctx,
                                    first.as_texture_source(),
                                    second.as_texture_target(),
                                    &native[1],
                                );
                                let mut pass = ctx.begin_pass(
                                    TextureTarget {
                                        view,
                                        kind: TextureTargetKind::RenderTexture,
                                    },
                                    None,
                                    "Native blur copy",
                                );
                                copy(&mut pass, second.as_texture_source());
                            } else {
                                let mut pass = ctx.begin_pass(
                                    TextureTarget {
                                        view,
                                        kind: TextureTargetKind::RenderTexture,
                                    },
                                    None,
                                    "Native blur copy",
                                );
                                copy(&mut pass, first.as_texture_source());
                            }
                        } else if stages == 2 {
                            let output = capture(ctx, &source);
                            assert_eq!(output.dimensions(), EXTENT);
                            let mut pass = ctx.begin_pass(
                                TextureTarget {
                                    view,
                                    kind: TextureTargetKind::RenderTexture,
                                },
                                None,
                                "Production blur copy",
                            );
                            copy(&mut pass, output.as_texture_source());
                        } else {
                            let mut pass = ctx.begin_pass(
                                TextureTarget {
                                    view,
                                    kind: TextureTargetKind::RenderTexture,
                                },
                                None,
                                "Production blur first",
                            );
                            packets()[0].render(&mut pass, source.as_texture_source());
                        }
                    })
                };
                let actual = render(false);
                let expected = render(true);
                let mismatches = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
                assert_eq!(
                    mismatches,
                    0,
                    "originalGLSL mismatch size{size:?} pattern{pattern} stages{stages}; first={:?}",
                    actual
                        .iter()
                        .zip(&expected)
                        .position(|(a, b)| a != b)
                        .map(|i| (i, actual[i], expected[i]))
                );
                frames += 1;
            }
        }
    }
    println!(
        "Higurashi modal blur:3 native cases/6 exact packets;{frames} exact full960x540 RGBA frames against original GLSL on Metal; first and both horizontal passes, four source extents, six color/alpha/edge/axis patterns pass"
    );
}

fn screen_context<'a>(
    ctx: &'a mut PreRenderContext,
    resize: &'a SurfaceResizeSource,
) -> PreRenderContext<'a, 'a, 'a, 'a> {
    PreRenderContext {
        device: ctx.device,
        queue: ctx.queue,
        breakup_texture: ctx.breakup_texture,
        resize_source: resize,
        sampler_store: ctx.sampler_store,
        depth_stencil: ctx.depth_stencil,
        pipeline_storage: &mut *ctx.pipeline_storage,
        dynamic_buffer: &mut *ctx.dynamic_buffer,
        encoder: &mut *ctx.encoder,
    }
}

#[test]
#[ignore = "real cached screen/transition capture and host entry on Metal/the default audio output"]
fn higurashi_modal_capture_uses_cached_screen_and_keeps_snapshot_across_resize() {
    use crate::adv::higurashi::{
        audio::HigurashiAudio,
        host::HigurashiHost,
        layers::HigurashiLayers,
        replay::tests::scenario,
        runner::{HigurashiRunner, Policy},
        scene::HigurashiScene,
        scene_textures::HigurashiSceneTextures,
    };
    use shin_core::{
        format::scenario::{
            higurashi::HigurashiScenario, instruction_elements::UntypedNumberArray,
        },
        time::Ticks,
        vm::higurashi::command::HigurashiRuntimeCommand as C,
    };
    use std::sync::Arc;
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
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
    let load_tile = |color| C::LAYERLOAD {
        args: [0, 1, 0],
        params: (color, -640, -720, 1280, 720, 0, 0, 0),
    };
    let mut scene = HigurashiScene::new(HigurashiLayers::new(|| [1; 4]));
    shin_tasks::block_on(scene.layers_mut().unwrap().dispatch(
        &load_tile(0xff00),
        &assets,
        &authored,
        || [1; 4],
    ))
    .unwrap();
    // Red front -> blue back, retained midpoint. The cache is intentionally
    // distinct from the host runner's empty scene, proving entry consumes the
    // supplied prepared screen rather than redrawing current ADV/message.
    shin_tasks::block_on(scene.layers_mut().unwrap().dispatch(
        &C::LAYERBACK { argument: 0 },
        &assets,
        &authored,
        || [1; 4],
    ))
    .unwrap();
    shin_tasks::block_on(scene.layers_mut().unwrap().dispatch(
        &load_tile(0xf00f),
        &assets,
        &authored,
        || [1; 4],
    ))
    .unwrap();
    scene
        .dispatch(
            &C::WIPE {
                args: [0, 1, 60],
                params: UntypedNumberArray::default(),
            },
            false,
        )
        .unwrap();
    scene.advance(Ticks::from_f32(30.0), true, false, None);
    let screen_resize = SurfaceResizeSource::new(ViewportParams::both(
        winit::dpi::PhysicalSize::new(1280, 720),
    ));
    let mut retained_screens = None;
    let first = render_to_texture(&device, &queue, (1280, 720), |ctx, view| {
        let mut owned_context = screen_context(ctx, &screen_resize);
        let ctx = &mut owned_context;
        let mut screens = HigurashiSceneTextures::new(ctx);
        assert!(host.enter_modal_from_screen(ctx, &screens).is_err());
        assert_eq!(host.modal_pause_depth(), 0);
        assert!(!host.runner.frame().message.playback.hidden);
        screens.prepare_scene(ctx, &scene).unwrap();
        let pc = host.runner.position();
        host.enter_modal_from_screen(ctx, &screens).unwrap();
        assert_eq!(host.runner.position(), pc);
        assert!(host.runner.frame().message.playback.hidden);
        assert_eq!(host.modal_pause_depth(), 1);
        assert_eq!(host.modal_snapshot().unwrap().dimensions(), EXTENT);
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            None,
            "Retained blurred midpoint",
        );
        copy(
            &mut pass,
            host.modal_snapshot().unwrap().as_texture_source(),
        );
        drop(pass);
        retained_screens = Some(screens);
    });
    // Every output pixel is the midpoint, including the message region.
    for p in first.chunks_exact(4) {
        assert!(
            p[0].abs_diff(128) <= 1 && p[1] == 0 && p[2].abs_diff(128) <= 1 && p[3] == 255,
            "cached midpoint {p:?}"
        );
    }
    scene.advance(Ticks::from_f32(30.0), true, false, None);
    // Change host viewport and complete the source transition. Without cache
    // preparation, a nested entry still captures the old screen pixels.
    let mut screens = retained_screens.unwrap();
    screen_resize.resize(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let stale = render_to_texture(&device, &queue, (1920, 1080), |ctx, view| {
        host.enter_modal_from_screen(ctx, &screens).unwrap();
        assert_eq!(host.modal_pause_depth(), 2);
        assert_eq!(host.modal_snapshot().unwrap().dimensions(), EXTENT);
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            None,
            "Cached midpoint after viewport change",
        );
        copy(
            &mut pass,
            host.modal_snapshot().unwrap().as_texture_source(),
        );
    });
    for p in stale.chunks_exact(4) {
        assert!(p[0].abs_diff(128) <= 1 && p[1] == 0 && p[2].abs_diff(128) <= 1 && p[3] == 255);
    }
    let refreshed = render_to_texture(&device, &queue, (1920, 1080), |ctx, view| {
        screens
            .prepare_scene(&mut screen_context(ctx, &screen_resize), &scene)
            .unwrap();
        // Preparing the source cannot mutate a previously retained snapshot.
        assert_eq!(host.modal_snapshot().unwrap().dimensions(), EXTENT);
        let captured = screens.capture_modal(ctx).unwrap();
        assert_eq!(captured.dimensions(), EXTENT);
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            None,
            "New cached blue screen",
        );
        copy(&mut pass, captured.as_texture_source());
    });
    for p in refreshed.chunks_exact(4) {
        assert_eq!(p, &[0, 0, 255, 255]);
    }
    let retained = render_to_texture(&device, &queue, (1920, 1080), |ctx, view| {
        let mut pass = ctx.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            None,
            "Snapshot after source refresh",
        );
        copy(
            &mut pass,
            host.modal_snapshot().unwrap().as_texture_source(),
        );
    });
    assert_eq!(retained, stale);
    host.exit_modal();
    host.exit_modal();
    assert_eq!(host.modal_pause_depth(), 0);
    assert!(!host.runner.frame().message.playback.hidden);
    println!(
        "Higurashi modal cached-screen host:real Tile midpoint/endpoint, supplied prepared cache without redraw/message, failed preparation, nested entry, fixed960x540 snapshot across720p/1080p resize/source refresh and counted conceal restoration pass; the default audio output; native root cache traversal and ordinary menu attachment remain outside fixture"
    );
}
