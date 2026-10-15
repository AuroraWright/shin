use super::*;
use shin_core::{primitives::update::FrameId, time::Tween, vm::command::types::LayerProperty};

pub(crate) fn reference_saved_sentence_state(message: &MessageLayer, static_snapshot: bool) {
    assert_eq!(message.disable_voice, static_snapshot);
    assert!(message.recv_sync_is_waiting(-1));
    assert!(message.total_voices_count > 0);
    if static_snapshot {
        assert!(message.chars.iter().filter(|c| c.block_index <= message.current_block_index && c.time <= message.current_time).all(|c| c.current_progress == 1.0));
        assert!(message.wait_kind == Some(WaitKind::Last)
            || matches!(message.blocks[message.current_block_index].ty, BlockType::Sync(_)));
    } else {
        assert_eq!(message.current_time, 0.0);
        assert_eq!(message.voice_counter, 0);
        assert_eq!(message.completed_sections, 0);
        assert!(message.recv_sync_is_waiting(0));
        assert!(matches!(&message.blocks[message.voice_block_index].ty,
            BlockType::Voice(voice) if voice.filename == "10/10100035"));
        assert!(message.chars.iter().all(|c| c.current_progress == 0.0));
        assert!(message.current_block_index < message.blocks.len());
    }
}

#[test]
fn voice_sync_tracks_indices_milliseconds_and_unsigned_negative_targets() {
    let playing = AudioWaitStatus::PLAYING;
    let silent = AudioWaitStatus::empty();
    assert!(!voice_sync_waiting(0, -1, 99, playing, 0, 100));
    assert!(voice_sync_waiting(2, -1, 0, silent, 0, 0));
    assert!(voice_sync_waiting(2, 0, 1, silent, 0, 0));
    assert!(!voice_sync_waiting(2, 1, 0, playing, 0, 100));
    assert!(!voice_sync_waiting(2, 0, 0, silent, 0, 100));
    assert!(voice_sync_waiting(2, 0, 0, playing, 999, 1000));
    assert!(!voice_sync_waiting(2, 0, 0, playing, 1000, 1000));
    assert!(!voice_sync_waiting(2, 0, 0, playing, 1001, 1000));
    assert!(!voice_sync_waiting(
        2,
        0,
        0,
        playing | AudioWaitStatus::LOADING,
        0,
        0
    ));
    assert!(voice_sync_waiting(2, 0, 0, playing, 3600000, -1));
    assert!(!voice_sync_waiting(2, 0, 0, silent, 3600000, -1));
}

pub(crate) fn reference_live_msgsync<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut crate::adv::AdvState,
) {
    use crate::adv::{CommandStartResult, ExecutingCommand, UpdatableCommand};
    use shin_core::format::scenario::instruction_elements::NumberSpec;
    use shin_core::vm::{
        IntoRuntimeForm, VmCtx,
        command::{CompiletimeCommand as C, compiletime as c},
    };
    let mut vm = crate::adv::VmState::for_scenario(0, 42);
    let params = MsgsetParams {
        flags: MessageFlags::empty(),
        messagebox_type: vm.messagebox_state.msginit.messagebox_type,
        text_layout: vm.messagebox_state.msginit.text_layout,
        message_id: MessageId(1),
    };
    {
        let mut message = adv.message_layer_mut();
        message.on_msgset(context, scenario, "@v19/11900001.", params, true);
        assert_eq!(message.total_voices_count, 1);
        assert_eq!(message.voice_counter, -1);
        assert!(message.voice_sync_is_waiting(0, 0));
        message.natural_slide.set_value(1.0);
        message.settings.v2_voicevol = 100;
    }
    let mut update = crate::update::UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: FrameId::default(),
        delta_ticks: Ticks::ZERO,
        asset_server: server,
        pre_render: context,
    };
    let start = crate::adv::apply_command_state_and_start(
        C::MSGSYNC(c::MSGSYNC {
            voice_index: NumberSpec::constant(0),
            sync_time: NumberSpec::constant(-1),
        })
        .into_runtime_form(&VmCtx::new(0, 42)),
        &mut update,
        scenario,
        &mut vm,
        adv,
    );
    let CommandStartResult::Yield(ExecutingCommand::MSGSYNC(mut pending)) = start else {
        panic!()
    };
    assert!(
        pending
            .update(&mut update, scenario, &mut vm, adv, true)
            .is_none()
    );
    {
        let mut frame = FrameId::default();
        frame.advance();
        let mut message = adv.message_layer_mut();
        message.update(&crate::update::AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: update.pre_render.device,
            queue: update.pre_render.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
        });
        assert_eq!(message.voice_counter, 0);
        // Start an actual segment of the parsed voice, retaining its logical
        // index. The native playback clock includes this100ms origin.
        let voice_block = message.voice_block_index;
        message.play_voice(voice_block, 100, 1000);
        assert_eq!(message.voice_player.position_millis(), 100);
        assert!(!message.voice_sync_is_waiting(0, 100));
        assert!(message.voice_sync_is_waiting(0, 101));
        assert!(!message.voice_sync_is_waiting(0, 0));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let mut message = adv.message_layer_mut();
        message.voice_player.update(Ticks::ZERO);
        if !message
            .voice_player
            .get_wait_status()
            .contains(AudioWaitStatus::LOADING)
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        drop(message);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        adv.message_layer()
            .voice_player
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING)
    );
    assert!(adv.message_layer().voice_player.position_millis() >= 100);
    for skipping in [false, true] {
        assert!(
            pending
                .update(&mut update, scenario, &mut vm, adv, skipping)
                .is_none()
        );
    }
    adv.message_layer_mut().voice_player.stop();
    assert!(matches!(
        pending.update(&mut update, scenario, &mut vm, adv, false),
        Some(shin_core::vm::command::CommandResult::None)
    ));
    adv.message_layer_mut()
        .on_msgset(update.pre_render, scenario, "No voice", params, true);
    assert_eq!(adv.message_layer().total_voices_count, 0);
    assert!(!adv.message_layer().voice_sync_is_waiting(99, -1));
    eprintln!(
        "Native MSGSYNC: parsed voice count, future/current index, loading clock, actual100ms segment origin, unsigned negative wait, skip policy and stop completion pass on Loopback Audio"
    );
}

pub(crate) fn reference_live_msgclose<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut crate::adv::AdvState,
) {
    use crate::adv::{CommandStartResult, ExecutingCommand, UpdatableCommand};
    use shin_core::{
        format::scenario::instruction_elements::U8Bool,
        vm::{
            IntoRuntimeForm, VmCtx,
            command::{CompiletimeCommand as C, compiletime as c},
        },
    };
    let mut frame = FrameId::default();
    for wait in [false, true] {
        for skip in [false, true] {
            let mut vm = crate::adv::VmState::for_scenario(0, 42);
            vm.messagebox_state.text = Some("Close".into());
            vm.messagebox_state.messagebox_shown = true;
            {
                let mut message = adv.message_layer_mut();
                message.natural_slide =
                    SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing);
                message.modal_slide =
                    SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing);
                message.sliding_out_messageboxes = vec![SlidingOutMessagebox {
                    ty: vm.messagebox_state.msginit.messagebox_type,
                    slide_out: SlideInterpolator::new(1.0, SlideInterpolatorDirection::Decreasing),
                    height: 100.0,
                }];
            }
            let mut update = crate::update::UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let start = crate::adv::apply_command_state_and_start(
                C::MSGCLOSE(c::MSGCLOSE {
                    wait_for_close: U8Bool(wait),
                })
                .into_runtime_form(&VmCtx::new(0, 42)),
                &mut update,
                scenario,
                &mut vm,
                adv,
            );
            assert!(!vm.messagebox_state.messagebox_shown);
            assert!(vm.messagebox_state.text.is_none());
            let CommandStartResult::Yield(ExecutingCommand::MSGCLOSE(mut pending)) = start else {
                panic!()
            };
            let first = pending.update(&mut update, scenario, &mut vm, adv, skip);
            assert_eq!(first.is_none(), wait);
            assert_eq!(
                adv.message_layer().natural_slide.value(),
                if skip { 0.0 } else { 1.0 }
            );
            if !wait {
                continue;
            }
            // Even a skip-snapped natural slide must wait for a retained old
            // window. Modal hiding freezes those clocks without completing it.
            adv.message_layer_mut().modal_slide =
                SlideInterpolator::new(0.0, SlideInterpolatorDirection::Decreasing);
            frame.advance();
            let tick = |adv: &mut crate::adv::AdvState, frame, delta_ticks| {
                adv.message_layer_mut()
                    .update(&crate::update::AdvUpdateContext {
                        frame_id: frame,
                        delta_ticks,
                        asset_server: server,
                        device: update.pre_render.device,
                        queue: update.pre_render.queue,
                        are_animations_allowed: true,
                        voice_callback: None,
                        movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
                    });
            };
            tick(adv, frame, Ticks::from_f32(20.0));
            assert!(
                pending
                    .update(&mut update, scenario, &mut vm, adv, !skip)
                    .is_none()
            );
            adv.message_layer_mut().modal_slide =
                SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing);
            frame.advance();
            tick(adv, frame, Ticks::from_f32(5.0));
            assert!(
                pending
                    .update(&mut update, scenario, &mut vm, adv, !skip)
                    .is_none()
            );
            frame.advance();
            tick(adv, frame, Ticks::from_f32(5.0));
            assert!(!adv.message_layer().close_is_waiting());
            assert!(
                pending
                    .update(&mut update, scenario, &mut vm, adv, !skip)
                    .is_some()
            );
        }
    }
    eprintln!(
        "Native MSGCLOSE: first-poll skip, wait/nonwait, retained outgoing windows, modal clock suspension and delayed actual token completion pass"
    );
}

pub(crate) fn reference_live_frame_guard(
    message: &mut MessageLayer,
    context: &mut AdvUpdateContext,
) {
    message.modal_slide.set_value(0.5);
    message.natural_slide = SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing);
    message.autoplay_requested = true;
    message
        .props
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(60.0, Tween::linear(Ticks::from_f32(60.0)));
    context.frame_id = FrameId::default();
    context.delta_ticks = Ticks::from_f32(1.0);
    message.update(context);
    assert!(
        message.autoplay_requested,
        "Native initial frame zero is skipped"
    );
    assert_eq!(message.modal_slide.value(), 0.5);
    context.frame_id.advance();
    context.are_animations_allowed = false;
    message.update(context);
    assert!(!message.autoplay_requested);
    assert!((message.modal_slide.value() - 0.6).abs() < 0.00001);
    assert_eq!(message.props.get_value(LayerProperty::TranslateX), 0.0);
    message.autoplay_requested = true;
    context.delta_ticks = Ticks::from_f32(9.0);
    context.are_animations_allowed = true;
    message.update(context);
    assert!(
        message.autoplay_requested,
        "Second owner must not consume a new request"
    );
    assert!((message.modal_slide.value() - 0.6).abs() < 0.00001);
    assert_eq!(message.props.get_value(LayerProperty::TranslateX), 0.0);
    context.frame_id.advance();
    context.delta_ticks = Ticks::from_f32(4.0);
    message.update(context);
    assert!(!message.autoplay_requested);
    assert_eq!(message.modal_slide.value(), 1.0);
    assert_eq!(message.ticks_since_last_wait, Ticks::from_f32(4.0));
    assert_eq!(message.props.get_value(LayerProperty::TranslateX), 4.0);
    message.update(context);
    assert_eq!(message.ticks_since_last_wait, Ticks::from_f32(4.0));
    assert_eq!(message.props.get_value(LayerProperty::TranslateX), 4.0);
    // Native checks equality, so revisiting frame zero is a distinct update.
    context.frame_id = FrameId::default();
    context.delta_ticks = Ticks::from_f32(1.0);
    message.update(context);
    assert_eq!(message.ticks_since_last_wait, Ticks::from_f32(5.0));
    eprintln!(
        "Native message frame guard: initial zero, duplicate-owner request preservation, modal slide, animation permission and message/property clocks pass"
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_root_clone_retains_message_and_updates_both_scenes() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    for (address, target) in [
        (0x1d0378, 0x163866), // MSGCLOSE RTTI name
        (0x1d0368, 0x5880),   // MSGCLOSE update
        (0x1d0328, 0x163852), // MSGSYNC RTTI name
        (0x1d0318, 0x57c0),   // MSGSYNC update
        (0x1d7ed0, 0x1700cd), // LayerGroup RTTI name
        (0x1d7e30, 0x7a120),  // clone
        (0x1d7e18, 0x6aa40),  // guarded update
        (0x1daf30, 0x171eb1), // MessageLayer RTTI name
        (0x1dad80, 0xb00e0),  // clone retains same object
        (0x1dad68, 0x6aa40),  // guarded update
        (0x1dad70, 0xb2090),  // message update body
    ] {
        assert_eq!(
            relocations.get(&address),
            Some(&target),
            "Native relocation {address:x}"
        );
    }
    for (address, expected) in [
        (0x5234, 0x3829690a),
        (0x5258, 0x3829691f),
        (0x920, 0x38296a7f),
        (0xc6c, 0x38686a6b),
        (0xc7c, 0x382a6a6b),
        (0xc98, 0x52921e09),
        (0xc9c, 0xf8296a7f),
        (0x588c, 0x39406408),
        (0x58d0, 0x940032d8),
        (0x58e4, 0x9402aed7),
        (0x58ec, 0x39006a68),
        (0x5900, 0x9402b058),
        (0xb1440, 0x3900c01f),
        (0xb1444, 0x37000041),
        (0xb1448, 0xb900341f),
        (0xb1ab8, 0x1e202008),
        (0xb1abc, 0x54fffe8d),
        (0x57ec, 0x93407c42),
        (0x57f0, 0x9402b070),
        (0x57f4, 0x52000013),
        (0xb19bc, 0xb9409008),
        (0xb19d0, 0x54000221),
        (0xb19ec, 0x370801e0),
        (0xb1a18, 0x1a9fa7e0),
        (0xb1a4c, 0xeb48127f),
        (0xb1a50, 0x1a9f97e0),
        (0xb0edc, 0xb9409268),
        (0xb0ee4, 0xb9009268),
        (0xb0320, 0xb9009a68),
        (0xb24a4, 0xb9009a68),
        (0x11a1e0, 0x8b000100),
        (0x11a1a8, 0x54000169),
        (0x1199a8, 0xb900135f),
        (0x119c88, 0xf9000a93),
        (0xf4c28, 0xaa1f03f4),
        (0x7a134, 0x52801500u32),
        (0x7a148, 0x97fffed2),
        (0x79d24, 0xc8dffd00),
        (0x79d2c, 0xf9401509),
        (0x79d34, 0xd63f0120),
        (0xb00f0, 0xf90007e0),
        (0xb00f8, 0x91004000),
        (0xb00fc, 0x940075e9),
        (0xb0110, 0xf9000269),
        (0x6aa5c, 0x94024411),
        (0x6aa64, 0x885ffd28),
        (0x6aa70, 0x6b00011f),
        (0x6aa74, 0x540000c1),
        (0x6aa90, 0xf9400d08),
        (0x6aaa0, 0xd63f0100),
        (0xb20cc, 0x7900801f),
        (0xd038, 0xf9400668),
        (0xd054, 0xd63f0120),
        (0x11360, 0x3951ca68),
        (0x11374, 0x35ffffc9),
        (0x1157c, 0x9102a275),
        (0x11584, 0x3951ca61),
        (0x11594, 0xd63f0100),
        (0x11598, 0x9102c276),
        (0x1159c, 0xc8dffec8),
        (0x115a0, 0xb4000128),
        (0x115a8, 0x3951ca61),
        (0x115b8, 0xd63f0100),
        (0x11b04, 0x9102c008),
        (0x11b08, 0xc8dffd09),
        (0x11b0c, 0x9102a00a),
        (0x11b14, 0x9a880148),
        (0x11b20, 0xc8dffd00),
        (0x11bec, 0x94017d3d),
        (0x14ac4, 0x9102a008),
        (0x14ad8, 0x94019a02),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native instruction {address:x}",
        );
    }
}

pub(crate) fn reference_authored_truth_color_render<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    use crate::{
        adv::{AdvState, assets::AdvAssets},
        layer::Layer,
    };
    use shin_core::{
        primitives::color::UnormColor,
        vm::{
            Scripter,
            command::{CommandResult, RuntimeCommand},
        },
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let scenario = assets.scenario.clone();
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let pc = if updated { 0x85b5c1 } else { 0x85b565 };
    let mut script = Scripter::new(&scenario, 7, 42);
    script.unsafe_set_position(shin_core::format::scenario::instruction_elements::CodeAddress(pc));
    let RuntimeCommand::MSGSET(command) = script.run(CommandResult::None).unwrap() else {
        panic!("Expected authored red-truth MSGSET at {pc:x}")
    };
    let expected = "@r@|@y@c900.@[右代宮金蔵は死亡している。@k@r@|@y右代宮蔵臼は死亡している。@k@r@|@y右代宮夏妃は死亡している。@]@c.";
    assert_eq!(
        command.text, expected,
        "Exact reported authored payload/version"
    );
    let mut live = AdvState::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets,
    );
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Authored red-truth channel regression"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut draw =
        |text: &str,
         override_color: Option<UnormColor>,
         context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
            let mut message = live.message_layer_mut();
            message.on_msgset(
                context,
                &scenario,
                text,
                MsgsetParams {
                    flags: MessageFlags::empty(),
                    messagebox_type: MessageboxType::WitchSpace,
                    text_layout: MessageTextLayout::Justify,
                    message_id: command.msg_id,
                },
                false,
            );
            for _ in 0..3 {
                message.send_sync();
            }
            message.fast_forward();
            message.modal_slide.set_value(1.0);
            assert!(!message.chars.is_empty());
            assert!(message.chars.iter().all(|c| c.current_progress == 1.0));
            if let Some(color) = override_color {
                for ch in &mut message.chars {
                    ch.color_rgba = color;
                }
            } else {
                let expected_color = if text.contains("@c900.") {
                    UnormColor::RED
                } else {
                    UnormColor::BLUE
                };
                assert!(
                    message.chars.iter().all(|c| c.color_rgba == expected_color),
                    "Authored color persists across all three waits/newlines"
                );
            }
            drop(message);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Authored truth colors",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                live.message_layer_mut().render(
                    &mut pass,
                    &TransformParams::default(),
                    0,
                    PassKind::Transparent,
                );
            }
            crate::layer::user::readback_test_texture(context, &target)
        };
    let black = draw(&command.text, Some(UnormColor::BLACK), context);
    let white = draw(&command.text, Some(UnormColor::WHITE), context);
    let red = draw(&command.text, None, context);
    let blue = draw(&command.text.replace("@c900.", "@c009."), None, context);
    let mut glyph_pixels = 0;
    for i in 0..1920 * 1080usize {
        let offset = i * 4;
        for c in 0..3 {
            let expected_red = if c == 0 {
                white[offset + c]
            } else {
                black[offset + c]
            };
            let expected_blue = if c == 2 {
                white[offset + c]
            } else {
                black[offset + c]
            };
            assert!(
                (red[offset + c] as i16 - expected_red as i16).abs() <= 1,
                "Red truth pixel {i}, channel {c}: {} vs {expected_red}",
                red[offset + c]
            );
            assert!(
                (blue[offset + c] as i16 - expected_blue as i16).abs() <= 1,
                "Blue text pixel {i}, channel {c}: {} vs {expected_blue}",
                blue[offset + c]
            );
        }
        if white[offset..offset + 3] != black[offset..offset + 3] {
            glyph_pixels += 1;
        }
    }
    assert!(
        glyph_pixels > 1000,
        "Actual supplied glyphs must contribute visible pixels"
    );
    std::fs::write(
        if updated {
            "/tmp/shin-red-truth-1.03.rgba"
        } else {
            "/tmp/shin-red-truth-original.rgba"
        },
        &red,
    )
    .unwrap();
    eprintln!(
        "Authored MSGSET {pc:x}, message {:?}: reported three truths stay red across waits/newlines; {glyph_pixels} glyph pixels, full2073600 pixel/channel black-white oracle, blue counterpart passes",
        command.msg_id
    );
}

pub(crate) fn reference_live_msgset<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    use crate::adv::{
        Adv, AdvState, CommandStartResult, ExecutingCommand, UpdatableCommand, VmState,
        assets::AdvAssets,
    };
    use crate::app::AppAction;
    use binrw::BinWrite;
    use shin_core::vm::command::types::MessageboxStyle;
    use shin_core::{
        format::scenario::{
            instruction_elements::{FromNumber, NumberSpec, U8Bool},
            instructions::Instruction,
            types::U16FixupString,
        },
        vm::{
            IntoRuntimeForm, Scripter, VmCtx,
            command::{CompiletimeCommand as C, compiletime as c},
        },
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let scenario = assets.scenario.clone();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut adv = AdvState::new(manager.clone(), server.clone(), assets.clone());
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("MSGSET first and repeated polls"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let draw = |adv: &mut AdvState,
                context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
        adv.root_layer_group_mut().pre_render(
            context,
            &crate::layer::render_params::TransformParams::default(),
        );
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "MSGSET retained pixels",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            adv.render(&mut pass, false, false, false, false);
        }
        crate::layer::user::readback_test_texture(context, &target)
    };
    let fields = |message: &MessageLayer| -> Vec<u32> {
        let old = message.sliding_out_messageboxes.first();
        vec![
            message.message_flags.bits(),
            message.messagebox_type as u32,
            message.text_layout as u32,
            message.message_id.0,
            message.natural_slide.value().to_bits(),
            message.height.value().to_bits(),
            message.sliding_out_messageboxes.len() as u32,
            old.map_or(0, |v| v.ty as u32),
            old.map_or(0, |v| v.slide_out.value().to_bits()),
        ]
    };
    let mut rows = Vec::new();
    let mut actual = Vec::new();
    let mut pixel_cases = 0;
    for (style, old, quarters, request, read, readonly, event, auto) in itertools::iproduct!(
        [0, 1, 4, 5, -1, 0x26, 0x35, 0x40],
        [0, 4],
        [0, 2, 4],
        [false, true],
        [false, true],
        [false, true],
        [false, true],
        [false, true]
    ) {
        let mut vm = VmState::for_scenario(0, 42);
        vm.settings.v11_disallowskipunread = readonly;
        vm.messagebox_state.was_read = !read;
        vm.current_event = event.then_some(24);
        if read {
            vm.save_vectors.seen_messages.add(5);
        }
        {
            let mut message = adv.message_layer_mut();
            message.messagebox_type = MessageboxStyle::from_number(old).messagebox_type;
            message.natural_slide = SlideInterpolator::new(
                quarters as f32 * 0.25,
                SlideInterpolatorDirection::Increasing,
            );
            message.modal_slide =
                SlideInterpolator::new(1.0, SlideInterpolatorDirection::Increasing);
            message.height.set_target(750.0);
            message.height.set_value(750.0);
            message.sliding_out_messageboxes.clear();
        }
        let mut update = crate::update::UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: FrameId::default(),
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: context,
        };
        let raw = NumberSpec::new(NumberSpec::constant(style).into_untyped());
        let _ = crate::adv::apply_command_state_and_start(
            C::MSGINIT(c::MSGINIT {
                messagebox_style: raw,
            })
            .into_runtime_form(&VmCtx::new(0, 42)),
            &mut update,
            &scenario,
            &mut vm,
            &mut adv,
        );
        assert_eq!(
            adv.message_layer().messagebox_type as i32,
            old,
            "MSGINIT does not immediately change the window"
        );
        assert_eq!(
            adv.message_layer().natural_slide.value(),
            quarters as f32 * 0.25
        );
        let start = crate::adv::apply_command_state_and_start_with_skip(
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(5),
                auto_wait: U8Bool(auto),
                text: U16FixupString::new("@rtest"),
            })
            .into_runtime_form(&VmCtx::new(0, 42)),
            &mut update,
            &scenario,
            &mut vm,
            &mut adv,
            request && !event,
        );
        let initial = fields(&adv.message_layer());
        let blocks = adv.message_layer().blocks.len();
        assert!(blocks <= 32);
        let first = matches!(&start, CommandStartResult::Continue(_)) as u32;
        assert_eq!(vm.save_position, if event { 0 } else { 1 });
        assert_eq!(
            vm.auto_save_slot.as_ref().unwrap().entry.save_position,
            if event { 0 } else { 1 }
        );
        assert_eq!(vm.save_revision, 1);
        let render = request && read && readonly && auto && !event && old == 0 && quarters == 0;
        let before = render.then(|| draw(&mut adv, update.pre_render));
        vm.messagebox_state.msginit = MessageboxStyle::from_number(0x35);
        vm.messagebox_state.was_read = !read;
        vm.settings.v11_disallowskipunread = !readonly;
        let current = match rows.len() % 3 {
            0 => 0,
            1 => blocks,
            _ => blocks + 1,
        };
        let sections = (rows.len() % 4) * 10;
        let signal = [i32::MIN, -1, 0, 1, 2, 32, i32::MAX][rows.len() % 7];
        {
            let mut message = adv.message_layer_mut();
            message.current_block_index = current;
            message.completed_sections = sections as u32;
        }
        let waiting = adv.message_layer().recv_sync_is_waiting(signal) as u32;
        let (second, third) = match start {
            CommandStartResult::Yield(ExecutingCommand::MSGSET(mut command)) => {
                let second = command
                    .update(&mut update, &scenario, &mut vm, &mut adv, !request)
                    .is_some() as u32;
                assert_eq!(
                    fields(&adv.message_layer()),
                    initial,
                    "Waiting MSGSET must not reinstall or snap the window"
                );
                if let Some(before) = before.as_ref() {
                    assert_eq!(before, &draw(&mut adv, update.pre_render));
                    pixel_cases += 1;
                }
                let third = if second == 0 {
                    adv.message_layer_mut().current_block_index = blocks;
                    command
                        .update(&mut update, &scenario, &mut vm, &mut adv, request)
                        .is_some() as u32
                } else {
                    1
                };
                (second, third)
            }
            CommandStartResult::Continue(_) => (1, 1),
            _ => panic!("Unexpected MSGSET start"),
        };
        assert_eq!(fields(&adv.message_layer()), initial);
        assert_eq!(vm.save_revision, 1, "Repeated polls must not capture again");
        assert_eq!(vm.save_position, if event { 0 } else { 1 });
        rows.push(vec![
            style as i64,
            old as i64,
            quarters as i64,
            if request { 2 } else { 0 },
            0,
            1,
            event as i64,
            read as i64,
            readonly as i64,
            -1,
            0,
            auto as i64,
            5,
            blocks as i64,
            current as i64,
            sections as i64,
            signal as i64,
        ]);
        let mut values = vec![first, second, third, waiting];
        values.extend(initial);
        values.push(1);
        actual.push(values);
    }
    let folder = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let root = folder.parent().unwrap();
    let base = std::env::temp_dir().join(format!("shin-msgset-port-{}", std::process::id()));
    let program = base.with_extension("oracle");
    let input = base.with_extension("input");
    assert!(
        std::process::Command::new("clang")
            .args(["-O2", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-msgset.c"))
            .arg("-o")
            .arg(&program)
            .status()
            .unwrap()
            .success()
    );
    let payload = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
                + "\n"
        })
        .collect::<String>();
    std::fs::write(&input, payload).unwrap();
    let executable = folder.join(if std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some() {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    });
    let native = std::process::Command::new(&program)
        .arg(executable)
        .stdin(std::fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "Native MSGSET oracle: {}",
        String::from_utf8_lossy(&native.stderr)
    );
    let output = String::from_utf8(native.stdout).unwrap();
    let expected = output
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), actual.len());
    for ((row, actual), expected) in rows.iter().zip(&actual).zip(&expected) {
        assert_eq!(actual, expected, "Native MSGSET input{row:?}");
    }
    std::fs::remove_file(program).unwrap();
    std::fs::remove_file(input).unwrap();
    eprintln!(
        "Native MSGSET: {} actual dispatcher/install/wait cases, {pixel_cases} unchanged full-frame repeated-poll cases, one capture each, signed styles and new read flag match executed native",
        rows.len()
    );
    drop(adv);
    // Actual ADV controller/CTRL path must retain request while MSGSET replaces
    // the old read flag. Neither a stale true nor a stale false may govern it.
    let mut bytes = scenario.raw().to_vec();
    let entrypoint = bytes.len() as u32;
    bytes[0x20..0x24].copy_from_slice(&entrypoint.to_le_bytes());
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(entrypoint as u64);
    for command in [
        C::MSGINIT(c::MSGINIT {
            messagebox_style: NumberSpec::new(NumberSpec::constant(1).into_untyped()),
        }),
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(5),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("@rtest"),
        }),
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }),
    ] {
        Instruction::Command(command).write_le(&mut cursor).unwrap();
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let appended = Arc::new(Scenario::new(bytes.into()).unwrap());
    let mut synthetic = assets.clone();
    synthetic.scenario = appended.clone();
    let mut frame = FrameId::default();
    let mut root_cases = 0;
    for (control, old_read, new_read, readonly) in
        itertools::iproduct!([0, 1, 2], [false, true], [false, true], [false, true])
    {
        // Kira retires dropped scene tracks on the real audio callback.
        // Wait for that retirement rather than racing the finite track pool.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while manager.kira_manager().lock().num_sub_tracks() != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "Loopback callback did not retire scene tracks"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut live = Adv::new(
            manager.clone(),
            server.clone(),
            synthetic.clone(),
            Scripter::new(&appended, 0, 42),
        );
        {
            let (vm, adv) = live.reference_msgset_states();
            vm.messagebox_state.was_read = old_read;
            vm.messagebox_state.messagebox_shown = true;
            vm.settings.v11_disallowskipunread = readonly;
            if new_read {
                vm.save_vectors.seen_messages.add(5);
            }
            let mut message = adv.message_layer_mut();
            message.on_msgset(
                context,
                &scenario,
                "@rold",
                MsgsetParams {
                    flags: MessageFlags::empty(),
                    messagebox_type: MessageboxType::Neutral,
                    text_layout: MessageTextLayout::Justify,
                    message_id: MessageId(3),
                },
                true,
            );
            assert!(
                message.recv_sync_is_waiting(-1),
                "Old message must be waiting to exercise its cached read flag"
            );
            message.natural_slide.set_value(0.25);
        }
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        if control == 1 {
            input[AppAction::HoldSkip].is_held = true;
        }
        if control == 2 {
            input[AppAction::ToggleSkip].is_clicked = true;
        }
        frame.advance();
        live.update(
            &mut crate::update::UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        let (vm, adv) = live.reference_msgset_states();
        assert_eq!(vm.messagebox_state.was_read, new_read);
        assert_eq!(
            adv.message_layer().natural_slide.value(),
            if control != 0 && (!readonly || new_read) {
                1.0
            } else {
                0.0
            },
            "Actual ADV MSGSET old_read{old_read} new_read{new_read} readonly{readonly} control{control}"
        );
        root_cases += 1;
        let _ = draw(adv, context); // Submit actual root and retire per-case resources.
    }
    eprintln!(
        "Actual ADV MSGSET: {root_cases} no-input/CTRL/S read-to-unread and unread-to-read transitions pass on Loopback Audio"
    );
}
