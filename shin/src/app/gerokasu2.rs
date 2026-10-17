//! Gerokasu2 window adapter; the legacy App keeps its own startup policy.
use super::AppAction;
use crate::cli::Game;
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
    cli::Cli,
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
mod backlog;
mod bustup_gallery;
mod chapter_skip;
mod cg_gallery;
mod config;
mod gallery_mode;
mod in_game;
mod load_menu;
mod manual;
mod menu_audio;
mod music_gallery;
mod notifications;
mod root_touch;
mod saves;
mod system_menu;
mod title;
mod title_child;
mod first_boot;
mod system_voice;
mod title_demo;
mod transitions;
mod write_wait;
mod debug_input;

/// Native menu modes share one global music player, including loading and
/// stop tails. Dropping a mode must not stop its music before the next owner.
pub(super) type MenuBgm = Arc<parking_lot::Mutex<menu_audio::MenuMusic>>;

pub(crate) fn validate_options(cli: &Cli) -> Result<()> {
    ensure!(
        cli.game.is_gerokasu_family(),
        "Expected Gerokasu or Gerokasu2 game selection"
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
    notifications: notifications::Notifications,
    exit_notified: bool,
    /// Native HiddenWindow (1BAE0) child: holds conceal 14210 until X/Y/A/B
    /// or a tap (1BBF0/1BC90).
    hidden_window: bool,
    /// Plus/Minus Question child (1264C/129E4).
    chapter: Option<chapter_skip::ChapterQuestion>,
    /// Indicators, navi toolbar and event hint (`/adv.txa`).
    root: crate::adv::gerokasu2::root_ui::RootUi,
    root_atlas: Arc<crate::adv::characters::view::CharacterTextures>,
    touch: root_touch::Touch,
    /// Contact-requested openings, dispatched by the app's menu gates.
    pub request: Option<root_touch::Request>,
}
impl GerHost {
    pub async fn restore_saved(
        &mut self,
        scenario: Arc<Gerokasu2Scenario>,
        data: &shin_core::format::save::gerokasu2::SaveData,
        slot: u8,
    ) -> Result<()> {
        self.restore_saved_cancellable(
            scenario,
            data,
            slot,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await
    }
    async fn restore_saved_cancellable(
        &mut self,
        scenario: Arc<Gerokasu2Scenario>,
        data: &shin_core::format::save::gerokasu2::SaveData,
        slot: u8,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
        let game = saves::slot(data, slot)?;
        self.restore_memory_cancellable(scenario, data, game, cancelled)
            .await
    }
    /// Fresh ADV mode0 also accepts a memory-only backlog payload. Preserve
    /// its distinct save_position/replay_target rather than passing a disk slot.
    async fn restore_memory_cancellable(
        &mut self,
        scenario: Arc<Gerokasu2Scenario>,
        data: &shin_core::format::save::gerokasu2::SaveData,
        game: &shin_core::format::save::gerokasu2::GameData,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
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
        let worker_cancelled = cancelled.clone();
        let mut replay = shin_tasks::compute::spawn(move || -> Result<_> {
            replay.pump_to_ready(&seen, || {
                worker_cancelled.load(std::sync::atomic::Ordering::Acquire)
            })?;
            Ok(replay)
        })
        .await?;
        self.scene
            .reconstruct_replay_for_load_cancellable(&mut replay, || {
                cancelled.load(std::sync::atomic::Ordering::Acquire)
            })
            .await?;
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
        let root_atlas = Self::load_root_ui(scene.assets()).await?;
        Ok(Self {
            scene,
            pointer: Default::default(),
            pointer_target: PointerTarget::None,
            buttons_armed: true,
            suspended: false,
            status: RunnerStatus::Waiting,
            notifications: notifications::Notifications::new(),
            exit_notified: false,
            hidden_window: false,
            chapter: None,
            root: Default::default(),
            root_atlas,
            touch: Default::default(),
            request: None,
        })
    }
    /// Parent-owned notification clocks advance every frame, including while
    /// modal children pause the scene. Script EXIT phase0 closes both first.
    pub fn update_notifications(&mut self, delta: Ticks) {
        if self.scene.is_exiting() && !self.exit_notified {
            self.exit_notified = true;
            self.notifications.close();
        }
        self.notifications
            .update(delta.as_f32(), self.scene.skip_active());
    }
    pub fn show_scene_title(&mut self, title: &str) {
        let assets = self.scene.assets().clone();
        if let Err(error) = self.notifications.show_scene_title(&assets, title) {
            tracing::warn!(%error, "Ger scene-title notification failed");
        }
    }
    pub fn show_bgm_title(&mut self, id: i32) {
        let assets = self.scene.assets().clone();
        let name = usize::try_from(id)
            .ok()
            .and_then(|id| self.scene.scenario().bgm_info.get(id))
            .map(|info| info.display_name.as_str().to_owned())
            .unwrap_or_default();
        if let Err(error) = self.notifications.show_bgm_title(&assets, &name) {
            tracing::warn!(%error, "Ger BGM-title notification failed");
        }
    }
    pub fn render_notifications(
        &self,
        pass: &mut RenderPass,
        settings: &shin_core::format::save::Settings,
    ) -> Result<()> {
        // Native reads settings+E/+F (save+BE/+BF): Ger Config's scene-title
        // and music-title rows, stored in the historical v14/v15 fields.
        self.notifications
            .render(pass, settings.v14_showroutenavi, settings.v15)
    }
    pub fn cancel_input(&mut self) {
        self.pointer.cancel_contact(true);
        self.cancel_touch();
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
        self.update_root_availability();
        self.poll_pending_backlog();
        if self.scene.reconstruction_phase().is_some() || self.scene.is_exiting() {
            let animations_allowed = self.scene.reconstruction_phase().is_none();
            self.cancel_input();
            self.status = self.scene.step(
                frame,
                delta,
                FramePolicy {
                    paused: self.suspended,
                    scene_changing: false,
                    animations_allowed,
                    autoplay: false,
                    fast_skip: false,
                },
                command_context(),
            )?;
            if !self.suspended {
                self.root.advance(delta.as_f32());
            }
            return Ok(self.status);
        }
        let mapped = viewport.map(point);
        if self.chapter.is_none() {
            self.pointer.sample(
                mapped.unwrap_or(Vec2::splat(-1.0)),
                input[AppAction::PointerClick].is_held,
                seconds,
            );
        }
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
            AppAction::SystemMenu,
            AppAction::Backlog,
        ]
        .into_iter()
        .all(|action| !input[action].is_held)
        {
            self.buttons_armed = true;
        }
        self.scene.playback_mut().held_skip = !self.suspended
            && self.chapter.is_none()
            && self.buttons_armed
            && input[AppAction::HoldSkip].is_held;
        if self.chapter.is_some() {
            // The open child returns false to 115B8: no ADV button input.
            if !self.suspended {
                self.update_chapter_question(delta, input, mapped, seconds)?;
            }
        } else if !self.suspended {
            let mut consumed = false;
            if self.buttons_armed {
                // 1163C..11678: L/R toggles, then B/X clear an active mode
                // word (Y is handled with menu eligibility). A press
                // that changed the word plays its cue and ends button input.
                let cue = self.scene.playback_mut().input(
                    input[AppAction::ToggleAuto].is_clicked,
                    input[AppAction::ToggleSkip].is_clicked,
                    input[AppAction::Cancel].is_clicked || input[AppAction::Backlog].is_clicked,
                );
                if let Some(cue) = cue {
                    self.scene.play_system_cue(cue);
                    consumed = true;
                } else {
                    consumed = self.chapter_input(input);
                }
                // 1175C..11790: during an event A shows the skip hint and B
                // (bit29) skips the event; neither reaches the message.
                if !consumed
                    && self.scene.playback().system_controls
                    && self.scene.save_session().timeline.event >= 0
                {
                    if input[AppAction::Enter].is_clicked {
                        self.root.show_hint(false);
                        consumed = true;
                    } else if input[AppAction::Cancel].is_clicked {
                        self.event_skip()?;
                        consumed = true;
                    } else if self.scene.selection().is_none() && !self.scene.card_ready() {
                        // 1177C returns before 117D8: Up/Down do not reach
                        // the message during an event.
                        consumed = true;
                    }
                }
            }
            if self.backlog_pending() {
                // The blocking child holds ADV buttons until its flash ends.
                consumed = true;
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
            if self.hidden_window {
                self.cancel_touch();
            } else {
                self.update_touch(input[AppAction::PointerClick].is_held)?;
            }
            let toolbar = self.touch.toolbar_contact;
            for event in self.pointer.frame().events {
                match event.kind {
                    PointerKind::Down if toolbar => {}
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
            if self.buttons_armed && !consumed {
                if choosing {
                    self.scene.selection_input(
                        input[AppAction::Up].is_clicked_or_repeated,
                        input[AppAction::AnyDown].is_clicked_or_repeated,
                        input[AppAction::Left].is_clicked_or_repeated,
                        input[AppAction::Right].is_clicked_or_repeated,
                        input[AppAction::Up].is_held || input[AppAction::AnyDown].is_held,
                        input[AppAction::Enter].is_clicked,
                    );
                } else if ready && debug_input::card_autoplay() {
                    use crate::adv::gerokasu2::card_select::gerokasu::AutoAction;
                    static STEP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                    if STEP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 12 == 0 {
                        match self.scene.card_autoplay_action() {
                            Some(AutoAction::Navigate(direction)) => {
                                self.scene.navigate_card(direction);
                            }
                            Some(AutoAction::Confirm) => {
                                self.scene.confirm_card();
                            }
                            Some(AutoAction::Cancel) => {
                                self.scene.cancel_card();
                            }
                            None => {}
                        }
                    }
                } else if ready {
                    if input[AppAction::Left].is_clicked_or_repeated {
                        self.scene.navigate_card(-1);
                    } else if input[AppAction::Right].is_clicked_or_repeated {
                        self.scene.navigate_card(1);
                    }
                    if input[AppAction::Enter].is_clicked {
                        self.scene.confirm_card();
                    } else if input[AppAction::Cancel].is_clicked {
                        // Gerokasu 49A00: B undoes or puts the card back.
                        self.scene.cancel_card();
                    }
                } else if input[AppAction::Up].is_clicked_or_repeated {
                    // 117D8: the message layer takes repeated Up (bit24)
                    // before the pressed Down/A aliases (bits 25/28).
                    self.message_back()?;
                } else if input[AppAction::Enter].is_clicked
                    || input[AppAction::AnyDown].is_clicked
                {
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
        if !self.suspended {
            self.root.advance(delta.as_f32());
        }
        if self.status == RunnerStatus::External {
            bail!(
                "Gerokasu2 command owner is not connected: {:?}",
                self.scene.runner().pending_command()
            );
        }
        Ok(self.status)
    }
    pub fn pre_render(&mut self, ctx: &mut PreRenderContext) -> Result<()> {
        self.scene.pre_render(ctx, &TransformParams::default())?;
        self.prepare_chapter_question(ctx)
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
    backlog: Option<backlog::Slot>,
    #[cfg(test)]
    backlog_options: backlog::Options,
    system: Option<system_menu::Host>,
    loading_system: Option<shin_tasks::AsyncTask<Result<system_menu::Host>>>,
    modal: crate::adv::gerokasu2::modal::Modal,
    backdrop: Option<crate::adv::gerokasu2::modal::backdrop::Backdrop>,
    frozen_manual: Option<shin_core::format::save::gerokasu2::GameData>,
    system_title_exit: bool,
    host: Option<GerHost>,
    title: Option<title::TitleHost>,
    first_boot: Option<first_boot::FirstBoot>,
    title_demo: Option<title_demo::TitleDemo>,
    gallery: Option<gallery_mode::Host>,
    loading_gallery: Option<shin_tasks::AsyncTask<Result<gallery_mode::Host>>>,
    loading_title: Option<shin_tasks::AsyncTask<Result<title::TitleHost>>>,
    title_child: Option<title_child::TitleChild>,
    loading_child: Option<shin_tasks::AsyncTask<Result<title_child::TitleChild>>>,
    write_wait: Option<write_wait::Host>,
    waiting_for_child_save: bool,
    waiting_for_exit_save: bool,
    exit_save_ready: bool,
    adv_frame: Option<shin_render::render_texture::RenderTexture>,
    loading_game: Option<shin_tasks::AsyncTask<Result<GerHost>>>,
    loading_cancelled: Arc<std::sync::atomic::AtomicBool>,
    assets: Arc<AssetServer>,
    scenario: Arc<Gerokasu2Scenario>,
    suspended: bool,
    audio: Arc<AudioManager>,
    menu_bgm: MenuBgm,
    frame: FrameId,
    point: Vec2,
    error: Option<String>,
    save_store: Option<saves::Store>,
    play_clock: crate::adv::play_clock::PlayClock,
    /// Global TouchEffect (`systex:drag` stars and tap ripples), enabled by
    /// Config's touch-effect row (settings v17, as in Umineko).
    touch_effect: super::touch_effect::TouchEffect,
    touch_background: Option<shin_render::render_texture::RenderTexture>,
    system_textures: Option<Arc<crate::adv::characters::view::CharacterTextures>>,
}
impl Drop for Gerokasu2App {
    fn drop(&mut self) {
        if let Some(task) = self.loading_system.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        self.loading_cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(task) = self.loading_game.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        if let Some(task) = self.loading_child.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        if let Some(task) = self.loading_gallery.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
        if let Some(task) = self.loading_title.take() {
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        }
    }
}
impl Gerokasu2App {
    fn commit_menu_unlocks(&mut self) {
        let mut unlocks = self.menu_bgm.lock().take_unlocks();
        if let Some(gallery) = self.gallery.as_mut() {
            unlocks.extend(gallery.take_unlocks());
        }
        if let Some(store) = self.save_store.as_mut() {
            if menu_audio::merge_unlocks(&mut store.data.save_vectors.unlocked_bgms, unlocks) {
                store.mark_dirty();
            }
        }
    }
    fn capture_menu_source(
        &self,
        ctx: &mut PreRenderContext,
    ) -> Result<shin_render::render_texture::RenderTexture> {
        let mut snapshot = ctx.new_render_texture("Ger outer-mode source".into());
        let depth = ctx.depth_stencil;
        {
            let mut pass = ctx.begin_pass(
                snapshot.as_texture_target(),
                Some(depth),
                "Ger outer-mode capture",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
            if let Some(title) = self.title.as_ref() {
                title.render(&mut pass);
            } else if let Some(gallery) = self.gallery.as_ref() {
                gallery.render(&mut pass)?;
            } else if let Some(source) = self.adv_frame.as_ref() {
                crate::adv::scene_load_fade::render_snapshot(source, &mut pass);
            } else {
                bail!("Ger mode factory has no menu source");
            }
        }
        Ok(snapshot)
    }
    fn start_gallery(
        &mut self,
        kind: gallery_mode::Kind,
        snapshot: shin_render::render_texture::RenderTexture,
    ) {
        let (assets, scenario, audio, data, bgm) = (
            self.assets.clone(),
            self.scenario.clone(),
            self.audio.clone(),
            self.save_store.as_ref().unwrap().data.clone(),
            self.menu_bgm.clone(),
        );
        self.loading_gallery = Some(shin_tasks::async_io::spawn_loader(
            gallery_mode::Host::load(kind, assets, scenario, audio, data, bgm, snapshot),
        ));
    }
    fn start_gallery_title(
        &mut self,
        requested: i32,
        snapshot: shin_render::render_texture::RenderTexture,
    ) {
        let (assets, scenario, audio, data, bgm) = (
            self.assets.clone(),
            self.scenario.clone(),
            self.audio.clone(),
            self.save_store.as_ref().unwrap().data.clone(),
            self.menu_bgm.clone(),
        );
        self.loading_title = Some(shin_tasks::async_io::spawn_loader(async move {
            let mut title =
                title::TitleHost::load(assets, &scenario, audio, &data, requested, bgm).await?;
            title.set_previous(Some(snapshot));
            Ok(title)
        }));
    }
    fn spawn_boot_title(&mut self) {
        let (assets, scenario, audio, data, bgm) = (
            self.assets.clone(),
            self.scenario.clone(),
            self.audio.clone(),
            self.save_store.as_ref().unwrap().data.clone(),
            self.menu_bgm.clone(),
        );
        self.loading_title = Some(shin_tasks::async_io::spawn_loader(async move {
            title::TitleHost::load(assets, &scenario, audio, &data, -1, bgm).await
        }));
    }
    fn update_first_boot(&mut self, delta: Ticks) {
        if self.suspended || self.loading_title.is_some() {
            return;
        }
        let boot = self.first_boot.as_mut().unwrap();
        boot.update(delta);
        if !boot.take_finished() {
            return;
        }
        // 5D0FC -> DF9B0: TitleDemo once the opening was seen, else Title.
        let store = self.save_store.as_mut().unwrap();
        if title_demo::enabled(&store.data) {
            let (demo, changed) = title_demo::TitleDemo::new(
                self.assets.clone(),
                self.audio.clone(),
                &self.scenario,
                &mut store.data,
            );
            if changed {
                store.mark_dirty();
            }
            self.first_boot = None;
            self.title_demo = Some(demo);
        } else {
            self.spawn_boot_title();
        }
    }
    fn update_title_demo(
        &mut self,
        queue: &wgpu::Queue,
        input: &EnumMap<AppAction, ActionState>,
        map: Viewport,
        elapsed: Duration,
    ) {
        let demo = self.title_demo.as_mut().unwrap();
        if self.suspended || self.loading_title.is_some() {
            return;
        }
        if let Err(error) = demo.update(
            self.frame,
            Ticks::from_duration(elapsed),
            input,
            self.point,
            map,
            elapsed.as_secs_f64(),
            queue,
        ) {
            tracing::error!(%error, "Ger TitleDemo failed; publishing Title");
        }
        if demo.take_title_request() || demo.update_failed() {
            self.spawn_boot_title();
        }
    }
    fn start_game(&mut self, slot: Option<u8>) {
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        let audio = self.audio.clone();
        let data = self.save_store.as_ref().unwrap().data.clone();
        self.loading_cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled = self.loading_cancelled.clone();
        self.loading_game = Some(shin_tasks::async_io::spawn_loader(async move {
            let mut host = GerHost::load(assets, scenario.clone(), audio, fresh_seed()?).await?;
            host.scene.defer_audio_publication();
            host.scene.import_save_globals(&data);
            if let Some(slot) = slot {
                host.restore_saved_cancellable(scenario, &data, slot, cancelled)
                    .await?;
            }
            Ok(host)
        }));
    }

    fn start_title_child(&mut self, kind: title_child::Kind) {
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        let audio = self.audio.clone();
        let data = self.save_store.as_ref().unwrap().data.clone();
        self.loading_child = Some(shin_tasks::async_io::spawn_loader(
            title_child::TitleChild::load(kind, assets, scenario, audio, data),
        ));
    }

    fn recover_title_child(&mut self) {
        self.title_child = None;
        self.waiting_for_child_save = false;
        self.frozen_manual = None;
        if let Some(system) = self.system.as_mut() {
            system.recover_child();
        }
        if let Some(title) = self.title.as_mut() {
            title.model.recover_child();
            title.cancel_input();
        }
    }

    fn poll_saves(&mut self, delta: Ticks) {
        if self.suspended {
            return;
        }
        let Some(wait) = self.write_wait.as_mut() else {
            return;
        };
        let Some(success) = wait.update(self.save_store.as_mut().unwrap(), delta) else {
            return;
        };
        let purpose = wait.purpose;
        self.write_wait = None;
        match purpose {
            write_wait::Purpose::Manual => {
                if let Some(child) = self.title_child.as_mut() {
                    child.manual_write_finished(success, &self.save_store.as_ref().unwrap().data);
                }
            }
            write_wait::Purpose::Config => {
                self.waiting_for_child_save = false;
                if let Some(child) = self.title_child.as_mut() {
                    if success {
                        child.save_finished();
                    } else {
                        child.save_failed();
                    }
                    child.cancel_input();
                }
            }
            write_wait::Purpose::Exit => {
                self.waiting_for_exit_save = false;
                if success {
                    self.exit_save_ready = true;
                } else if self.system_title_exit {
                    self.system_title_exit = false;
                    if let Some(system) = self.system.as_mut() {
                        system.recover_child();
                    }
                } else {
                    self.error = Some(
                        "Ger EXIT save publication failed; see log for the desktop IO error".into(),
                    );
                }
            }
        }
    }
    fn begin_write_wait(&mut self, purpose: write_wait::Purpose) -> Result<()> {
        ensure!(
            self.write_wait.is_none(),
            "Ger SaveGame owner is already active"
        );
        self.write_wait = Some(write_wait::Host::load(
            &self.assets,
            purpose,
            &self.save_store.as_ref().unwrap().data,
        )?);
        Ok(())
    }
    fn prepare_write_wait(&mut self, context: &mut PreRenderContext) -> Result<()> {
        if let Some(wait) = self.write_wait.as_mut() {
            wait.prepare(context)?;
        }
        Ok(())
    }

    fn handle_child_events(&mut self, events: Vec<title_child::Event>) {
        for event in events {
            match event {
                title_child::Event::Save(slot) => self.manual_write(slot, true),
                title_child::Event::Delete(slot) => self.manual_write(slot, false),
                title_child::Event::Load(slot) => self.start_game(Some(slot + 1)),
                title_child::Event::Remember(slot) => {
                    let store = self.save_store.as_mut().unwrap();
                    store.data.save_menu_position = slot;
                    store.mark_dirty();
                }
                title_child::Event::SettingsChanged(settings) => {
                    if let Some(host) = self.host.as_mut() {
                        host.scene.apply_settings(&settings);
                    }
                    if let Some(system) = self.system.as_mut() {
                        system.apply_settings(&settings);
                    }
                    if let Some(title) = self.title.as_mut() {
                        title.apply_settings(&settings);
                    }
                    let store = self.save_store.as_mut().unwrap();
                    store.data.settings = settings;
                    store.mark_dirty();
                }
                title_child::Event::SaveRequested => {
                    self.waiting_for_child_save = true;
                    if let Err(error) = self.begin_write_wait(write_wait::Purpose::Config) {
                        tracing::error!(%error, "Ger Config publication failed; retaining Config");
                        self.waiting_for_child_save = false;
                        if let Some(child) = self.title_child.as_mut() {
                            child.save_failed();
                        }
                    }
                }
                title_child::Event::Close => self.finish_system_child(),
            }
        }
    }
}
impl ShinApp for Gerokasu2App {
    const WINDOW_TITLE: &'static str = "幻想牢獄のカレイドスコープ 2";
    fn window_title() -> &'static str {
        if Game::current() == Game::Gerokasu {
            "幻想牢獄のカレイドスコープ"
        } else {
            Self::WINDOW_TITLE
        }
    }
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
        let save_store = Some(saves::Store::load(
            saves::progress_path(&cli)?,
            imported.clone(),
        )?);
        let io = locate_assets_for_game(cli.game, cli.assets_dir.as_deref())?;
        let data = io
            .read_file("/main.snr")?
            .read_all_sync()
            .context("Reading Gerokasu2 scenario")?;
        let scenario = Arc::new(Gerokasu2Scenario::new(data.into())?);
        let assets = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: cli.game,
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
        let menu_bgm = Arc::new(parking_lot::Mutex::new(menu_audio::MenuMusic::new(
            audio.clone(),
            &scenario,
        )));
        let mut first_boot = None;
        let host = if let Some(data) = imported.as_ref() {
            let mut host = shin_tasks::block_on(GerHost::load(
                assets.clone(),
                scenario.clone(),
                audio.clone(),
                fresh_seed()?,
            ))?;
            host.scene.defer_audio_publication();
            shin_tasks::block_on(host.restore_saved(scenario.clone(), data, cli.save_slot))?;
            host.scene
                .publish_global_audio(menu_bgm.lock().shared_audio())?;
            Some(host)
        } else {
            // 252C8/5C2D0: the first owner is always FirstBoot; its finish
            // runs the DF9B0 TitleDemo/Title gate.
            let voice = (cli.game == Game::Gerokasu).then(|| {
                system_voice::SystemVoice::new(
                    audio.clone(),
                    assets.clone(),
                    save_store.as_ref().unwrap().data.settings.v2_voicevol,
                )
            });
            first_boot = Some(shin_tasks::block_on(first_boot::FirstBoot::load(
                assets.clone(),
                voice,
            ))?);
            None
        };
        let system_textures = shin_tasks::block_on(assets.load("/systex.txa"))
            .inspect_err(|error| tracing::error!(%error, "Ger touch-effect art failed to load"))
            .ok();
        let play_clock = crate::adv::play_clock::PlayClock::from_saved_seconds(
            imported
                .as_ref()
                .map(|data| data.play_seconds)
                .or_else(|| save_store.as_ref().map(|store| store.data.play_seconds))
                .unwrap_or(0),
        );
        Ok(Self {
            backlog: None,
            #[cfg(test)]
            backlog_options: Default::default(),
            system: None,
            loading_system: None,
            modal: Default::default(),
            backdrop: None,
            frozen_manual: None,
            system_title_exit: false,
            host,
            title: None,
            first_boot,
            title_demo: None,
            gallery: None,
            loading_gallery: None,
            loading_title: None,
            title_child: None,
            loading_child: None,
            write_wait: None,
            waiting_for_child_save: false,
            waiting_for_exit_save: false,
            exit_save_ready: false,
            adv_frame: None,
            loading_game: None,
            loading_cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            assets,
            scenario,
            suspended: false,
            audio,
            menu_bgm,
            frame: FrameId::default(),
            point: Vec2::ZERO,
            error: None,
            save_store,
            play_clock,
            touch_effect: Default::default(),
            touch_background: None,
            system_textures,
        })
    }
    fn custom_event(&mut self, _: AppContext<Self>, _: ()) {}
    /// Config's 自動スリープの抑制 row (settings v18, serialized under its
    /// historical name) keeps playing while unfocused, as in Umineko.
    fn run_in_background(&self) -> bool {
        self.save_store
            .as_ref()
            .is_some_and(|store| store.data.settings.v18_showsongtitle)
    }
    fn focus_lost(&mut self) {
        self.touch_effect.cancel_contact();
        if let Some(backlog) = self.backlog.as_mut() {
            backlog.cancel_input();
            backlog.set_scroll_axis(0.);
        }
        if let Some(system) = self.system.as_mut() {
            system.cancel_input();
        }
        if let Some(host) = self.host.as_mut() {
            host.cancel_input();
        }
        if let Some(demo) = self.title_demo.as_mut() {
            demo.cancel_input();
        }
        if let Some(title) = self.title.as_mut() {
            title.cancel_input();
        }
        if let Some(gallery) = self.gallery.as_mut() {
            gallery.cancel_input();
            gallery.set_viewer_axes(Vec2::ZERO);
        }
        if let Some(child) = self.title_child.as_mut() {
            child.cancel_input();
            child.set_scroll_axis(0.0);
        }
    }
    fn set_suspended(&mut self, value: bool) {
        self.suspended = value;
        if value {
            self.touch_effect.cancel_contact();
        }
        if value && let Some(backlog) = self.backlog.as_mut() {
            backlog.cancel_input();
            backlog.set_scroll_axis(0.);
        }
        if value && let Some(system) = self.system.as_mut() {
            system.cancel_input();
        }
        if let Some(host) = self.host.as_mut() {
            host.suspended = value;
            if value {
                host.cancel_input();
            }
        }
        if value && let Some(title) = self.title.as_mut() {
            title.cancel_input();
        }
        if value && let Some(gallery) = self.gallery.as_mut() {
            gallery.cancel_input();
            gallery.set_viewer_axes(Vec2::ZERO);
        }
        if value && let Some(child) = self.title_child.as_mut() {
            child.cancel_input();
            child.set_scroll_axis(0.0);
        }
        self.audio.set_suspended(value);
    }
    fn raw_input(&mut self, raw: &RawInputState) {
        self.point = raw.mouse.position;
        if let Some(backlog) = self.backlog.as_mut() {
            backlog.set_scroll_axis(super::text_scroll_axis(raw));
        }
        if let Some(child) = self.title_child.as_mut() {
            child.set_scroll_axis(super::text_scroll_axis(raw));
        }
        if let Some(gallery) = self.gallery.as_mut() {
            use shin_input::inputs::GamepadAxis;
            gallery.set_viewer_axes(Vec2::new(
                raw.gamepads.unified.axes[GamepadAxis::LeftStickX],
                -raw.gamepads.unified.axes[GamepadAxis::LeftStickY],
            ));
        }
    }
    fn update(
        &mut self,
        mut ctx: AppContext<Self>,
        mut input: EnumMap<AppAction, ActionState>,
        elapsed: Duration,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        {
            static UPDATES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let frame = UPDATES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug_input::apply(frame, &mut input);
        }
        let held = input[AppAction::PointerClick].is_held;
        self.update_owner(&mut ctx, input, elapsed, encoder);
        if let Err(error) = self.prepare_write_wait(&mut PreRenderContext {
            device: &ctx.wgpu.device,
            queue: &ctx.wgpu.queue,
            breakup_texture: None,
            resize_source: &ctx.winit.resize_source,
            sampler_store: &ctx.render.sampler_store,
            depth_stencil: ctx.render.canvas_depth_stencil_buffer.get_target_view(),
            pipeline_storage: &mut ctx.render.pipelines,
            dynamic_buffer: &mut ctx.render.dynamic_buffer,
            encoder,
        }) {
            self.error = Some(format!("{error:#}"));
        }
        // Every early owner return reaches the single global audio step.
        self.advance_global_audio(Ticks::from_duration(elapsed));
        self.prepare_touch_frame(
            &mut PreRenderContext {
                device: &ctx.wgpu.device,
                queue: &ctx.wgpu.queue,
                breakup_texture: None,
                resize_source: &ctx.winit.resize_source,
                sampler_store: &ctx.render.sampler_store,
                depth_stencil: ctx.render.canvas_depth_stencil_buffer.get_target_view(),
                pipeline_storage: &mut ctx.render.pipelines,
                dynamic_buffer: &mut ctx.render.dynamic_buffer,
                encoder,
            },
            Ticks::from_duration(elapsed),
            held,
            ctx.winit.window.inner_size(),
        );
    }

    fn render(&mut self, _: RenderContext, pass: &mut RenderPass) {
        if let Err(error) = self.render_touch_frame(pass) {
            tracing::error!(%error,"Ger frame rendering failed");
            self.error = Some(format!("{error:#}"));
        }
    }
}
impl Gerokasu2App {
    fn update_owner(
        &mut self,
        ctx: &mut AppContext<Self>,
        mut input: EnumMap<AppAction, ActionState>,
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
        self.poll_saves(Ticks::from_duration(elapsed));
        if self.write_wait.is_some() {
            input = EnumMap::default();
        }
        self.poll_system_menu();
        if self.error.is_some() {
            return;
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
        if let Some(task) = self.loading_gallery.as_mut()
            && let Some(result) = task.poll_naive()
        {
            self.loading_gallery = None;
            match result {
                Ok(mut gallery) => {
                    gallery.cancel_input();
                    self.gallery = Some(gallery);
                    self.title = None;
                }
                Err(error) => {
                    tracing::error!(%error,"Ger gallery preparation failed; retaining Title");
                    self.recover_title_child();
                }
            }
        }
        self.poll_title_destination();
        if self.first_boot.is_some() {
            self.update_first_boot(Ticks::from_duration(elapsed));
            return;
        }
        if self.title_demo.is_some() {
            self.update_title_demo(&ctx.wgpu.queue, &input, map, elapsed);
            return;
        }
        if self.host.is_some() && self.loading_title.is_some() {
            // The last actually composed ADV frame remains present while its
            // destination worker runs. The global audio still updates.
            return;
        }
        if self.exit_save_ready {
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
            if let Err(error) = self.complete_exit_save(&mut pre) {
                self.error = Some(format!("{error:#}"));
            }
            return;
        }
        if self.gallery.is_some() {
            let mut requested = None;
            if !self.suspended && self.loading_title.is_none() {
                match self.gallery.as_mut().unwrap().update(
                    Ticks::from_duration(elapsed).as_f32(),
                    &input,
                    self.point,
                    map,
                    elapsed.as_secs_f64(),
                ) {
                    Ok(destination) => requested = destination,
                    Err(error) => {
                        tracing::error!(%error,"Ger gallery update failed");
                        self.error = Some(format!("{error:#}"));
                    }
                }
            }
            self.commit_menu_unlocks();
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
            if let Err(error) = self.gallery.as_mut().unwrap().prepare(&mut pre) {
                tracing::error!(%error,"Ger gallery preparation failed");
                self.error = Some(format!("{error:#}"));
            }
            if let Some(requested) = requested {
                match self.capture_menu_source(&mut pre) {
                    Ok(snapshot) => self.start_gallery_title(requested, snapshot),
                    Err(error) => {
                        tracing::error!(%error,"Ger gallery return capture failed; retaining source");
                        self.gallery.as_mut().unwrap().recover_return();
                    }
                }
            }
            return;
        }
        self.poll_game_destination();
        self.poll_child();
        if self.title.is_some() {
            let mut requested_gallery = None;
            let retained_parent_clock = self.title_child.is_some()
                || self.loading_child.is_some()
                || self.loading_game.is_some()
                || self.loading_gallery.is_some();
            if retained_parent_clock {
                self.title.as_mut().unwrap().update_audio();
            }
            if let Some(child) = self.title_child.as_mut() {
                if !self.suspended && self.loading_game.is_none() {
                    match child.update(
                        Ticks::from_duration(elapsed).as_f32(),
                        &input,
                        self.point,
                        map,
                        elapsed.as_secs_f64(),
                    ) {
                        Ok(events) => self.handle_child_events(events),
                        Err(error) => {
                            tracing::error!(%error, "Ger title child update failed");
                            self.recover_title_child();
                        }
                    }
                }
            } else if !self.suspended
                && self.loading_game.is_none()
                && self.loading_child.is_none()
                && self.loading_gallery.is_none()
            {
                let title = self.title.as_mut().unwrap();
                if let Some(action) = title.update(
                    Ticks::from_duration(elapsed).as_f32(),
                    &input,
                    self.point,
                    map,
                    elapsed.as_secs_f64(),
                ) {
                    use crate::adv::gerokasu2::title::TitleAction;
                    match action {
                        TitleAction::NewGame { .. } => self.start_game(None),
                        TitleAction::Continue => self.start_game(Some(0)),
                        TitleAction::Load => self.start_title_child(title_child::Kind::Load),
                        TitleAction::Config => self.start_title_child(title_child::Kind::Config),
                        TitleAction::Manual => self.start_title_child(title_child::Kind::Manual),
                        TitleAction::CgGallery => requested_gallery = Some(gallery_mode::Kind::Cg),
                        TitleAction::MusicGallery => {
                            requested_gallery = Some(gallery_mode::Kind::Music)
                        }
                        TitleAction::BustupGallery => {
                            requested_gallery = Some(gallery_mode::Kind::Bustup)
                        }
                    }
                }
            }
            self.commit_menu_unlocks();
            if let Some(kind) = requested_gallery {
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
                match self.capture_menu_source(&mut pre) {
                    Ok(snapshot) => self.start_gallery(kind, snapshot),
                    Err(error) => {
                        tracing::error!(%error,"Ger gallery capture failed; retaining Title");
                        self.recover_title_child();
                    }
                }
            }
            if retained_parent_clock && !self.suspended {
                // Native DA170 polls children/callbacks before its literal-one
                // parent tween tick. Normal Title::update already does this.
                self.title
                    .as_mut()
                    .unwrap()
                    .advance_retained_parent_frame(Ticks::from_duration(elapsed).as_f32());
            }
            if let Some(child) = self.title_child.as_mut() {
                if let Err(error) = child.prepare(&mut PreRenderContext {
                    device: &ctx.wgpu.device,
                    queue: &ctx.wgpu.queue,
                    breakup_texture: None,
                    resize_source: &ctx.winit.resize_source,
                    sampler_store: &ctx.render.sampler_store,
                    depth_stencil: ctx.render.canvas_depth_stencil_buffer.get_target_view(),
                    pipeline_storage: &mut ctx.render.pipelines,
                    dynamic_buffer: &mut ctx.render.dynamic_buffer,
                    encoder,
                }) {
                    tracing::error!(%error, "Ger title child preparation failed");
                    self.error = Some(format!("{error:#}"));
                }
            }
            return;
        }
        {
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
            if let Err(error) = self.prepare_adv_menus(&mut input, &mut pre, map, elapsed.as_secs_f64()) {
                self.error = Some(format!("{error:#}"));
                return;
            }
        }
        let result = self.update_adv_scene(
            Ticks::from_duration(elapsed),
            &input,
            map,
            elapsed.as_secs_f64(),
        );
        if !self.suspended && self.modal.advance(Ticks::from_duration(elapsed).as_f32()) {
            self.backdrop = None;
        }
        match result {
            Ok(RunnerStatus::Exited) => {
                if let Err(error) = self.begin_exit_write() {
                    self.error = Some(format!("{error:#}"));
                    return;
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                return;
            }
        }
        self.collect_scene_progress();
        if !self.suspended
            && let Some(host) = self.host.as_mut()
        {
            host.update_notifications(Ticks::from_duration(elapsed));
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
        if let Err(error) = self.compose_adv_frame(&mut pre) {
            self.error = Some(format!("{error:#}"));
        }
    }
}
#[cfg(test)]
mod tests;

// Original DDE94 takes the low sixteen bits of the platform tick for fresh ADV.
fn fresh_seed() -> Result<u32> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u32 & 0xffff)
}
