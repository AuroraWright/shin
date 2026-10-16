//! Original SaveGame wait owner (B02B8/B0524/B23B0/B24A0).
//!
//! The Question owns its animation and polls `poll_write` only after opening.
//! Disk publication is a host service. Native SDK write failures abort; the
//! explicitly named desktop failure path allows reversible Store rollback.
use std::collections::VecDeque;

pub const PROMPT: &str =
    "セーブデータを更新中です。@r電源を切ったり、ソフトを終了したりしないでください。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A generic Question with no answer buttons. Its worker predicate is the
    /// only way to close it; controller/pointer cancellation is unavailable.
    ShowWait,
    /// Snapshot already belongs to the SaveGame owner. Start its file task only
    /// when the opened Question first polls this request.
    RequestWrite,
    /// Native completion is always true. False is desktop IO error recovery.
    Completed { success: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Start,
    Waiting,
    Completing,
    Done,
}

pub struct Controller {
    flags: u8,
    phase: Phase,
    started: bool,
    ready: bool,
    success: bool,
    events: VecDeque<Event>,
}
impl Controller {
    /// B0368 retains only bit0; no normal SaveGame body reads the retained bit.
    /// Save/Delete pass0, ExitGame and changed Config close pass1.
    pub fn new(flags: u32) -> Self {
        Self {
            flags: (flags & 1) as u8,
            phase: Phase::Start,
            started: false,
            ready: false,
            success: true,
            events: VecDeque::new(),
        }
    }
    pub fn flags(&self) -> u8 {
        self.flags
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }
    /// Call before advancing the Question child each parent frame. A final
    /// Question callback during that child tick is consumed next parent frame.
    pub fn update(&mut self) {
        match self.phase {
            Phase::Start => {
                self.phase = Phase::Waiting;
                self.events.push_back(Event::ShowWait);
            }
            Phase::Completing => {
                self.phase = Phase::Done;
                self.events.push_back(Event::Completed {
                    success: self.success,
                });
            }
            Phase::Waiting | Phase::Done => {}
        }
    }
    /// Question open-state predicate B24A0. Input permission does not gate this
    /// poll. A worker is installed once; only its native status2 closes the panel.
    pub fn poll_write(&mut self) -> bool {
        if self.phase != Phase::Waiting {
            return false;
        }
        if !self.started {
            self.started = true;
            self.events.push_back(Event::RequestWrite);
            return false;
        }
        self.ready
    }
    /// The platform task completed successfully, including its minimum elapsed
    /// time. It must still pass the Question close animation and final callback.
    pub fn io_finished(&mut self) {
        if self.phase == Phase::Waiting && self.started {
            self.ready = true;
        }
    }
    /// Desktop-only recovery: native nn::diag::detail::AbortImpl never returns
    /// a failed completion. Keep the retained menu usable after Store rollback.
    pub fn desktop_io_failed(&mut self) {
        if self.phase == Phase::Waiting && self.started {
            self.success = false;
            self.ready = true;
        }
    }
    /// B23B0 ignores stage0. Final stage1 releases the worker, then marks state2;
    /// B0524 invokes the parent's completion on the following parent update.
    pub fn question_response(&mut self, final_stage: bool) {
        if final_stage && self.phase == Phase::Waiting {
            self.phase = Phase::Completing;
        }
    }
    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }
}

#[cfg(test)]
mod native_reference;
#[cfg(test)]
mod tests;
