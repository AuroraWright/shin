//! Outer Ger BGMMode owner. Retains original atlas/capture plus the shared menu
//! music player; the app advances that player once even while this owner loads.
use super::{AppAction, MenuBgm, Viewport};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::{
            music_gallery::{self, Action, CatalogItem, Controller, Input, Phase},
            reconstruction_mode::{OverlayClock, overlay::Overlay},
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::{mask::MaskTexture, system::AssetServer},
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Result, anyhow};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    format::{
        save::gerokasu2::SaveData,
        scenario::{gerokasu2::Gerokasu2Scenario, info::BgmId, instruction_elements::FromNumber},
    },
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Volume},
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;
mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MusicGalleryAction {
    ReturnTitle { requested_id: i32 },
}
pub(super) struct MusicGalleryHost {
    model: Controller,
    atlas: Arc<CharacterTextures>,
    system: Arc<CharacterTextures>,
    mask: Arc<MaskTexture>,
    previous: Option<Overlay>,
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    bgm: MenuBgm,
    sounds: SystemSoundPlayer,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
    unlocks: Vec<u32>,
}
impl MusicGalleryHost {
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
        bgm: MenuBgm,
    ) -> Result<Self> {
        let items: Vec<_> = scenario
            .music_box_info
            .iter()
            .map(|item| CatalogItem {
                bgm: item.bgm_id.into_i32() as u16,
                name: item.name_index,
                once: item.once_flag,
            })
            .collect();
        let mut model =
            Controller::new(&items, &data.save_vectors.unlocked_bgms).map_err(|e| anyhow!(e))?;
        let atlas: Arc<CharacterTextures> = assets.load(music_gallery::ATLAS).await?;
        atlas.require_names(music_gallery::ATLAS, music_gallery::TEXTURES)?;
        if assets.context().game == crate::cli::Game::Gerokasu {
            atlas.require_names(music_gallery::ATLAS, &["bottom"])?;
        }
        for row in &model.rows {
            let name = format!("title{}", row.name / 32);
            atlas.require_names(music_gallery::ATLAS, &[name.as_str()])?;
            if row.unlocked && scenario.bgm_info.get(usize::from(row.item.bgm)).is_none() {
                return Err(anyhow!(
                    "Ger Music row references missing BGM{}",
                    row.item.bgm
                ));
            }
        }
        let mask = assets.load(music_gallery::MASK).await?;
        let system: Arc<CharacterTextures> = assets.load("/systex.txa").await?;
        if model.document_height > 800 {
            system.require_names("/systex.txa", &["scroll"])?;
        }
        model.assets_ready();
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(data.settings.v3_sysvol);
        bgm.lock().set_settings_volume(data.settings.v0_bgmvol);
        Ok(Self {
            model,
            atlas,
            system,
            mask,
            previous: None,
            assets,
            scenario,
            bgm,
            sounds,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
            unlocks: vec![],
        })
    }
    /// Complete composed outgoing frame, captured before retiring the Title.
    pub fn set_previous(&mut self, snapshot: Option<RenderTexture>) {
        self.previous = snapshot.map(|snapshot| {
            let mut clock = OverlayClock::captured(true);
            clock.masked(60, 0.3, 0, true);
            Overlay {
                clock,
                snapshot,
                mask: Some(self.mask.clone()),
            }
        });
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.model);
        self.pointer.cancel_contact(true);
        self.armed = false;
    }
    /// Keep the usable gallery when asynchronous Title reconstruction fails.
    /// This desktop error boundary does not model a native C++ exception path.
    pub fn recover_return(&mut self) {
        self.model.recover_return();
        self.cancel_input();
    }
    /// Only newly accepted global playback requests can publish these links.
    /// The app merges them with the durable save owner, preserving vector tails.
    pub fn take_unlocks(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.unlocks)
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Option<MusicGalleryAction>> {
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.));
        if [
            AppAction::PointerClick,
            AppAction::Enter,
            AppAction::Cancel,
            AppAction::CharacterVariant,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PageUp,
            AppAction::PageDown,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        self.pointer
            .sample(point, input[AppAction::PointerClick].is_held, seconds);
        self.sounds.update();
        let captured = if self.armed && self.model.accepts_input() {
            self.area.update_with_mouse_release(
                &mut self.model,
                &self.pointer.frame(),
                self.pointer.mouse_release(),
            )
        } else {
            self.area.disable(&mut self.model);
            false
        };
        self.model.poll_frame(
            delta,
            self.armed,
            captured,
            Input {
                confirm: input[AppAction::Enter].is_clicked,
                cancel: input[AppAction::Cancel].is_clicked,
                stop: input[AppAction::CharacterVariant].is_clicked,
                left: input[AppAction::Left].is_clicked_or_repeated,
                right: input[AppAction::Right].is_clicked_or_repeated,
                up: input[AppAction::Up].is_clicked_or_repeated,
                down: input[AppAction::AnyDown].is_clicked_or_repeated,
                previous: input[AppAction::PageUp].is_clicked_or_repeated,
                next: input[AppAction::PageDown].is_clicked_or_repeated,
            },
        );
        let mut event = None;
        for action in self.model.take_actions() {
            match action {
                Action::Cue(name) => self.sounds.play(&self.assets, name, Volume(1.)),
                Action::Stop(ticks) => self.bgm.lock().stop(Tween::linear(Ticks::from_u32(ticks))),
                Action::Play { bgm, once } => {
                    let info = self
                        .scenario
                        .bgm_info
                        .get(usize::from(bgm))
                        .ok_or_else(|| anyhow!("Missing Ger Music BGM{bgm}"))?;
                    if self.bgm.lock().request_with_loop_count(
                        self.assets.clone(),
                        format!("/bgm/{}.nxa", info.name.as_str()),
                        BgmId::from_number(i32::from(bgm)),
                        u32::from(once),
                        Volume(1.),
                        Tween::IMMEDIATE,
                    ) {
                        self.unlocks.push(u32::from(bgm));
                        if let Ok(link) = u32::try_from(info.linked_bgm_id) {
                            self.unlocks.push(link);
                        }
                    }
                }
                Action::ReturnTitle { requested_id } => {
                    event = Some(MusicGalleryAction::ReturnTitle { requested_id })
                }
                Action::Achievement(id) => {
                    tracing::debug!(id, "Ger Music completion platform notification")
                }
            }
        }
        let playing = self
            .bgm
            .lock()
            .get_wait_status()
            .contains(AudioWaitStatus::PLAYING);
        self.model.advance_clocks(delta, playing);
        if let Some(previous) = &mut self.previous {
            previous.clock.elapsed = self.model.opening_elapsed;
        }
        if matches!(
            self.model.phase,
            Phase::Active | Phase::Closing | Phase::Closed
        ) {
            self.previous = None;
        }
        Ok(event)
    }
    pub fn prepare(&mut self, _ctx: &mut PreRenderContext) -> Result<()> {
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let game = self.assets.context().game;
        view::render(&self.model, &self.atlas, &self.system, pass, game);
        if let Some(previous) = &self.previous {
            previous.render(pass);
        }
        Ok(())
    }
}
impl InputAreaTarget for Controller {
    fn probe(&mut self, p: Vec2) -> bool {
        self.accepts_input() && p.is_finite() && Controller::probe(self, p.x, p.y)
    }
    fn reset(&mut self) {
        Controller::reset(self)
    }
    fn press(&mut self, _: Vec2) {
        Controller::press(self)
    }
    fn release(&mut self, _: Vec2) {
        Controller::release(self)
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        Controller::drag(self, previous.y, current.y)
    }
    fn gesture(&mut self, v: Vec2) {
        Controller::gesture(self, v.y)
    }
    fn cancel(&mut self) {
        Controller::cancel(self)
    }
}
