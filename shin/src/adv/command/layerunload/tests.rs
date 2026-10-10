use super::*;
use crate::{layer::user::TileLayer, update::Updatable};
use shin_core::{
    format::scenario::instruction_elements::NumberSpec,
    primitives::{color::FloatColor4, update::FrameId},
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{
            CompiletimeCommand as C, compiletime as c,
            types::{LayerId, LayerType},
        },
    },
};

pub(crate) fn reference_live_delayed_layerunload<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
    scenario: &Arc<Scenario>,
    adv: &mut AdvState,
) {
    let mut vm = VmState::for_scenario(0, 42);
    let plane = PlaneId::new(0);
    let layer = LayerId::new(7);
    vm.layers.current_plane = plane;
    let bank = vm
        .layers
        .layerbank_allocator
        .alloc_layerbank(plane, layer)
        .unwrap();
    vm.layers.layerbanks[bank].layer_type = Some(LayerType::Tile);
    adv.plane_layer_group_mut(plane).add_layer(
        bank,
        TileLayer::new(
            FloatColor4::BLUE,
            glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
        )
        .into(),
    );
    let command = C::LAYERUNLOAD(c::LAYERUNLOAD {
        layer_id: NumberSpec::new(NumberSpec::constant(7).into_untyped()),
        delay_time: NumberSpec::new(NumberSpec::constant(3).into_untyped()),
    })
    .into_runtime_form(&VmCtx::new(0, 42));
    let mut frame = FrameId::default();
    let result = crate::adv::command::apply_command_state_and_start(
        command,
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
        result,
        CommandStartResult::Continue(CommandResult::None)
    ));
    assert_eq!(vm.layers.layerbanks[bank].layer_type, None);
    assert!(
        vm.layers
            .layerbank_allocator
            .layers_in_range(plane, layer, layer)
            .is_empty()
    );
    assert!(adv.plane_layer_group(plane).get_layer(bank).is_none());
    assert_eq!(
        adv.plane_layer_group(plane)
            .reference_retained_layer_count(),
        1
    );
    frame.advance();
    Updatable::update(
        adv,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        },
    );
    assert!(adv.back_layer_group.is_none());
    assert_eq!(
        adv.plane_layer_group(plane)
            .reference_retained_layer_count(),
        1
    );
    let reused = vm
        .layers
        .layerbank_allocator
        .alloc_layerbank(plane, layer)
        .unwrap();
    assert_eq!(reused, bank);
    adv.plane_layer_group_mut(plane).add_layer(
        reused,
        TileLayer::new(FloatColor4::RED, glam::vec4(-960.0, -540.0, 1920.0, 1080.0)).into(),
    );
    frame.advance();
    Updatable::update(
        adv,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(2.0),
            asset_server: server,
            pre_render: context,
        },
    );
    assert_eq!(
        adv.plane_layer_group(plane)
            .reference_retained_layer_count(),
        0
    );
    assert!(adv.plane_layer_group(plane).get_layer(reused).is_some());
    eprintln!(
        "Native LAYERUNLOAD: actual dispatcher completes immediately, frees VM/bank lookup, retains old layer for three ticks and preserves replacement bank at retirement"
    );
}
