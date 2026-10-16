//! Retained Title task children; input is owned by the newest live child.
use super::TitleItem;
use crate::{
    adv::{
        GlobalData, config::screen::ConfigScreen, input_area::PointerFrame,
        manual::screen::ManualScreen, save_game::SaveGameTask, save_menu::SaveMenuRequest,
        save_menu::screen::SaveMenuScreen,
    },
    app::AppAction,
    asset::system::AssetServer,
    render::PreRenderContext,
};
use enum_map::EnumMap;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

pub(super) struct LoadResources {
    pub scenario: Arc<shin_core::format::scenario::Scenario>,
    pub audio: Arc<shin_audio::AudioManager>,
    pub canvas: (u32, u32),
}
pub(super) enum TitleChild {
    Episodes(super::episode_chooser::scene::EpisodeChooserScene),
    Manual(ManualScreen),
    Load {
        screen: SaveMenuScreen,
        finished: bool,
        entry: Option<shin_core::format::save::GameData>,
    },
    Config {
        screen: ConfigScreen,
        finished: bool,
        server: Arc<AssetServer>,
        save: Option<SaveGameTask>,
    },
}
impl TitleChild {
    pub(super) fn suspend_pointer(&mut self, held: bool) {
        match self {
            Self::Config { screen, save, .. } => {
                screen.suspend_pointer(held);
                if let Some(task) = save {
                    task.suspend_pointer(held);
                }
            }
            Self::Load { screen, .. } => screen.suspend_pointer(held),
            _ => {}
        }
    }
    pub fn new(
        item: TitleItem,
        server: Arc<AssetServer>,
        globals: &GlobalData,
        resources: Option<&LoadResources>,
        completed_episodes: u16,
    ) -> anyhow::Result<Self> {
        Ok(match item {
            TitleItem::Characters | TitleItem::Tips => {
                Self::Episodes(super::episode_chooser::scene::EpisodeChooserScene::new(
                    item,
                    completed_episodes,
                    server,
                    resources
                        .ok_or_else(|| anyhow::anyhow!("Title viewers require global scenario"))?
                        .scenario
                        .clone(),
                )?)
            }
            TitleItem::Load => {
                let resources = resources
                    .ok_or_else(|| anyhow::anyhow!("Title Load requires global scenario/audio"))?;
                Self::Load {
                    screen: SaveMenuScreen::new(
                        server,
                        resources.audio.clone(),
                        resources.scenario.clone(),
                        globals.save().clone(),
                        None,
                        resources.canvas,
                    ),
                    finished: false,
                    entry: None,
                }
            }
            TitleItem::Manual => Self::Manual(ManualScreen::new(server)),
            TitleItem::Config => Self::Config {
                screen: ConfigScreen::new(server.clone(), globals.save().settings.clone()),
                finished: false,
                server,
                save: None,
            },
            _ => unreachable!("unsupported Title child"),
        })
    }
    pub fn is_finished(&self) -> bool {
        match self {
            Self::Episodes(screen) => screen.is_finished(),
            Self::Manual(screen) => screen.is_finished(),
            Self::Config { finished, .. } | Self::Load { finished, .. } => *finished,
        }
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        match self {
            Self::Episodes(screen) => screen.resize(extent),
            Self::Manual(screen) => screen.set_pointer_extent(extent),
            Self::Config { screen, .. } => screen.set_pointer_extent(extent),
            Self::Load { screen, .. } => screen.set_pointer_extent(extent),
        }
    }
    pub fn input(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        pointer: glam::Vec2,
        held: bool,
        elapsed: f64,
        axis: f32,
        extent: (u16, u16),
    ) {
        let canvas_pointer = pointer
            * glam::Vec2::new(
                1920.0 / extent.0.max(1) as f32,
                1080.0 / extent.1.max(1) as f32,
            );
        match self {
            Self::Episodes(screen) => screen.input(frame, state, pointer, held, elapsed, axis),
            Self::Manual(screen) => screen.input_frame(frame, state),
            Self::Load { screen, .. } => {
                screen.pointer_state(canvas_pointer, held, elapsed);
                screen.input_frame(frame, state, axis);
            }
            Self::Config { screen, save, .. } => {
                if let Some(save) = save {
                    save.pointer_state(canvas_pointer, held, elapsed, extent);
                    save.keyboard(state);
                    return;
                }
                screen.pointer_state(canvas_pointer, held, elapsed);
                screen.input_frame(frame, state, axis);
            }
        }
    }
    pub fn update(&mut self, delta: Ticks, globals: &mut GlobalData) -> anyhow::Result<bool> {
        match self {
            Self::Episodes(screen) => {
                screen.update(delta)?;
                Ok(false)
            }
            Self::Manual(screen) => Ok(screen.update(delta)? == Some(1)),
            Self::Load {
                screen,
                finished,
                entry,
            } => {
                let response = screen.update(delta)?;
                // db1a0 publishes remembered selection at final dialog Yes,
                // before the closing animation emits its entry request.
                globals.remember_load_slot(screen.model().remembered());
                for request in screen.take_requests() {
                    match request {
                        SaveMenuRequest::Load { game: selected, .. } => *entry = Some(selected),
                        _ => anyhow::bail!("Unexpected request in Title Load menu"),
                    }
                }
                *finished = response == Some(1);
                // Successful Load has no parent final callback/dimming reset.
                Ok(*finished)
            }
            Self::Config {
                screen,
                finished,
                server,
                save,
            } => {
                if let Some(task) = save {
                    if let Some(result) = globals.update_save_task(task, delta) {
                        if let Err(error) = result {
                            tracing::warn!(%error, "Saving Title Config settings failed");
                        }
                        screen.save_finished();
                        *save = None;
                    }
                }
                let response = screen.update(delta);
                let edits = screen.take_edits();
                if !edits.is_empty() {
                    globals.apply_config_settings(screen.settings());
                }
                let response = response?;
                if screen.take_save_request() {
                    match SaveGameTask::load(server).and_then(|task| {
                        globals.prepare_config_save()?;
                        Ok(task)
                    }) {
                        Ok(task) => *save = Some(task),
                        Err(error) => {
                            tracing::warn!(%error, "Preparing Title Config save failed");
                            // Keep the dirty initial-settings baseline and the
                            // same editable child; a later close retries saving.
                            screen.save_failed();
                        }
                    }
                }
                *finished = response == Some(1);
                Ok(*finished)
            }
        }
    }
    pub fn sounds(&mut self) -> Vec<&'static str> {
        match self {
            Self::Episodes(screen) => screen.sounds(),
            Self::Manual(screen) => screen.take_sounds(),
            Self::Config { screen, save, .. } => {
                let mut sounds = screen.take_sounds();
                if let Some(save) = save {
                    sounds.extend(save.take_sounds());
                }
                sounds
            }
            Self::Load { screen, .. } => screen.take_sounds(),
        }
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        match self {
            Self::Config { screen, save, .. } => {
                screen.prepare(context);
                if let Some(save) = save {
                    save.prepare(context);
                }
            }
            Self::Load { screen, .. } => screen.prepare(context),
            _ => {}
        }
    }
    pub fn render(
        &self,
        atlas: &crate::adv::characters::view::CharacterTextures,
        background: shin_render::shaders::types::texture::TextureSource<'_>,
        pass: &mut RenderPass,
    ) {
        match self {
            Self::Episodes(screen) => screen.render(atlas, background, pass),
            Self::Manual(screen) => screen.render(pass),
            Self::Config { screen, save, .. } => {
                screen.render(pass);
                if let Some(save) = save {
                    save.render(pass);
                }
            }
            Self::Load { screen, .. } => screen.render(pass),
        }
    }
    pub fn take_parent_requests(&mut self) -> Vec<super::episode_chooser::scene::ParentRequest> {
        match self {
            Self::Episodes(screen) => screen.take_parent_requests(),
            _ => Vec::new(),
        }
    }
    pub fn take_load_game_data(&mut self) -> Option<shin_core::format::save::GameData> {
        match self {
            Self::Load { entry, .. } => entry.take(),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn episodes(&self) -> Option<&super::episode_chooser::scene::EpisodeChooserScene> {
        match self {
            Self::Episodes(screen) => Some(screen),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn load(&self) -> Option<&SaveMenuScreen> {
        match self {
            Self::Load { screen, .. }
                if screen.model().phase() != crate::adv::save_menu::SaveMenuPhase::Closed =>
            {
                Some(screen)
            }
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn manual(&self) -> Option<&ManualScreen> {
        match self {
            Self::Manual(screen) if !screen.is_finished() => Some(screen),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn config(&self) -> Option<&ConfigScreen> {
        match self {
            Self::Config {
                screen,
                finished: false,
                ..
            } => Some(screen),
            _ => None,
        }
    }
}
