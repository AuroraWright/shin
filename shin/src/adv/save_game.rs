//! Native SaveGame d3390/d3680 and no-answer updating dialog239c0.
//! Storage snapshots/workers live in SaveStore so shutdown cannot outlive them.
use super::{
    backlog_rollback::Confirmation, dialog_input::DialogInput, dialog_view::DialogView,
    persistence::SaveStore,
};
use crate::app::AppAction;
use crate::{asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::time::{Duration, Instant};

pub(super) const PROMPT: &str =
    "セーブデータを更新中です。@r電源を切ったり、ソフトを終了したりしないでください。";
pub(super) const ERROR_PROMPT: &str =
    "セーブデータを保存できませんでした。@r保存先をご確認ください。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Running,
    Closing,
    Notify,
    Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    StartWorker,
    FinishWorker,
}
struct SaveGameModel {
    dialog: Confirmation,
    phase: Phase,
}
impl SaveGameModel {
    fn new() -> Self {
        Self {
            dialog: Confirmation::notification(),
            phase: Phase::Opening,
        }
    }
    fn update(&mut self, delta: Ticks, worker_ready: bool) -> Option<Event> {
        // The final dialog callback sets the owner to state2 at the end of
        // its child update. d3680 notifies its owner on the following update.
        if self.phase == Phase::Notify {
            self.phase = Phase::Done;
            return Some(Event::FinishWorker);
        }
        self.dialog.update(delta);
        match self.phase {
            Phase::Opening if self.dialog.is_open() => {
                self.phase = Phase::Running;
                Some(Event::StartWorker)
            }
            Phase::Running if worker_ready => {
                self.dialog.close_notification();
                self.phase = Phase::Closing;
                None
            }
            Phase::Closing if self.dialog.is_done() => {
                self.phase = Phase::Notify;
                None
            }
            _ => None,
        }
    }
}

// The native worker calls the imported nn::diag::detail::AbortImpl on failed
// writes. This acknowledgement is the desktop recovery adapter, rather than
// an authored native save dialog or proof of the external diagnostic handler.
struct SaveFailure {
    error: Option<anyhow::Error>,
    dialog: Confirmation,
    input: DialogInput,
}

pub(crate) struct SaveGameTask {
    model: SaveGameModel,
    view: DialogView,
    memory_started: Option<Instant>,
    error_view: DialogView,
    failure: Option<SaveFailure>,
    pointer_held: bool,
    pointer_extent: Option<(u16, u16)>,
}
impl SaveGameTask {
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.pointer_held = held;
        if let Some(failure) = &mut self.failure {
            failure.input.ignore_pointer(held);
        }
    }
    pub fn load(server: &AssetServer) -> anyhow::Result<Self> {
        Ok(Self {
            model: SaveGameModel::new(),
            view: DialogView::load_text(server, PROMPT, &[])?,
            memory_started: None,
            error_view: DialogView::load_text(server, ERROR_PROMPT, &["確認"])?,
            failure: None,
            pointer_held: false,
            pointer_extent: None,
        })
    }
    pub(super) fn update(
        &mut self,
        delta: Ticks,
        mut store: Option<&mut SaveStore>,
    ) -> Option<anyhow::Result<()>> {
        if let Some(failure) = &mut self.failure {
            failure
                .input
                .update_motion(&mut self.error_view.geometry, delta);
            failure.dialog.update(delta);
            return if failure.dialog.is_done() {
                failure.error.take().map(Err)
            } else {
                None
            };
        }
        let ready = self.model.phase == Phase::Running
            && match store.as_deref_mut() {
                Some(store) => store.poll_explicit(),
                None => self
                    .memory_started
                    .is_some_and(|started| started.elapsed() >= Duration::from_secs(1)),
            };
        match self.model.update(delta, ready) {
            Some(Event::StartWorker) => {
                match store {
                    Some(store) => store.start_explicit(),
                    None => self.memory_started = Some(Instant::now()),
                }
                None
            }
            Some(Event::FinishWorker) => match store.map_or(Ok(()), SaveStore::finish_explicit) {
                Ok(()) => Some(Ok(())),
                Err(error) => {
                    let mut input = DialogInput::default();
                    input.ignore_pointer(self.pointer_held);
                    let extent = self.pointer_extent.unwrap_or((1280, 720));
                    input.set_pointer_scale(Vec2::new(
                        1920.0 / extent.0.max(1) as f32,
                        1080.0 / extent.1.max(1) as f32,
                    ));
                    self.failure = Some(SaveFailure {
                        error: Some(error),
                        dialog: Confirmation::new(1, 0, false),
                        input,
                    });
                    None
                }
            },
            None => None,
        }
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64, extent: (u16, u16)) {
        self.pointer_held = held;
        if let Some(failure) = &mut self.failure {
            if self.pointer_extent != Some(extent) {
                failure.input.ignore_pointer(held);
                failure.input.set_pointer_scale(Vec2::new(
                    1920.0 / extent.0.max(1) as f32,
                    1080.0 / extent.1.max(1) as f32,
                ));
            }
            failure.input.pointer_timed(
                &mut failure.dialog,
                &mut self.error_view.geometry,
                position,
                held,
                elapsed,
            );
        }
        self.pointer_extent = Some(extent);
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        if let Some(failure) = &mut self.failure {
            failure.input.keyboard(
                &mut failure.dialog,
                self.error_view.geometry.vertical,
                state,
            );
        }
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.failure
            .as_mut()
            .map_or_else(Vec::new, |failure| failure.input.take_sounds())
    }
    #[cfg(test)]
    pub(crate) fn reference_state(&self) -> String {
        format!(
            "phase={:?}, failure={:?}",
            self.model.phase,
            self.failure
                .as_ref()
                .and_then(|failure| failure.error.as_ref())
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_failure_is_open(&self) -> bool {
        self.failure
            .as_ref()
            .is_some_and(|failure| failure.dialog.is_open())
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        if let Some(failure) = &self.failure {
            self.error_view.prepare(context, &failure.dialog);
        } else {
            self.view.prepare(context, &self.model.dialog);
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some(failure) = &self.failure {
            self.error_view.render(pass, &failure.dialog);
        } else {
            self.view.render(pass, &self.model.dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    include!("save_error_reference_tests.rs");

    #[test]
    fn opens_before_one_worker_and_notifies_after_exit_and_cooldown() {
        let mut task = SaveGameModel::new();
        assert_eq!(task.dialog.button_count(), 0);
        assert!(!task.dialog.cancel() && !task.dialog.confirm());
        assert_eq!(task.update(Ticks::from_f32(12.0), true), None);
        assert_eq!(task.phase, Phase::Opening);
        assert_eq!(task.update(Ticks::ZERO, true), Some(Event::StartWorker));
        for _ in 0..10 {
            assert_eq!(task.update(Ticks::from_f32(1000.0), false), None);
            assert_eq!(task.phase, Phase::Running);
        }
        assert_eq!(task.update(Ticks::ZERO, true), None);
        assert_eq!(task.update(Ticks::from_f32(12.0), true), None);
        assert!(!task.dialog.is_done());
        assert_eq!(task.update(Ticks::from_f32(4.0), true), None);
        assert_eq!(task.update(Ticks::from_f32(1.0), true), None);
        assert_eq!(task.phase, Phase::Notify);
        assert_eq!(task.update(Ticks::ZERO, true), Some(Event::FinishWorker));
        assert_eq!(task.update(Ticks::ZERO, true), None);
    }

    #[test]
    fn pending_dialog_matches_complete_original_and_updated_native_traces() {
        let golden =
            include_str!("../../../reverse-engineering/native-save-dialog-timing-golden.txt");
        let mut lines = golden.lines();
        for case in 0..128 {
            let input: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(input[0].parse::<usize>().unwrap(), case);
            let frames = input[1].parse::<usize>().unwrap();
            assert_eq!(input.len(), 2 + 2 * frames);
            let mut model = SaveGameModel::new();
            let (mut polls, mut starts, mut early, mut final_calls, mut owner_calls) =
                (0, 0, 0, 0, 0);
            let mut final_frame = None;
            for frame in 0..frames {
                let before = model.phase;
                let delta = input[2 + frame * 2].parse::<f32>().unwrap();
                let ready = input[3 + frame * 2] == "1";
                if before == Phase::Running {
                    polls += 1;
                }
                match model.update(Ticks::from_f32(delta), ready) {
                    Some(Event::StartWorker) => {
                        polls += 1;
                        starts += 1;
                    }
                    Some(Event::FinishWorker) => {
                        owner_calls += 1;
                        assert_eq!(Some(frame), final_frame.map(|at| at + 1));
                    }
                    None => {}
                }
                if model.dialog.take_intermediate_outcome().is_some() {
                    early += 1;
                }
                if model.dialog.take_outcome().is_some() {
                    final_calls += 1;
                    final_frame = Some(frame);
                }
                let native: Vec<_> = lines.next().unwrap().split_whitespace().collect();
                assert_eq!(native[0].parse::<usize>().unwrap(), case);
                assert_eq!(native[1].parse::<usize>().unwrap(), frame);
                let phase = match model.phase {
                    Phase::Opening => 1,
                    Phase::Running => 2,
                    Phase::Closing if early == 0 => 3,
                    Phase::Closing => 4,
                    Phase::Notify | Phase::Done => -1,
                };
                assert_eq!(
                    phase,
                    native[2].parse::<i32>().unwrap(),
                    "case{case} frame{frame}"
                );
                for (value, bits) in [
                    (model.dialog.scale.value(), native[3]),
                    (model.dialog.opacity.value(), native[4]),
                ] {
                    let expected = f32::from_bits(u32::from_str_radix(bits, 16).unwrap());
                    assert!(
                        (value - expected).abs() <= 0.000001,
                        "case{case} frame{frame}: {value} != {expected}"
                    );
                }
                assert_eq!(
                    [polls, starts, early, final_calls],
                    std::array::from_fn(|i| native[5 + i].parse::<usize>().unwrap()),
                    "case{case} frame{frame}"
                );
            }
            assert_eq!((starts, early, final_calls, owner_calls), (1, 1, 1, 1));
        }
        assert!(lines.next().is_none());
    }
}
