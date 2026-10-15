use super::*;
use crate::adv::higurashi::{
    message_ui::{HigurashiMessageTextures, tests::gpu::replay_native_owner},
    scene_textures::HigurashiSceneTextures,
};
use shin_core::{format::scenario::instruction_elements::MessageId, primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

pub(in crate::adv::higurashi) fn render_to_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    render: impl FnOnce(&mut crate::render::PreRenderContext, &wgpu::TextureView),
) -> Vec<u8> {
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{DepthStencilTarget, TextureSamplerStore},
        },
    };
    let samplers = TextureSamplerStore::new(device);
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        size.0, size.1,
    )));
    let make = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
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
    render(&mut context, &view);
    crate::layer::user::readback_test_texture(&mut context, &target)
}

fn native_plan(message_priority: i32) -> [Draw; 4] {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_FRAME").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "C -3 5 {message_priority} 0 100"
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let words: Vec<u32> = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|w| w.parse().unwrap())
        .collect();
    let words = &words[words.len() - 12..];
    std::array::from_fn(|i| Draw {
        child: match words[i * 3] {
            1 => Child::Message,
            2 => Child::Screen,
            _ => panic!("child"),
        },
        stencil: words[i * 3 + 1] as u8,
        pass: match words[i * 3 + 2] {
            1 => PassKind::Opaque,
            0 => PassKind::Transparent,
            _ => panic!("pass"),
        },
    })
}

#[test]
#[ignore = "composes scene/message against actual native traversal and UI/glyph packet replay at720p/1080p"]
fn higurashi_frame_composes_message_and_screen_at_both_viewports() {
    let (device, queue, assets) = gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let textures = shin_tasks::block_on(HigurashiMessageTextures::load(&assets)).unwrap();
    let capture = std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_FRAME_CAPTURES").unwrap());
    std::fs::create_dir_all(&capture).unwrap();
    let text = "名前@r@bかな.@<漢字@>、続く。";
    let mut voice = Voice;
    let mut count = 0;
    let priorities = [
        (900, native_plan(900)),
        (1000, native_plan(1000)),
        (1100, native_plan(1100)),
    ];
    for style in 0..5 {
        for (priority, plan) in priorities {
            let mut baseline: [Option<Vec<u8>>; 2] = [None, None];
            for variant in 0..3 {
                let mut frame = HigurashiFrame::new(
                    HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4])),
                    shin_tasks::block_on(HigurashiMessageOwner::load(&assets)).unwrap(),
                );
                let mut dispatch = |command, frame: &mut HigurashiFrame| {
                    shin_tasks::block_on(frame.dispatch(
                        &command,
                        &assets,
                        &scenario,
                        || [1, 2, 3, 4],
                        &device,
                        &queue,
                        false,
                        0,
                        50,
                        0,
                        &voice,
                    ))
                    .unwrap()
                };
                let control = |id, property, value| HigurashiRuntimeCommand::LAYERCTRL {
                    args: [id, property],
                    params: (value, 0, 0x100, 0, 0, 0, 0, 0),
                };
                dispatch(
                    HigurashiRuntimeCommand::LAYERLOAD {
                        args: [1, 1, 0],
                        params: (0xf68b, -640, -720, 1280, 720, 0, 0, 0),
                    },
                    &mut frame,
                );
                dispatch(
                    HigurashiRuntimeCommand::MSGINIT {
                        style,
                        updated_options: Some([0, 0]),
                    },
                    &mut frame,
                );
                dispatch(
                    HigurashiRuntimeCommand::MSGSET {
                        msg_id: MessageId(123),
                        auto_wait: 0,
                        argument: -1,
                        text: text.into(),
                    },
                    &mut frame,
                );
                dispatch(control(-3, 5, priority), &mut frame);
                // Native message84f6c does not use property19's visibility gate.
                dispatch(control(-3, 19, 0), &mut frame);
                if variant == 1 {
                    dispatch(control(-1, 0, 320), &mut frame);
                }
                if variant == 2 {
                    dispatch(control(-1, 19, 0), &mut frame);
                }
                drop(dispatch);
                frame.message.fast_forward(&mut voice);
                frame.message.playback.wait = Some(WaitKind::Last);
                frame.message.playback.ticks_since_wait = 24.0;
                assert_eq!(frame.render_plan(), plan);
                for (output, size) in [(1280, 720), (1920, 1080)].into_iter().enumerate() {
                    let render = |reference: bool| {
                        render_to_texture(&device, &queue, size, |context, view| {
                            let mut screens = HigurashiSceneTextures::new(context);
                            screens.prepare_scene(context, &frame.scene).unwrap();
                            let mut pass = context.begin_pass(
                                TextureTarget {
                                    view,
                                    kind: TextureTargetKind::RenderTexture,
                                },
                                Some(context.depth_stencil),
                                "Higurashi ADV sibling composition",
                            );
                            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                            if reference {
                                for draw in plan {
                                    match (draw.child, draw.pass) {
                                        (Child::Screen, PassKind::Opaque) => screens.render_screen(
                                            &mut pass,
                                            &frame.scene,
                                            draw.stencil,
                                        ),
                                        (Child::Message, PassKind::Transparent) => {
                                            replay_native_owner(
                                                &mut pass,
                                                &frame.message,
                                                text,
                                                &textures,
                                                draw.stencil,
                                            )
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                screens.render_frame(&mut pass, &frame).unwrap();
                            }
                        })
                    };
                    let actual = render(false);
                    let expected = render(true);
                    assert_eq!(
                        actual, expected,
                        "style{style} priority{priority} variant{variant} {size:?}"
                    );
                    // A full-canvas opaque tile makes movement/hide observable.
                    // The first pixel lies outside all glyph/cursor geometry.
                    if variant == 0 {
                        assert_ne!(&actual[..4], &[0, 0, 0, 255], "screen must be visible");
                        baseline[output] = Some(actual.clone());
                    } else {
                        assert_eq!(
                            &actual[..4],
                            &[0, 0, 0, 255],
                            "moved/hidden screen left edge"
                        );
                        assert_ne!(
                            &actual,
                            baseline[output].as_ref().unwrap(),
                            "screen change produced no pixels"
                        );
                    }
                    if style != 2 && priority <= 1000 {
                        assert!(
                            actual.chunks_exact(4).any(|p| p != [0, 0, 0, 255]),
                            "empty composition"
                        );
                    }
                    std::fs::write(
                        capture.join(format!(
                            "style{style}-priority{priority}-variant{variant}-{}x{}.rgba",
                            size.0, size.1
                        )),
                        actual,
                    )
                    .unwrap();
                    count += 1;
                }
            }
        }
    }
    println!(
        "Higurashi ADV composition: {count} whole frames match native outer traversal and UI/glyph packet replay at720p/1080p; screen transforms/hide leave message independent"
    );
}
