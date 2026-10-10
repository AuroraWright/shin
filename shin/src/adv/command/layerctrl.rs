use shin_core::time::{Easing, Tween};
use shin_render::shaders::types::RenderCloneCtx;

use super::prelude::*;
use crate::{adv::vm_state::layers::LayerOperationTargetList, layer::LayerProperties};

impl StartableCommand for command::runtime::LAYERCTRL {
    type StateInfo = LayerOperationTargetList;
    fn apply_state(&self, state: &mut VmState) -> LayerOperationTargetList {
        let layers = &mut state.layers;
        let (target_value, _time, flags, _easing_param, ..) = self.params;

        let mut affected_layers = Default::default();
        let apply = |properties: &mut crate::layer::LayerPropertiesState| {
            let value = if flags.delta() {
                properties
                    .get_property(self.property_id)
                    .wrapping_add(target_value)
            } else {
                target_value
            };
            properties.set_property(self.property_id, value);
        };

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup => {
                apply(&mut layers.root_layer_group);
            }
            VLayerIdRepr::ScreenLayer => {
                apply(&mut layers.screen_layer);
            }
            VLayerIdRepr::PageLayer => {
                apply(&mut layers.page_layer);
            }
            VLayerIdRepr::PlaneLayerGroup => {
                apply(&mut layers.plane_layergroups[layers.current_plane].properties);
            }
            VLayerIdRepr::Selected => {
                affected_layers = layers.layerbank_allocator.layers_in_range(
                    layers.current_plane,
                    layers.layer_selection.from,
                    layers.layer_selection.to,
                )
            }
            VLayerIdRepr::Layer(layer_id) => {
                affected_layers = layers.layerbank_allocator.layers_in_range(
                    layers.current_plane,
                    layer_id,
                    layer_id,
                )
            }
        }

        for &target in &affected_layers {
            apply(&mut layers.layerbanks[target.layerbank].properties);
        }

        affected_layers
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &VmState,
        affected_layers: LayerOperationTargetList,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let (target_value, duration, flags, easing_param, ..) = self.params;

        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());

        if flags.unused_1() != 0 || flags.unused_2() != 0 || flags.unused_3() != 0 {
            panic!("LAYERCTRL: unused flags are set: {:?}", flags);
        }

        if flags.ff_to_current() && flags.ff_to_target() {
            panic!("LAYERCTRL: both ff_to_current and ff_to_target flags are set");
        }

        let easing = match flags.easing() {
            0 => Easing::Linear,
            1 => Easing::SineIn,
            2 => Easing::SineOut,
            3 => Easing::SineInOut,
            4 => Easing::Jump,
            5 => Easing::Power(easing_param),
            _ => panic!("LAYERCTRL: unknown easing function: {}", flags.easing()),
        };

        let apply_to_properties = |properties: &mut LayerProperties| {
            let mut tweener = properties.property_tweener_mut(self.property_id);

            enqueue_control(&mut tweener, target_value, duration, flags, easing);
        };

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup => {
                apply_to_properties(adv_state.root_layer_group_mut().properties_mut());
            }
            VLayerIdRepr::ScreenLayer => {
                apply_to_properties(adv_state.screen_layer_mut().properties_mut());
            }
            VLayerIdRepr::PageLayer => {
                apply_to_properties(adv_state.page_layer_mut().properties_mut());
            }
            VLayerIdRepr::PlaneLayerGroup => {
                apply_to_properties(
                    adv_state
                        .plane_layer_group_mut(vm_state.layers.current_plane)
                        .properties_mut(),
                );
            }
            VLayerIdRepr::Selected | VLayerIdRepr::Layer(_) => {
                for &target in &affected_layers {
                    apply_to_properties(
                        adv_state
                            .plane_layer_group_mut(vm_state.layers.current_plane)
                            .get_layer_mut(target.layerbank)
                            .expect("BUG: layerbank not found")
                            .properties_mut(),
                    );
                }
            }
        }

        self.token.finish().into()
    }
}

fn enqueue_control(
    tweener: &mut shin_core::time::Tweener,
    target_value: i32,
    duration: Ticks,
    flags: shin_core::vm::command::types::LayerCtrlFlags,
    easing: Easing,
) {
    let from_value = tweener.target_value();
    let to_value = if flags.delta() {
        from_value + target_value as f32
    } else {
        target_value as f32
    };
    let mut duration = duration;

    if flags.ff_to_current() {
        let current = tweener.value();
        tweener.fast_forward_to(current);
    }
    if flags.ff_to_target() {
        tweener.fast_forward();
    }

    if flags.scale_time() {
        // this flag makes "duration" actually mean change rate (in value per tick)
        let change = (to_value - tweener.target_value()).abs();
        duration = if duration > Ticks::ZERO {
            Ticks::from_f32(change / duration.as_f32())
        } else {
            Ticks::ZERO
        };
    }

    tweener.enqueue_with_options(
        to_value,
        Tween { duration, easing },
        shin_core::time::TweenOptions {
            prohibit_fast_forward: flags.prohibit_fast_forward(),
            ignore_wait: flags.ignore_wait(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use binrw::{BinRead, BinWrite};
    use shin_core::vm::command::types::{LayerCtrlFlags, LayerProperty};
    use shin_core::{
        format::scenario::instruction_elements::NumberSpec,
        vm::{IntoRuntimeForm, VmCtx},
    };

    fn control(id: i32, value: i32, flags: i32, state: &mut VmState) {
        let mut encoded = std::io::Cursor::new(Vec::new());
        7u8.write_le(&mut encoded).unwrap();
        for value in [value, 0, flags] {
            NumberSpec::<i32>::constant(value)
                .write_le(&mut encoded)
                .unwrap();
        }
        encoded.set_position(0);
        let command = command::CompiletimeCommand::LAYERCTRL(command::compiletime::LAYERCTRL {
            layer_id: NumberSpec::new(NumberSpec::constant(id).into_untyped()),
            property_id: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
            params: BinRead::read_le(&mut encoded).unwrap(),
        })
        .into_runtime_form(&VmCtx::new(0, 42));
        super::super::apply_command_state(command, state);
    }

    #[test]
    fn single_logical_layer_relative_control_changes_saved_target_once() {
        let mut state = VmState::new();
        let bank = state
            .layers
            .layerbank_allocator
            .alloc_layerbank(
                state.layers.current_plane,
                shin_core::vm::command::types::LayerId::new(3),
            )
            .unwrap();
        control(3, 100, 0, &mut state);
        control(3, 25, 128, &mut state);
        assert_eq!(
            state.layers.layerbanks[bank]
                .properties
                .get_property(LayerProperty::TranslateX),
            125
        );
    }

    #[test]
    fn special_layer_properties_record_absolute_and_wrapping_relative_targets() {
        let mut state = VmState::new();
        for id in [-1, -2, -3, -4] {
            control(id, 100, 0, &mut state);
            control(id, 25, 128, &mut state);
            let properties = match id {
                -1 => &state.layers.root_layer_group,
                -2 => &state.layers.screen_layer,
                -3 => &state.layers.page_layer,
                _ => &state.layers.plane_layergroups[state.layers.current_plane].properties,
            };
            assert_eq!(properties.get_property(LayerProperty::TranslateX), 125);
            let properties = match id {
                -1 => &mut state.layers.root_layer_group,
                -2 => &mut state.layers.screen_layer,
                -3 => &mut state.layers.page_layer,
                _ => &mut state.layers.plane_layergroups[state.layers.current_plane].properties,
            };
            properties.set_property(LayerProperty::TranslateX, i32::MAX);
            control(id, 1, 128, &mut state);
        }
        assert_eq!(
            state
                .layers
                .root_layer_group
                .get_property(LayerProperty::TranslateX),
            i32::MIN
        );
        assert_eq!(
            state
                .layers
                .screen_layer
                .get_property(LayerProperty::TranslateX),
            i32::MIN
        );
        assert_eq!(
            state
                .layers
                .page_layer
                .get_property(LayerProperty::TranslateX),
            i32::MIN
        );
        assert_eq!(
            state.layers.plane_layergroups[state.layers.current_plane]
                .properties
                .get_property(LayerProperty::TranslateX),
            i32::MIN
        );
    }

    #[test]
    fn relative_queue_and_current_or_target_clears_use_final_queued_destination() {
        for flags in [128, 128 | 256, 128 | 512] {
            let mut tweener = shin_core::time::Tweener::new(0.0);
            let duration = Ticks::from_f32(10.0);
            enqueue_control(
                &mut tweener,
                100,
                duration,
                LayerCtrlFlags(0),
                Easing::Linear,
            );
            tweener.update(Ticks::from_f32(5.0));
            enqueue_control(
                &mut tweener,
                200,
                duration,
                LayerCtrlFlags(0),
                Easing::Linear,
            );
            enqueue_control(
                &mut tweener,
                300,
                duration,
                LayerCtrlFlags(0),
                Easing::Linear,
            );
            assert_eq!(tweener.value(), 50.0);
            enqueue_control(
                &mut tweener,
                25,
                duration,
                LayerCtrlFlags(flags),
                Easing::Linear,
            );
            assert_eq!(tweener.target_value(), 325.0);
            assert_eq!(tweener.value(), if flags & 512 != 0 { 300.0 } else { 50.0 });
            tweener.fast_forward();
            assert_eq!(tweener.value(), 325.0);
            assert!(tweener.is_idle());
        }
    }

    #[test]
    fn flags_protect_active_motion_ignore_wait_and_scale_from_current_after_clear() {
        let mut tweener = shin_core::time::Tweener::new(0.0);
        enqueue_control(
            &mut tweener,
            100,
            Ticks::from_f32(10.0),
            LayerCtrlFlags(4096 | 65536),
            Easing::Linear,
        );
        tweener.fast_forward_unless_prohibited();
        assert_eq!(tweener.value(), 0.0);
        assert!(!tweener.is_waiting());
        tweener.update(Ticks::from_f32(5.0));
        assert_eq!(tweener.value(), 50.0);
        enqueue_control(
            &mut tweener,
            200,
            Ticks::from_f32(10.0),
            LayerCtrlFlags(0),
            Easing::Linear,
        );
        enqueue_control(
            &mut tweener,
            25,
            Ticks::from_f32(10.0),
            LayerCtrlFlags(64 | 128 | 256),
            Easing::Linear,
        );
        // Delta resolves against queued200 before the clear, then rate10 uses live50.
        assert_eq!(tweener.target_value(), 225.0);
        tweener.update(Ticks::from_f32(1.0));
        assert_eq!(tweener.value(), 60.0);
        assert!(tweener.is_waiting());
        enqueue_control(
            &mut tweener,
            300,
            Ticks::ZERO,
            LayerCtrlFlags(64 | 256),
            Easing::Linear,
        );
        assert_eq!(tweener.value(), 300.0);
        assert!(tweener.is_idle());
    }
}
