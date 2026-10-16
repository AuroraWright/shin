//! Source-free CGMode retains the application's complete Globals. Viewer
//! captures redraw the requested old composition during the native main poll.
use super::{
    Action, Model,
    owner::{Composition, Input, Owner, Seeds, Services},
};
use crate::{
    adv::{
        higurashi::{input::Buttons, music_commands::MusicCommands},
        input_area::PointerFrame,
    },
    app::higurashi::{input_bridge::Analog, scene_host::Globals, title_demo::TitleRequest},
    render::PreRenderContext,
};
use anyhow::{Context, Result, ensure};
use shin_core::{primitives::color::UnormColor, time::Ticks, vm::command::types::Volume};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};

struct Frame<'a, 'device, 'pipeline, 'buffer, 'encoder> {
    globals: &'a mut Globals,
    context: Option<&'a mut PreRenderContext<'device, 'pipeline, 'buffer, 'encoder>>,
    request: &'a mut Option<TitleRequest>,
    achievements: &'a mut Vec<u32>,
}
impl Services for Frame<'_, '_, '_, '_, '_> {
    fn capture_parent(&mut self, composition: &Composition<'_, '_>) -> Result<RenderTexture> {
        ensure!(
            composition.badges_hidden(),
            "CG capture requires hidden Viewer badges"
        );
        let context = self
            .context
            .as_mut()
            .context("CG capture outside main-poll render context")?;
        let mut snapshot = context.new_render_texture("Higurashi CG Viewer parent capture".into());
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Higurashi CG old parent/Viewer composition",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            composition.render(&mut pass)?;
        }
        Ok(snapshot)
    }
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(name) => {
                self.globals
                    .sounds
                    .lock()
                    .play(&self.globals.server, name, Volume(1.))
            }
            Action::PlayBgm { id, loops, fade } => {
                self.globals.audio.play_bgm(id, fade as i32, loops, 1.)?
            }
            Action::StopBgm(fade) => self.globals.audio.stop_bgm(fade as i32),
            Action::ReturnTitle { selected, mode } => {
                *self.request = Some(TitleRequest {
                    selected,
                    mode: mode as u32,
                })
            }
            // The desktop has no console achievement SDK. Retain the literal
            // request separately; it never changes save payloads or disk bytes.
            Action::Achievement(id) => self.achievements.push(id),
            Action::DisableInput | Action::OpenViewer(_) => {
                anyhow::bail!("CG private event escaped its owner")
            }
        }
        Ok(())
    }
}
pub(in crate::app::higurashi) struct Opening {
    owner: Option<Owner>,
    pub globals: Globals,
    input: Option<Analog>,
    request: Option<TitleRequest>,
    achievements: Vec<u32>,
}
impl Opening {
    pub fn preflight(globals: &Globals, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid CG input extent");
        Model::new(
            &globals.common.scenario.picture_box_info,
            &globals.data,
            true,
        )?;
        Ok(())
    }
    pub fn new_retained(
        globals: &mut Option<Globals>,
        capture: &mut Option<RenderTexture>,
        seeds: Seeds,
        extent: (u16, u16),
    ) -> Result<Self> {
        let source = globals.as_mut().context("CG factory lost globals")?;
        Self::preflight(source, extent)?;
        let mut request = None;
        let mut achievements = Vec::new();
        let owner = Owner::new_retained(
            source.server.clone(),
            source.common.scenario.clone(),
            source.common.font.clone(),
            &source.data.clone(),
            capture,
            seeds,
            extent,
            &mut Frame {
                globals: source,
                context: None,
                request: &mut request,
                achievements: &mut achievements,
            },
        )?;
        Ok(Self {
            owner: Some(owner),
            globals: globals.take().unwrap(),
            input: None,
            request,
            achievements,
        })
    }
    pub fn set_analog_input(&mut self, input: Analog) {
        self.input = Some(input);
    }
    pub fn update(
        &mut self,
        context: &mut PreRenderContext,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        let analog = self.input.take();
        self.owner.as_mut().unwrap().poll(
            delta,
            permission,
            Input {
                buttons,
                viewer_buttons: analog.map_or(buttons, |a| a.buttons(buttons)),
                first_stick: analog.map_or([0; 2], |a| [a.axes[0], a.axes[1]]),
            },
            pointer,
            &mut Frame {
                globals: &mut self.globals,
                context: Some(context),
                request: &mut self.request,
                achievements: &mut self.achievements,
            },
        )?;
        while let Some(event) = self.globals.audio.pop_music_event() {
            if let crate::adv::higurashi::audio::MusicEvent::Started(info) = event {
                for id in [info.id, i32::from(info.linked_id)]
                    .into_iter()
                    .filter(|id| *id >= 0)
                {
                    let bank = &mut self.globals.data.save_vectors.unlocked_bgms;
                    let word = id as usize / 32;
                    if word >= bank.len() {
                        bank.resize((word + 1).next_multiple_of(256), 0);
                    }
                    bank[word] |= 1 << (id as usize % 32);
                }
            }
        }
        self.globals.audio.update_global_music(delta);
        self.globals.sounds.lock().update();
        Ok(())
    }
    pub fn take_title_request(&mut self) -> Option<TitleRequest> {
        self.request.take()
    }
    pub fn prepare(&mut self, _: &mut PreRenderContext) -> Result<()> {
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.owner.as_ref().unwrap().render(pass)
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
}
