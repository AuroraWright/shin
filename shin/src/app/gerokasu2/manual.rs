//! Ger Title Manual child using original atlas, retained controller and shared services.
use super::{AppAction, Viewport};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::manual::{self, Action, Controller, Input},
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
};
use anyhow::{Result, ensure};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{format::save::Settings, vm::command::types::Volume};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;
mod view;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManualAction {
    Close,
}
pub(super) struct ManualHost {
    model: Controller,
    atlas: Arc<CharacterTextures>,
    assets: Arc<AssetServer>,
    sounds: SystemSoundPlayer,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
}
impl ManualHost {
    pub async fn load(
        assets: Arc<AssetServer>,
        audio: Arc<shin_audio::AudioManager>,
        settings: &Settings,
    ) -> Result<Self> {
        let atlas: Arc<CharacterTextures> = assets.load(manual::ATLAS).await?;
        atlas.require_names(manual::ATLAS, manual::TEXTURES)?;
        if assets.context().game == crate::cli::Game::Gerokasu {
            atlas.require_names(manual::ATLAS, &["bottom"])?;
        }
        let mut count = 0i32;
        loop {
            let next = count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Manual page count overflow"))?;
            if !atlas.contains(&format!("{next:03}")) {
                break;
            }
            count = next;
        }
        ensure!(
            count > 0,
            "Ger Manual archive has no consecutive numbered pages"
        );
        let mut model = Controller::new();
        model.assets_ready(count as usize);
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(settings.v3_sysvol);
        Ok(Self {
            model,
            atlas,
            assets,
            sounds,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
        })
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.model);
        self.pointer.cancel_contact(true);
        self.armed = false;
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Option<ManualAction> {
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.));
        let held = input[AppAction::PointerClick].is_held;
        if [
            AppAction::PointerClick,
            AppAction::Cancel,
            AppAction::Enter,
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
        self.pointer.sample(point, held, seconds);
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
        self.model.update(
            delta,
            self.armed,
            captured,
            Input {
                left: input[AppAction::Left].is_clicked_or_repeated,
                right: input[AppAction::Right].is_clicked_or_repeated,
                previous: input[AppAction::PageUp].is_clicked_or_repeated,
                next: input[AppAction::PageDown].is_clicked_or_repeated,
                close: input[AppAction::Cancel].is_clicked,
            },
        );
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.assets, cue, Volume(1.));
        }
        self.model
            .take_actions()
            .into_iter()
            .find_map(|action| match action {
                Action::ReturnSource(1) => Some(ManualAction::Close),
                Action::ReturnSource(_) => None,
            })
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        view::render(&self.model, &self.atlas, pass, self.assets.context().game);
        Ok(())
    }
}
impl InputAreaTarget for Controller {
    fn probe(&mut self, p: Vec2) -> bool {
        self.accepts_input() && Controller::probe(self, p.x, p.y)
    }
    fn reset(&mut self) {
        Controller::reset(self)
    }
    fn press(&mut self, _p: Vec2) {
        Controller::press(self)
    }
    fn release(&mut self, _p: Vec2) {
        Controller::release(self)
    }
    fn long_press(&mut self, _p: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        Controller::drag(self, previous.x, current.x)
    }
    fn gesture(&mut self, v: Vec2) {
        Controller::gesture(self, v.x, v.y)
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        let v = crate::adv::input_area::desktop_gesture_vector(displacement, velocity);
        Controller::gesture(self, v.x, v.y)
    }
    fn cancel(&mut self) {
        Controller::cancel(self)
    }
}
