//! Original9fae0/a120c quiz and17490 score factories at the next frame boundary.
use super::*;
use crate::app::higurashi::quiz_scene::opening::Opening as QuizOpening;

impl TitleSlot {
    pub fn dispatch_quiz(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        self.dispatch_quiz_with_seed(context, composed, extent, crate::engine_random::next_seed)
    }
    fn dispatch_quiz_with_seed(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
        seed: impl FnOnce() -> u32,
    ) -> Result<bool> {
        let Some(Pending::Quiz(request)) = &self.pending else {
            return Ok(false);
        };
        ensure!(
            self.game.is_some()
                && self.title.is_none()
                && self.tips.is_none()
                && self.kakera.is_none()
                && self.quiz.is_none(),
            "Quiz factory requires source Gameplay"
        );
        ensure!(
            self.continuation.is_none(),
            "Quiz has an unconsumed continuation"
        );
        let old = self.game.as_ref().unwrap();
        let prepared = QuizOpening::prepare_scene(
            request.data.clone(),
            request.args,
            &old.globals.common.scenario,
            extent,
        )?;
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let pending = self.pending.take();
        let mut old = self.game.take().unwrap();
        let seeds = old.seeds.clone();
        let clock = std::mem::replace(
            &mut old.clock_source,
            Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
        );
        let mode_ids = old.host().unwrap().next_mode_ids();
        let (mut globals, history, choices, autosave) = old.into_globals_and_histories();
        globals.audio.retire_adv_for_scene();
        #[cfg(test)]
        self.reference_scene_events.push(1);
        self.continuation = Some(Continuation {
            seeds,
            clock,
            history,
            choices,
            autosave,
            mode_ids,
        });
        #[cfg(test)]
        self.reference_scene_events.push(2);
        self.quiz = Some(QuizOpening::from_prepared(
            globals,
            prepared,
            Some(snapshot),
            seed,
        ));
        // The queued factory copy outlives construction, then retires before
        // the new scene's first update, as in2714c.
        drop(pending);
        Ok(true)
    }
    pub fn update_quiz(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending quiz return before updating source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let quiz = self.quiz.as_mut().context("No active quiz scene")?;
        #[cfg(test)]
        self.reference_scene_events.push(3);
        quiz.update(delta, permission, buttons, pointer)?;
        if let Some(request) = quiz.take_scene_request() {
            self.pending = Some(Pending::Gameplay(Return {
                data: request.data,
                flags: request.flags,
                globals: None,
                bindings: None,
                snapshot: None,
                seeds: None,
            }));
        }
        quiz.globals.clock.advance(elapsed.as_f32());
        quiz.globals.data.play_seconds = quiz.globals.clock.saved_seconds();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
