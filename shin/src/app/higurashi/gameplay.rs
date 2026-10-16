//! Updated ADV11cd8/First18648 resource gate and retained scene initialization.
use super::{first_boot::CommonAssets, scene_host::Globals};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            audio::AudioFramePolicy,
            host::HigurashiHost,
            input::Buttons,
            message_owner::HigurashiMessageOwner,
            read_state::ReadPolicy,
            reconstruction_mode::OverlayClock,
            replay::ReplaySession,
            runner::reconstruction_mode::Overlay,
            runner::{HigurashiRunner, Policy, Status},
            scene_textures::HigurashiSceneTextures,
        },
        input_area::PointerFrame,
        play_clock::PlayClock,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{
    format::save::higurashi::{GameData, SaveData},
    time::Ticks,
};
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;
pub(super) mod bindings;
pub(super) mod bonus;
mod controls;
mod exit;
pub(super) mod kakera;
mod music_title;
mod navigation;
mod notification;
mod notification_get;
pub(super) mod quiz;
mod save_menu;
pub(super) mod scenario_select;
mod selection;
mod temporary_job;
mod tips_get;
pub(super) mod title_slot;
use bindings::Bindings;
type Seeds = Arc<Mutex<Box<dyn FnMut() -> [u32; 4] + Send>>>;
struct Job {
    task: Option<shin_tasks::AsyncTask<Result<Arc<CharacterTextures>>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct Metadata {
    common: Arc<CommonAssets>,
    data: SaveData,
    storage: Option<Arc<dyn super::load_game::Storage>>,
    runtime_saves: Option<super::runtime_saves::Slots>,
    clock: PlayClock,
    platform_flag: bool,
    last_title_mode: i32,
    server: Arc<AssetServer>,
    manager: Arc<shin_audio::AudioManager>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
}
pub(super) struct Gameplay {
    globals: Metadata,
    host: Option<HigurashiHost>,
    bindings: Bindings,
    snapshot: Option<Overlay>,
    job: Option<Job>,
    atlas: Option<Arc<CharacterTextures>>,
    screens: Option<HigurashiSceneTextures>,
    seeds: Seeds,
    notification: notification::Notification,
    music_title: music_title::MusicTitle,
    controls: controls::Controls,
    navigation: navigation::Navigation,
    selection: selection::Selection,
    tips_get: tips_get::TipsGet,
    notification_get: notification_get::NotificationGet,
    kakera_request: Option<kakera::SceneRequest>,
    kakera_queued: bool,
    exit_request: Option<exit::Request>,
    bonus_request: Option<bonus::SceneRequest>,
    title_request: Option<super::title_demo::TitleRequest>,
    save_id: Option<u64>,
    save_destination: Option<exit::SaveDestination>,
    tips_request: Option<super::tips_menu::factory::Request>,
    quiz_request: Option<quiz::SceneRequest>,
    scenario_select_request: Option<scenario_select::SceneRequest>,
    manual_menu: Option<save_menu::Retained>,
    config_menu: Option<save_menu::RetainedConfig>,
    help_menu: Option<save_menu::RetainedManual>,
    chart_menu: Option<save_menu::RetainedChart>,
    chart_input: Option<super::input_bridge::Analog>,
    chart_seed: Arc<dyn Fn() -> u32 + Send + Sync>,
    chart_adv: Option<(super::runtime_game_data::RuntimeGameData, u32)>,
    manual_load: Option<(super::runtime_game_data::RuntimeGameData, u32)>,
    menu_extent: (u16, u16),
    #[cfg(test)]
    reference_overlay_cues: Vec<&'static str>,
    clock_source: Box<dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send>,
    #[cfg(test)]
    pub flags: u32,
    pub quick_return: bool,
    pub input_permission: bool,
    fault: Option<String>,
}
impl Gameplay {
    #[cfg(test)]
    pub fn new(
        globals: Globals,
        data: impl Into<super::runtime_game_data::RuntimeGameData>,
        flags: u32,
        snapshot: RenderTexture,
        seeds: impl FnMut() -> [u32; 4] + Send + 'static,
    ) -> Result<Self> {
        let mut bindings = Bindings::new_runtime(&globals.data, &data.into())?;
        if let Some(slots) = &globals.runtime_saves {
            bindings.autosave = slots.autosave();
        }
        let seeds: Seeds = Arc::new(Mutex::new(Box::new(seeds)));
        let runner = Self::prepare_runner(&globals, &seeds)?;
        Ok(Self::from_prepared(
            globals, bindings, runner, flags, snapshot, seeds,
        ))
    }
    /// Fallible constructor work borrows retained globals. A scene slot can
    /// retry it after source retirement without losing audio/data/snapshot.
    fn prepare_runner(globals: &Globals, seeds: &Seeds) -> Result<HigurashiRunner> {
        let constructor_seeds = seeds.clone();
        HigurashiRunner::fresh_from_loaded_assets(
            globals.server.clone(),
            globals.common.scenario.clone(),
            HigurashiMessageOwner::from_common_resources(
                globals.common.font.clone(),
                globals.common.message.clone(),
            ),
            move || (constructor_seeds.lock())(),
        )
    }
    fn from_prepared(
        globals: Globals,
        bindings: Bindings,
        mut runner: HigurashiRunner,
        flags: u32,
        snapshot: RenderTexture,
        seeds: Seeds,
    ) -> Self {
        let server = globals.server.clone();
        runner.set_quiz_score(flags as i32);
        runner.configure_movies(
            globals.manager.clone(),
            crate::layer::user::higurashi::movie::percentages(&globals.data),
            None,
        );
        let screens = HigurashiSceneTextures::for_scene(
            &globals.server.context().wgpu_device,
            &globals.server.context().wgpu_queue,
            crate::adv::higurashi::adv_cache::NATIVE_EXTENT,
            &runner.frame().scene,
        )
        .expect("Native literal dimensions supply the Screen constructor extent");
        let init = bindings.progress.progress().active_entry().scenario_id;
        let task = shin_tasks::async_io::spawn_loader(async move {
            // Original First worker2009c publishes/adv.txa before18950 creates its VM.
            server.load("/adv.txa").await
        });
        let retirement = task.retirement_handle().unwrap();
        let Globals {
            common,
            data,
            storage,
            runtime_saves,
            clock,
            platform_flag,
            last_title_mode,
            mut audio,
            server,
            manager,
            sounds,
        } = globals;
        let runtime_saves =
            Some(runtime_saves.unwrap_or_else(|| super::runtime_saves::Slots::loaded(&data)));
        audio.retire_adv_for_scene();
        runner.retain_live_exit();
        let mut host = HigurashiHost::new(runner, audio);
        host.bind_graphics(&server.context().wgpu_device, &server.context().wgpu_queue)
            .expect("Fresh native Screen and ADV cache use the same constructor extent");
        Self {
            globals: Metadata {
                common,
                data,
                storage,
                runtime_saves,
                clock,
                platform_flag,
                last_title_mode,
                server,
                manager,
                sounds,
            },
            host: Some(host),
            bindings,
            snapshot: Some(Overlay {
                clock: OverlayClock::captured(true),
                snapshot,
                mask: None,
            }),
            job: Some(Job {
                task: Some(task),
                retirement,
            }),
            atlas: None,
            screens: Some(screens),
            seeds,
            notification: Default::default(),
            music_title: Default::default(),
            controls: Default::default(),
            navigation: Default::default(),
            selection: Default::default(),
            tips_get: Default::default(),
            notification_get: Default::default(),
            kakera_request: None,
            kakera_queued: false,
            exit_request: None,
            bonus_request: None,
            title_request: None,
            save_id: None,
            save_destination: None,
            tips_request: None,
            quiz_request: None,
            scenario_select_request: None,
            manual_menu: None,
            config_menu: None,
            help_menu: None,
            manual_load: None,
            chart_menu: None,
            chart_input: None,
            chart_seed: Arc::new(crate::engine_random::next_seed),
            chart_adv: None,
            menu_extent: (1920, 1080),
            #[cfg(test)]
            reference_overlay_cues: Vec::new(),
            clock_source: Box::new(|| Ok(shin_core::format::save::higurashi::SaveDate::now())),
            #[cfg(test)]
            flags,
            quick_return: (5..13).contains(&init),
            input_permission: false,
            fault: None,
        }
    }
    pub fn host(&self) -> Option<&HigurashiHost> {
        self.host.as_ref()
    }
    #[cfg(test)]
    pub(super) fn reference_play_modes(&self) -> u32 {
        self.controls.state.modes
    }
    #[cfg(test)]
    pub(super) fn reference_effective_play_modes(&self) -> u32 {
        self.controls.read_policy(false).modes
    }
    #[cfg(test)]
    pub(super) fn reference_fake_select_state(
        &self,
    ) -> Option<(u32, u32, i32, u32, [u32; 2], u32)> {
        let ui = self.selection.fake.ui.as_ref()?.0.lock();
        let m = &ui.model;
        Some((
            m.phase,
            m.remaining,
            m.selected,
            m.countdown.to_bits(),
            m.rows.each_ref().map(|r| r.alpha.value().to_bits()),
            m.seed,
        ))
    }
    #[cfg(test)]
    pub(super) fn reference_story_choice(&self) -> Option<(u32, i32, Vec<(usize, String)>)> {
        if let Some(ui) = &self.selection.ui {
            let ui = ui.lock();
            return Some((
                ui.model.phase,
                ui.model.selected,
                ui.model
                    .rows
                    .iter()
                    .map(|row| (row.spec.original, row.spec.label.clone()))
                    .collect(),
            ));
        }
        let child = self
            .tips_get
            .job
            .as_ref()?
            .ui
            .as_ref()?
            .wrapper
            .reference_child()?;
        Some((
            child.0,
            child.1,
            child.6.into_iter().map(|row| (row.0, row.1)).collect(),
        ))
    }
    #[cfg(test)]
    pub(super) fn reference_story_notification(&self) -> Option<(i32, u32)> {
        let job = self
            .tips_get
            .job
            .as_ref()
            .or(self.notification_get.job.as_ref())?;
        let ui = job.ui.as_ref()?;
        if ui.wrapper.reference_child().is_some() {
            return None;
        }
        let widget = self
            .host
            .as_ref()?
            .runner
            .frame()
            .scene
            .layers()
            .layer(ui.target)?
            .temporary_owner()?;
        Some((widget.native_id(), widget.phase()))
    }
    #[cfg(test)]
    pub(super) fn reference_story_character(&self) -> Option<(u32, i32, Vec<usize>)> {
        let view = &self.selection.character.job.as_ref()?.ui.as_ref()?.1;
        let model = view.model.lock();
        Some((
            model.phase,
            model.selected,
            model.rows.iter().map(|row| row.original).collect(),
        ))
    }
    #[cfg(test)]
    pub(super) fn reference_story_variables(
        &self,
    ) -> (&shin_core::format::save::PersistData, &[(u32, i32)]) {
        (
            self.bindings.progress.persistent(),
            self.bindings.progress.pending_persistent(),
        )
    }
    pub fn host_mut(&mut self) -> Option<&mut HigurashiHost> {
        self.host.as_mut()
    }
    #[cfg(test)]
    pub fn game_data(&self) -> Result<GameData> {
        self.bindings.game_data()
    }
    #[cfg(test)]
    pub fn runtime_game_data(&self) -> Result<super::runtime_game_data::RuntimeGameData> {
        self.bindings.runtime_game_data()
    }
    pub(super) fn touch_diagnostic_save_menu(&self) -> Option<&super::save_menu::Owner> {
        self.manual_menu.as_ref().map(|menu| &menu.owner)
    }
    #[cfg(test)]
    pub(super) fn reference_save_menu(&self) -> Option<&super::save_menu::Owner> {
        self.touch_diagnostic_save_menu()
    }
    #[cfg(test)]
    pub(super) fn reference_bindings(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.bindings.memory.entries().len(),
            self.bindings.progress.responses().1,
            self.bindings.history.bits().words().as_ptr() as usize,
            self.bindings.choices.seen().words().as_ptr() as usize,
            self.bindings.choices.last().as_ptr() as usize,
        )
    }
    pub fn take_kakera_scene_request(&mut self) -> Option<kakera::SceneRequest> {
        self.kakera_request.take()
    }
    fn take_bonus_scene_request(&mut self) -> Option<bonus::SceneRequest> {
        self.bonus_request.take()
    }
    pub fn take_tips_scene_request(&mut self) -> Option<super::tips_menu::factory::Request> {
        self.tips_request.take()
    }
    pub fn take_quiz_scene_request(&mut self) -> Option<quiz::SceneRequest> {
        self.quiz_request.take()
    }
    pub fn take_scenario_select_scene_request(&mut self) -> Option<scenario_select::SceneRequest> {
        self.scenario_select_request.take()
    }
    pub(super) fn take_manual_load(
        &mut self,
    ) -> Option<(super::runtime_game_data::RuntimeGameData, u32)> {
        self.manual_load.take()
    }
    pub(super) fn take_chart_adv(
        &mut self,
    ) -> Option<(super::runtime_game_data::RuntimeGameData, u32)> {
        self.chart_adv.take()
    }
    pub(super) fn set_desktop_mouse_release(
        &mut self,
        release: Option<crate::adv::input_area::MouseRelease>,
    ) {
        self.controls.desktop_mouse_release = release;
    }
    pub(super) fn set_temporary_skip(&mut self, held: bool) {
        self.controls.desktop_held_skip = held;
    }
    pub(super) fn set_analog_input(&mut self, input: super::input_bridge::Analog) {
        self.chart_input = Some(input);
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_chart_owner(
        &self,
    ) -> Option<&crate::app::higurashi::chart::owner::Owner> {
        self.chart_menu.as_ref().map(|menu| &menu.owner)
    }
    pub(super) fn resize_manual_input(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid gameplay menu extent");
        self.menu_extent = extent;
        if let Some(menu) = &mut self.manual_menu {
            menu.owner.resize(extent)?;
        }
        if let Some(menu) = &mut self.config_menu {
            menu.owner.resize(extent)?;
        }
        if let Some(menu) = &mut self.help_menu {
            menu.owner.resize(extent)?;
        }
        Ok(())
    }
    /// The application supplies the same1280x720 native touch domain to Host
    /// modes and this manual owner; output resolution is independent.
    pub fn resize_input(&mut self, extent: (u16, u16)) -> Result<()> {
        self.resize_manual_input(extent)?;
        if let Some(menu) = &mut self.chart_menu {
            menu.owner.resize(extent)?;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.chart_input = None;
        self.controls.desktop_mouse_release = None;
        self.controls.desktop_held_skip = false;
        self.controls.allow_temporary_skip(false);
        if let Some(host) = &mut self.host {
            host.cancel_input();
        }
        if let Some(menu) = &mut self.manual_menu {
            menu.owner.cancel_input();
        }
        if let Some(menu) = &mut self.config_menu {
            menu.owner.cancel_input();
        }
        if let Some(menu) = &mut self.help_menu {
            menu.owner.cancel_input();
        }
        if let Some(menu) = &mut self.chart_menu {
            menu.owner.cancel_input();
        }
        self.controls.flush_input = true;
        Ok(())
    }
    pub fn has_save_task(&self) -> bool {
        self.host.as_ref().is_some_and(|host| host.has_save_task())
            || self
                .manual_menu
                .as_ref()
                .is_some_and(|menu| menu.owner.has_save_task())
            || self
                .config_menu
                .as_ref()
                .is_some_and(|menu| menu.owner.has_save_task())
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn reference_config(&self) -> Option<(u32, usize, bool, u8)> {
        self.config_menu
            .as_ref()
            .map(|menu| menu.owner.reference_state())
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn reference_config_question(&self) -> Option<(i32, i32)> {
        self.config_menu
            .as_ref()
            .and_then(|menu| menu.owner.reference_question())
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn reference_manual(&self) -> Option<(usize, i32, bool, u32, i32)> {
        self.help_menu
            .as_ref()
            .map(|menu| menu.owner.reference_state())
    }
    pub fn freeze_progress(&self) -> Result<super::persistence::FrozenProgress> {
        let mut data = self.globals.data.clone();
        data.play_seconds = self.globals.clock.saved_seconds();
        self.bindings.publish_globals(&mut data);
        let mut slots = self.globals.runtime_saves.clone();
        if let Some(slots) = &mut slots {
            slots.update_auto(&self.bindings.autosave);
        }
        Ok(super::persistence::FrozenProgress {
            data,
            slots,
            storage: self
                .globals
                .storage
                .clone()
                .context("Higurashi progress has no storage owner")?,
        })
    }
    pub fn quiesce_progress(&mut self) -> Result<()> {
        if let Some(menu) = &mut self.manual_menu {
            menu.owner.quiesce_progress(&mut self.globals.data)?;
        }
        if let Some(menu) = &self.config_menu {
            menu.owner.publish_progress(&mut self.globals.data);
        }
        Ok(())
    }
    #[cfg(test)]
    fn pending_exit_request(&self) -> Option<&exit::Request> {
        self.exit_request.as_ref()
    }
    pub fn take_title_scene_request(&mut self) -> Option<super::title_demo::TitleRequest> {
        self.title_request.take()
    }
    #[cfg(test)]
    pub fn set_clock_source(
        &mut self,
        clock: impl FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send + 'static,
    ) {
        self.clock_source = Box::new(clock);
    }
    #[cfg(test)]
    pub fn notification_busy(&self) -> bool {
        self.host
            .as_ref()
            .is_some_and(|host| host.ui.indicator.transition.is_waiting())
    }
    #[cfg(test)]
    fn set_input_boundary(&mut self, boundary: controls::Boundary) {
        self.controls.boundary = boundary;
    }
    #[cfg(test)]
    pub fn pending_input_request(&self) -> Option<crate::adv::higurashi::input::Request> {
        self.controls.requests.front().copied()
    }
    pub fn take_input_flush(&mut self) -> bool {
        std::mem::take(&mut self.controls.flush_input)
    }
    pub async fn update(
        &mut self,
        elapsed: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        mut policy: Policy,
        audio_policy: AudioFramePolicy,
    ) -> Result<Status> {
        self.controls.allow_temporary_skip(false);
        let delta = crate::adv::higurashi::host::frame_delta(elapsed)?;
        ensure!(
            !self.kakera_queued,
            "Dispatch the retained menu scene before updating source Gameplay"
        );
        if let Some(fault) = &self.fault {
            bail!("Higurashi story initialization failed:{fault}");
        }
        // Explicit desktop loss of the whole scene's permission quarantines
        //sampled controls. Native lower ADV denial under a modal is local.
        if !permission {
            self.controls.flush_input = true;
        }
        let result = self
            .update_inner(
                delta,
                permission,
                buttons,
                pointer,
                &mut policy,
                audio_policy,
            )
            .await;
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        if result.is_ok() {
            if let Some(host) = &mut self.host {
                host.advance_touch(self.controls.state.temporary_indicator, delta);
            }
            self.controls.boundary.forced = false;
            self.globals.clock.advance(elapsed.as_f32());
            self.globals.data.play_seconds = self.globals.clock.saved_seconds();
            self.bindings.publish_globals(&mut self.globals.data);
            if let Some(slots) = &mut self.globals.runtime_saves {
                slots.update_auto(&self.bindings.autosave);
            }
            if let Some(host) = &mut self.host {
                for cue in host.take_overlay_cues() {
                    #[cfg(test)]
                    self.reference_overlay_cues.push(cue);
                    self.globals.sounds.lock().play(
                        &self.globals.server,
                        cue,
                        shin_core::vm::command::types::Volume(1.0),
                    );
                }
            }
            self.globals.sounds.lock().update();
        }
        result
    }
    async fn update_inner(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        policy: &mut Policy,
        audio_policy: AudioFramePolicy,
    ) -> Result<Status> {
        let mut constructed = false;
        // Runtime supplies a fresh snapshot each frame. Consume it even when
        // First/a different modal owns this update, so wheel motion cannot wait
        // for a future Chart or replay as a held control.
        let chart_input = self.chart_input.take();
        if let Some(host) = &self.host {
            host.runner
                .set_movie_preferences(crate::layer::user::higurashi::movie::percentages(
                    &self.globals.data,
                ));
        }
        if let Some(loaded) = self
            .job
            .as_mut()
            .and_then(|job| job.task.as_mut())
            .and_then(|task| task.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            let atlas = loaded?;
            let entry = self.bindings.progress.progress().active_entry();
            let fork = self.bindings.memory.fork_for_restore(
                entry.save_position,
                entry.replay_target,
                &self.bindings.progress,
            )?;
            let replay = ReplaySession::new(self.globals.common.scenario.clone(), fork)?;
            let host = self.host.as_mut().unwrap();
            let seeds = self.seeds.clone();
            host.begin_fresh_scene(
                replay,
                self.snapshot.take().unwrap().snapshot,
                self.bindings.history.clone(),
                move || (seeds.lock())(),
            )?;
            self.atlas = Some(atlas);
            constructed = true;
        }
        let host = self.host.as_mut().unwrap();
        if self.atlas.is_none() {
            self.input_permission = false;
            self.controls.flush_input = true; // First's d11c8 result gates13140.
            policy.paused = true;
            policy.animations_enabled = false;
            let mut audio_policy = audio_policy;
            audio_policy.focus_mode_active = false; // First still owns the mode stack.
            let seeds = self.seeds.clone();
            return host
                .update(delta, *policy, audio_policy, 4096, 100000, move || {
                    (seeds.lock())()
                })
                .await;
        }
        // Native settings+100 and play clock are supplied before polling menus.
        host.set_menu_frame(
            u8::from(self.globals.data.settings.extra_flag),
            self.globals.clock.ticks(),
        );
        host.set_browser_frame(crate::adv::higurashi::host::browser::BrowserFrame {
            duck_music: self.globals.data.settings.common.v4_voicefocus,
            outline_mode: self.globals.data.settings.common.v8,
        })?;
        self.globals
            .sounds
            .lock()
            .set_settings_volume(self.globals.data.settings.common.v3_sysvol);
        host.set_navigation_disabled(self.controls.boundary.disabled);
        // Native First queues the reconstruction child;new modes first poll next frame.
        self.bindings.publish_globals(&mut self.globals.data);
        if let Some(slots) = &mut self.globals.runtime_saves {
            slots.update_auto(&self.bindings.autosave);
        }
        let permitted = if constructed {
            false
        } else {
            host.poll_modes_with_menu_children_and_mouse_release(
                delta,
                u32::from(permission),
                buttons,
                pointer,
                &mut save_menu::Frame {
                    menu: &mut self.manual_menu,
                    config: &mut self.config_menu,
                    manual: &mut self.help_menu,
                    chart: &mut self.chart_menu,
                    chart_input,
                    chart_seed: self.chart_seed.clone(),
                    chart_adv: &mut self.chart_adv,
                    bindings: &self.bindings,
                    globals: &mut self.globals,
                    buttons,
                    pointer,
                    date: &mut *self.clock_source,
                    load: &mut self.manual_load,
                    extent: self.menu_extent,
                },
                self.controls.desktop_mouse_release,
            )?
        };
        for request in host.take_manual_menu_requests() {
            ensure!(
                self.manual_menu
                    .as_ref()
                    .is_some_and(|menu| menu.id == request.id),
                "Manual constructor notification lost its owner"
            );
        }
        for request in host.take_config_menu_requests() {
            ensure!(
                self.config_menu
                    .as_ref()
                    .is_some_and(|menu| menu.id == request.id),
                "Config constructor notification lost its owner"
            );
        }
        for request in host.take_help_menu_requests() {
            ensure!(
                self.help_menu
                    .as_ref()
                    .is_some_and(|menu| menu.id == request.id),
                "Manual constructor notification lost its owner"
            );
        }
        for request in host.take_chart_menu_requests() {
            ensure!(
                self.chart_menu
                    .as_ref()
                    .is_some_and(|menu| menu.id == request.id),
                "Chart constructor notification lost its owner"
            );
        }
        for id in host.take_save_notifications() {
            ensure!(
                self.save_id == Some(id),
                "Unexpected Higurashi save callback owner"
            );
            match self
                .save_destination
                .take()
                .context("Save callback lost its destination")?
            {
                exit::SaveDestination::Title => {
                    self.title_request = Some(super::title_demo::TitleRequest {
                        selected: -1,
                        mode: u32::MAX,
                    })
                }
                exit::SaveDestination::Tips(request) => {
                    self.tips_request = Some(request);
                    self.kakera_queued = true;
                }
            }
            self.save_id = None;
        }
        //20844 stage1/Yes calls15c64(unlock0) directly;it does not run EXIT's
        //closing/commit services or mark an unfinished tip complete.
        if host.take_menu_tips_return() {
            let data = self.bindings.runtime_game_data()?;
            let mut base = self.globals.data.clone();
            self.bindings.publish_globals(&mut base);
            let returned = super::tips_menu::factory::from_exit(
                &data,
                &self.globals.common.scenario,
                &mut base.save_vectors,
                0,
            )?;
            self.globals
                .data
                .save_vectors
                .extra_words_0
                .clone_from(&base.save_vectors.extra_words_0);
            host.audio.browser_music_multiplier(1.0);
            if !returned.save {
                self.bindings.progress.progress_mut().set_active_index(0)?;
                self.tips_request = Some(returned.request);
                self.kakera_queued = true;
            } else if let Some(storage) = self.globals.storage.clone() {
                ensure!(
                    self.save_id.is_none(),
                    "A retained save task already exists"
                );
                let mut slots = self
                    .globals
                    .runtime_saves
                    .clone()
                    .unwrap_or_else(|| super::runtime_saves::Slots::loaded(&self.globals.data));
                slots.update_auto(&self.bindings.autosave);
                let recover = storage.recover_write_errors();
                let write = Arc::new(move || storage.write(&slots.disk(&base).encode()?));
                host.push_save_mode(
                    crate::adv::higurashi::save_game::SaveGame::new(
                        self.globals.server.clone(),
                        write,
                        1,
                        0,
                    )
                    .with_desktop_recovery(
                        recover,
                        crate::adv::higurashi::save_game::RecoveryPolicy::RetryOnly,
                    ),
                );
                self.save_id = Some(0);
                self.save_destination = Some(exit::SaveDestination::Tips(returned.request));
            } else {
                ensure!(
                    self.exit_request.is_none(),
                    "A retained TIPS destination already exists"
                );
                self.exit_request = Some(exit::Request {
                    destination: crate::adv::higurashi::exit::Destination::Tips,
                    data,
                });
            }
        }
        //20790 and208f8 act only after the real return Question closes. These
        //services do not run EXIT's close/wait/commit or consume the source VM.
        if let Some(destination) = host.take_menu_return() {
            use crate::adv::higurashi::host::system_menu::MenuReturn;
            match destination {
                MenuReturn::Title => {
                    //15b6c snapshots global SaveData through the existing
                    //flag1 SaveGame;18134 queues Title(-1,-1) on its notice.
                    ensure!(
                        self.save_id.is_none(),
                        "A retained save task already exists"
                    );
                    let storage = self
                        .globals
                        .storage
                        .clone()
                        .context("SystemMenu Title return requires loaded progress storage")?;
                    let mut base = self.globals.data.clone();
                    self.bindings.publish_globals(&mut base);
                    let mut slots = self
                        .globals
                        .runtime_saves
                        .clone()
                        .unwrap_or_else(|| super::runtime_saves::Slots::loaded(&base));
                    slots.update_auto(&self.bindings.autosave);
                    let recover = storage.recover_write_errors();
                    let write = Arc::new(move || storage.write(&slots.disk(&base).encode()?));
                    host.push_save_mode(
                        crate::adv::higurashi::save_game::SaveGame::new(
                            self.globals.server.clone(),
                            write,
                            1,
                            0,
                        )
                        .with_desktop_recovery(
                            recover,
                            crate::adv::higurashi::save_game::RecoveryPolicy::RetryOnly,
                        ),
                    );
                    self.save_id = Some(0);
                    self.save_destination = Some(exit::SaveDestination::Title);
                }
                MenuReturn::Kakera => {
                    //15e54 uses the current signed ID,restores the global
                    //music multiplier,and copies the complete VM2530 owner.
                    //There is no12049 Bonus/12051 Title branch outside EXIT.
                    ensure!(
                        self.kakera_request.is_none() && !self.kakera_queued,
                        "Kakera return queued twice"
                    );
                    let scenario = self.bindings.progress.progress().active_entry().scenario_id;
                    let selected =
                        crate::adv::higurashi::host::system_menu::fragment_selected(scenario);
                    host.audio.browser_music_multiplier(1.0);
                    self.kakera_request = Some(kakera::SceneRequest {
                        mode: selected,
                        data: self.bindings.runtime_game_data()?,
                    });
                    self.kakera_queued = true;
                }
            }
        }
        // Source SELECT and its noncopyable child0 retire when reconstruction
        // publishes the new root, before its replayed VM is installed.
        if !host.runner.native_vm_attached() && host.runner.reconstruction_phase().is_some() {
            self.selection.retire_source();
            self.tips_get = Default::default();
            self.notification_get = Default::default();
        }
        if let Some(replay) = host.runner.take_reconstruction_result() {
            self.bindings.progress = replay.progress;
            self.bindings.memory = replay.memory;
        }
        // d11c8's result gates13140 input,not12bec command polling or the
        // reconstruction fade. Only counted/modal pause state pauses the VM.
        self.input_permission = permitted;
        let settings = &self.globals.data.settings.common;
        host.runner.set_message_settings(
            settings.v10_skipspeed,
            settings.v12,
            settings.v13_msgwinalpha,
        );
        policy.text_speed = i32::from(settings.v9_msgspeed);
        policy.outline_mode = settings.v8;
        let navigation = &mut self.navigation;
        let cues = self.controls.process_with_pointer(
            host,
            &mut self.bindings,
            permitted,
            buttons,
            Some(pointer),
            *policy,
            self.quick_return,
            |runner, bindings| {
                navigation.event_jump(runner, bindings, settings.v11_disallowskipunread)
            },
        )?;
        for cue in cues {
            let mut sounds = self.globals.sounds.lock();
            sounds.set_settings_volume(settings.v3_sysvol);
            controls::Controls::cue(cue, &mut sounds, &self.globals.server);
        }
        self.navigation.process(
            host,
            &self.bindings,
            &mut self.controls,
            settings.v11_disallowskipunread,
            &self.globals.sounds,
            &self.globals.server,
        )?;
        if host.has_modes() {
            self.input_permission = false;
        }
        self.controls.allow_temporary_skip(
            self.input_permission
                && !self.quick_return
                && !policy.paused
                && !self.navigation.pending(),
        );
        let temporary_skip_modes = self.controls.temporary_skip_modes();
        let read_policy = self.controls.read_policy(settings.v11_disallowskipunread);
        let mut audio_policy = audio_policy;
        audio_policy.voice_focus_enabled = settings.v4_voicefocus;
        if self.navigation.pending() {
            policy.paused = true;
            policy.animations_enabled = false;
            audio_policy.focus_mode_active = false;
        }
        // A concrete menu/search owner must accept this request before commands
        // resume. Retaining it does not invent a save or scene-jump response.
        if !self.controls.requests.is_empty() {
            policy.paused = true;
        }
        // Route labels read current settings at drawing time. Mode polling can
        // change settings or publish a restored Character11 before this tick.
        host.runner
            .set_layer_route_navigation(settings.v14_showroutenavi);
        let seeds = self.seeds.clone();
        let notice = &mut self.notification;
        let music_title = &mut self.music_title;
        let saved = &mut self.globals.data;
        let clock = &mut self.clock_source;
        let font = self.globals.common.font.clone();
        let context = self.globals.server.context();
        let input_state = std::cell::RefCell::new(&mut self.controls.state);
        let selection = &mut self.selection;
        let tips_get = &mut self.tips_get;
        let notification_get = &mut self.notification_get;
        let kakera_request = &mut self.kakera_request;
        let kakera_queued = &mut self.kakera_queued;
        let exit_request = &mut self.exit_request;
        let save_id = &mut self.save_id;
        let save_destination = &mut self.save_destination;
        let tips_request = &mut self.tips_request;
        let bonus_request = &mut self.bonus_request;
        let quiz_request = &mut self.quiz_request;
        let scenario_select_request = &mut self.scenario_select_request;
        let storage = self.globals.storage.clone();
        let runtime_saves = &self.globals.runtime_saves;
        let common = self.globals.common.clone();
        let server = self.globals.server.clone();
        let choice_seeds = self.seeds.clone();
        let status = host
            .update_native_with_overlay_audio_services(
                delta,
                *policy,
                read_policy,
                &mut bindings::LiveReads {
                    bindings: &mut self.bindings,
                    saved,
                    clock,
                },
                audio_policy,
                4096,
                100000,
                move || (seeds.lock())(),
                |runner, reads, ui, overlay, audio, modes_ready, save_modes| {
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::EXIT {arg1:0,..})) {
                        return exit::poll(runner,reads.bindings,reads.saved,audio,ui,&mut input_state.borrow_mut(),modes_ready,kakera_request,kakera_queued,exit_request,save_id,save_destination,tips_request,bonus_request,&common.scenario,storage.clone(),runtime_saves,&server,save_modes);
                    }
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::SELECT { .. })) {
                        return selection.poll(runner, reads.bindings, reads.saved, overlay, &common, &server, &choice_seeds, &mut **reads.clock);
                    }
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::FAKESELECT {})) {
                        return selection.fake.poll(runner, reads.bindings, reads.saved, overlay, &common, &server, &choice_seeds, &mut **reads.clock);
                    }
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::CHARSELECT { .. })) {
                        return selection.character.poll(runner, reads.bindings, reads.saved, overlay, &common, &server, &choice_seeds, &mut **reads.clock);
                    }
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::TIPSGET { .. })) {
                        return tips_get.poll(runner, reads.bindings, reads.saved, overlay,
                            &common, &server, &choice_seeds, tips_request, &mut **reads.clock);
                    }
                    if matches!(runner.pending_command(), Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::OTSUGET { .. } | shin_core::vm::higurashi::command::HigurashiRuntimeCommand::KAKERAGET { .. })) {
                        return notification_get.poll(runner, reads.saved, overlay,
                            &common, &server, &choice_seeds);
                    }
                    if matches!(runner.pending_command(),Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::KAKERA {})) {
                        return kakera::poll(runner,reads.bindings,reads.saved,kakera_request,kakera_queued,&mut **reads.clock);
                    }
                    if matches!(runner.pending_command(),Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::SNRSEL {..})) {
                        return scenario_select::poll(runner,reads.bindings,reads.saved,scenario_select_request,kakera_queued,&mut **reads.clock);
                    }
                    if matches!(runner.pending_command(),Some(shin_core::vm::higurashi::command::HigurashiRuntimeCommand::QUIZ {..})) {
                        return quiz::poll(runner,reads.bindings,reads.saved,quiz_request,kakera_queued,&mut **reads.clock);
                    }
                    reads.bindings.poll_commands(
                        runner,
                        &mut AutosavePublication {
                            saved: reads.saved,
                            clock: reads.clock,
                            notice,
                            font: font.clone(),
                            device: &context.wgpu_device,
                            queue: &context.wgpu_queue,
                            indicator: &mut ui.indicator,
                        },
                    )
                },
                |runner, audio, ui, reads| {
                    music_title.consume(audio, &mut ui.menu, reads.saved, &font, context)?;
                    // Command polling can publish a new generic Character11.
                    // Supply its current setting before the eager cache pass.
                    runner.set_layer_route_navigation(
                        reads.saved.settings.common.v14_showroutenavi,
                    );
                    let mut input_state = input_state.borrow_mut();
                    // Actual12c2c clears464 after polling an event command.
                    if runner
                        .native_read_state()
                        .is_some_and(|v| v.flags & 0x8000_0000 == 0)
                    {
                        input_state.temporary_indicator = 0;
                    }
                    let mut current = read_policy;
                    current.modes = input_state.modes | temporary_skip_modes;
                    controls::post_poll(runner, audio, ui, current)
                },
            )
            .await?;
        Ok(status)
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) -> Result<()> {
        if let Some(host) = &mut self.host {
            host.prepare_save_modes(context);
            host.prepare_menu_questions(context);
            if self.screens.is_none() {
                self.screens = Some(HigurashiSceneTextures::for_scene(
                    context.device,
                    context.queue,
                    host.runner.frame().cache.extent(),
                    &host.runner.frame().scene,
                )?);
            }
            let screens = self.screens.as_mut().unwrap();
            self.navigation.prepare(
                host,
                &self.bindings,
                &mut self.controls,
                context,
                screens,
                self.seeds.clone(),
            )?;
            use crate::adv::higurashi::{host::system_menu, input::Request};
            match self.controls.requests.front().copied() {
                Some(Request::SystemMenu) => {
                    ensure!(
                        self.globals.common.sysse.sounds.contains_key("page"),
                        "Missing Higurashi SystemMenu opening cue page"
                    );
                    let atlas = self.atlas.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("SystemMenu requires the completed First atlas")
                    })?;
                    host.open_system_menu_from_resources(
                        self.bindings.progress.progress().active_entry().scenario_id,
                        self.globals.sounds.clone(),
                        system_menu::Resources {
                            atlas: atlas.clone(),
                            font: self.globals.common.font.clone(),
                            sounds: self.globals.common.sysse.clone(),
                        },
                        context,
                        screens,
                    )?;
                    // Outer14690 owns the opening cue and input reset;1a8bc
                    // alone owns the menu. Both openings use the page cue.
                    self.controls.state.modes = 0;
                    self.controls.state.temporary_indicator = 0;
                    self.globals.sounds.lock().play(
                        &self.globals.server,
                        "page",
                        shin_core::vm::command::types::Volume(1.0),
                    );
                    self.controls.requests.pop_front();
                    self.input_permission = false;
                }
                Some(Request::MessageBrowser) => {
                    host.configure_browser_audio_from_bank(
                        self.globals.sounds.clone(),
                        self.globals.data.settings.common.v2_voicevol,
                        self.globals.common.sysse.clone(),
                    )?;
                    host.open_message_browser_from_resources(
                        &mut self.controls.state,
                        &self.bindings.progress,
                        &self.bindings.history,
                        &self.bindings.choices,
                        self.globals.data.settings.common.v11_disallowskipunread,
                        100000,
                        self.globals.common.system.clone(),
                        context,
                        screens,
                    )?;
                    // False availability is the native decline, with no capture
                    // or modal/input reset; both outcomes consume this request.
                    self.controls.requests.pop_front();
                    self.input_permission = false;
                }
                _ => {}
            }
            host.prepare_mode_views(context);
        }
        if let Some(menu) = &mut self.manual_menu {
            menu.owner.prepare(context)?;
        }
        if let Some(menu) = &self.config_menu {
            menu.owner.prepare(context);
        }
        if let Some(menu) = &self.chart_menu {
            menu.owner.prepare(context);
        }
        Ok(())
    }
    pub fn render(&mut self, pass: &mut RenderPass) -> Result<()> {
        if let Some(host) = &mut self.host {
            let screens = self
                .screens
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Story scene was not prepared"))?;
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            host.render_adv(pass, screens)?;
            self.music_title.render(
                pass,
                &host.ui.menu,
                self.globals.data.settings.common.v18_showsongtitle,
            );
            self.notification.render(
                pass,
                &host.ui.indicator,
                self.globals.data.settings.common.v17_showscenetitle,
            );
            if let Some(atlas) = &self.atlas {
                let touch = host.touch_state();
                crate::adv::higurashi::root_ui::render(
                    crate::adv::higurashi::root_ui::State {
                        modes: self.controls.read_policy(false).modes,
                        vm: host.runner.native_read_state(),
                        forced: self.controls.boundary.forced,
                        read_only: self.globals.data.settings.common.v11_disallowskipunread,
                        elapsed_seconds: host.ui_elapsed_seconds(),
                        navi_slide: touch.opacity,
                        pressed: touch.pressed,
                        hint: host.ui.hint.value(),
                    },
                    atlas,
                    pass,
                );
            }
            // Native14074 draws modes after the ADV indicators/hints.
            host.render_modes_with_children(pass, |request, pass| {
                if let Some(menu) = &mut self.manual_menu {
                    if menu.id == request.id {
                        menu.owner.render(pass);
                    }
                }
                if let Some(menu) = &self.config_menu {
                    if menu.id == request.id {
                        menu.owner
                            .render(pass)
                            .expect("Config has its prepared native resources");
                    }
                }
                if let Some(menu) = &self.help_menu {
                    if menu.id == request.id {
                        menu.owner.render(pass);
                    }
                }
                if let Some(menu) = &self.chart_menu {
                    if menu.id == request.id {
                        menu.owner
                            .render(pass)
                            .expect("Chart has its prepared native resources");
                    }
                }
            });
        }
        // Constructor55380 retains the actual previous composed platform frame.
        if let Some(snapshot) = &self.snapshot {
            snapshot.render(pass);
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn into_globals(self) -> Globals {
        self.into_globals_and_histories().0
    }
    fn into_globals_and_histories(
        mut self,
    ) -> (
        Globals,
        crate::adv::higurashi::read_history::MessageReadHistory,
        crate::adv::higurashi::choice::ChoiceHistory,
        crate::adv::higurashi::autosave::AutosaveMemory,
    ) {
        self.bindings.publish_globals(&mut self.globals.data);
        if let Some(slots) = &mut self.globals.runtime_saves {
            slots.update_auto(&self.bindings.autosave);
        }
        let audio = self.host.as_mut().unwrap().audio.take_for_adv_replacement();
        if let Some(menu) = self.manual_menu.take() {
            menu.owner.retire(&mut self.globals.data);
        }
        drop(self.config_menu.take());
        drop(self.help_menu.take());
        drop(self.chart_menu.take());
        drop(self.host.take());
        drop(self.job.take());
        let Metadata {
            common,
            data,
            storage,
            runtime_saves,
            clock,
            platform_flag,
            last_title_mode,
            server,
            manager,
            sounds,
        } = self.globals;
        (
            Globals {
                common,
                data,
                storage,
                runtime_saves,
                clock,
                platform_flag,
                last_title_mode,
                audio,
                server,
                manager,
                sounds,
            },
            self.bindings.history,
            self.bindings.choices,
            self.bindings.autosave,
        )
    }
}
struct AutosavePublication<'a> {
    saved: &'a mut SaveData,
    clock: &'a mut Box<dyn FnMut() -> Result<shin_core::format::save::higurashi::SaveDate> + Send>,
    notice: &'a mut notification::Notification,
    font: Arc<crate::asset::font::GpuFontLazy>,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    indicator: &'a mut crate::adv::higurashi::ui_state::Indicator,
}
impl crate::adv::higurashi::autosave::AutosaveServices for AutosavePublication<'_> {
    fn packed_time(&mut self) -> Result<u64> {
        Ok(bindings::packed_date((self.clock)()?))
    }
    fn captured(&mut self, memory: &crate::adv::higurashi::autosave::AutosaveMemory) -> Result<()> {
        bindings::publish_autosave(memory, self.saved);
        Ok(())
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        self.notice.prepare(
            self.device,
            self.queue,
            self.font.clone(),
            text,
            self.indicator,
        )
    }
}
#[cfg(test)]
mod tests;
