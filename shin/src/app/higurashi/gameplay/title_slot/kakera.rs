//! Updated2714c dispatch of656c0/17490 owned Kakera and ADV factories.
use super::*;
use crate::app::higurashi::{
    kakera_menu::opening::Opening as KakeraOpening, runtime_game_data::RuntimeGameData,
};

pub(super) struct Continuation {
    pub(super) seeds: Seeds,
    pub(super) clock:
        Box<dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send>,
    pub(super) history: crate::adv::higurashi::read_history::MessageReadHistory,
    pub(super) choices: crate::adv::higurashi::choice::ChoiceHistory,
    pub(super) autosave: crate::adv::higurashi::autosave::AutosaveMemory,
    pub(super) mode_ids: (u64, u64),
}

#[cfg(test)]
mod tests;
pub(super) struct Return {
    pub data: RuntimeGameData,
    pub flags: u32,
    pub(super) globals: Option<Globals>,
    pub(super) bindings: Option<Bindings>,
    pub(super) snapshot: Option<RenderTexture>,
    pub(super) seeds: Option<Seeds>,
}
impl TitleSlot {
    /// One platform frame:accept the previous frame's factory before updating
    /// the newly installed scene. Requests produced below stay pending until
    /// the next call,with this frame's composed output still available to draw.
    pub async fn update_frame(
        &mut self,
        context: &mut PreRenderContext<'_, '_, '_, '_>,
        composed: &RenderTexture,
        extent: (u16, u16),
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        policy: Policy,
        audio_policy: AudioFramePolicy,
    ) -> Result<Option<Status>> {
        // Reject a bad frame before accepting/retiring any owned factory.
        crate::adv::higurashi::host::frame_delta(elapsed)?;
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Invalid active-scene input extent"
        );
        ensure!(
            self.title_demo_request.is_none(),
            "Dispatch the retained TitleDemo factory before updating its source"
        );
        match &self.pending {
            Some(Pending::Movie | Pending::PreparedMovie(_)) => {
                self.dispatch_movie_gallery(context, composed, extent)?;
            }
            Some(Pending::MovieTitle(_)) => {
                self.dispatch_movie_title(context, composed)?;
            }
            Some(Pending::Music | Pending::PreparedMusic(_)) => {
                self.dispatch_music_gallery(context, composed, extent)?;
            }
            Some(Pending::MusicTitle(_)) => {
                self.dispatch_music_title(context, composed)?;
            }
            Some(Pending::Cg | Pending::PreparedCg(_)) => {
                self.dispatch_cg_gallery(context, composed, extent)?;
            }
            Some(Pending::CgTitle(_)) => {
                self.dispatch_cg_title(context, composed)?;
            }
            Some(
                Pending::ScenarioSelect(_)
                | Pending::Origin(_)
                | Pending::PreparedScenarioSelect(_),
            ) => {
                self.dispatch_scenario_select(context, composed, extent)?;
            }
            Some(Pending::PickerTitle(_)) => {
                self.dispatch_picker_title(context, composed)?;
            }
            Some(Pending::Quiz(_)) => {
                self.dispatch_quiz(context, composed, extent)?;
            }
            Some(Pending::Tips(_) | Pending::PreparedTips(_)) => {
                self.dispatch_tips(context, composed, extent)?;
            }
            Some(Pending::TipsReturn(_)) => {
                if self.pending_origin().is_some() {
                    self.dispatch_scenario_select(context, composed, extent)?;
                } else {
                    self.dispatch_tips_return(context, composed)?;
                }
            }
            Some(Pending::Title(_)) => {
                self.dispatch_title_return(context, composed)?;
            }
            Some(Pending::Kakera(_)) => {
                self.dispatch_kakera(context, composed, extent)?;
            }
            Some(Pending::Gameplay(_)) => {
                self.dispatch_kakera_return(context, composed)?;
            }
            Some(Pending::Browser(_)) => {
                self.dispatch_gameplay_replacement(context, composed)?;
            }
            Some(Pending::ManualLoad(_)) => {
                self.dispatch_manual_load(context, composed)?;
            }
            Some(Pending::ChartAdv(_)) => {
                self.dispatch_chart_adv(context, composed)?;
            }
            Some(Pending::BonusAdv(_)) => {
                self.dispatch_bonus_adv(context, composed)?;
            }
            Some(Pending::Story(..) | Pending::PreparedStory(_)) => {
                self.dispatch_story(context, composed, || {
                    std::array::from_fn(|_| crate::engine_random::next_seed())
                })?;
            }
            None => {}
        }
        if self.game.is_some() {
            self.game.as_mut().unwrap().resize_input(extent)?;
            self.update_game(elapsed, permission, buttons, pointer, policy, audio_policy)
                .await
                .map(Some)
        } else if self.movie_gallery.is_some() {
            self.movie_gallery.as_mut().unwrap().resize_input(extent)?;
            self.update_movie_gallery(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.music_gallery.is_some() {
            self.music_gallery.as_mut().unwrap().resize_input(extent)?;
            self.update_music_gallery(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.cg_gallery.is_some() {
            self.cg_gallery.as_mut().unwrap().resize_input(extent)?;
            self.update_cg_gallery(context, elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.scenario_select.is_some() {
            self.scenario_select
                .as_mut()
                .unwrap()
                .resize_input(extent)?;
            self.update_scenario_select(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.quiz.is_some() {
            self.quiz.as_mut().unwrap().resize_input(extent)?;
            self.update_quiz(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.tips.is_some() {
            self.tips.as_mut().unwrap().resize_input(extent);
            self.update_tips(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.kakera.is_some() {
            self.kakera.as_mut().unwrap().resize_input(extent);
            self.update_kakera(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else if self.title.is_some() {
            self.title.as_mut().unwrap().resize_menu_input(extent);
            self.update_title(elapsed, permission, buttons, pointer)?;
            Ok(None)
        } else {
            bail!("Higurashi scene factory has no active owner")
        }
    }
    pub fn pending_kakera(&self) -> bool {
        matches!(self.pending, Some(Pending::Kakera(_)))
    }
    pub fn pending_kakera_return(&self) -> bool {
        matches!(self.pending, Some(Pending::Gameplay(_)))
    }
    /// At the next platform boundary,copy the last whole composed frame,clear
    /// the old owner and drain its workers before invoking the menu constructor.
    pub fn dispatch_kakera(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        if !self.pending_kakera() {
            return Ok(false);
        }
        ensure!(
            self.game.is_some() && self.title.is_none() && self.kakera.is_none(),
            "Kakera factory requires the retained source Gameplay"
        );
        ensure!(
            self.continuation.is_none(),
            "Previous Kakera continuation was not consumed"
        );
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let Some(Pending::Kakera(request)) = self.pending.take() else {
            unreachable!()
        };
        let mut old = self.game.take().unwrap();
        let seeds = old.seeds.clone();
        let clock = std::mem::replace(
            &mut old.clock_source,
            Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
        );
        let mode_ids = old.host().unwrap().next_mode_ids();
        let (mut globals, history, choices, autosave) = old.into_globals_and_histories();
        // Voice/SE belong to the retired ADV;global BGM and its events survive.
        globals.audio.reset_adv_for_replacement();
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
        //656c0's integer is the616a0 selected-fragment argument,including0
        // produced by KAKERA. It is not an alternate-game selector.
        self.kakera = Some(KakeraOpening::new(
            globals,
            request.mode,
            request.data,
            Some(snapshot),
            extent,
        ));
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn update_kakera(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch the pending Kakera return before updating the old menu"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let menu = self
            .kakera
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No active Kakera menu"))?;
        #[cfg(test)]
        self.reference_scene_events.push(3);
        menu.update(delta, permission, buttons, pointer)?;
        if let Some(request) = menu.take_gameplay_request() {
            self.pending = Some(Pending::Gameplay(Return {
                data: request.data,
                flags: request.flags,
                globals: None,
                bindings: None,
                snapshot: None,
                seeds: None,
            }));
        }
        //2714c advances the global clock after the scene with full elapsed time.
        menu.globals.clock.advance(elapsed.as_f32());
        menu.globals.data.play_seconds = menu.globals.clock.saved_seconds();
        Ok(())
    }
    /// Retain the complete factory payload through preflight failures. Once
    /// retired,the globals/bindings/capture survive a constructor retry too.
    pub fn dispatch_kakera_return(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::Gameplay(transfer)) = &mut self.pending else {
            return Ok(false);
        };
        ensure!(
            self.continuation.is_some() || transfer.seeds.is_some(),
            "Menu return has no retained constructor seed owner"
        );
        if transfer.globals.is_none() {
            let globals = if let Some(quiz) = &self.quiz {
                &quiz.globals
            } else if let Some(picker) = &self.scenario_select {
                &picker.globals
            } else if let Some(tips) = &self.tips {
                &tips.globals
            } else {
                &self
                    .kakera
                    .as_ref()
                    .context("Menu return lost its source scene")?
                    .globals
            };
            let mut bindings = Bindings::new_runtime(&globals.data, &transfer.data)?;
            if let Some(slots) = &globals.runtime_saves {
                bindings.autosave = slots.autosave();
            }
            let entry = bindings.progress.progress().active_entry();
            let fork = CheckpointMemory::new(Vec::new()).fork_for_restore(
                entry.save_position,
                entry.replay_target,
                &bindings.progress,
            )?;
            ReplaySession::new(globals.common.scenario.clone(), fork)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = if let Some(quiz) = self.quiz.take() {
                quiz.into_globals()
            } else if let Some(picker) = self.scenario_select.take() {
                picker.into_globals()
            } else if let Some(tips) = self.tips.take() {
                tips.into_globals()
            } else {
                self.kakera.take().unwrap().into_globals()
            };
            #[cfg(test)]
            self.reference_scene_events.push(1);
            transfer.bindings = Some(bindings);
            transfer.globals = Some(globals);
            transfer.snapshot = Some(snapshot);
        }
        let seeds = self
            .continuation
            .as_ref()
            .map(|c| c.seeds.clone())
            .or_else(|| transfer.seeds.clone())
            .unwrap();
        let runner = Gameplay::prepare_runner(transfer.globals.as_ref().unwrap(), &seeds)?;
        let snapshot = transfer
            .snapshot
            .as_ref()
            .unwrap()
            .copy_snapshot(&mut context.render_clone_ctx());
        let continuation = self.continuation.take();
        let mut bindings = transfer.bindings.take().unwrap();
        let clock_and_ids = if let Some(continuation) = continuation {
            bindings.history = continuation.history;
            bindings.choices = continuation.choices;
            bindings.autosave = continuation.autosave;
            Some((continuation.clock, continuation.mode_ids))
        } else {
            None
        };
        let mut game = Gameplay::from_prepared(
            transfer.globals.take().unwrap(),
            bindings,
            runner,
            transfer.flags,
            snapshot,
            seeds,
        );
        if let Some((clock, ids)) = clock_and_ids {
            game.clock_source = clock;
            game.host_mut().unwrap().set_next_mode_ids(ids);
        }
        self.game = Some(game);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
}
