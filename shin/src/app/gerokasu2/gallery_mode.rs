//! Outer-mode gallery routing. The source remains owned until the destination
//! assets are ready; composed snapshots and global music survive publication.
use super::{
    AppAction, MenuBgm, Viewport,
    bustup_gallery::BustupGalleryHost,
    cg_gallery::{CgGalleryAction, CgGalleryHost},
    music_gallery::{MusicGalleryAction, MusicGalleryHost},
};
use crate::{asset::system::AssetServer, render::PreRenderContext};
use anyhow::Result;
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::format::{save::gerokasu2::SaveData, scenario::gerokasu2::Gerokasu2Scenario};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Cg,
    Music,
    Bustup,
}
pub(super) enum Host {
    Cg(CgGalleryHost),
    Music(MusicGalleryHost),
    Bustup(BustupGalleryHost),
}
impl Host {
    pub async fn load(
        kind: Kind,
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: SaveData,
        bgm: MenuBgm,
        snapshot: RenderTexture,
    ) -> Result<Self> {
        Ok(match kind {
            Kind::Cg => {
                let mut h = CgGalleryHost::load(assets, scenario, audio, &data, bgm).await?;
                h.set_previous(Some(snapshot));
                Self::Cg(h)
            }
            Kind::Music => {
                let mut h = MusicGalleryHost::load(assets, scenario, audio, &data, bgm).await?;
                h.set_previous(Some(snapshot));
                Self::Music(h)
            }
            Kind::Bustup => {
                let mut h = BustupGalleryHost::load(assets, scenario, audio, &data, bgm).await?;
                h.set_previous(Some(snapshot));
                Self::Bustup(h)
            }
        })
    }
    pub fn cancel_input(&mut self) {
        match self {
            Self::Cg(h) => h.cancel_input(),
            Self::Music(h) => h.cancel_input(),
            Self::Bustup(h) => h.cancel_input(),
        }
    }
    pub fn set_viewer_axes(&mut self, v: Vec2) {
        match self {
            Self::Cg(h) => h.set_viewer_axes(v),
            Self::Bustup(h) => h.set_viewer_axes(v),
            Self::Music(_) => {}
        }
    }
    pub fn recover_return(&mut self) {
        match self {
            Self::Cg(h) => h.recover_return(),
            Self::Music(h) => h.recover_return(),
            Self::Bustup(h) => h.recover_return(),
        }
    }
    pub fn take_unlocks(&mut self) -> Vec<u32> {
        match self {
            Self::Cg(_) | Self::Bustup(_) => vec![],
            Self::Music(h) => h.take_unlocks(),
        }
    }
    pub fn update(
        &mut self,
        dt: f32,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<Option<i32>> {
        Ok(match self {
            Self::Cg(h) => h
                .update(dt, input, point, viewport, seconds)
                .map(|CgGalleryAction::ReturnTitle { requested_id }| requested_id),
            Self::Music(h) => h
                .update(dt, input, point, viewport, seconds)?
                .map(|MusicGalleryAction::ReturnTitle { requested_id }| requested_id),
            Self::Bustup(h) => h.update(dt, input, point, viewport, seconds)?,
        })
    }
    pub fn prepare(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        match self {
            Self::Cg(h) => h.prepare(ctx),
            Self::Music(h) => h.prepare(ctx),
            Self::Bustup(h) => h.prepare(ctx),
        }
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        match self {
            Self::Cg(h) => h.render(pass),
            Self::Music(h) => h.render(pass),
            Self::Bustup(h) => h.render(pass),
        }
    }
}
