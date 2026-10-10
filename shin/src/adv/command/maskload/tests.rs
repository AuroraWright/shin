use super::*;
use binrw::{BinRead, BinWrite};
use shin_core::{
    format::scenario::{instruction_elements::NumberSpec, instructions::Instruction},
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{RuntimeCommand, types::LayerProperty},
    },
};

fn command(id: i32, flags: i32, allow_animations: bool) -> RuntimeCommand {
    let mut bytes = std::io::Cursor::new(Vec::new());
    0xceu8.write_le(&mut bytes).unwrap();
    for value in [id, flags, i32::from(allow_animations)] {
        NumberSpec::constant(value).write_le(&mut bytes).unwrap();
    }
    bytes.set_position(0);
    let Instruction::Command(command) = Instruction::read_le(&mut bytes).unwrap() else {
        panic!()
    };
    command.into_runtime_form(&VmCtx::new(0, 42))
}

pub(crate) fn reference_live_maskload_policy<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut AdvState,
) {
    use crate::{layer::DrawableLayer, update::Updatable};
    use shin_core::{
        format::scenario::instruction_elements::FromNumber, primitives::update::FrameId,
        time::Tween,
    };
    let mask = server
        .load_sync::<MaskTexture>(scenario.info_tables().mask_info[1].path())
        .unwrap();
    let plane = PlaneId::new(0);
    let other = PlaneId::new(1);
    let old_flags = MaskFlags::from_number(0x40000010);
    let mut vm = VmState::for_scenario(0, 42);
    vm.layers.current_plane = plane;
    let mut frame = FrameId::default();
    for allow in [false, true] {
        for id in [
            -32768,
            -1,
            scenario.info_tables().mask_info.len() as i32,
            32767,
        ] {
            adv.plane_layer_group_mut(plane)
                .set_mask_texture(mask.clone(), old_flags);
            adv.plane_layer_group_mut(other)
                .set_mask_texture(mask.clone(), old_flags);
            adv.allow_running_animations = false;
            let start = crate::adv::command::apply_command_state_and_start(
                command(id, -1, allow),
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                },
                scenario,
                &mut vm,
                adv,
            );
            assert!(matches!(
                start,
                CommandStartResult::Continue(CommandResult::None)
            ));
            assert!(adv.allow_running_animations);
            let (texture, flags) = adv.plane_layer_group(plane).reference_mask_state();
            assert!(texture.is_none());
            assert_eq!(
                flags, old_flags,
                "Clearing a mask retains its previous render flags"
            );
            assert!(Arc::ptr_eq(
                adv.plane_layer_group(other)
                    .reference_mask_state()
                    .0
                    .unwrap(),
                &mask
            ));
            assert_eq!(vm.layers.plane_layergroups[plane].mask_id.into_i32(), id);
            assert_eq!(vm.layers.plane_layergroups[plane].mask_flags.bits(), -1);
        }
        // Replaying the retained missing ID must stay loadable in both scene
        // restoration and thumbnail construction instead of indexing the table.
        let sounds = adv.audio_manager.kira_manager().lock().num_sounds();
        assert!(
            shin_tasks::block_on(crate::adv::restoration::LoadedScene::load(
                server,
                &adv.audio_manager,
                scenario,
                &vm
            ))
            .is_ok()
        );
        assert!(
            shin_tasks::block_on(crate::adv::save_menu_preview::SaveMenuPreviewScene::load(
                server,
                &adv.audio_manager,
                scenario,
                &vm
            ))
            .is_ok()
        );
        assert_eq!(adv.audio_manager.kira_manager().lock().num_sounds(), sounds);
        adv.allow_running_animations = true;
        adv.plane_layer_group_mut(plane)
            .set_mask_texture(mask.clone(), old_flags);
        let start = crate::adv::command::apply_command_state_and_start(
            command(1, -1, allow),
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            &mut vm,
            adv,
        );
        let CommandStartResult::Yield(crate::adv::command::ExecutingCommand::MASKLOAD(mut pending)) =
            start
        else {
            panic!()
        };
        assert_eq!(adv.allow_running_animations, allow);
        // Hold the loader deterministically while exercising the real command
        // and ADV updater. No I/O thread is blocked by the test gate.
        let (release, gate) = futures::channel::oneshot::channel::<()>();
        let loaded = mask.clone();
        pending.load_task = shin_tasks::async_io::spawn(async move {
            gate.await.unwrap();
            loaded
        });
        let mut tween = adv
            .plane_layer_group_mut(plane)
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX);
        tween.fast_forward_to(0.0);
        tween.enqueue_now(60.0, Tween::linear(Ticks::from_f32(60.0)));
        drop(tween);
        frame.advance();
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(10.0),
            asset_server: server,
            pre_render: context,
        };
        assert!(
            pending
                .update(&mut update, scenario, &mut vm, adv, true)
                .is_none()
        );
        Updatable::update(adv, &mut update);
        assert_eq!(
            adv.back_layer_group.is_some(),
            !allow,
            "Native retains the first back snapshot only while animation remains suspended"
        );
        assert_eq!(
            adv.plane_layer_group(plane)
                .properties()
                .get_value(LayerProperty::TranslateX),
            if allow { 10.0 } else { 0.0 }
        );
        assert_eq!(
            adv.plane_layer_group(plane).reference_mask_state().1,
            old_flags
        );
        // The command captured plane0; changing the VM's current plane while
        // pending must not redirect installation.
        vm.layers.current_plane = other;
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pending
            .update(&mut update, scenario, &mut vm, adv, false)
            .is_none()
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(adv.allow_running_animations);
        assert_eq!(
            adv.plane_layer_group(plane).reference_mask_state().1.bits(),
            -1
        );
        assert_eq!(
            adv.plane_layer_group(other).reference_mask_state().1,
            old_flags
        );
        frame.advance();
        update.frame_id = frame;
        Updatable::update(adv, &mut update);
        assert_eq!(
            adv.plane_layer_group(plane)
                .properties()
                .get_value(LayerProperty::TranslateX),
            if allow { 20.0 } else { 10.0 }
        );
        vm.layers.current_plane = plane;
    }
    eprintln!(
        "Native MASKLOAD: absent metadata clears only the target mask, retains render flags, restores animation; gated valid loads honor both animation policies and captured plane, then resume property tweens"
    );
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_maskload_animation_metadata_clear_and_switch_trophy() {
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
        (0x1d1550, 0xcdd0),
        (0x1d1560, 0x1644b0),
        (0x1d0c68, 0x7c80),
        (0x1d0c78, 0x163b12),
    ] {
        assert_eq!(relocations.get(&address), Some(&target));
    }
    for (address, expected) in [
        (0xea230, 0x37f80101u32),
        (0xea234, 0xa942a408u32),
        (0xea244, 0x540000adu32),
        (0xea250, 0xaa1f03e0u32),
        (0xce50, 0x940374f8u32),
        (0xce74, 0xb4000d54u32),
        (0xd004, 0x39408268u32),
        (0xd008, 0x360004c8u32),
        (0xd0a4, 0x3911c91fu32),
        (0xd150, 0xc8dffd09u32),
        (0xd15c, 0x94006d81u32),
        (0xd164, 0x34000400u32),
        (0xd1ac, 0xb9401e62u32),
        (0xd1bc, 0x9401ba11u32),
        (0xd208, 0xc8dffe80u32),
        (0xd20c, 0x9401ba19u32),
        (0xd23c, 0x3911c900u32),
        (0x11584, 0x3951ca61u32),
        (0x115a8, 0x3951ca61u32),
        (0x7c50, 0xd65f03c0u32),
        (0x7c88, 0xb9401800u32),
        (0x7c8c, 0x94043b29u32),
        (0x7c90, 0x320003e0u32),
        (0x11693c, 0x90000968u32),
        (0x116940, 0xf946cd13u32),
        (0x116944, 0xaa1303e0u32),
        (0x116948, 0x940018e6u32),
        (0x116950, 0xaa1303e0u32),
        (0x116958, 0x140018e6u32),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native command instruction {address:x}"
        );
    }
}
