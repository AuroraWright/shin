//! Actual dc994/dcd08 First and dad7c parent ordering over the shared mode scope.
use super::{Action, Model, SceneRequest};
use crate::adv::{
    higurashi::{
        browser_controls::Scrollbar,
        mode_stack::{Mode, ModePush, ModeStack},
        reconstruction_mode::OverlayClock,
    },
    input_area::InputAreaTarget,
};
use anyhow::Result;
use glam::Vec2;
use shin_core::{
    format::{save::higurashi::SaveDate, scenario::higurashi::HigurashiScenario},
    time::Ticks,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    PlayBgm(i32),
    StopBgm(i32),
    Cue(&'static str),
}
pub(super) trait Services {
    fn input(&mut self, model: &mut Model, permission: bool);
    fn fresh_date(&mut self) -> Result<SaveDate>;
    fn seed(&mut self) -> Result<u32>;
    fn event(&mut self, event: Event) -> Result<()>;
    fn retire_first(&mut self);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First,
    Input,
    Callback,
}
pub(super) struct Task {
    pub kind: Kind,
    pub phase: i32,
    pub done: bool,
    pub clock: Option<OverlayClock>,
    #[allow(dead_code, reason = "Preserve native callback identity for motion task inspection.")]
    pub callback: Option<super::Pending>,
}
struct Frame<'a> {
    model: &'a mut Model,
    status: u32,
    mask_present: bool,
    bgm_id: Option<i32>,
    services: &'a mut dyn Services,
    request: &'a mut Option<SceneRequest>,
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
                let clock = self.clock.as_mut().unwrap();
                if self.phase == 0 && frame.status == 2 {
                    clock.masked(30, f32::from_bits(0x3e99999a), 0, frame.mask_present);
                    if let Some(id) = frame.bgm_id {
                        frame.services.event(Event::PlayBgm(id))?;
                    } else {
                        frame.services.event(Event::StopBgm(60))?;
                    }
                    self.phase = 1;
                } else if self.phase == 1 && !clock.busy() {
                    frame.model.closing = false;
                    pushes.push(Task {
                        kind: Kind::Input,
                        phase: 0,
                        done: false,
                        clock: None,
                        callback: None,
                    });
                    self.phase = -1;
                    self.done = true;
                }
                clock.advance(delta);
            }
            Kind::Input => {
                frame.services.input(frame.model, permission);
                self.done = frame.model.closing;
                if self.done {
                    pushes.push(Task {
                        kind: Kind::Callback,
                        phase: 0,
                        done: false,
                        clock: None,
                        callback: frame.model.pending,
                    });
                }
            }
            Kind::Callback => {
                // The original callback does not consume caller input. Its
                // scene request waits for the pre-update row/back flash gate.
                let services = &mut frame.services;
                let mut date = None;
                if frame.model.data.is_none()
                    && matches!(frame.model.pending,Some(super::Pending::Start(at)) if frame.model.rows[at].flash.is_idle())
                {
                    date = Some(services.fresh_date()?);
                }
                if let Some(request) = frame.model.poll_request(
                    || Ok(date.expect("Fresh TIPS callback date")),
                    || services.seed(),
                )? {
                    *frame.request = Some(request);
                    self.done = true;
                }
            }
        }
        for action in frame.model.take_actions() {
            frame.services.event(match action {
                Action::Cue(name) => Event::Cue(name),
                Action::FadeMusic(ticks) => Event::StopBgm(ticks as i32),
            })?;
        }
        Ok(0)
    }
}
pub(in crate::app::higurashi) struct Motion {
    pub model: Model,
    pub scrollbar: Scrollbar,
    tasks: ModeStack<Task>,
    pub(super) borrowed: Vec<Kind>,
    bgm_id: Option<i32>,
    pub request: Option<SceneRequest>,
}
impl Motion {
    pub fn new(model: Model, captured: bool, scenario: &HigurashiScenario) -> Self {
        let bgm_id = if model.alternate {
            scenario
                .bgm_info
                .iter()
                .position(|b| b.name.as_str() == "org/hou/Tip")
                .map(|i| i as i32)
        } else {
            Some(77)
        };
        let mut tasks = ModeStack::default();
        tasks.push(Task {
            kind: Kind::First,
            phase: 0,
            done: false,
            clock: Some(OverlayClock::captured(captured)),
            callback: None,
        });
        Self {
            model,
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            tasks,
            borrowed: vec![],
            bgm_id,
            request: None,
        }
    }
    pub(super) fn first(&self) -> Option<&Task> {
        self.tasks.iter().find(|t| t.kind == Kind::First)
    }
    #[cfg(test)]
    pub(super) fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter()
    }
    pub(super) fn advance(
        &mut self,
        status: u32,
        mask_present: bool,
        atlas_present: bool,
        delta: Ticks,
        services: &mut dyn Services,
    ) -> Result<()> {
        self.tasks.poll(
            delta,
            1,
            &mut Frame {
                model: &mut self.model,
                status,
                mask_present,
                bgm_id: self.bgm_id,
                services,
                request: &mut self.request,
            },
        )?;
        self.borrowed.clear();
        self.tasks.draw(|t| self.borrowed.push(t.kind));
        self.model
            .advance_with_metrics(delta, atlas_present.then_some(&mut self.scrollbar));
        Ok(())
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.contact(p.x, p.y)
    }
    fn reset(&mut self) {
        self.hit = -1;
        self.hovered = -1;
        self.pressed = -1;
    }
    fn press(&mut self, _: Vec2) {
        self.press_contact();
    }
    fn release(&mut self, _: Vec2) {
        self.confirm_contact();
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        Model::drag(self, previous.y, current.y);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.inertia(velocity.y);
    }
    fn cancel(&mut self) {
        self.release_contact();
    }
}

#[cfg(test)]
mod tests;
