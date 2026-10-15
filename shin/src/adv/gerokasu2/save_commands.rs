//! SAVEINFO/AUTOSAVE metadata, boundaries and retained capture/notification.
use super::{layers::Gerokasu2Layers, responses, variables::GerVariables};
use anyhow::{Result, ensure};
use shin_core::{
    format::save::gerokasu2::{AutosaveMemory, Progress},
    vm::gerokasu2::{GerReplayState, Gerokasu2Scripter},
};
use std::sync::Arc;

#[cfg(test)]
mod checkpoint_tests;

#[derive(Debug, Clone)]
pub struct SaveMetadata {
    current: [Arc<str>; 2],
    saved: [Arc<str>; 2],
}
impl Default for SaveMetadata {
    fn default() -> Self {
        let empty: Arc<str> = Arc::from("");
        Self {
            current: [empty.clone(), empty.clone()],
            saved: [empty.clone(), empty],
        }
    }
}
impl SaveMetadata {
    pub fn current(&self) -> &[Arc<str>; 2] {
        &self.current
    }
    pub fn saved(&self) -> &[Arc<str>; 2] {
        &self.saved
    }
    pub fn set(&mut self, level: i32, text: &str) -> Result<()> {
        ensure!(
            (0..2).contains(&level),
            "Ger SAVEINFO level {level} exceeds two entries"
        );
        self.current[level as usize] = Arc::from(text);
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTimeline {
    pub boundary: u32,
    pub previous_message: u32,
    pub current_message: u32,
    pub previous_chapter: u32,
    pub current_chapter: u32,
    pub last_skip: u32,
    pub current_interaction: u32,
    pub resume_position: u32,
    pub normal_messages: u32,
    pub chapter_messages: u32,
    pub event: i32,
}
impl Default for SaveTimeline {
    fn default() -> Self {
        Self {
            boundary: 0,
            previous_message: 0,
            current_message: 0,
            previous_chapter: 0,
            current_chapter: 0,
            last_skip: 0,
            current_interaction: 0,
            resume_position: 0,
            normal_messages: 0,
            chapter_messages: 0,
            event: -1,
        }
    }
}
impl SaveTimeline {
    pub fn message(&mut self) {
        self.previous_message = self.current_message;
        if self.event < 0 {
            self.boundary = self.boundary.wrapping_add(1);
            self.current_message = self.boundary;
            self.last_skip = self.boundary;
            self.normal_messages = self.normal_messages.wrapping_add(1);
            self.chapter_messages = self.chapter_messages.wrapping_add(1);
        }
    }
    fn chapter(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        self.previous_chapter = self.current_chapter;
        self.current_chapter = self.boundary;
        self.last_skip = self.boundary;
        self.chapter_messages = 0;
    }
}
/// Re-decodable numeric VM, layer targets and command-current descriptors.
/// Live GPU/audio owners, disk storage and publication remain separate.
#[derive(Clone)]
pub struct StateCheckpoint {
    pub vm: GerReplayState,
    pub layers: Box<Gerokasu2Layers>,
    pub metadata: SaveMetadata,
    pub timeline: SaveTimeline,
    pub response_bit_position: usize,
    pub descriptors: Box<super::descriptors::Descriptors>,
}
pub struct SaveSession {
    pub metadata: SaveMetadata,
    pub timeline: SaveTimeline,
    pub progress: Progress,
    pub autosave: AutosaveMemory,
    pub checkpoints: Vec<Arc<StateCheckpoint>>,
    pub descriptors: Box<super::descriptors::Descriptors>,
    pub variables: GerVariables,
    pub response_bit_position: usize,
}
impl SaveSession {
    pub fn new(
        progress: Progress,
        autosave: AutosaveMemory,
        metadata: SaveMetadata,
        timeline: SaveTimeline,
    ) -> Self {
        Self {
            progress,
            autosave,
            metadata,
            timeline,
            checkpoints: Vec::new(),
            descriptors: Default::default(),
            variables: Default::default(),
            response_bit_position: 0,
        }
    }
    pub fn fresh(scenario_id: i32, seed: u32) -> Self {
        Self::new(
            Progress::fresh(scenario_id, seed),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }
    fn checkpoint(&mut self, vm: &Gerokasu2Scripter, layers: &Gerokasu2Layers) -> Result<()> {
        self.variables.commit()?;
        // Native eec0 appends at most one snapshot when the count is below
        // boundary/200+1. The constructor calls it before advancing a boundary.
        if self.checkpoints.len() < self.timeline.boundary as usize / 200 + 1 {
            self.checkpoints.push(Arc::new(StateCheckpoint {
                vm: vm.checkpoint_for_replay()?,
                layers: Box::new(layers.clone()),
                metadata: self.metadata.clone(),
                timeline: self.timeline,
                response_bit_position: self.response_bit_position,
                descriptors: self.descriptors.clone(),
            }));
        }
        Ok(())
    }
    pub fn message(&mut self, vm: &Gerokasu2Scripter, layers: &Gerokasu2Layers) -> Result<()> {
        if self.timeline.event < 0 {
            self.checkpoint(vm, layers)?;
        }
        self.timeline.message();
        Ok(())
    }
    pub fn interactive(
        &mut self,
        vm: &Gerokasu2Scripter,
        layers: &Gerokasu2Layers,
        has_text: bool,
    ) -> Result<()> {
        if !has_text {
            self.checkpoint(vm, layers)?;
            self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
            self.timeline.current_interaction = self.timeline.boundary;
            self.timeline.last_skip = self.timeline.boundary;
        }
        Ok(())
    }
    pub fn persistent_get(&mut self, slot: i32) -> i32 {
        let value = self.variables.get(slot);
        responses::write_persistent(
            &mut self.progress.active_entry_mut().responses,
            &mut self.response_bit_position,
            value,
        );
        value
    }
    pub fn persistent_replay(&mut self) -> i32 {
        responses::read_persistent(
            &mut self.progress.active_entry_mut().responses,
            &mut self.response_bit_position,
        )
    }
    pub fn event_begin(
        &mut self,
        vm: &Gerokasu2Scripter,
        layers: &Gerokasu2Layers,
        event: i32,
    ) -> Result<()> {
        self.checkpoint(vm, layers)?;
        self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
        self.timeline.event = event;
        self.timeline.previous_message = self.timeline.current_message;
        Ok(())
    }
    pub fn event_end(&mut self, vm: &Gerokasu2Scripter, layers: &Gerokasu2Layers) -> Result<()> {
        self.checkpoint(vm, layers)?;
        self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
        self.timeline.event = -1;
        Ok(())
    }
    pub fn resume_set(&mut self, vm: &Gerokasu2Scripter, layers: &Gerokasu2Layers) -> Result<()> {
        self.checkpoint(vm, layers)?;
        self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
        self.timeline.resume_position = self.timeline.boundary;
        Ok(())
    }
    pub fn record_quiz(&mut self, result: i32) {
        responses::write_bits(
            &mut self.progress.active_entry_mut().responses,
            &mut self.response_bit_position,
            6,
            result as u32,
        );
        self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
    }
    pub fn record_selection(&mut self, result: i32) {
        responses::write_bits(
            &mut self.progress.active_entry_mut().responses,
            &mut self.response_bit_position,
            4,
            result as u32,
        );
        self.timeline.boundary = self.timeline.boundary.wrapping_add(1);
    }
    pub fn begin_autosave(
        &mut self,
        vm: &Gerokasu2Scripter,
        layers: &Gerokasu2Layers,
    ) -> Result<AutosaveCommand> {
        self.checkpoint(vm, layers)?;
        self.timeline.chapter();
        self.metadata.saved.clone_from(&self.metadata.current);
        Ok(AutosaveCommand::new())
    }
    /// Native E500/E820 retains current progress and committed globals. Only
    /// constructor state and its route cursor come from the selected payload.
    /// Pending variable writes start empty, as in native DFE0 initialization.
    pub fn fork_for_restore(
        &self,
        saved_position: u32,
        replay_target: u32,
    ) -> (Self, Option<Arc<StateCheckpoint>>) {
        let archive = super::checkpoints::Archive::from_entries(
            self.checkpoints
                .iter()
                .map(|state| super::checkpoints::Entry {
                    boundary: state.timeline.boundary,
                    state: state.clone(),
                })
                .collect(),
        );
        let fork = archive.fork_for_restore(saved_position);
        let restored = fork.restored.map(|entry| entry.state);
        let mut progress = self.progress.clone();
        let entry = progress.active_entry_mut();
        entry.save_position = saved_position;
        entry.replay_target = replay_target;
        let mut session = Self::fresh(entry.scenario_id, entry.random_seed);
        session.progress = progress;
        session.autosave = self.autosave.clone();
        session
            .variables
            .replace_committed(self.variables.committed().clone());
        session.checkpoints = fork
            .archive
            .into_entries()
            .into_iter()
            .map(|entry| entry.state)
            .collect();
        if let Some(state) = &restored {
            session.metadata = state.metadata.clone();
            session.timeline = state.timeline;
            session.response_bit_position = state.response_bit_position;
            session.descriptors = state.descriptors.clone();
        }
        (session, restored)
    }
}
pub trait SaveServices {
    fn message_started(&mut self) {}
    fn save_session(&mut self) -> &mut SaveSession;
    fn packed_time(&mut self) -> Result<u64>;
    fn autosave_captured(&mut self, memory: &AutosaveMemory) -> Result<()>;
    fn show_scene_title(&mut self, title: &str) -> Result<()>;
}
#[derive(Default)]
enum CapturePhase {
    #[default]
    New,
    Clock,
    Publish,
    Complete,
}
#[derive(Default)]
pub struct SaveCapture {
    phase: CapturePhase,
}
impl SaveCapture {
    pub fn poll(&mut self, services: &mut dyn SaveServices) -> Result<()> {
        if matches!(self.phase, CapturePhase::New) {
            let s = services.save_session();
            let b = s.timeline.boundary;
            let e = s.progress.active_entry_mut();
            e.save_position = b;
            e.replay_target = b;
            s.autosave.valid = true;
            self.phase = CapturePhase::Clock;
        }
        if matches!(self.phase, CapturePhase::Clock) {
            let time = services.packed_time()?;
            let s = services.save_session();
            s.autosave.packed_time = time;
            s.autosave.progress.clone_from(&s.progress);
            self.phase = CapturePhase::Publish;
        }
        if matches!(self.phase, CapturePhase::Publish) {
            let memory = services.save_session().autosave.clone();
            services.autosave_captured(&memory)?;
            self.phase = CapturePhase::Complete;
        }
        Ok(())
    }
}
pub struct AutosaveCommand {
    armed: u8,
    reached: u8,
    capture: SaveCapture,
    title: Option<Arc<str>>,
    complete: bool,
}
impl AutosaveCommand {
    fn new() -> Self {
        Self {
            armed: 1,
            reached: 0,
            capture: Default::default(),
            title: None,
            complete: false,
        }
    }
    pub fn flags(&self) -> [u8; 2] {
        [self.armed, self.reached]
    }
    pub fn poll_replay(&mut self, progress: &Progress, boundary: u32) -> Result<bool> {
        ensure!(
            matches!(self.capture.phase, CapturePhase::New),
            "Ger autosave live effects have started"
        );
        if boundary >= progress.active_entry().save_position {
            self.armed = 0;
            self.reached = 1;
            Ok(false)
        } else {
            Ok(true)
        }
    }
    pub fn poll_live(&mut self, services: &mut dyn SaveServices) -> Result<bool> {
        if self.complete {
            return Ok(true);
        }
        self.capture.poll(services)?;
        if self.title.is_none() {
            self.title = Some(services.save_session().metadata.saved[1].clone());
        }
        services.show_scene_title(self.title.as_ref().unwrap())?;
        self.complete = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
