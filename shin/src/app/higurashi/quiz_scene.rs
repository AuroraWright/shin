//! Updated9bb48 scene data,original catalogs and independent quiz randomness.
use super::runtime_game_data::RuntimeGameData;
use crate::{adv::higurashi::input::Buttons, layer::higurashi_properties::HigurashiTweener};
use anyhow::{Result, ensure};
use shin_core::time::Ticks;

pub(super) mod catalog;
pub(super) mod motion;
pub(super) mod opening;
mod random;
pub(super) mod resources;
#[cfg(test)]
mod tests;
pub(super) mod view;
pub(super) mod voice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Question {
    pub voice: &'static str,
    pub texture: &'static str,
    /// The original descriptor's first answer is correct;the view permutes it.
    pub answers: [&'static str; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct QuestionId(u16);
impl QuestionId {
    pub fn index(self) -> u16 {
        self.0
    }
    pub fn descriptor(self) -> &'static Question {
        if self.0 < 480 {
            &catalog::POOLS[self.0 as usize / 60][self.0 as usize % 60]
        } else {
            &catalog::SPECIAL[self.0 as usize - 480]
        }
    }
}
pub(super) const ANSWER_POSITIONS: [[f32; 2]; 4] = [
    [752.0, 828.0],
    [1340.0, 904.0],
    [752.0, 980.0],
    [168.0, 904.0],
];
pub(super) const ANSWER_RECTS: [[f32; 4]; 4] = [
    [684.0, 784.0, 1236.0, 916.0],
    [1272.0, 860.0, 1824.0, 992.0],
    [684.0, 936.0, 1236.0, 1068.0],
    [100.0, 860.0, 652.0, 992.0],
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    Voice { name: &'static str, reaction: bool },
    DisableInput,
}
pub(super) struct Model {
    pub data: RuntimeGameData,
    pub args: [i32; 3],
    pub clamped_argument: i32,
    pub score: i32,
    pub questions: [QuestionId; 10],
    pub marks: [i32; 11],
    pub rng: random::Random,
    pub current_question: i32,
    pub correct_answer: i32,
    pub chosen_answer: i32,
    pub answer_order: [usize; 4],
    pub contact: [i32; 3],
    pub reaction_variant: u32,
    pub closing: bool,
    /// Native180/1f8/248: scene fade, question scale and question opacity.
    pub clocks: [HigurashiTweener; 3],
    actions: Vec<Action>,
}
impl Model {
    pub fn new(data: RuntimeGameData, args: [i32; 3], seed: u32) -> Result<Self> {
        ensure!(
            (0..8).contains(&args[0]),
            "Quiz question catalog index is outside0..8"
        );
        let mut rng = random::Random::new(seed);
        let mut pool =
            std::array::from_fn::<_, 60, _>(|i| QuestionId((args[0] as usize * 60 + i) as u16));
        for first in 0..59 {
            let other = first + rng.uniform((60 - first) as u32) as usize;
            pool.swap(first, other);
        }
        let mut questions: [QuestionId; 10] = pool[..10].try_into().unwrap();
        if rng.uniform(10) == 0 {
            let position = 1 + rng.uniform(8) as usize;
            questions[position] = QuestionId(480 + position as u16);
        }
        Ok(Self {
            data,
            args,
            clamped_argument: args[2].clamp(0, 9),
            score: 0,
            questions,
            marks: [0; 11],
            rng,
            current_question: -1,
            correct_answer: -1,
            chosen_answer: -1,
            answer_order: [0, 1, 2, 3],
            contact: [-1; 3],
            reaction_variant: 0,
            closing: false,
            clocks: std::array::from_fn(|_| HigurashiTweener::new(0.0)),
            actions: Vec::new(),
        })
    }
    /// Actual9ce2c consumes three more draws from the scene's existing RNG.
    pub fn prepare_question(&mut self, index: usize) -> Result<()> {
        ensure!(
            index < self.questions.len(),
            "Quiz question index outside0..10"
        );
        self.answer_order = [0, 1, 2, 3];
        for first in 0..3 {
            let other = first + self.rng.uniform((4 - first) as u32) as usize;
            self.answer_order.swap(first, other);
        }
        self.correct_answer = self.answer_order.iter().position(|&i| i == 0).unwrap() as i32;
        self.current_question = index as i32;
        for (clock, value) in self.clocks[1..].iter_mut().zip([1.5, 0.0]) {
            clock.fast_forward_to(value);
            clock.control(1.0, Ticks::from_f32(10.0), 0)?;
        }
        self.chosen_answer = -1;
        self.actions.push(Action::Voice {
            name: self.questions[index].descriptor().voice,
            reaction: false,
        });
        self.reaction_variant = 0;
        self.contact[1] = -1;
        Ok(())
    }
    pub fn answers(&self) -> Option<[&'static str; 4]> {
        let index = usize::try_from(self.current_question).ok()?;
        let question = self.questions.get(index)?.descriptor();
        Some(self.answer_order.map(|i| question.answers[i]))
    }
    /// The native answer callback itself allows repeated invocations. Its
    /// controller/probe wrappers own the eligibility gates, not this callback.
    pub fn answer(&mut self, answer: i32) -> Result<()> {
        ensure!((0..4).contains(&answer), "Quiz answer index outside0..4");
        let index = usize::try_from(self.current_question)?;
        ensure!(index < 10, "Quiz has no current question");
        let context = usize::try_from(self.args[1])?;
        ensure!(
            context < catalog::REACTIONS.len(),
            "Quiz reaction context outside0..9"
        );
        let mut variant = self.rng.uniform(5) as usize;
        self.chosen_answer = answer;
        let correct = answer == self.correct_answer;
        self.actions
            .push(Action::Cue(if correct { "correct" } else { "incorrect" }));
        if correct {
            self.marks[index] = 1;
            self.score = self.score.wrapping_add(1);
            self.clamped_argument = (self.clamped_argument - 1).max(0);
        } else {
            self.marks[index] = 0;
            variant += 5;
        }
        self.actions.push(Action::Voice {
            name: catalog::REACTIONS[context][variant],
            reaction: true,
        });
        self.reaction_variant = variant as u32 + 1;
        Ok(())
    }
    fn input_ready(&self) -> bool {
        self.current_question >= 0 && self.chosen_answer < 0 && self.clocks[2].is_idle()
    }
    /// Actual9e878 consumes pressed directions in right/down/left/up priority.
    /// It clears the caller permission even when no answer is accepted.
    pub fn input(
        &mut self,
        permission: &mut bool,
        captured: bool,
        buttons: Buttons,
    ) -> Result<bool> {
        if !*permission {
            self.actions.push(Action::DisableInput);
        } else if !captured && self.input_ready() {
            for (bit, answer) in [(13, 1), (14, 2), (15, 3), (12, 0)] {
                if buttons.pressed & (1 << bit) != 0 {
                    self.answer(answer)?;
                    break;
                }
            }
        }
        *permission = false;
        Ok(!self.closing)
    }
    /// Actual9e990 always returns true; its half-open rectangles only update
    /// the contact selection after the question opacity becomes idle.
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        if self.input_ready() {
            self.contact[1] = ANSWER_RECTS
                .iter()
                .position(|r| x >= r[0] && y >= r[1] && x < r[2] && y < r[3])
                .map_or(-1, |i| i as i32);
        }
        true
    }
    pub fn release(&mut self) -> Result<()> {
        if self.contact[1] >= 0 {
            self.answer(self.contact[1])?;
        }
        Ok(())
    }
    pub fn reset_contact(&mut self) {
        self.contact = [-1; 3];
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        for clock in &mut self.clocks {
            clock.update(delta);
        }
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
}
