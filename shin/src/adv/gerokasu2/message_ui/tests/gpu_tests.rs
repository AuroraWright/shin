//! Native captured UI/matrix packets replayed on host GPU with supplied assets.
use super::*;
use crate::{
    adv::gerokasu2::{
        message::Gerokasu2Message,
        message_clock::{MessageEffects, VoiceRequest},
        message_text::Gerokasu2MessageText,
    },
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, AssetServer, locate_assets_for_game},
    },
    cli::Game,
};
use shin_core::{primitives::update::FrameId, time::Ticks, vm::command::types::MessageTextLayout};
use shin_render::{
    dynamic_buffer::DynamicBuffer,
    pipelines::PipelineStorage,
    shaders::types::{
        buffer::BytesAddress,
        texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    },
};

pub(crate) fn frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    occluded: bool,
    render: impl FnOnce(&mut RenderPass),
) -> Vec<u8> {
    let make = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Gerokasu2 message UI offscreen"),
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
    let depth_view = depth.create_view(&Default::default());
    let mut pipelines = PipelineStorage::new(device.clone(), wgpu::TextureFormat::Rgba8Unorm);
    let samplers = TextureSamplerStore::new(device);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1048576));
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = RenderPass::new(
            &mut pipelines,
            &mut dynamic,
            &samplers,
            device,
            &mut encoder,
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            Some(DepthStencilTarget { view: &depth_view }),
            None,
            "Gerokasu2 UI native packet replay",
        );
        pass.clear(
            Some(UnormColor::from_rgba(96, 136, 184, 255)),
            Some(if occluded { 10 } else { 0 }),
            Some(1.0),
        );
        render(&mut pass);
    }
    let mut upload = device.create_command_encoder(&Default::default());
    dynamic.finish(&mut upload);
    let row_bytes = (size.0 * 4 + 255) & !255;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Gerokasu2 UI readback"),
        size: u64::from(row_bytes) * u64::from(size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(size.1),
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([upload.finish(), encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let data = readback.slice(..).get_mapped_range().unwrap();
    let pixels = data
        .chunks_exact(row_bytes as usize)
        .flat_map(|r| r[..size.0 as usize * 4].iter().copied())
        .collect();
    drop(data);
    readback.unmap();
    pixels
}
struct Effects;
impl MessageEffects for Effects {
    fn voice_is_playing(&self) -> bool {
        false
    }
    fn stop_voice(&mut self) {}
    fn play_voice(&mut self, _: VoiceRequest) -> bool {
        false
    }
    fn observe_voice(&mut self, _: &str) {}
    fn completed(&mut self, _: u32) {}
}

#[test]
#[ignore = "requires supplied ROM/executable, Metal GPU and exact Loopback Audio output"]
fn supplied_message_resources_shared_font_and_composed_windows_cursors_text_match_native_packets() {
    let gpu = Offscreen::new();
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    let assets = AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, path.parent().unwrap())
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let resources = shin_tasks::block_on(MessageResources::load(&assets)).unwrap();
    assert!(Arc::ptr_eq(resources.fonts()[0], resources.fonts()[1]));
    assert_eq!(resources.textures.window.size_vec(), vec2(2048.0, 960.0));
    assert_eq!(resources.textures.key_wait.size_vec(), vec2(160.0, 60.0));
    assert_eq!(resources.textures.selection.size_vec(), vec2(1200.0, 80.0));
    let paths = assets.reference_requested_paths();
    assert!(
        paths
            .iter()
            .all(|p| p == "/newrodin-medium.fnt" || p == "/msgtex.txa")
    );
    let parent =
        shin_render::shin_orthographic_projection_matrix(-960.0, 960.0, 540.0, -540.0, -1.0, 1.0);
    let local = Mat4::from_translation(vec3(-960.0, -540.0, 0.0));
    let mut oracle = Native::new();
    let mut frames = 0;
    let mut channels = 0;
    for mode in 0..5 {
        for variant in 0..3 {
            let source = if variant == 1 {
                "@r@{A@}@kB"
            } else {
                "@r@{A@}"
            };
            let text = Gerokasu2MessageText::layout_and_prepare(
                &gpu.device,
                &gpu.queue,
                resources.fonts(),
                source,
                mode,
                MessageTextLayout::Justify,
                50,
            )
            .unwrap();
            let mut message = Gerokasu2Message::new(
                text,
                MessageParams {
                    flags: 0,
                    mode,
                    message_id: 42,
                    message_arg: 0,
                    animate_open: variant == 2,
                },
            )
            .unwrap();
            let mut frame_id = FrameId::default();
            let mut effects = Effects;
            for _ in 0..if variant == 2 { 3 } else { 100 } {
                frame_id.advance();
                message
                    .update(frame_id, Ticks::from_f32(1.0), true, &mut effects)
                    .unwrap();
            }
            if variant != 2 {
                assert_eq!(message.clock().wait_kind, if variant == 1 { 0 } else { 1 });
            }
            let settings = MessageUiSettings { window_opacity: 75 };
            let packets = oracle.capture(
                message.clock(),
                true,
                settings,
                parent,
                local,
                7,
                PassKind::Transparent,
            );
            for size in [(640, 360), (1280, 720)] {
                let actual = frame(&gpu.device, &gpu.queue, size, false, |pass| {
                    message
                        .render_layer(
                            pass,
                            &resources.textures,
                            settings,
                            parent,
                            local,
                            7,
                            PassKind::Transparent,
                        )
                        .unwrap()
                });
                let expected = frame(&gpu.device, &gpu.queue, size, false, |pass| {
                    for draw in &packets.draws {
                        draw.render(pass, &resources.textures);
                    }
                    if let Some(transform) = packets.text_transform {
                        message
                            .text()
                            .render(
                                pass,
                                RenderRequestBuilder::new()
                                    .color_blend_type(ColorBlendType::Layer1)
                                    .depth_stencil_shorthand(packets.text_stencil, true, true),
                                transform,
                            )
                            .unwrap();
                    }
                });
                assert_eq!(
                    actual, expected,
                    "composed mode{mode} variant{variant} size{size:?}"
                );
                // Native mode2 may only have an invisible blinking cursor,
                // and mode4 emits nothing until the slide completes.
                let visible = packets.text_transform.is_some()
                    || packets
                        .draws
                        .iter()
                        .any(|draw| draw.vertices[0].color.0 >> 24 != 0);
                if visible {
                    assert!(
                        actual.chunks_exact(4).any(|p| p != [96, 136, 184, 255]),
                        "visible composed frame"
                    );
                }
                channels += actual.len();
                frames += 1;
            }
            if variant == 0 {
                let pixels = frame(&gpu.device, &gpu.queue, (640, 360), true, |pass| {
                    message
                        .render_layer(
                            pass,
                            &resources.textures,
                            settings,
                            parent,
                            local,
                            7,
                            PassKind::Transparent,
                        )
                        .unwrap()
                });
                assert!(
                    pixels.chunks_exact(4).all(|p| p == [96, 136, 184, 255]),
                    "higher parent stencil clips UI and text"
                );
                frames += 1;
                channels += pixels.len();
                let pixels = frame(&gpu.device, &gpu.queue, (640, 360), false, |pass| {
                    message
                        .render_layer(
                            pass,
                            &resources.textures,
                            settings,
                            parent,
                            local,
                            7,
                            PassKind::Opaque,
                        )
                        .unwrap()
                });
                assert!(
                    pixels.chunks_exact(4).all(|p| p == [96, 136, 184, 255]),
                    "opaque pass has no portrait and emits no text/UI"
                );
                frames += 1;
                channels += pixels.len();
            }
        }
    }
    oracle.finish();
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_MESSAGE_UI_GPU modes=5 states=3 sizes=640x360,1280x720 frames={frames} channels={channels} audio=Loopback Audio shared_medium_font_resources_native_UI_matrix_replay_composed_text_stencil_pass=verified"
    );
}

#[test]
#[ignore = "requires supplied ROM/executable, Metal GPU and exact Loopback Audio output"]
fn evaluated_message_owner_commands_preserve_windows_text_and_portrait_requests() {
    use crate::adv::gerokasu2::{
        message_commands::{MessageCommandContext, MessageCommandStatus},
        message_owner::Gerokasu2MessageOwner,
    };
    use shin_core::{
        format::scenario::instruction_elements::{MessageId, NumberSpec, U8Bool},
        vm::{
            IntoRuntimeForm, VmCtx,
            command::{CompiletimeCommand as C, compiletime as c},
            gerokasu2::{Gerokasu2RuntimeCommand as G, message_face::PortraitChange},
        },
    };
    #[derive(Default)]
    struct OwnerEffects {
        completed: Vec<u32>,
    }
    impl MessageEffects for OwnerEffects {
        fn voice_is_playing(&self) -> bool {
            false
        }
        fn stop_voice(&mut self) {}
        fn play_voice(&mut self, _: VoiceRequest) -> bool {
            false
        }
        fn observe_voice(&mut self, _: &str) {}
        fn completed(&mut self, id: u32) {
            self.completed.push(id);
        }
    }
    let shared = |command: C| G::Shared(command.into_runtime_form(&VmCtx::new(0, 42)));
    let gpu = Offscreen::new();
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    let assets = AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, path.parent().unwrap())
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let mut owner =
        shin_tasks::block_on(Gerokasu2MessageOwner::load(&assets, 0, false, -1)).unwrap();
    let textures: Arc<MessageTextures> = shin_tasks::block_on(assets.load("/msgtex.txa")).unwrap();
    let parent =
        shin_render::shin_orthographic_projection_matrix(-960.0, 960.0, 540.0, -540.0, -1.0, 1.0);
    let local = Mat4::from_translation(vec3(-960.0, -540.0, 0.0));
    let mut native = Native::new();
    let mut frames = 0;
    let mut channels = 0;
    let empty = frame(&gpu.device, &gpu.queue, (640, 360), false, |pass| {
        owner
            .render(pass, parent, local, 7, PassKind::Transparent)
            .unwrap()
    });
    assert!(
        empty
            .chunks_exact(4)
            .all(|pixel| pixel == [96, 136, 184, 255])
    );
    frames += 1;
    channels += empty.len();
    let mut compare = |owner: &Gerokasu2MessageOwner| {
        let message = owner.message().unwrap();
        let packets = native.capture(
            message.clock(),
            true,
            owner.ui_settings,
            parent,
            local,
            7,
            PassKind::Transparent,
        );
        let actual = frame(&gpu.device, &gpu.queue, (640, 360), false, |pass| {
            owner
                .render(pass, parent, local, 7, PassKind::Transparent)
                .unwrap()
        });
        let expected = frame(&gpu.device, &gpu.queue, (640, 360), false, |pass| {
            for draw in &packets.draws {
                draw.render(pass, &textures);
            }
            if let Some(transform) = packets.text_transform {
                message
                    .text()
                    .render(
                        pass,
                        RenderRequestBuilder::new()
                            .color_blend_type(ColorBlendType::Layer1)
                            .depth_stencil_shorthand(packets.text_stencil, true, true),
                        transform,
                    )
                    .unwrap();
            }
        });
        assert_eq!(actual, expected, "Owner command flow frame{frames}");
        frames += 1;
        channels += actual.len();
    };
    let mut effects = OwnerEffects::default();
    let mut frame_id = FrameId::default();
    for mode in 0..5 {
        let context = MessageCommandContext {
            was_read: false,
            event_mode: -1,
            skip: false,
            message_speed: 50,
        };
        let dispatch = |owner: &mut Gerokasu2MessageOwner, command: &G, context| {
            owner
                .dispatch(command, &gpu.device, &gpu.queue, context, None, &[])
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            dispatch(&mut owner, &G::MessageInit { style: mode | 0x20 }, context).status,
            MessageCommandStatus::Complete
        );
        assert!(matches!(
            dispatch(&mut owner, &G::MessageFace { face: 13 }, context).portrait,
            PortraitChange::Unchanged
        ));
        let set = G::MessageSet {
            msg_id: MessageId(100 + mode as u32),
            auto_wait: false,
            message_arg: -1,
            text: "@rA@|@yB".to_owned(),
        };
        let result = dispatch(&mut owner, &set, context);
        assert_eq!(result.status, MessageCommandStatus::Complete);
        assert!(matches!(result.portrait, PortraitChange::Clear));
        assert_eq!(result.installed_message, Some(100 + mode as u32));
        assert_eq!(owner.message().unwrap().clock().params.message_arg, 9);
        compare(&owner);
        owner.fast_forward(&mut effects).unwrap();
        assert_eq!(owner.message().unwrap().clock().sections, 1);
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGWAIT(c::MSGWAIT {
                    signal_num: NumberSpec::constant(0)
                })),
                context
            )
            .status,
            MessageCommandStatus::Complete
        );
        assert!(owner.async_message_active());
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGCLOSE(c::MSGCLOSE {
                    wait_for_close: U8Bool(true)
                })),
                context
            )
            .status,
            MessageCommandStatus::Complete
        );
        assert!(owner.message().unwrap().clock().open);
        assert_eq!(owner.retained_text(), Some("@rA@|@yB"));
        compare(&owner);
        assert!(matches!(
            dispatch(&mut owner, &G::MessageFace { face: 17 }, context).portrait,
            PortraitChange::Clear
        ));
        assert_eq!(owner.retained_face(), 17);
        dispatch(&mut owner, &shared(C::MSGSIGNAL(c::MSGSIGNAL {})), context);
        owner.fast_forward(&mut effects).unwrap();
        compare(&owner);
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGWAIT(c::MSGWAIT {
                    signal_num: NumberSpec::constant(-1)
                })),
                context
            )
            .status,
            MessageCommandStatus::Complete
        );
        assert!(!owner.async_message_active());
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGSYNC(c::MSGSYNC {
                    voice_index: NumberSpec::constant(0),
                    sync_time: NumberSpec::constant(-1)
                })),
                context
            )
            .status,
            MessageCommandStatus::Complete
        );
        let glyphs = owner.message().unwrap().text().character_count();
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGCLOSE(c::MSGCLOSE {
                    wait_for_close: U8Bool(true)
                })),
                context
            )
            .status,
            MessageCommandStatus::Pending
        );
        assert_eq!(owner.retained_text(), None);
        assert_eq!(owner.message().unwrap().text().character_count(), glyphs);
        frame_id.advance();
        owner
            .update(frame_id, Ticks::from_f32(3.0), true, &mut effects)
            .unwrap();
        assert_eq!(owner.poll_command(None), MessageCommandStatus::Pending);
        compare(&owner);
        frame_id.advance();
        owner
            .update(frame_id, Ticks::from_f32(10.0), true, &mut effects)
            .unwrap();
        assert_eq!(owner.poll_command(None), MessageCommandStatus::Complete);
        compare(&owner);
        let fast = MessageCommandContext {
            skip: true,
            ..context
        };
        let set = G::MessageSet {
            msg_id: MessageId(200 + mode as u32),
            auto_wait: true,
            message_arg: 1,
            text: "@rReplacement".to_owned(),
        };
        assert_eq!(
            dispatch(&mut owner, &set, fast).status,
            MessageCommandStatus::Pending
        );
        assert!(!owner.async_message_active());
        owner.fast_forward(&mut effects).unwrap();
        assert_eq!(owner.poll_command(None), MessageCommandStatus::Complete);
        compare(&owner);
        assert_eq!(
            dispatch(
                &mut owner,
                &shared(C::MSGCLOSE(c::MSGCLOSE {
                    wait_for_close: U8Bool(true)
                })),
                fast
            )
            .status,
            MessageCommandStatus::Complete
        );
        compare(&owner);
    }
    assert_eq!(effects.completed.len(), 10);
    assert_eq!(effects.completed[0], 100);
    assert_eq!(effects.completed[1], 200);
    native.finish();
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    println!(
        "GEROKASU2_MESSAGE_OWNER_GPU modes=5 frames={frames} channels={channels} audio=Loopback Audio evaluated_commands_waits_close_replacement_portrait_requests_native_UI_replay=verified"
    );
}
