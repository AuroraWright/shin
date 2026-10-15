//! App payloads execute inside the host's borrowed MenuChild scope.
use super::*;
use crate::adv::higurashi::host::system_menu::{ChildId, ChildKind, ChildRequest, ChildServices};
use crate::app::higurashi::save_menu::{Environment, Owner, Prepared, Request};
mod chart;
pub(super) struct Retained {
    pub id: ChildId,
    pub owner: Owner,
}
pub(super) struct RetainedConfig {
    pub id: ChildId,
    pub owner: crate::app::higurashi::config::owner::Owner,
}
pub(super) struct RetainedManual {
    pub id: ChildId,
    pub owner: crate::app::higurashi::manual::owner::Owner,
}
pub(super) struct RetainedChart {
    pub id: ChildId,
    pub owner: crate::app::higurashi::chart::owner::Owner,
}
pub(super) struct Frame<'a, 'p> {
    pub menu: &'a mut Option<Retained>,
    pub config: &'a mut Option<RetainedConfig>,
    pub manual: &'a mut Option<RetainedManual>,
    pub chart: &'a mut Option<RetainedChart>,
    pub chart_input: Option<super::super::input_bridge::Analog>,
    pub chart_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    pub chart_adv: &'a mut Option<(super::super::runtime_game_data::RuntimeGameData, u32)>,
    pub bindings: &'a Bindings,
    pub globals: &'a mut Metadata,
    pub buttons: Buttons,
    pub pointer: &'a PointerFrame<'p>,
    pub date: &'a mut dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate>,
    pub load: &'a mut Option<(super::super::runtime_game_data::RuntimeGameData, u32)>,
    pub extent: (u16, u16),
}
impl ChildServices for Frame<'_, '_> {
    fn layer_settings(&mut self) -> Option<&mut shin_core::format::save::higurashi::SaveData> {
        Some(&mut self.globals.data)
    }
    fn construct_config(&mut self, request: ChildRequest, flags: u32) -> Result<()> {
        ensure!(
            request.kind == ChildKind::Item(2) && self.config.is_none() && self.menu.is_none(),
            "Config constructor lost its exclusive native child boundary"
        );
        let owner = crate::app::higurashi::config::owner::Owner::new(
            Environment {
                common: self.globals.common.clone(),
                server: self.globals.server.clone(),
                sounds: self.globals.sounds.clone(),
                storage: self.globals.storage.clone(),
            },
            &self.globals.data,
            flags,
            self.globals.server.contains("/append.txt"),
            self.extent,
        )?;
        *self.config = Some(RetainedConfig {
            id: request.id,
            owner,
        });
        Ok(())
    }
    fn construct(&mut self, request: ChildRequest) -> Result<()> {
        if request.kind == ChildKind::Item(4) {
            return self.construct_chart(request);
        }
        if request.kind == ChildKind::Item(3) {
            ensure!(
                self.manual.is_none() && self.config.is_none() && self.menu.is_none(),
                "Manual constructor lost its exclusive native child boundary"
            );
            let owner = crate::app::higurashi::manual::owner::Owner::new(
                Environment {
                    common: self.globals.common.clone(),
                    server: self.globals.server.clone(),
                    sounds: self.globals.sounds.clone(),
                    storage: self.globals.storage.clone(),
                },
                self.extent,
            )?;
            *self.manual = Some(RetainedManual {
                id: request.id,
                owner,
            });
            return Ok(());
        }
        if !matches!(request.kind, ChildKind::Item(0 | 1)) {
            return Ok(());
        }
        ensure!(
            self.menu.is_none(),
            "SystemMenu already retains a manual menu"
        );
        let current = if request.kind == ChildKind::Item(0) {
            Some(self.bindings.runtime_game_data()?)
        } else {
            None
        };
        let slots = self
            .globals
            .runtime_saves
            .as_ref()
            .context("Manual menu requires complete slots")?;
        let prepared = Prepared::new(
            current.as_ref(),
            &self.globals.data,
            slots,
            self.globals.server.contains("/append.txt"),
            self.extent,
        )?;
        let owner = Owner::from_prepared(
            Environment {
                common: self.globals.common.clone(),
                server: self.globals.server.clone(),
                sounds: self.globals.sounds.clone(),
                storage: self.globals.storage.clone(),
            },
            prepared,
        )?;
        *self.menu = Some(Retained {
            id: request.id,
            owner,
        });
        Ok(())
    }
    fn poll(&mut self, request: ChildRequest, delta: Ticks, permission: bool) -> Result<bool> {
        if request.kind == ChildKind::Item(4) {
            return self.poll_chart(request, delta, permission);
        }
        if request.kind == ChildKind::Item(3) {
            let manual = self
                .manual
                .as_mut()
                .context("Manual child lost its app owner")?;
            ensure!(
                manual.id == request.id,
                "Manual child ID differs from borrowed host mode"
            );
            manual
                .owner
                .poll(delta, permission, self.buttons, self.pointer)?;
            return Ok(manual
                .owner
                .take_requests()
                .into_iter()
                .any(|stage| stage == 1));
        }
        if request.kind == ChildKind::Item(2) {
            let menu = self
                .config
                .as_mut()
                .context("Config child lost its app owner")?;
            ensure!(
                menu.id == request.id,
                "Config child ID differs from borrowed host mode"
            );
            menu.owner.poll(
                &mut self.globals.data,
                self.globals.runtime_saves.as_ref(),
                &mut self.globals.platform_flag,
                delta,
                permission,
                self.buttons,
                self.pointer,
            )?;
            return Ok(menu
                .owner
                .take_requests()
                .into_iter()
                .any(|stage| stage == 1));
        }
        if !matches!(request.kind, ChildKind::Item(0 | 1)) {
            return Ok(false);
        }
        let retained = self
            .menu
            .as_mut()
            .context("Manual child lost its app owner")?;
        ensure!(
            retained.id == request.id,
            "Manual child ID differs from borrowed host mode"
        );
        let slots = self.globals.runtime_saves.as_mut().unwrap();
        retained.owner.poll(
            &mut self.globals.data,
            slots,
            delta,
            permission,
            self.buttons,
            self.pointer,
            Some(self.date),
        )?;
        let mut returned = false;
        for event in retained.owner.take_requests() {
            match event {
                Request::ReturnSource(1) => returned = true,
                Request::ReturnSource(_) => {}
                Request::Load { data, flags } => {
                    ensure!(self.load.is_none(), "Manual Load queued twice");
                    *self.load = Some((data, flags));
                }
            }
        }
        Ok(returned)
    }
    fn retired(&mut self, request: ChildRequest) {
        if self
            .chart
            .as_ref()
            .is_some_and(|menu| menu.id == request.id)
        {
            self.chart.take();
        }
        if self
            .manual
            .as_ref()
            .is_some_and(|menu| menu.id == request.id)
        {
            self.manual.take();
        }
        if self
            .config
            .as_ref()
            .is_some_and(|menu| menu.id == request.id)
        {
            self.config.take();
        }
        if self.menu.as_ref().is_some_and(|menu| menu.id == request.id) {
            self.menu
                .take()
                .unwrap()
                .owner
                .retire(&mut self.globals.data);
        }
    }
}
