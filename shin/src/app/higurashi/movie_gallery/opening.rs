//! Stateless962b0 Movie factory retains the complete application Globals.
use super::{
    Action, Model,
    owner::{Owner, Services},
};
use crate::{
    adv::{
        higurashi::{input::Buttons, music_commands::MusicCommands},
        input_area::PointerFrame,
    },
    app::higurashi::{save_menu::Environment, scene_host::Globals, title_demo::TitleRequest},
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use shin_core::{primitives::update::FrameId, time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};

struct Frame<'a> {
    globals: &'a mut Globals,
    request: &'a mut Option<TitleRequest>,
    achievements: &'a mut Vec<u32>,
    platform_byte: &'a mut bool,
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    cues: &'a mut [u64; 4],
}
impl Services for Frame<'_> {
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(name) => {
                self.globals
                    .sounds
                    .lock()
                    .play(&self.globals.server, name, Volume(1.));
                #[cfg(any(test, feature = "higurashi-window-probe"))]
                {
                    let index = match name {
                        "move" => 0,
                        "decide" => 1,
                        "cancel" => 2,
                        "error" => 3,
                        _ => bail!("Unexpected Movie cue {name}"),
                    };
                    self.cues[index] += 1;
                }
            }
            Action::StopBgm(fade) => self.globals.audio.stop_bgm(fade as i32),
            Action::PlatformByte => {
                //ccb10 stores its release byte independently of FirstBoot's
                //platform_flag and SaveData. No unspecified SDK action follows.
                *self.platform_byte = true;
            }
            Action::Achievement(id) => self.achievements.push(id),
            Action::ReturnTitle { selected, mode } => {
                *self.request = Some(TitleRequest {
                    selected,
                    mode: mode as u32,
                });
            }
            other => bail!("Movie private service escaped its concrete owner: {other:?}"),
        }
        Ok(())
    }
}
pub(in crate::app::higurashi) struct Opening {
    owner: Option<Owner>,
    pub globals: Globals,
    request: Option<TitleRequest>,
    achievements: Vec<u32>,
    platform_byte: bool,
    // One ID per permitted scene callback. This is decoder-presentation
    // deduplication, independent of the native tween delta and saved PlayClock.
    frame: FrameId,
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    cues: [u64; 4],
}
impl Opening {
    pub fn preflight(globals: &Globals, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Movie input extent");
        let model = Model::new(
            &globals.common.scenario.movie_info,
            &globals.data.save_vectors.unlocked_movies,
            globals.server.contains("/append.txt"),
        )?;
        ensure!(!model.rows.is_empty(), "Movie has no selectable rows");
        Ok(())
    }
    pub fn new_retained(
        globals: &mut Option<Globals>,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Result<Self> {
        let source = globals.as_mut().context("Movie factory lost Globals")?;
        Self::preflight(source, extent)?;
        let env = Environment {
            common: source.common.clone(),
            server: source.server.clone(),
            sounds: source.sounds.clone(),
            storage: source.storage.clone(),
        };
        let settings = source.data.clone();
        let scenario = source.common.scenario.clone();
        let audio = source.manager.clone();
        let mut request = None;
        let mut achievements = vec![];
        let mut platform_byte = false;
        #[cfg(any(test, feature = "higurashi-window-probe"))]
        let mut cues = [0; 4];
        let owner = Owner::new_retained(
            env,
            audio,
            scenario,
            &settings,
            capture,
            extent,
            &mut Frame {
                globals: source,
                request: &mut request,
                achievements: &mut achievements,
                platform_byte: &mut platform_byte,
                #[cfg(any(test, feature = "higurashi-window-probe"))]
                cues: &mut cues,
            },
        )?;
        Ok(Self {
            owner: Some(owner),
            globals: globals.take().unwrap(),
            request,
            achievements,
            platform_byte,
            frame: FrameId::default(),
            #[cfg(any(test, feature = "higurashi-window-probe"))]
            cues,
        })
    }
    pub fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid Movie delta"
        );
        self.frame.advance();
        self.owner.as_mut().unwrap().poll(
            delta,
            permission,
            buttons,
            pointer,
            self.frame,
            &mut Frame {
                globals: &mut self.globals,
                request: &mut self.request,
                achievements: &mut self.achievements,
                platform_byte: &mut self.platform_byte,
                #[cfg(any(test, feature = "higurashi-window-probe"))]
                cues: &mut self.cues,
            },
        )?;
        // Gallery playback does not invoke the story Movie acquisition hook.
        // Continue global BGM fades and system sounds without changing unlocks.
        self.globals.audio.update_global_music(delta);
        self.globals.sounds.lock().update();
        Ok(())
    }
    pub fn take_title_request(&mut self) -> Option<TitleRequest> {
        self.request.take()
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        self.owner.as_ref().unwrap().prepare(context);
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.owner
            .as_ref()
            .unwrap()
            .render(&self.globals.data, pass)
    }
    pub fn sync_suspension(&self) {
        self.owner.as_ref().unwrap().sync_suspension();
    }
    pub fn resize_input(&mut self, extent: (u16, u16)) -> Result<()> {
        self.owner.as_mut().unwrap().resize(extent)
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.owner.as_mut().unwrap().cancel_input();
        Ok(())
    }
    pub fn into_globals(mut self) -> Globals {
        self.owner.take();
        self.globals
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_owner(&self) -> &Owner {
        self.owner.as_ref().unwrap()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(
        dead_code,
        reason = "Inspection hook shared by native tests and optional window probes."
    )]
    pub fn reference_cues(&self) -> [u64; 4] {
        self.cues
    }
}
