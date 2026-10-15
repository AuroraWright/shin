//! Categorye272c queues stateless962b0; Movie Back queues fresh Title17/-1.
use super::*;
use crate::app::higurashi::movie_gallery::opening::Opening as MovieOpening;

pub(super) struct Transfer {
    pub(super) globals: Option<Globals>,
    snapshot: Option<RenderTexture>,
    extent: (u16, u16),
}
#[cfg(test)]
mod tests;
impl TitleSlot {
    pub fn dispatch_movie_gallery(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
        extent: (u16, u16),
    ) -> Result<bool> {
        if !matches!(
            self.pending,
            Some(Pending::Movie | Pending::PreparedMovie(_))
        ) {
            return Ok(false);
        }
        if matches!(self.pending, Some(Pending::Movie)) {
            let source = self
                .title
                .as_ref()
                .context("Movie factory lost Title source")?;
            MovieOpening::preflight(&source.globals, extent)?;
            let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
            let globals = self.title.take().unwrap().into_globals();
            self.pending = Some(Pending::PreparedMovie(Transfer {
                globals: Some(globals),
                snapshot: Some(snapshot),
                extent,
            }));
            #[cfg(test)]
            self.reference_scene_events.push(1);
        }
        #[cfg(test)]
        self.reference_check_factory_fault()?;
        let Some(Pending::PreparedMovie(transfer)) = &mut self.pending else {
            unreachable!()
        };
        self.movie_gallery = Some(MovieOpening::new_retained(
            &mut transfer.globals,
            &mut transfer.snapshot,
            transfer.extent,
        )?);
        self.pending = None;
        #[cfg(test)]
        self.reference_scene_events.push(2);
        Ok(true)
    }
    pub fn update_movie_gallery(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "Dispatch pending Movie return before updating source"
        );
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        let movie = self
            .movie_gallery
            .as_mut()
            .context("No active Movie gallery")?;
        movie.update(delta, permission, buttons, pointer)?;
        if let Some(request) = movie.take_title_request() {
            self.pending = Some(Pending::MovieTitle(request));
        }
        movie.globals.clock.advance(elapsed.as_f32());
        movie.globals.data.play_seconds = movie.globals.clock.saved_seconds();
        Ok(())
    }
    pub fn dispatch_movie_title(
        &mut self,
        context: &mut PreRenderContext,
        composed: &RenderTexture,
    ) -> Result<bool> {
        let Some(Pending::MovieTitle(request)) = &self.pending else {
            return Ok(false);
        };
        let request = *request;
        ensure!(
            self.movie_gallery.is_some(),
            "Movie title return lost source"
        );
        let snapshot = composed.copy_snapshot(&mut context.render_clone_ctx());
        let globals = self.movie_gallery.take().unwrap().into_globals();
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
    pub fn sync_movie_suspension(&self) {
        if let Some(movie) = &self.movie_gallery {
            movie.sync_suspension();
        }
    }
}
