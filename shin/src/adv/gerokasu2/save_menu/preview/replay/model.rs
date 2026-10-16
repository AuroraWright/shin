//! D5B80's specialized preview callbacks, independent of live save replay.
//! No retained commands, checkpoints, reforks, globals, audio or Message owner.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callback<'a> {
    None,
    Exit { continue_running: bool },
    PersistentGet,
    MessageSet { auto_wait: bool },
    MessageWait { signal: i32 },
    MessageClose,
    Selection,
    Quiz,
    SaveInfo { level: usize, text: &'a str },
    Autosave,
    EventBegin { event: i32 },
    EventEnd,
    ResumeSet,
    Resume,
    PageBack,
    Wipe,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effect {
    /// A SELECT/QUIZ stopped at its entry boundary returns no result.
    pub response: Option<i32>,
    /// QUIZ applies its returned result to the addressed integer layer bank.
    pub complete_quiz: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controller {
    pub target: u32,
    pub boundary: u32,
    pub ready: bool,
    pub event: i32,
    pub asynchronous: bool,
    pub message_present: bool,
    pub back_started: bool,
    pub current_labels: [String; 2],
    pub saved_labels: [String; 2],
    route: Vec<u8>,
    pub response_bit_position: usize,
}

impl Controller {
    pub fn new(target: u32, route: Vec<u8>) -> Self {
        Self {
            target,
            boundary: 0,
            ready: target == 0,
            event: -1,
            asynchronous: false,
            message_present: false,
            back_started: false,
            current_labels: Default::default(),
            saved_labels: Default::default(),
            route,
            response_bit_position: 0,
        }
    }

    fn advance(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        self.ready |= self.boundary >= self.target;
    }

    fn bits(&mut self, count: usize) -> u32 {
        let mut value = 0;
        for _ in 0..count {
            let bit = self.response_bit_position;
            value = (value << 1)
                | u32::from(self.route.get(bit / 8).copied().unwrap_or(0) >> (7 - bit % 8) & 1);
            self.response_bit_position += 1;
        }
        value
    }

    pub fn apply(&mut self, callback: Callback<'_>) -> Result<Effect, &'static str> {
        let mut effect = Effect::default();
        match callback {
            Callback::None => {}
            Callback::Exit { continue_running } => self.ready |= !continue_running,
            Callback::PersistentGet => {
                effect.response = Some(if self.bits(1) != 0 {
                    self.bits(1) as i32
                } else {
                    i32::from(self.bits(16) as i16)
                });
            }
            Callback::MessageSet { auto_wait } => {
                if self.event < 0 && !self.back_started {
                    self.advance();
                }
                self.asynchronous = !auto_wait;
                self.message_present = true;
            }
            Callback::MessageWait { signal } => {
                if signal < 0 {
                    self.asynchronous = false;
                }
            }
            Callback::MessageClose => {
                if !self.asynchronous {
                    self.message_present = false;
                }
            }
            Callback::Selection | Callback::Quiz => {
                if !self.message_present {
                    self.advance();
                }
                if !self.ready {
                    effect.response = Some(self.bits(if callback == Callback::Selection {
                        4
                    } else {
                        6
                    }) as i32);
                    effect.complete_quiz = callback == Callback::Quiz;
                    self.advance();
                }
            }
            Callback::SaveInfo { level, text } => {
                let Some(label) = self.current_labels.get_mut(level) else {
                    return Err("Ger preview SAVEINFO level is outside two labels");
                };
                *label = text.to_owned();
            }
            Callback::Autosave => {
                self.advance();
                self.saved_labels.clone_from(&self.current_labels);
            }
            Callback::EventBegin { event } => {
                self.advance();
                self.event = event;
            }
            Callback::EventEnd => {
                self.advance();
                self.event = -1;
            }
            Callback::ResumeSet => self.advance(),
            Callback::Resume => self.ready = true,
            Callback::PageBack => self.back_started = true,
            Callback::Wipe => self.back_started = false,
        }
        Ok(effect)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
