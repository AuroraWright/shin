use super::*;
use crate::{
    layer::{Layer, LayerGroup, user::TileLayer},
    update::AdvUpdatable,
    wiper::{AnyWiper, DefaultWiper},
};
use binrw::{BinRead, BinWrite};
use shin_core::{
    format::scenario::{instruction_elements::NumberSpec, instructions::Instruction},
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{
            RuntimeCommand,
            types::{LayerId, LayerbankId, WipeFlags},
        },
    },
};

fn command(opcode: u8, values: &[i32], params: Option<&[i32]>) -> RuntimeCommand {
    let mut bytes = std::io::Cursor::new(Vec::new());
    opcode.write_le(&mut bytes).unwrap();
    for &value in values {
        NumberSpec::constant(value).write_le(&mut bytes).unwrap();
    }
    if let Some(params) = params {
        (((1u16 << params.len()) - 1) as u8)
            .write_le(&mut bytes)
            .unwrap();
        for &value in params {
            NumberSpec::constant(value).write_le(&mut bytes).unwrap();
        }
    }
    bytes.set_position(0);
    let Instruction::Command(cmd) = Instruction::read_le(&mut bytes).unwrap() else {
        panic!()
    };
    cmd.into_runtime_form(&VmCtx::new(0, 42))
}

#[test]
fn transition_settings_decode_and_replay_every_field_without_changing_layers() {
    let mut vm = VmState::new();
    for (ty, flags, duration, params) in [
        (1, 3, 25, vec![12, 200, 4]),
        (0, 0, -10, vec![]),
        (8, 1, 60, vec![-1]),
        (-65536, 0, 30, vec![1, 2]),
        (-1, 3, 60, vec![-1]),
        (10, 0, 60, vec![1]),
        (65536, 1, -10, vec![3]),
    ] {
        let RuntimeCommand::TRANSSET(cmd) = command(0xc9, &[ty, flags, duration], Some(&params))
        else {
            panic!()
        };
        let RuntimeCommand::WIPE(wipe) =
            command(0x8e, &[ty, flags, duration.max(0)], Some(&params))
        else {
            panic!()
        };
        assert_eq!(wipe.ty, ty);
        cmd.apply_state(&mut vm);
        assert_eq!(vm.layers.transition.ty, ty);
        assert_eq!(vm.layers.transition.flags.bits(), flags);
        assert_eq!(
            vm.layers.transition.duration,
            Ticks::from_f32(duration as f32)
        );
        let expected = std::array::from_fn::<_, 8, _>(|i| params.get(i).copied().unwrap_or(0));
        let actual = vm.layers.transition.params;
        assert_eq!(
            [
                actual.0, actual.1, actual.2, actual.3, actual.4, actual.5, actual.6, actual.7
            ],
            expected
        );
        assert_eq!(vm.layers.layer_load_counter, 0);
        assert!(!vm.layers.is_page_back_started);
    }
}

#[test]
fn transition_wait_captures_plane_and_allocated_selection_and_waits_for_wrapper_cleanup() {
    let mut vm = VmState::new();
    let plane = PlaneId::new(2);
    vm.layers.current_plane = plane;
    let bank = vm
        .layers
        .layerbank_allocator
        .alloc_layerbank(plane, LayerId::new(7))
        .unwrap();
    let other = vm
        .layers
        .layerbank_allocator
        .alloc_layerbank(plane, LayerId::new(12))
        .unwrap();
    vm.layers.layer_selection.from = LayerId::new(5);
    vm.layers.layer_selection.to = LayerId::new(10);
    let targets = TransitionWaitTargets::capture(&mut vm, VLayerId::new(-5));
    assert_eq!(targets.layers.len(), 1);
    assert_eq!(targets.layers[0].layerbank, bank);
    vm.layers.current_plane = PlaneId::new(0);
    vm.layers.layer_selection.to = LayerId::new(20);
    assert_eq!(targets.plane, plane);
    assert!(!targets.is_waiting(|b| b == other));
    let mut group: LayerGroup<TileLayer> = LayerGroup::new(None);
    group.add_layer_with_wiper(
        bank,
        TileLayer::new(
            shin_core::primitives::color::FloatColor4::WHITE,
            glam::Vec4::ZERO,
        ),
        Some(AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(10.0)))),
    );
    assert!(targets.is_waiting(|b| group.has_wiper_for_layerbank(b)));
    group.fast_forward();
    assert!(
        targets.is_waiting(|b| group.has_wiper_for_layerbank(b)),
        "Native waiter tests retained wrapper identity until the next layer update"
    );
    group.remove_layer(bank, Ticks::ZERO);
    assert!(!targets.is_waiting(|b| group.has_wiper_for_layerbank(b)));
    for id in [-1, -2, -3, -4, 99] {
        let empty = TransitionWaitTargets::capture(&mut vm, VLayerId::new(id));
        assert!(empty.layers.is_empty());
        assert!(!empty.is_waiting(|_| panic!("Empty native target list must finish")));
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_transition_settings_and_waiter_instructions() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x2192c, 0x7100243fu32),
        (0x21930, 0xf900011f),
        (0x21934, 0x540030e8),
        (0x6544, 0xb9401a61),
        (0x655c, 0x94006ced),
        (0x65ec, 0xb40001a8),
        (0x676c, 0xb40003e8),
        (0x67fc, 0xf90007ff),
        (0x6808, 0x94037d16),
        (0x3b88, 0xb937da68),
        (0x8e54, 0xb977da61),
        (0x8f04, 0xb40001a8),
        (0xe5c8c, 0xb4000dc8),
        (0xe5e48, 0xc8dffd20),
        (0xe5e50, 0xc85ffd13),
        (0xe5e84, 0xc809febf),
        (0x3a84, 0xb937da68u32),
        (0x3aac, 0xb937de69),
        (0x3ac8, 0xb937e26b),
        (0x3ab4, 0xb937e66a),
        (0xc220, 0x320003e0),
        (0xc274, 0x39407008),
        (0xc29c, 0xb97dcd01),
        (0xc354, 0x320003e8),
        (0xc398, 0x9401bce6),
        (0xc39c, 0x37000140),
        (0x87dc, 0x121d7908),
        (0x8df8, 0x36100a08),
        (0x8e70, 0x940062a8),
        (0x9450, 0x37180b48),
        (0x9658, 0x9401c422),
        (0x9934, 0x37080428),
        (0x9968, 0x9401c772),
    ] {
        let word = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(word, expected, "Native{address:x}");
    }
}

pub(crate) fn reference_live_layer_transitions<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut AdvState,
) {
    use crate::adv::command::{ExecutingCommand, apply_command_state_and_start};
    use shin_core::primitives::update::FrameId;
    let mut vm = VmState::new();
    let plane = PlaneId::new(3);
    vm.layers.current_plane = plane;
    let frame = FrameId::default();
    let run = |cmd: RuntimeCommand,
               vm: &mut VmState,
               adv: &mut AdvState,
               context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
        apply_command_state_and_start(
            cmd,
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            vm,
            adv,
        )
    };
    let poll = |cmd: &mut ExecutingCommand,
                vm: &mut VmState,
                adv: &mut AdvState,
                context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
        cmd.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            vm,
            adv,
            false,
        )
    };
    let install =
        |flags: i32,
         color: i32,
         vm: &mut VmState,
         adv: &mut AdvState,
         context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
            let Yield(mut cmd) = run(
                command(0xc1, &[7, 1, flags], Some(&[color, -960, -540, 1920, 1080])),
                vm,
                adv,
                context,
            ) else {
                panic!()
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !matches!(&cmd,ExecutingCommand::LAYERLOAD(c) if c.reference_published()) {
                let _ = poll(&mut cmd, vm, adv, context);
                assert!(
                    std::time::Instant::now() < deadline,
                    "Layer publication timeout"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            cmd
        };
    let _ = install(0, 0xf900, &mut vm, adv, context);
    let bank =
        vm.layers
            .layerbank_allocator
            .layers_in_range(plane, LayerId::new(7), LayerId::new(7))[0]
            .layerbank;
    run(command(0xc9, &[0, 0, 10], Some(&[])), &mut vm, adv, context);
    let mut load = install(6, 0xf090, &mut vm, adv, context);
    assert!(adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
    assert!(adv.allow_running_animations);
    assert!(poll(&mut load, &mut vm, adv, context).is_none());
    let Yield(mut wait) = run(command(0xca, &[7], None), &mut vm, adv, context) else {
        panic!()
    };
    assert!(poll(&mut wait, &mut vm, adv, context).is_none());
    vm.layers.current_plane = PlaneId::new(0);
    adv.plane_layer_group_mut(plane).fast_forward();
    assert!(poll(&mut wait, &mut vm, adv, context).is_none());

    let mut cleanup_frame = frame;
    cleanup_frame.advance();
    adv.plane_layer_group_mut(plane)
        .update(&crate::update::AdvUpdateContext {
            frame_id: cleanup_frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
    // Both commands retain the entry plane despite the VM plane change.
    let mut update = UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::ZERO,
        asset_server: server,
        pre_render: context,
    };
    assert!(
        wait.update(&mut update, scenario, &mut vm, adv, true)
            .is_some()
    );
    assert!(
        load.update(&mut update, scenario, &mut vm, adv, true)
            .is_some()
    );
    vm.layers.current_plane = plane;
    run(command(0xc9, &[0, 3, 10], Some(&[])), &mut vm, adv, context);
    let no_wait = install(6, 0xf009, &mut vm, adv, context);
    assert!(matches!(no_wait, ExecutingCommand::LAYERLOAD(ref c) if c.reference_finished()));
    assert!(adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
    let load_counter = vm.layers.layer_load_counter;
    // Native cached assets still create a fresh wipe; no new layer load order.
    run(
        command(0xc9, &[8, 3, 12], Some(&[-1])),
        &mut vm,
        adv,
        context,
    );
    let cached = install(6, 0xf009, &mut vm, adv, context);
    assert!(matches!(cached, ExecutingCommand::LAYERLOAD(ref c) if c.reference_finished()));
    assert_eq!(vm.layers.layer_load_counter, load_counter);
    assert!(adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
    // Start a clean occupied slot, then retain its real property object.
    let _ = install(2, 0xf009, &mut vm, adv, context);
    use shin_core::time::Tween;
    use shin_core::vm::command::types::LayerProperty;
    adv.plane_layer_group_mut(plane)
        .get_layer_mut(bank)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(60.0, Tween::linear(Ticks::from_f32(60.0)));
    let _ = install(14, 0xf900, &mut vm, adv, context);
    let trace = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].0, trace[1].0);
    let mut shared_frame = cleanup_frame;
    shared_frame.advance();
    adv.plane_layer_group_mut(plane)
        .update(&crate::update::AdvUpdateContext {
            frame_id: shared_frame,
            delta_ticks: Ticks::from_f32(2.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
    let trace = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(trace[0].1, 2.0);
    assert_eq!(trace[1].1, 2.0);
    // A replacement of an already transitioning shared slot keeps all three
    // layer bodies on one property object. The scene clone preserves that
    // topology inside the copy while retaining independent scene ownership.
    let _ = install(14, 0xf090, &mut vm, adv, context);
    let trace = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(trace.len(), 3);
    assert!(
        trace
            .iter()
            .all(|&(id, value)| id == trace[0].0 && value == 2.0)
    );
    use shin_render::shaders::types::RenderClone;
    let mut clone = adv
        .plane_layer_group(plane)
        .render_clone(&mut context.render_clone_ctx());
    let cloned = clone.reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(cloned.len(), 3);
    assert!(
        cloned
            .iter()
            .all(|&(id, value)| id == cloned[0].0 && value == 2.0)
    );
    assert_ne!(cloned[0].0, trace[0].0);
    clone
        .get_layer_mut(bank)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateY)
        .fast_forward_to(17.0);
    assert!(
        clone
            .reference_slot_properties(bank, LayerProperty::TranslateY)
            .iter()
            .all(|&(_, value)| value == 17.0)
    );
    assert!(
        adv.plane_layer_group(plane)
            .reference_slot_properties(bank, LayerProperty::TranslateY)
            .iter()
            .all(|&(_, value)| value == 0.0)
    );
    shared_frame.advance();
    adv.plane_layer_group_mut(plane)
        .update(&crate::update::AdvUpdateContext {
            frame_id: shared_frame,
            delta_ticks: Ticks::from_f32(12.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
    let trace = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].1, 14.0);
    assert!(!adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
    // Ordinary copies inherit the source frame marker, even without flag8.
    // Publish a replacement after its source advanced, then visit both again
    // in that frame: neither property object may consume delta a second time.
    adv.plane_layer_group_mut(plane).clear_layers();
    let _ = install(0, 0xf900, &mut vm, adv, context);
    run(
        command(0xc9, &[0, 0, 100], Some(&[])),
        &mut vm,
        adv,
        context,
    );
    adv.plane_layer_group_mut(plane)
        .get_layer_mut(bank)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(60.0, Tween::linear(Ticks::from_f32(60.0)));
    let mut ordinary_frame = shared_frame;
    ordinary_frame.advance();
    let mut advance = |frame_id, delta_ticks, adv: &mut AdvState| {
        adv.plane_layer_group_mut(plane)
            .update(&crate::update::AdvUpdateContext {
                frame_id,
                delta_ticks,
                asset_server: server,
                device: context.device,
                queue: context.queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
    };
    advance(ordinary_frame, Ticks::from_f32(2.0), adv);
    let _ = install(6, 0xf090, &mut vm, adv, context);
    let before = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(before.len(), 2);
    assert_ne!(before[0].0, before[1].0);
    assert_eq!(before[0].1, 2.0);
    assert_eq!(before[1].1, 2.0);
    advance(ordinary_frame, Ticks::from_f32(9.0), adv);
    assert_eq!(
        adv.plane_layer_group(plane)
            .reference_slot_properties(bank, LayerProperty::TranslateX),
        before
    );
    ordinary_frame.advance();
    advance(ordinary_frame, Ticks::from_f32(3.0), adv);
    let next = adv
        .plane_layer_group(plane)
        .reference_slot_properties(bank, LayerProperty::TranslateX);
    assert_eq!(next.len(), 2);
    assert!(next.iter().all(|&(_, value)| (value - 5.0).abs() < 0.00001));
    // Rejected raw types replace both occupied and cached slots immediately,
    // including AUTO_WIPE plus the shared-properties flag.
    for (index, ty) in [-65536, -1, 10, 65536].into_iter().enumerate() {
        run(
            command(0xc9, &[ty, 0, 60], Some(&[-1])),
            &mut vm,
            adv,
            context,
        );
        let color = if index % 2 == 0 { 0xf900 } else { 0xf090 };
        for color in [color, color] {
            let loaded = install(14, color, &mut vm, adv, context);
            assert!(matches!(loaded, ExecutingCommand::LAYERLOAD(ref c) if c.reference_finished()));
            assert!(!adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
            assert_eq!(
                adv.plane_layer_group(plane)
                    .reference_slot_properties(bank, LayerProperty::TranslateX)
                    .len(),
                1
            );
        }
        assert!(matches!(
            run(command(0xcb, &[], None), &mut vm, adv, context),
            super::super::CommandStartResult::Continue(_)
        ));
        let Yield(mut wipe) = run(
            command(0x8e, &[ty, 0, 60], Some(&[-1])),
            &mut vm,
            adv,
            context,
        ) else {
            panic!()
        };
        assert!(!vm.layers.is_page_back_started);
        assert!(poll(&mut wipe, &mut vm, adv, context).is_some());
        assert!(!adv.screen_layer().is_transition_active());
        assert!(adv.allow_running_animations);
    }
    adv.plane_layer_group_mut(plane).clear_layers();
    vm.layers.is_page_back_started = true;
    let _ = install(4, 0xf999, &mut vm, adv, context);
    assert!(!adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
    adv.plane_layer_group_mut(plane).clear_layers();
    assert!(adv.allow_running_animations);
    println!(
        "Native layer commands: TRANSSET, AUTO_WIPE publication/wait, DONT_WAIT, first-poll TRANSWAIT plane snapshot, fast-forward cleanup and PAGEBACK suppression pass"
    );
}
