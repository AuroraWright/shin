//! CG viewer's native task group: publication 4db90, fade waiters 4e060/4e130.
//! Generic 17ae0 blocking tasks suppress the older 32980 input task.
use shin_core::time::Ticks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewerEvent {
    PublishPicture,
    CaptureClosing,
}
#[derive(Clone, Copy)]
enum Kind {
    Input,
    Load,
    Fade,
}
struct Task {
    kind: Kind,
    alive: bool,
}
pub(crate) struct CgViewerMotion {
    tasks: Vec<Task>,
    capture_elapsed: f32,
    capture_present: bool,
    closing: bool,
    input_enabled: bool,
    prepared: bool,
    events: Vec<ViewerEvent>,
}
impl CgViewerMotion {
    pub fn new() -> Self {
        Self {
            tasks: vec![
                Task {
                    kind: Kind::Input,
                    alive: true,
                },
                Task {
                    kind: Kind::Load,
                    alive: true,
                },
            ],
            capture_elapsed: 0.0,
            capture_present: false,
            closing: false,
            input_enabled: false,
            prepared: false,
            events: Vec::new(),
        }
    }
    /// Fully locked rows create a blank viewer with only its input task (4a8d4).
    pub fn without_load() -> Self {
        let mut this = Self::new();
        this.tasks.pop();
        this
    }
    /// Retire dead tasks, snapshot newest-first, then propagate eligibility.
    pub fn prepare_frame(&mut self, worker_ready: bool, eligible: bool) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        self.tasks.retain(|task| task.alive);
        self.input_enabled = false;
        let mut eligible = eligible;
        for index in (0..self.tasks.len()).rev() {
            match self.tasks[index].kind {
                Kind::Load => {
                    if worker_ready {
                        self.events.push(ViewerEvent::PublishPicture);
                        self.begin_capture();
                        self.tasks[index].alive = false;
                        // New waiter is excluded from this update snapshot.
                        self.tasks.push(Task {
                            kind: Kind::Fade,
                            alive: true,
                        });
                    }
                    eligible = false; // Generic blocking byte40 stays set even on completion.
                }
                Kind::Fade => {
                    self.tasks[index].alive = self.capture_elapsed < 30.0;
                    eligible = false;
                }
                Kind::Input => {
                    self.input_enabled = eligible && !self.closing;
                    self.tasks[index].alive = !self.closing;
                    // 4aad4 clears the mutable eligibility byte even when disabled.
                    eligible = false;
                }
            }
        }
    }
    fn begin_capture(&mut self) {
        self.capture_present = true;
        self.capture_elapsed = 0.0;
    }
    /// A controller/pointer next request is inserted after the input task's snapshot.
    pub fn begin_load(&mut self) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.tasks.push(Task {
            kind: Kind::Load,
            alive: true,
        });
        self.input_enabled = false;
        true
    }
    pub fn close(&mut self) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.closing = true;
        self.input_enabled = false;
        for task in &mut self.tasks {
            if matches!(task.kind, Kind::Input) {
                task.alive = false;
            }
        }
        self.events.push(ViewerEvent::CaptureClosing);
        self.begin_capture();
        self.tasks.push(Task {
            kind: Kind::Fade,
            alive: true,
        });
        true
    }
    /// 4b218 advances the shared signed capture clock after task polling.
    pub fn advance(&mut self, delta: Ticks) {
        if self.capture_present {
            self.capture_elapsed = (self.capture_elapsed + delta.as_f32()).min(30.0);
        }
        self.prepared = false;
    }
    pub fn accepts_input(&self) -> bool {
        self.input_enabled && !self.closing
    }
    pub fn is_finished(&self) -> bool {
        self.tasks.is_empty()
    }
    pub fn capture_opacity(&self) -> f32 {
        if self.capture_present {
            1.0 - self.capture_elapsed / 30.0
        } else {
            0.0
        }
    }
    pub fn take_events(&mut self) -> Vec<ViewerEvent> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests;
