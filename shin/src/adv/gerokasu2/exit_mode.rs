//! Native script EXIT350/4730 -> ADV::ExitGame145D0 -> completion19034.
//! Root/message/UI/audio/save effects remain explicit parent services.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExitGates {
    pub hide_motion_busy: bool,
    pub message_busy: bool,
    pub tasks_idle: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitAction {
    /// Clear hide-message/other UI motions, disable input/modes, close the
    /// message with animation, stop voice and fade all SE (native minimum4).
    PrepareExit,
    /// Native F120 commits pending variables immediately before145D0 requests
    /// its save task. The host must await its completion before save_completed.
    RequestSave,
    /// Native38040(1) resets the voice-focus target, preserving script volume.
    RestoreBgmFocus,
    ReturnTitle {
        requested_id: i32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPhase {
    Pending,
    Waiting,
    SaveQueued,
    Complete,
}
pub struct ExitMode {
    phase: ExitPhase,
    remaining: f32,
}
impl ExitMode {
    pub fn for_command(argument: i32) -> Option<Self> {
        (argument == 0).then_some(Self {
            phase: ExitPhase::Pending,
            remaining: 0.,
        })
    }
    pub fn phase(&self) -> ExitPhase {
        self.phase
    }
    pub fn remaining(&self) -> f32 {
        self.remaining
    }
    /// Native4730 polls gates before the parent's WAIT/tween updates. Charge
    /// this frame's delta through advance_clock after polling, including entry.
    pub fn poll(&mut self, gates: ExitGates) -> Vec<ExitAction> {
        let mut actions = vec![];
        if self.phase == ExitPhase::Pending {
            actions.push(ExitAction::PrepareExit);
            self.remaining = 30.;
            self.phase = ExitPhase::Waiting;
        }
        if self.phase == ExitPhase::Waiting
            && self.remaining <= 0.
            && !gates.hide_motion_busy
            && !gates.message_busy
            && gates.tasks_idle
        {
            self.phase = ExitPhase::SaveQueued;
            actions.push(ExitAction::RequestSave);
        }
        actions
    }
    /// Native10DB8 subtracts once when positive and retains negative overshoot.
    /// Suspension/pause permission is decided by the caller, never inferred.
    pub fn advance_clock(&mut self, delta: f32) {
        if delta.is_finite() && delta >= 0. && self.remaining > 0. {
            self.remaining -= delta;
        }
    }
    pub fn save_completed(&mut self) -> Vec<ExitAction> {
        if self.phase != ExitPhase::SaveQueued {
            return vec![];
        }
        self.phase = ExitPhase::Complete;
        vec![
            ExitAction::RestoreBgmFocus,
            ExitAction::ReturnTitle { requested_id: -1 },
        ]
    }
    /// Desktop save-error retry boundary; native modal/error owner is separate.
    pub fn retry_save(&mut self) {
        if self.phase == ExitPhase::SaveQueued {
            self.phase = ExitPhase::Waiting;
        }
    }
}
#[cfg(test)]
#[path = "exit_mode/tests.rs"]
mod tests;
