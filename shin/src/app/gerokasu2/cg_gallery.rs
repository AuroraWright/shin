//! Ger outer CG mode, real atlas/Picture jobs and composed transition captures.
use super::{AppAction, MenuBgm, Viewport};
use crate::{
    adv::{
        characters::view::CharacterTextures,
        gerokasu2::{
            cg_gallery::{
                self, Action, Controller, Entry, Input,
                viewer::{self, Viewer},
            },
            reconstruction_mode::{OverlayClock, overlay::Overlay},
        },
        input_area::{InputArea, InputAreaTarget, PointerSampler},
    },
    asset::{mask::MaskTexture, picture::Picture, system::AssetServer},
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result, ensure};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    format::{
        save::gerokasu2::SaveData,
        scenario::{gerokasu2::Gerokasu2Scenario, info::BgmId, instruction_elements::FromNumber},
    },
    time::{Ticks, Tween},
    vm::command::types::Volume,
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;
mod view;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CgGalleryAction {
    ReturnTitle { requested_id: i32 },
}
struct PictureJob {
    task: Option<shin_tasks::AsyncTask<Result<Arc<Picture>>>>,
}
impl Drop for PictureJob {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}
struct ViewerHost {
    model: Viewer,
    picture: Option<Arc<Picture>>,
    job: Option<PictureJob>,
    prepared: Option<Arc<Picture>>,
    capture: Option<Overlay>,
    capture_pending: Option<bool>,
    publication_delta: f32,
}
pub(super) struct CgGalleryHost {
    model: Controller,
    atlas: Arc<CharacterTextures>,
    mask: Arc<MaskTexture>,
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    bgm: MenuBgm,
    sounds: SystemSoundPlayer,
    pointer: PointerSampler,
    area: InputArea,
    armed: bool,
    viewer: Option<ViewerHost>,
    previous: Option<Overlay>,
    axes: Vec2,
}
impl CgGalleryHost {
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        data: &SaveData,
        bgm: MenuBgm,
    ) -> Result<Self> {
        let entries = scenario
            .picture_box_info
            .iter()
            .map(|item| {
                let pictures = item.picture_ids.0.to_vec();
                let unlocked = pictures
                    .iter()
                    .map(|id| {
                        data.save_vectors
                            .unlocked_cgs
                            .get(*id as usize >> 5)
                            .is_some_and(|w| w & (1 << (*id as usize & 31)) != 0)
                    })
                    .collect();
                Entry {
                    name: item.name.as_str().to_owned(),
                    pictures,
                    unlocked,
                }
            })
            .collect();
        let mut model = Controller::new(entries).map_err(anyhow::Error::msg)?;
        let atlas: Arc<CharacterTextures> = assets.load(cg_gallery::ATLAS).await?;
        atlas.require_names(cg_gallery::ATLAS, cg_gallery::TEXTURES)?;
        // Native4EE70 publishes the atlas before acquiring change.msk. This host
        // awaits both before replacing Title; renderer publication timing differs.
        for entry in &model.entries {
            atlas.require_names(cg_gallery::ATLAS, &[&entry.name])?;
            for id in &entry.pictures {
                ensure!(
                    scenario.picture_info.get(*id as usize).is_some(),
                    "CG catalog picture metadata ID{id} missing"
                );
            }
        }
        let mask = assets.load("/change.msk").await?;
        model.assets_ready();
        let mut sounds = SystemSoundPlayer::new(audio);
        sounds.set_settings_volume(data.settings.v3_sysvol);
        bgm.lock().set_settings_volume(data.settings.v0_bgmvol);
        Ok(Self {
            model,
            atlas,
            mask,
            assets,
            scenario,
            bgm,
            sounds,
            pointer: Default::default(),
            area: InputArea::canvas(),
            armed: false,
            viewer: None,
            previous: None,
            axes: Vec2::ZERO,
        })
    }
    /// Keep this retained gallery usable if the desktop destination loader fails.
    pub fn recover_return(&mut self) {
        self.viewer = None;
        self.model.recover_return();
        self.cancel_input();
    }
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
    pub fn set_viewer_axes(&mut self, axes: Vec2) {
        self.axes = if axes.is_finite() {
            axes.clamp(Vec2::splat(-1.), Vec2::ONE)
        } else {
            Vec2::ZERO
        };
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut Target {
            grid: &mut self.model,
            viewer: self.viewer.as_mut().map(|v| &mut v.model),
        });
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
    ) -> Option<CgGalleryAction> {
        if self.viewer.as_ref().is_some_and(|v| v.model.finished) {
            self.viewer = None;
            self.model.set_viewer_owned(false);
            self.cancel_input();
        }
        if [
            AppAction::PointerClick,
            AppAction::Cancel,
            AppAction::Enter,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::PageUp,
            AppAction::PageDown,
            AppAction::CharacterState,
            AppAction::CharacterVariant,
        ]
        .into_iter()
        .all(|a| !input[a].is_held)
        {
            self.armed = true;
        }
        self.pointer.sample(
            viewport.map(point).unwrap_or(Vec2::splat(-1.)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        self.sounds.update();
        let enabled = self.armed
            && self
                .viewer
                .as_ref()
                .map_or_else(|| self.model.accepts_input(), |v| v.model.accepts_input());
        let mut target = Target {
            grid: &mut self.model,
            viewer: self.viewer.as_mut().map(|v| &mut v.model),
        };
        let captured = if enabled {
            self.area.update_with_mouse_release(
                &mut target,
                &self.pointer.frame(),
                self.pointer.mouse_release(),
            )
        } else {
            self.area.disable(&mut target);
            false
        };
        if let Some(v) = &mut self.viewer {
            // Native fixed task snapshot sees Ready before its lower Input task.
            if let Some(result) = v
                .job
                .as_mut()
                .and_then(|j| j.task.as_mut())
                .and_then(|t| t.poll_naive())
            {
                v.job.as_mut().unwrap().task = None;
                match result {
                    Ok(picture) => {
                        v.prepared = Some(picture);
                        v.model.picture_ready();
                    }
                    Err(error) => {
                        tracing::warn!(%error,"Ger CG picture preparation failed; retaining gallery");
                        self.viewer = None;
                        self.model.set_viewer_owned(false);
                        self.cancel_input();
                        return None;
                    }
                }
            }
            v.model.controls(
                enabled,
                captured,
                viewer::Input {
                    confirm: input[AppAction::Enter].is_clicked,
                    cancel: input[AppAction::Cancel].is_clicked,
                    fit: input[AppAction::CharacterVariant].is_clicked,
                    info: input[AppAction::CharacterState].is_clicked,
                    left: input[AppAction::DpadLeft].is_held,
                    right: input[AppAction::DpadRight].is_held,
                    up: input[AppAction::DpadUp].is_held,
                    down: input[AppAction::DpadDown].is_held,
                    axes: self.axes.to_array(),
                },
            );
            if let Err(error) = process_viewer_events(v, &self.assets, &self.scenario) {
                tracing::warn!(%error,"Ger CG viewer request failed; retaining gallery");
                self.viewer = None;
                self.model.set_viewer_owned(false);
                self.cancel_input();
                return None;
            }
            for cue in v.model.take_sounds() {
                self.sounds.play(&self.assets, cue, Volume(1.));
            }
            if v.capture_pending.is_some() {
                v.publication_delta = delta;
            } else {
                v.model.update(delta);
                if let Some(c) = &mut v.capture {
                    c.clock.elapsed = v.model.capture_elapsed;
                }
            }
        }
        self.model.update(
            delta,
            enabled && self.viewer.is_none(),
            captured,
            Input {
                confirm: input[AppAction::Enter].is_clicked,
                cancel: input[AppAction::Cancel].is_clicked,
                left: input[AppAction::Left].is_clicked_or_repeated,
                right: input[AppAction::Right].is_clicked_or_repeated,
                up: input[AppAction::Up].is_clicked_or_repeated,
                down: input[AppAction::AnyDown].is_clicked_or_repeated,
                previous: input[AppAction::PageUp].is_clicked_or_repeated,
                next: input[AppAction::PageDown].is_clicked_or_repeated,
            },
        );
        let mut result = None;
        for action in self.model.take_actions() {
            match action {
                Action::OpenViewer(index) => {
                    match Viewer::new(self.model.entries[index].clone()).map_err(anyhow::Error::msg)
                    {
                        Ok(model) => {
                            let mut v = ViewerHost {
                                model,
                                picture: None,
                                job: None,
                                prepared: None,
                                capture: None,
                                capture_pending: None,
                                publication_delta: 0.,
                            };
                            match process_viewer_events(&mut v, &self.assets, &self.scenario) {
                                Ok(()) => {
                                    self.viewer = Some(v);
                                    self.model.set_viewer_owned(true);
                                    self.cancel_input();
                                }
                                Err(error) => {
                                    tracing::warn!(%error,"Ger CG viewer construction failed")
                                }
                            }
                        }
                        Err(error) => tracing::warn!(%error,"Ger CG viewer is unavailable"),
                    }
                }
                Action::ReturnTitle { requested_id } => {
                    result = Some(CgGalleryAction::ReturnTitle { requested_id })
                }
                Action::StopBgm(ticks) => {
                    self.bgm.lock().stop(Tween::linear(Ticks::from_u32(ticks)))
                }
                Action::PlayBgm { id, fade } => {
                    if let Some(info) = self.scenario.bgm_info.get(id as usize) {
                        self.bgm.lock().request(
                            self.assets.clone(),
                            format!("/bgm/{}.nxa", info.name.as_str()),
                            BgmId::from_number(id),
                            true,
                            Volume(1.),
                            Tween::linear(Ticks::from_u32(fade)),
                        );
                    }
                }
                // Ger e29a0 is the already reconstructed mutex-only Switch service.
                Action::Achievement(id) => crate::adv::gerokasu2::gallery::trophy(id as i32),
            }
        }
        for cue in self.model.take_sounds() {
            self.sounds.play(&self.assets, cue, Volume(1.));
        }
        if let Some(c) = &mut self.previous {
            c.clock.elapsed = self.model.capture_elapsed;
            if !c.clock.busy() {
                self.previous = None;
            }
        }
        result
    }
    pub fn prepare(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        let Some(closing) = self.viewer.as_ref().and_then(|v| v.capture_pending) else {
            return Ok(());
        };
        let mut snapshot = ctx.new_render_texture("Ger CG viewer composed capture".into());
        {
            let mut pass = ctx.begin_pass(
                snapshot.as_texture_target(),
                Some(ctx.depth_stencil),
                "Ger CG viewer capture",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
            self.render(&mut pass)?;
        }
        let v = self.viewer.as_mut().unwrap();
        let mut clock = OverlayClock::captured(true);
        clock.clear(30);
        v.capture = Some(Overlay {
            clock,
            snapshot,
            mask: None,
        });
        if closing {
            v.picture = None;
            v.model.publish_close();
        } else {
            let picture = v
                .prepared
                .take()
                .context("CG publication has no prepared Picture")?;
            ensure!(
                picture.effective_width > 0 && picture.effective_height > 0,
                "CG picture has zero effective extent"
            );
            v.model.publish([
                picture.effective_width as f32,
                picture.effective_height as f32,
            ]);
            v.picture = Some(picture);
        }
        v.model.update(v.publication_delta);
        v.capture.as_mut().unwrap().clock.elapsed = v.model.capture_elapsed;
        v.capture_pending = None;
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        view::render_grid(&self.model, &self.atlas, pass);
        if let Some(v) = &self.viewer {
            view::render_viewer(
                &v.model,
                v.picture.as_deref(),
                v.capture.as_ref(),
                &self.atlas,
                pass,
            );
        }
        if let Some(c) = &self.previous {
            c.render(pass);
        }
        Ok(())
    }
}
fn process_viewer_events(
    v: &mut ViewerHost,
    assets: &Arc<AssetServer>,
    scenario: &Gerokasu2Scenario,
) -> Result<()> {
    for event in v.model.take_events() {
        match event {
            viewer::Event::Load(id) => {
                let info = scenario
                    .picture_info
                    .get(id as usize)
                    .context("CG picture metadata is missing")?;
                let path = format!("/picture/{}.pic", info.name.as_str());
                let assets = assets.clone();
                v.job = Some(PictureJob {
                    task: Some(shin_tasks::async_io::spawn_loader(async move {
                        assets.load::<Picture, _>(path).await
                    })),
                });
            }
            viewer::Event::CaptureForPublication => v.capture_pending = Some(false),
            viewer::Event::CaptureForClose => v.capture_pending = Some(true),
        }
    }
    Ok(())
}
struct Target<'a> {
    grid: &'a mut Controller,
    viewer: Option<&'a mut Viewer>,
}
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        if !p.is_finite() || p.x < 0. || p.x >= 1920. || p.y < 0. || p.y >= 1080. {
            return false;
        }
        if let Some(v) = &mut self.viewer {
            v.probe();
            true
        } else {
            self.grid.probe(p.x, p.y)
        }
    }
    fn reset(&mut self) {
        if self.viewer.is_none() {
            self.grid.reset();
        }
    }
    fn press(&mut self, _: Vec2) {
        if self.viewer.is_none() {
            self.grid.press();
        }
    }
    fn release(&mut self, _: Vec2) {
        if let Some(v) = &mut self.viewer {
            v.release();
        } else {
            self.grid.release();
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, old: Vec2, new: Vec2) {
        if let Some(v) = &mut self.viewer {
            v.drag(old.to_array(), new.to_array());
        } else {
            self.grid.drag();
        }
    }
    fn gesture(&mut self, v: Vec2) {
        if let Some(model) = &mut self.viewer {
            model.gesture(v.to_array());
        } else {
            self.grid.gesture(v.x, v.y);
        }
    }
    fn mouse_gesture(&mut self, d: Vec2, v: Vec2) {
        self.gesture(crate::adv::input_area::desktop_gesture_vector(d, v));
    }
    fn cancel(&mut self) {
        if self.viewer.is_none() {
            self.grid.cancel();
        }
    }
}
