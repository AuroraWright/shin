//! e093c/e59b0: retained Chart modal, stage1 Top3 return, separate17490 factory.
use super::*;
use crate::app::higurashi::{
    chart::{
        catalog::Catalog,
        owner::{Owner, Request},
    },
    input_bridge::Analog,
};

impl Opening {
    pub(super) fn construct_chart(&mut self, selected: i32, return_id: i32) -> Result<()> {
        ensure!(self.chart.is_none(), "Title already retains a Chart child");
        let catalog = Arc::new(Catalog::from_scenario(&self.globals.common.scenario)?);
        let owner = Owner::new(
            super::super::save_menu::Environment {
                common: self.globals.common.clone(),
                server: self.globals.server.clone(),
                sounds: self.globals.sounds.clone(),
                storage: self.globals.storage.clone(),
            },
            catalog,
            selected,
            self.globals.data.persist_data.get(0),
            &self.globals.data.save_vectors.extra_words_1,
            &[],
            self.menu_extent,
            self.chart_seed.clone(),
        )?;
        self.chart = Some((return_id, owner));
        Ok(())
    }
    pub(super) fn poll_chart(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: crate::adv::higurashi::input::Buttons,
        input: Option<Analog>,
        pointer: &crate::adv::input_area::PointerFrame<'_>,
    ) -> Result<()> {
        let origin_available = self.origin_available();
        let Some((return_id, owner)) = &mut self.chart else {
            return Ok(());
        };
        owner.poll(
            delta,
            permission,
            super::super::chart::model::Input {
                buttons: input.map_or(buttons, |input| input.buttons(buttons)),
                axes: input.map_or([0; 4], |input| input.axes),
                wheel_scroll: input.map_or(0., |input| input.wheel_scroll),
            },
            pointer,
        )?;
        while let Some(request) = owner.take_request() {
            match request {
                Request::ReturnSource(1) => {
                    self.top = Some(top_menu::TopMenu::new(
                        origin_available,
                        self.globals.data.auto_save_slot.is_some(),
                        *return_id,
                        self.menu_extent,
                    ));
                    self.motion.clocks[4].control(
                        0.,
                        Ticks::from_f32(f32::from_bits(0x3dcccccd)),
                        0x40,
                    )?;
                }
                Request::ReturnSource(_) => {}
                Request::StartAdv { data, flags } => {
                    ensure!(self.chart_adv.is_none(), "Chart queued two ADV factories");
                    self.chart_adv = Some((data, flags));
                }
            }
        }
        Ok(())
    }
    pub(in crate::app::higurashi) fn set_analog_input(&mut self, input: Analog) {
        self.chart_input = Some(input);
    }
    pub(in crate::app::higurashi) fn take_chart_adv(
        &mut self,
    ) -> Option<(super::super::runtime_game_data::RuntimeGameData, u32)> {
        self.chart_adv.take()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub(in crate::app::higurashi) fn reference_chart(
        &self,
    ) -> Option<(bool, bool, bool, u32, i32, i32)> {
        self.chart
            .as_ref()
            .map(|(_, owner)| owner.reference_state())
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub(in crate::app::higurashi) fn reference_chart_owner(&self) -> Option<&Owner> {
        self.chart.as_ref().map(|(_, owner)| owner)
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_chart_seed(
        &mut self,
        seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    ) {
        self.chart_seed = seed;
    }
}
