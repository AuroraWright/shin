//! Original b6be0/b819c and b6d10/b8274 factories at the next frame boundary.
use super::*;
use crate::app::higurashi::scenario_select::{
    SceneRequest,
    opening::{Opening as PickerOpening, Prepared, origin_data},
};

pub(super) struct Transfer {
    prepared: Prepared,
    pub(super) globals: Option<Globals>,
    snapshot: Option<RenderTexture>,
    next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    pub(super) origin_selected: Option<i32>,
}

impl TitleSlot {
    pub fn pending_scenario_select(&self) -> bool {
        matches!(
            self.pending,
            Some(
                Pending::ScenarioSelect(_)
                    | Pending::Origin(_)
                    | Pending::PreparedScenarioSelect(_)
            )
        ) || self.pending_origin().is_some()
    }

    pub fn dispatch_scenario_select(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        self.dispatch_scenario_select_with_seed(
            context,
            composed,
            extent,
            Arc::new(crate::engine_random::next_seed),
        )
    }

    fn dispatch_scenario_select_with_seed(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
        next_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) -> Result<bool> {
        if !self.pending_scenario_select() {
            return Ok(false);
        }
        ensure!(
            self.scenario_select.is_none(),
            "Picker factory overlaps a live picker"
        );
        if !matches!(self.pending, Some(Pending::PreparedScenarioSelect(_))) {
            let (data, kind, selected, origin_selected) = match self.pending.as_ref().unwrap() {
                Pending::ScenarioSelect(request) => {
                    ensure!(
                        self.game.is_some(),
                        "SNRSEL factory lost its Gameplay source"
                    );
                    ensure!(
                        self.continuation.is_none(),
                        "SNRSEL has an unconsumed ADV continuation"
                    );
                    (request.data.clone(), request.kind, 0, None)
                }
                Pending::Origin(selected)
                | Pending::TipsReturn(crate::app::higurashi::tips_menu::SceneRequest::Origin {
                    selected,
                }) => {
                    ensure!(
                        self.title.is_some() || self.tips.is_some(),
                        "Origin factory lost its title/TIPS source"
                    );
                    // Pure preflight uses a placeholder. The real b8274 seed
                    // is sampled only once, after the old scene is retired.
                    (origin_data(0), 5, *selected, Some(*selected))
                }
                _ => unreachable!(),
            };
            let mut prepared = {
                let globals = self.globals()?;
                Prepared::new(
                    &globals.common.scenario,
                    globals.data,
                    globals.server.contains("/append.txt"),
                    data,
                    kind,
                    selected,
                    extent,
                )?
            };
            // This is a pure copy of the prior whole composition, not a fresh
            // ADV redraw. Fallible schema preflight has left source intact.
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let queued_factory = self.pending.take();
            let globals = if let Some(mut old) = self.game.take() {
                let seeds = old.seeds.clone();
                let clock = std::mem::replace(
                    &mut old.clock_source,
                    Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
                );
                let mode_ids = old.host().unwrap().next_mode_ids();
                let (mut globals, history, choices, autosave) = old.into_globals_and_histories();
                globals.audio.retire_adv_for_scene();
                self.continuation = Some(Continuation {
                    seeds,
                    clock,
                    history,
                    choices,
                    autosave,
                    mode_ids,
                });
                globals
            } else if let Some(tips) = self.tips.take() {
                tips.into_globals()
            } else {
                self.title.take().unwrap().into_globals()
            };
            #[cfg(test)]
            self.reference_scene_events.push(1);
            if origin_selected.is_some() {
                prepared.data.progress.entries[0].random_seed = next_seed();
            }
            self.pending = Some(Pending::PreparedScenarioSelect(Transfer {
                prepared,
                globals: Some(globals),
                snapshot: Some(snapshot),
                next_seed,
                origin_selected,
            }));
            // Prepared retains the full factory payload across an error/retry.
            // The obsolete owned request can retire after its replacement copy.
            drop(queued_factory);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedScenarioSelect(transfer)) = &mut self.pending else {
            unreachable!()
        };
        self.scenario_select = Some(PickerOpening::new_retained(
            &mut transfer.globals,
            &transfer.prepared,
            &mut transfer.snapshot,
            transfer.next_seed.clone(),
        )?);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }

    pub fn update_scenario_select(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending picker return before updating its source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let picker = self
            .scenario_select
            .as_mut()
            .context("No active scenario picker")?;
        #[cfg(test)]
        self.reference_scene_events.push(3);
        picker.update(delta, permission, buttons, pointer)?;
        if let Some(request) = picker.take_scene_request() {
            self.pending = Some(match request {
                SceneRequest::Adv { data, flags } => Pending::Gameplay(Return {
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
                }),
                SceneRequest::Title { selected, mode } => {
                    Pending::PickerTitle(crate::app::higurashi::title_demo::TitleRequest {
                        selected,
                        mode: mode as u32,
                    })
                }
                SceneRequest::Tips {
                    selected,
                    alternate,
                } => Pending::Tips(crate::app::higurashi::tips_menu::factory::Request {
                    data: None,
                    selected,
                    alternate: u32::from(alternate),
                }),
            });
        }
        picker.globals.clock.advance(elapsed.as_f32());
        picker.globals.data.play_seconds = picker.globals.clock.saved_seconds();
        Ok(())
    }

    pub fn dispatch_picker_title(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::PickerTitle(request)) = &self.pending else {
            return Ok(false);
        };
        ensure!(
            self.scenario_select.is_some(),
            "Picker title return lost its source"
        );
        let request = *request;
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let globals = self.scenario_select.take().unwrap().into_globals();
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
