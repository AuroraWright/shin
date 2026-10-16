//! Shared root/message/voice scene adapter used by gameplay and hardware probes.
use super::{
    audio_commands::AudioCommands,
    card_select::CardSelectModel,
    drawables::BasicLoadBackend,
    gallery::{GalleryServices, GalleryUnlocks},
    layer_control::LayerControlServices,
    layer_edit::LayerEditServices,
    layer_factory::GerDrawable,
    layer_load::{LoadCounters, LoadServices},
    layers::Gerokasu2Layers,
    message_commands::MessageCommandContext,
    message_owner::SharedMessageOwner,
    movie::MovieServices,
    quiz::QuizServices,
    runner::{FramePolicy, Gerokasu2Runner, InitialMessage, RunnerServices, RunnerStatus},
    save_commands::{SaveCapture, SaveServices, SaveSession},
    scene::SceneServices,
    voice::{Gerokasu2VoicePlayer, MessageNotifications},
};
use crate::{
    asset::system::AssetServer, audio::GlobalAudio, layer::render_params::TransformParams,
    render::PreRenderContext, update::AdvUpdateContext,
};
use anyhow::Result;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::gerokasu2::Gerokasu2Scenario,
    format::scenario::info::{BgmId, SeId},
    format::scenario::instruction_elements::FromNumber,
    primitives::update::FrameId,
    time::Ticks,
    time::Tween,
    vm::command::types::{Pan, Volume},
    vm::gerokasu2::{Gerokasu2RuntimeCommand as Cmd, message_face::PortraitChange},
};
use shin_render::{PassKind, render_pass::RenderPass};
use std::{sync::Arc, time::Instant};
mod navigation;

pub enum SceneEvent {
    AutosaveCaptured(shin_core::format::save::gerokasu2::AutosaveMemory),
    SceneTitle(String),
    BgmChanged(i32),
    MessageInstalled(u32, PortraitChange),
    PortraitChanged(PortraitChange),
    MessageCompleted(u32),
    VoiceObserved(String),
}
struct Services {
    scene_title: String,
    exit: Option<super::exit_mode::ExitMode>,
    playback: super::playback::Playback,
    selection: Option<super::selection::SharedSelection>,
    selection_history: super::selection::SelectionHistory,
    interactive: Option<Arc<parking_lot::Mutex<CardSelectModel>>>,
    gallery: GalleryUnlocks,
    save: SaveSession,
    message_capture: Option<SaveCapture>,
    audio: Arc<parking_lot::Mutex<GlobalAudio>>,
    externally_updated_audio: bool,
    defer_audio_publication: bool,
    audio_restore_pending: bool,
    scenario: Arc<Gerokasu2Scenario>,
    backend: Box<BasicLoadBackend>,
    assets: Arc<AssetServer>,
    context: MessageCommandContext,
    loaded_settings: Option<shin_core::format::save::Settings>,
    started: Instant,
    events: Vec<SceneEvent>,
    resume_requested: bool,
}
impl Services {
    fn update_audio(&mut self) {
        if !self.externally_updated_audio {
            let mut audio = self.audio.lock();
            audio.bgm_player.update();
            audio.se_player.update();
            audio.system_player.update();
        }
    }
    fn play_selection_cues(&mut self) {
        let cues = self
            .selection
            .as_ref()
            .map_or_else(Vec::new, |s| s.lock().model.take_sounds());
        for cue in cues {
            self.audio
                .lock()
                .system_player
                .play(&self.assets, cue, Volume(1.0));
        }
    }
}
impl super::selection::SelectionServices for Services {
    fn begin_selection(&mut self, request: &super::selection::SelectionRequest) -> Result<()> {
        let owner = Arc::new(parking_lot::Mutex::new(
            super::selection::SelectionOwner::prepare(
                &self.assets,
                self.backend.message_resources()?,
                request,
                &self.selection_history,
            )?,
        ));
        self.backend.attach_selection(owner.clone());
        self.selection = Some(owner);
        Ok(())
    }
    fn selection_result(&mut self) -> Option<i32> {
        self.selection.as_ref().and_then(|s| s.lock().model.result)
    }
    fn record_selection_history(
        &mut self,
        r: &super::selection::SelectionRequest,
        result: i32,
    ) -> Result<()> {
        self.selection_history.record(r, result)
    }
    fn finish_selection(&mut self) {
        self.backend.remove_selection();
        self.selection = None;
    }
}
impl QuizServices for Services {
    fn bind_quiz(
        &mut self,
        target: Option<(
            shin_core::vm::command::types::PlaneId,
            shin_core::vm::command::types::LayerbankId,
        )>,
    ) -> Result<()> {
        self.interactive = target
            .and_then(|(plane, bank)| self.backend.layer(plane, bank))
            .and_then(|layer| match &layer.layer {
                GerDrawable::Card(card) => Some(card.inner_ref().model.clone()),
                _ => None,
            });
        if let Some(model) = &self.interactive {
            model.lock().listener = true;
        }
        Ok(())
    }
    fn quiz_result(&mut self) -> Result<Option<i32>> {
        Ok(self
            .interactive
            .as_ref()
            .map_or(Some(0), |m| m.lock().result()))
    }
    fn finish_quiz(&mut self) {
        self.interactive = None;
    }
}
impl MessageNotifications for Services {
    fn observed_voice(&mut self, name: &str) {
        self.events.push(SceneEvent::VoiceObserved(name.into()));
    }
    fn completed(&mut self, id: u32) {
        self.playback.complete_message(id);
        self.events.push(SceneEvent::MessageCompleted(id));
    }
}
impl GalleryServices for Services {
    fn unlock_gallery(&mut self, kind: u8, indices: &[i32]) -> Result<()> {
        self.gallery.unlock(kind, indices, false)
    }
}
impl SaveServices for Services {
    fn save_session(&mut self) -> &mut SaveSession {
        &mut self.save
    }
    fn message_started(&mut self) {
        self.message_capture = Some(SaveCapture::default());
    }
    fn packed_time(&mut self) -> Result<u64> {
        Ok(shin_core::format::save::gerokasu2::packed_local_time())
    }
    fn autosave_captured(
        &mut self,
        memory: &shin_core::format::save::gerokasu2::AutosaveMemory,
    ) -> Result<()> {
        self.events
            .push(SceneEvent::AutosaveCaptured(memory.clone()));
        Ok(())
    }
    fn show_scene_title(&mut self, title: &str) -> Result<()> {
        self.scene_title = title.to_owned();
        self.events.push(SceneEvent::SceneTitle(title.to_owned()));
        Ok(())
    }
}
fn fade(ticks: i32) -> Tween {
    Tween::linear(Ticks::from_f32(ticks as f32))
}
fn volume(value: f32) -> Volume {
    Volume(value.clamp(0.0, 2.0))
}
impl AudioCommands for Services {
    fn bgm_play(&mut self, id: i32, loops: i32, value: f32, time: i32) -> Result<bool> {
        let info = self
            .scenario
            .bgm_info
            .get(usize::try_from(id)?)
            .ok_or_else(|| anyhow::anyhow!("Invalid Ger BGM {id}"))?;
        let accepted = self.audio.lock().bgm_player.request_with_loop_count(
            self.assets.clone(),
            format!("/bgm/{}.nxa", info.name.as_str()),
            BgmId::from_number(id),
            loops as u32,
            volume(value),
            fade(time),
        );
        // Global38368..3838C is a producer even for restored requests. The
        // command-level notification only supplies title/UI cues.
        if accepted {
            self.gallery.bgms.add(id as usize);
            if info.linked_bgm_id >= 0 {
                self.gallery.bgms.add(info.linked_bgm_id as usize);
            }
        }
        Ok(accepted)
    }
    fn bgm_changed(&mut self, id: i32) -> Result<()> {
        self.events.push(SceneEvent::BgmChanged(id));
        Ok(())
    }
    fn bgm_stop(&mut self, time: i32) -> Result<()> {
        self.audio.lock().bgm_player.stop(fade(time));
        Ok(())
    }
    fn bgm_volume(&mut self, value: f32, time: i32) -> Result<()> {
        self.audio
            .lock()
            .bgm_player
            .set_volume(volume(value), fade(time));
        Ok(())
    }
    fn bgm_status(&self) -> i32 {
        self.audio.lock().bgm_player.get_wait_status().bits()
    }
    fn bgm_position(&self) -> u64 {
        self.audio.lock().bgm_player.position_micros()
    }
    fn se_play(
        &mut self,
        slot: i32,
        id: i32,
        no_repeat: i32,
        value: f32,
        pan: f32,
        speed: f32,
        time: i32,
    ) -> Result<()> {
        anyhow::ensure!(
            slot < crate::audio::SE_SLOT_COUNT as i32,
            "Invalid Ger SE slot {slot}"
        );
        let info = self
            .scenario
            .se_info
            .get(usize::try_from(id)?)
            .ok_or_else(|| anyhow::anyhow!("Invalid Ger SE {id}"))?;
        self.audio.lock().se_player.request(
            self.assets.clone(),
            format!("/se/{}.nxa", info.name.as_str()),
            slot,
            SeId::from_number(id),
            no_repeat == 0,
            volume(value),
            Pan(pan.clamp(-1.0, 1.0)),
            speed,
            fade(time),
        );
        Ok(())
    }
    fn se_stop(&mut self, slot: i32, time: i32) -> Result<()> {
        anyhow::ensure!(
            slot < crate::audio::SE_SLOT_COUNT as i32,
            "Invalid Ger SE slot {slot}"
        );
        self.audio.lock().se_player.stop(slot, fade(time));
        Ok(())
    }
    fn se_stop_all(&mut self, time: i32) -> Result<()> {
        self.audio.lock().se_player.stop_all(fade(time));
        Ok(())
    }
    fn se_volume(&mut self, slot: i32, value: f32, time: i32) -> Result<()> {
        anyhow::ensure!(
            slot < crate::audio::SE_SLOT_COUNT as i32,
            "Invalid Ger SE slot {slot}"
        );
        self.audio
            .lock()
            .se_player
            .set_volume(slot, volume(value), fade(time));
        Ok(())
    }
    fn se_pan(&mut self, slot: i32, value: f32, time: i32) -> Result<()> {
        anyhow::ensure!(
            slot < crate::audio::SE_SLOT_COUNT as i32,
            "Invalid Ger SE slot {slot}"
        );
        self.audio
            .lock()
            .se_player
            .set_panning(slot, Pan(value.clamp(-1.0, 1.0)), fade(time));
        Ok(())
    }
    fn se_status(&self, slot: i32) -> i32 {
        if slot < crate::audio::SE_SLOT_COUNT as i32 {
            self.audio.lock().se_player.get_wait_status(slot).bits()
        } else {
            0
        }
    }
    fn se_once_status(&self, slot: i32) -> i32 {
        self.audio
            .lock()
            .se_player
            .get_once_wait_status(slot)
            .bits()
    }
}
impl super::live_reconstruction::ReconstructionAudio for Services {
    fn prepare_effect_reconstruction(&mut self) -> Result<()> {
        self.audio.lock().se_player.prepare_restore();
        Ok(())
    }
}
impl RunnerServices for Services {
    fn command_blocked(&mut self) -> Result<()> {
        self.backend.command_blocked();
        Ok(())
    }
    fn begin_exit(&mut self) -> Result<bool> {
        self.exit = super::exit_mode::ExitMode::for_command(0);
        self.poll_exit_ready()?;
        self.playback.hidden_ui = false;
        self.playback.system_controls = false;
        self.playback.held_skip = false;
        self.playback.modes.reset();
        self.backend
            .root()
            .message_owner()
            .unwrap()
            .lock()
            .close_for_exit();
        let mut audio = self.audio.lock();
        audio.se_player.prepare_restore();
        audio.se_player.stop_all(Tween::IMMEDIATE);
        Ok(true)
    }
    fn poll_exit_ready(&mut self) -> Result<bool> {
        let gates = super::exit_mode::ExitGates {
            // Concrete hide-message and notification motion owners are not
            // installed yet; the current app has no such pending tasks.
            hide_motion_busy: false,
            message_busy: self
                .backend
                .root()
                .message_owner()
                .is_some_and(|m| m.lock().exit_busy()),
            tasks_idle: !self.backend.reconstruction_has_jobs(),
        };
        let Some(exit) = self.exit.as_mut() else {
            return Ok(true);
        };
        for action in exit.poll(gates) {
            if action == super::exit_mode::ExitAction::RequestSave {
                // Original F120 commits pending persistent variables before
                // requesting the global save task, never at Title publication.
                self.save.variables.commit()?;
            }
        }
        Ok(exit.phase() == super::exit_mode::ExitPhase::SaveQueued)
    }
    fn mask_services(&mut self) -> Option<&mut (dyn super::masks::MaskServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn resume_scene(&mut self) -> Result<bool> {
        self.resume_requested = true;
        Ok(true)
    }
    fn selection_services(
        &mut self,
    ) -> Option<&mut (dyn super::selection::SelectionServices + '_)> {
        Some(self)
    }
    fn movie_services(&mut self) -> Option<&mut (dyn MovieServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn quiz_services(&mut self) -> Option<&mut (dyn QuizServices + '_)> {
        Some(self)
    }
    fn gallery_services(&mut self) -> Option<&mut (dyn GalleryServices + '_)> {
        Some(self)
    }
    fn save_services(&mut self) -> Option<&mut (dyn SaveServices + '_)> {
        Some(self)
    }
    fn layer_edit_services(&mut self) -> Option<&mut (dyn LayerEditServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn audio_commands(&mut self) -> Option<&mut (dyn AudioCommands + '_)> {
        Some(self)
    }
    fn attach_message(&mut self, owner: SharedMessageOwner) -> Result<()> {
        self.backend.attach_message(owner)
    }
    fn layer_load_services(&mut self) -> Option<&mut (dyn LoadServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn scene_services(&mut self) -> Option<&mut (dyn SceneServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn layer_control_services(&mut self) -> Option<&mut (dyn LayerControlServices + '_)> {
        Some(self.backend.as_mut())
    }
    fn system_call(&mut self, id: i32, argument: i32) -> Result<bool> {
        self.playback.system_call(id, argument);
        Ok(true)
    }
    fn frame_policy(&mut self, mut policy: FramePolicy) -> FramePolicy {
        // Recompute after pumping: a new message or SYS command can change
        // the native gate during this frame's command loop.
        let blocked = self
            .selection
            .as_ref()
            .is_some_and(|s| s.lock().blocks_playback());
        policy.autoplay |= self.playback.autoplay(self.save.timeline.event, blocked);
        policy.fast_skip |= self.playback.skip(self.save.timeline.event, blocked);
        policy
    }
    fn command_context(&mut self, command: &Cmd, new: bool) -> Result<MessageCommandContext> {
        if new && let Cmd::MessageSet { msg_id, .. } = command {
            self.playback.enter_message(msg_id.0);
        }
        let blocked = self
            .selection
            .as_ref()
            .is_some_and(|s| s.lock().blocks_playback());
        let skip = self.context.skip || self.playback.skip(self.save.timeline.event, blocked);
        self.backend.set_frame(self.started, skip);
        Ok(MessageCommandContext {
            was_read: self.playback.was_read,
            event_mode: self.save.timeline.event,
            skip,
            ..self.context
        })
    }
    fn message_installed(&mut self, id: u32, portrait: PortraitChange) -> Result<()> {
        if let Some(mut capture) = self.message_capture.take() {
            let result = capture.poll(self);
            self.message_capture = Some(capture);
            result?;
        }
        self.events.push(SceneEvent::MessageInstalled(id, portrait));
        Ok(())
    }
    fn portrait_changed(&mut self, portrait: PortraitChange) -> Result<()> {
        self.events.push(SceneEvent::PortraitChanged(portrait));
        Ok(())
    }
    fn prepare_root_with_voice(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        allowed: bool,
        voice: &Gerokasu2VoicePlayer,
    ) -> Result<()> {
        if let Some(settings) = self.loaded_settings.as_ref() {
            if let Some(message) = self.backend.root().message_owner() {
                message.lock().message_mut_settings(settings.v10_skipspeed);
            }
        }
        let assets = self.assets.clone();
        let callback = |character, pan| voice.character_lipsync(character, pan);
        let ctx = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: delta,
            asset_server: &assets,
            device: &assets.context().wgpu_device,
            queue: &assets.context().wgpu_queue,
            are_animations_allowed: allowed,
            voice_callback: Some(&callback),
            movie_volume_settings: self.backend.movie_volume_settings(),
        };
        self.backend.prepare_message_frame(&ctx);
        Ok(())
    }
    fn prepare_root_fast_forward(&mut self) -> Result<()> {
        self.backend.prepare_root_fast_forward();
        Ok(())
    }
    fn advance_layers(&mut self, _: &Gerokasu2Layers, _: Ticks, _: bool) -> Result<()> {
        unreachable!("Scene uses presentation-frame updates")
    }
    fn advance_layers_with_voice(
        &mut self,
        frame: FrameId,
        _: &Gerokasu2Layers,
        delta: Ticks,
        allowed: bool,
        voice: &Gerokasu2VoicePlayer,
    ) -> Result<()> {
        let callback = |character, pan| voice.character_lipsync(character, pan);
        let assets = self.assets.clone();
        let ctx = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: delta,
            asset_server: &assets,
            device: &assets.context().wgpu_device,
            queue: &assets.context().wgpu_queue,
            are_animations_allowed: allowed,
            voice_callback: Some(&callback),
            movie_volume_settings: self.backend.movie_volume_settings(),
        };
        self.backend.update(&ctx);
        if let Some(exit) = self.exit.as_mut() {
            exit.advance_clock(delta.as_f32());
        }
        Ok(())
    }
    fn fast_forward_layers(&mut self, _: &Gerokasu2Layers) -> Result<()> {
        // prepare_root_fast_forward already forwarded the native root tree
        // before the runner advanced its message clock.
        Ok(())
    }
}

pub struct Gerokasu2PlayScene {
    runner: Box<Gerokasu2Runner>,
    services: Services,
    navigation: Option<Box<navigation::Navigation>>,
}
impl Gerokasu2PlayScene {
    pub(crate) fn is_exiting(&self) -> bool {
        self.services.exit.is_some()
    }
    pub(crate) fn focus_policy_ready(&self) -> bool {
        !self.services.backend.reconstruction_has_jobs() && self.navigation.is_none()
    }
    pub(crate) fn exit_save_completed(&mut self) -> Vec<super::exit_mode::ExitAction> {
        self.services
            .exit
            .as_mut()
            .map_or_else(Vec::new, |e| e.save_completed())
    }
    /// Keep background save hydration silent until its destination is ready.
    /// Standalone scenes retain their existing immediate publication contract.
    pub(crate) fn defer_audio_publication(&mut self) {
        self.services.defer_audio_publication = true;
    }

    pub(crate) fn publish_global_audio(
        &mut self,
        audio: Arc<parking_lot::Mutex<GlobalAudio>>,
    ) -> Result<()> {
        // Validate every restored ID before attaching or touching the audible
        // source. A rejected destination must leave the retained menu usable.
        self.check_audio_publication()?;
        self.services.audio = audio;
        self.services.externally_updated_audio = true;
        self.services.defer_audio_publication = false;
        if let Some(settings) = self.services.loaded_settings.as_ref() {
            let mut audio = self.services.audio.lock();
            audio.bgm_player.set_settings_volume(settings.v0_bgmvol);
            audio.se_player.set_settings_volume(settings.v1_sfxvol);
            audio.system_player.set_settings_volume(settings.v3_sysvol);
        }
        if self.services.audio_restore_pending {
            let descriptors = self.services.save.descriptors.clone();
            super::live_reconstruction::restore_audio(&descriptors, &mut self.services)?;
            self.services.audio_restore_pending = false;
        }
        Ok(())
    }
    pub(crate) fn check_audio_publication(&self) -> Result<()> {
        if self.services.audio_restore_pending {
            super::live_reconstruction::check_audio_metadata(
                &self.services.save.descriptors,
                self.services.scenario.bgm_info.len(),
                self.services.scenario.se_info.len(),
            )?;
        }
        Ok(())
    }
    pub fn import_save_globals(&mut self, data: &shin_core::format::save::gerokasu2::SaveData) {
        use shin_core::format::save::index_set::IndexSet;
        let vectors = &data.save_vectors;
        self.services
            .save
            .variables
            .replace_committed(data.persist_data.clone());
        self.services.playback.seen_messages =
            IndexSet::from_raw_words(vectors.seen_messages.clone());
        self.services.playback.disallow_skip_unread = data.settings.v11_disallowskipunread;
        self.services.selection_history = super::selection::SelectionHistory {
            seen: IndexSet::from_raw_words(vectors.select_seen_choices.clone()),
            last: vectors.select_last_choices.clone(),
        };
        self.services.gallery = GalleryUnlocks {
            cgs: IndexSet::from_raw_words(vectors.unlocked_cgs.clone()),
            bgms: IndexSet::from_raw_words(vectors.unlocked_bgms.clone()),
            movies: IndexSet::from_raw_words(vectors.unlocked_movies.clone()),
        };
        self.apply_settings(&data.settings);
    }
    /// Native 258F0 system cue at volume 1.0 (ADV playback-mode feedback).
    pub fn play_system_cue(&mut self, cue: &str) {
        self.services
            .audio
            .lock()
            .system_player
            .play(&self.services.assets, cue, Volume(1.0));
    }
    /// Live Config must update preferences without importing older progress,
    /// persistent variables or read/gallery vectors from the desktop store.
    pub fn apply_settings(&mut self, settings: &shin_core::format::save::Settings) {
        self.services.playback.disallow_skip_unread = settings.v11_disallowskipunread;
        self.services.context.message_speed = settings.v9_msgspeed;
        self.services
            .audio
            .lock()
            .bgm_player
            .set_settings_volume(settings.v0_bgmvol);
        self.services
            .audio
            .lock()
            .se_player
            .set_settings_volume(settings.v1_sfxvol);
        self.services
            .audio
            .lock()
            .system_player
            .set_settings_volume(settings.v3_sysvol);
        self.runner.voice_mut().set_preferences(
            settings.v2_voicevol,
            settings.v19,
            settings.v5_voicepanapot,
        );
        self.services
            .backend
            .set_movie_volume_settings(settings.into());
        self.runner
            .message_owner()
            .lock()
            .ui_settings
            .window_opacity = settings.v13_msgwinalpha;
        self.services.loaded_settings = Some(settings.clone());
    }
    pub fn scene_title(&self) -> &str {
        &self.services.scene_title
    }
    pub fn system_menu_allowed(&self) -> bool {
        self.navigation.is_none()
            && !self.is_exiting()
            && self.services.playback.system_controls
            && self.services.save.timeline.event < 0
            && (self.selection_ready()
                || self
                    .runner
                    .message_owner()
                    .lock()
                    .message()
                    .is_some_and(|m| {
                        let c = m.clock();
                        c.open && c.slide >= 1.0 && !c.is_finished()
                    }))
    }
    pub fn set_menu_paused(&mut self, paused: bool) {
        self.runner.voice_mut().set_paused(paused);
        self.services.audio.lock().se_player.set_paused(paused);
    }
    pub fn pause_menu_effects(&mut self, paused: bool) {
        self.services.audio.lock().se_player.set_paused(paused);
    }
    pub fn step_menu(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        suspended: bool,
        paused: bool,
    ) -> Result<RunnerStatus> {
        // Original10A38 gates VM and10DCC gates *all* root/message updates
        // while pause_depth!=0. The independently advanced parent backdrop is
        // outside these gates. Final14194 releases clocks in the same frame.
        if paused || suspended {
            self.services.update_audio();
            return Ok(RunnerStatus::Waiting);
        }
        self.step(
            frame,
            delta,
            FramePolicy {
                paused: false,
                scene_changing: false,
                animations_allowed: true,
                autoplay: false,
                fast_skip: false,
            },
            self.services.context,
        )
    }
    pub fn export_save_globals(&self, data: &mut shin_core::format::save::gerokasu2::SaveData) {
        data.persist_data = self.persistent().clone();
        data.save_vectors = shin_core::format::save::gerokasu2::SaveVectors {
            seen_messages: self.services.playback.seen_messages.raw_words().to_vec(),
            select_seen_choices: self.services.selection_history.seen.raw_words().to_vec(),
            select_last_choices: self.services.selection_history.last.clone(),
            unlocked_cgs: self.services.gallery.cgs.raw_words().to_vec(),
            unlocked_bgms: self.services.gallery.bgms.raw_words().to_vec(),
            unlocked_movies: self.services.gallery.movies.raw_words().to_vec(),
        };
    }
    /// Immediate publication of a memory replay. Modal/wipe navigation uses a
    /// separate reconstruction mode; prepare failures retain the current scene.
    pub async fn reconstruct_replay(
        &mut self,
        replay: &mut super::replay::ReplaySession,
    ) -> Result<()> {
        self.reconstruct_replay_with_display(replay, false, || false)
            .await
    }
    /// Load publication clears the displayed MessageLayer (original mode0).
    /// Its retained MSGSET still installs text only on the first live poll.
    pub async fn reconstruct_replay_for_load(
        &mut self,
        replay: &mut super::replay::ReplaySession,
    ) -> Result<()> {
        self.reconstruct_replay_with_display(replay, true, || false)
            .await
    }
    pub(crate) async fn reconstruct_replay_for_load_cancellable(
        &mut self,
        replay: &mut super::replay::ReplaySession,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        self.reconstruct_replay_with_display(replay, true, cancelled)
            .await
    }
    async fn reconstruct_replay_with_display(
        &mut self,
        replay: &mut super::replay::ReplaySession,
        clear_display: bool,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        replay.check_publication(&self.services.scenario)?;
        let mut prepared = self
            .services
            .backend
            .prepare_reconstruction(
                &replay.layers,
                &replay.save.descriptors,
                !clear_display,
                &cancelled,
            )
            .await?;
        anyhow::ensure!(
            !cancelled(),
            "Ger reconstruction cancelled before live publication"
        );
        self.runner.voice_mut().stop();
        if self.services.defer_audio_publication {
            self.services.audio_restore_pending = true;
        } else if let Err(error) =
            super::live_reconstruction::restore_audio(&replay.save.descriptors, &mut self.services)
        {
            self.runner.require_publication_retry(&error);
            return Err(error);
        }
        self.services.backend.retire_reconstruction_jobs();
        self.install_reconstruction(replay, prepared, clear_display);
        Ok(())
    }
    fn install_reconstruction(
        &mut self,
        replay: &mut super::replay::ReplaySession,
        mut prepared: Box<BasicLoadBackend>,
        clear_display: bool,
    ) {
        if let Some(properties) = prepared.reconstruction_root_mut().message_properties_mut() {
            super::reconstruction::apply_properties(
                properties,
                replay.save.descriptors.message_properties,
            );
        }
        self.services.playback.modes.reset();
        self.services.playback.held_skip = false;
        self.services.playback.system_controls = replay.save.descriptors.system_controls;
        self.services.playback.was_read = replay.save.descriptors.current_was_read != 0;
        self.services.selection = None;
        self.services.interactive = None;
        self.services.message_capture = None;
        {
            let message = self.runner.message_owner();
            let mut message = message.lock();
            message.restore_descriptor(&replay.save.descriptors.message);
            if clear_display {
                message.clear_for_reconstruction();
            }
        }
        let (vm, layers, save, pending, response) = replay.take_live_parts();
        self.runner
            .install_replayed_parts(vm, layers, pending, response);
        self.services.context.event_mode = save.timeline.event;
        self.services.save = save;
        self.services.scene_title = self.services.save.metadata.saved()[1].to_string();
        self.services.backend = prepared;
        for (kind, params) in self.services.backend.take_loaded_notifications() {
            self.services
                .gallery
                .loaded_layer(&self.services.scenario, kind, params, false);
        }
    }
    /// Host captures the outgoing composed frame before this owner's worker.
    pub fn begin_reconstruction(
        &mut self,
        replay: super::replay::ReplaySession,
        mode: u32,
    ) -> Result<()> {
        anyhow::ensure!(
            self.navigation.is_none(),
            "Ger reconstruction already active"
        );
        anyhow::ensure!(
            replay.vm.matches_scenario(&self.services.scenario),
            "Ger reconstruction scenario mismatch"
        );
        if mode != 0 {
            self.services.backend.screen_mut().discard_pending_page();
        }
        self.services.backend.set_animations_allowed(false);
        self.navigation = Some(Box::new(navigation::Navigation::new(replay, mode)));
        Ok(())
    }
    pub fn reconstruction_phase(&self) -> Option<super::reconstruction_mode::Phase> {
        self.navigation.as_ref().map(|owner| owner.mode.phase())
    }
    fn poll_reconstruction(&mut self, delta: Ticks, paused: bool) -> Result<()> {
        let Some(mut owner) = self.navigation.take() else {
            return Ok(());
        };
        let result = (|| {
            owner.poll_worker()?;
            if owner.mode.phase() == super::reconstruction_mode::Phase::Preparing {
                owner.clock.mask_present = owner.ready.as_ref().is_some_and(|p| p.mask.is_some());
            }
            let ready = u32::from(owner.ready.is_some());
            owner.mode.poll(
                ready,
                &mut owner.clock,
                &mut navigation::Publication {
                    scene: self,
                    ready: &mut owner.ready,
                    overlay: &mut owner.overlay,
                },
            )?;
            if owner.mode.phase() != super::reconstruction_mode::Phase::Preparing {
                owner.release_published_retirement();
            }
            if !paused {
                owner.clock.advance(delta);
            }
            if let Some(overlay) = owner.overlay.as_mut() {
                overlay.clock = owner.clock.clone();
            }
            Ok(())
        })();
        if result.is_err() || owner.mode.phase() != super::reconstruction_mode::Phase::Complete {
            self.navigation = Some(owner);
        }
        result
    }
    #[cfg(test)]
    pub(crate) fn reference_audio(&self) -> parking_lot::MutexGuard<'_, GlobalAudio> {
        self.services.audio.lock()
    }
    #[cfg(test)]
    pub(crate) fn reference_audio_mut(&mut self) -> parking_lot::MutexGuard<'_, GlobalAudio> {
        self.services.audio.lock()
    }
    /// Bootstrap and settings remain caller inputs until native title/save/input
    /// integration resolves them. Unsupported commands remain retained owners.
    pub async fn load(
        assets: Arc<AssetServer>,
        scenario: Arc<Gerokasu2Scenario>,
        audio: Arc<AudioManager>,
        init: i32,
        seed: u32,
        initial: InitialMessage,
        layers: Gerokasu2Layers,
        context: MessageCommandContext,
    ) -> Result<Self> {
        let mut backend = Box::new(BasicLoadBackend::new(
            assets.clone(),
            scenario.clone(),
            LoadCounters::from_layers(&layers),
        ));
        backend.attach_movie_audio(audio.clone());
        let descriptors =
            super::descriptors::Descriptors::new(initial.style, initial.asynchronous, initial.face);
        let runner = Gerokasu2Runner::load(
            assets.clone(),
            scenario.clone(),
            audio.clone(),
            init,
            seed,
            initial,
            layers,
        )
        .await?;
        backend.attach_message(runner.message_owner())?;
        let mut save = SaveSession::fresh(init, seed);
        save.descriptors = Box::new(descriptors);
        save.timeline.event = context.event_mode;
        Ok(Self {
            runner: Box::new(runner),
            navigation: None,
            services: Services {
                scene_title: String::new(),
                exit: None,
                playback: Default::default(),
                selection: None,
                selection_history: Default::default(),
                interactive: None,
                gallery: GalleryUnlocks::default(),
                save,
                message_capture: None,
                audio: Arc::new(parking_lot::Mutex::new(GlobalAudio::new(audio))),
                externally_updated_audio: false,
                defer_audio_publication: false,
                audio_restore_pending: false,
                scenario,
                backend,
                assets,
                context,
                loaded_settings: None,
                started: Instant::now(),
                events: Vec::new(),
                resume_requested: false,
            },
        })
    }
    pub fn runner(&self) -> &Gerokasu2Runner {
        &self.runner
    }
    pub fn save_session(&self) -> &SaveSession {
        &self.services.save
    }
    pub fn gallery(&self) -> &GalleryUnlocks {
        &self.services.gallery
    }
    /// Loaded global gallery bits have a lifetime separate from progress entries.
    pub fn replace_gallery(&mut self, gallery: GalleryUnlocks) {
        self.services.gallery = gallery;
    }
    pub fn playback(&self) -> &super::playback::Playback {
        &self.services.playback
    }
    pub fn playback_mut(&mut self) -> &mut super::playback::Playback {
        &mut self.services.playback
    }
    pub fn assets(&self) -> &Arc<AssetServer> {
        &self.services.assets
    }
    pub fn scenario(&self) -> &Arc<Gerokasu2Scenario> {
        &self.services.scenario
    }
    /// Native11B00 skip predicate, as consumed by parent-owned clocks such as
    /// the save/BGM title notifications (11014..11028).
    pub fn skip_active(&self) -> bool {
        self.services
            .playback
            .skip(self.services.save.timeline.event, self.playback_blocked())
    }
    /// Native11950 autoplay predicate (the auto indicator's pulse).
    pub fn autoplay_active(&self) -> bool {
        self.services
            .playback
            .autoplay(self.services.save.timeline.event, self.playback_blocked())
    }
    /// The interactive overlay withholds playback modes.
    pub fn playback_blocked(&self) -> bool {
        self.services
            .selection
            .as_ref()
            .is_some_and(|s| s.lock().blocks_playback())
    }
    pub fn persistent(&self) -> &shin_core::format::save::PersistData {
        self.services.save.variables.committed()
    }
    pub fn replace_persistent(&mut self, value: shin_core::format::save::PersistData) {
        self.services.save.variables.replace_committed(value);
    }
    pub fn runner_mut(&mut self) -> &mut Gerokasu2Runner {
        &mut self.runner
    }
    pub fn backend(&self) -> &BasicLoadBackend {
        &self.services.backend
    }
    pub fn backend_mut(&mut self) -> &mut BasicLoadBackend {
        &mut self.services.backend
    }
    pub fn take_events(&mut self) -> Vec<SceneEvent> {
        std::mem::take(&mut self.services.events)
    }
    pub fn step(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        mut policy: FramePolicy,
        context: MessageCommandContext,
    ) -> Result<RunnerStatus> {
        if self.navigation.is_some() {
            self.poll_reconstruction(delta, policy.paused)?;
            if self.navigation.is_some() {
                // Native10DCC still updates the root with ADV4D2=false while
                // preparing, thentrue after publication. VM pumping is gated.
                policy.scene_changing = true;
                self.runner
                    .advance(frame, delta, policy, &mut self.services)?;
                self.services.update_audio();
                return Ok(RunnerStatus::Waiting);
            }
            // Native10A20 polls modes before10A48 gates the command loop.
            // A completed immediate RESUME pumps its rebuilt VM in this frame.
        }
        self.services.started = Instant::now();
        self.services.context = context;
        if let Some(settings) = self.services.loaded_settings.as_ref() {
            self.services.context.message_speed = settings.v9_msgspeed;
        }
        let result = self
            .runner
            .update(frame, delta, policy, 256, 100000, &mut self.services);
        // Publication has already happened even if a following owner fails.
        for (kind, params) in self.services.backend.take_loaded_notifications() {
            self.services
                .gallery
                .loaded_layer(&self.services.scenario, kind, params, false);
        }
        let status = result?;
        if std::mem::take(&mut self.services.resume_requested) {
            let position = self.services.save.timeline.resume_position;
            let (save, restored) = self.services.save.fork_for_restore(position, position);
            let replay =
                super::replay::ReplaySession::new(self.services.scenario.clone(), save, restored)?;
            self.begin_reconstruction(replay, 3)?;
        }
        self.services.play_selection_cues();
        self.services.update_audio();
        Ok(status)
    }
    pub fn selection(&self) -> Option<super::selection::SharedSelection> {
        self.services.selection.clone()
    }
    pub fn selection_history(&self) -> &super::selection::SelectionHistory {
        &self.services.selection_history
    }
    pub fn selection_ready(&self) -> bool {
        self.services
            .selection
            .as_ref()
            .is_some_and(|s| s.lock().ready())
    }
    pub fn selection_input(
        &mut self,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        vertical: bool,
        confirm: bool,
    ) {
        if let Some(s) = &self.services.selection {
            s.lock().input(up, down, left, right, vertical, confirm);
        }
        self.services.play_selection_cues();
    }
    pub fn selection_pointer(&mut self, position: glam::Vec2, held: bool) {
        if let Some(s) = &self.services.selection {
            s.lock().pointer(position, held);
        }
        self.services.play_selection_cues();
    }
    pub fn selection_pointer_cancel(&mut self) {
        if let Some(s) = &self.services.selection {
            s.lock().cancel_pointer();
        }
    }
    pub fn card_ready(&self) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().ready())
    }
    /// Index into the displayed cards, whose returned face IDs preserve the mask.
    pub fn select_card(&mut self, index: usize) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().select(index))
    }
    pub fn confirm_card(&mut self) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().confirm())
    }
    pub fn navigate_card(&mut self, direction: i32) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().navigate(direction))
    }
    /// Widget canvas coordinates; App-level coordinate/input routing remains separate.
    pub fn card_pointer_move(&mut self, position: glam::Vec2) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().pointer_move(position))
    }
    pub fn card_pointer_press(&mut self) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().pointer_press())
    }
    pub fn card_pointer_release(&mut self) -> bool {
        self.services
            .interactive
            .as_ref()
            .is_some_and(|m| m.lock().pointer_release())
    }
    pub fn card_pointer_cancel(&mut self) {
        if let Some(m) = &self.services.interactive {
            m.lock().pointer_cancel();
        }
    }
    pub fn click_message(&mut self, keep_voice: bool) -> Result<bool> {
        self.runner.click_message(keep_voice, &mut self.services)
    }
    pub fn pre_render(
        &mut self,
        ctx: &mut PreRenderContext,
        transform: &TransformParams,
    ) -> Result<()> {
        self.services.backend.pre_render_scene(ctx, transform)?;
        if self
            .navigation
            .as_ref()
            .is_some_and(|owner| owner.needs_start())
        {
            let mut owner = self.navigation.take().unwrap();
            if owner.mode.mode != 3 {
                let mut snapshot = ctx.new_render_texture("Ger reconstruction snapshot".into());
                {
                    let mut pass = ctx.begin_pass(
                        snapshot.as_texture_target(),
                        Some(ctx.depth_stencil),
                        "Ger reconstruction snapshot",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    for kind in [PassKind::Opaque, PassKind::Transparent] {
                        self.services
                            .backend
                            .render_scene(&mut pass, transform, 0, kind)?;
                    }
                }
                owner.overlay = Some(super::reconstruction_mode::overlay::Overlay {
                    clock: owner.clock.clone(),
                    snapshot,
                    mask: None,
                });
            }
            let source = self
                .services
                .backend
                .reconstruction_source(&mut ctx.render_clone_ctx());
            owner.start(source, self.services.playback.seen_messages.clone());
            self.navigation = Some(owner);
        }
        Ok(())
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        self.services
            .backend
            .render_scene(pass, transform, stencil, kind)?;
        if kind == PassKind::Transparent {
            if let Some(overlay) = self.navigation.as_ref().and_then(|o| o.overlay.as_ref()) {
                overlay.render(pass);
            }
        }
        Ok(())
    }
}
