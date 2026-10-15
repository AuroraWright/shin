//! Updated SELECT continuation, settings memory and four-bit response ownership.
//! UI/clock services are explicit; native rendering and disk encoding are separate.
use super::{autosave::AutosaveMemory, progress::ProgressMemory};
use anyhow::{Result, ensure};
use shin_core::{
    format::{save::index_set::IndexSet, scenario::instruction_elements::Register},
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

#[derive(Clone, Debug)]
pub struct ChoiceRequest {
    pub base: i32,
    pub index: i32,
    pub destination: Register,
    pub visibility: i32,
    pub title: String,
    pub variants: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ChoiceHistory {
    seen: IndexSet,
    last: Vec<u8>,
}
impl ChoiceHistory {
    /// Explicit supplied settings+40/+58 memory, not default/disk loading.
    pub fn new(seen: IndexSet, last: Vec<u8>) -> Self {
        Self { seen, last }
    }
    pub fn seen(&self) -> &IndexSet {
        &self.seen
    }
    pub fn last(&self) -> &[u8] {
        &self.last
    }
    fn seen_mask(&self, request: &ChoiceRequest) -> u32 {
        if request.base < 0 {
            return 0;
        }
        (0..request.variants.len()).fold(0, |mask, i| {
            mask | if self
                .seen
                .contains((request.base as u32).wrapping_add(i as u32) as usize)
            {
                1 << i
            } else {
                0
            }
        })
    }
    fn mark_seen(&mut self, base: i32, value: i32) {
        if base >= 0 {
            self.seen
                .add((base as u32).wrapping_add(value as u32) as usize);
        }
    }
    pub(super) fn record_last(&mut self, index: i32, value: i32) -> Result<()> {
        if index < 0 {
            return Ok(());
        }
        let index = index as usize;
        if self.last.len() <= index {
            let size = (index + 32) & !31;
            self.last.try_reserve(size - self.last.len())?;
            self.last.resize(size, 0);
        }
        // Native5d2c uses low8(result)+1, then aa00c stores a byte.
        self.last[index] = (value as u8).wrapping_add(1);
        Ok(())
    }
}
pub trait ChoiceServices {
    /// Create the native-equivalent choice/overlay owner and its resolved UI
    /// side effects. An error leaves creation unacknowledged.
    fn open(&mut self, request: &ChoiceRequest, seen_mask: u32) -> Result<()>;
    /// Original script index, not the filtered row index. None/negative waits.
    fn poll(&mut self) -> Result<Option<i32>>;
    /// Native a9790 packed clock for each 11780 memory copy.
    fn packed_time(&mut self) -> Result<u64>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    New,
    OpeningCopy,
    Waiting,
    Last,
    Response,
    FinalCopy,
    Complete,
}
#[derive(Clone, Debug)]
pub struct ChoiceCommand {
    request: ChoiceRequest,
    starts_resume: bool,
    replay_stop: i32,
    replay_polled: bool,
    result: Option<i32>,
    phase: Phase,
    copy_started: bool,
}
impl ChoiceCommand {
    pub fn new(command: &Cmd, starts_resume: bool) -> Result<Self> {
        let Cmd::SELECT {
            base,
            index,
            dest,
            visibility,
            title,
            variants,
        } = command
        else {
            anyhow::bail!("Choice owner requires SELECT");
        };
        ensure!(
            variants.len() <= 12,
            "Higurashi SELECT exceeds the native 12-string payload"
        );
        Ok(Self {
            request: ChoiceRequest {
                base: i32::from(*base) - 1,
                index: i32::from(*index) - 1,
                destination: *dest,
                visibility: *visibility,
                title: title.clone(),
                variants: variants.clone(),
            },
            starts_resume,
            replay_stop: -1,
            replay_polled: false,
            result: None,
            phase: Phase::New,
            copy_started: false,
        })
    }
    pub fn request(&self) -> &ChoiceRequest {
        &self.request
    }
    pub fn replay_stop(&self) -> i32 {
        self.replay_stop
    }
    pub fn result(&self) -> Option<i32> {
        self.result
    }
    /// Native5784 uses active.save_position, independently of replay_target.
    /// A stopped callback stays retained until live handoff or reconstruction.
    pub fn poll_replay(
        &mut self,
        progress: &mut ProgressMemory,
        boundary: &mut u32,
        mut publish: impl FnMut(Register, i32) -> Result<()>,
    ) -> Result<bool> {
        ensure!(
            self.phase == Phase::New,
            "SELECT live effects must finish before replay"
        );
        if self.replay_polled {
            return Ok(self.replay_stop < 0);
        }
        let target = progress.progress().active_entry().save_position;
        if self.result.is_none() && self.starts_resume && *boundary >= target {
            self.replay_stop = 0;
            self.replay_polled = true;
            return Ok(false);
        }
        let value = if let Some(value) = self.result {
            value
        } else {
            let value = progress.choice_response(true, 0)?;
            self.result = Some(value);
            value
        };
        publish(self.request.destination, value)?;
        *boundary = boundary.wrapping_add(1);
        if *boundary >= target {
            self.replay_stop = 1;
        }
        self.replay_polled = true;
        Ok(self.replay_stop < 0)
    }
    fn copy(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        boundary: u32,
        services: &mut impl ChoiceServices,
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
    /// Original5848 creates UI once and copies11780 before waiting; completion
    /// records settings, publishes full32 feedback, writes low4 response bits,
    /// increments only boundary, then copies11780 again. Service failures retain
    /// the applied prefix and retry only the unacknowledged step.
    pub fn poll_live(
        &mut self,
        progress: &mut ProgressMemory,
        memory: &mut AutosaveMemory,
        history: &mut ChoiceHistory,
        boundary: &mut u32,
        services: &mut impl ChoiceServices,
        mut publish: impl FnMut(Register, i32) -> Result<()>,
    ) -> Result<bool> {
        if self.replay_stop > 0 {
            return Ok(true);
        }
        if self.phase == Phase::New {
            services.open(&self.request, history.seen_mask(&self.request))?;
            self.phase = Phase::OpeningCopy;
        }
        if self.phase == Phase::OpeningCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::Waiting;
        }
        if self.phase == Phase::Waiting {
            let Some(value) = services.poll()?.filter(|value| *value >= 0) else {
                return Ok(false);
            };
            self.result = Some(value);
            history.mark_seen(self.request.base, value);
            self.phase = Phase::Last;
        }
        let value = self.result.unwrap();
        if self.phase == Phase::Last {
            history.record_last(self.request.index, value)?;
            publish(self.request.destination, value)?;
            self.phase = Phase::Response;
        }
        if self.phase == Phase::Response {
            progress.choice_response(false, value)?;
            *boundary = boundary.wrapping_add(1);
            self.phase = Phase::FinalCopy;
        }
        if self.phase == Phase::FinalCopy {
            self.copy(progress, memory, *boundary, services)?;
            self.phase = Phase::Complete;
        }
        Ok(self.phase == Phase::Complete)
    }
}

#[cfg(test)]
pub(super) mod tests;
