use super::*;
use binrw::BinRead;
use shin_core::{
    format::scenario::{instruction_elements::FromNumber, instructions::Instruction},
    primitives::update::FrameId,
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{RuntimeCommand, types::MaskFlags},
    },
};

fn command() -> RuntimeCommand {
    let Instruction::Command(command) =
        Instruction::read_le(&mut std::io::Cursor::new([0xcf])).unwrap()
    else {
        panic!()
    };
    command.into_runtime_form(&VmCtx::new(0, 42))
}

#[test]
fn maskunload_replay_clears_only_selected_mask_id_and_preserves_flags() {
    let mut vm = VmState::for_scenario(0, 42);
    let plane = PlaneId::new(2);
    let other = PlaneId::new(1);
    vm.layers.current_plane = plane;
    for target in [plane, other] {
        vm.layers.plane_layergroups[target].mask_id = MaskIdOpt::from_number(7);
        vm.layers.plane_layergroups[target].mask_flags = MaskFlags::from_number(0x40000010);
    }
    super::super::apply_command_state_ref(&command(), &mut vm).unwrap();
    assert_eq!(
        vm.layers.plane_layergroups[plane].mask_id,
        MaskIdOpt::none()
    );
    assert_eq!(vm.layers.plane_layergroups[other].mask_id.into_i32(), 7);
    for target in [plane, other] {
        assert_eq!(
            vm.layers.plane_layergroups[target].mask_flags.bits(),
            0x40000010
        );
    }
}

pub(crate) fn reference_live_maskunload<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut AdvState,
) {
    use crate::asset::mask::MaskTexture;
    let mask = server
        .load_sync::<MaskTexture>(scenario.info_tables().mask_info[1].path())
        .unwrap();
    let plane = PlaneId::new(0);
    let other = PlaneId::new(1);
    let flags = MaskFlags::from_number(0x40000010);
    for allowed in [false, true] {
        adv.back_layer_group = None;
        adv.allow_running_animations = allowed;
        for target in [plane, other] {
            adv.plane_layer_group_mut(target)
                .set_mask_texture(mask.clone(), flags);
        }
        let mut vm = VmState::for_scenario(0, 42);
        vm.layers.current_plane = plane;
        vm.layers.plane_layergroups[plane].mask_id = MaskIdOpt::from_number(1);
        vm.layers.plane_layergroups[plane].mask_flags = flags;
        // Exercise split state/start capture, then the real dispatcher below.
        let RuntimeCommand::MASKUNLOAD(unload) = command() else {
            panic!()
        };
        let captured_plane = unload.apply_state(&mut vm);
        vm.layers.current_plane = other;
        let sounds = adv.audio_manager.kira_manager().lock().num_sounds();
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: FrameId::default(),
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: context,
        };
        let result = unload.start(&mut update, scenario, &vm, captured_plane, adv);
        assert!(matches!(
            result,
            CommandStartResult::Continue(CommandResult::None)
        ));
        assert_eq!(adv.allow_running_animations, allowed);
        assert!(
            adv.plane_layer_group(plane)
                .reference_mask_state()
                .0
                .is_none()
        );
        assert_eq!(adv.plane_layer_group(plane).reference_mask_state().1, flags);
        assert!(Arc::ptr_eq(
            adv.plane_layer_group(other)
                .reference_mask_state()
                .0
                .unwrap(),
            &mask
        ));
        let retained = adv.back_layer_group.as_ref().unwrap();
        for target in [plane, other] {
            let (texture, retained_flags) = retained
                .screen_layer()
                .page_layer()
                .get_plane(target)
                .reference_mask_state();
            assert!(Arc::ptr_eq(texture.unwrap(), &mask));
            assert_eq!(retained_flags, flags);
        }
        // Mutate the front after the first capture, then unload again. A second
        // root capture would erase the original mask from the retained scene.
        adv.plane_layer_group_mut(plane)
            .set_mask_texture(mask.clone(), MaskFlags::empty());
        vm.layers.current_plane = plane;
        let result = super::super::apply_command_state_and_start(
            command(),
            &mut update,
            scenario,
            &mut vm,
            adv,
        );
        assert!(matches!(
            result,
            CommandStartResult::Continue(CommandResult::None)
        ));
        assert!(
            adv.plane_layer_group(plane)
                .reference_mask_state()
                .0
                .is_none()
        );
        assert_eq!(
            adv.plane_layer_group(plane).reference_mask_state().1,
            MaskFlags::empty()
        );
        let (texture, retained_flags) = adv
            .back_layer_group
            .as_ref()
            .unwrap()
            .screen_layer()
            .page_layer()
            .get_plane(plane)
            .reference_mask_state();
        assert!(Arc::ptr_eq(texture.unwrap(), &mask));
        assert_eq!(retained_flags, flags);
        assert_eq!(
            vm.layers.plane_layergroups[plane].mask_id,
            MaskIdOpt::none()
        );
        assert_eq!(vm.layers.plane_layergroups[plane].mask_flags, flags);
        assert_eq!(adv.allow_running_animations, allowed);
        assert_eq!(adv.audio_manager.kira_manager().lock().num_sounds(), sounds);
    }
    adv.back_layer_group = None;
    adv.allow_running_animations = true;
    eprintln!(
        "Native MASKUNLOAD: captured plane, selected mask removal, preserved flags/animation permission and retained first back scene pass on Loopback Audio"
    );
}
