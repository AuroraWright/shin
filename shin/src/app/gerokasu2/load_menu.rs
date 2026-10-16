//! Retained native manual Save/Load owner; payloads remain in the Ger store.
use super::{AppAction, Viewport};
use crate::{
    adv::{
        gerokasu2::{
            config::dialog::GerDialog,
            save_menu::{self, Controller, Input, Phase, Pointer},
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::{font::gerokasu2::GpuGerokasu2Font, system::AssetServer},
    audio::SystemSoundPlayer,
};
use anyhow::Result;
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    format::{
        save::gerokasu2::{SaveData, SaveDate},
        scenario::gerokasu2::Gerokasu2Scenario,
    },
    time::Ticks,
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;
mod preview;
mod view;
pub(super) use save_menu::{Action as LoadMenuAction, StorageRequest};
pub(super) struct LoadMenuHost {
    model: Controller,
    assets: Arc<AssetServer>,
    data: SaveData,
    atlas: Arc<crate::adv::characters::view::CharacterTextures>,
    system: Arc<crate::adv::characters::view::CharacterTextures>,
    dates: [Option<SaveDate>; 100],
    previews: preview::Previews,
    empty: view::Labels,
    loading: view::Labels,
    dialog: Option<GerDialog>,
    pointer: PointerSampler,
    area: InputArea,
    target: Pointer,
    armed: bool,
    axis: f32,
    sounds: SystemSoundPlayer,
}
impl LoadMenuHost {
    #[cfg(test)]
    pub(super) fn reference_phase(&self) -> Phase {
        self.model.phase()
    }
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
    ) -> Result<Self> {
        Self::load_with_mode(assets, scenario, audio, data, save_menu::Mode::Load).await
    }
    pub async fn load_with_mode(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
        mode: save_menu::Mode,
    ) -> Result<Self> {
        let (atlas, font, system) = futures::try_join!(
            assets.load::<crate::adv::characters::view::CharacterTextures, _>(save_menu::ATLAS),
            assets.load::<GpuGerokasu2Font, _>("/system.fnt"),
            assets.load::<crate::adv::characters::view::CharacterTextures, _>("/systex.txa")
        )?;
        atlas.require_names(save_menu::ATLAS, &save_menu::TEXTURES)?;
        if assets.context().game == crate::cli::Game::Gerokasu {
            atlas.require_names(save_menu::ATLAS, &["bottom"])?;
        }
        system.require_names("/systex.txa", &["scroll"])?;
        let mut model = Controller::new_with_mode(
            mode,
            data.save_menu_position,
            std::array::from_fn(|i| data.manual_save_slots[i].is_some()),
        )
        .map_err(anyhow::Error::msg)?;
        let empty = view::Labels::prepare(&assets, &font, &font.localize("データがありません"), None)?;
        let loading = view::Labels::prepare(&assets, &font, &font.localize("loading..."), None)?;
        model.assets_ready();
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(data.settings.v3_sysvol);
        let mut area = InputArea::canvas();
        area.set_long_press_enabled(true);
        Ok(Self {
            model,
            previews: preview::Previews::new(assets.clone(), scenario, font),
            assets,
            data: data.clone(),
            atlas,
            system,
            dates: std::array::from_fn(|i| data.manual_save_slots[i].as_ref().map(|s| s.date)),
            empty,
            loading,
            dialog: None,
            pointer: Default::default(),
            area,
            target: Pointer::Outside,
            armed: false,
            axis: 0.0,
            sounds,
        })
    }
    pub fn cancel_input(&mut self) {
        self.pointer.cancel_contact(true);
        self.armed = false;
        let mut target = Contact {
            model: &mut self.model,
            target: &mut self.target,
        };
        self.area.disable(&mut target);
        self.model.pointer_cancel();
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.cancel_input();
        }
    }
    pub fn set_scroll_axis(&mut self, value: f32) {
        self.axis = if value.is_finite() { value } else { 0.0 }
    }
    pub fn take_storage_requests(&mut self) -> Vec<StorageRequest> {
        self.model.take_storage_requests()
    }
    pub fn load_failed(&mut self) {
        self.model.load_failed();
        self.cancel_input()
    }
    pub fn write_finished(&mut self, success: bool, data: &SaveData) {
        self.refresh_slots(data);
        self.model.write_finished(success);
        self.cancel_input();
    }
    pub fn refresh_slots(&mut self, data: &SaveData) {
        for i in 0..100 {
            if self.data.manual_save_slots[i] != data.manual_save_slots[i] {
                self.dates[i] = data.manual_save_slots[i].as_ref().map(|s| s.date);
                self.previews.invalidate(i);
            }
        }
        self.data = data.clone();
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Option<LoadMenuAction> {
        self.sounds.update();
        let point = viewport.map(point).unwrap_or(Vec2::splat(-1.0));
        let held = input[AppAction::PointerClick].is_held;
        if [
            AppAction::Enter,
            AppAction::Cancel,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PointerClick,
            AppAction::DeleteSave,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true
        }
        self.pointer.sample(point, held, seconds);
        if let Some(dialog) = self.dialog.as_mut() {
            if self.armed {
                dialog.pointer_state(point, held, seconds);
                dialog.keyboard(input)
            }
            let responses = dialog.update(Ticks::from_f32(delta));
            let sounds = dialog.take_sounds();
            for cue in sounds {
                self.sounds.play(&self.assets, cue, Volume(1.0));
            }
            let done = responses.iter().any(|r| r.final_flag);
            for response in responses {
                self.model
                    .dialog_response(response.final_flag, response.button)
            }
            if done {
                self.dialog = None;
                self.cancel_input()
            }
        } else if self.armed && self.model.phase() == Phase::Active {
            let mut target = Contact {
                model: &mut self.model,
                target: &mut self.target,
            };
            let captured = self.area.update_with_mouse_release(
                &mut target,
                &self.pointer.frame(),
                self.pointer.mouse_release(),
            );
            if !captured {
                self.model.input(Input {
                    up: input[AppAction::Up].is_clicked_or_repeated,
                    down: input[AppAction::AnyDown].is_clicked_or_repeated,
                    left: input[AppAction::Left].is_clicked_or_repeated,
                    right: input[AppAction::Right].is_clicked_or_repeated,
                    previous_page: input[AppAction::PageUp].is_clicked_or_repeated,
                    next_page: input[AppAction::PageDown].is_clicked_or_repeated,
                    confirm: input[AppAction::Enter].is_clicked,
                    // Ger B56A8 tests native bit7, which the A14D0 producer
                    // assigns to Y: the shared DeleteSave binding.
                    delete: input[AppAction::DeleteSave].is_clicked,
                    close: input[AppAction::Cancel].is_clicked,
                    axis: self.axis,
                })
            }
        }
        self.axis = 0.0;
        let action = self.model.update(delta);
        if self.model.phase() != Phase::Handoff {
            self.previews.update(self.model.scroll(), &self.data);
        }
        if let Some(question) = self.model.take_question() {
            match GerDialog::load(
                &self.assets,
                question.kind.message(),
                &["はい", "いいえ"],
                1,
                true,
                held,
            ) {
                Ok(dialog) => {
                    self.dialog = Some(dialog);
                    self.cancel_input()
                }
                Err(error) => {
                    tracing::error!(%error,"Ger Load confirmation failed");
                    self.model.dialog_response(true, -1);
                    self.sounds.play(&self.assets, "error", Volume(1.0));
                }
            }
        }
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.assets, cue, Volume(1.0))
        }
        action
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        view::render(
            pass,
            &self.atlas,
            &self.system,
            &self.model,
            &self.dates,
            &self.previews.labels,
            &self.previews.textures,
            &self.empty,
            &self.loading,
            self.assets.context().game,
        )?;
        if let Some(dialog) = &self.dialog {
            dialog.render(pass)?
        }
        Ok(())
    }

    pub fn prepare(&mut self, context: &mut crate::render::PreRenderContext) -> Result<()> {
        self.previews.prepare(context)?;
        if let Some(dialog) = &mut self.dialog {
            dialog.prepare(context)?;
        }
        Ok(())
    }
}
struct Contact<'a> {
    model: &'a mut Controller,
    target: &'a mut Pointer,
}
impl InputAreaTarget for Contact<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        if !p.is_finite() || p.x < 0.0 || p.y < 0.0 || p.x >= 1920.0 || p.y >= 1080.0 {
            return false;
        }
        *self.target = self.model.pointer(p.x, p.y);
        match *self.target {
            Pointer::Content(None) => {
                (160.0..1760.0).contains(&p.x) && (176.0..976.0).contains(&p.y)
            }
            _ => true,
        }
    }
    fn reset(&mut self) {
        *self.target = Pointer::Outside
    }
    fn press(&mut self, _: Vec2) {
        self.model.pointer_press(*self.target)
    }
    fn release(&mut self, _: Vec2) {
        self.model.pointer_release(*self.target)
    }
    fn long_press(&mut self, _: Vec2) {
        self.model.pointer_cancel();
        *self.target = Pointer::Content(None)
    }
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        if matches!(*self.target, Pointer::Content(_)) {
            self.model.pointer_cancel();
            self.model
                .drag_scroll(self.model.scroll() - (current.y - previous.y));
            *self.target = Pointer::Content(None)
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if matches!(*self.target, Pointer::Content(_)) {
            self.model.release_scroll(velocity.y)
        }
    }
    fn cancel(&mut self) {
        self.model.pointer_cancel();
        *self.target = Pointer::Outside
    }
}
