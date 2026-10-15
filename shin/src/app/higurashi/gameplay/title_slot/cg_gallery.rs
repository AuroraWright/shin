//! Actual Category e2704/40b00/42380 factory, then e2a40(15,-1) grid return.
use super::*;
use crate::app::higurashi::cg_gallery::{opening::Opening as CgOpening, owner::Seeds as CgSeeds};

pub(super) struct Transfer {
    pub(super) globals: Option<Globals>,
    snapshot: Option<RenderTexture>,
    extent: (u16, u16),
    seeds: CgSeeds,
}

#[cfg(test)]
mod tests;
impl TitleSlot {
    pub fn dispatch_cg_gallery(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        if !matches!(self.pending, Some(Pending::Cg | Pending::PreparedCg(_))) {
            return Ok(false);
        }
        if matches!(self.pending, Some(Pending::Cg)) {
            let source = self
                .title
                .as_ref()
                .context("CG factory lost its title source")?;
            CgOpening::preflight(&source.globals, extent)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = self.title.take().unwrap().into_globals();
            self.pending = Some(Pending::PreparedCg(Transfer {
                globals: Some(globals),
                snapshot: Some(snapshot),
                extent,
                seeds: CgSeeds::new(|| {
                    std::array::from_fn(|_| {
                        getrandom::u32().expect("Higurashi CG picture OS entropy unavailable")
                    })
                }),
            }));
            #[cfg(test)]
            self.reference_scene_events.push(1);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedCg(transfer)) = &mut self.pending else {
            unreachable!()
        };
        self.cg_gallery = Some(CgOpening::new_retained(
            &mut transfer.globals,
            &mut transfer.snapshot,
            transfer.seeds.clone(),
            transfer.extent,
        )?);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn update_cg_gallery(
        &mut self,
        context: &mut PreRenderContext,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending CG return before updating source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let cg = self.cg_gallery.as_mut().context("No active CG gallery")?;
        cg.update(context, delta, permission, buttons, pointer)?;
        if let Some(request) = cg.take_title_request() {
            self.pending = Some(Pending::CgTitle(request));
        }
        cg.globals.clock.advance(elapsed.as_f32());
        cg.globals.data.play_seconds = cg.globals.clock.saved_seconds();
        Ok(())
    }
    pub fn dispatch_cg_title(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::CgTitle(request)) = &self.pending else {
            return Ok(false);
        };
        let request = *request;
        ensure!(self.cg_gallery.is_some(), "CG title return lost its source");
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let globals = self.cg_gallery.take().unwrap().into_globals();
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
