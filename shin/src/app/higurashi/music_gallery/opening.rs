//! Stateless Music factory retains the complete application Globals.
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
use anyhow::{Context, Result, ensure};
use shin_core::{
    format::scenario::higurashi::HigurashiVariant, time::Ticks, vm::command::types::Volume,
};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};

struct Frame<'a> {
    globals: &'a mut Globals,
    request: &'a mut Option<TitleRequest>,
    achievements: &'a mut Vec<u32>,
    platform_byte: &'a mut bool,
    #[cfg(feature = "higurashi-window-probe")]
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
                #[cfg(feature = "higurashi-window-probe")]
                {
                    let index = match name {
                        "move" => 0,
                        "decide" => 1,
                        "cancel" => 2,
                        "error" => 3,
                        _ => anyhow::bail!("Unexpected Music cue {name}"),
                    };
                    self.cues[index] += 1;
                }
            }
            Action::PlayBgm {
                id,
                once,
                fade,
                gain,
            } => self
                .globals
                .audio
                .play_bgm(i32::from(id), fade as i32, u32::from(once), gain)?,
            Action::StopBgm(fade) => self.globals.audio.stop_bgm(fade as i32),
            Action::PlatformByte => {
                // Inspected ccb10 only stores a release byte1. Retain this
                // platform request separately from FirstBoot's platform_flag
                // and from save data; no undocumented SDK behavior is invented.
                *self.platform_byte = true;
            }
            Action::Achievement(id) => self.achievements.push(id),
            Action::ReturnTitle { selected, mode } => {
                *self.request = Some(TitleRequest {
                    selected,
                    mode: mode as u32,
                })
            }
            Action::DisableInput => anyhow::bail!("Music private input event escaped owner"),
        }
        Ok(())
    }
    fn bgm_status(&self) -> u32 {
        self.globals.audio.bgm_status() as u32
    }
}
pub(in crate::app::higurashi) struct Opening {
    owner: Option<Owner>,
    pub globals: Globals,
    request: Option<TitleRequest>,
    achievements: Vec<u32>,
    platform_byte: bool,
    #[cfg(feature = "higurashi-window-probe")]
    cues: [u64; 4],
}
impl Opening {
    pub fn preflight(globals: &Globals, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Music input extent");
        ensure!(
            globals.common.scenario.variant == HigurashiVariant::Updated,
            "Music requires updated Higurashi"
        );
        Model::new(
            &globals.common.scenario.music_box_info,
            &globals.data.save_vectors.unlocked_bgms,
            globals.server.contains("/append.txt"),
        )?;
        Ok(())
    }
    pub fn new_retained(
        globals: &mut Option<Globals>,
        capture: &mut Option<RenderTexture>,
        extent: (u16, u16),
    ) -> Result<Self> {
        let source = globals.as_mut().context("Music factory lost Globals")?;
        Self::preflight(source, extent)?;
        let env = Environment {
            common: source.common.clone(),
            server: source.server.clone(),
            sounds: source.sounds.clone(),
            storage: source.storage.clone(),
        };
        let settings = source.data.clone();
        let scenario = source.common.scenario.clone();
        let mut request = None;
        let mut achievements = vec![];
        let mut platform_byte = false;
        #[cfg(feature = "higurashi-window-probe")]
        let mut cues = [0; 4];
        let owner = Owner::new_retained(
            env,
            &scenario,
            &settings,
            capture,
            extent,
            &mut Frame {
                globals: source,
                request: &mut request,
                achievements: &mut achievements,
                platform_byte: &mut platform_byte,
                #[cfg(feature = "higurashi-window-probe")]
                cues: &mut cues,
            },
        )?;
        Ok(Self {
            owner: Some(owner),
            globals: globals.take().unwrap(),
            request,
            achievements,
            platform_byte,
            #[cfg(feature = "higurashi-window-probe")]
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
        self.owner.as_mut().unwrap().poll(
            delta,
            permission,
            buttons,
            pointer,
            &mut Frame {
                globals: &mut self.globals,
                request: &mut self.request,
                achievements: &mut self.achievements,
                platform_byte: &mut self.platform_byte,
                #[cfg(feature = "higurashi-window-probe")]
                cues: &mut self.cues,
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
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        self.owner.as_ref().unwrap().prepare(context);
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
    pub fn reference_owner(&self) -> &Owner {
        self.owner.as_ref().unwrap()
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub fn reference_window_state(
        &self,
    ) -> crate::app::higurashi::window_probe::music_gallery::State {
        use crate::app::higurashi::window_probe::music_gallery::State;
        let owner = self.reference_owner();
        let model = owner.reference_model();
        let selected = usize::try_from(model.selected).expect("Music selected index");
        let row = model.rows.get(selected).expect("Music selected row");
        let (atlas, mask, first_complete, input, _, _) = owner.reference_state();
        State {
            atlas,
            mask,
            first_complete,
            input,
            closing: model.closing,
            selected,
            playing: model.playing,
            rows: model.rows.len(),
            scroll_bits: model.scroll.value().to_bits(),
            scroll_busy: !model.scroll.is_idle(),
            elapsed_bits: model.elapsed.to_bits(),
            bgm: row.bgm,
            once: row.once,
            unlocked: row.unlocked,
            bgm_status: Some(self.globals.audio.bgm_status() as u32),
            cues: self.cues,
        }
    }
}
