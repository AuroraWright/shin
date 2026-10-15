//! Category e271c/31d80 queues the stateless BGM factory; Back queues Title16/-1.
use super::*;
use crate::app::higurashi::music_gallery::opening::Opening as MusicOpening;

pub(super) struct Transfer {
    pub(super) globals: Option<Globals>,
    snapshot: Option<RenderTexture>,
    extent: (u16, u16),
}
#[cfg(test)]
mod tests;
impl TitleSlot {
    pub fn dispatch_music_gallery(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        if !matches!(
            self.pending,
            Some(Pending::Music | Pending::PreparedMusic(_))
        ) {
            return Ok(false);
        }
        if matches!(self.pending, Some(Pending::Music)) {
            let source = self
                .title
                .as_ref()
                .context("Music factory lost Title source")?;
            MusicOpening::preflight(&source.globals, extent)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = self.title.take().unwrap().into_globals();
            self.pending = Some(Pending::PreparedMusic(Transfer {
                globals: Some(globals),
                snapshot: Some(snapshot),
                extent,
            }));
            #[cfg(test)]
            self.reference_scene_events.push(1);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedMusic(transfer)) = &mut self.pending else {
            unreachable!()
        };
        self.music_gallery = Some(MusicOpening::new_retained(
            &mut transfer.globals,
            &mut transfer.snapshot,
            transfer.extent,
        )?);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn update_music_gallery(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending Music return before updating source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let music = self
            .music_gallery
            .as_mut()
            .context("No active Music gallery")?;
        music.update(delta, permission, buttons, pointer)?;
        if let Some(request) = music.take_title_request() {
            self.pending = Some(Pending::MusicTitle(request));
        }
        music.globals.clock.advance(elapsed.as_f32());
        music.globals.data.play_seconds = music.globals.clock.saved_seconds();
        Ok(())
    }
    pub fn dispatch_music_title(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::MusicTitle(request)) = &self.pending else {
            return Ok(false);
        };
        let request = *request;
        ensure!(
            self.music_gallery.is_some(),
            "Music title return lost source"
        );
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let globals = self.music_gallery.take().unwrap().into_globals();
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
