//! Updated420/448c EXIT owner:ordered close gates,commits and scene routing.
use super::progress::ProgressMemory;
use anyhow::Result;

/// The native24-variable completion list at GOT1cbef0.
pub const COMPLETION_VARIABLES: [usize; 24] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 162, 163, 164, 165,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    MarkExiting,
    ClearAutoSkip,
    HideMessage,
    StopVoice,
    FinishStoppingEffects,
    StopEffects,
    StopNotification,
    StopMenuIndicator,
    WaitThirtyTicks,
}
const BEGIN: [Begin; 9] = [
    Begin::MarkExiting,
    Begin::ClearAutoSkip,
    Begin::HideMessage,
    Begin::StopVoice,
    Begin::FinishStoppingEffects,
    Begin::StopEffects,
    Begin::StopNotification,
    Begin::StopMenuIndicator,
    Begin::WaitThirtyTicks,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    ///15b6c:save child;18134 later queues title(-1,-1).
    SaveThenTitle,
    ///15c64:tip lookup/completion bit and browser factory after optional save.
    Tips,
    ///15e54:normalize alternate ID by10000,then return to the fragment menu.
    Kakera { selected: i32 },
    ///12049 with byte52>=2:17490 queues fresh primary scenario10010.
    Bonus,
}
pub trait ExitServices {
    fn begin(&mut self, action: Begin) -> Result<()>;
    fn wait_busy(&mut self) -> Result<bool>;
    fn message_busy(&mut self) -> Result<bool>;
    fn view_child_busy(&mut self) -> Result<bool>;
    fn modes_ready(&mut self) -> Result<bool>;
    ///e8eb0 only locks/unlocks on this Switch build;retain the authored query.
    fn achievement(&mut self, id: u32) -> Result<()>;
    fn destination(
        &mut self,
        destination: Destination,
        progress: &mut ProgressMemory,
    ) -> Result<()>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Begin(usize),
    Wait,
    Notify(usize),
    Dispatch,
    Done,
}
#[derive(Debug)]
pub struct ExitCommand {
    mode: u8,
    argument: i32,
    phase: Phase,
    achievements: Vec<u32>,
    destination: Option<Destination>,
}
impl ExitCommand {
    pub fn new(mode: u8, argument: i32) -> Self {
        Self {
            mode,
            argument,
            phase: Phase::Begin(0),
            achievements: Vec::new(),
            destination: None,
        }
    }
    pub fn blocks_fast_forward(&self) -> bool {
        self.mode == 0
    }
    pub fn phase(&self) -> i32 {
        match self.phase {
            Phase::Begin(_) => 0,
            Phase::Done => -1,
            _ => 1,
        }
    }
    ///447c/4484 both returnfalse for EXIT;the nonzero420 branch is nativeNOPE.
    pub fn poll_replay(&self) -> bool {
        self.mode != 0
    }
    pub fn forward_complete(&self) -> bool {
        self.mode != 0
    }
    ///Returnsfalse even after a scene/save request,as original448c does. A
    ///nonzero EXIT completes as NOPE and leaves every host owner untouched.
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        commit_enabled: bool,
        services: &mut impl ExitServices,
    ) -> Result<bool> {
        if self.mode != 0 {
            return Ok(true);
        }
        if let Phase::Begin(mut step) = self.phase {
            while step < BEGIN.len() {
                services.begin(BEGIN[step])?;
                step += 1;
                self.phase = Phase::Begin(step);
            }
            self.phase = Phase::Wait;
        }
        if self.phase == Phase::Wait {
            // Preserve short-circuit query order;no later owner is queried
            // while an earlier clock/message/view/scope gate blocks.
            if services.wait_busy()?
                || services.message_busy()?
                || services.view_child_busy()?
                || !services.modes_ready()?
            {
                return Ok(false);
            }
            progress.commit_pending(commit_enabled)?;
            let scenario = progress.progress().active_entry().scenario_id;
            if scenario <= 9999
                && self.argument == 1
                && COMPLETION_VARIABLES
                    .iter()
                    .all(|&v| progress.persistent().get(v) > 1)
            {
                self.achievements.push(39);
            }
            let fragment = (scenario.wrapping_sub(2000) as u32) < 1000
                || (scenario.wrapping_sub(12000) as u32) < 1000;
            let tip = (scenario.wrapping_sub(1000) as u32) < 1000
                || (scenario.wrapping_sub(11000) as u32) < 1000;
            self.destination = Some(if fragment {
                if scenario <= 9999 && (60..162).all(|v| progress.persistent().get(v) >= 2) {
                    self.achievements.push(37);
                }
                if scenario == 12051 {
                    Destination::SaveThenTitle
                } else if scenario == 12049 && progress.bytes().get(52).copied().unwrap_or(0) >= 2 {
                    Destination::Bonus
                } else {
                    Destination::Kakera {
                        selected: if scenario > 9999 {
                            scenario - 12000
                        } else {
                            scenario - 2000
                        },
                    }
                }
            } else if tip {
                Destination::Tips
            } else {
                Destination::SaveThenTitle
            });
            self.phase = Phase::Notify(0);
        }
        if let Phase::Notify(mut index) = self.phase {
            while index < self.achievements.len() {
                services.achievement(self.achievements[index])?;
                index += 1;
                self.phase = Phase::Notify(index);
            }
            self.phase = Phase::Dispatch;
        }
        if self.phase == Phase::Dispatch {
            services.destination(self.destination.unwrap(), progress)?;
            self.phase = Phase::Done;
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
