//! Retained TIPS atlas/mask worker,First overlay,input/audio and scene request.
use super::{
    Model, SceneRequest,
    motion::{Event, Motion, Services},
    view::View,
};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            input::Buttons, music_commands::MusicCommands, runner::reconstruction_mode::Overlay,
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, scene_host::Globals},
    asset::{mask::MaskTexture, system::AssetServer},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use shin_core::{format::save::higurashi::SaveDate, time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::{Arc, mpsc};

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Arc<MaskTexture>>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
pub(in crate::app::higurashi) struct Opening {
    pub globals: Globals,
    pub atlas: Option<Arc<CharacterTextures>>,
    pub motion: Motion,
    job: Option<Job>,
    received: mpsc::Receiver<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
    overlay: Option<Overlay>,
    view: View,
    area: InputArea,
    extent: (u16, u16),
    ready: bool,
    transferred: bool,
    fault: Option<String>,
    date: Box<dyn FnMut() -> Result<SaveDate> + Send>,
    seed: Box<dyn FnMut() -> Result<u32> + Send>,
}
struct FrameServices<'a, 'p> {
    globals: &'a mut Globals,
    area: &'a mut InputArea,
    pointer: &'a PointerFrame<'p>,
    permission: bool,
    buttons: Buttons,
    job: &'a mut Option<Job>,
    mask: &'a mut Option<Arc<MaskTexture>>,
    overlay: &'a mut Option<Overlay>,
    date: &'a mut dyn FnMut() -> Result<SaveDate>,
    seed: &'a mut dyn FnMut() -> Result<u32>,
}
impl Services for FrameServices<'_, '_> {
    fn input(&mut self, model: &mut Model, permission: bool) {
        let permission = permission && self.permission;
        let captured = if permission {
            self.area.update(model, self.pointer)
        } else {
            self.area.disable(model);
            false
        };
        model.poll_controls(permission, captured, self.buttons);
    }
    fn fresh_date(&mut self) -> Result<SaveDate> {
        (self.date)()
    }
    fn seed(&mut self) -> Result<u32> {
        (self.seed)()
    }
    fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Cue(name) => {
                self.globals
                    .sounds
                    .lock()
                    .play(&self.globals.server, name, Volume(1.0))
            }
            Event::PlayBgm(id) => self.globals.audio.play_bgm(id, 60, 0, 1.0)?,
            Event::StopBgm(ticks) => self.globals.audio.stop_bgm(ticks),
        }
        Ok(())
    }
    fn retire_first(&mut self) {
        self.job.take();
        self.mask.take();
        self.overlay.take();
    }
}
impl Opening {
    #[cfg(test)]
    pub fn new(
        globals: Globals,
        data: Option<RuntimeGameData>,
        selected: i32,
        alternate: u32,
        mut capture: Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Result<Self> {
        Self::new_retained(
            &mut Some(globals),
            data,
            selected,
            alternate,
            &mut capture,
            extent,
        )
    }
    /// A retired factory keeps its globals and capture if model validation
    /// fails. Actual scene construction still follows deletion of the source.
    pub fn new_retained(
        globals: &mut Option<Globals>,
        data: Option<RuntimeGameData>,
        selected: i32,
        alternate: u32,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Result<Self> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid TIPS input extent");
        let retained = globals
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TIPS factory lost its globals"))?;
        let model = Model::new(
            &retained.common.scenario,
            data,
            selected,
            alternate,
            &retained.data.save_vectors,
        )?;
        let globals = globals.take().unwrap();
        let capture = capture.take();
        let motion = Motion::new(model, capture.is_some(), &globals.common.scenario);
        let view = View::new(globals.common.font.clone());
        let (sender, received) = mpsc::channel();
        let server: Arc<AssetServer> = globals.server.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas = server.load::<CharacterTextures, _>("/tipsmode.txa").await?;
            let _ = sender.send(atlas);
            server.load::<MaskTexture, _>("/default.msk").await
        });
        let retirement = task.retirement_handle().unwrap();
        let overlay = capture.map(|snapshot| Overlay {
            clock: motion.first().unwrap().clock.clone().unwrap(),
            snapshot,
            mask: None,
        });
        Ok(Self {
            globals,
            atlas: None,
            motion,
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            received,
            mask: None,
            overlay,
            view,
            area: InputArea::new(extent.0, extent.1),
            extent,
            ready: false,
            transferred: false,
            fault: None,
            date: Box::new(|| Ok(SaveDate::now())),
            seed: Box::new(|| Ok(crate::engine_random::next_seed())),
        })
    }
    #[cfg(test)]
    pub fn set_sources(
        &mut self,
        date: impl FnMut() -> Result<SaveDate> + Send + 'static,
        seed: impl FnMut() -> Result<u32> + Send + 'static,
    ) {
        self.date = Box::new(date);
        self.seed = Box::new(seed);
    }
    pub fn resize_input(&mut self, extent: (u16, u16)) {
        if extent == self.extent {
            return;
        }
        self.area.disable(&mut self.motion.model);
        self.area = InputArea::new(extent.0, extent.1);
        self.extent = extent;
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.area.disable(&mut self.motion.model);
        Ok(())
    }
    #[cfg(test)]
    pub fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        self.update_with_date(delta, permission, buttons, pointer, None)
    }
    pub fn update_with_date(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        date: Option<&mut dyn FnMut() -> Result<SaveDate>>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid TIPS scene delta"
        );
        ensure!(
            !self.transferred && self.motion.request.is_none(),
            "Dispatch the retained TIPS request before updating its source"
        );
        if let Some(fault) = &self.fault {
            bail!("TIPS preparation failed:{fault}");
        }
        let result = (|| {
            while let Ok(atlas) = self.received.try_recv() {
                self.atlas = Some(atlas);
            }
            if let Some(job) = &mut self.job {
                if let Some(task) = &mut job.task {
                    if let Some(mask) = task.poll_naive() {
                        self.mask = Some(mask?);
                        job.task = None;
                        // The producer publishes atlas before its mask result.
                        // Catch publication between the first drain and poll.
                        while let Ok(atlas) = self.received.try_recv() {
                            self.atlas = Some(atlas);
                        }
                        ensure!(
                            self.atlas.is_some(),
                            "TIPS worker completed without its atlas"
                        );
                        self.ready = true;
                    }
                }
            }
            self.motion.advance(
                if self.ready { 2 } else { 1 },
                self.mask.is_some(),
                self.atlas.is_some(),
                delta,
                &mut FrameServices {
                    globals: &mut self.globals,
                    area: &mut self.area,
                    pointer,
                    permission,
                    buttons,
                    job: &mut self.job,
                    mask: &mut self.mask,
                    overlay: &mut self.overlay,
                    date: date.unwrap_or(&mut *self.date),
                    seed: &mut *self.seed,
                },
            )?;
            if let Some(first) = self.motion.first() {
                if let Some(overlay) = &mut self.overlay {
                    overlay.clock = first.clock.clone().unwrap();
                    overlay.mask = if overlay.clock.mask_present {
                        self.mask.clone()
                    } else {
                        None
                    };
                }
            }
            while let Some(event) = self.globals.audio.pop_music_event() {
                if let crate::adv::higurashi::audio::MusicEvent::Started(info) = event {
                    for id in [info.id, i32::from(info.linked_id)]
                        .into_iter()
                        .filter(|id| *id >= 0)
                    {
                        let words = &mut self.globals.data.save_vectors.unlocked_bgms;
                        let at = id as usize / 32;
                        if at >= words.len() {
                            words.resize((at + 1).next_multiple_of(256), 0);
                        }
                        words[at] |= 1 << (id as usize % 32);
                    }
                }
            }
            self.globals.audio.update(delta, false);
            self.globals.sounds.lock().update();
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn take_scene_request(&mut self) -> Option<SceneRequest> {
        let request = self.motion.request.take()?;
        self.transferred = true;
        Some(request)
    }
    pub fn into_globals(self) -> Globals {
        self.globals
    }
    pub fn prepare(&mut self, _context: &mut PreRenderContext) -> Result<()> {
        self.view
            .synchronize(&self.globals.server, &self.motion.model)
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.view.render(
            &self.motion.model,
            self.atlas.as_deref(),
            &self.globals.common.system,
            self.motion.scrollbar,
            pass,
        );
        if self.motion.borrowed.contains(&super::motion::Kind::First) {
            if let Some(overlay) = &self.overlay {
                overlay.render(pass);
            }
        }
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_capture(&self) -> Option<&RenderTexture> {
        self.overlay.as_ref().map(|o| &o.snapshot)
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_first_pending(&self) -> bool {
        self.motion.first().is_some()
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_input_ready(&self) -> bool {
        self.ready
            && self.atlas.is_some()
            && self.motion.first().is_none()
            && self.motion.borrowed.contains(&super::motion::Kind::Input)
            && !self.motion.model.closing
            && self.motion.request.is_none()
    }
    #[cfg(test)]
    fn retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.job.iter().map(|job| job.retirement.clone()).collect()
    }
}

#[cfg(test)]
mod tests;
