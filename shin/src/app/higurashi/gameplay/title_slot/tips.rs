//! Next-platform-frame TIPS factories and full story/title return owners.
use super::*;
#[cfg(test)]
use crate::app::higurashi::tips_menu::factory::Request;
use crate::app::higurashi::{
    tips_menu::{SceneRequest, opening::Opening as TipsOpening},
    title_demo::TitleRequest,
};

impl TitleSlot {
    pub fn pending_tips(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending::Tips(_) | Pending::PreparedTips(_))
        )
    }
    pub fn pending_origin(&self) -> Option<i32> {
        match self.pending {
            Some(Pending::Origin(selected)) => Some(selected),
            Some(Pending::PreparedScenarioSelect(ref transfer)) => transfer.origin_selected,
            Some(Pending::TipsReturn(SceneRequest::Origin { selected })) => Some(selected),
            _ => None,
        }
    }
    /// Callers queue an owned copy exactly as275b0 does. It is accepted at the
    /// next frame boundary;the current title/ADV remains alive until preflight.
    #[cfg(test)]
    pub fn queue_tips(&mut self, request: Request) {
        self.pending = Some(Pending::Tips(request));
    }
    pub fn dispatch_tips(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        if !self.pending_tips() {
            return Ok(false);
        }
        ensure!(
            self.tips.is_none() && self.kakera.is_none(),
            "TIPS factory requires source title/ADV"
        );
        if let Some(Pending::Tips(request)) = &self.pending {
            ensure!(extent.0 > 0 && extent.1 > 0, "Invalid TIPS input extent");
            if let Some(game) = &self.game {
                request.preflight(
                    &game.globals.common.scenario,
                    &game.globals.data.save_vectors,
                )?;
                ensure!(
                    self.continuation.is_none(),
                    "TIPS factory has an unconsumed continuation"
                );
            } else if let Some(picker) = &self.scenario_select {
                request.preflight(
                    &picker.globals.common.scenario,
                    &picker.globals.data.save_vectors,
                )?;
            } else {
                let title = self
                    .title
                    .as_ref()
                    .context("TIPS factory lost its source")?;
                request.preflight(
                    &title.globals.common.scenario,
                    &title.globals.data.save_vectors,
                )?;
            }
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let Some(Pending::Tips(request)) = self.pending.take() else {
                unreachable!()
            };
            let globals = if let Some(mut old) = self.game.take() {
                let seeds = old.seeds.clone();
                let clock = std::mem::replace(
                    &mut old.clock_source,
                    Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
                );
                let mode_ids = old.host().unwrap().next_mode_ids();
                let (mut globals, history, choices, autosave) = old.into_globals_and_histories();
                globals.audio.reset_adv_for_replacement();
                self.continuation = Some(Continuation {
                    seeds,
                    clock,
                    history,
                    choices,
                    autosave,
                    mode_ids,
                });
                globals
            } else if let Some(picker) = self.scenario_select.take() {
                picker.into_globals()
            } else {
                self.title.take().unwrap().into_globals()
            };
            self.pending = Some(Pending::PreparedTips(TipsTransfer {
                request,
                globals: Some(globals),
                snapshot: Some(snapshot),
                extent,
            }));
            #[cfg(test)]
            self.reference_scene_events.push(1);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedTips(transfer)) = &mut self.pending else {
            unreachable!()
        };
        self.tips = Some(TipsOpening::new_retained(
            &mut transfer.globals,
            transfer.request.data.clone(),
            transfer.request.selected,
            transfer.request.alternate,
            &mut transfer.snapshot,
            transfer.extent,
        )?);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn update_tips(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending TIPS scene before updating its source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let menu = self.tips.as_mut().context("No active TIPS scene")?;
        #[cfg(test)]
        self.reference_scene_events.push(3);
        let date = self.continuation.as_mut().map(|c| {
            &mut *c.clock
                as &mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>
        });
        menu.update_with_date(delta, permission, buttons, pointer, date)?;
        if let Some(request) = menu.take_scene_request() {
            match request {
                SceneRequest::Story { data, flags } => {
                    self.pending = Some(Pending::Gameplay(Return {
                        data,
                        flags,
                        globals: None,
                        bindings: None,
                        snapshot: None,
                        seeds: Some(
                            self.continuation
                                .as_ref()
                                .map(|c| c.seeds.clone())
                                .unwrap_or_else(|| {
                                    Arc::new(Mutex::new(Box::new(|| {
                                        std::array::from_fn(|_| crate::engine_random::next_seed())
                                    })))
                                }),
                        ),
                    }))
                }
                SceneRequest::Origin { selected } => self.pending = Some(Pending::Origin(selected)),
                other => self.pending = Some(Pending::TipsReturn(other)),
            }
        }
        menu.globals.clock.advance(elapsed.as_f32());
        menu.globals.data.play_seconds = menu.globals.clock.saved_seconds();
        Ok(())
    }
    pub fn dispatch_tips_return(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::TipsReturn(SceneRequest::Title { selected, mode })) = &self.pending
        else {
            return Ok(false);
        };
        let request = TitleRequest {
            selected: *selected,
            mode: *mode,
        };
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let globals = self
            .tips
            .take()
            .context("TIPS title return lost its source")?
            .into_globals();
        #[cfg(test)]
        self.reference_scene_events.push(1);
        self.title = Some(Opening::new(
            TitleTransition { request, globals },
            Some(snapshot),
        ));
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
