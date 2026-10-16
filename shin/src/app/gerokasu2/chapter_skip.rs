//! ADV Plus/Minus chapter navigation: `115B8` bit8 -> `1264C`, bit9 ->
//! `129E4`. The Question is an ADV child (`23670`, pushed to `ADV+1B0`): the
//! scene keeps running, while the child group returns false to `115B8`, so
//! no ADV button is dispatched until it closes. Yes callbacks `18DB0`/`18A84`
//! store an `E820` fork at `ADV+4B8`; `105E0` starts reconstruction mode 0.
use super::*;
use crate::adv::gerokasu2::{
    config::dialog::GerDialog,
    replay::{ReplaySession, chapter_skip},
};

/// `F96A5`, selected while settings `+0xBB` (skip unread disallowed) is set.
pub(super) const NEXT_UNREAD: &str =
    "次のチャプター、未読メッセージ、選択肢まで@rスキップします。よろしいですか？";
/// `FAA2A`, selected while settings `+0xBB` is zero.
pub(super) const NEXT: &str = "次のチャプターか選択肢までスキップします。@rよろしいですか？";
/// `F9FC6`.
pub(super) const PREVIOUS: &str = "前のチャプターに戻ります。@rよろしいですか？";

pub(super) struct ChapterQuestion {
    pub dialog: GerDialog,
    pub forward: bool,
}

impl GerHost {
    /// The `115B8` gate reached by bit8/bit9 (live VM, no reconstruction,
    /// VM `92B0`). Unlike the menus it does not require `event < 0`.
    fn chapter_permitted(&self) -> bool {
        self.scene.reconstruction_phase().is_none()
            && !self.scene.is_exiting()
            && self.scene.playback().system_controls
    }
    fn next_chapter(&self) -> Option<ReplaySession> {
        let playback = self.scene.playback();
        chapter_skip::next(
            self.scene.scenario(),
            self.scene.save_session(),
            &playback.seen_messages,
            !playback.disallow_skip_unread,
        )
        .unwrap_or_else(|error| {
            tracing::error!(%error, "Ger chapter forward search failed");
            None
        })
    }
    /// Plus/Minus dispatch; returns whether the press was consumed.
    pub(super) fn chapter_input(&mut self, input: &EnumMap<AppAction, ActionState>) -> bool {
        let plus = input[AppAction::NextChapter].is_clicked;
        if !(plus || input[AppAction::PreviousChapter].is_clicked) {
            return false;
        }
        self.chapter_request(plus, input[AppAction::PointerClick].is_held)
    }
    /// `1264C` (Plus) / `129E4` (Minus), from the buttons or the toolbar.
    pub(super) fn chapter_request(&mut self, plus: bool, held: bool) -> bool {
        if !self.chapter_permitted() {
            return false;
        }
        // 1264C preflights EBB0; without a target only the error cue plays.
        if plus && self.next_chapter().is_none() {
            self.scene.play_system_cue("error");
            return true;
        }
        self.scene.playback_mut().modes.reset();
        self.scene.play_system_cue("decide");
        let prompt = match (plus, self.scene.playback().disallow_skip_unread) {
            (false, _) => PREVIOUS,
            (true, true) => NEXT_UNREAD,
            (true, false) => NEXT,
        };
        // 20AAC(q, 1): Yes/No with No selected, cancellable.
        match GerDialog::load(
            self.scene.assets(),
            prompt,
            &["はい", "いいえ"],
            1,
            true,
            held,
        ) {
            Ok(dialog) => {
                self.chapter = Some(ChapterQuestion {
                    dialog,
                    forward: plus,
                });
                self.cancel_input();
            }
            Err(error) => tracing::error!(%error, "Ger chapter Question preparation failed"),
        }
        true
    }
    /// `12C4C`: B during an event forks past it (`ECC0`) with the decide cue
    /// and starts reconstruction mode 0.
    pub(super) fn event_skip(&mut self) -> Result<()> {
        if !self.chapter_permitted() {
            return Ok(());
        }
        let playback = self.scene.playback();
        let replay = chapter_skip::event(
            self.scene.scenario(),
            self.scene.save_session(),
            &playback.seen_messages,
            !playback.disallow_skip_unread,
        )?;
        if let Some(replay) = replay {
            self.scene.play_system_cue("decide");
            self.scene.begin_reconstruction(replay, 0)?;
        }
        Ok(())
    }
    /// `13538` (repeated Up, bit24): unless skipping, fork back to the
    /// previous message (the current one when no text is retained, VM
    /// `90F8`) with `decide` and reconstruction mode 1. Fewer than two
    /// normal messages (`2DD4`) plays `error`.
    pub(super) fn message_back(&mut self) -> Result<()> {
        if self.scene.reconstruction_phase().is_some()
            || self.scene.is_exiting()
            || self.scene.skip_active()
        {
            return Ok(());
        }
        let save = self.scene.save_session();
        let t = save.timeline;
        if t.normal_messages < 2 {
            self.scene.play_system_cue("error");
            return Ok(());
        }
        let target = if save.descriptors.message.text.is_some() {
            t.previous_message
        } else {
            t.current_message
        };
        let (fork, restored) = save.fork_for_restore(target, target);
        let replay = ReplaySession::new(self.scene.scenario().clone(), fork, restored)?;
        self.scene.play_system_cue("decide");
        self.scene.begin_reconstruction(replay, 1)
    }
    pub(super) fn update_chapter_question(
        &mut self,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        point: Option<Vec2>,
        seconds: f64,
    ) -> Result<()> {
        let armed = self.buttons_armed;
        let question = self.chapter.as_mut().unwrap();
        if armed {
            question.dialog.pointer_state(
                point.unwrap_or(Vec2::splat(-1.0)),
                input[AppAction::PointerClick].is_held,
                seconds,
            );
            question.dialog.keyboard(input);
        }
        let responses = question.dialog.update(delta);
        let cues = question.dialog.take_sounds();
        let forward = question.forward;
        for cue in cues {
            self.scene.play_system_cue(cue);
        }
        let Some(answer) = responses.iter().find(|r| r.final_flag).map(|r| r.button) else {
            return Ok(());
        };
        self.chapter = None;
        self.cancel_input();
        // 18DB0/18A84 act on stage 1, answer 0, VM 92B0 and ADV+4D0 clear.
        if answer != 0 || !self.chapter_permitted() {
            return Ok(());
        }
        let replay = if forward {
            match self.next_chapter() {
                Some(replay) => replay,
                None => {
                    self.scene.play_system_cue("error");
                    return Ok(());
                }
            }
        } else {
            chapter_skip::previous(self.scene.scenario(), self.scene.save_session())?
        };
        self.scene.begin_reconstruction(replay, 0)
    }
    pub(super) fn prepare_chapter_question(&mut self, pre: &mut PreRenderContext) -> Result<()> {
        match self.chapter.as_mut() {
            Some(question) => question.dialog.prepare(pre),
            None => Ok(()),
        }
    }
    pub(super) fn render_chapter_question(&self, pass: &mut RenderPass) -> Result<()> {
        match self.chapter.as_ref() {
            Some(question) => question.dialog.render(pass),
            None => Ok(()),
        }
    }
}
