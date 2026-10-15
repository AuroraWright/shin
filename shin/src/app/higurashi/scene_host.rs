//! Updated275b0/2714c deferred scene transfer through startup and TitleDemo.
//! Main-title construction receives owned globals after the previous scene retires.
use super::{
    first_boot::CommonAssets,
    load_game::Storage,
    startup::{Startup, TitleDemoRequest},
    title_demo::{TitleDemo, TitleRequest},
};
use crate::{
    adv::{
        higurashi::{audio::HigurashiAudio, input::Buttons},
        input_area::PointerFrame,
        play_clock::PlayClock,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{format::save::higurashi::SaveData, primitives::update::FrameId, time::Ticks};
use std::sync::Arc;

pub(super) struct Globals {
    pub common: Arc<CommonAssets>,
    pub data: SaveData,
    pub storage: Option<Arc<dyn Storage>>,
    pub runtime_saves: Option<super::runtime_saves::Slots>,
    pub clock: PlayClock,
    pub platform_flag: bool,
    pub last_title_mode: i32,
    pub audio: HigurashiAudio,
    pub server: Arc<AssetServer>,
    pub manager: Arc<shin_audio::AudioManager>,
    pub sounds: Arc<Mutex<SystemSoundPlayer>>,
}
/// Actual e2a40 factory arguments and global owners. Main Title ddcf8 is the
/// next scene adapter; this transfer never substitutes a synthetic menu.
pub(super) struct TitleTransition {
    pub request: TitleRequest,
    pub globals: Globals,
}
enum Scene {
    Startup(Box<Startup>),
    Demo(Box<TitleDemo>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Demo(TitleDemoRequest),
    Title(TitleRequest),
}
pub(super) struct SceneHost {
    active: Option<Scene>,
    pending: Option<Destination>,
    globals: Option<Globals>,
    server: Arc<AssetServer>,
    manager: Arc<shin_audio::AudioManager>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    transferred: bool,
    storage: Option<Arc<dyn Storage>>,
}
impl SceneHost {
    pub fn new(
        server: Arc<AssetServer>,
        manager: Arc<shin_audio::AudioManager>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        let startup = Startup::new(
            server.clone(),
            manager.clone(),
            sounds.clone(),
            storage.clone(),
        );
        Self {
            active: Some(Scene::Startup(Box::new(startup))),
            pending: None,
            globals: None,
            server,
            manager,
            sounds,
            transferred: false,
            storage: Some(storage),
        }
    }
    /// Main title's idle-demo factory transfers the same loaded global owners.
    /// The caller has already captured and retired its title before this call.
    pub fn from_demo(globals: Globals, request: TitleDemoRequest) -> Self {
        let server = globals.server.clone();
        let manager = globals.manager.clone();
        let sounds = globals.sounds.clone();
        let storage = globals.storage.clone();
        let demo = TitleDemo::new(
            server.clone(),
            manager.clone(),
            sounds.clone(),
            &globals.common.scenario,
            &globals.data,
            request.mode,
        );
        Self {
            active: Some(Scene::Demo(Box::new(demo))),
            pending: None,
            globals: Some(globals),
            server,
            manager,
            sounds,
            transferred: false,
            storage,
        }
    }
    pub fn globals(&self) -> Option<&Globals> {
        self.globals.as_ref()
    }
    pub fn pending_scene_factory(&self) -> bool {
        self.pending.is_some()
    }
    /// Normal-build observer of concrete readiness, never a scheduling input.
    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        if self.pending.is_some() || self.transferred {
            return None;
        }
        match self.active.as_ref()? {
            Scene::Startup(startup) => startup.touch_diagnostic_phase(),
            Scene::Demo(demo) => demo.touch_diagnostic_phase(),
        }
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_notice(&self) -> Option<(i32, glam::Vec2)> {
        let Some(Scene::Startup(startup)) = &self.active else {
            return None;
        };
        let phase = startup
            .reference_questions()
            .iter()
            .find(|(kind, _, _)| *kind == super::load_game::QuestionKind::AutoSleep)?
            .1;
        Some((phase, startup.reference_notice_center()?))
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_demo_ready(&self) -> bool {
        matches!(&self.active, Some(Scene::Demo(demo)) if demo.reference_frame_ready())
    }
    #[cfg(test)]
    pub(super) fn reference_demo_motion(&self) -> Option<(f32, bool)> {
        match &self.active {
            Some(Scene::Demo(demo)) => Some(demo.reference_motion()),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(super) fn reference_demo_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        match &self.active {
            Some(Scene::Demo(demo)) => demo.reference_retirements(),
            _ => Vec::new(),
        }
    }
    pub fn cancel_input(&mut self) {
        match &mut self.active {
            Some(Scene::Startup(startup)) => startup.cancel_input(),
            Some(Scene::Demo(demo)) => demo.cancel_input(),
            None => {}
        }
    }
    // Direct native-owner comparisons admit input. Runtime must provide its
    // explicit desktop gate through update_with_permission.
    #[cfg(test)]
    pub fn update(
        &mut self,
        id: FrameId,
        delta: Ticks,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        queue: &wgpu::Queue,
    ) -> Result<Option<TitleTransition>> {
        self.update_with_permission(id, delta, true, buttons, pointer, queue)
    }
    /// Dispatch the previous frame's owned request before updating any scene.
    /// On title transfer,the caller constructs Title before advancing that frame.
    pub fn update_with_permission(
        &mut self,
        id: FrameId,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        queue: &wgpu::Queue,
    ) -> Result<Option<TitleTransition>> {
        ensure!(
            !self.transferred,
            "Higurashi startup host already transferred its global owners"
        );
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid Higurashi frame delta"
        );
        let elapsed = delta.as_f32();
        let delta = Ticks::from_f32(elapsed.min(3.0));
        if let Some(destination) = self.pending.take() {
            match destination {
                Destination::Demo(request) => {
                    ensure!(
                        self.globals.is_some(),
                        "Opening scene requires loaded globals"
                    );
                    // Native clears its active pointer before deleting old owner.
                    drop(self.active.take());
                    let globals = self.globals.as_ref().unwrap();
                    self.active = Some(Scene::Demo(Box::new(TitleDemo::new(
                        self.server.clone(),
                        self.manager.clone(),
                        self.sounds.clone(),
                        &globals.common.scenario,
                        &globals.data,
                        request.mode,
                    ))));
                }
                Destination::Title(request) => {
                    drop(self.active.take());
                    self.transferred = true;
                    return Ok(Some(TitleTransition {
                        request,
                        globals: self.globals.take().expect("Opening scene retains globals"),
                    }));
                }
            }
        }
        match self.active.as_mut() {
            Some(Scene::Startup(startup)) => {
                if let Some(ready) =
                    startup.poll_with_permission(delta, permission, buttons, pointer)?
                {
                    ensure!(
                        self.globals.is_none(),
                        "Startup globals were published twice"
                    );
                    let clock = PlayClock::from_saved_seconds(ready.data.play_seconds);
                    ensure!(
                        clock.ticks() == ready.play_ticks,
                        "Startup clock seed differs from loaded seconds"
                    );
                    let mut audio = HigurashiAudio::new(
                        self.manager.clone(),
                        self.server.clone(),
                        ready.common.scenario.clone(),
                    );
                    let settings = &ready.data.settings.common;
                    audio.set_preferences(
                        settings.v2_voicevol,
                        settings.v19,
                        settings.v5_voicepanapot,
                        settings.v0_bgmvol,
                    );
                    audio.set_effects_volume(settings.v1_sfxvol);
                    self.globals = Some(Globals {
                        storage: self.storage.clone(),
                        runtime_saves: Some(super::runtime_saves::Slots::loaded(&ready.data)),
                        common: ready.common,
                        data: ready.data,
                        clock,
                        platform_flag: ready.platform_flag,
                        last_title_mode: 0,
                        audio,
                        server: self.server.clone(),
                        manager: self.manager.clone(),
                        sounds: self.sounds.clone(),
                    });
                }
                if let Some(request) = startup.take_title_demo_request() {
                    self.pending = Some(Destination::Demo(request));
                }
            }
            Some(Scene::Demo(demo)) => {
                let globals = self
                    .globals
                    .as_mut()
                    .expect("Opening scene requires globals");
                demo.update(
                    id,
                    delta,
                    permission,
                    buttons,
                    pointer,
                    queue,
                    &mut globals.data,
                )?;
                if let Some(request) = demo.take_title_request() {
                    self.pending = Some(Destination::Title(request));
                }
            }
            None => bail!("Higurashi scene host has no active owner"),
        }
        // Updated271d0 clamps scene delta only.2721c/cc9f0/27224 then add
        // the original elapsed ticks, including the frame that restores a save.
        if let Some(globals) = &mut self.globals {
            globals.audio.update(delta, false);
            globals.clock.advance(elapsed);
            globals.data.play_seconds = globals.clock.saved_seconds();
        }
        self.sounds.lock().update();
        Ok(None)
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        if let Some(Scene::Startup(startup)) = &mut self.active {
            startup.prepare(context);
        }
    }
    pub fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) -> Result<()> {
        match &mut self.active {
            Some(Scene::Startup(startup)) => startup.render(pass),
            Some(Scene::Demo(demo)) => {
                demo.render(pass, &self.globals.as_ref().expect("Opening globals").data)
            }
            None => bail!("Higurashi scene host was retired before rendering"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
