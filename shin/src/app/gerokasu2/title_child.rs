//! Retained title children. Their native controllers own animation and input;
//! the app owns disk publication and replacement of the live ADV scene.
use super::{AppAction, Viewport, config, load_menu, manual};
use crate::{asset::system::AssetServer, render::PreRenderContext};
use anyhow::Result;
use enum_map::EnumMap;
use glam::Vec2;
use shin_audio::AudioManager;
use shin_core::format::{
    save::{Settings, gerokasu2::SaveData},
    scenario::gerokasu2::Gerokasu2Scenario,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Save,
    Load,
    Config,
    Manual,
}
pub(super) enum TitleChild {
    Load(load_menu::LoadMenuHost),
    Config(config::ConfigHost),
    Manual(manual::ManualHost),
}
pub(super) enum Event {
    /// Native manual slots use0..99; the restore API reserves0 for autosave.
    Load(u8),
    Remember(u8),
    Save(u8),
    Delete(u8),
    SettingsChanged(Settings),
    SaveRequested,
    Close,
}
impl TitleChild {
    pub async fn load(
        kind: Kind,
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<AudioManager>,
        data: SaveData,
    ) -> Result<Self> {
        Ok(match kind {
            Kind::Save => Self::Load(
                load_menu::LoadMenuHost::load_with_mode(
                    assets,
                    scenario,
                    audio,
                    &data,
                    crate::adv::gerokasu2::save_menu::Mode::Save,
                )
                .await?,
            ),
            Kind::Load => {
                Self::Load(load_menu::LoadMenuHost::load(assets, scenario, audio, &data).await?)
            }
            Kind::Config => {
                Self::Config(config::ConfigHost::load(assets, audio, data.settings).await?)
            }
            Kind::Manual => {
                Self::Manual(manual::ManualHost::load(assets, audio, &data.settings).await?)
            }
        })
    }
    pub fn cancel_input(&mut self) {
        match self {
            Self::Load(host) => host.cancel_input(),
            Self::Config(host) => host.cancel_input(),
            Self::Manual(host) => host.cancel_input(),
        }
    }
    pub fn set_scroll_axis(&mut self, axis: f32) {
        match self {
            Self::Load(host) => host.set_scroll_axis(axis),
            Self::Config(host) => host.set_scroll_axis(axis),
            Self::Manual(_) => {}
        }
    }
    pub fn update(
        &mut self,
        delta: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Vec<Event>> {
        Ok(match self {
            Self::Manual(host) => host
                .update(delta, input, point, viewport, seconds)
                .map(|action| match action {
                    manual::ManualAction::Close => Event::Close,
                })
                .into_iter()
                .collect(),
            Self::Load(host) => {
                let action = host.update(delta, input, point, viewport, seconds);
                let mut events: Vec<_> = host
                    .take_storage_requests()
                    .into_iter()
                    .map(|request| match request {
                        load_menu::StorageRequest::Remember(slot) => Event::Remember(slot),
                        load_menu::StorageRequest::Save { slot } => Event::Save(slot),
                        load_menu::StorageRequest::Delete { slot } => Event::Delete(slot),
                    })
                    .collect();
                if let Some(action) = action {
                    events.push(match action {
                        load_menu::LoadMenuAction::Load { slot } => Event::Load(slot),
                        load_menu::LoadMenuAction::Close => Event::Close,
                    });
                }
                events
            }
            Self::Config(host) => host
                .update(delta, input, point, viewport, seconds)?
                .into_iter()
                .map(|event| match event {
                    config::ConfigEvent::SettingsChanged(settings) => {
                        Event::SettingsChanged(settings)
                    }
                    config::ConfigEvent::SaveRequested => Event::SaveRequested,
                    config::ConfigEvent::Close => Event::Close,
                })
                .collect(),
        })
    }
    pub fn load_failed(&mut self) {
        if let Self::Load(host) = self {
            host.load_failed();
            host.cancel_input();
        }
    }
    pub fn save_finished(&mut self) {
        if let Self::Config(host) = self {
            host.save_finished();
        }
    }
    pub fn manual_write_finished(&mut self, success: bool, data: &SaveData) {
        if let Self::Load(host) = self {
            host.write_finished(success, data);
        }
    }
    pub fn manual_write_staged(&mut self, data: &SaveData) {
        if let Self::Load(host) = self {
            host.refresh_slots(data);
        }
    }
    pub fn save_failed(&mut self) {
        if let Self::Config(host) = self {
            host.save_failed();
        }
    }
    pub fn prepare(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        match self {
            Self::Config(host) => host.prepare(ctx),
            Self::Load(host) => host.prepare(ctx),
            Self::Manual(_) => Ok(()),
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        match self {
            Self::Load(host) => host.render(pass),
            Self::Manual(host) => host.render(pass),
            Self::Config(host) => {
                host.render(pass);
                Ok(())
            }
        }
    }
}
