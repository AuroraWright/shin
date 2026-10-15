//! Original LAYERLOAD constructor decisions and five retained polling phases.
use anyhow::{Result, ensure};
use shin_core::vm::{
    command::types::{LAYERBANKS_COUNT, LayerId, LayerbankId, PlaneId},
    gerokasu2::Gerokasu2RuntimeCommand,
};

use super::layers::Gerokasu2Layers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadTarget {
    pub bank: LayerbankId,
    pub logical: LayerId,
    pub keep_properties: bool,
    pub already_same: bool,
    pub load_counter: u32,
}

/// The live command constructor has per-bank resource counters in addition to
/// the integer/save target state. Bootstrap/restoration supplies these values.
pub struct LoadCounters {
    next: u32,
    banks: [u32; LAYERBANKS_COUNT],
}
impl LoadCounters {
    pub fn new(next: u32, banks: [u32; LAYERBANKS_COUNT]) -> Self {
        Self { next, banks }
    }
    /// Seed live loading from replayed resource generations. Property-reset
    /// counts are independent and cannot identify a reusable drawable.
    pub fn from_layers(layers: &Gerokasu2Layers) -> Self {
        Self::new(
            layers.next_initialization_id(),
            std::array::from_fn(|bank| {
                layers
                    .record(LayerbankId::new(bank as u8))
                    .initialization_id()
            }),
        )
    }
    pub fn next(&self) -> u32 {
        self.next
    }
    pub fn bank(&self, id: LayerbankId) -> u32 {
        self.banks[usize::from(id.raw())]
    }
}

#[derive(Debug, Clone)]
pub struct LoadPlan {
    pub plane: PlaneId,
    pub kind: i32,
    pub flags: i32,
    pub params: [i32; 8],
    pub targets: Vec<LoadTarget>,
    pub needs_loader: bool,
}
impl LoadPlan {
    pub fn prepare(
        layers: &mut Gerokasu2Layers,
        counters: &mut LoadCounters,
        command: &Gerokasu2RuntimeCommand,
    ) -> Result<Self> {
        let Gerokasu2RuntimeCommand::LayerLoad {
            layer_id,
            layer_type,
            flags,
            params,
        } = command
        else {
            anyhow::bail!("Expected Gerokasu2 LAYERLOAD")
        };
        let params = [
            params.0, params.1, params.2, params.3, params.4, params.5, params.6, params.7,
        ];
        let mut next = layers.clone();
        let targets = next.load(*layer_id, *layer_type, *flags, params)?;
        let mut result = Self {
            plane: layers.current_plane(),
            kind: *layer_type,
            flags: if layers.back_started() {
                *flags & !4
            } else {
                *flags
            },
            params,
            targets: Vec::new(),
            needs_loader: false,
        };
        for target in &targets {
            let old = layers.record(target.layerbank);
            // Native84dc treats an existing type9 as reusable even if its
            // constructor parameter words change; ordinary kinds compare all8.
            let same =
                old.kind() == *layer_type && (*layer_type == 9 || old.parameters() == params);
            let keep = old.kind() >= 0 && *flags & 2 != 0;
            let bank = usize::from(target.layerbank.raw());
            if !same {
                counters.banks[bank] = counters.next;
                counters.next = counters.next.wrapping_add(1);
                result.needs_loader = true;
            }
            result.targets.push(LoadTarget {
                bank: target.layerbank,
                logical: target.layer,
                keep_properties: keep,
                already_same: same,
                load_counter: counters.banks[bank],
            });
        }
        *layers = next;
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LoadTransition {
    pub kind: i32,
    pub flags: i32,
    pub duration: i32,
    pub params: [i32; 8],
}

/// Services own real resource/transition jobs and the retained drawable tree.
/// Publication must implement cloning/reuse, property retention/reset and the
/// plane/back-tree boundary; this controller never fabricates their success.
pub trait LoadServices {
    fn frame_elapsed_us(&self) -> u64;
    fn prepare_plan(
        &mut self,
        layers: &mut Gerokasu2Layers,
        command: &Gerokasu2RuntimeCommand,
    ) -> Result<LoadPlan>;
    fn start_resource(&mut self, plan: &LoadPlan) -> Result<()>;
    fn resource_present(&self) -> bool;
    fn wait_resource(&mut self, microseconds: u32) -> Result<()>;
    fn resource_ready(&mut self) -> Result<bool>;
    fn set_animations_allowed(&mut self, allowed: bool);
    fn animations_allowed(&self) -> bool;
    fn transition(&self) -> LoadTransition;
    fn configure_transition(&mut self, settings: LoadTransition) -> Result<()>;
    fn start_transition(&mut self, settings: LoadTransition) -> Result<()>;
    fn transition_present(&self) -> bool;
    fn wait_transition(&mut self, microseconds: u32) -> Result<()>;
    fn clear_transition(&mut self);
    fn transition_ready(&mut self) -> Result<bool>;
    fn skip_requested(&mut self) -> bool;
    fn publish(&mut self, plan: &LoadPlan, with_transition: bool) -> Result<()>;
    fn loaded_layer(&mut self, kind: i32, params: [i32; 8]) -> Result<()>;
    fn any_target_wiping(&mut self, targets: &[LoadTarget]) -> Result<bool>;
    /// Shared service backends release command-local acquisition owners when
    /// this retained continuation retires. Native poll field snapshots exclude
    /// the subsequent command destructor; fixtures can leave this hook empty.
    fn command_finished(&mut self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStage {
    Start,
    Resource,
    Transition,
    Publication,
    Wipes,
    Complete,
}
pub struct LayerLoadController {
    plan: LoadPlan,
    stage: LoadStage,
    published: bool,
    notified: bool,
    resource_started: bool,
    resource_wait_attempted: bool,
    transition_started: bool,
    transition_wait_attempted: bool,
}
impl LayerLoadController {
    pub fn new(plan: LoadPlan) -> Self {
        Self {
            plan,
            stage: LoadStage::Start,
            published: false,
            notified: false,
            resource_started: false,
            resource_wait_attempted: false,
            transition_started: false,
            transition_wait_attempted: false,
        }
    }
    pub fn plan(&self) -> &LoadPlan {
        &self.plan
    }
    pub fn stage(&self) -> LoadStage {
        self.stage
    }
    /// Native's optional5000us initial wait is attempted only on loader
    /// creation, when(frame elapsed_us >>4)<=624. Later polls only test ready.
    pub fn poll(&mut self, services: &mut (dyn LoadServices + '_)) -> Result<bool> {
        if self.stage == LoadStage::Complete {
            return Ok(true);
        }
        if self.stage == LoadStage::Start {
            if self.plan.needs_loader {
                if !self.resource_started {
                    services.start_resource(&self.plan)?;
                    self.resource_started = true;
                    if self.plan.flags & 1 == 0 {
                        services.set_animations_allowed(false);
                    }
                }
                if !self.resource_wait_attempted {
                    self.resource_wait_attempted = true;
                    if services.resource_present() && services.frame_elapsed_us() >> 4 <= 624 {
                        services.wait_resource(5000)?;
                    }
                }
            }
            self.stage = LoadStage::Resource;
        }
        if self.stage == LoadStage::Resource {
            if services.resource_present() && !services.resource_ready()? {
                return Ok(false);
            }
            services.set_animations_allowed(true);
            self.stage = LoadStage::Transition;
        }
        if self.stage == LoadStage::Transition {
            let transition = services.transition();
            if !self.transition_started && self.plan.flags & 4 != 0 && transition.duration >= 1 {
                services.start_transition(transition)?;
                self.transition_started = true;
                if transition.flags & 1 == 0 {
                    services.set_animations_allowed(false);
                }
            }
            if self.transition_started && !self.transition_wait_attempted {
                self.transition_wait_attempted = true;
                if services.transition_present() && services.frame_elapsed_us() >> 4 <= 624 {
                    services.wait_transition(5000)?;
                }
            }
            self.stage = LoadStage::Publication;
        }
        if self.stage == LoadStage::Publication {
            if !self.published {
                if services.skip_requested() {
                    services.clear_transition();
                }
                if services.transition_present() && !services.transition_ready()? {
                    return Ok(false);
                }
                let with_transition = services.transition_present();
                services.publish(&self.plan, with_transition)?;
                self.published = true;
            }
            if !self.notified {
                services.loaded_layer(self.plan.kind, self.plan.params)?;
                self.notified = true;
            }
            services.set_animations_allowed(true);
            self.stage = LoadStage::Wipes;
        }
        ensure!(
            self.stage == LoadStage::Wipes,
            "Gerokasu2 layer load stage is inconsistent"
        );
        // Native97a4 reads current transition flags rather than a creation-time
        // snapshot. An active wipe may release the command when DONT_WAIT changes.
        if services.transition().flags & 2 == 0 && services.any_target_wiping(&self.plan.targets)? {
            return Ok(false);
        }
        services.command_finished()?;
        self.stage = LoadStage::Complete;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
