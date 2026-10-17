//! Native Title artwork and retained menu ownership.
use super::{AppAction, MenuBgm, Viewport};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::reconstruction_mode::{OverlayClock, overlay::Overlay},
        gerokasu2::title::{self, TitleAction, TitleAvailability, TitleController, TitleInput},
        input_area::{PointerKind, PointerSampler},
    },
    asset::{mask::MaskTexture, system::AssetServer},
    audio::SystemSoundPlayer,
};
use anyhow::Result;
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::{
        save::gerokasu2::SaveData,
        scenario::{gerokasu2::Gerokasu2Scenario, info::BgmId, instruction_elements::FromNumber},
    },
    primitives::color::FloatColor4,
    time::{Ticks, Tween},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

pub(super) struct TitleHost {
    pub model: TitleController,
    atlas: Arc<CharacterTextures>,
    input: MenuInput,
    bgm: MenuBgm,
    sounds: SystemSoundPlayer,
    assets: Arc<AssetServer>,
    previous: Option<Overlay>,
    mask: Arc<MaskTexture>,
    music_path: Option<String>,
    masked_entry: bool,
    pending_settings_volume: Option<u8>,
    /// Gerokasu's greeting on a negative (boot/story-return) request.
    voice: Option<super::system_voice::SystemVoice>,
}
impl TitleHost {
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: &Gerokasu2Scenario,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
        requested_item: i32,
        bgm: MenuBgm,
    ) -> Result<Self> {
        let (atlas, mask) = futures::try_join!(
            assets.load::<CharacterTextures, _>(title::TITLE_ATLAS_PATH),
            assets.load::<MaskTexture, _>("/change.msk"),
        )?;
        atlas.require_names(title::TITLE_ATLAS_PATH, &title::TITLE_TEXTURE_NAMES)?;
        let mut model = TitleController::new(availability(data), requested_item);
        let mut sounds = SystemSoundPlayer::new(audio.clone());
        sounds.set_settings_volume(data.settings.v3_sysvol);
        let id = model.bgm_id();
        let music_path = scenario
            .bgm_info
            .get(id as usize)
            .map(|info| format!("/bgm/{}.nxa", info.name.as_str()));
        model.assets_ready();
        // dbef0: a negative request plays a random `/voice/sys/` greeting
        // before installing the Start task (Gerokasu only).
        let voice = (assets.context().game == crate::cli::Game::Gerokasu && requested_item < 0)
            .then(|| {
                let mut voice = super::system_voice::SystemVoice::new(
                    audio.clone(),
                    assets.clone(),
                    data.settings.v2_voicevol,
                );
                voice.play(&super::system_voice::title_clip(
                    availability(data).extra_unlocked,
                    crate::engine_random::next_seed,
                ));
                voice
            });
        let host = Self {
            model,
            atlas,
            input: Default::default(),
            bgm,
            sounds,
            assets,
            previous: None,
            mask,
            music_path,
            masked_entry: requested_item >= 0,
            pending_settings_volume: Some(data.settings.v0_bgmvol),
            voice,
        };
        // First's effects run after publication on the application's thread.
        // A canceled background factory must not change the retained source's
        // shared music player or gallery unlocks.
        Ok(host)
    }

    pub fn cancel_input(&mut self) {
        self.input.cancel();
    }

    pub fn set_previous(&mut self, snapshot: Option<shin_render::render_texture::RenderTexture>) {
        self.previous = snapshot.map(|snapshot| {
            let mut clock = OverlayClock::captured(true);
            // Original DD110 clears a negative request's capture; positive
            // requests use DD1A8's masked entry.
            if self.masked_entry {
                clock.masked(60, f32::from_bits(0x3e99999a), 0, true);
            } else {
                clock.clear(60);
            }
            Overlay {
                clock,
                snapshot,
                mask: self.masked_entry.then(|| self.mask.clone()),
            }
        });
    }

    pub fn apply_settings(&mut self, settings: &shin_core::format::save::Settings) {
        self.pending_settings_volume = None;
        self.bgm.lock().set_settings_volume(settings.v0_bgmvol);
        self.sounds.set_settings_volume(settings.v3_sysvol);
    }

    pub fn update_audio(&mut self) {
        self.sounds.update();
    }

    /// The app calls this after retained child polling and parent callbacks.
    /// Ordinary Title::update already advances the same parent clock itself.
    pub fn advance_retained_parent_frame(&mut self, delta: f32) {
        self.model.advance_retained_parent_frame(delta);
    }

    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Option<TitleAction> {
        self.update_audio();
        if let Some(voice) = &mut self.voice {
            voice.update(Ticks::from_f32(delta));
        }
        for cue in self
            .input
            .update(&mut self.model, input, point, viewport, seconds)
        {
            self.sounds.play(&self.assets, cue, Volume(1.0));
        }
        let phase = self.model.phase();
        let action = self.model.update(delta);
        self.dispatch_music();
        if let Some(previous) = self.previous.as_mut() {
            if phase == title::TitlePhase::Opening {
                previous.clock.advance(Ticks::from_f32(delta));
            } else {
                self.previous = None;
            }
        }
        if phase != title::TitlePhase::Fading && self.model.phase() == title::TitlePhase::Fading {
            self.bgm.lock().stop(Tween::linear(Ticks::from_f32(120.0)));
        }
        action
    }

    fn dispatch_music(&mut self) {
        if let Some(percent) = self.pending_settings_volume.take() {
            self.bgm.lock().set_settings_volume(percent);
        }
        for action in self.model.take_music_actions() {
            let mut music = self.bgm.lock();
            match action {
                title::TitleMusicAction::Stop(ticks) => {
                    if !music.get_wait_status().is_empty() {
                        music.stop(Tween::linear(Ticks::from_u32(ticks)));
                    }
                }
                title::TitleMusicAction::Play(ticks) => {
                    if let Some(path) = self.music_path.as_ref() {
                        music.request(
                            self.assets.clone(),
                            path.clone(),
                            BgmId::from_number(self.model.bgm_id()),
                            true,
                            Volume(1.),
                            Tween::linear(Ticks::from_u32(ticks)),
                        );
                    }
                }
            }
        }
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let matrix = Mat4::from_cols_array(&[
            2.0 / 1920.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / 1080.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -0.5,
            0.0,
            -1.0,
            1.0,
            0.5,
            1.0,
        ]);
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        for (name, rect) in [
            ("bg", title::TITLE_BACKGROUND_RECT),
            ("logo", title::TITLE_LOGO_RECT),
            ("copyright", title::TITLE_COPYRIGHT_RECT),
        ] {
            let [x, y, width, height] = rect;
            QuadVertices::new()
                .with_box(x, y, x + width, y + height)
                .render_sprite(pass, builder, self.atlas.get(name).as_source(), matrix);
        }
        for (index, item) in self.model.rows().iter().enumerate() {
            let alpha = self.model.row_alpha(index);
            if alpha <= 0.0 {
                continue;
            }
            let [x, y, width, height] = title::button_rect(index);
            let [u, v, uw, vh] = title::button_source_rect(*item);
            let [sw, sh] = title::TITLE_BUTTON_SHEET_SIZE;
            let name = if self.model.selected() == Some(index) {
                "menu_focus"
            } else {
                "menu_normal"
            };
            QuadVertices::new()
                .with_box(x, y, x + width, y + height)
                .with_tex_box(u / sw, v / sh, (u + uw) / sw, (v + vh) / sh)
                .with_color(FloatColor4::WHITE.with_alpha(alpha).into_unorm())
                .render_button(
                    pass,
                    builder,
                    self.atlas.get(name).as_source(),
                    matrix,
                    Vec4::new(1.0, 1.0, 1.0, self.model.row_flash(index)),
                    Vec4::new(1.0, 1.0, 1.0, 0.0),
                );
        }
        let alpha = self.model.blackout();
        if self.model.phase() == title::TitlePhase::Opening && self.previous.is_some() {
            self.previous.as_ref().unwrap().render(pass);
        } else if alpha > 0.0 {
            QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(FloatColor4::BLACK.with_alpha(alpha).into_unorm())
                .fill(pass, builder, matrix);
        }
    }
}

#[derive(Default)]
struct MenuInput {
    pointer: PointerSampler,
    pressed_row: Option<usize>,
    armed: bool,
}
impl MenuInput {
    fn cancel(&mut self) {
        self.pointer.cancel_contact(true);
        self.pressed_row = None;
        self.armed = false;
    }
    fn update(
        &mut self,
        model: &mut TitleController,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Vec<&'static str> {
        let mut cues = Vec::new();
        if [
            AppAction::Enter,
            AppAction::Cancel,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::PointerClick,
        ]
        .into_iter()
        .all(|action| !input[action].is_held)
        {
            self.armed = true;
        }
        let point = viewport.map(point);
        self.pointer.sample(
            point.unwrap_or(Vec2::splat(-1.0)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        if self.armed {
            for event in self.pointer.frame().events {
                match event.kind {
                    PointerKind::Down => {
                        self.pressed_row = point
                            .and_then(|p| model.point(p.x, p.y))
                            .filter(|row| model.select(*row));
                    }
                    PointerKind::Up => {
                        if let Some(row) = self.pressed_row.take()
                            && point.and_then(|p| model.point(p.x, p.y)) == Some(row)
                        {
                            model.select(row);
                            if model.confirm_selected() {
                                cues.push("decide");
                            }
                        }
                    }
                    PointerKind::Cancel => self.pressed_row = None,
                    _ => {}
                }
            }
            let before_phase = model.phase();
            let before_selected = model.selected();
            model.input(TitleInput {
                up: input[AppAction::Up].is_clicked_or_repeated,
                down: input[AppAction::AnyDown].is_clicked_or_repeated,
                confirm: input[AppAction::Enter].is_clicked,
                back: input[AppAction::Cancel].is_clicked,
            });
            if before_phase == title::TitlePhase::Active {
                if model.phase() == title::TitlePhase::Leaving {
                    cues.push(if input[AppAction::Enter].is_clicked {
                        "decide"
                    } else {
                        "cancel"
                    });
                } else if model.selected() != before_selected {
                    cues.push("move");
                }
            }
        }
        cues
    }
}

pub(super) fn availability(data: &SaveData) -> TitleAvailability {
    TitleAvailability {
        continue_available: data.auto_save_slot.is_some(),
        extra_unlocked: data.persist_data.0.first().copied().unwrap_or(0) != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::save::gerokasu2::{GameData, Progress, SaveDate};

    fn ready() -> TitleController {
        let mut model = TitleController::new(TitleAvailability::default(), -1);
        model.assets_ready();
        for _ in 0..5 {
            model.update(100.0);
        }
        assert_eq!(model.phase(), title::TitlePhase::Active);
        model
    }
    fn input(
        owner: &mut MenuInput,
        model: &mut TitleController,
        held: bool,
        x: f32,
        y: f32,
        time: f64,
    ) {
        let mut actions: EnumMap<AppAction, ActionState> = EnumMap::default();
        actions[AppAction::PointerClick].is_held = held;
        owner.update(
            model,
            &actions,
            Vec2::new(x, y),
            Viewport {
                offset: Vec2::ZERO,
                size: Vec2::new(1920.0, 1080.0),
            },
            time,
        );
    }
    #[test]
    fn title_input_ignores_contacts_from_loading_letterbox_or_focus_loss() {
        let mut owner = MenuInput::default();
        let mut model = TitleController::new(TitleAvailability::default(), -1);
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.0);
        input(&mut owner, &mut model, true, 1500.0, 460.0, 0.1);
        model.assets_ready();
        for _ in 0..5 {
            model.update(100.0);
        }
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.2);
        assert_eq!(model.phase(), title::TitlePhase::Active);
        input(&mut owner, &mut model, true, -1.0, 460.0, 0.3);
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.4);
        assert_eq!(model.phase(), title::TitlePhase::Active);
        input(&mut owner, &mut model, true, 1500.0, 460.0, 0.5);
        owner.cancel();
        input(&mut owner, &mut model, true, 1500.0, 460.0, 0.6);
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.7);
        assert_eq!(model.phase(), title::TitlePhase::Active);
        input(&mut owner, &mut model, true, 1500.0, 460.0, 0.8);
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.9);
        assert_eq!(model.phase(), title::TitlePhase::Leaving);
    }
    #[test]
    fn title_pointer_release_must_match_the_captured_row() {
        let mut model = ready();
        let mut owner = MenuInput::default();
        input(&mut owner, &mut model, false, 1500.0, 460.0, 0.0);
        input(&mut owner, &mut model, true, 1500.0, 460.0, 0.1);
        input(&mut owner, &mut model, false, 1500.0, 544.0, 0.2);
        assert_eq!(model.phase(), title::TitlePhase::Active);
        input(&mut owner, &mut model, true, 1500.0, 544.0, 0.3);
        input(&mut owner, &mut model, false, 1500.0, 544.0, 0.4);
        assert_eq!(
            model.rows()[model.selected().unwrap()],
            title::TitleItem::Load
        );
        assert_eq!(model.phase(), title::TitlePhase::Leaving);
    }

    #[test]
    fn title_availability_uses_active_autosave_and_signed_persistent_setting_zero() {
        let mut data = SaveData::default();
        assert_eq!(availability(&data), TitleAvailability::default());
        let game = GameData {
            date: SaveDate::default(),
            progress: Progress::fresh(0, 42),
        };
        data.manual_save_slots[99] = Some(game.clone());
        assert!(!availability(&data).continue_available);
        data.auto_save_slot = Some(game);
        assert!(availability(&data).continue_available);
        for value in [i16::MIN, -1, 0, 1, 2, i16::MAX] {
            data.persist_data.0 = vec![value, 1];
            assert_eq!(availability(&data).extra_unlocked, value != 0);
        }
    }

    /// Scripted gallery runs: `SHIN_GEROKASU_UNLOCKED_SAVE=<path>`.
    #[test]
    #[ignore = "writes an Extra-unlocked save for scripted gallery runs"]
    fn write_extra_unlocked_save() {
        let path = std::env::var_os("SHIN_GEROKASU_UNLOCKED_SAVE").unwrap();
        let mut data = SaveData::default();
        data.persist_data.0 = vec![1];
        std::fs::write(path, data.encode().unwrap()).unwrap();
    }
}
