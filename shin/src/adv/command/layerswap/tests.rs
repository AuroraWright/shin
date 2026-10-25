use super::*;
use binrw::{BinRead, BinWrite};
use shin_core::{
    format::scenario::{instruction_elements::NumberSpec, instructions::Instruction},
    primitives::{
        color::{FloatColor4, UnormColor},
        update::FrameId,
    },
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{
            RuntimeCommand,
            types::{LayerProperty, LayerType},
        },
    },
};

fn command(first: i32, second: i32) -> RuntimeCommand {
    let mut bytes = std::io::Cursor::new(Vec::new());
    0xc5u8.write_le(&mut bytes).unwrap();
    for id in [first, second] {
        NumberSpec::constant(id).write_le(&mut bytes).unwrap();
    }
    bytes.set_position(0);
    let Instruction::Command(command) = Instruction::read_le(&mut bytes).unwrap() else {
        panic!()
    };
    command.into_runtime_form(&VmCtx::new(0, 42))
}

#[test]
fn layerswap_replay_moves_mappings_and_saved_ids_without_replacing_bank_state() {
    let plane = PlaneId::new(2);
    let first = LayerId::new(0);
    let second = LayerId::new(255);
    for populated in 0..4 {
        let mut vm = VmState::for_scenario(0, 42);
        vm.layers.current_plane = plane;
        let mut originals = Vec::new();
        for (bit, layer) in [(1, first), (2, second)] {
            if populated & bit == 0 {
                continue;
            }
            let bank = vm
                .layers
                .layerbank_allocator
                .alloc_layerbank(plane, layer)
                .unwrap();
            let saved = &mut vm.layers.layerbanks[bank];
            saved.layer_id = layer;
            saved.layer_type = Some(LayerType::Tile);
            saved.params.0 = i32::from(layer.raw());
            saved.layer_load_counter = 99;
            saved.interaction_result = Some(17);
            originals.push((layer, bank, saved.clone()));
        }
        let other = PlaneId::new(1);
        let unrelated = vm
            .layers
            .layerbank_allocator
            .alloc_layerbank(other, first)
            .unwrap();
        // Warm the range cache before swapping to exercise invalidation.
        vm.layers
            .layerbank_allocator
            .layers_in_range(plane, first, second);
        super::super::apply_command_state_ref(&command(0, 255), &mut vm).unwrap();
        let targets = vm
            .layers
            .layerbank_allocator
            .layers_in_range(plane, first, second);
        assert_eq!(targets.len(), originals.len());
        for (old_id, bank, mut expected) in originals {
            let new_id = if old_id == first { second } else { first };
            assert_eq!(
                vm.layers
                    .layerbank_allocator
                    .get_layerbank_id(plane, new_id),
                Some(bank)
            );
            expected.layer_id = new_id;
            assert_eq!(
                format!("{:?}", vm.layers.layerbanks[bank]),
                format!("{expected:?}")
            );
            assert!(
                targets
                    .iter()
                    .any(|target| target.layer == new_id && target.layerbank == bank)
            );
            vm.layers.layerbank_allocator.free_layerbank(plane, new_id);
            assert_eq!(
                vm.layers.layerbank_allocator.alloc_layerbank(plane, new_id),
                Some(bank)
            );
        }
        assert_eq!(
            vm.layers.layerbank_allocator.get_layerbank_id(other, first),
            Some(unrelated)
        );
    }
}

#[test]
fn layerswap_invalid_and_equal_ids_leave_replay_state_unchanged() {
    let mut vm = VmState::for_scenario(0, 42);
    vm.layers
        .layerbank_allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(7))
        .unwrap();
    for (first, second) in [
        (-1, 7),
        (7, -1),
        (256, 7),
        (7, 256),
        (i32::MIN, 7),
        (7, i32::MAX),
        (7, 7),
    ] {
        let before = format!("{:?}", vm.layers);
        // Extreme i32 values can come from VM registers, but do not fit the
        // instruction's compact literal encoding.
        let RuntimeCommand::LAYERSWAP(mut swap) = command(0, 1) else {
            panic!()
        };
        swap.arg1 = first;
        swap.arg2 = second;
        super::super::apply_command_state_ref(&RuntimeCommand::LAYERSWAP(swap), &mut vm).unwrap();
        assert_eq!(format!("{:?}", vm.layers), before);
    }
}

fn pixel(
    context: &mut crate::render::PreRenderContext,
    group: &mut crate::layer::LayerGroup,
) -> [u8; 4] {
    use crate::layer::{Layer, render_layer_without_bg, render_params::TransformParams};
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("LAYERSWAP draw order"),
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
    let view = texture.create_view(&Default::default());
    group.pre_render(context, &TransformParams::default());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "LAYERSWAP draw order",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        render_layer_without_bg(&mut pass, &TransformParams::default(), group, 0);
    }
    let bytes = crate::layer::user::readback_test_texture(context, &texture);
    bytes[(540 * 1920 + 960) * 4..(540 * 1920 + 960) * 4 + 4]
        .try_into()
        .unwrap()
}

pub(crate) fn reference_live_layerswap<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut AdvState,
) {
    use crate::layer::user::TileLayer;
    let plane = PlaneId::new(0);
    for allowed in [false, true] {
        adv.back_layer_group = None;
        adv.allow_running_animations = allowed;
        adv.plane_layer_group_mut(plane).clear_layers();
        adv.plane_layer_group_mut(plane).clear_mask_texture();
        let mut vm = VmState::for_scenario(0, 42);
        let ids = [LayerId::new(7), LayerId::new(200)];
        let mut banks = Vec::new();
        let mut owners = Vec::new();
        for (id, color) in ids.into_iter().zip([FloatColor4::RED, FloatColor4::BLUE]) {
            let bank = vm
                .layers
                .layerbank_allocator
                .alloc_layerbank(plane, id)
                .unwrap();
            vm.layers.layerbanks[bank].layer_id = id;
            let mut tile = TileLayer::new(color, glam::vec4(-960.0, -540.0, 1920.0, 1080.0));
            tile.properties_mut().set_layer_id(id);
            owners.push(tile.properties().identity());
            banks.push(bank);
            adv.plane_layer_group_mut(plane)
                .add_layer(bank, tile.into());
        }
        assert_eq!(
            pixel(context, adv.plane_layer_group_mut(plane)),
            [0, 0, 255, 255]
        );
        let sounds = adv.audio_manager.kira_manager().lock().main_track()
            .num_sounds();
        // Invalid runtime commands still retain the first back scene.
        let result = super::super::apply_command_state_and_start(
            command(-1, 200),
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: FrameId::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            &mut vm,
            adv,
        );
        assert!(matches!(
            result,
            CommandStartResult::Continue(CommandResult::None)
        ));
        assert!(adv.back_layer_group.is_some());
        let RuntimeCommand::LAYERSWAP(swap) = command(7, 200) else {
            panic!()
        };
        let info = swap.apply_state(&mut vm);
        vm.layers.current_plane = PlaneId::new(1);
        let result = swap.start(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: FrameId::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            &vm,
            info,
            adv,
        );
        assert!(matches!(
            result,
            CommandStartResult::Continue(CommandResult::None)
        ));
        for (index, bank) in banks.iter().enumerate() {
            let layer = adv.plane_layer_group(plane).get_layer(*bank).unwrap();
            let props = layer.properties();
            assert_eq!(props.get_layer_id(), ids[1 - index]);
            assert_eq!(props.identity(), owners[index]);
            assert_eq!(
                adv.back_layer_group
                    .as_ref()
                    .unwrap()
                    .screen_layer()
                    .page_layer()
                    .get_plane(plane)
                    .get_layer(*bank)
                    .unwrap()
                    .properties()
                    .get_layer_id(),
                ids[index]
            );
        }
        assert_eq!(
            pixel(context, adv.plane_layer_group_mut(plane)),
            [255, 0, 0, 255]
        );
        adv.plane_layer_group_mut(plane)
            .get_layer_mut(banks[0])
            .unwrap()
            .properties_mut()
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(2000.0);
        assert_eq!(
            pixel(context, adv.plane_layer_group_mut(plane)),
            [0, 0, 255, 255]
        );
        adv.plane_layer_group_mut(plane)
            .get_layer_mut(banks[0])
            .unwrap()
            .properties_mut()
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(1000.0);
        assert_eq!(
            pixel(
                context,
                adv.back_layer_group
                    .as_mut()
                    .unwrap()
                    .screen_layer_mut()
                    .page_layer_mut()
                    .get_plane_mut(plane)
            ),
            [0, 0, 255, 255]
        );
        vm.layers.current_plane = plane;
        let result = super::super::apply_command_state_and_start(
            command(200, 255),
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: FrameId::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            scenario,
            &mut vm,
            adv,
        );
        assert!(matches!(
            result,
            CommandStartResult::Continue(CommandResult::None)
        ));
        assert_eq!(
            vm.layers
                .layerbank_allocator
                .get_layerbank_id(plane, LayerId::new(255)),
            Some(banks[0])
        );
        assert_eq!(
            adv.plane_layer_group(plane)
                .get_layer(banks[0])
                .unwrap()
                .properties()
                .get_layer_id(),
            LayerId::new(255)
        );
        assert_eq!(adv.plane_layer_group(plane).get_used_layerbank_ids(), banks);
        assert_eq!(adv.allow_running_animations, allowed);
        assert_eq!(adv.audio_manager.kira_manager().lock().main_track()
                .num_sounds(), sounds);
    }
    adv.back_layer_group = None;
    adv.allow_running_animations = true;
    adv.plane_layer_group_mut(plane).clear_layers();
    eprintln!(
        "Native LAYERSWAP: mappings, captured plane, fixed bank/property owners, first back root, empty target and red/blue draw-order pixels pass on the default audio output"
    );
}
