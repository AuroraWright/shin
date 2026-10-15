//! b2be4/b2cc8 full manual slot ->17490 flags0,next2714c frame boundary.
use super::*;
use crate::app::higurashi::runtime_game_data::RuntimeGameData;

pub(super) struct Transfer {
    pub(super) data: RuntimeGameData,
    pub(super) flags: u32,
    pub(super) globals: Option<Globals>,
    pub(super) bindings: Option<Bindings>,
    pub(super) snapshot: Option<RenderTexture>,
    seeds: Option<Seeds>,
    clock: Option<Box<dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send>>,
    mode_ids: Option<(u64, u64)>,
}

#[cfg(test)]
mod tests;
impl Transfer {
    pub fn new(data: RuntimeGameData, flags: u32) -> Self {
        Self {
            data,
            flags,
            globals: None,
            bindings: None,
            snapshot: None,
            seeds: None,
            clock: None,
            mode_ids: None,
        }
    }
}
impl TitleSlot {
    pub fn pending_manual_load(&self) -> bool {
        matches!(self.pending, Some(Pending::ManualLoad(_)))
    }
    /// Keep the source playable through fallible preflight. Once retired,
    /// retain the complete payload and global owners for constructor retry.
    pub fn dispatch_manual_load(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        if !self.pending_manual_load() {
            return Ok(false);
        }
        if let Some(Pending::ManualLoad(transfer)) = &self.pending {
            ensure!(transfer.flags == 0, "Manual Load requires native flags0");
        }
        self.dispatch_full_menu_adv(context, composed)
    }
    pub fn pending_chart_adv(&self) -> bool {
        matches!(self.pending, Some(Pending::ChartAdv(_)))
    }
    pub fn dispatch_chart_adv(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        if !self.pending_chart_adv() {
            return Ok(false);
        }
        self.dispatch_full_menu_adv(context, composed)
    }
    #[cfg(test)]
    pub fn pending_bonus_adv(&self) -> bool {
        matches!(self.pending, Some(Pending::BonusAdv(_)))
    }
    pub fn dispatch_bonus_adv(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::BonusAdv(transfer)) = &self.pending else {
            return Ok(false);
        };
        ensure!(
            self.game.is_some() || transfer.globals.is_some(),
            "Bonus ADV lost its source or retained factory"
        );
        self.dispatch_full_menu_adv(context, composed)
    }
    fn dispatch_full_menu_adv(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(
            Pending::ManualLoad(transfer)
            | Pending::ChartAdv(transfer)
            | Pending::BonusAdv(transfer),
        ) = &mut self.pending
        else {
            return Ok(false);
        };
        ensure!(
            transfer.flags == 0,
            "Menu ADV replacement requires native flags0"
        );
        if transfer.globals.is_none() {
            // Publish globals before the fresh109bc constructor reads them.
            // Old VM checkpoint/response cursors belong only to the retired ADV.
            if let Some(game) = &mut self.game {
                game.bindings.publish_globals(&mut game.globals.data);
                if let Some(slots) = &mut game.globals.runtime_saves {
                    slots.update_auto(&game.bindings.autosave);
                }
            }
            let (saved, slots, scenario) = if let Some(game) = &self.game {
                (
                    &game.globals.data,
                    &game.globals.runtime_saves,
                    &game.globals.common.scenario,
                )
            } else {
                let title = self
                    .title
                    .as_ref()
                    .context("Manual Load lost its source scene")?;
                (
                    &title.globals.data,
                    &title.globals.runtime_saves,
                    &title.globals.common.scenario,
                )
            };
            let mut bindings = Bindings::new_runtime(saved, &transfer.data)?;
            if let Some(slots) = slots {
                bindings.autosave = slots.autosave();
            }
            let entry = bindings.progress.progress().active_entry();
            let fork = CheckpointMemory::new(Vec::new()).fork_for_restore(
                entry.save_position,
                entry.replay_target,
                &bindings.progress,
            )?;
            ReplaySession::new(scenario.clone(), fork)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = if let Some(mut old) = self.game.take() {
                transfer.mode_ids = Some(old.host().unwrap().next_mode_ids());
                transfer.seeds = Some(old.seeds.clone());
                transfer.clock = Some(std::mem::replace(
                    &mut old.clock_source,
                    Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
                ));
                old.into_globals_and_histories().0
            } else {
                if let Some(continuation) = self.continuation.take() {
                    transfer.mode_ids = Some(continuation.mode_ids);
                    transfer.seeds = Some(continuation.seeds);
                    transfer.clock = Some(continuation.clock);
                } else {
                    transfer.seeds = Some(Arc::new(Mutex::new(Box::new(|| {
                        std::array::from_fn(|_| crate::engine_random::next_seed())
                    }))));
                }
                self.title.take().unwrap().into_globals()
            };
            #[cfg(test)]
            self.reference_scene_events.push(1);
            transfer.bindings = Some(bindings);
            transfer.globals = Some(globals);
            transfer.snapshot = Some(snapshot);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(
            Pending::ManualLoad(transfer)
            | Pending::ChartAdv(transfer)
            | Pending::BonusAdv(transfer),
        ) = &mut self.pending
        else {
            unreachable!()
        };
        let seeds = transfer.seeds.as_ref().unwrap();
        let runner = Gameplay::prepare_runner(transfer.globals.as_ref().unwrap(), seeds)?;
        let snapshot = transfer
            .snapshot
            .as_ref()
            .unwrap()
            .copy_snapshot(&mut context.render_clone_ctx());
        let mut game = Gameplay::from_prepared(
            transfer.globals.take().unwrap(),
            transfer.bindings.take().unwrap(),
            runner,
            transfer.flags,
            snapshot,
            seeds.clone(),
        );
        if let Some(clock) = transfer.clock.take() {
            game.clock_source = clock;
        }
        if let Some(ids) = transfer.mode_ids.take() {
            game.host_mut().unwrap().set_next_mode_ids(ids);
        }
        self.game = Some(game);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
}
