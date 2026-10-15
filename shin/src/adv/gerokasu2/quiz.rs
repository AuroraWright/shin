//! Retained interactive owner for the later-engine QUIZ command.
use super::{
    layers::Gerokasu2Layers,
    save_commands::{SaveCapture, SaveServices},
};
use anyhow::Result;
use shin_core::vm::command::types::{LayerbankId, PlaneId};
pub trait QuizServices {
    /// Retain the actual interactive drawable; missing/noninteractive returns zero.
    fn bind_quiz(&mut self, target: Option<(PlaneId, LayerbankId)>) -> Result<()>;
    fn quiz_result(&mut self) -> Result<Option<i32>>;
    fn finish_quiz(&mut self);
}
pub struct QuizCommand {
    pub target: Option<(PlaneId, LayerbankId)>,
    bound: bool,
    before: SaveCapture,
    after: SaveCapture,
    result: Option<i32>,
    recorded: bool,
}
impl QuizCommand {
    pub fn new(target: Option<(PlaneId, LayerbankId)>) -> Self {
        Self {
            target,
            bound: false,
            before: Default::default(),
            after: Default::default(),
            result: None,
            recorded: false,
        }
    }
    pub fn bind(&mut self, services: &mut dyn QuizServices) -> Result<()> {
        if !self.bound {
            services.bind_quiz(self.target)?;
            self.bound = true;
        }
        Ok(())
    }
    pub fn capture_before(&mut self, services: &mut dyn SaveServices) -> Result<()> {
        self.before.poll(services)
    }
    pub fn poll(&mut self, services: &mut dyn QuizServices) -> Result<Option<i32>> {
        if self.result.is_none() {
            self.result = services.quiz_result()?;
        }
        Ok(self.result)
    }
    pub fn capture_after(
        &mut self,
        result: i32,
        layers: &mut Gerokasu2Layers,
        services: &mut dyn SaveServices,
    ) -> Result<()> {
        if !self.recorded {
            services.save_session().record_quiz(result);
            self.recorded = true;
        }
        self.after.poll(services)?;
        // Natived980 captures befored994 marks the bank completed.
        if let Some((_, bank)) = self.target {
            layers.complete_interaction(bank, result);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
