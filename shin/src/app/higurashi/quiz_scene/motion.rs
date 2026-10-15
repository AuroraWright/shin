//! Original First/child scope, countdown, question cadence and score return.
use super::{Action, Model};
use crate::adv::higurashi::{
    mode_stack::{Mode, ModePush, ModeStack},
    reconstruction_mode::OverlayClock,
};
use anyhow::{Result, ensure};
use shin_core::time::Ticks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Event {
    Model(Action),
    StopBgm(u32),
    PlayBgm { id: i32, fade: u32 },
    ReturnScore(i32),
}
pub(super) trait Services {
    fn event(&mut self, event: Event) -> Result<()>;
    fn publish_resources(&mut self) -> Result<()>;
    fn retire_first(&mut self);
    fn advance_voice(&mut self, delta: Ticks);
    fn advance_characters(&mut self, delta: Ticks);
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()>;
    fn voice_status(&self) -> u32;
    fn bgm_status(&self) -> u32;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    First,
    Input,
    Countdown,
    Finish,
    Return,
}
pub(super) struct Task {
    pub kind: Kind,
    pub phase: i32,
    pub done: bool,
    pub overlay: Option<OverlayClock>,
}
struct Frame<'a> {
    model: &'a mut Model,
    ready: &'a mut bool,
    countdown_phase: &'a mut i32,
    countdown_elapsed: &'a mut f32,
    question_elapsed: &'a mut f32,
    worker_status: u32,
    score_request: &'a mut Option<i32>,
    services: &'a mut dyn Services,
}
fn animate(model: &mut Model) -> Result<()> {
    for (clock, value) in model.clocks[1..].iter_mut().zip([1.5, 0.0]) {
        clock.fast_forward_to(value);
        clock.control(1.0, Ticks::from_f32(10.0), 0)?;
    }
    Ok(())
}
fn flush(model: &mut Model, services: &mut dyn Services) -> Result<()> {
    for action in model.take_actions() {
        services.event(Event::Model(action))?;
    }
    Ok(())
}
impl Mode<Frame<'_>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.done)
    }
    fn retire(&mut self, frame: &mut Frame<'_>) {
        if self.kind == Kind::First {
            frame.services.retire_first();
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self.kind {
            Kind::First => {
                let overlay = self.overlay.as_mut().unwrap();
                if self.phase == 0 && frame.worker_status == 2 {
                    frame.services.publish_resources()?;
                    *frame.ready = true;
                    overlay.clear(30);
                    frame.services.event(Event::StopBgm(60))?;
                    self.phase = 1;
                }
                if self.phase == 1 && !overlay.busy() {
                    frame.model.closing = false;
                    pushes.push(Task::new(Kind::Input));
                    pushes.push(Task::new(Kind::Countdown));
                    self.phase = -1;
                    self.done = true;
                }
                overlay.advance(delta);
            }
            Kind::Input => {
                frame.services.input(frame.model, permission)?;
                self.done = frame.model.closing;
                flush(frame.model, frame.services)?;
            }
            Kind::Countdown => {
                let phase = (*frame.countdown_elapsed / 60.0).floor() as i32;
                if phase != *frame.countdown_phase {
                    if phase > 3 {
                        *frame.countdown_phase = -1;
                        *frame.question_elapsed = 0.0;
                        self.done = true;
                    } else {
                        *frame.countdown_phase = phase;
                        animate(frame.model)?;
                        if phase == 3 {
                            frame.services.event(Event::Model(Action::Voice {
                                name: "VT_QUIZ_SYS01",
                                reaction: false,
                            }))?;
                            frame.services.event(Event::PlayBgm { id: 85, fade: 60 })?;
                        } else {
                            frame.services.event(Event::Model(Action::Cue("count")))?;
                        }
                    }
                }
                if !self.done {
                    *frame.countdown_elapsed += delta.as_f32();
                }
            }
            Kind::Finish => {
                if frame.services.voice_status() & 2 == 0 && frame.services.bgm_status() & 2 == 0 {
                    frame.model.clocks[0].control(0.0, Ticks::from_f32(30.0), 0x80)?;
                    frame.model.clocks[0].control(1.0, Ticks::from_f32(60.0), 0)?;
                    pushes.push(Task::new(Kind::Return));
                    self.done = true;
                }
            }
            Kind::Return => {
                if frame.model.clocks[0].is_idle() {
                    frame
                        .services
                        .event(Event::ReturnScore(frame.model.score))?;
                    *frame.score_request = Some(frame.model.score);
                    self.done = true;
                }
            }
        }
        Ok(0)
    }
}
impl Task {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            phase: 0,
            done: false,
            overlay: None,
        }
    }
}
pub(in crate::app::higurashi) struct Motion {
    pub model: Model,
    pub ready: bool,
    pub countdown_phase: i32,
    pub countdown_elapsed: f32,
    pub question_elapsed: f32,
    pub next_question: u32,
    pub score_request: Option<i32>,
    pub borrowed: Vec<Kind>,
    tasks: ModeStack<Task>,
}
impl Motion {
    pub fn new(model: Model, captured: bool) -> Self {
        let mut tasks = ModeStack::default();
        tasks.push(Task {
            overlay: Some(OverlayClock::captured(captured)),
            ..Task::new(Kind::First)
        });
        Self {
            model,
            ready: false,
            countdown_phase: -1,
            countdown_elapsed: 0.0,
            question_elapsed: -1.0,
            next_question: 0,
            score_request: None,
            borrowed: Vec::new(),
            tasks,
        }
    }
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter()
    }
    pub fn first(&self) -> Option<&Task> {
        self.tasks.iter().find(|t| t.kind == Kind::First)
    }
    pub fn advance(
        &mut self,
        worker_status: u32,
        delta: Ticks,
        services: &mut impl Services,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid quiz scene delta"
        );
        ensure!(
            self.score_request.is_none(),
            "Dispatch retained quiz score before updating source"
        );
        self.tasks.poll(
            delta,
            1,
            &mut Frame {
                model: &mut self.model,
                ready: &mut self.ready,
                countdown_phase: &mut self.countdown_phase,
                countdown_elapsed: &mut self.countdown_elapsed,
                question_elapsed: &mut self.question_elapsed,
                worker_status,
                score_request: &mut self.score_request,
                services,
            },
        )?;
        self.borrowed.clear();
        self.tasks.draw(|t| self.borrowed.push(t.kind));
        // Actual9cb1c advances all three clocks, then voice, then eleven BUPs.
        self.model.advance_clocks(delta);
        services.advance_voice(delta);
        if self.ready {
            services.advance_characters(delta);
            if self.question_elapsed >= 0.0 {
                if self.question_elapsed >= (self.next_question as i32).wrapping_mul(360) as f32 {
                    if self.next_question < 10 {
                        self.model.prepare_question(self.next_question as usize)?;
                        self.next_question += 1;
                    } else if self.model.current_question >= 0 {
                        self.model.current_question = -1;
                        self.model.closing = true;
                        self.countdown_phase = 4;
                        animate(&mut self.model)?;
                        services.event(Event::Model(Action::Voice {
                            name: "VT_QUIZ_SYS02",
                            reaction: false,
                        }))?;
                        self.tasks.push(Task::new(Kind::Finish));
                    }
                }
                self.question_elapsed = (self.question_elapsed + delta.as_f32()).min(3600.0);
            }
        }
        flush(&mut self.model, services)
    }
}

#[cfg(test)]
mod tests;
