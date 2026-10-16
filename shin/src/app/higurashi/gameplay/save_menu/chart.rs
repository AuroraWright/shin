//!1ca2c/206f8: actual VM route/full group to a borrowed Item4 modal child.
use super::*;
use crate::app::higurashi::chart::{
    catalog::Catalog,
    owner::{Owner as Chart, Request as ChartRequest},
};
impl Frame<'_, '_> {
    pub(super) fn construct_chart(&mut self, request: ChildRequest) -> Result<()> {
        ensure!(
            self.chart.is_none()
                && self.menu.is_none()
                && self.config.is_none()
                && self.manual.is_none(),
            "Chart constructor lost its exclusive native child boundary"
        );
        let catalog = Arc::new(Catalog::from_scenario(&self.globals.common.scenario)?);
        let owner = Chart::new(
            Environment {
                common: self.globals.common.clone(),
                server: self.globals.server.clone(),
                sounds: self.globals.sounds.clone(),
                storage: self.globals.storage.clone(),
            },
            catalog,
            self.bindings.progress.progress().extra_word as i32,
            self.bindings.progress.persistent().get(0),
            &self.bindings.progress.chart_unlocks.saved_words(),
            &self.bindings.memory.extra_words,
            self.extent,
            self.chart_seed.clone(),
        )?;
        *self.chart = Some(RetainedChart {
            id: request.id,
            owner,
        });
        Ok(())
    }
    pub(super) fn poll_chart(
        &mut self,
        request: ChildRequest,
        delta: Ticks,
        permission: bool,
    ) -> Result<bool> {
        let menu = self
            .chart
            .as_mut()
            .context("Chart child lost its app owner")?;
        ensure!(
            menu.id == request.id,
            "Chart ID differs from borrowed host mode"
        );
        let input = self.chart_input;
        menu.owner.poll(
            delta,
            permission,
            crate::app::higurashi::chart::model::Input {
                buttons: input.map_or(self.buttons, |input| input.buttons(self.buttons)),
                axes: input.map_or([0; 4], |input| input.axes),
                wheel_scroll: input.map_or(0., |input| input.wheel_scroll),
            },
            self.pointer,
        )?;
        let mut returned = false;
        while let Some(event) = menu.owner.take_request() {
            match event {
                ChartRequest::ReturnSource(1) => returned = true,
                ChartRequest::ReturnSource(_) => {}
                ChartRequest::StartAdv { data, flags } => {
                    ensure!(self.chart_adv.is_none(), "Chart queued two ADV factories");
                    *self.chart_adv = Some((data, flags));
                }
            }
        }
        Ok(returned)
    }
}
