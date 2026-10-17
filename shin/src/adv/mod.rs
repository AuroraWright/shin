pub mod assets;
mod backlog;

mod bgm_title;
pub mod characters;
mod choice;
mod command;
pub(crate) mod scrollbar;
#[cfg(test)]
pub(crate) use command::apply_command_state_and_start;
#[cfg(test)]
pub(crate) use command::apply_command_state_and_start_with_skip;
mod chapter_navigation;
pub mod config;
mod event_skip;
mod story_touch;
pub(crate) use dialog::view::DialogText;
mod gallery;
pub mod gerokasu2;
pub mod higurashi;
pub mod information;
pub mod input_area;
pub mod manual;
pub mod modal_backdrop;
pub(crate) mod overlay_clock;
pub(crate) mod persistence;
pub(crate) use persistence::{SaveRecovery, SaveRecoveryKind};
pub mod play_clock;
mod playback;
mod reconstruction;
mod restoration;
mod route_label;
pub(crate) mod save_game;
pub mod save_menu;
mod skip_indicator;
pub(crate) use save_menu::dialog::SaveMenuDialog;
pub(crate) mod scene_load_fade;
pub(crate) mod storage_retry_dialog;
pub mod system_menu;
pub mod tips;
mod title_return;
mod vm_state;

use std::{borrow::Cow, sync::Arc};

pub use command::{CommandStartResult, ExecutingCommand, StartableCommand, UpdatableCommand};
use egui::Window;
use enum_map::{Enum, EnumMap, enum_map};
use futures::FutureExt as _;
use glam::Mat4;
use itertools::Itertools;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{Scenario, instruction_elements::CodeAddress},
    primitives::color::UnormColor,
    vm::{
        Scripter,
        breakpoint::BreakpointObserver,
        command::{
            CommandResult,
            types::{LayerId, PLANES_COUNT, PlaneId, VLayerId, VLayerIdRepr},
        },
    },
};
use shin_input::{Action, ActionState, inputs::MouseButton};
use shin_render::{
    render_pass::RenderPass,
    shaders::types::{RenderClone as _, RenderCloneCtx},
};
use smallvec::{SmallVec, smallvec};
use tracing::{debug, warn};
use vm_state::layers::ITER_VLAYER_SMALL_VECTOR_SIZE;
pub use vm_state::{VmState, layers::LayerSelection};
mod global_data;
#[cfg(test)]
pub(crate) mod reference_tests;
mod dialog;
pub(crate) use global_data::GlobalData;
use winit::keyboard::KeyCode;

use crate::{
    adv::assets::AdvAssets,
    app::AppAction,
    audio::{GlobalAudio, GlobalAudioSlot, VoicePlayer},
    layer::{
        AnyLayer, AnyLayerMut, DrawableLayer as _, Layer as _, LayerGroup, PageLayer,
        RootLayerGroup, ScreenLayer, message_layer::MessageLayer, render_layer_without_bg,
        render_params::TransformParams, user::UserLayer,
    },
    render::{
        PreRenderContext,
        overlay::{OverlayCollector, OverlayVisitable},
    },
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

/// Actions available in all ADV contexts
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Enum)]
pub enum AdvMessageAction {
    Advance,
    HoldFastForward,
    Backlog,
    Rollback,
}

// impl Action for AdvMessageAction {
//     fn default_action_map() -> ActionMap<Self> {
//         fn map(v: AdvMessageAction) -> InputSet {
//             match v {
//                 AdvMessageAction::Advance => [
//                     MouseButton::Left.into(),
//                     KeyCode::Enter.into(),
//                     KeyCode::Space.into(),
//                 ]
//                 .into_iter()
//                 .collect(),
//                 AdvMessageAction::HoldFastForward => {
//                     [KeyCode::ControlLeft.into()].into_iter().collect()
//                 }
//                 AdvMessageAction::Backlog => [].into_iter().collect(),
//                 AdvMessageAction::Rollback => [].into_iter().collect(),
//             }
//         }
//
//         ActionMap::new(enum_map! { v => map(v) })
//     }
// }

type RestoredScene = (
    restoration::LoadedScene,
    Box<reconstruction::ReconstructedScene>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneLoadKind {
    Saved,
    History,
    Resume,
}

struct PendingSceneRestore {
    task: Option<shin_tasks::AsyncTask<anyhow::Result<RestoredScene>>>,
    ready: Option<anyhow::Result<RestoredScene>>,
    retirement: Option<shin_tasks::LoaderRetirement>,
    kind: SceneLoadKind,
    source: Option<shin_render::render_texture::RenderTexture>,
    present_source: bool,
}

impl Drop for PendingSceneRestore {
    fn drop(&mut self) {
        let ready = self.ready.take();
        if let Some(task) = self.task.take() {
            drop(ready);
            // Native 19bd0 signals cancellation and waits before retiring the
            // loading task's resources and VM. First cancel its future, then
            // drain child IO/compute work from this caller. The IO executor must
            // stay free while a Movie worker waits for FFmpeg's first frame.
            #[cfg(not(target_arch = "wasm32"))]
            task.cancel_and_retire();
            #[cfg(target_arch = "wasm32")]
            drop(task);
        } else if let Some(retirement) = self.retirement.take() {
            #[cfg(not(target_arch = "wasm32"))]
            retirement.retire(ready);
            #[cfg(target_arch = "wasm32")]
            drop((retirement, ready));
        }
    }
}

enum SavePurpose {
    Manual {
        game: Option<shin_core::format::save::GameData>,
    },
    Config,
    TitleReturn,
    Exit,
}
struct PendingSaveTask {
    task: save_game::SaveGameTask,
    purpose: SavePurpose,
}

#[cfg(test)]
pub(crate) fn reference_saved_backlog_latency<'a>(
    server: &'a Arc<crate::asset::system::AssetServer>,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
) {
    backlog::screen::reference_saved_backlog_latency(server, context);
}

pub struct Adv {
    assets: AdvAssets,
    persistence: Option<persistence::SaveStore>,
    pending_save: Option<PendingSaveTask>,
    progress_error: Option<storage_retry_dialog::StorageRetryDialog>,
    exit_save_finished: bool,
    volatile_save: shin_core::format::save::SaveData,
    globals_detached: bool,
    play_clock: play_clock::PlayClock,
    pending_restore: Option<PendingSceneRestore>,
    restore_error: Option<String>,
    restore_error_dialog: Option<storage_retry_dialog::StorageRetryDialog>,
    retry_live_command_after_error: bool,
    previous_rollback_position: Option<u32>,
    pending_rollback_position: Option<u32>,
    pending_result: shin_core::vm::command::CommandResult,
    scenario: Arc<Scenario>,
    scripter: Scripter,
    vm_state: VmState,
    adv_state: AdvState,
    // action_state: ActionState<AdvMessageAction>,
    current_command: Option<ExecutingCommand>,
    finished: bool,
    character_scroll_axis: f32,
    interactive_axes: glam::Vec2,
    pending_backlog: bool,
    pending_system_menu: bool,
    system_menu: Option<system_menu::modal::SystemMenuModal>,
    modal_pointer: input_area::PointerSampler,
    modal_pointer_extent: (u16, u16),
    modal_pointer_held: bool,
    last_desktop_browser_phase: Option<&'static str>,
    hidden_tap_input: input_area::TapInput,
    pending_previous_scene: bool,
    pending_chapter: Option<chapter_navigation::Direction>,
    chapter_dialog: Option<(chapter_navigation::Direction, SaveMenuDialog)>,
    pending_playback_sound: Option<&'static str>,
    held_skip: bool,
    event_skip: event_skip::EventSkip,
    story_touch: story_touch::StoryTouch,
    pending_event_skip: bool,
}

impl Adv {
    pub fn new(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<crate::asset::system::AssetServer>,
        assets: AdvAssets,
        scripter: Scripter,
    ) -> Self {
        Self::create(audio_manager, asset_server, assets, scripter, None)
    }
    pub(crate) fn new_with_audio(
        asset_server: Arc<crate::asset::system::AssetServer>,
        assets: AdvAssets,
        scripter: Scripter,
        audio: GlobalAudio,
    ) -> Self {
        Self::create(
            audio.manager.clone(),
            asset_server,
            assets,
            scripter,
            Some(audio),
        )
    }
    fn create(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<crate::asset::system::AssetServer>,
        assets: AdvAssets,
        scripter: Scripter,
        retained_audio: Option<GlobalAudio>,
    ) -> Self {
        let scenario = assets.scenario.clone();
        let retained_assets = assets.clone();
        let (scenario_id, random_seed) = scripter.initial_values();
        let vm_state = VmState::for_scenario(scenario_id, random_seed);
        let adv_state =
            AdvState::new_with_audio(audio_manager, asset_server, assets, retained_audio);

        Self {
            assets: retained_assets,
            persistence: None,
            pending_save: None,
            progress_error: None,
            exit_save_finished: false,
            volatile_save: Default::default(),
            globals_detached: false,
            play_clock: play_clock::PlayClock::default(),
            pending_restore: None,
            restore_error: None,
            restore_error_dialog: None,
            retry_live_command_after_error: false,
            previous_rollback_position: None,
            pending_rollback_position: None,
            pending_result: shin_core::vm::command::CommandResult::None,
            scenario,
            scripter,
            vm_state,
            adv_state,
            current_command: None,
            finished: false,
            character_scroll_axis: 0.0,
            interactive_axes: glam::Vec2::ZERO,
            pending_backlog: false,
            pending_system_menu: false,
            system_menu: None,
            last_desktop_browser_phase: None,
            modal_pointer: Default::default(),
            modal_pointer_extent: (1920, 1080),
            modal_pointer_held: false,
            hidden_tap_input: Default::default(),
            pending_previous_scene: false,
            pending_chapter: None,
            chapter_dialog: None,
            pending_playback_sound: None,
            held_skip: false,
            event_skip: event_skip::EventSkip::new(),
            story_touch: story_touch::StoryTouch::new(),
            pending_event_skip: false,
        }
    }

    /// Called by the app every frame, outside story/loading/menu input gates.
    pub fn advance_play_clock(&mut self, delta: shin_core::time::Ticks) {
        self.play_clock.advance(delta.as_f32());
        if let Some(store) = &mut self.persistence {
            store.data.play_seconds = self.play_clock.saved_seconds();
        }
    }
    /// Restore the app-global clock once at startup; scene/history loads leave it intact.
    pub fn restore_play_clock(&mut self, seconds: u32) {
        self.play_clock = play_clock::PlayClock::from_saved_seconds(seconds);
        if let Some(store) = &mut self.persistence {
            store.data.play_seconds = seconds;
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_global_save(&self) -> shin_core::format::save::SaveData {
        self.save_menu_data()
    }
    #[cfg(test)]
    pub(crate) fn reference_entry(&self) -> shin_core::format::save::GameDataEntry {
        self.vm_state.snapshot_entry()
    }
    #[cfg(test)]
    pub(crate) fn reference_scene_reveal(&self) -> bool {
        self.adv_state.scene_load_fade.is_some()
    }
    #[cfg(test)]
    pub(crate) fn reference_msgset_states(&mut self) -> (&mut VmState, &mut AdvState) {
        (&mut self.vm_state, &mut self.adv_state)
    }
    #[cfg(test)]
    pub(crate) fn adv_state_audio(&self) -> &GlobalAudio {
        &self.adv_state.audio
    }
    pub fn play_ticks(&self) -> f64 {
        self.play_clock.ticks()
    }
    pub(crate) fn suspend_shutdown_input(&mut self, held: bool) {
        self.modal_pointer.cancel_contact(held);
        self.cancel_event_contact();
        if let Some(menu) = &mut self.system_menu {
            menu.suspend_pointer(held);
        }
        if let Some(backlog) = &mut self.adv_state.backlog {
            backlog.suspend_pointer(held);
        }
        if let Some(dialog) = &mut self.progress_error {
            dialog.suspend_pointer(held);
        }
        if let Some(dialog) = &mut self.restore_error_dialog {
            dialog.suspend_pointer(held);
        }
        if let Some(save) = &mut self.pending_save {
            save.task.suspend_pointer(held);
        }
    }
    pub(crate) fn has_save_task(&self) -> bool {
        self.pending_save.is_some()
    }
    pub(crate) fn flush_shutdown(&mut self) -> anyhow::Result<()> {
        if let Some(store) = &mut self.persistence {
            store.flush_final(&self.vm_state)?;
        }
        Ok(())
    }
    pub(crate) fn run_in_background(&self) -> bool {
        self.vm_state.settings.v18_showsongtitle
    }
    pub(crate) fn touch_effects_enabled(&self) -> bool {
        self.vm_state.settings.v17_showscenetitle
    }

    pub fn configure_save_file(
        &mut self,
        path: std::path::PathBuf,
        initial: Option<shin_core::format::save::SaveData>,
    ) -> anyhow::Result<()> {
        let store = persistence::SaveStore::open(path, initial)?;
        self.restore_play_clock(store.data.play_seconds);
        self.vm_state.settings = store.data.settings.clone();
        self.vm_state.persist = store.data.persist_data.clone();
        self.vm_state.save_vectors = store.data.save_vectors.clone();
        self.vm_state.auto_save_slot = store.data.auto_save_slot.clone();
        self.persistence = Some(store);
        Ok(())
    }

    /// Native globals outlive scene reconstruction. Manual slots remain available
    /// in memory even when this host has not configured a disk save file.
    fn save_menu_data(&self) -> shin_core::format::save::SaveData {
        let mut data = self
            .persistence
            .as_ref()
            .map(|store| store.data.clone())
            .unwrap_or_else(|| self.volatile_save.clone());
        data.settings = self.vm_state.settings.clone();
        data.persist_data = self.vm_state.persist.clone();
        data.save_vectors = self.vm_state.save_vectors.clone();
        data.auto_save_slot = self.vm_state.auto_save_slot.clone();
        data.play_seconds = self.play_clock.saved_seconds();
        data
    }
    /// Transfer globals after menu return or authored EXIT completes.
    /// The host must separately transfer global audio and then destroy this ADV.
    pub(crate) fn take_title_global_data(&mut self) -> anyhow::Result<GlobalData> {
        anyhow::ensure!(!self.globals_detached, "ADV globals already transferred");
        anyhow::ensure!(
            self.pending_title_setup().is_some() || self.failed_scene_title_setup().is_some(),
            "Title return is not ready"
        );
        let data = self.save_menu_data();
        self.globals_detached = true;
        Ok(GlobalData {
            data,
            persistence: self.persistence.take(),
            clock: std::mem::take(&mut self.play_clock),
            revision: self.vm_state.save_revision,
        })
    }
    pub(crate) fn take_title_audio(&mut self) -> anyhow::Result<GlobalAudio> {
        anyhow::ensure!(self.globals_detached, "Transfer title globals before audio");
        self.adv_state.audio.se_player.retire_scene();
        // Retire the old scene's pause scopes before detaching global effects.
        // Voice remains scene-owned and is stopped when its root is destroyed.
        while self.adv_state.audio_pause_count != 0 {
            self.adv_state.release_audio_pause();
        }
        self.adv_state
            .audio
            .take()
            .ok_or_else(|| anyhow::anyhow!("ADV audio already transferred"))
    }
    /// Install retained app globals before queuing the title's saved-scene factory.
    pub(crate) fn install_global_data(&mut self, mut globals: GlobalData) {
        self.vm_state.settings = globals.data.settings.clone();
        self.vm_state.persist = globals.data.persist_data.clone();
        self.vm_state.save_vectors = globals.data.save_vectors.clone();
        self.vm_state.auto_save_slot = globals.data.auto_save_slot.clone();
        self.vm_state.save_revision = globals.revision;
        self.volatile_save = std::mem::take(&mut globals.data);
        self.persistence = globals.persistence.take();
        self.play_clock = std::mem::take(&mut globals.clock);
        self.globals_detached = false;
        self.adv_state.apply_audio_settings(&self.vm_state.settings);
    }
    #[cfg(test)]
    pub fn load_saved_scene(
        &mut self,
        globals: &shin_core::format::save::SaveData,
        entry: &shin_core::format::save::GameDataEntry,
        server: Arc<crate::asset::system::AssetServer>,
    ) -> anyhow::Result<()> {
        let scene = reconstruction::ReconstructedScene::from_game_data_for_load(
            &self.scenario,
            &shin_core::format::save::GameData::from_entry(entry.clone()),
            globals,
            1_000_000,
        )?;
        self.queue_reconstructed_scene(Box::new(scene), server, None);
        Ok(())
    }

    pub fn load_game_data(
        &mut self,
        globals: &shin_core::format::save::SaveData,
        game: &shin_core::format::save::GameData,
        server: Arc<crate::asset::system::AssetServer>,
    ) -> anyhow::Result<()> {
        let scene = reconstruction::ReconstructedScene::from_game_data_for_load(
            &self.scenario,
            game,
            globals,
            1_000_000,
        )?;
        self.queue_reconstructed_scene(Box::new(scene), server, None);
        Ok(())
    }

    fn queue_reconstructed_scene(
        &mut self,
        scene: Box<reconstruction::ReconstructedScene>,
        server: Arc<crate::asset::system::AssetServer>,
        previous_position: Option<u32>,
    ) {
        self.queue_reconstructed_scene_with_kind(
            scene,
            server,
            previous_position,
            SceneLoadKind::Saved,
        );
    }

    fn queue_reconstructed_scene_with_kind(
        &mut self,
        scene: Box<reconstruction::ReconstructedScene>,
        server: Arc<crate::asset::system::AssetServer>,
        previous_position: Option<u32>,
        kind: SceneLoadKind,
    ) {
        let scenario = self.scenario.clone();
        let audio = self.adv_state.audio_manager.clone();
        // Retire the previous loader before scheduling its replacement.
        self.pending_restore = None;
        let reused = if kind == SceneLoadKind::Saved {
            Vec::new()
        } else {
            restoration::reuse_plan(&self.adv_state, &scene.state, &server)
        };
        self.restore_error = None;
        self.retry_live_command_after_error = false;
        if self.restore_error_dialog.take().is_some() {
            self.adv_state.release_audio_pause();
        }
        self.pending_previous_scene = false;
        self.pending_chapter = None;
        self.chapter_dialog = None;
        self.story_touch.close();
        // Loading/reveal frames bypass both pointer delivery paths. Retire
        // the source capture now, before its release can be lost there.
        self.cancel_event_contact();
        self.modal_pointer.cancel_contact(self.modal_pointer_held);
        self.pending_playback_sound = None;
        self.pending_rollback_position = previous_position;
        self.pending_restore = Some(PendingSceneRestore {
            task: Some(shin_tasks::async_io::spawn_loader(async move {
                let loaded =
                    std::panic::AssertUnwindSafe(restoration::LoadedScene::load_with_reuse(
                        &server,
                        &audio,
                        &scenario,
                        &scene.state,
                        reused,
                    ))
                    .catch_unwind()
                    .await
                    .map_err(|panic| {
                        let message = panic
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic.downcast_ref::<&str>().copied())
                            .unwrap_or("unknown panic");
                        anyhow::anyhow!("Scene resource worker failed: {message}")
                    })??;
                Ok((loaded, scene))
            })),
            ready: None,
            retirement: None,
            kind,
            source: None,
            present_source: false,
        });
    }

    /// Keep the outgoing composed screen visible during saved-scene preparation,
    /// then transfer this same capture to the normal saved-load wipe.
    pub(crate) fn set_saved_load_source(
        &mut self,
        source: shin_render::render_texture::RenderTexture,
    ) {
        if let Some(pending) = &mut self.pending_restore {
            pending.source = Some(source);
            pending.present_source = true;
        }
    }

    #[inline(never)]
    fn queue_resume_scene(&mut self, context: &mut UpdateContext) {
        self.flush_progress(context);
        let globals = shin_core::format::save::SaveData {
            settings: self.vm_state.settings.clone(),
            persist_data: self.vm_state.persist.clone(),
            save_vectors: self.vm_state.save_vectors.clone(),
            auto_save_slot: self.vm_state.auto_save_slot.clone(),
            ..Default::default()
        };
        match reconstruction::ReconstructedScene::from_game_data(
            &self.scenario,
            &self.vm_state.resume_game_data(),
            &globals,
            1_000_000,
        ) {
            Ok(scene) => {
                self.queue_reconstructed_scene_with_kind(
                    Box::new(scene),
                    context.asset_server.clone(),
                    None,
                    SceneLoadKind::Resume,
                );
            }
            Err(error) => {
                self.report_scene_load_error(error, context);
            }
        }
    }

    pub fn previous_rollback_position(&self) -> Option<u32> {
        self.previous_rollback_position
    }

    #[cfg(test)]
    pub(crate) fn reference_load_transition_masked(&self) -> Option<bool> {
        self.adv_state
            .scene_load_fade
            .as_ref()
            .map(|fade| fade.is_masked())
    }
    #[cfg(test)]
    pub(crate) fn reference_load_transition_elapsed(&self) -> Option<f32> {
        self.adv_state
            .scene_load_fade
            .as_ref()
            .map(|fade| fade.reference_elapsed())
    }

    /// Native mode1 quick return to the previous/current message checkpoint.
    pub fn load_previous_scene(
        &mut self,
        server: Arc<crate::asset::system::AssetServer>,
    ) -> anyhow::Result<bool> {
        let Some(position) = self.vm_state.quick_return_position() else {
            return Ok(false);
        };
        if self.is_loading_saved_scene() || !self.vm_state.allows_system_controls() {
            return Ok(false);
        }
        let mut game = self.vm_state.snapshot_game_data();
        game.entry.save_position = position;
        let globals = shin_core::format::save::SaveData {
            settings: self.vm_state.settings.clone(),
            persist_data: self.vm_state.persist.clone(),
            save_vectors: self.vm_state.save_vectors.clone(),
            auto_save_slot: self.vm_state.auto_save_slot.clone(),
            ..Default::default()
        };
        let scene = reconstruction::ReconstructedScene::from_game_data(
            &self.scenario,
            &game,
            &globals,
            1_000_000,
        )?;
        self.queue_reconstructed_scene_with_kind(
            Box::new(scene),
            server,
            Some(self.vm_state.save_position),
            SceneLoadKind::History,
        );
        Ok(true)
    }

    fn flush_progress(&mut self, context: &mut UpdateContext) {
        if self.progress_error.is_some() {
            return;
        }
        if let Some(store) = &mut self.persistence {
            if let Err(error) = store.flush(&self.vm_state) {
                warn!(%error, "Saving game progress failed");
                match storage_retry_dialog::StorageRetryDialog::load_save_error(
                    context.asset_server,
                    self.modal_pointer_held,
                    self.modal_pointer_extent,
                ) {
                    Ok(mut dialog) => {
                        dialog.prepare(context.pre_render);
                        self.adv_state.acquire_audio_pause();
                        self.progress_error = Some(dialog);
                        self.cancel_event_contact();
                    }
                    Err(error) => warn!(%error, "Loading progress save error dialog failed"),
                }
            }
        }
    }

    fn begin_save_task(
        &mut self,
        server: &crate::asset::system::AssetServer,
        purpose: SavePurpose,
        edit: persistence::ExplicitSave,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.pending_save.is_none(),
            "A SaveGame task is already active"
        );
        let task = save_game::SaveGameTask::load(server)?;
        if let Some(store) = &mut self.persistence {
            store.prepare_explicit(&self.vm_state, edit)?;
        } else {
            let mut snapshot = self.save_menu_data();
            if let persistence::ExplicitSave::Slot { slot, game } = edit {
                anyhow::ensure!(
                    slot < snapshot.manual_save_slots.len(),
                    "Invalid manual save slot {slot}"
                );
                if game.is_some() {
                    snapshot.save_menu_position = slot as u8;
                }
                snapshot.manual_save_slots[slot] = game;
            }
            self.volatile_save = snapshot;
        }
        self.pending_save = Some(PendingSaveTask { task, purpose });
        Ok(())
    }

    fn update_save_task(&mut self, context: &mut UpdateContext) {
        let Some(save) = &mut self.pending_save else {
            return;
        };
        let result = save
            .task
            .update(context.delta_ticks, self.persistence.as_mut());
        save.task.prepare(context.pre_render);
        let sounds = save.task.take_sounds();
        for sound in sounds {
            self.adv_state.play_system_sound(
                context.asset_server,
                sound,
                shin_core::vm::command::types::Volume::default(),
            );
        }
        let Some(result) = result else {
            return;
        };
        let save = self.pending_save.take().unwrap();
        if let Err(error) = &result {
            warn!(%error, "Saving game data failed");
        }
        match save.purpose {
            SavePurpose::Manual { game } => {
                if let Some(menu) = &mut self.system_menu {
                    menu.save_menu_write_finished(
                        result.is_ok(),
                        result.is_ok().then_some(game).flatten(),
                    );
                }
            }
            SavePurpose::Config => self.system_menu.as_mut().unwrap().config_save_finished(),
            SavePurpose::TitleReturn => {
                let saves = self.save_menu_data();
                self.system_menu
                    .as_mut()
                    .unwrap()
                    .title_save_finished(&saves);
            }
            SavePurpose::Exit => self.exit_save_finished = true,
        }
    }

    pub fn is_loading_saved_scene(&self) -> bool {
        self.pending_restore.is_some() || self.adv_state.scene_load_fade.is_some()
    }

    pub fn saved_scene_load_error(&self) -> Option<&str> {
        self.restore_error.as_deref()
    }

    pub(crate) fn show_scene_load_error(
        &mut self,
        error: &anyhow::Error,
        server: &crate::asset::system::AssetServer,
    ) -> anyhow::Result<()> {
        tracing::error!("Scene restoration failed: {error:#}");
        self.restore_error = Some(format!("{error:#}"));
        let dialog = storage_retry_dialog::StorageRetryDialog::load_scene_error(
            server,
            self.modal_pointer_held,
            self.modal_pointer_extent,
        )?;
        if self.restore_error_dialog.is_none() {
            self.adv_state.acquire_audio_pause();
        }
        self.restore_error_dialog = Some(dialog);
        self.cancel_event_contact();
        Ok(())
    }

    fn report_scene_load_error(&mut self, error: anyhow::Error, context: &mut UpdateContext) {
        if let Err(error) = self.show_scene_load_error(&error, context.asset_server) {
            warn!(%error, "Loading scene restoration error dialog failed");
        }
        if let Some(dialog) = &mut self.restore_error_dialog {
            dialog.prepare(context.pre_render);
        }
    }

    /// A new ADV has no previous playable scene after a title-menu load fails.
    /// Its host returns to Load only after the error acknowledgement closes.
    pub(crate) fn failed_scene_title_setup(&self) -> Option<crate::title::TitleSetup> {
        (self.restore_error.is_some()
            && self.restore_error_dialog.is_none()
            && self.progress_error.is_none()
            && self.pending_save.is_none()
            && !self.is_loading_saved_scene())
        .then(|| {
            crate::title::TitleSetup::from_save(
                &self.save_menu_data(),
                crate::title::TitleItem::Load as i32,
            )
        })
    }

    fn update_saved_scene_load(&mut self, context: &mut UpdateContext) -> bool {
        if self.pending_restore.is_none() && self.adv_state.scene_load_fade.is_some() {
            self.update_loading_scene(context);
            return true;
        }
        let Some(pending) = self.pending_restore.as_mut() else {
            return false;
        };
        if pending.kind != SceneLoadKind::Resume && pending.source.is_none() {
            let mut source = context
                .pre_render
                .new_render_texture("Scene load snapshot".into());
            self.adv_state
                .root_layer_group
                .pre_render(context.pre_render, &TransformParams::default());
            let mut pass = context.pre_render.begin_pass(
                source.as_texture_target(),
                Some(context.pre_render.depth_stencil),
                "Scene load snapshot",
            );
            self.adv_state.render_scene(&mut pass);
            drop(pass);
            pending.source = Some(source);
        }
        // Native global cue retirement keeps running while scene loading
        // blocks ADV. Release finished buses so capacity waits can recover.
        self.adv_state.audio.system_player.update();
        if pending.ready.is_none() {
            pending.ready = pending.task.as_mut().unwrap().poll_naive();
            if pending.ready.is_some() {
                // The future has already retired. Retain its result while
                // waiting for audio capacity, without polling it again.
                pending.retirement = pending.task.as_ref().unwrap().retirement_handle();
                pending.task = None;
            }
        }
        let Some(ready) = pending.ready.as_ref() else {
            self.update_loading_scene(context);
            return true;
        };
        if ready.is_ok() {
            // TrackHandle drops are reclaimed by the audio callback. Several
            // rapid replacements can finish before that callback runs; retain
            // the loaded result and old scene until a fresh scene fits.
            let lacks_capacity = {
                let manager = self.adv_state.audio_manager.kira_manager().lock();
                manager
                    .sub_track_capacity()
                    .saturating_sub(manager.num_sub_tracks())
                    < AdvState::AUDIO_TRACK_COUNT
            };
            if lacks_capacity {
                self.update_loading_scene(context);
                return true;
            }
        }
        let kind = pending.kind;
        let source = pending.source.take();
        // Successful publication transfers resource ownership to the new scene.
        // Discarded/error results instead drain the retained loader work.
        let retirement = pending.retirement.take();
        let result = pending.ready.take().unwrap();
        self.pending_restore = None;
        let (loaded, scene) = match result {
            Ok(scene) => scene,
            Err(error) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(retirement) = retirement {
                    retirement.retire(());
                }
                self.pending_rollback_position = None;
                if let Some(backlog) = &mut self.adv_state.backlog {
                    backlog.recover_restore_failure();
                }
                self.report_scene_load_error(error, context);
                return true;
            }
        };
        let mut fresh = AdvState::new(
            self.adv_state.audio_manager.clone(),
            context.asset_server.clone(),
            self.assets.clone(),
        );
        let mut loaded_audio =
            loaded.install(&mut fresh, &scene.state, &self.scenario, context.pre_render);
        if kind != SceneLoadKind::Resume {
            // Prepare a candidate before reconstructed commands enqueue new
            // notices. Keep the source intact until start validation succeeds.
            fresh.notifications = self.adv_state.notifications.clone();
            fresh.notifications.cancel_for_scene_load();
        }
        let publish_continue = scene.publishes_continue_checkpoint();
        let starts_message = matches!(
            &scene.stop,
            reconstruction::ReconstructionStop::Command(
                shin_core::vm::command::RuntimeCommand::MSGSET(_)
            )
        );
        let start = match scene.stop {
            reconstruction::ReconstructionStop::Response(result) => {
                Ok(CommandStartResult::Continue(result))
            }
            reconstruction::ReconstructionStop::Command(command) => {
                command::start_reconstructed_command(
                    command,
                    context,
                    &self.scenario,
                    &scene.state,
                    &mut fresh,
                )
            }
        };
        let start = match start {
            Ok(start) => start,
            Err(error) => {
                self.pending_rollback_position = None;
                if let Some(backlog) = &mut self.adv_state.backlog {
                    backlog.recover_restore_failure();
                }
                // The candidate owns installed Movie decoders even though its
                // command was rejected. Drop it under the loader scope and
                // drain that work before returning to the preserved live scene.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(retirement) = retirement {
                    retirement.retire((fresh, loaded_audio));
                }
                self.report_scene_load_error(error, context);
                return true;
            }
        };
        loaded_audio.install_container_properties(
            &mut self.adv_state,
            &mut fresh,
            &scene.state,
            kind != SceneLoadKind::Saved,
        );
        loaded_audio.install_reused(&mut fresh, &scene.state);
        // Native audio players are global to the running ADV. Transfer them
        // only after validation, then restore through their identity reuse paths.
        std::mem::swap(
            &mut fresh.audio.bgm_player,
            &mut self.adv_state.audio.bgm_player,
        );
        std::mem::swap(
            &mut fresh.audio.se_player,
            &mut self.adv_state.audio.se_player,
        );
        loaded_audio.install(&mut fresh, &scene.state, &self.scenario);
        // MSGSET installs a fresh sentence at the saved command boundary.
        // Reinstalling the static snapshot here fast-forwards that sentence
        // and suppresses its voice before its first ordinary update.
        if !starts_message {
            restoration::restore_message(
                &mut fresh,
                &scene.state,
                &self.scenario,
                context.pre_render,
            );
        }
        // Native mode3 bypasses title/route cancellation and the notification
        // deque/opacity reset at1aad0..1abf8 by branching directly to1ac20.
        if kind == SceneLoadKind::Resume {
            fresh.bgm_title =
                std::mem::replace(&mut self.adv_state.bgm_title, bgm_title::BgmTitle::new());
            fresh.route_label = std::mem::replace(
                &mut self.adv_state.route_label,
                route_label::RouteLabel::new(),
            );
            fresh.notifications = std::mem::take(&mut self.adv_state.notifications);
            fresh
                .audio
                .system_player
                .preserve_active_from(&mut self.adv_state.audio.system_player);
            fresh.hidden_ui_active = self.adv_state.hidden_ui_active;
            fresh.ui_hide_count = self.adv_state.ui_hide_count;
            fresh.set_ui_hidden(fresh.ui_hide_count != 0);
        } else {
            fresh
                .bgm_title
                .restore_previous_for_exit(&mut self.adv_state.bgm_title);
            fresh
                .route_label
                .restore_previous_for_exit(&mut self.adv_state.route_label);
            // Modal scopes owned by the retired scene are released. Retained
            // exit animations follow the reconstructed scene's visibility.
            fresh.set_ui_hidden(fresh.ui_hide_count != 0);
        }
        self.adv_state.message_layer_mut().voice_player_mut().stop();
        let mut load_fade = source.map(|source| {
            let fade = scene_load_fade::SceneLoadFade::new(
                source,
                shin_core::time::Ticks::from_f32(if kind == SceneLoadKind::History {
                    4.0
                } else {
                    30.0
                }),
            );
            if kind == SceneLoadKind::Saved {
                fade.with_mask(self.assets.scene_load_mask.clone(), 0.3)
            } else {
                fade
            }
        });
        if kind != SceneLoadKind::Saved {
            fresh.scene_load_fade = load_fade.take();
        }
        self.previous_rollback_position = self.pending_rollback_position.take();
        let mut previous = std::mem::replace(&mut self.adv_state, fresh);
        self.adv_state
            .audio
            .system_player
            .absorb_retired(previous.audio.take().unwrap().system_player);
        if let Some(store) = &mut self.persistence {
            store.restore_revision(&self.vm_state, &scene.state);
        }
        self.vm_state = scene.state;
        // Publish restored user checkpoints for immediate Load/title returns.
        // Native19ea0 installs event transitions without f720: an event skip
        // reaching EVEND must retain the preceding message's Continue snapshot.
        if publish_continue {
            self.vm_state.capture_autosave();
        }
        self.scripter = scene.scripter;
        self.current_command = None;
        self.pending_result = shin_core::vm::command::CommandResult::None;
        self.finished = false;
        self.exit_save_finished = false;
        match start {
            CommandStartResult::Continue(result) => self.pending_result = result,
            CommandStartResult::Yield(command) => self.current_command = Some(command),
            CommandStartResult::Exit => self.finished = true,
        }
        self.adv_state.update(context);
        // A saved Load becomes visible only after publication. Its current
        // delta still belongs to outgoing capture/resource preparation; the
        // next delta also includes this frame's first pipeline compilation.
        if let Some(fade) = load_fade {
            self.adv_state.scene_load_fade = Some(fade.start_after_publication());
        }
        tracing::debug!(
            delta = context.delta_ticks.as_f32(),
            frame_ms = context.frame_started_at.elapsed().as_secs_f64() * 1000.0,
            "Saved scene published"
        );
        true
    }

    fn update_loading_scene(&mut self, context: &mut UpdateContext) {
        // Native ADV471 gates script/input, then rejoins0x11574: layers,
        // voice/SE, transitions, labels and notices keep their normal clocks.
        self.adv_state
            .message_layer_mut()
            .set_settings(&self.vm_state.settings);
        self.adv_state.apply_audio_settings(&self.vm_state.settings);
        self.adv_state.update(context);
    }

    fn can_skip_current_message(&self) -> bool {
        self.vm_state.can_skip_current_message()
            || !self
                .adv_state
                .root_layer_group
                .message_layer()
                .recv_sync_is_waiting(-1)
    }

    fn collect_gallery_unlocks(&mut self) {
        gallery::apply(
            &mut self.vm_state,
            self.adv_state.pending_gallery_unlocks.drain(..),
        );
    }

    fn collect_completed_messages(&mut self) {
        for id in self.adv_state.message_layer_mut().take_completed_messages() {
            self.vm_state.mark_message_read(id);
        }
    }

    /// Called every app frame, including frames before a modal opens. Coordinates
    /// are integer physical viewport positions before native canvas scaling.
    pub fn sample_modal_pointer(
        &mut self,
        position: glam::Vec2,
        extent: (u16, u16),
        held: bool,
        elapsed_seconds: f64,
    ) {
        let extent_changed = self.modal_pointer_extent != extent;
        self.modal_pointer.sample(position, held, elapsed_seconds);
        self.modal_pointer_extent = extent;
        self.modal_pointer_held = held;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let phase = if let Some(menu) = &self.system_menu {
                menu.desktop_diagnostic_ready().then_some("system_menu")
            } else if let Some(browser) = &self.adv_state.backlog {
                browser.desktop_diagnostic_phase()
            } else {
                let e = self.story_touch_eligibility();
                (e.input
                    && e.system
                    && !e.event
                    && self.adv_state.message_layer().is_interested_in_input())
                .then_some("dialogue")
            };
            if phase != self.last_desktop_browser_phase {
                self.last_desktop_browser_phase = phase;
                tracing::debug!(
                    phase = phase.unwrap_or("transition"),
                    pc = self.scripter.position().0,
                    save_position = self.vm_state.save_position,
                    "UMINEKO_BROWSER"
                );
            }
        }

        if let Some(menu) = &mut self.system_menu {
            menu.set_pointer_extent(extent);
        }
        if let Some((_, dialog)) = &mut self.chapter_dialog {
            if extent_changed {
                dialog.set_pointer_extent(extent, held);
            }
        }
    }

    pub(crate) fn cancel_event_contact(&mut self) {
        self.hidden_tap_input.cancel();
        self.story_touch.disable();
        self.event_skip.pointer(
            &self.modal_pointer.frame(),
            self.modal_pointer_extent,
            false,
        );
        self.pending_event_skip = false;
        self.held_skip = false;
    }
    /// Retained factory request for menu children still under reconstruction.
    pub fn pending_system_menu_request(&self) -> Option<&system_menu::screen::SystemMenuRequest> {
        self.system_menu
            .as_ref()
            .and_then(system_menu::modal::SystemMenuModal::pending_request)
    }
    /// Both authored EXIT and confirmed menu return queue a separate title.
    /// The scene host installs it next frame, retaining global save and audio.
    pub(crate) fn pending_title_setup(&self) -> Option<crate::title::TitleSetup> {
        if self.progress_error.is_some() || self.restore_error_dialog.is_some() {
            return None;
        }
        self.system_menu
            .as_ref()
            .and_then(system_menu::modal::SystemMenuModal::pending_title_setup)
            .cloned()
            .or_else(|| {
                (self.finished && self.exit_save_finished)
                    .then(|| crate::title::TitleSetup::from_save(&self.save_menu_data(), -1))
            })
    }
    pub fn handle_pointer(&mut self, position: glam::Vec2, clicked: bool) {
        if self.pending_save.is_some()
            || self.chapter_dialog.is_some()
            || self.progress_error.is_some()
            || self.restore_error_dialog.is_some()
        {
            return;
        }
        if self.system_menu.is_some() || self.adv_state.modal_backdrop.is_some() {
            return;
        }
        if self.is_loading_saved_scene() {
            return;
        }
        if self.adv_state.hidden_ui_active {
            return;
        }
        if let Some(backlog) = &mut self.adv_state.backlog {
            backlog.pointer(position, clicked);
        } else if let Some(screen) = &mut self.adv_state.character_screen {
            screen.model.pointer(position, clicked);
        } else if let Some(menu) = &mut self.adv_state.choice_menu {
            menu.pointer(position, clicked);
        } else if let Some(layer) = self.active_interactive_layer() {
            layer.pointer(position, clicked);
        }
    }

    pub fn handle_pointer_state(
        &mut self,
        position: glam::Vec2,
        clicked: bool,
        held: bool,
        elapsed_seconds: f64,
    ) {
        if let Some((_, dialog)) = &mut self.chapter_dialog {
            dialog.pointer_state(position, held, elapsed_seconds);
            return;
        }
        if let Some(dialog) = &mut self.progress_error {
            dialog.pointer_state(position, held, self.modal_pointer_extent, elapsed_seconds);
            return;
        }
        if let Some(dialog) = &mut self.restore_error_dialog {
            dialog.pointer_state(position, held, self.modal_pointer_extent, elapsed_seconds);
            return;
        }
        if let Some(save) = &mut self.pending_save {
            save.task
                .pointer_state(position, held, elapsed_seconds, self.modal_pointer_extent);
            return;
        }
        if self.system_menu.is_some() || self.adv_state.modal_backdrop.is_some() {
            if let Some(layer) = self.active_interactive_layer() {
                layer.ignore_pointer_state(held);
            }
        }
        if let Some(menu) = &mut self.system_menu {
            menu.pointer_state(position, held, elapsed_seconds);
            return;
        }
        if self.adv_state.modal_backdrop.is_some() {
            return;
        }
        if self.is_loading_saved_scene() || self.adv_state.hidden_ui_active {
            if let Some(menu) = &mut self.adv_state.choice_menu {
                menu.ignore_pointer_state(held);
            }
            if let Some(layer) = self.active_interactive_layer() {
                layer.ignore_pointer_state(held);
            }
            return;
        }
        if self.process_story_pointer().1 {
            if let Some(menu) = &mut self.adv_state.choice_menu {
                menu.ignore_pointer_state(held);
            }
            if let Some(layer) = self.active_interactive_layer() {
                layer.ignore_pointer_state(held);
            }
            return;
        }
        if let Some(backlog) = &mut self.adv_state.backlog {
            let scale = glam::vec2(
                1920.0 / self.modal_pointer_extent.0.max(1) as f32,
                1080.0 / self.modal_pointer_extent.1.max(1) as f32,
            );
            backlog.pointer_state_with_mouse_release(
                position,
                held,
                elapsed_seconds,
                self.modal_pointer
                    .mouse_release()
                    .map(|release| release.scaled(scale)),
            );
        } else if let Some(screen) = &mut self.adv_state.character_screen {
            screen.pointer_state(position, held, elapsed_seconds);
        } else if let Some(menu) = &mut self.adv_state.choice_menu {
            menu.pointer_state(position, held);
        } else if let Some(layer) = self.active_interactive_layer() {
            if !layer.pointer_frame(
                &self.modal_pointer.frame(),
                self.modal_pointer_extent,
                position,
            ) {
                layer.pointer_state(position, clicked, held, elapsed_seconds);
            }
        }
    }

    fn play_interactive_sounds(&mut self, server: &crate::asset::system::AssetServer) {
        let sounds = self
            .active_interactive_layer()
            .map(|layer| layer.take_sounds())
            .unwrap_or_default();
        for sound in sounds {
            self.adv_state
                .play_system_sound(server, sound, Default::default());
        }
    }
    fn active_interactive_layer(&self) -> Option<crate::layer::user::InteractiveLayerRef<'static>> {
        let (plane, bank) = self.current_command.as_ref()?.interactive_target()?;
        crate::layer::user::InteractiveLayerRef::from_owned(
            self.adv_state.plane_layer_group(plane).layer_owner(bank)?,
        )
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    // TODO: impl Scene for Adv
    #[tracing::instrument(skip_all)]
    pub fn render(&self, pass: &mut RenderPass) {
        self.adv_state.render(
            pass,
            self.skip_indicator_requested(),
            self.autoplay_indicator_active(),
            self.skip_indicator_active(),
            self.vm_state.settings.v11_disallowskipunread,
        );
        if let Some(pending) = &self.pending_restore {
            if pending.present_source {
                if let Some(source) = &pending.source {
                    scene_load_fade::render_snapshot(source, pass);
                }
            }
        }
        self.event_skip.render(pass, &self.assets.menu_textures);
        self.story_touch
            .render(self.assets.menu_textures.get("navi"), pass);
        if let Some(menu) = &self.adv_state.choice_menu {
            menu.render(pass);
        }
        if let Some(backlog) = &self.adv_state.backlog {
            backlog.render(pass);
        }
        if let Some(menu) = &self.system_menu {
            menu.render(pass);
        }
        if let Some(save) = &self.pending_save {
            save.task.render(pass);
        }
        if let Some((_, dialog)) = &self.chapter_dialog {
            dialog.render(pass);
        }
        if let Some(dialog) = &self.progress_error {
            dialog.render(pass);
        }
        if self.progress_error.is_none() {
            if let Some(dialog) = &self.restore_error_dialog {
                dialog.render(pass);
            }
        }
    }

    fn autoplay_indicator_active(&self) -> bool {
        // Native 122f0: autoplay bit, UI hide, SYS/event and child gates.
        self.adv_state.playback_modes.autoplay && self.playback_indicator_controls_allowed()
    }

    fn skip_indicator_active(&self) -> bool {
        // Native 12430 also requires a read message when read-only skip is set.
        self.skip_indicator_requested()
            && self.playback_indicator_controls_allowed()
            && (!self.vm_state.settings.v11_disallowskipunread
                || self.vm_state.messagebox_state.was_read)
    }

    fn skip_indicator_requested(&self) -> bool {
        // Hiding the message UI clears latched modes and leaves the scene clear.
        self.adv_state.playback_modes.skip || (self.held_skip && !self.adv_state.hidden_ui_active)
    }

    fn playback_indicator_controls_allowed(&self) -> bool {
        !self.adv_state.hidden_ui_active
            && self.chapter_dialog.is_none()
            && self.vm_state.allows_system_controls()
            && self.adv_state.modal_backdrop.is_none()
            && self.adv_state.choice_menu.is_none()
            && self.adv_state.character_screen.is_none()
            && self.system_menu.is_none()
            && self.pending_save.is_none()
            && self.progress_error.is_none()
            && self.restore_error_dialog.is_none()
            && self.active_interactive_layer().is_none()
    }

    pub fn set_character_scroll_axis(&mut self, axis: f32) {
        self.character_scroll_axis = if axis.is_finite() { axis } else { 0.0 };
    }
    pub(crate) fn set_interactive_axes(&mut self, axes: glam::Vec2) {
        self.interactive_axes = if axes.is_finite() {
            axes.clamp(glam::Vec2::NEG_ONE, glam::Vec2::ONE)
        } else {
            glam::Vec2::ZERO
        };
    }

    pub fn handle_input(&mut self, state: EnumMap<AppAction, ActionState>, is_focused: bool) {
        if !self.adv_state.hidden_ui_active {
            self.hidden_tap_input.cancel();
        }
        // CTRL is a temporary port control; never latch the native skip mode.
        self.held_skip = is_focused
            && self.chapter_dialog.is_none()
            && !self.is_loading_saved_scene()
            && self.pending_save.is_none()
            && self.progress_error.is_none()
            && self.restore_error_dialog.is_none()
            && !self.finished
            && state[AppAction::HoldSkip].is_held;
        if !is_focused
            || self.is_loading_saved_scene()
            || self.pending_save.is_some()
            || self.progress_error.is_some()
            || self.restore_error_dialog.is_some()
            || self.finished
        {
            self.hidden_tap_input.cancel();
            self.event_skip.pointer(
                &self.modal_pointer.frame(),
                self.modal_pointer_extent,
                false,
            );
            self.story_touch.disable();
            return;
        }
        if self.system_menu.is_some()
            || self.chapter_dialog.is_some()
            || self.adv_state.modal_backdrop.is_some()
            || self.adv_state.backlog.is_some()
            || self.adv_state.hidden_ui_active
            || self.adv_state.character_screen.is_some()
        {
            self.event_skip.pointer(
                &self.modal_pointer.frame(),
                self.modal_pointer_extent,
                false,
            );
            self.story_touch.disable();
        }

        if let Some(menu) = &mut self.system_menu {
            self.hidden_tap_input.cancel();
            menu.input_with_mouse_release(
                &self.modal_pointer.frame(),
                &state,
                self.character_scroll_axis,
                self.modal_pointer.mouse_release(),
            );
            return;
        }
        if self.chapter_dialog.is_some() {
            return;
        }
        // Native11f80..11fc4 blocks story controls until backdrop fade is idle0.
        if self.adv_state.modal_backdrop.is_some() {
            return;
        }

        let overlay_allows_playback_modes = self
            .active_interactive_layer()
            .is_none_or(|layer| layer.allows_playback_mode_changes());
        if !overlay_allows_playback_modes {
            // Native0x11198..0x111a0 disables both slots and clears47c.
            self.adv_state.playback_modes.reset();
        }

        if let Some(backlog) = &mut self.adv_state.backlog {
            backlog.input(&state, self.character_scroll_axis);
            return;
        }
        if self.adv_state.hidden_ui_active {
            let tapped = self
                .hidden_tap_input
                .update(&self.modal_pointer, self.modal_pointer_extent);
            if tapped
                || [
                    AppAction::Enter,
                    AppAction::Cancel,
                    AppAction::HideMessage,
                    AppAction::Backlog,
                    AppAction::CharacterState,
                ]
                .iter()
                .any(|&action| state[action].is_clicked)
            {
                tracing::debug!(tapped, "Hidden text restored");
                self.hidden_tap_input.cancel();
                self.adv_state.hidden_ui_active = false;
                self.adv_state.release_ui_hide();
            }
            return;
        }
        if let Some(screen) = &mut self.adv_state.character_screen {
            if screen.consumes_keyboard() {
                return;
            }
            screen
                .model
                .handle_controller_input(&state, self.character_scroll_axis);
            return;
        }

        let event_controls = self.vm_state.system_controls_allowed
            && self.vm_state.current_event.is_some()
            && self.adv_state.choice_menu.is_none()
            && self.active_interactive_layer().is_none();
        let pointer_consumed = self.process_story_pointer().0;
        self.apply_story_touch_requests();
        if pointer_consumed || self.story_touch.blocks_input() {
            return;
        }

        if self.vm_state.allows_playback_mode_changes() && overlay_allows_playback_modes {
            // Native L/autoplay (bit 0) takes priority over R/skip (bit 1).
            if state[AppAction::ToggleAuto].is_clicked {
                self.adv_state.playback_modes.toggle_autoplay();
                self.pending_playback_sound = Some(if self.adv_state.playback_modes.autoplay {
                    "son"
                } else {
                    "cancel"
                });
                return;
            }
            if state[AppAction::ToggleSkip].is_clicked {
                self.adv_state.playback_modes.toggle_skip();
                self.pending_playback_sound = Some(if self.adv_state.playback_modes.skip {
                    "son"
                } else {
                    "cancel"
                });
                return;
            }
        }

        // Native 0x11f2c..0x11f54 consumes cancel/X/Y to clear active modes.
        if (self.adv_state.playback_modes.skip || self.adv_state.playback_modes.autoplay)
            && [
                AppAction::Cancel,
                AppAction::Backlog,
                AppAction::CharacterState,
            ]
            .iter()
            .any(|&action| state[action].is_clicked)
        {
            self.adv_state.playback_modes.reset();
            self.pending_playback_sound = Some("cancel");
            return;
        }

        if self.vm_state.system_controls_allowed
            && self.adv_state.ui_hide_count == 0
            && self.adv_state.audio_pause_count == 0
        {
            // Native12028 handles chapter triggers before the event A/B path.
            if state[AppAction::NextChapter].is_clicked {
                self.pending_chapter = Some(chapter_navigation::Direction::Next);
                return;
            }
            if state[AppAction::PreviousChapter].is_clicked {
                self.pending_chapter = Some(chapter_navigation::Direction::Previous);
                return;
            }
        }

        if event_controls {
            // Native12038: A only displays the controller hint. B12040 and
            // full-screen long press15cc0 both prepare an event-end mode0 load.
            if state[AppAction::Enter].is_clicked {
                self.event_skip.show_hint(false);
            } else if state[AppAction::Cancel].is_clicked {
                self.pending_event_skip = true;
            }
            return;
        }

        // Native120b4/12238 routes Y/bit7 to13320, constructing SystemMenu1c100.
        // Cancel alias120cc/12248 instead constructs HiddenWindow1bd80 through13720.
        if state[AppAction::SystemMenu].is_clicked
            && self.vm_state.allows_system_controls()
            && self.adv_state.ui_hide_count == 0
            && self.adv_state.audio_pause_count == 0
        {
            self.pending_system_menu = true;
            return;
        }

        if state[AppAction::Backlog].is_clicked
            && self.vm_state.allows_system_controls()
            && self.vm_state.save_position > 0
        {
            self.pending_backlog = true;
            return;
        }

        if state[AppAction::HideMessage].is_clicked
            && self.vm_state.allows_system_controls()
            && (self
                .adv_state
                .choice_menu
                .as_ref()
                .is_some_and(choice::ChoiceMenu::can_hide)
                || self.adv_state.message_layer().is_interested_in_input())
        {
            self.adv_state.playback_modes.reset();
            self.adv_state.hidden_ui_active = true;
            self.story_touch.close();
            self.adv_state.acquire_ui_hide();
            return;
        }
        // Native reads the regular repeat mask's combined Up bit (0x12138).
        if state[AppAction::PreviousScene].is_clicked_or_repeated
            && self.vm_state.allows_system_controls()
            && self.adv_state.choice_menu.is_none()
            && self.active_interactive_layer().is_none()
        {
            self.pending_previous_scene = true;
            return;
        }

        if state[AppAction::Cancel].is_clicked {
            self.adv_state.playback_modes.reset();
        }

        if let Some(menu) = &mut self.adv_state.choice_menu {
            menu.input(&state);
            return;
        }

        if let Some(layer) = self.active_interactive_layer() {
            if layer.handle_controller_input(
                &state,
                self.character_scroll_axis,
                self.interactive_axes,
            ) {
                return;
            }
            if state[AppAction::Left].is_clicked_or_repeated {
                layer.navigate_horizontal(-1);
            }
            if state[AppAction::Right].is_clicked_or_repeated {
                layer.navigate_horizontal(1);
            }
            if state[AppAction::Cancel].is_clicked {
                layer.back();
            }
            if state[AppAction::PageUp].is_clicked_or_repeated {
                layer.scroll_text(-180.0);
            }
            if state[AppAction::PageDown].is_clicked_or_repeated {
                layer.scroll_text(180.0);
            }
            if state[AppAction::AnyDown].is_clicked_or_repeated {
                layer.navigate(1);
            }
            if state[AppAction::Up].is_clicked_or_repeated {
                layer.navigate(-1);
            }
            if state[AppAction::Enter].is_clicked {
                layer.confirm();
            }
            return;
        }

        if state[AppAction::Enter].is_clicked || state[AppAction::PointerClick].is_clicked {
            if self
                .current_command
                .as_mut()
                .is_some_and(UpdatableCommand::advance)
            {
                return;
            }
            self.adv_state.message_layer_mut().try_advance();
        }
    }

    // TODO: impl Scene for Adv
    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        input_state: EnumMap<AppAction, ActionState>,
    ) {
        self.held_skip = false;
        self.story_touch.configure(self.story_touch_eligibility());
        self.story_touch.update(context.delta_ticks);
        self.event_skip.update(context.delta_ticks);
        let progress_error = self.progress_error.is_some();
        if let Some(dialog) = if progress_error {
            self.progress_error.as_mut()
        } else {
            self.restore_error_dialog.as_mut()
        } {
            dialog.keyboard(&input_state);
            let done = dialog.update(context.delta_ticks);
            dialog.prepare(context.pre_render);
            for sound in dialog.take_sounds() {
                self.adv_state
                    .play_system_sound(context.asset_server, sound, Default::default());
            }
            self.adv_state.update(context);
            if let Some(menu) = &mut self.system_menu {
                menu.prepare(context.pre_render);
            }
            if done {
                if progress_error {
                    self.progress_error = None;
                } else {
                    self.restore_error_dialog = None;
                    if std::mem::take(&mut self.retry_live_command_after_error) {
                        self.restore_error = None;
                    }
                }
                self.adv_state.release_audio_pause();
            }
            // Do not deliver the acknowledgement input to a story/menu owner.
            return;
        }
        // If even the failure notice uses the damaged dialog assets, retain
        // the finished scene without reloading/logging every frame. A fresh
        // Enter press explicitly retries preparation after data is repaired.
        if self.finished
            && !self.exit_save_finished
            && self.pending_save.is_none()
            && self.restore_error.is_some()
        {
            if input_state[AppAction::Enter].is_clicked {
                self.restore_error = None;
                self.retry_live_command_after_error = false;
            } else {
                self.adv_state.update_presentation(context);
                return;
            }
        }
        if let Some(save) = &mut self.pending_save {
            save.task.keyboard(&input_state);
        }
        self.update_save_task(context);
        if self.pending_save.is_some() && self.system_menu.is_none() {
            self.adv_state.update(context);
            return;
        }
        if self.update_saved_scene_load(context) {
            return;
        }
        self.adv_state
            .message_layer_mut()
            .set_settings(&self.vm_state.settings);
        self.adv_state.apply_audio_settings(&self.vm_state.settings);
        // self.action_state.update(context.raw_input_state);

        let fast_forward_button_held =
            input_state[AppAction::HoldSkip].is_held && self.chapter_dialog.is_none();
        // self
        //     .action_state
        //     .is_pressed(AdvMessageAction::HoldFastForward);

        // if self.action_state.is_just_pressed(AdvMessageAction::Advance) {
        //     self.adv_state
        //         .root_layer_group
        //         .message_layer_mut()
        //         .advance();
        // }

        // TODO: tasks from task pool can steal focus
        self.handle_input(input_state, true);
        if self.update_chapter_navigation(context, &input_state) {
            return;
        }
        // Native f610 returns null if the event has ended since input was read.
        if std::mem::take(&mut self.pending_event_skip)
            && self.vm_state.current_event.is_some()
            && self.vm_state.system_controls_allowed
        {
            let globals = self.save_menu_data();
            match reconstruction::ReconstructedScene::skip_event_game_data(
                &self.scenario,
                &self.vm_state.snapshot_game_data(),
                &globals,
                1_000_000,
            ) {
                Ok(scene) => {
                    self.adv_state.play_system_sound(
                        context.asset_server,
                        "decide",
                        Default::default(),
                    );
                    self.queue_reconstructed_scene(
                        Box::new(scene),
                        context.asset_server.clone(),
                        None,
                    );
                    self.update_saved_scene_load(context);
                }
                Err(error) => {
                    self.report_scene_load_error(error, context);
                }
            }
            return;
        }
        if let Some(sound) = self.pending_playback_sound.take() {
            self.adv_state
                .play_system_sound(context.asset_server, sound, Default::default());
        }
        if std::mem::take(&mut self.pending_system_menu) {
            self.story_touch.close();
            // Native1c1b4 calls13a70 before copying VM metadata into the menu.
            let backdrop = self.adv_state.capture_modal_backdrop(context.pre_render);
            self.adv_state.acquire_ui_hide();
            self.adv_state.acquire_audio_pause();
            self.adv_state.modal_backdrop = Some(backdrop);
            self.adv_state.playback_modes.reset();
            self.adv_state
                .play_system_sound(context.asset_server, "page", Default::default());
            self.system_menu = Some(system_menu::modal::SystemMenuModal::new(
                context.asset_server,
                system_menu::SystemMenuSnapshot::from_vm(&self.vm_state),
                self.assets.menu_textures.clone(),
                self.play_clock.ticks(),
                self.modal_pointer_extent,
            ));
            // The constructor runs after native task update; only the global
            // backdrop advances on its creation frame, not the new menu's rows.
            self.adv_state.update(context);
            return;
        }
        if self.system_menu.is_some() {
            // Native112ec updates tasks before VM/root/global backdrop clocks.
            let saves = self.save_menu_data();
            let canvas: shin_render::resize::CanvasSize =
                context.pre_render.resize_source.handle().get();
            let mut menu = self.system_menu.take().unwrap();
            let closed = match menu.update(
                context.delta_ticks,
                context.asset_server,
                &self.scenario,
                self.play_clock.ticks(),
                &self.vm_state.settings,
                &saves,
                &self.adv_state.audio_manager,
                (canvas.width, canvas.height),
            ) {
                Ok(closed) => closed,
                Err(error) => {
                    warn!(%error, "Loading system menu child failed");
                    menu.cancel_failed_child();
                    false
                }
            };
            if let Some(settings) = menu.config_settings() {
                self.vm_state.settings = settings.clone();
                self.adv_state
                    .message_layer_mut()
                    .set_settings(&self.vm_state.settings);
                self.adv_state.apply_audio_settings(&self.vm_state.settings);
            }
            // Setting values are published above; consume widget notifications
            // so repeated edits do not accumulate for the screen's lifetime.
            let _edits = menu.take_config_edits();
            if menu.take_config_save_request() {
                if let Err(error) = self.begin_save_task(
                    context.asset_server,
                    SavePurpose::Config,
                    persistence::ExplicitSave::Globals,
                ) {
                    warn!(%error, "Preparing Config save failed");
                    menu.config_save_failed();
                }
            }
            if menu.take_title_save_request() {
                if let Err(error) = self.begin_save_task(
                    context.asset_server,
                    SavePurpose::TitleReturn,
                    persistence::ExplicitSave::Globals,
                ) {
                    warn!(%error, "Preparing title-return save failed");
                    menu.cancel_failed_child();
                }
            }
            let mut load_entry = None;
            for request in menu.take_save_menu_requests() {
                use save_menu::SaveMenuRequest;
                match request {
                    SaveMenuRequest::Save { slot, mut game } => {
                        game.refresh_timestamp();
                        menu.save_menu_write_started(Some(game.clone()));
                        if let Err(error) = self.begin_save_task(
                            context.asset_server,
                            SavePurpose::Manual {
                                game: Some(game.clone()),
                            },
                            persistence::ExplicitSave::Slot {
                                slot,
                                game: Some(game),
                            },
                        ) {
                            warn!(%error, "Preparing manual save failed");
                            menu.save_menu_write_finished(false, None);
                        }
                    }
                    SaveMenuRequest::Delete { slot } => {
                        menu.save_menu_write_started(None);
                        if let Err(error) = self.begin_save_task(
                            context.asset_server,
                            SavePurpose::Manual { game: None },
                            persistence::ExplicitSave::Slot { slot, game: None },
                        ) {
                            warn!(%error, "Preparing save deletion failed");
                            menu.save_menu_write_finished(false, None);
                        }
                    }
                    SaveMenuRequest::Load { slot, game } => {
                        if let Some(store) = &mut self.persistence {
                            store.data.save_menu_position = slot as u8;
                        } else {
                            self.volatile_save.save_menu_position = slot as u8;
                        }
                        load_entry = Some(game);
                    }
                    SaveMenuRequest::Confirmation { .. } => {
                        unreachable!("SaveMenu owns its confirmation dialog")
                    }
                }
            }
            menu.prepare(context.pre_render);
            if let Some(save) = &mut self.pending_save {
                save.task.prepare(context.pre_render);
            }
            let sounds = menu.take_sounds();
            for sound in sounds {
                self.adv_state
                    .play_system_sound(context.asset_server, sound, Default::default());
            }
            // Native manual Load queues a fresh ADV (dbf38/19524). Its
            // constructor10060 copies the completed canvas, including the
            // paused scene's dimmed backdrop, into capture owner62d80. Preserve
            // that composition before releasing the old modal scopes; a raw
            // story-to-story capture is invisible when the same save is loaded.
            let load_source = load_entry.as_ref().map(|_| {
                let mut source = context
                    .pre_render
                    .new_render_texture("Pause-menu Load outgoing frame".into());
                let mut pass = context.pre_render.begin_pass(
                    source.as_texture_target(),
                    Some(context.pre_render.depth_stencil),
                    "Pause-menu Load outgoing frame",
                );
                self.render(&mut pass);
                menu.render(&mut pass);
                drop(pass);
                source
            });
            if closed || load_entry.is_some() {
                self.system_menu = None;
                // Native1d2b8 releases audio/UI immediately, then starts fade0.
                self.adv_state.release_audio_pause();
                self.adv_state.release_ui_hide();
                self.adv_state
                    .modal_backdrop
                    .as_mut()
                    .unwrap()
                    .begin_close();
            } else {
                self.system_menu = Some(menu);
            }
            if let Some(entry) = load_entry {
                let globals = self.save_menu_data();
                if let Err(error) =
                    self.load_game_data(&globals, &entry, context.asset_server.clone())
                {
                    self.report_scene_load_error(error, context);
                } else {
                    self.set_saved_load_source(load_source.unwrap());
                }
                self.update_loading_scene(context);
                return;
            }
            if self.system_menu.is_some() {
                self.adv_state.update(context);
                return;
            }
            // Native11304 gates VM only on the audio scope, so VM/root updates
            // can resume as soon as the task releases it, during fade-out.
        }
        if std::mem::take(&mut self.pending_previous_scene) {
            self.adv_state.playback_modes.reset();
            let accepted = match self.load_previous_scene(context.asset_server.clone()) {
                Ok(accepted) => accepted,
                Err(error) => {
                    self.report_scene_load_error(error, context);
                    false
                }
            };
            self.adv_state.play_system_sound(
                context.asset_server,
                if accepted { "decide" } else { "error" },
                Default::default(),
            );
            self.adv_state.update(context);
            return;
        }
        if self.pending_backlog {
            self.story_touch.close();
            self.pending_backlog = false;
            let entry = self.vm_state.snapshot_game_data();
            let globals = shin_core::format::save::SaveData {
                settings: self.vm_state.settings.clone(),
                persist_data: self.vm_state.persist.clone(),
                save_vectors: self.vm_state.save_vectors.clone(),
                auto_save_slot: self.vm_state.auto_save_slot.clone(),
                ..Default::default()
            };
            self.adv_state.playback_modes.reset();
            self.adv_state.acquire_ui_hide();
            self.adv_state.acquire_audio_pause();
            self.adv_state.backlog = Some(backlog::modal::BacklogModal::new(
                self.scenario.clone(),
                entry,
                globals,
                context.asset_server.clone(),
                self.adv_state.audio_manager.clone(),
                self.assets.fonts.clone(),
            ));
        }
        if self.adv_state.backlog.is_some() {
            self.adv_state.update(context);
            if let Some((scene, previous)) = self
                .adv_state
                .backlog
                .as_mut()
                .and_then(|backlog| backlog.take_prepared_scene())
            {
                self.queue_reconstructed_scene(scene, context.asset_server.clone(), Some(previous));
            }
            return;
        }
        self.play_interactive_sounds(context.asset_server);

        if self.adv_state.modal_backdrop.is_none()
            && !self.adv_state.hidden_ui_active
            && self.adv_state.choice_menu.is_none()
            && self.adv_state.character_screen.is_none()
            && self.active_interactive_layer().is_none()
            && self.adv_state.playback_modes.is_skipping(
                self.vm_state.allows_system_controls(),
                fast_forward_button_held,
            )
            && self.can_skip_current_message()
        {
            self.adv_state.root_layer_group_mut().fast_forward();
            if let Some(back_layer_group) = &mut self.adv_state.back_layer_group {
                back_layer_group.fast_forward();
            }
        }

        let mut result = std::mem::replace(&mut self.pending_result, CommandResult::None);
        while !self.finished {
            let is_fast_forwarding = self.chapter_dialog.is_none()
                && self.adv_state.modal_backdrop.is_none()
                && !self.adv_state.hidden_ui_active
                && self.adv_state.playback_modes.is_skipping(
                    self.vm_state.allows_system_controls(),
                    fast_forward_button_held,
                )
                && self.can_skip_current_message();

            if is_fast_forwarding
                && !self.adv_state.hidden_ui_active
                && self.adv_state.choice_menu.is_none()
                && self.adv_state.character_screen.is_none()
                && self.active_interactive_layer().is_none()
            {
                self.adv_state.root_layer_group_mut().fast_forward();
            }
            self.collect_completed_messages();

            // TODO: maybe yield if spent too much time in this loop?
            let runtime_command = if let Some(command) = &mut self.current_command {
                let updated = command.update(
                    context,
                    &self.scenario,
                    &mut self.vm_state,
                    &mut self.adv_state,
                    is_fast_forwarding,
                );
                if let Some(error) = command.take_error() {
                    self.report_scene_load_error(error, context);
                    self.retry_live_command_after_error = true;
                    return;
                }
                match updated {
                    None => break,
                    Some(result) => {
                        self.current_command = None;
                        self.collect_gallery_unlocks();
                        self.scripter.run(result).expect("scripter run failed")
                    }
                }
            } else {
                self.scripter.run(result).expect("scripter run failed")
            };

            let message_skip = if self.playback_indicator_controls_allowed()
                && (self.adv_state.playback_modes.skip || fast_forward_button_held)
            {
                command::NewMessageSkip::Requested
            } else {
                command::NewMessageSkip::Normal
            };
            let start = command::apply_command_state_and_start_with_skip_policy(
                runtime_command,
                context,
                &self.scenario,
                &mut self.vm_state,
                &mut self.adv_state,
                is_fast_forwarding,
                message_skip,
            );
            self.collect_gallery_unlocks();
            match start {
                CommandStartResult::Continue(r) => result = r,
                CommandStartResult::Yield(executing_command) => {
                    self.current_command = Some(executing_command);
                }
                CommandStartResult::Exit => {
                    self.finished = true;
                }
            }
        }

        if std::mem::take(&mut self.adv_state.pending_resume) {
            self.queue_resume_scene(context);
            self.adv_state.update_presentation(context);
            return;
        }

        if self.adv_state.modal_backdrop.is_none()
            && self.vm_state.allows_system_controls()
            && self.adv_state.playback_modes.autoplay
            && !self.adv_state.hidden_ui_active
            && self.adv_state.choice_menu.is_none()
            && self.adv_state.character_screen.is_none()
            && self.active_interactive_layer().is_none()
        {
            self.adv_state.message_layer_mut().request_autoplay();
        }
        self.adv_state.update(context);
        self.collect_completed_messages();
        self.play_interactive_sounds(context.asset_server);
        if self.finished && !self.exit_save_finished && self.pending_save.is_none() {
            match self.begin_save_task(
                context.asset_server,
                SavePurpose::Exit,
                persistence::ExplicitSave::Globals,
            ) {
                Ok(()) => self
                    .pending_save
                    .as_mut()
                    .unwrap()
                    .task
                    .prepare(context.pre_render),
                Err(error) => {
                    warn!(%error, "Preparing scenario-exit save failed; Enter retries");
                    self.report_scene_load_error(error, context);
                    self.retry_live_command_after_error = true;
                }
            }
        } else {
            self.flush_progress(context);
        }
    }
}

impl Drop for Adv {
    fn drop(&mut self) {
        // Join the loader while the scene, audio and globals still exist.
        self.pending_restore = None;
        if let Some(store) = &mut self.persistence {
            if let Err(error) = store.flush_final(&self.vm_state) {
                warn!(%error, "Saving game progress on shutdown failed");
            }
        }
    }
}

// impl Renderable for Adv {
//     fn render<'enc>(
//         &'enc self,
//         resources: &'enc GpuCommonResources,
//         render_pass: &mut wgpu::RenderPass<'enc>,
//         transform: Mat4,
//         projection: Mat4,
//     ) {
//         self.adv_state
//             .render(resources, render_pass, transform, projection);
//     }
//
//     fn resize(&mut self, resources: &GpuCommonResources) {
//         self.adv_state.resize(resources);
//     }
// }

// impl OverlayVisitable for Adv {
//     fn visit_overlay(&self, collector: &mut OverlayCollector) {
//         collector.subgroup(
//             "ADV",
//             |collector| {
//                 collector.overlay(
//                     "Command",
//                     |_ctx, top_left| {
//                         let command = if let Some(command) = &self.current_command {
//                             Cow::Owned(format!("{:?}", command))
//                         } else {
//                             Cow::Borrowed("None")
//                         };
//                         top_left.label(format!(
//                             "Command: {:08x} {}",
//                             self.scripter.position().0,
//                             command
//                         ));
//                     },
//                     true,
//                 );
//                 self.adv_state
//                     .root_layer_group
//                     .message_layer()
//                     .visit_overlay(collector);
//                 collector.overlay(
//                     "User Layers",
//                     |ctx, _top_left| {
//                         let page_layer =
//                             self.adv_state.root_layer_group.screen_layer().page_layer();
//                         Window::new("User Layers").show(ctx, |ui| {
//                             for plane in 0..PLANES_COUNT {
//                                 let layer_group = page_layer.plane(plane as u32);
//                                 let layer_ids =
//                                     layer_group.get_layer_ids().sorted().collect::<Vec<_>>();
//                                 if !layer_ids.is_empty() {
//                                     ui.monospace(format!("Plane {}:", plane));
//                                     for layer_id in layer_ids {
//                                         let layer = layer_group.get_layer(layer_id).unwrap();
//                                         ui.monospace(format!(
//                                             "  {:>2}: {:?}",
//                                             layer_id.raw(),
//                                             layer
//                                         ));
//                                     }
//                                 }
//                             }
//                         });
//                     },
//                     false,
//                 );
//             },
//             true,
//         );
//     }
// }

/// Contains all the state of the ADV system NOT pertaining to the VM
///
/// This is the object the VM manipulates
pub struct AdvState {
    scene_load_fade: Option<scene_load_fade::SceneLoadFade>,
    pub root_layer_group: RootLayerGroup,
    pub back_layer_group: Option<RootLayerGroup>,
    pub audio_manager: Arc<AudioManager>,
    pub audio: GlobalAudioSlot,
    pending_gallery_unlocks: Vec<gallery::GalleryUnlock>,
    voice_focus_enabled: bool,
    movie_volume_settings: crate::audio::MovieVolumeSettings,
    pub playback_modes: playback::PlaybackModes,
    pub notifications: information::NotificationQueue,
    bgm_title: bgm_title::BgmTitle,
    route_label: route_label::RouteLabel,
    skip_indicator: skip_indicator::SkipIndicator,
    ui_hide_count: u32,
    audio_pause_count: u32,
    hidden_ui_active: bool,
    modal_backdrop: Option<modal_backdrop::ModalBackdrop>,
    backlog: Option<backlog::modal::BacklogModal>,
    pending_resume: bool,
    pub choice_menu: Option<choice::ChoiceMenu>,
    pub character_screen: Option<characters::screen::CharacterScreen>,
    pub choice_font: Arc<crate::asset::font::GpuFontLazy>,
    pub choice_textures: Arc<crate::layer::message_layer::MessageboxTextures>,
    pub allow_running_animations: bool,
}

impl AdvState {
    // System cue, voice, two BGM tracks, 32 SE slots and one-shot SE.
    const AUDIO_TRACK_COUNT: usize = crate::audio::SE_SLOT_COUNT + 5;

    fn update_presentation(&mut self, context: &mut UpdateContext) {
        self.skip_indicator.update(context.delta_ticks);
        self.bgm_title.update(context.delta_ticks);
        self.route_label.update(context.delta_ticks);
        self.audio.system_player.update();
        self.update_notifications(context.asset_server, context.delta_ticks);
    }

    fn update_notifications(
        &mut self,
        server: &crate::asset::system::AssetServer,
        delta: shin_core::time::Ticks,
    ) {
        if let Some(code) = self.notifications.update(delta) {
            // Artwork failure must not unwind an ADV input/update callback or
            // undo an unlock. Keep normal toast/cue timing; a later toast can
            // retry, while a retained healthy atlas needs no new file access.
            if let Err(error) = self.notifications.load_textures(server) {
                warn!(%error, "Loading notification artwork failed");
            }
            self.play_system_sound(
                server,
                if code & 0x10 == 0 {
                    "notify1"
                } else {
                    "notify2"
                },
                shin_core::vm::command::types::Volume::default(),
            );
        }
    }
    // Native 0x145f0 changes overlay visibility only at the first acquisition
    // and final release, allowing nested modal tasks to retain their hide scope.
    fn acquire_ui_hide(&mut self) {
        self.ui_hide_count += 1;
        if self.ui_hide_count == 1 {
            self.set_ui_hidden(true);
        }
    }
    fn release_ui_hide(&mut self) {
        assert!(self.ui_hide_count > 0, "Unbalanced ADV UI hide scope");
        self.ui_hide_count -= 1;
        if self.ui_hide_count == 0 {
            self.set_ui_hidden(false);
        }
    }
    // Native counter474 is independent of common UI-hide counter478.
    fn acquire_audio_pause(&mut self) {
        self.audio_pause_count += 1;
        if self.audio_pause_count == 1 {
            self.message_layer_mut().voice_player_mut().set_paused(true);
            self.audio.se_player.set_paused(true);
        }
    }
    fn release_audio_pause(&mut self) {
        assert!(
            self.audio_pause_count > 0,
            "Unbalanced ADV audio pause scope"
        );
        self.audio_pause_count -= 1;
        if self.audio_pause_count == 0 {
            self.message_layer_mut()
                .voice_player_mut()
                .set_paused(false);
            self.audio.se_player.set_paused(false);
        }
    }
    fn set_ui_hidden(&mut self, hidden: bool) {
        self.message_layer_mut().set_modal_hidden(hidden);
        self.bgm_title.set_hidden(hidden);
        self.route_label.set_hidden(hidden);
        if let Some(menu) = &mut self.choice_menu {
            menu.set_hidden(hidden);
        }
    }
    pub fn apply_audio_settings(&mut self, settings: &shin_core::format::save::Settings) {
        self.voice_focus_enabled = settings.v4_voicefocus;
        if let Some(backlog) = &mut self.backlog {
            backlog.set_volume(settings.v2_voicevol);
        }
        self.movie_volume_settings = settings.into();
        self.audio
            .bgm_player
            .set_settings_volume(settings.v0_bgmvol);
        self.audio.bgm_player.update();
        self.bgm_title.set_enabled(settings.v15);
        self.route_label.set_enabled(settings.v14_showroutenavi);
        self.audio.se_player.set_settings_volume(settings.v1_sfxvol);
        self.audio.se_player.update();
        let mut message = self.message_layer_mut();
        let voice = message.voice_player_mut();
        voice.set_settings_volume(settings.v2_voicevol);
        voice.set_character_voice_mask(settings.v19);
        voice.set_character_panning_enabled(settings.v5_voicepanapot);
        drop(message);
        self.audio
            .system_player
            .set_settings_volume(settings.v3_sysvol);
    }

    pub fn play_system_sound(
        &mut self,
        server: &crate::asset::system::AssetServer,
        name: &str,
        volume: shin_core::vm::command::types::Volume,
    ) {
        self.audio.system_player.play(server, name, volume);
    }
    pub fn new(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<crate::asset::system::AssetServer>,
        assets: AdvAssets,
    ) -> Self {
        Self::new_with_audio(audio_manager, asset_server, assets, None)
    }
    fn new_with_audio(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<crate::asset::system::AssetServer>,
        assets: AdvAssets,
        retained_audio: Option<GlobalAudio>,
    ) -> Self {
        let retained = retained_audio.is_some();
        let mut state = Self {
            scene_load_fade: None,
            backlog: None,
            pending_resume: false,
            choice_menu: None,
            character_screen: None,
            choice_font: assets.fonts.medium_font.clone(),
            choice_textures: assets.messagebox_textures.clone(),
            root_layer_group: RootLayerGroup::new(
                assets.fonts.clone(),
                assets.messagebox_textures.clone(),
                VoicePlayer::new(audio_manager.clone(), asset_server),
            ),
            back_layer_group: None,
            audio_manager: audio_manager.clone(),
            audio: retained_audio
                .unwrap_or_else(|| GlobalAudio::new(audio_manager.clone()))
                .into(),
            pending_gallery_unlocks: Vec::new(),
            voice_focus_enabled: false,
            movie_volume_settings: Default::default(),
            playback_modes: playback::PlaybackModes::default(),
            notifications: information::NotificationQueue::default(),
            bgm_title: bgm_title::BgmTitle::new(),
            route_label: route_label::RouteLabel::new(),
            skip_indicator: skip_indicator::SkipIndicator::new(assets.menu_textures.clone()),
            ui_hide_count: 0,
            audio_pause_count: 0,
            hidden_ui_active: false,
            modal_backdrop: None,
            allow_running_animations: true,
        };
        if !retained {
            state.apply_audio_settings(&Default::default());
        }
        state
    }

    pub fn root_layer_group(&self) -> &RootLayerGroup {
        &self.root_layer_group
    }
    pub fn root_layer_group_mut(&mut self) -> &mut RootLayerGroup {
        &mut self.root_layer_group
    }

    pub fn message_layer(&self) -> parking_lot::MutexGuard<'_, MessageLayer> {
        self.root_layer_group.message_layer()
    }
    pub fn message_layer_mut(&mut self) -> parking_lot::MutexGuard<'_, MessageLayer> {
        self.root_layer_group.message_layer_mut()
    }

    pub fn screen_layer(&self) -> &ScreenLayer {
        self.root_layer_group.screen_layer()
    }
    pub fn screen_layer_mut(&mut self) -> &mut ScreenLayer {
        self.root_layer_group.screen_layer_mut()
    }

    pub fn page_layer(&self) -> &PageLayer {
        self.root_layer_group.screen_layer().page_layer()
    }
    pub fn page_layer_mut(&mut self) -> &mut PageLayer {
        self.root_layer_group.screen_layer_mut().page_layer_mut()
    }

    pub fn plane_layer_group(&self, plane: PlaneId) -> &LayerGroup {
        self.root_layer_group
            .screen_layer()
            .page_layer()
            .get_plane(plane)
    }

    pub fn plane_layer_group_mut(&mut self, plane: PlaneId) -> &mut LayerGroup {
        self.root_layer_group
            .screen_layer_mut()
            .page_layer_mut()
            .get_plane_mut(plane)
    }

    pub fn create_back_layer_group_if_needed(&mut self, ctx: &mut RenderCloneCtx) {
        // Native MASKLOAD d038..d054 clones the front root once into +b0.
        // LayerGroup 79c90 clones each child through virtual +28; MessageLayer
        // b00e0 retains the same object, including voice ownership.
        // Native 1157c..115b8 updates both roots; 11b04..11bec renders the back
        // root while present. Releasing +b0 when animations resume is at11368.

        if self.back_layer_group.is_none() {
            self.back_layer_group = Some(self.root_layer_group.render_clone(ctx));
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn render(
        &self,
        pass: &mut RenderPass,
        skip_requested: bool,
        auto_active: bool,
        skip_active: bool,
        read_only_skip: bool,
    ) {
        if self
            .modal_backdrop
            .as_ref()
            .is_none_or(modal_backdrop::ModalBackdrop::needs_scene)
        {
            self.render_scene(pass);
        } else {
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        }
        if let Some(backdrop) = &self.modal_backdrop {
            backdrop.render(pass);
        }
        self.notifications.render(pass);
        self.route_label.render(pass);
        self.bgm_title.render(pass);
        self.skip_indicator.render(
            pass,
            &self.playback_modes,
            skip_requested,
            auto_active,
            skip_active,
            read_only_skip,
        );
        // Native ADV draws its overlays before the modal task list (0x12c4c).
        if let Some(screen) = &self.character_screen {
            screen.render(pass);
        }
    }

    /// Native13c54 chooses back root when present, otherwise the current root.
    /// Caller must acquire modal UI/audio ownership after this capture completes.
    pub fn capture_modal_backdrop(
        &mut self,
        context: &mut PreRenderContext,
    ) -> modal_backdrop::ModalBackdrop {
        let root = self
            .back_layer_group
            .as_mut()
            .unwrap_or(&mut self.root_layer_group);
        let source = root.capture_modal_snapshot(context);
        modal_backdrop::ModalBackdrop::new(context, source)
    }

    fn render_scene(&self, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        let root = self
            .back_layer_group
            .as_ref()
            .unwrap_or(&self.root_layer_group);
        render_layer_without_bg(pass, &TransformParams::default(), root, 0);
        if let Some(fade) = &self.scene_load_fade {
            fade.render(pass);
        }
    }
}

impl Updatable for AdvState {
    fn update(&mut self, context: &mut UpdateContext) {
        if let Some(backdrop) = &mut self.modal_backdrop {
            backdrop.update(context.delta_ticks);
            if backdrop.is_closed() {
                self.modal_backdrop = None;
            }
        }
        if self
            .scene_load_fade
            .as_mut()
            .is_some_and(|fade| fade.update(context.delta_ticks))
        {
            self.scene_load_fade = None;
        }
        let backlog_sounds = self
            .backlog
            .as_mut()
            .map(|backlog| backlog.take_sounds())
            .unwrap_or_default();
        for sound in backlog_sounds {
            self.play_system_sound(context.asset_server, sound, Default::default());
        }
        let backlog_closed = self
            .backlog
            .as_mut()
            .is_some_and(|backlog| backlog.update(context));
        if backlog_closed {
            self.backlog = None;
            self.release_audio_pause();
            self.release_ui_hide();
        }
        self.skip_indicator.update(context.delta_ticks);
        self.bgm_title.update(context.delta_ticks);
        self.route_label.update(context.delta_ticks);
        self.audio.system_player.update();
        if let Some(menu) = &mut self.choice_menu {
            menu.update(context.delta_ticks);
        }
        if let Some(screen) = &mut self.character_screen {
            screen.update(context.delta_ticks, context.asset_server);
        }
        let choice_sounds = self
            .choice_menu
            .as_mut()
            .map(|menu| menu.take_sounds())
            .unwrap_or_default();
        for sound in choice_sounds {
            self.play_system_sound(context.asset_server, sound, Default::default());
        }
        let character_sounds = self
            .character_screen
            .as_mut()
            .map(|screen| screen.model.take_sounds())
            .unwrap_or_default();
        for sound in character_sounds {
            self.play_system_sound(context.asset_server, sound, Default::default());
        }

        self.update_notifications(context.asset_server, context.delta_ticks);
        let message_owner = self.root_layer_group.message_owner();
        let voice_callback = |character, pan| {
            let mut message = message_owner.lock();
            let voice = message.voice_player_mut();
            voice.pan_for_character(character, pan);
            voice.lipsync_for_character(character)
        };
        let adv_update_context = AdvUpdateContext {
            frame_id: context.frame_id,
            delta_ticks: context.delta_ticks,
            asset_server: context.asset_server,
            device: context.pre_render.device,
            queue: context.pre_render.queue,
            are_animations_allowed: self.allow_running_animations,
            voice_callback: Some(&voice_callback),
            movie_volume_settings: self.movie_volume_settings,
        };

        // this seems like a pre-PAGEBACK feature to stop incomplete transitions from rendering
        // why did they leave it in :/
        if self.allow_running_animations && self.audio_pause_count == 0 {
            self.back_layer_group = None;
        }

        let story_voice_active = self
            .message_layer()
            .voice_player()
            .get_wait_status()
            .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING);
        if self.audio_pause_count == 0 {
            self.root_layer_group.update(&adv_update_context);
            if let Some(back) = &mut self.back_layer_group {
                back.update(&adv_update_context);
            }
        }
        self.audio.bgm_player.update_voice_focus(
            context.delta_ticks,
            self.voice_focus_enabled,
            self.backlog
                .as_ref()
                .map_or(story_voice_active, backlog::modal::BacklogModal::is_playing),
        );

        let transform = TransformParams::default();

        self.root_layer_group
            .pre_render(context.pre_render, &transform);
        if let Some(back) = &mut self.back_layer_group {
            back.pre_render(context.pre_render, &transform);
        }
    }
}

#[cfg(test)]
mod native_ui_hide_tests {
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_ui_hide_task_counter_and_input() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0x12018, 0x52922788),
            (0x12020, 0x34000588),
            (0x120bc, 0xaa1303e0),
            (0x120cc, 0x37280be8),
            (0x1224c, 0x94000535),
            (0x13658, 0x9402791e),
            (0x13738, 0xb9047c1f),
            (0x13774, 0x94002183),
            (0x1bdc4, 0x320003e1),
            (0x1bdc8, 0x97ffe20a),
            (0x1be28, 0x2a1f03e1),
            (0x1be2c, 0x97ffe1f1),
            (0x1bfe4, 0x52801809),
            (0x1bfe8, 0x72a60009),
            (0x1bfec, 0x6a09011f),
            (0x1bff8, 0x39020268),
            (0x14610, 0xb9447a68),
            (0x14618, 0x350003c8),
            (0x14634, 0x94027547),
            (0x14688, 0x940226da),
            (0x14690, 0x11000508),
            (0x1469c, 0x71000508),
            (0x146a0, 0xb9047a68),
            (0x146a4, 0x540003a1),
            (0x146b0, 0x940226d0),
            (0x146bc, 0x94013f05),
            (0x146f8, 0x94027516),
            (0xb1b54, 0x3900e008),
        ] {
            assert_eq!(word(a), expected, "native ADV hide UI {a:x}");
        }
    }
}
