//! Retained quiz resources, input, scene voice, captured First and score factory.
use super::{
    Action, Model,
    motion::{Event, Kind, Motion, Services},
    resources::{Loader, Plan},
    view::View,
    voice::Voice,
};
use crate::{
    adv::{
        higurashi::{input::Buttons, music_commands::MusicCommands},
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    app::higurashi::{runtime_game_data::RuntimeGameData, scene_host::Globals},
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use glam::Vec2;
use shin_core::{time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};

pub(in crate::app::higurashi) struct Request {
    pub data: RuntimeGameData,
    pub flags: u32,
}
pub(in crate::app::higurashi) struct Prepared {
    data: RuntimeGameData,
    args: [i32; 3],
    bustups: [i32; 11],
    extent: (u16, u16),
}
impl Prepared {
    pub(in crate::app::higurashi) fn into_model(
        self,
        seed: impl FnOnce() -> u32,
    ) -> (Model, Plan, (u16, u16)) {
        let Self {
            data,
            args,
            bustups,
            extent,
        } = self;
        let model = Model::new(data, args, seed()).expect("Preflighted quiz pool");
        let plan = Plan {
            bustups,
            questions: model
                .questions
                .map(|q| format!("/quiz/{}.txa", q.descriptor().texture)),
        };
        (model, plan, extent)
    }
}
pub(in crate::app::higurashi) struct Opening {
    pub globals: Globals,
    pub motion: Motion,
    pub resources: Loader,
    voice: Voice,
    view: View,
    area: InputArea,
    extent: (u16, u16),
    overlay: Option<crate::adv::higurashi::runner::reconstruction_mode::Overlay>,
    transferred: bool,
    fault: Option<String>,
}
struct Target<'a> {
    model: &'a mut Model,
    error: Option<anyhow::Error>,
}
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, position: Vec2) -> bool {
        self.model.probe(position.x, position.y)
    }
    fn reset(&mut self) {
        self.model.reset_contact();
    }
    fn press(&mut self, _position: Vec2) {}
    fn release(&mut self, _position: Vec2) {
        if let Err(error) = self.model.release() {
            self.error = Some(error);
        }
    }
    fn long_press(&mut self, _position: Vec2) {}
    fn drag(&mut self, _previous: Vec2, _current: Vec2) {}
    fn gesture(&mut self, _velocity: Vec2) {}
    fn cancel(&mut self) {}
}
struct FrameServices<'a, 'p> {
    globals: &'a mut Globals,
    resources: &'a mut Loader,
    voice: &'a mut Voice,
    area: &'a mut InputArea,
    pointer: &'a PointerFrame<'p>,
    buttons: Buttons,
    permission: bool,
    overlay: &'a mut Option<crate::adv::higurashi::runner::reconstruction_mode::Overlay>,
}
impl Services for FrameServices<'_, '_> {
    fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Model(Action::Cue(name)) => {
                self.globals
                    .sounds
                    .lock()
                    .play(&self.globals.server, name, Volume(1.0))
            }
            Event::Model(Action::Voice { name, reaction }) => self.voice.play(name, reaction),
            // InputArea was disabled before the denied controller callback.
            Event::Model(Action::DisableInput) | Event::ReturnScore(_) => {}
            Event::StopBgm(fade) => self.globals.audio.stop_bgm(fade as i32),
            Event::PlayBgm { id, fade } => {
                self.globals.audio.play_bgm(id, fade as i32, 1, 1.0)?;
            }
        }
        Ok(())
    }
    fn publish_resources(&mut self) -> Result<()> {
        self.resources.publish()
    }
    fn retire_first(&mut self) {
        self.resources.retire_first();
        self.overlay.take();
    }
    fn advance_voice(&mut self, delta: Ticks) {
        self.voice
            .set_volume(self.globals.data.settings.common.v2_voicevol);
        self.voice.advance(delta);
    }
    fn advance_characters(&mut self, delta: Ticks) {
        if let Some(prepared) = &mut self.resources.prepared {
            for bustup in &mut prepared.bustups {
                bustup.advance(
                    delta,
                    Some(&|id, _pan| self.voice.character_lipsync(id, 0.0)),
                );
            }
        }
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let mut permission = permission && self.permission;
        let mut target = Target { model, error: None };
        let captured = if permission {
            self.area.update(&mut target, self.pointer)
        } else {
            self.area.disable(&mut target);
            false
        };
        if let Some(error) = target.error {
            return Err(error);
        }
        target
            .model
            .input(&mut permission, captured, self.buttons)?;
        Ok(())
    }
    fn voice_status(&self) -> u32 {
        self.voice.status()
    }
    fn bgm_status(&self) -> u32 {
        self.globals.audio.bgm_status() as u32
    }
}
impl Opening {
    pub fn new(
        globals: Globals,
        data: RuntimeGameData,
        args: [i32; 3],
        seed: u32,
        capture: Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Result<Self> {
        let prepared = Self::prepare_scene(data, args, &globals.common.scenario, extent)?;
        Ok(Self::from_prepared(globals, prepared, capture, || seed))
    }
    /// Pure constructor preflight runs while the old scene and factory remain
    /// owned. Validate before reading the one quiz seed or starting any worker.
    pub fn prepare_scene(
        data: RuntimeGameData,
        args: [i32; 3],
        scenario: &shin_core::format::scenario::higurashi::HigurashiScenario,
        extent: (u16, u16),
    ) -> Result<Prepared> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid quiz input extent");
        ensure!((0..8).contains(&args[0]), "Invalid quiz question pool");
        let bustups = Plan::bustups(scenario, args[1])?;
        Ok(Prepared {
            data,
            args,
            bustups,
            extent,
        })
    }
    pub fn from_prepared(
        globals: Globals,
        prepared: Prepared,
        capture: Option<RenderTexture>,
        seed: impl FnOnce() -> u32,
    ) -> Self {
        let (model, plan, extent) = prepared.into_model(seed);
        let motion = Motion::new(model, capture.is_some());
        let resources = Loader::new(
            globals.server.clone(),
            globals.common.scenario.clone(),
            plan,
            crate::engine_random::next_seed,
        );
        let voice = Voice::new(
            globals.manager.clone(),
            globals.server.clone(),
            &globals.common.scenario,
        );
        let view = View::new(globals.common.font.clone());
        let overlay =
            capture.map(
                |snapshot| crate::adv::higurashi::runner::reconstruction_mode::Overlay {
                    clock: motion.first().unwrap().overlay.clone().unwrap(),
                    snapshot,
                    mask: None,
                },
            );
        Self {
            globals,
            motion,
            resources,
            voice,
            view,
            area: InputArea::new(extent.0, extent.1),
            extent,
            overlay,
            transferred: false,
            fault: None,
        }
    }
    pub fn resize_input(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid quiz input extent");
        if self.extent != extent {
            self.area.disable(&mut Target {
                model: &mut self.motion.model,
                error: None,
            });
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.area.disable(&mut Target {
            model: &mut self.motion.model,
            error: None,
        });
        Ok(())
    }
    pub fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid quiz delta"
        );
        ensure!(
            !self.transferred && self.motion.score_request.is_none(),
            "Dispatch retained quiz score before updating source"
        );
        if let Some(fault) = &self.fault {
            bail!("Quiz owner failed:{fault}");
        }
        let result = (|| {
            let status = self.resources.poll()?;
            self.motion.advance(
                status,
                delta,
                &mut FrameServices {
                    globals: &mut self.globals,
                    resources: &mut self.resources,
                    voice: &mut self.voice,
                    area: &mut self.area,
                    pointer,
                    buttons,
                    permission,
                    overlay: &mut self.overlay,
                },
            )?;
            if let (Some(first), Some(overlay)) = (self.motion.first(), &mut self.overlay) {
                overlay.clock = first.overlay.clone().unwrap();
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
            // The scene advances its own voice above; native27200 advances
            // global music afterwards without deriving focus from quiz voice.
            self.globals.audio.update_global_music(delta);
            self.globals.sounds.lock().update();
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn take_scene_request(&mut self) -> Option<Request> {
        let score = self.motion.score_request.take()?;
        self.transferred = true;
        Some(Request {
            data: self.motion.model.data.clone(),
            flags: score as u32,
        })
    }
    pub fn prepare(&mut self, _context: &mut PreRenderContext) -> Result<()> {
        self.view
            .synchronize(&self.globals.server, &self.motion.model)
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.view.render(
            &self.motion,
            self.resources.atlas.as_deref(),
            self.resources.prepared.as_ref(),
            pass,
        )?;
        if self.motion.borrowed.contains(&Kind::First) {
            if let Some(overlay) = &self.overlay {
                overlay.render(pass);
            }
        }
        Ok(())
    }
    pub fn into_globals(self) -> Globals {
        self.globals
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_capture(&self) -> Option<&RenderTexture> {
        self.overlay.as_ref().map(|o| &o.snapshot)
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_input_ready(&self) -> bool {
        self.motion.ready
            && self.resources.atlas.is_some()
            && self.resources.prepared.is_some()
            && self.motion.first().is_none()
            && self.motion.borrowed.contains(&Kind::Input)
            && !self.motion.model.closing
            && self.motion.score_request.is_none()
    }
    #[cfg(test)]
    pub(in crate::app::higurashi) fn reference_retirement(
        &self,
    ) -> Option<shin_tasks::LoaderRetirement> {
        self.resources.retirement()
    }
}

#[cfg(test)]
mod tests;
