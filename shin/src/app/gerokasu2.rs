//! Gerokasu2 window adapter; the legacy App keeps its own startup policy.
use super::AppAction;
use crate::{
    adv::{
        gerokasu2::{
            message_commands::MessageCommandContext,
            play_scene::{Gerokasu2PlayScene, SceneEvent},
            runner::{FramePolicy, InitialMessage, RunnerStatus},
        },
        input_area::{PointerKind, PointerSampler},
    },
    asset::system::{AssetLoadContext, AssetServer, cache::AssetCache, locate_assets_for_game},
    cli::{Cli, Game},
    layer::render_params::TransformParams,
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use enum_map::EnumMap;
use glam::Vec2;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario, primitives::update::FrameId, time::Ticks,
};
use shin_input::{ActionState, RawInputState};
use shin_render::{PassKind, render_pass::RenderPass};
use shin_window::{AppContext, RenderContext, ShinApp};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod saves;

pub(crate) fn validate_options(cli: &Cli) -> Result<()> {
    ensure!(
        cli.game == Game::Gerokasu2,
        "Expected Gerokasu2 game selection"
    );
    ensure!(
        cli.unsafe_entry_point.is_none() && cli.fast_forward_to.is_none(),
        "Gerokasu2 debug-entry reconstruction is not connected yet"
    );
    Ok(())
}
fn command_context() -> MessageCommandContext {
    MessageCommandContext {
        was_read: false,
        event_mode: -1,
        skip: false,
        message_speed: 50,
    }
}

/// Exact viewport rectangle;points in letterbox bars never reach game input.
#[derive(Clone, Copy)]
pub(crate) struct Viewport {
    pub offset: Vec2,
    pub size: Vec2,
}
impl Viewport {
    pub fn map(self, point: Vec2) -> Option<Vec2> {
        if !point.is_finite()
            || !self.offset.is_finite()
            || !self.size.is_finite()
            || self.size.min_element() <= 0.0
        {
            return None;
        }
        let local = point - self.offset;
        (local.cmpge(Vec2::ZERO).all() && local.cmplt(self.size).all())
            .then(|| local * Vec2::new(1920.0, 1080.0) / self.size)
    }
}
#[derive(Default)]
enum PointerTarget {
    #[default]
    None,
    Message,
    Card,
    Selection,
}
pub(crate) struct GerHost {
    pub scene: Gerokasu2PlayScene,
    pointer: PointerSampler,
    pointer_target: PointerTarget,
    buttons_armed: bool,
    pub suspended: bool,
    pub status: RunnerStatus,
}
impl GerHost {
    pub async fn restore_saved(
        &mut self,
        scenario: Arc<Gerokasu2Scenario>,
        data: &shin_core::format::save::gerokasu2::SaveData,
        slot: u8,
    ) -> Result<()> {
        let game = saves::slot(data, slot)?;
        self.scene.import_save_globals(data);
        let mut replay = crate::adv::gerokasu2::replay::ReplaySession::from_progress(
            scenario,
            game.progress.clone(),
        )?;
        replay
            .save
            .variables
            .replace_committed(data.persist_data.clone());
        if let Some(auto) = data.auto_save_slot.as_ref() {
            replay.save.autosave = auto.clone().into_memory();
        }
        let seen = self.scene.playback().seen_messages.clone();
        let mut replay = shin_tasks::compute::spawn(move || -> Result<_> {
            loop {
                let status = replay.pump(&seen, 4096, 100000, || false)?;
                if status == crate::adv::gerokasu2::replay::ReplayStatus::Ready {
                    break;
                }
                ensure!(
                    status == crate::adv::gerokasu2::replay::ReplayStatus::CommandBudget,
                    "Ger save reconstruction did not reach publication"
                );
            }
            Ok(replay)
        })
        .await?;
        self.scene.reconstruct_replay_for_load(&mut replay).await?;
        self.cancel_input();
        self.status = RunnerStatus::Waiting;
        Ok(())
    }
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<AudioManager>,
        seed: u32,
    ) -> Result<Self> {
        let scene = Gerokasu2PlayScene::load(
            assets,
            scenario,
            audio,
            0,
            seed,
            InitialMessage {
                style: 0,
                asynchronous: false,
                face: -1,
            },
            Default::default(),
            command_context(),
        )
        .await?;
        Ok(Self {
            scene,
            pointer: Default::default(),
            pointer_target: PointerTarget::None,
            buttons_armed: true,
            suspended: false,
            status: RunnerStatus::Waiting,
        })
    }
    pub fn cancel_input(&mut self) {
        self.pointer.cancel_contact(true);
        self.pointer_target = PointerTarget::None;
        self.buttons_armed = false;
        self.scene.playback_mut().held_skip = false;
        self.scene.card_pointer_cancel();
        self.scene.selection_pointer_cancel();
    }
    pub fn update(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        point: Vec2,
        viewport: Viewport,
        seconds: f64,
    ) -> Result<RunnerStatus> {
        if self.scene.reconstruction_phase().is_some() {
            self.cancel_input();
            self.status = self.scene.step(
                frame,
                delta,
                FramePolicy {
                    paused: self.suspended,
                    scene_changing: false,
                    animations_allowed: false,
                    autoplay: false,
                    fast_skip: false,
                },
                command_context(),
            )?;
            return Ok(self.status);
        }
        let mapped = viewport.map(point);
        self.pointer.sample(
            mapped.unwrap_or(Vec2::splat(-1.0)),
            input[AppAction::PointerClick].is_held,
            seconds,
        );
        if [
            AppAction::Enter,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Left,
            AppAction::Right,
            AppAction::ToggleAuto,
            AppAction::ToggleSkip,
            AppAction::HoldSkip,
            AppAction::Cancel,
        ]
        .into_iter()
        .all(|action| !input[action].is_held)
        {
            self.buttons_armed = true;
        }
        self.scene.playback_mut().held_skip =
            !self.suspended && self.buttons_armed && input[AppAction::HoldSkip].is_held;
        if !self.suspended {
            if self.buttons_armed {
                self.scene.playback_mut().input(
                    input[AppAction::ToggleAuto].is_clicked,
                    input[AppAction::ToggleSkip].is_clicked,
                    input[AppAction::Enter].is_clicked || input[AppAction::Cancel].is_clicked,
                );
            }
            let choosing = self.scene.selection().is_some();
            let choice_ready = self.scene.selection_ready();
            let ready = self.scene.card_ready();
            if choosing && matches!(self.pointer_target, PointerTarget::Selection) {
                if let Some(p) = mapped {
                    self.scene
                        .selection_pointer(p, input[AppAction::PointerClick].is_held);
                } else {
                    self.scene.selection_pointer_cancel();
                    self.pointer_target = PointerTarget::None;
                }
            }
            if ready {
                if let Some(p) = mapped {
                    self.scene.card_pointer_move(p);
                } else {
                    self.scene.card_pointer_cancel();
                }
            }
            for event in self.pointer.frame().events {
                match event.kind {
                    PointerKind::Down if choosing => {
                        if choice_ready && let Some(p) = mapped {
                            self.scene.selection_pointer(p, true);
                            self.pointer_target = PointerTarget::Selection;
                        }
                    }
                    PointerKind::Down if ready && mapped.is_some() => {
                        self.pointer_target = if self.scene.card_pointer_press() {
                            PointerTarget::Card
                        } else {
                            PointerTarget::None
                        };
                    }
                    PointerKind::Down if !ready && mapped.is_some() => {
                        self.pointer_target = PointerTarget::Message;
                    }
                    PointerKind::Up => match std::mem::take(&mut self.pointer_target) {
                        PointerTarget::Selection => {
                            if let Some(p) = mapped {
                                self.scene.selection_pointer(p, false);
                            } else {
                                self.scene.selection_pointer_cancel();
                            }
                        }
                        PointerTarget::Card if ready && mapped.is_some() => {
                            self.scene.card_pointer_release();
                        }
                        PointerTarget::Card => {
                            self.scene.card_pointer_cancel();
                        }
                        PointerTarget::Message if mapped.is_some() && !ready && !choosing => {
                            self.scene.playback_mut().input(false, false, true);
                            self.scene.runner_mut().interrupt_wait();
                            self.scene.click_message(false)?;
                        }
                        _ => {}
                    },
                    PointerKind::Cancel => {
                        self.scene.card_pointer_cancel();
                        self.scene.selection_pointer_cancel();
                        self.pointer_target = PointerTarget::None;
                    }
                    _ => {}
                }
            }
            if self.buttons_armed {
                if choosing {
                    self.scene.selection_input(
                        input[AppAction::Up].is_clicked_or_repeated,
                        input[AppAction::AnyDown].is_clicked_or_repeated,
                        input[AppAction::Left].is_clicked_or_repeated,
                        input[AppAction::Right].is_clicked_or_repeated,
                        input[AppAction::Up].is_held || input[AppAction::AnyDown].is_held,
                        input[AppAction::Enter].is_clicked,
                    );
                } else if ready {
                    if input[AppAction::Left].is_clicked_or_repeated {
                        self.scene.navigate_card(-1);
                    } else if input[AppAction::Right].is_clicked_or_repeated {
                        self.scene.navigate_card(1);
                    }
                    if input[AppAction::Enter].is_clicked {
                        self.scene.confirm_card();
                    }
                } else if input[AppAction::Enter].is_clicked {
                    self.scene.runner_mut().interrupt_wait();
                    self.scene.click_message(false)?;
                }
            }
        }
        self.status = self.scene.step(
            frame,
            delta,
            FramePolicy {
                paused: self.suspended,
                scene_changing: false,
                animations_allowed: true,
                autoplay: false,
                fast_skip: false,
            },
            command_context(),
        )?;
        if self.status == RunnerStatus::External {
            bail!(
                "Gerokasu2 command owner is not connected: {:?}",
                self.scene.runner().pending_command()
            );
        }
        Ok(self.status)
    }
    pub fn pre_render(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        self.scene.pre_render(ctx, &TransformParams::default())
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        for kind in [PassKind::Opaque, PassKind::Transparent] {
            self.scene
                .render(pass, &TransformParams::default(), 0, kind)?;
        }
        Ok(())
    }
}
pub struct Gerokasu2App {
    host: GerHost,
    audio: Arc<AudioManager>,
    frame: FrameId,
    point: Vec2,
    error: Option<String>,
    save_store: Option<saves::Store>,
    play_clock: crate::adv::play_clock::PlayClock,
}
impl ShinApp for Gerokasu2App {
    type Parameters = Cli;
    type EventType = ();
    type ActionType = AppAction;
    const FULLSCREEN_SHORTCUT: Option<winit::keyboard::KeyCode> =
        Some(winit::keyboard::KeyCode::KeyF);
    fn init(ctx: AppContext<Self>, cli: Cli) -> Result<Self> {
        validate_options(&cli)?;
        let imported = cli
            .load_save
            .as_ref()
            .map(|path| saves::read(path))
            .transpose()?;
        let save_store = cli
            .save_file
            .clone()
            .map(|path| saves::Store::load(path, imported.clone()))
            .transpose()?;
        let io = locate_assets_for_game(cli.game, cli.assets_dir.as_deref())?;
        let data = io
            .read_file("/main.snr")?
            .read_all_sync()
            .context("Reading Gerokasu2 scenario")?;
        let scenario = Arc::new(Gerokasu2Scenario::new(data.into())?);
        let assets = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Gerokasu2,
                wgpu_device: ctx.wgpu.device.clone(),
                wgpu_queue: ctx.wgpu.queue.clone(),
                bustup_cache: AssetCache::new(),
                higurashi_bustup_cache: Default::default(),


                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        #[cfg(not(test))]
        let audio = Arc::new(if cli.loopback_audio {
            AudioManager::new_for_tests()
        } else {
            AudioManager::new()
        });
        #[cfg(test)]
        let audio = Arc::new(AudioManager::new_for_tests());
        let seed = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u32;
        let mut host = shin_tasks::block_on(GerHost::load(
            assets.clone(),
            scenario.clone(),
            audio.clone(),
            seed,
        ))?;
        if let Some(data) = imported.as_ref() {
            shin_tasks::block_on(host.restore_saved(scenario, data, cli.save_slot))?;
        } else if let Some(store) = save_store.as_ref() {
            host.scene.import_save_globals(&store.data);
        }
        ctx.winit.window.set_title("Gerokasu2");
        let play_clock = crate::adv::play_clock::PlayClock::from_saved_seconds(
            imported
                .as_ref()
                .map(|data| data.play_seconds)
                .or_else(|| save_store.as_ref().map(|store| store.data.play_seconds))
                .unwrap_or(0),
        );
        Ok(Self {
            host,
            audio,
            frame: FrameId::default(),
            point: Vec2::ZERO,
            error: None,
            save_store,
            play_clock,
        })
    }
    fn custom_event(&mut self, _: AppContext<Self>, _: ()) {}
    fn focus_lost(&mut self) {
        self.host.cancel_input();
    }
    fn set_suspended(&mut self, value: bool) {
        self.host.suspended = value;
        if value {
            self.host.cancel_input();
        }
        self.audio.set_suspended(value);
    }
    fn raw_input(&mut self, raw: &RawInputState) {
        self.point = raw.mouse.position;
    }
    fn update(
        &mut self,
        ctx: AppContext<Self>,
        input: EnumMap<AppAction, ActionState>,
        elapsed: Duration,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.error.is_some() {
            return;
        }
        self.frame.advance();
        self.play_clock
            .advance(Ticks::from_duration(elapsed).as_f32());
        if let Some(store) = self.save_store.as_mut() {
            store.data.play_seconds = self.play_clock.saved_seconds();
        }
        let viewport = Self::map_canvas_size(ctx.winit.window.inner_size());
        let map = Viewport {
            offset: Vec2::new(
                viewport.canvas_offset.x as f32,
                viewport.canvas_offset.y as f32,
            ),
            size: Vec2::new(
                viewport.canvas_size.width as f32,
                viewport.canvas_size.height as f32,
            ),
        };
        let result = self.host.update(
            self.frame,
            Ticks::from_duration(elapsed),
            &input,
            self.point,
            map,
            elapsed.as_secs_f64(),
        );
        match result {
            Ok(RunnerStatus::Exited) => {
                if let Some(store) = self.save_store.as_mut() {
                    self.host.scene.export_save_globals(&mut store.data);
                    if let Some(auto) = shin_core::format::save::gerokasu2::GameData::from_memory(
                        &self.host.scene.save_session().autosave,
                    ) {
                        store.data.auto_save_slot = Some(auto);
                    }
                    if let Err(error) = store.finish() {
                        tracing::error!(%error,"Ger save publication failed on exit");
                    }
                }
                ctx.event_loop.exit();
                return;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(error=%e,"Gerokasu2 scene update failed");
                self.error = Some(format!("{e:#}"));
                return;
            }
        }
        if let Some(store) = self.save_store.as_mut() {
            self.host.scene.export_save_globals(&mut store.data);
            store.mark_dirty();
        }
        for event in self.host.scene.take_events() {
            match event {
                SceneEvent::SceneTitle(title) => {
                    ctx.winit.window.set_title(&format!("Gerokasu2 — {title}"))
                }
                SceneEvent::AutosaveCaptured(memory) => {
                    if let Some(store) = self.save_store.as_mut() {
                        store.data.auto_save_slot =
                            shin_core::format::save::gerokasu2::GameData::from_memory(&memory);
                        self.host.scene.export_save_globals(&mut store.data);
                        if let Err(error) = store.request_flush() {
                            tracing::error!(%error,"Ger autosave publication failed");
                            self.error = Some(format!("{error:#}"));
                            return;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(store) = self.save_store.as_mut() {
            if let Err(error) = store.poll_flush() {
                tracing::error!(%error,"Ger save worker failed");
                self.error = Some(format!("{error:#}"));
                return;
            }
        }
        let mut pre = PreRenderContext {
            device: &ctx.wgpu.device,
            queue: &ctx.wgpu.queue,
            breakup_texture: None,
            resize_source: &ctx.winit.resize_source,
            sampler_store: &ctx.render.sampler_store,
            depth_stencil: ctx.render.canvas_depth_stencil_buffer.get_target_view(),
            pipeline_storage: &mut ctx.render.pipelines,
            dynamic_buffer: &mut ctx.render.dynamic_buffer,
            encoder,
        };
        if let Err(e) = self.host.pre_render(&mut pre) {
            tracing::error!(error=%e,"Gerokasu2 preparation failed");
            self.error = Some(format!("{e:#}"));
        }
    }
    fn render(&mut self, _: RenderContext, pass: &mut RenderPass) {
        if let Err(e) = self.host.render(pass) {
            tracing::error!(error=%e,"Gerokasu2 rendering failed");
            self.error = Some(format!("{e:#}"));
        }
    }
}
#[cfg(test)]
mod tests;
