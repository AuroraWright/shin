use std::fmt::{Debug, Formatter};

use shin_core::vm::command::types::{
    LAYERBANKS_COUNT, LayerId, LayerLoadFlags, LayerType, PlaneId, WipeFlags,
};
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
use shin_tasks::AsyncTask;
use tracing::error;

use super::prelude::*;
use crate::{
    adv::vm_state::layers::LayerOperationTarget,
    layer::{
        LayerProperties,
        user::{PreparedUserLayer, UserLayer},
    },
};

pub struct LAYERLOAD {
    token: Option<command::token::LAYERLOAD>,
    state_info: LayerLoadStateInfo,
    load_task: Option<AsyncTask<PreparedUserLayer>>,
    resource_free: Option<(
        LayerType,
        shin_core::format::scenario::instruction_elements::UntypedNumberArray,
    )>,
    load_wait_attempted: bool,
    wipe_wait_attempted: bool,
    loaded_kind: LayerType,
    loaded_id: i32,
    share_properties: bool,
    wipe_task: Option<AsyncTask<Option<crate::wiper::AnyWiper>>>,
    pending_transition: Option<crate::adv::vm_state::layers::LayerTransitionSettings>,
    wipe_flags: WipeFlags,
    published: bool,
    ready_wiper: Option<crate::wiper::AnyWiper>,
    ready_layer: Option<PreparedUserLayer>,
}

struct LayerInfo {
    operation_target: LayerOperationTarget,
    // yes, this is duplicated with the field above
    // don't ask me why
    layer_id_for_properties: LayerId,
    layer_load_counter1: u32,
    keep_old_props: bool,
    already_the_same: bool,
}

pub struct LayerLoadStateInfo {
    skip_layerloader_creation: bool,
    plane: PlaneId,
    affected_layers: heapless::Vec<LayerInfo, { LAYERBANKS_COUNT }>,
}

impl StartableCommand for command::runtime::LAYERLOAD {
    type StateInfo = LayerLoadStateInfo;
    fn apply_state(&self, state: &mut VmState) -> LayerLoadStateInfo {
        let layers = &mut state.layers;

        let plane = layers.current_plane;

        let mut state_info = LayerLoadStateInfo {
            skip_layerloader_creation: true,
            plane,
            affected_layers: heapless::Vec::new(),
        };

        let mut add_layer = |layer: LayerId| {
            let Some(layerbank) = layers.layerbank_allocator.alloc_layerbank(plane, layer) else {
                error!("Layerbank allocation failed");
                return;
            };

            let Ok(()) = state_info.affected_layers.push(LayerInfo {
                operation_target: LayerOperationTarget { layerbank, layer },
                // these will be set later
                layer_id_for_properties: layer,
                layer_load_counter1: 0,
                keep_old_props: false,
                already_the_same: false,
            }) else {
                unreachable!()
            };
        };

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup
            | VLayerIdRepr::ScreenLayer
            | VLayerIdRepr::PageLayer
            | VLayerIdRepr::PlaneLayerGroup => {
                warn!(
                    "Attempt to load a special layer ({:?})",
                    self.layer_id.repr()
                );
                return state_info;
            }
            VLayerIdRepr::Selected => {
                for layer in layers.layer_selection.iter() {
                    add_layer(layer);
                }
            }
            VLayerIdRepr::Layer(layer) => {
                add_layer(layer);
            }
        }

        for info in &mut state_info.affected_layers {
            let state = &mut layers.layerbanks[info.operation_target.layerbank];

            let old_layer_type = state.layer_type;

            if state.layer_type == Some(self.layer_type) && state.params == self.params {
                info.already_the_same = true;
            }
            state.layer_type = Some(self.layer_type);
            state.params = self.params;
            if self.layer_type == LayerType::Quiz {
                state.is_interation_completed = false;
                state.interaction_result = None;
            }
            if !info.already_the_same {
                state.layer_load_counter = layers.layer_load_counter;
                layers.layer_load_counter = layers.layer_load_counter.wrapping_add(1);
                state_info.skip_layerloader_creation = false;
            }
            info.layer_load_counter1 = state.layer_load_counter;
            if old_layer_type.is_some()
                && self
                    .flags
                    .contains(LayerLoadFlags::KEEP_PREVIOUS_PROPERTIES)
            {
                info.keep_old_props = true;
            } else {
                state.properties.init();
                state.layer_id = info.operation_target.layer;
                info.layer_id_for_properties = info.operation_target.layer;
                info.keep_old_props = false;
                state.plane = plane;
                layers.layer_load_with_init_counter += 1;
            }
        }

        state_info
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        state_info: LayerLoadStateInfo,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let asset_server = context.asset_server.clone();
        let audio_manager = adv_state.audio_manager.clone();
        let movie_volume_settings = (&vm_state.settings).into();
        let scenario = scenario.clone();

        let device = context.pre_render.device.clone();
        let resource_free = (!state_info.skip_layerloader_creation
            && UserLayer::is_resource_free(self.layer_type))
        .then_some((self.layer_type, self.params));
        let load_task =
            (!state_info.skip_layerloader_creation && resource_free.is_none()).then(|| {
                shin_tasks::async_io::spawn(async move {
                    UserLayer::prepare(
                        &device,
                        &asset_server,
                        &audio_manager,
                        &scenario,
                        movie_volume_settings,
                        self.layer_type,
                        self.params,
                    )
                    .await
                })
            });

        if !state_info.skip_layerloader_creation
            && !self.flags.contains(LayerLoadFlags::DONT_BLOCK_ANIMATIONS)
        {
            adv_state.allow_running_animations = false;
        }

        let transition = vm_state.layers.transition;
        // Native87d0 clears AUTO_WIPE while PAGEBACK is active.
        let pending_transition = (self.flags.contains(LayerLoadFlags::AUTO_WIPE)
            && !vm_state.layers.is_page_back_started
            && transition.duration >= Ticks::from_f32(1.0))
        .then_some(transition);

        Yield(
            LAYERLOAD {
                token: Some(self.token),
                state_info,
                load_task,
                resource_free,
                load_wait_attempted: false,
                wipe_wait_attempted: false,
                loaded_kind: self.layer_type,
                loaded_id: self.params.0,
                share_properties: self.flags.contains(LayerLoadFlags::SHARE_PROPS_DURING_WIPE),
                wipe_task: None,
                pending_transition,
                wipe_flags: transition.flags,
                published: false,
                ready_wiper: None,
                ready_layer: None,
            }
            .into(),
        )
    }
}

#[cfg(test)]
impl LAYERLOAD {
    pub(super) fn reference_hold_loading(&mut self) -> futures::channel::oneshot::Sender<()> {
        let task = self
            .load_task
            .take()
            .expect("Asset-backed pending layer loader");
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.load_task = Some(shin_tasks::async_io::spawn(async move {
            receiver
                .await
                .expect("Publication gate released or task canceled");
            task.await
        }));
        sender
    }
}

impl UpdatableCommand for LAYERLOAD {
    fn update(
        &mut self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.published {
            return self.finish_if_wipes_complete(adv_state);
        }
        if let Some(task) = self.load_task.as_mut() {
            self.ready_layer = Some(if std::mem::replace(&mut self.load_wait_attempted, true) {
                task.poll_naive()?
            } else {
                task.poll_with_timeout(context.loader_wait_budget())?
            });
            self.load_task = None;
            // Native8ddc releases layer-loader animation suspension before
            // applying the transition-loader's independent animation policy.
            adv_state.allow_running_animations = true;
        }
        if self.resource_free.is_some() && !self.load_wait_attempted {
            self.load_wait_attempted = true;
            adv_state.allow_running_animations = true;
        }
        if let Some(transition) = self.pending_transition.take()
            && (0..=9).contains(&transition.ty)
        {
            let asset_server = context.asset_server.clone();
            let scenario = scenario.clone();
            self.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                crate::wiper::AnyWiper::load_raw(
                    &asset_server,
                    &scenario,
                    transition.ty,
                    transition.duration,
                    transition.params,
                )
                .await
            }));
        }
        if let Some(task) = self.wipe_task.as_mut() {
            if !self.wipe_flags.contains(WipeFlags::DONT_BLOCK_ANIMATIONS) {
                adv_state.allow_running_animations = false;
            }
            self.ready_wiper = if std::mem::replace(&mut self.wipe_wait_attempted, true) {
                task.poll_naive()?
            } else {
                task.poll_with_timeout(context.loader_wait_budget())?
            };
            self.wipe_task = None;
        }
        let loaded_layer = self.ready_layer.take();

        let wiper = self.ready_wiper.take();

        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());

        let mut plane_layer_group = adv_state.plane_layer_group_mut(self.state_info.plane);

        for info in &self.state_info.affected_layers {
            let previous = plane_layer_group.get_layer(info.operation_target.layerbank);
            let mut layer = if info.already_the_same {
                previous
                    .expect("Cached layer needs its installed target")
                    .render_clone(&mut context.pre_render.render_clone_ctx())
            } else if let Some((kind, params)) = self.resource_free {
                UserLayer::load_resource_free(kind, params).expect("Resource-free factory")
            } else {
                loaded_layer
                    .as_ref()
                    .expect("Changed layer needs a loader")
                    .publish(&mut context.pre_render.render_clone_ctx())
            };

            // Native94dc/96b4 creates defaults without copying the old owner.
            // Native94f0 retains it directly when there is no replacement wipe.
            let previous_props = previous.filter(|_| info.keep_old_props).map(|layer| {
                if wiper.is_none() || self.share_properties {
                    layer.properties().share()
                } else {
                    layer.properties().clone()
                }
            });
            let mut properties = match (previous_props, info.keep_old_props) {
                (Some(previous_props), true) => previous_props,
                _ => {
                    let mut properties = LayerProperties::new();
                    properties.set_layer_id(info.layer_id_for_properties);
                    properties
                }
            };

            properties.set_layerload_counter1(info.layer_load_counter1);
            layer.set_properties(properties);

            let slot_wiper = wiper
                .as_ref()
                .map(|wiper| wiper.render_clone(&mut context.pre_render.render_clone_ctx()));
            plane_layer_group.add_layer_with_wiper(
                info.operation_target.layerbank,
                layer,
                slot_wiper,
            );
        }

        crate::adv::gallery::apply(
            vm_state,
            crate::adv::gallery::loaded_layer(
                scenario.info_tables(),
                self.loaded_kind,
                self.loaded_id,
            ),
        );

        adv_state.allow_running_animations = true;

        self.published = true;
        self.finish_if_wipes_complete(adv_state)
    }
}

impl LAYERLOAD {
    #[cfg(test)]
    pub(super) fn reference_published(&self) -> bool {
        self.published
    }
    #[cfg(test)]
    pub(super) fn reference_finished(&self) -> bool {
        self.token.is_none()
    }

    fn finish_if_wipes_complete(&mut self, adv_state: &AdvState) -> Option<CommandResult> {
        let group = adv_state.plane_layer_group(self.state_info.plane);
        if !self.wipe_flags.contains(WipeFlags::DONT_WAIT)
            && self
                .state_info
                .affected_layers
                .iter()
                .any(|info| group.has_wiper_for_layerbank(info.operation_target.layerbank))
        {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

impl Debug for LAYERLOAD {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LAYERLOAD")
            .field(
                &self
                    .state_info
                    .affected_layers
                    .iter()
                    .map(|info| info.operation_target)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use binrw::{BinRead, BinWrite};
    use shin_core::{
        format::scenario::{instruction_elements::NumberSpec, instructions::Instruction},
        vm::{IntoRuntimeForm, VmCtx, command::types::LayerProperty},
    };

    fn load(id: i32, flags: i32, picture: i32) -> command::runtime::LAYERLOAD {
        let mut encoded = std::io::Cursor::new(Vec::new());
        0xc1u8.write_le(&mut encoded).unwrap();
        for value in [id, 2, flags] {
            NumberSpec::constant(value).write_le(&mut encoded).unwrap();
        }
        1u8.write_le(&mut encoded).unwrap();
        NumberSpec::constant(picture)
            .write_le(&mut encoded)
            .unwrap();
        encoded.set_position(0);
        let Instruction::Command(command) = Instruction::read_le(&mut encoded).unwrap() else {
            panic!("Layer command")
        };
        let command::RuntimeCommand::LAYERLOAD(load) =
            command.into_runtime_form(&VmCtx::new(0, 42))
        else {
            panic!("Layer load")
        };
        load
    }

    pub(crate) fn reference_live_publication_constructors<'a>(
        context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<crate::asset::system::AssetServer>,
        scenario: &Arc<Scenario>,
        adv: &mut AdvState,
    ) {
        use crate::layer::DrawableLayer;
        let mut vm = VmState::for_scenario(0, 42);
        vm.layers.layer_selection.from = LayerId::new(0);
        vm.layers.layer_selection.to = LayerId::new(1);
        let plane = vm.layers.current_plane;
        adv.create_back_layer_group_if_needed(&mut context.render_clone_ctx());
        let mut previous_identities = Vec::new();
        for (kind, flags, color, expected_draws, retain) in [
            (LayerType::Effect, 0, 123, 16, false),
            (LayerType::Tile, 2, 0xfff, 2, true),
            (LayerType::Tile, 2, 0xfff, 12, true),
            (LayerType::Tile, 0, 0xf00, 16, false),
            (LayerType::Null, 0, 0, 14, false),
        ] {
            let mut encoded = std::io::Cursor::new(Vec::new());
            0xc1u8.write_le(&mut encoded).unwrap();
            for value in [-5, kind as i32, flags] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            0xffu8.write_le(&mut encoded).unwrap();
            for value in [color, 0, 0, 200, 100, 0, 0, 0] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!("Layer command");
            };
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&VmCtx::new(0, 42))
            else {
                panic!("Layer load");
            };
            assert_eq!(runtime.params, (color, 0, 0, 200, 100, 0, 0, 0));
            let info = runtime.apply_state(&mut vm);
            let seed_calls = crate::engine_random::seed_call_count();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                frame_id: Default::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut command)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!("Layer load must yield");
            };
            assert!(
                command.load_task.is_none(),
                "Immediate load must not spawn a worker"
            );
            assert_eq!(
                crate::engine_random::seed_call_count(),
                seed_calls,
                "Start keeps factory deferred"
            );
            if kind == LayerType::Effect {
                // Hold replacement-wipe readiness to prove the synchronous
                // factory stays deferred even after the layer loader is ready.
                let (sender, receiver) = std::sync::mpsc::channel();
                command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                    receiver.recv().unwrap()
                }));
                assert!(
                    command
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
                sender.send(None).unwrap();
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while command
                .update(&mut update, scenario, &mut vm, adv, false)
                .is_none()
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Publication timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                crate::engine_random::seed_call_count(),
                seed_calls + expected_draws,
                "Per-bank constructor/property RNG for {kind:?},flags{flags}"
            );
            let identities = [LayerId::new(0), LayerId::new(1)].map(|id| {
                let bank = vm
                    .layers
                    .layerbank_allocator
                    .get_layerbank_id(plane, id)
                    .unwrap();
                adv.plane_layer_group(plane)
                    .get_layer(bank)
                    .unwrap()
                    .properties()
                    .identity()
            });
            if retain {
                assert_eq!(identities.as_slice(), previous_identities);
            }
            previous_identities = identities.to_vec();
            assert!(adv.allow_running_animations);
        }
        // Unlike the immediate factories above, native Picture82d60 and
        // Bustup83520 publish virtual+28 clones of a worker-built prototype.
        let bustup_id = scenario
            .info_tables()
            .bustup_info
            .iter()
            .position(|info| info.lipsync_character_id == 19)
            .unwrap() as i32;
        for (kind, asset_id) in [(LayerType::Picture, 0), (LayerType::Bustup, bustup_id)] {
            fn snapshot(layer: &UserLayer) -> (usize, u32, Option<(u32, u32, u32, Vec<u32>)>) {
                match layer {
                    UserLayer::Picture(layer) => (
                        layer.reference_resource_identity(),
                        layer.reference_dissolve_seed(),
                        None,
                    ),
                    UserLayer::Bustup(layer) => (
                        layer.reference_resource_identity(),
                        layer.reference_dissolve_seed(),
                        Some(layer.reference_blink_state()),
                    ),
                    _ => panic!("Expected asset-backed prototype"),
                }
            }
            let mut encoded = std::io::Cursor::new(Vec::new());
            0xc1u8.write_le(&mut encoded).unwrap();
            for value in [-5, kind as i32, 0] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            1u8.write_le(&mut encoded).unwrap();
            NumberSpec::constant(asset_id)
                .write_le(&mut encoded)
                .unwrap();
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!("Layer command");
            };
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&VmCtx::new(0, 42))
            else {
                panic!("Layer load");
            };
            let info = runtime.apply_state(&mut vm);
            let seed_calls = crate::engine_random::seed_call_count();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                frame_id: Default::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut command)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!("Asset load must yield");
            };
            assert!(command.load_task.is_some());
            let (sender, receiver) = futures::channel::oneshot::channel();
            command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                receiver.await.unwrap()
            }));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while command.ready_layer.is_none() {
                assert!(
                    command
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "Prototype preparation timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
            let PreparedUserLayer::Prototype(prepared) = command.ready_layer.as_ref().unwrap()
            else {
                panic!("Expected worker-built prototype");
            };
            let prototype = snapshot(prepared);
            sender.send(None).unwrap();
            while command
                .update(&mut update, scenario, &mut vm, adv, false)
                .is_none()
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Prototype publication timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                crate::engine_random::seed_call_count(),
                seed_calls + 14,
                "Only two fresh property objects draw on the publication thread for {kind:?}"
            );
            let banks = [LayerId::new(0), LayerId::new(1)].map(|id| {
                vm.layers
                    .layerbank_allocator
                    .get_layerbank_id(plane, id)
                    .unwrap()
            });
            let group = adv.plane_layer_group(plane);
            let first = group.get_layer(banks[0]).unwrap();
            let second = group.get_layer(banks[1]).unwrap();
            assert_eq!(snapshot(first), prototype);
            assert_eq!(snapshot(second), prototype);
            assert_ne!(
                first.properties().identity(),
                second.properties().identity()
            );
            let group = adv.plane_layer_group_mut(plane);
            let first = group.get_layer_mut(banks[0]).unwrap();
            first
                .properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX)
                .fast_forward_to(123.0);
            if let UserLayer::Bustup(layer) = first {
                layer.reference_advance_blink();
                assert_ne!(snapshot(first), prototype);
            }
            let second = group.get_layer(banks[1]).unwrap();
            assert_eq!(
                second.properties().get_value(LayerProperty::TranslateX),
                0.0
            );
            assert_eq!(snapshot(second), prototype);
            assert!(adv.allow_running_animations);
        }
        for (kind, params) in [
            (LayerType::FocusLine, vec![120, -45]),
            (LayerType::Rain, vec![1, 100, 1500]),
            (LayerType::Animation, vec![0]),
        ] {
            fn state(layer: &UserLayer) -> (usize, u32, u32) {
                match layer {
                    UserLayer::FocusLine(layer) => layer.reference_constructor_state(),
                    UserLayer::Rain(layer) => layer.reference_constructor_state(),
                    UserLayer::Animation(layer) => layer.reference_constructor_state(),
                    _ => panic!("Expected procedural layer"),
                }
            }
            let mut previous_states = None;
            for cached in [false, true] {
                let mut encoded = std::io::Cursor::new(Vec::new());
                0xc1u8.write_le(&mut encoded).unwrap();
                for value in [-5, kind as i32, 0] {
                    NumberSpec::constant(value).write_le(&mut encoded).unwrap();
                }
                ((1u8 << params.len()) - 1).write_le(&mut encoded).unwrap();
                for &value in &params {
                    NumberSpec::constant(value).write_le(&mut encoded).unwrap();
                }
                encoded.set_position(0);
                let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap()
                else {
                    panic!("Layer command");
                };
                let command::RuntimeCommand::LAYERLOAD(runtime) =
                    runtime.into_runtime_form(&VmCtx::new(0, 42))
                else {
                    panic!("Layer load");
                };
                let decoded = [
                    runtime.params.0,
                    runtime.params.1,
                    runtime.params.2,
                    runtime.params.3,
                    runtime.params.4,
                    runtime.params.5,
                    runtime.params.6,
                    runtime.params.7,
                ];
                assert_eq!(&decoded[..params.len()], params.as_slice());
                assert!(decoded[params.len()..].iter().all(|&value| value == 0));
                let info = runtime.apply_state(&mut vm);
                let seed_calls = crate::engine_random::seed_call_count();
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                    frame_id: Default::default(),
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                };
                let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(
                    mut command,
                )) = runtime.start(&mut update, scenario, &vm, info, adv)
                else {
                    panic!("Procedural load must yield");
                };
                assert_eq!(command.load_task.is_none(), cached);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                if !cached {
                    let (sender, receiver) = futures::channel::oneshot::channel();
                    command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                        receiver.await.unwrap()
                    }));
                    while command.ready_layer.is_none() {
                        assert!(
                            command
                                .update(&mut update, scenario, &mut vm, adv, false)
                                .is_none()
                        );
                        assert!(
                            std::time::Instant::now() < deadline,
                            "Procedural preparation timed out"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    assert!(!matches!(
                        command.ready_layer,
                        Some(PreparedUserLayer::Prototype(_))
                    ));
                    assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
                    sender.send(None).unwrap();
                }
                while command
                    .update(&mut update, scenario, &mut vm, adv, false)
                    .is_none()
                {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Procedural publication timed out"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert_eq!(
                    crate::engine_random::seed_call_count(),
                    seed_calls
                        + if cached {
                            26
                        } else if kind == LayerType::Animation {
                            // Each native Animation factory warms its base
                            // property owner before publication replaces it.
                            32
                        } else {
                            18
                        },
                    "Per-bank procedural constructors versus cached clones for {kind:?}"
                );
                let states = [LayerId::new(0), LayerId::new(1)].map(|id| {
                    let bank = vm
                        .layers
                        .layerbank_allocator
                        .get_layerbank_id(plane, id)
                        .unwrap();
                    state(adv.plane_layer_group(plane).get_layer(bank).unwrap())
                });
                assert_eq!(states[0].0, states[1].0, "Immutable atlas is shared");
                assert_ne!(
                    states[0].1, states[1].1,
                    "Each bank constructs its own drawable seed"
                );
                assert_ne!(
                    states[0].2, states[1].2,
                    "Each bank constructs its own particle seed"
                );
                if cached {
                    assert_eq!(Some(states), previous_states);
                }
                previous_states = Some(states);
            }
        }
        for id in [LayerId::new(0), LayerId::new(1)] {
            let bank = vm
                .layers
                .layerbank_allocator
                .get_layerbank_id(plane, id)
                .unwrap();
            adv.plane_layer_group_mut(plane)
                .remove_layer(bank, Ticks::ZERO);
        }
        adv.back_layer_group = None;
        println!(
            "Native publication constructors: two selected banks, deferred Effect/Tile/Null factories on command thread, gated wipe readiness and expired frame budget, exact16/2/12/16/14 RNG draws, reset skips unused copy and no-wipe keep retains owner pass"
        );
        println!(
            "Native Picture/Bustup publication: worker prototype, two selected banks share asset and dissolve seed, copied blink state and independent properties/blink,14 publication RNG draws each pass"
        );
        println!(
            "Native FocusLine/Rain/Animation publication: atlas-only preparation, gated per-bank constructors on command thread, distinct drawable/particle seeds, shared textures,18 FocusLine/Rain or32 Animation constructor/property RNG draws and26 cached-clone/reset draws each pass"
        );
    }

    pub(crate) fn reference_live_interactive_publication<'a>(
        context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<crate::asset::system::AssetServer>,
        scenario: &Arc<Scenario>,
        adv: &mut AdvState,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        fn state(layer: &UserLayer) -> (usize, usize, i32) {
            match layer {
                UserLayer::Quiz(layer) => layer.reference_controller_state(),
                UserLayer::Cake(layer) => layer.reference_controller_state(),
                UserLayer::Murderer(layer) => layer.reference_controller_state(),
                UserLayer::Last(layer) => layer.reference_controller_state(),
                _ => panic!("Expected interactive layer"),
            }
        }
        let mut vm = VmState::for_scenario(0, 42);
        vm.layers.layer_selection.from = LayerId::new(0);
        vm.layers.layer_selection.to = LayerId::new(1);
        let plane = vm.layers.current_plane;
        adv.create_back_layer_group_if_needed(&mut context.render_clone_ctx());
        for mode in 0..4 {
            let mut encoded = std::io::Cursor::new(Vec::new());
            0xc1u8.write_le(&mut encoded).unwrap();
            for value in [-5, LayerType::Quiz as i32, 0] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            0b11u8.write_le(&mut encoded).unwrap();
            for value in [mode, 0x7f] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!();
            };
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&VmCtx::new(0, 42))
            else {
                panic!();
            };
            assert_eq!(runtime.params, (mode, 0x7f, 0, 0, 0, 0, 0, 0));
            let info = runtime.apply_state(&mut vm);
            let seed_calls = crate::engine_random::seed_call_count();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                frame_id: Default::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut command)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!();
            };
            let (sender, receiver) = futures::channel::oneshot::channel();
            command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                receiver.await.unwrap()
            }));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while command.ready_layer.is_none() {
                assert!(
                    command
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "Interactive preparation timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(!matches!(
                command.ready_layer,
                Some(PreparedUserLayer::Prototype(_))
            ));
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
            sender.send(None).unwrap();
            while command
                .update(&mut update, scenario, &mut vm, adv, false)
                .is_none()
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Interactive publication timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                crate::engine_random::seed_call_count(),
                seed_calls + 14,
                "Plain InteractiveLayer has no drawable seed, mode{mode}"
            );
            let banks = [LayerId::new(0), LayerId::new(1)].map(|id| {
                vm.layers
                    .layerbank_allocator
                    .get_layerbank_id(plane, id)
                    .unwrap()
            });
            let group = adv.plane_layer_group(plane);
            let first = group.get_layer(banks[0]).unwrap();
            let second = group.get_layer(banks[1]).unwrap();
            let initial = state(first);
            let untouched = state(second);
            assert_eq!(initial.0, untouched.0, "Immutable atlas is shared");
            assert_ne!(
                initial.1, untouched.1,
                "Fresh banks own distinct controllers"
            );
            assert_ne!(
                first.properties().identity(),
                second.properties().identity()
            );
            let cake_fingers = match (first, second) {
                (UserLayer::Cake(a), UserLayer::Cake(b)) => {
                    Some((a.reference_feedback().0, b.reference_feedback().0))
                }
                _ => None,
            };
            first.interactive().unwrap().navigate(1);
            if let Some((a, b)) = cake_fingers {
                // Native Cake starts at1440,900, outside the half-open hit map.
                // Down moves its finger eight pixels without selecting a slice.
                let (UserLayer::Cake(first), UserLayer::Cake(second)) = (first, second) else {
                    panic!();
                };
                assert_eq!(first.reference_feedback().0, a + glam::Vec2::new(0.0, 8.0));
                assert_eq!(
                    second.reference_feedback().0,
                    b,
                    "Second bank's finger did not move"
                );
            } else {
                assert_ne!(
                    state(first).2,
                    initial.2,
                    "First bank accepted navigation, mode{mode}"
                );
            }
            assert_eq!(state(second), untouched, "Second bank did not navigate");
            if mode == 2 {
                let seed_calls = crate::engine_random::seed_call_count();
                let mut alias = first.render_clone(&mut update.pre_render.render_clone_ctx());
                let UserLayer::Murderer(original) = first else {
                    panic!();
                };
                let UserLayer::Murderer(retained) = &alias else {
                    panic!();
                };
                assert_eq!(
                    retained.reference_retained_state(),
                    original.reference_retained_state()
                );
                assert!(original.reference_retained_render_identity().is_some());
                assert_eq!(
                    retained.reference_retained_render_identity(),
                    original.reference_retained_render_identity()
                );
                assert_eq!(alias.properties().identity(), first.properties().identity());
                assert_eq!(state(&alias), state(first));
                assert_eq!(
                    crate::engine_random::seed_call_count(),
                    seed_calls,
                    "Retaining native Murderer owner must not copy property RNG"
                );
                alias
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .enqueue_now(60.0, shin_core::time::Tween::linear(Ticks::from_f32(60.0)));
                let mut frame_id = shin_core::primitives::update::FrameId::default();
                frame_id.advance();
                let advance = crate::update::AdvUpdateContext {
                    frame_id,
                    delta_ticks: Ticks::from_f32(1.0),
                    asset_server: server,
                    device: update.pre_render.device,
                    queue: update.pre_render.queue,
                    are_animations_allowed: true,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                };
                crate::update::AdvUpdatable::update(&mut alias, &advance);
                let mut late_alias = first.render_clone(&mut update.pre_render.render_clone_ctx());
                crate::update::AdvUpdatable::update(&mut late_alias, &advance);
                // Native9b130 updates the retained controller without advancing parent property tweens.
                assert_eq!(first.properties().get_value(LayerProperty::TranslateX), 0.0);
                assert_eq!(
                    second.properties().get_value(LayerProperty::TranslateX),
                    0.0
                );
                assert_eq!(
                    original.reference_retained_state().1,
                    0.25,
                    "Shared controller advances only once across retained aliases"
                );
                let old_properties = first.properties().share();
                let mut replacement = LayerProperties::new();
                replacement
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to(345.0);
                let replacement_id = replacement.identity();
                let replacement_draws = crate::engine_random::seed_call_count();
                alias.set_properties(replacement);
                assert_eq!(first.properties().identity(), replacement_id);
                assert_eq!(late_alias.properties().identity(), replacement_id);
                assert_eq!(
                    first.properties().get_value(LayerProperty::TranslateX),
                    345.0
                );
                assert_eq!(old_properties.get_value(LayerProperty::TranslateX), 0.0);
                assert_eq!(
                    second.properties().get_value(LayerProperty::TranslateX),
                    0.0
                );
                assert_eq!(crate::engine_random::seed_call_count(), replacement_draws);
                let copied_group = group.render_clone(&mut update.pre_render.render_clone_ctx());
                let group_alias = copied_group.get_layer(banks[0]).unwrap();
                assert_eq!(
                    group_alias.properties().identity(),
                    replacement_id,
                    "Scene clone keeps native retained layer pointer"
                );
                late_alias
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to(456.0);
                assert_eq!(
                    group_alias
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    456.0
                );
                assert_eq!(old_properties.get_value(LayerProperty::TranslateX), 0.0);
                let replaced_properties = alias.properties().share();
                let UserLayer::Murderer(layer) = &alias else {
                    panic!();
                };
                layer.confirm();
                layer.advance_animation(7.0);
                layer.advance_animation(0.0);
                assert!(layer.reference_wipe_state().is_some());
                alias.pre_render(update.pre_render, &Default::default());
                let UserLayer::Murderer(layer) = &alias else {
                    panic!();
                };
                let active_wipe = layer.reference_wipe_state().unwrap();
                assert_eq!((active_wipe.1, active_wipe.2), (0.0, 30.0));
                let frozen = layer.reference_frozen_pixels(update.pre_render);
                assert!(frozen.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
                let retained_controller = state(&alias);
                encoded.set_position(0);
                let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap()
                else {
                    panic!();
                };
                let command::RuntimeCommand::LAYERLOAD(runtime) =
                    runtime.into_runtime_form(&VmCtx::new(0, 42))
                else {
                    panic!();
                };
                let info = runtime.apply_state(&mut vm);
                assert!(info.skip_layerloader_creation);
                let cached_draws = crate::engine_random::seed_call_count();
                let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(
                    mut cached,
                )) = runtime.start(&mut update, scenario, &vm, info, adv)
                else {
                    panic!();
                };
                assert!(cached.load_task.is_none());
                assert!(
                    cached
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_some()
                );
                assert_eq!(crate::engine_random::seed_call_count(), cached_draws + 14);
                let installed = adv.plane_layer_group(plane).get_layer(banks[0]).unwrap();
                assert_eq!(
                    state(installed),
                    retained_controller,
                    "Cached Murderer retains controller state instead of resetting it"
                );
                assert_eq!(
                    alias.properties().identity(),
                    installed.properties().identity()
                );
                assert_eq!(
                    group_alias.properties().identity(),
                    installed.properties().identity()
                );
                assert_eq!(alias.properties().get_value(LayerProperty::TranslateX), 0.0);
                assert_eq!(
                    replaced_properties.get_value(LayerProperty::TranslateX),
                    456.0
                );
                alias.pre_render(update.pre_render, &Default::default());
                let UserLayer::Murderer(layer) = &alias else {
                    panic!();
                };
                assert_eq!(layer.reference_wipe_state(), Some(active_wipe));
                assert_eq!(
                    layer.reference_frozen_pixels(update.pre_render),
                    frozen,
                    "Cached property reset preserves the active frozen capture"
                );

                // Cached source/target are aliases of one native retained owner.
                // A real outer replacement wipe must not double its internal clock.
                vm.layers.transition.duration = Ticks::from_f32(6.0);
                let mut wiping = std::io::Cursor::new(Vec::new());
                0xc1u8.write_le(&mut wiping).unwrap();
                for value in [-5, LayerType::Quiz as i32, LayerLoadFlags::AUTO_WIPE.bits()] {
                    NumberSpec::constant(value).write_le(&mut wiping).unwrap();
                }
                0b11u8.write_le(&mut wiping).unwrap();
                for value in [mode, 0x7f] {
                    NumberSpec::constant(value).write_le(&mut wiping).unwrap();
                }
                wiping.set_position(0);
                let Instruction::Command(runtime) = Instruction::read_le(&mut wiping).unwrap()
                else {
                    panic!();
                };
                let command::RuntimeCommand::LAYERLOAD(runtime) =
                    runtime.into_runtime_form(&VmCtx::new(0, 42))
                else {
                    panic!();
                };
                let info = runtime.apply_state(&mut vm);
                assert!(info.skip_layerloader_creation);
                let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(
                    mut replacing,
                )) = runtime.start(&mut update, scenario, &vm, info, adv)
                else {
                    panic!();
                };
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !replacing.reference_published() {
                    assert!(
                        replacing
                            .update(&mut update, scenario, &mut vm, adv, false)
                            .is_none()
                    );
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(
                    adv.plane_layer_group(plane)
                        .has_wiper_for_layerbank(banks[0])
                );
                adv.plane_layer_group_mut(plane)
                    .pre_render(update.pre_render, &Default::default());
                let mut replacement_copy = adv
                    .plane_layer_group(plane)
                    .render_clone(&mut update.pre_render.render_clone_ctx());
                let mut outer_update = crate::update::AdvUpdateContext {
                    delta_ticks: Ticks::from_f32(2.0),
                    ..advance
                };
                outer_update.frame_id.advance();
                crate::update::AdvUpdatable::update(
                    adv.plane_layer_group_mut(plane),
                    &outer_update,
                );
                crate::update::AdvUpdatable::update(&mut replacement_copy, &outer_update);
                let UserLayer::Murderer(layer) = &alias else {
                    panic!();
                };
                assert_eq!(
                    layer.reference_wipe_state(),
                    Some((active_wipe.0, 2.0, 30.0)),
                    "Original/copied replacement source/target share one internal wipe update"
                );
                assert!(
                    replacing
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                outer_update.frame_id.advance();
                outer_update.delta_ticks = Ticks::from_f32(4.0);
                crate::update::AdvUpdatable::update(
                    adv.plane_layer_group_mut(plane),
                    &outer_update,
                );
                assert!(
                    replacing
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_some()
                );
                crate::update::AdvUpdatable::update(&mut replacement_copy, &outer_update);
                assert_eq!(
                    layer.reference_wipe_state(),
                    Some((active_wipe.0, 6.0, 30.0))
                );
                assert!(
                    !adv.plane_layer_group(plane)
                        .has_wiper_for_layerbank(banks[0])
                );
                assert!(!replacement_copy.has_wiper_for_layerbank(banks[0]));
                alias.pre_render(update.pre_render, &Default::default());
                let UserLayer::Murderer(layer) = &alias else {
                    panic!();
                };
                assert_eq!(
                    layer.reference_frozen_pixels(update.pre_render),
                    frozen,
                    "Outer wipe retirement preserves retained internal capture"
                );
                vm.layers.transition.duration = Ticks::ZERO;
            } else {
                // Quiz/Cake/Last copy their controller values; Murderer native
                // clone9b040 instead retains the whole layer owner.
                let cloned = first.render_clone(&mut update.pre_render.render_clone_ctx());
                assert_ne!(state(&cloned).1, state(first).1);
                assert_eq!(state(&cloned).2, state(first).2);
                let before = state(first);
                let cake_finger = match first {
                    UserLayer::Cake(layer) => Some(layer.reference_feedback().0),
                    _ => None,
                };
                if let UserLayer::Last(layer) = &cloned {
                    layer.advance_animation(7.0);
                    layer.advance_animation(20.0);
                    layer.back();
                } else {
                    cloned.interactive().unwrap().navigate(1);
                }
                if let Some(finger) = cake_finger {
                    let (UserLayer::Cake(original), UserLayer::Cake(copy)) = (first, &cloned)
                    else {
                        panic!();
                    };
                    assert_eq!(
                        copy.reference_feedback().0,
                        finger + glam::Vec2::new(0.0, 8.0)
                    );
                    assert_eq!(
                        original.reference_feedback().0,
                        finger,
                        "Clone's finger movement is independent"
                    );
                } else {
                    assert_ne!(state(&cloned).2, before.2);
                }
                assert_eq!(state(first), before, "Clone navigation is independent");
                if mode < 2 {
                    first.interactive().unwrap().confirm();
                }
                let before_cache = [state(first), state(second)];
                let result_before_cache = first.interactive().unwrap().result();
                encoded.set_position(0);
                let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap()
                else {
                    panic!();
                };
                let command::RuntimeCommand::LAYERLOAD(runtime) =
                    runtime.into_runtime_form(&VmCtx::new(0, 42))
                else {
                    panic!();
                };
                let info = runtime.apply_state(&mut vm);
                assert!(info.skip_layerloader_creation);
                let cached_draws = crate::engine_random::seed_call_count();
                let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(
                    mut cached,
                )) = runtime.start(&mut update, scenario, &vm, info, adv)
                else {
                    panic!();
                };
                assert!(cached.load_task.is_none());
                assert!(
                    cached
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_some()
                );
                assert_eq!(crate::engine_random::seed_call_count(), cached_draws + 26);
                for (bank, before) in banks.into_iter().zip(before_cache) {
                    let installed = adv.plane_layer_group(plane).get_layer(bank).unwrap();
                    let after = state(installed);
                    assert_eq!(after.0, before.0);
                    assert_ne!(
                        after.1, before.1,
                        "Cached ordinary interactive copy owns its controller"
                    );
                    assert_eq!(after.2, before.2, "Cached selection survives mode{mode}");
                }
                let installed = adv.plane_layer_group(plane).get_layer(banks[0]).unwrap();
                assert_eq!(
                    installed.interactive().unwrap().result(),
                    result_before_cache,
                    "Cached result survives mode{mode}"
                );
            }
        }
        // Native invalid factories return null; publication cannot install a
        // drawable. Preserve readiness ordering and reject a false empty layer.
        for (layer_type, kind) in [
            (LayerType::Quiz, -1),
            (LayerType::Quiz, 4),
            (LayerType::Quiz, 128),
            (LayerType::Animation, -1),
            (LayerType::Animation, 6),
            (LayerType::Animation, i32::MIN),
            (LayerType::Animation, i32::MAX),
        ] {
            let before = adv
                .plane_layer_group(plane)
                .get_layer(
                    vm.layers
                        .layerbank_allocator
                        .get_layerbank_id(plane, LayerId::new(0))
                        .unwrap(),
                )
                .unwrap()
                .properties()
                .identity();
            let mut encoded = std::io::Cursor::new(Vec::new());
            0xc1u8.write_le(&mut encoded).unwrap();
            for value in [-5, layer_type as i32, 0] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            0b11u8.write_le(&mut encoded).unwrap();
            // Signed32-bit extremes arrive through VM registers; the disk
            // constant encoding itself supports only signed28-bit values.
            let mode_register =
                shin_core::format::scenario::instruction_elements::Register::from_regular_register(
                    0,
                );
            NumberSpec::register(mode_register)
                .write_le(&mut encoded)
                .unwrap();
            NumberSpec::constant(0).write_le(&mut encoded).unwrap();
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!();
            };
            let mut vm_context = VmCtx::new(0, 42);
            vm_context.write_register(mode_register, kind);
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&vm_context)
            else {
                panic!();
            };
            assert_eq!(runtime.params, (kind, 0, 0, 0, 0, 0, 0, 0));
            let info = runtime.apply_state(&mut vm);
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                frame_id: Default::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let seed_calls = crate::engine_random::seed_call_count();
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut command)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!();
            };
            let (sender, receiver) = futures::channel::oneshot::channel();
            command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                receiver.await.unwrap()
            }));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while command.ready_layer.is_none() {
                assert!(
                    command
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "Invalid descriptor preparation timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            match &command.ready_layer {
                Some(PreparedUserLayer::InvalidInteractive(value))
                    if layer_type == LayerType::Quiz =>
                {
                    assert_eq!(*value, kind)
                }
                Some(PreparedUserLayer::InvalidAnimation(value))
                    if layer_type == LayerType::Animation =>
                {
                    assert_eq!(*value, kind)
                }
                _ => panic!("Invalid factory must remain a prepared failure until readiness"),
            }
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
            sender.send(None).unwrap();
            let failure = loop {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    command.update(&mut update, scenario, &mut vm, adv, false)
                }));
                match result {
                    Err(failure) => break failure,
                    Ok(None) => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Ok(Some(_)) => panic!("Invalid factory must not finish as an empty layer"),
                }
            };
            let message = failure
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| failure.downcast_ref::<&str>().copied())
                .unwrap();
            assert!(message.contains("native factory returned no drawable layer"));
            assert!(!command.reference_published());
            assert_eq!(
                adv.plane_layer_group(plane)
                    .get_layer(
                        vm.layers
                            .layerbank_allocator
                            .get_layerbank_id(plane, LayerId::new(0))
                            .unwrap()
                    )
                    .unwrap()
                    .properties()
                    .identity(),
                before,
                "Failed factory leaves installed layer intact"
            );
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
        }
        for id in [LayerId::new(0), LayerId::new(1)] {
            let bank = vm
                .layers
                .layerbank_allocator
                .get_layerbank_id(plane, id)
                .unwrap();
            adv.plane_layer_group_mut(plane)
                .remove_layer(bank, Ticks::ZERO);
        }
        adv.back_layer_group = None;
        println!(
            "Native interactive publication: four modes, two selected banks, asset-only preparation and gated factories,14 property-only RNG draws, independent controllers/navigation; Quiz/Cake/Last deep controller clones pass"
        );
    }

    pub(crate) fn reference_live_movie_publication<'a>(
        context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<crate::asset::system::AssetServer>,
        scenario: &Arc<Scenario>,
        adv: &mut AdvState,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use crate::update::AdvUpdatable;
        let mut vm = VmState::for_scenario(0, 42);
        vm.layers.layer_selection.from = LayerId::new(0);
        vm.layers.layer_selection.to = LayerId::new(1);
        let plane = vm.layers.current_plane;
        adv.create_back_layer_group_if_needed(&mut context.render_clone_ctx());
        assert_eq!(adv.audio_manager.kira_manager().lock().num_sounds(), 0);
        for (name, has_audio, repeat) in [
            ("door", false, 0),
            ("op1", true, 0),
            ("door", false, 1),
            ("op1", true, 1),
        ] {
            let id = scenario
                .info_tables()
                .movie_info
                .iter()
                .position(|item| item.name.as_str().eq_ignore_ascii_case(name))
                .unwrap() as i32;
            let mut encoded = std::io::Cursor::new(Vec::new());
            0xc1u8.write_le(&mut encoded).unwrap();
            for value in [-5, LayerType::Movie as i32, 0] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            0b111u8.write_le(&mut encoded).unwrap();
            for value in [id, 0, repeat] {
                NumberSpec::constant(value).write_le(&mut encoded).unwrap();
            }
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!();
            };
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&VmCtx::new(0, 42))
            else {
                panic!();
            };
            assert_eq!(runtime.params, (id, 0, repeat, 0, 0, 0, 0, 0));
            let info = runtime.apply_state(&mut vm);
            assert!(!info.skip_layerloader_creation);
            let seed_calls = crate::engine_random::seed_call_count();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
                frame_id: Default::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            };
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut command)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!();
            };
            assert!(command.load_task.is_some());
            let (sender, receiver) = futures::channel::oneshot::channel();
            command.wipe_task = Some(shin_tasks::async_io::spawn(async move {
                receiver.await.unwrap()
            }));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while command.ready_layer.is_none() {
                assert!(
                    command
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "Movie prototype preparation timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls);
            let PreparedUserLayer::Prototype(UserLayer::Movie(prototype)) =
                command.ready_layer.as_ref().unwrap()
            else {
                panic!();
            };
            let owner = prototype.reference_publication_owner();
            let dissolve_seed = prototype.reference_dissolve_seed();
            let still = prototype.reference_still_state();
            // Keep the compared cell alive after the worker prototype is
            // dropped; its address may otherwise be reused by a cached clone.
            let _prototype_still_properties = prototype.reference_still_properties();
            assert_eq!(
                owner.owners(),
                1,
                "Only the worker prototype owns playback before publication"
            );
            sender.send(None).unwrap();
            while command
                .update(&mut update, scenario, &mut vm, adv, false)
                .is_none()
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Movie prototype publication timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            drop(command);
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 14);
            assert_eq!(
                owner.owners(),
                2,
                "Two selected banks retain one playback owner"
            );
            let banks = [LayerId::new(0), LayerId::new(1)].map(|id| {
                vm.layers
                    .layerbank_allocator
                    .get_layerbank_id(plane, id)
                    .unwrap()
            });
            let group = adv.plane_layer_group(plane);
            for bank in banks {
                let UserLayer::Movie(layer) = group.get_layer(bank).unwrap() else {
                    panic!();
                };
                assert_eq!(
                    layer.reference_publication_owner().identity(),
                    owner.identity()
                );
                assert_eq!(layer.reference_dissolve_seed(), dissolve_seed);
                assert_eq!(layer.reference_still_state().0, still.0);
                assert_eq!(layer.reference_still_state().1, still.1);
                assert_ne!(layer.reference_still_state().2, still.2);
            }
            assert_ne!(
                group.get_layer(banks[0]).unwrap().properties().identity(),
                group.get_layer(banks[1]).unwrap().properties().identity()
            );
            // Silent movie time is deterministic and must advance only once,
            // including a new layer clone created after that frame was consumed.
            let mut frame_id = shin_core::primitives::update::FrameId::default();
            frame_id.advance();
            let advance = crate::update::AdvUpdateContext {
                frame_id,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                device: update.pre_render.device,
                queue: update.pre_render.queue,
                are_animations_allowed: false,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            };
            adv.plane_layer_group_mut(plane)
                .get_layer_mut(banks[0])
                .unwrap()
                .update(&advance);
            let consumed_time = owner.time();
            let mut late_alias = adv
                .plane_layer_group(plane)
                .get_layer(banks[0])
                .unwrap()
                .render_clone(&mut update.pre_render.render_clone_ctx());
            let duplicate = crate::update::AdvUpdateContext {
                delta_ticks: Ticks::from_seconds(60.0),
                ..advance
            };
            late_alias.update(&duplicate);
            adv.plane_layer_group_mut(plane)
                .get_layer_mut(banks[1])
                .unwrap()
                .update(&duplicate);
            if !has_audio {
                assert!(
                    consumed_time > 0,
                    "Movie clock runs while layer animations are disabled"
                );
                assert_eq!(
                    owner.time(),
                    consumed_time,
                    "Selected banks and late clones share the consumed frame"
                );
            }
            assert_eq!(owner.owners(), 3);
            // Identical cached loads preserve the decoder, voice, clock and seed.
            encoded.set_position(0);
            let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
                panic!();
            };
            let command::RuntimeCommand::LAYERLOAD(runtime) =
                runtime.into_runtime_form(&VmCtx::new(0, 42))
            else {
                panic!();
            };
            let info = runtime.apply_state(&mut vm);
            assert!(info.skip_layerloader_creation);
            let seed_calls = crate::engine_random::seed_call_count();
            let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(mut cached)) =
                runtime.start(&mut update, scenario, &vm, info, adv)
            else {
                panic!();
            };
            assert!(cached.load_task.is_none());
            assert!(
                cached
                    .update(&mut update, scenario, &mut vm, adv, false)
                    .is_some()
            );
            drop(cached);
            assert_eq!(crate::engine_random::seed_call_count(), seed_calls + 38);
            assert_eq!(owner.owners(), 3);
            for bank in banks {
                let UserLayer::Movie(layer) = adv.plane_layer_group(plane).get_layer(bank).unwrap()
                else {
                    panic!();
                };
                assert_eq!(
                    layer.reference_publication_owner().identity(),
                    owner.identity()
                );
                assert_eq!(layer.reference_dissolve_seed(), dissolve_seed);
                assert_eq!(layer.reference_still_state().0, still.0);
                assert_eq!(layer.reference_still_state().1, still.1);
                assert_ne!(layer.reference_still_state().2, still.2);
            }
            if !has_audio {
                assert_eq!(owner.time(), consumed_time);
            }
            if has_audio {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while adv.audio_manager.kira_manager().lock().num_sounds() == 0 {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Movie voice was not installed"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert_eq!(
                    adv.audio_manager.kira_manager().lock().num_sounds(),
                    1,
                    "All selected/cached movie layers share one AAC voice"
                );
            }
            if repeat != 0 {
                for bank in banks {
                    adv.plane_layer_group_mut(plane)
                        .get_layer_mut(bank)
                        .unwrap()
                        .fast_forward();
                }
                late_alias.fast_forward();
                assert_eq!(
                    owner.owners(),
                    3,
                    "Repeating movie skip preserves all owners"
                );
                if !has_audio {
                    // Cross door.mp4's native 5.005second/60000timescale boundary.
                    let mut next = crate::update::AdvUpdateContext {
                        delta_ticks: Ticks::from_seconds(5.1),
                        ..advance
                    };
                    next.frame_id.advance();
                    late_alias.update(&next);
                    let continued_time = owner.time();
                    assert!(continued_time > 300300);
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while owner.frame_start_time().is_none_or(|time| time < 300300) {
                        next.frame_id.advance();
                        next.delta_ticks = Ticks::ZERO;
                        late_alias.update(&next);
                        assert!(
                            std::time::Instant::now() < deadline,
                            "Repeating selected-bank movie did not rewind decoder"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    assert_eq!(
                        owner.time(),
                        continued_time,
                        "Decoder rewind preserves shared movie clock"
                    );
                    frame_id = next.frame_id;
                }
                vm.layers.transition.duration = Ticks::from_f32(4.0);
                vm.layers.transition.ty = 0;
                let mut replacement = std::io::Cursor::new(Vec::new());
                0xc1u8.write_le(&mut replacement).unwrap();
                for value in [-5, LayerType::Null as i32, LayerLoadFlags::AUTO_WIPE.bits()] {
                    NumberSpec::constant(value)
                        .write_le(&mut replacement)
                        .unwrap();
                }
                0u8.write_le(&mut replacement).unwrap();
                replacement.set_position(0);
                let Instruction::Command(runtime) = Instruction::read_le(&mut replacement).unwrap()
                else {
                    panic!();
                };
                let command::RuntimeCommand::LAYERLOAD(runtime) =
                    runtime.into_runtime_form(&VmCtx::new(0, 42))
                else {
                    panic!();
                };
                let info = runtime.apply_state(&mut vm);
                let CommandStartResult::Yield(super::super::ExecutingCommand::LAYERLOAD(
                    mut replacing,
                )) = runtime.start(&mut update, scenario, &vm, info, adv)
                else {
                    panic!();
                };
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !replacing.reference_published() {
                    assert!(
                        replacing
                            .update(&mut update, scenario, &mut vm, adv, false)
                            .is_none()
                    );
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Movie replacement wipe publication timed out"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                for bank in banks {
                    assert!(adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
                }
                assert_eq!(
                    owner.owners(),
                    3,
                    "Replacement wipe sources retain both movie banks"
                );
                adv.plane_layer_group_mut(plane).pre_render(
                    update.pre_render,
                    &crate::layer::render_params::TransformParams::default(),
                );
                let mut copied = adv
                    .plane_layer_group(plane)
                    .render_clone(&mut update.pre_render.render_clone_ctx());
                assert_eq!(
                    owner.owners(),
                    5,
                    "Copied active wipe retains two further movie sources"
                );
                frame_id.advance();
                let mut wipe_update = crate::update::AdvUpdateContext {
                    frame_id,
                    delta_ticks: Ticks::from_f32(2.0),
                    are_animations_allowed: true,
                    ..advance
                };
                adv.plane_layer_group_mut(plane).update(&wipe_update);
                let consumed_time = owner.time();
                copied.update(&wipe_update);
                if !has_audio {
                    assert_eq!(
                        owner.time(),
                        consumed_time,
                        "Copied wipe sources cannot duplicate shared playback update"
                    );
                }
                assert!(
                    replacing
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_none()
                );
                wipe_update.frame_id.advance();
                adv.plane_layer_group_mut(plane).update(&wipe_update);
                assert!(
                    replacing
                        .update(&mut update, scenario, &mut vm, adv, false)
                        .is_some()
                );
                for bank in banks {
                    assert!(!adv.plane_layer_group(plane).has_wiper_for_layerbank(bank));
                }
                assert_eq!(
                    owner.owners(),
                    3,
                    "Original wipe completion releases only its own source banks"
                );
                for bank in banks {
                    assert!(copied.has_wiper_for_layerbank(bank));
                }
                copied.update(&wipe_update);
                assert_eq!(
                    owner.owners(),
                    1,
                    "Copied scene independently retires its source movie banks"
                );
                drop(copied);
                drop(replacing);
                drop(late_alias);
            } else {
                adv.plane_layer_group_mut(plane)
                    .get_layer_mut(banks[0])
                    .unwrap()
                    .fast_forward();
                assert_eq!(owner.owners(), 2);
                let UserLayer::Movie(layer) = &late_alias else {
                    panic!();
                };
                assert!(!layer.is_finished());
                late_alias.fast_forward();
                assert_eq!(owner.owners(), 1);
                adv.plane_layer_group_mut(plane)
                    .get_layer_mut(banks[1])
                    .unwrap()
                    .fast_forward();
            }
            owner.assert_released();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while adv.audio_manager.kira_manager().lock().num_sounds() != 0 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Last movie bank leaked audio capacity"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            for bank in banks {
                adv.plane_layer_group_mut(plane)
                    .remove_layer(bank, Ticks::ZERO);
            }
        }
        adv.back_layer_group = None;
        println!(
            "Native Movie publication: gated worker prototype, two selected/cached banks share player/voice/clock/seed, independent properties, once-per-frame late clones,14/38 publication draws, independent skip, repeating decoder rewind, original/copied replacement-wipe source retirement and final audio-owner release pass"
        );
    }

    #[test]
    fn replacements_preserve_properties_and_same_assets_keep_load_order() {
        let mut state = VmState::new();
        let initial = load(7, 2, 100).apply_state(&mut state);
        let bank = initial.affected_layers[0].operation_target.layerbank;
        assert!(!initial.skip_layerloader_creation);
        assert!(
            !initial.affected_layers[0].keep_old_props,
            "Empty bank has no old properties"
        );
        assert_eq!(state.layers.layer_load_counter, 1);
        state.layers.layerbanks[bank]
            .properties
            .set_property(LayerProperty::TranslateX, 345);
        let replacement = load(7, 2, 101).apply_state(&mut state);
        assert!(replacement.affected_layers[0].keep_old_props);
        assert_eq!(
            state.layers.layerbanks[bank]
                .properties
                .get_property(LayerProperty::TranslateX),
            345
        );
        assert_eq!(replacement.affected_layers[0].layer_load_counter1, 1);
        assert_eq!(state.layers.layer_load_counter, 2);
        let same = load(7, 3, 101).apply_state(&mut state);
        assert!(same.skip_layerloader_creation);
        assert!(same.affected_layers[0].already_the_same);
        assert_eq!(same.affected_layers[0].layer_load_counter1, 1);
        assert_eq!(state.layers.layer_load_counter, 2);
        let nonblocking_replacement = load(7, 3, 102).apply_state(&mut state);
        assert!(!nonblocking_replacement.skip_layerloader_creation);
        assert!(nonblocking_replacement.affected_layers[0].keep_old_props);
        assert_eq!(
            nonblocking_replacement.affected_layers[0].layer_load_counter1,
            2
        );
        assert_eq!(state.layers.layer_load_counter, 3);
        let reset = load(7, 0, 102).apply_state(&mut state);
        assert!(reset.skip_layerloader_creation);
        assert!(!reset.affected_layers[0].keep_old_props);
        assert_eq!(
            reset.affected_layers[0].layer_id_for_properties,
            LayerId::new(7)
        );
        assert_eq!(
            state.layers.layerbanks[bank]
                .properties
                .get_property(LayerProperty::TranslateX),
            0
        );
        assert_eq!(state.layers.layer_load_counter, 3);
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_invalid_interactive_factory_does_not_publish_empty_layer() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x849b4, 0xb9400c08u32),
            (0x849b8, 0x71000d1f),
            (0x849bc, 0x54000308),
            (0x84a1c, 0xf900027f),
            (0x92f4, 0xb40002a0),
            (0x938c, 0xb4000940),
            (0x94b4, 0xf9002fff),
            (0x96b8, 0x9401a836),
            (0x973c, 0xc8dffea0),
            (0x9740, 0xf9400008),
            (0x9744, 0xf9403108),
            (0x974c, 0xd63f0100),
            (0x95d0, 0xc8dffea0),
            (0x95d4, 0xf9400008),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native invalid interactive factory instruction {address:x}"
            );
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_cached_interactive_copy_and_publication_without_controller_reset() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x92d8, 0x340002a8u32),
            (0x92e4, 0xf9401509),
            (0x92ec, 0xd63f0120),
            (0x930c, 0x94044dfd),
            (0x936c, 0x9400b585),
            (0x93a4, 0x94044dd7),
            (0x95d8, 0xf9403108),
            (0x95e0, 0xd63f0100),
            (0xc7e54, 0xb94046a8),
            (0xc7e58, 0xb9004668),
            (0xc7e5c, 0xb94042a8),
            (0xc7e60, 0xb9004268),
            (0x44800, 0xb940a2a8),
            (0x44804, 0xb900a268),
            (0x44808, 0xb940a6a8),
            (0x4480c, 0xb900a668),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native cached publication instruction {address:x}"
            );
        }
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        assert_eq!(
            relocation(0x224518),
            0x1d7cc8,
            "Publication casts DrawableLayer"
        );
        assert_eq!(
            relocation(0x224540),
            0x1d42e0,
            "Publication casts BustupLayer"
        );
        for (got, result) in [
            (0x2251d8, 0xc8670),
            (0x224a28, 0x452f0),
            (0x224cb0, 0x6f910),
        ] {
            assert_eq!(relocation(relocation(got) + 16 + 0x80), result);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_murderer_retained_owner_frame_guard_and_property_rebinding() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x6aa60, 0x91002289u32),
            (0x6aa70, 0x6b00011f),
            (0x6aa74, 0x540000c1),
            (0x9b050, 0xf90007e0),
            (0x9b058, 0xf8410c08),
            (0x9b060, 0xd63f0100),
            (0x9b078, 0xf9000269),
            (0x9b170, 0xf9402a68),
            (0x9b174, 0xbd005e60),
            (0x9b180, 0xa904fe68),
            (0x9b260, 0x9100e008),
            (0x9b268, 0xc8dffd00),
            (0x75bac, 0x91008008),
            (0x75bb4, 0xc85ffd13),
            (0x75bb8, 0xc809fd00),
            (0x95d8, 0xf9403108),
            (0x95e0, 0xd63f0100),
            // Child capture composes parent properties; the wiper receives viewport textures.
            (0x9b2fc, 0xf9403509),
            (0x9b318, 0xd63f0120),
            (0x9b33c, 0x97ff5985),
            (0x9b3a8, 0x97ff59f2),
            (0x9b404, 0xf9402680),
            (0x9b418, 0x910123e1),
            (0x9b41c, 0xd63f0100),
            (0x9b4e8, 0x9100e268),
            (0x9b4f4, 0x9100c268),
            (0x9b508, 0xf9402108),
            (0x9b50c, 0xaa0003e2),
            (0x9b514, 0xaa1503e1),
            (0x9b518, 0xd63f0100),
            // Completion releases wiper+40 and frozen texture+30.
            (0x9b1d8, 0xf9401908),
            (0x9b1e0, 0x370002a0),
            (0x9b1e8, 0xc808febf),
            (0x9b1fc, 0xd63f0100),
            (0x9b200, 0x9100c268),
            (0x9b208, 0xc809fd1f),
            (0x9b230, 0xd61f0020),
            // Destruction releases pending/active children, wiper, live/frozen captures.
            (0x9ae34, 0xf9402a60),
            (0x9ae4c, 0xf9402660),
            (0x9aef4, 0x91010268),
            (0x9aefc, 0xc809fd1f),
            (0x9af08, 0xf8410c08),
            (0x9af10, 0xd63f0100),
            (0x9af14, 0x9100e268),
            (0x9af1c, 0xc809fd1f),
            (0x9af30, 0xd63f0100),
            (0x9af34, 0x9100c268),
            (0x9af3c, 0xc809fd1f),
            (0x9af50, 0xd63f0100),
            (0x9af60, 0x17ff6ad0),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native retained-owner instruction {address:x}"
            );
        }
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        let primary = relocation(0x224f08) + 16;
        assert_eq!(relocation(primary), 0x9ae00, "Retained owner destructor");
        assert_eq!(relocation(primary + 8), 0x9afe0, "Deleting destructor");
        assert_eq!(relocation(primary + 0x10), 0x6aa40);
        assert_eq!(relocation(primary + 0x18), 0x9b130);
        assert_eq!(relocation(primary + 0x28), 0x9b040);
        assert_eq!(relocation(primary + 0x40), 0x9b250);
        assert_eq!(relocation(primary + 0x60), 0x75ba0);
        assert_eq!(relocation(primary + 0x68), 0x75c00);
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_interactive_factories_plain_base_and_controller_copy_semantics() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x849b4, 0xb9400c08u32),
            (0x849b8, 0x71000d1f),
            (0x849bc, 0x54000308),
            (0x849e4, 0x94010b3f), // Quiz factoryc76e0
            (0x84a38, 0x97fefd56), // Cake factory43f90
            (0x84a7c, 0x940057ad), // Murderer factory9a930
            (0x84abc, 0x97ffa219), // Last factory6d320
            (0xc777c, 0x97feb871),
            (0x4400c, 0x9400c64d),
            (0x9a9b8, 0x97ff6be2),
            (0x6d398, 0x9400216a), // DrawableLayer75940
            (0xc80c8, 0x97ffff3e), // Quiz clone invokes copyc7dc0
            (0xc7e48, 0x9400057e), // copies owned button sequence
            (0xc7e4c, 0xb9404ea8),
            (0xc7e50, 0xb9004e68), // copies state scalar
            (0x44aa8, 0x97ffff02), // Cake clone invokes copy446b0
            (0x44748, 0x9400088a),
            (0x44750, 0xf9002268), // copies owned state
            (0x6e538, 0x97fffdda), // Last clone invokes copy6dca0
            (0x9b050, 0xf90007e0),
            (0x9b078, 0xf9000269), // Murderer retains same owner
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native interactive instruction {address:x}"
            );
        }
        assert_eq!(
            &executable[0x170106 + 256..0x17010a + 256],
            &[0, 22, 38, 55]
        );
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        assert_eq!(relocation(0x1d8348), 0x849a0);
        assert_eq!(relocation(0x1d8338), 0x82340);
        for (got, clone) in [
            (0x2251d8, 0xc80a0),
            (0x224a28, 0x44a80),
            (0x224f08, 0x9b040),
            (0x224cb0, 0x6e510),
        ] {
            let table = relocation(got);
            assert_eq!(relocation(table + 0x38), clone);
            let type_info = relocation(table + 8);
            assert_eq!(
                relocation(type_info + 16),
                0x1d1740,
                "InteractiveLayer parent"
            );
        }
        assert_eq!(relocation(0x1d1740 + 16), 0x1d7cc8, "DrawableLayer parent");
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_movie_selected_bank_copy_and_shared_playback_owner() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x83f48, 0x91018288u32),
            (0x83f58, 0xf9401501),
            (0x83f64, 0xd61f0020),
            (0xc3238, 0x97ffff02),
            (0xc2e5c, 0x97fd88b1),
            (0xc2e88, 0x39418288),
            (0xc2e90, 0x39418688),
            (0xc2e98, 0x9101a288),
            (0xc2e9c, 0xc8dffd00),
            (0xc2eb8, 0xd63f0100),
            (0xc2ed0, 0x9101c288),
            (0xc2ee4, 0xf9401509),
            (0xc2ef0, 0xd63f0120),
            (0xc34d4, 0x9101a296),
            (0xc34e4, 0xf8410c08),
            (0xc34e8, 0xf9400908),
            (0xc34f4, 0xd63f0100),
            (0xc3574, 0xc85ffec0),
            (0xc3578, 0xc808fedf),
            (0xc358c, 0xd63f0100),
            // Linked PictureLayer owns a seeded base before MovieLayer/decoder construction.
            (0x83a4c, 0x940105a5),
            (0x83b9c, 0x9400fc09),
            (0xc516c, 0x97fec339),
            (0xc2c74, 0x97fd891b),
            (0x250f0, 0x94014358),
            (0x75efc, 0x940216f1),
            // Child updates independently and draws into the parent's fallback source pass.
            (0xc3590, 0x9101c288),
            (0xc35a4, 0xf9400908),
            (0xc35b0, 0xd63f0100),
            (0xc3934, 0xc8dffd08),
            (0xc3944, 0x9101c008),
            (0xc3950, 0x1a9f07e0),
            (0xc3ae0, 0x9101c268),
            (0xc3af4, 0xf9402508),
            (0xc3b04, 0xd63f0100),
            (0xc3b24, 0xd63f0100),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native Movie ownership instruction {address:x}"
            );
        }
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        let movie = relocation(0x225180) + 16;
        assert_eq!(movie, 0x1db8b8);
        assert_eq!(relocation(movie + 0x28), 0xc3210);
        assert_eq!(relocation(movie + 0x18), 0xc34a0);
        let shared_update = relocation(0x225178) + 0x48;
        assert_eq!(relocation(shared_update + 0x10), 0x6aa40);
        assert_eq!(relocation(shared_update + 0x18), 0xc2b10);
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_procedural_factories_and_movie_prototype_publication() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x8439c, 0x97ff89e9u32), // FocusLine publication constructs66b40
            (0x84620, 0x940114e8),    // Rain publication constructsc99c0
            (0x8488c, 0x97fe82ad),    // Animation publication constructs25340
            (0x84254, 0xf9000ea8),    // FocusLine descriptor stores params
            (0x844c8, 0x290352f6),    // Rain descriptor stores mode/min
            (0x844cc, 0xb90022f3),    // Rain descriptor stores max
            (0x2559c, 0x94035949),    // Animation derived seed
            (0x66bfc, 0x940253b1),    // FocusLine derived seed
            (0xc9a94, 0x9400c80b),    // Rain derived seed
            (0x83f3c, 0x97fe92a9),    // Movie task status
            (0x83f40, 0x7100081f),    // Movie complete2
            (0x83f48, 0x91018288),    // Movie prototype source+60
            (0x83f58, 0xf9401501),    // Movie prototype virtual+28 clone
            (0x83f64, 0xd61f0020),    // Movie clone dispatch
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native loader instruction {address:x}"
            );
        }
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        for (type_slot, publish, name) in [
            (
                0x1d81f8,
                0x84330,
                b"N4shin20FocusLineLayerLoaderE".as_slice(),
            ),
            (0x1d8250, 0x845b0, b"N4shin15RainLayerLoaderE".as_slice()),
            (
                0x1d82a8,
                0x84820,
                b"N4shin20AnimationLayerLoaderE".as_slice(),
            ),
            (0x1d8148, 0x83f20, b"N4shin16MovieLayerLoaderE".as_slice()),
        ] {
            assert_eq!(relocation(type_slot + 0x48), publish);
            if type_slot != 0x1d8148 {
                assert_eq!(
                    relocation(type_slot + 0x38),
                    0x82340,
                    "Procedural factory is immediately ready"
                );
            }
            let type_info = relocation(type_slot);
            let name_address = relocation(type_info + 8) as usize + 256;
            assert_eq!(&executable[name_address..name_address + name.len()], name);
            assert_eq!(executable[name_address + name.len()], 0);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_picture_bustup_worker_prototypes_and_publication_clones() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x82a90, 0x97fef7e8u32), // Picture worker loads resource
            (0x82ab4, 0x9401098b),    // constructs prototype via c50e0
            (0x82abc, 0x9100e268),    // loader+38 prepared layer
            (0x82ac4, 0xc809fd00),    // stores prototype
            (0x82d7c, 0x97fe9719),    // reads task status
            (0x82d80, 0x7100081f),    // requires completed status2
            (0x82d88, 0x9100e288),    // retrieves prepared layer
            (0x82d98, 0xf9401501),    // prepared layer virtual+28 clone
            (0x82da4, 0xd61f0020),    // tailcalls clone
            (0x83260, 0x97fec844),    // Bustup worker constructs prototype35370
            (0x83268, 0x91016268),    // loader+58 prepared layer
            (0x83270, 0xc809fd00),    // stores prototype
            (0x8353c, 0x97fe9529),    // reads task status
            (0x83540, 0x7100081f),    // requires completed status2
            (0x83548, 0x91016288),    // retrieves prepared layer
            (0x83558, 0xf9401501),    // prepared layer virtual+28 clone
            (0x83564, 0xd61f0020),    // tailcalls clone
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native prototype instruction {address:x}"
            );
        }
        // Resolve the executable's relative relocations to pin these publication
        // functions to their named loader vtables, rather than unrelated code.
        let relocation = |slot: u64| {
            executable[0x11e040 + 256..0x152eb8 + 256]
                .chunks_exact(24)
                .find_map(|entry| {
                    (u64::from_le_bytes(entry[..8].try_into().unwrap()) == slot)
                        .then(|| u64::from_le_bytes(entry[16..24].try_into().unwrap()))
                })
                .unwrap()
        };
        for (type_slot, publish_slot, publish, name) in [
            (
                0x1d8098,
                0x1d80e0,
                0x82d60,
                b"N4shin18PictureLayerLoaderE".as_slice(),
            ),
            (
                0x1d80f0,
                0x1d8138,
                0x83520,
                b"N4shin17BustupLayerLoaderE".as_slice(),
            ),
        ] {
            assert_eq!(relocation(publish_slot), publish);
            let type_info = relocation(type_slot);
            let name_address = relocation(type_info + 8) as usize + 256;
            assert_eq!(&executable[name_address..name_address + name.len()], name);
            assert_eq!(executable[name_address + name.len()], 0);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_layer_load_preservation_and_cache_order_instructions() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let path = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(path).unwrap();
        for (address, expected) in [
            (0x8ad4, 0xb97de929u32), // cached bank retains its load counter
            (0x8ae4, 0x36080228),    // property preservation gates on flag bit1
            (0x8af0, 0x39018109),    // keep-old-properties marker
            (0x8b04, 0xb93de949),    // changed bank stores current counter
            (0x8b18, 0x1100054a),    // changed asset increments next counter
            (0x8b20, 0x3910427f),    // changed asset requires a loader
            (0x92d8, 0x340002a8),    // cached layer bypasses fresh factory output
            (0x9338, 0xf9402109),
            (0x9340, 0xd63f0120),
            (0x82340, 0x320003e0),
            (0x84134, 0x97ff7c57),
            (0x825f0, 0x94020f28),
            (0x823cc, 0x94010785),
            (0x94dc, 0x34000ec8),
            (0x94f0, 0xd63f0120),
            (0x9450, 0x37180b48),
            (0x945c, 0x9401ad2d),
            (0x96b8, 0x9401a836),
        ] {
            let actual =
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap());
            assert_eq!(actual, expected, "Native instruction at {address:x}");
        }
    }
}
