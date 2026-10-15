//! Updated c0d0/c520/c5ac TIPSGET command, separate from its drawn type12
//! widget and optional input wrapper/ChoiceScreen child.
use super::{
    autosave::{AutosaveMemory, SaveProgress},
    layers::{HigurashiLayers, LayerBanks, Target},
    progress::ProgressMemory,
};
use anyhow::{Result, bail, ensure};
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipsGetRequest {
    /// Original signed32 operands, including order and duplicate IDs.
    pub ids: Vec<i32>,
}
impl TipsGetRequest {
    /// c5fc/c600: alternate byte, count, then original signed32 IDs. Alternate
    /// is command state sampled by replay; a fresh live command suppliesfalse.
    pub fn producer_params(&self, alternate: bool) -> [i32; 8] {
        let mut params = [0; 8];
        params[0] = i32::from(alternate);
        params[1] = self.ids.len() as i32;
        params[2..2 + self.ids.len()].copy_from_slice(&self.ids);
        params
    }
}

/// c710..c984's owned scene-copy payload. The app service adds the unchanged
/// live VM date to these complete banks, then invokes dcf30(data,selected).
/// This is in-memory GameData ownership; no disk projection is involved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipsSceneRequest {
    pub progress: SaveProgress,
    pub trailing_values: Vec<u16>,
    pub byte_variables: Vec<u8>,
    pub selected: i32,
}
impl TipsSceneRequest {
    fn copy(source: &ProgressMemory, boundary: u32, ids: &[i32]) -> Self {
        let mut progress = source.progress().clone();
        let active = progress.active_index();
        progress.entries[active].save_position = boundary;
        progress.entries[active].replay_target = boundary;
        // c7d8/c7dc clears only the copy's old trailing vector. STRH truncates
        // each requested operand; duplicates/order survive the replacement.
        let trailing_values: Vec<_> = ids.iter().map(|id| *id as u16).collect();
        let selected = trailing_values.first().copied().map_or(0, i32::from);
        Self {
            progress,
            trailing_values,
            byte_variables: source.bytes().to_vec(),
            selected,
        }
    }
}

pub trait TipsGetServices {
    /// Retain one asset-only producer, then construct the actual kind12 layer
    /// and distinct wrapper on the main poll with real global settings. Publish
    /// front drawable before optional root child0; false retains readiness.
    fn prepare(
        &mut self,
        request: &TipsGetRequest,
        alternate: bool,
        target: Target,
        layers: &mut HigurashiLayers,
    ) -> Result<bool>;
    /// Actual wrapper callback delivery. Negative values remain pending;
    /// EXACT1 queues the TIPS scene, any other nonnegative value closesnormally.
    fn poll(&mut self) -> Result<Option<i32>>;
    fn packed_time(&mut self) -> Result<u64>;
    /// cd58..cde8 runs after initial11780 and before polling the callback: when
    /// active signedscenario<=9999, inspect normal descriptors' acquired bits
    /// and invoke e8eb0(38) if allpresent. e8eb0 is a mutex-only Switch service.
    /// The service owns the real catalog/global bits; it does not acquire them.
    fn opening_copied(&mut self, source: &ProgressMemory) -> Result<()>;
    /// Complete independent current-VM scene copy, including the unchangeddate
    /// supplied by the app. dcf30 supplies alternate0; it does not take a new
    /// date, commit queuedvariables, write a response or mutate the source.
    /// Success retains a pending factory; the source command remains blocked.
    fn queue_scene(&mut self, request: &TipsSceneRequest) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    OpeningCopy,
    OpeningGate,
    Waiting,
    QueueScene,
    FinalCopy,
    Complete,
}

#[derive(Debug)]
pub struct TipsGetCommand {
    request: TipsGetRequest,
    reservation: Option<Target>,
    replay_stop: i32,
    alternate: bool,
    result: Option<i32>,
    phase: Phase,
    copy_started: bool,
    scene_request: Option<TipsSceneRequest>,
}
impl TipsGetCommand {
    /// Caller performs1185c+wrappingboundary increment without clearingtext or
    /// resettingresume/replay first. Reserve after that acknowledged prefix.
    pub(in crate::adv::higurashi) fn new(command: &Cmd, layers: &mut LayerBanks) -> Result<Self> {
        let Cmd::TIPSGET { ids } = command else {
            bail!("TipsGet owner requires TIPSGET");
        };
        ensure!(
            ids.len() <= 6,
            "TIPSGET exceeds original six-operand payload"
        );
        Ok(Self {
            request: TipsGetRequest { ids: ids.to_vec() },
            reservation: Some(layers.reserve_temporary()?),
            replay_stop: -1,
            alternate: false,
            result: None,
            phase: Phase::Loading,
            copy_started: false,
            scene_request: None,
        })
    }
    pub fn request(&self) -> &TipsGetRequest {
        &self.request
    }
    pub fn target(&self) -> Option<Target> {
        self.reservation
    }
    pub fn replay_stop(&self) -> i32 {
        self.replay_stop
    }
    pub fn alternate(&self) -> bool {
        self.alternate
    }
    pub fn result(&self) -> Option<i32> {
        self.result
    }
    /// c59c: forward search stops on unanswered/stop0 commands.
    pub fn forward_complete(&self) -> bool {
        self.replay_stop > 0
    }
    /// Caller must retire wrapper then original front drawable byidentity
    /// before releasing this mapping, matching c300/c39c/c3d8. Scene replacement
    /// drops the old banks instead of freeing an old reservation in newbanks.
    pub(in crate::adv::higurashi) fn release(&mut self, layers: &mut LayerBanks) {
        if let Some(target) = self.reservation.take() {
            layers.release_temporary(target);
        }
    }
    /// c520 performs both UNSIGNED boundary tests on each invocation. There is
    /// no lifetime poll latch; its first branch resamples fullADV28!=0. No
    /// response/register/global/date/entropy service is due here.
    pub fn poll_replay(
        &mut self,
        progress: &ProgressMemory,
        boundary: &mut u32,
        constructor_word: u32,
    ) -> Result<bool> {
        ensure!(
            self.phase == Phase::Loading,
            "TIPSGET live effects must finish before replay"
        );
        let saved = progress.progress().active_entry().save_position;
        if *boundary >= saved {
            self.replay_stop = 0;
            self.alternate = constructor_word != 0;
            return Ok(false);
        }
        *boundary = boundary.wrapping_add(1);
        if *boundary >= saved {
            self.replay_stop = 1;
            Ok(false)
        } else {
            Ok(true)
        }
    }
    fn copy(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl TipsGetServices,
    ) -> Result<()> {
        if !self.copy_started {
            let active = progress.progress().active_index();
            let entry = &mut progress.progress_mut().entries[active];
            entry.save_position = boundary;
            entry.replay_target = boundary;
            memory.valid = true;
            self.copy_started = true;
        }
        let time = services.packed_time()?;
        memory.capture(progress.progress(), time);
        self.copy_started = false;
        Ok(())
    }
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: &mut u32,
        layers: &mut HigurashiLayers,
        services: &mut impl TipsGetServices,
    ) -> Result<bool> {
        // c5d0: only POSITIVE savedstop skips loading/copies; stop0 resumesUI.
        if self.replay_stop > 0 {
            return Ok(true);
        }
        if self.phase == Phase::Loading {
            if !services.prepare(
                &self.request,
                self.alternate,
                self.reservation.expect("Retained TIPSGET reservation"),
                layers,
            )? {
                return Ok(false);
            }
            self.phase = Phase::OpeningCopy;
        }
        if self.phase == Phase::OpeningCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::OpeningGate;
        }
        if self.phase == Phase::OpeningGate {
            services.opening_copied(progress)?;
            self.phase = Phase::Waiting;
        }
        if self.phase == Phase::Waiting {
            let Some(value) = services.poll()?.filter(|value| *value >= 0) else {
                return Ok(false);
            };
            self.result = Some(value);
            if value == 1 {
                self.phase = Phase::QueueScene;
            } else {
                *boundary = boundary.wrapping_add(1);
                self.phase = Phase::FinalCopy;
            }
        }
        if self.phase == Phase::QueueScene {
            let request = self.scene_request.get_or_insert_with(|| {
                TipsSceneRequest::copy(progress, *boundary, &self.request.ids)
            });
            services.queue_scene(request)?;
            // Repeated successful c5ac calls replace the queued factory with
            // another independent CURRENT copy. Failed desktop services retry
            // only their unfinished copy/queue prefix; successful calls do not
            // complete or retire the source script command.
            self.scene_request = None;
            return Ok(false);
        }
        if self.phase == Phase::FinalCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::Complete;
        }
        Ok(self.phase == Phase::Complete)
    }
}

#[cfg(test)]
mod tests;
