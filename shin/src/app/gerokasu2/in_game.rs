//! ADV-owned modal publication and game-specific child/storage routing.
use super::*;
use crate::adv::gerokasu2::{
    modal::{Modal, Services, backdrop::Backdrop},
    system_menu::{Child, Event},
};
use shin_core::format::save::gerokasu2::{GameData, SaveDate};
impl Services for GerHost {
    fn hide_message(&mut self, hidden: bool) {
        self.scene
            .runner()
            .message_owner()
            .lock()
            .set_modal_hidden(hidden);
    }
    fn hide_overlay(&mut self, hidden: bool) {
        self.scene
            .backend_mut()
            .reconstruction_root_mut()
            .conceal_overlay(hidden);
    }
    // Native14384..14398/14438..1444C: ADV+250 scene title and ADV+340 BGM title.
    fn hide_indicator(&mut self, hidden: bool) {
        self.notifications.conceal_scene_title(hidden);
    }
    fn hide_menu(&mut self, hidden: bool) {
        self.notifications.conceal_bgm_title(hidden);
    }
    fn pause_voice(&mut self, paused: bool) {
        self.scene.runner_mut().voice_mut().set_paused(paused);
    }
    fn pause_effects(&mut self, paused: bool) {
        self.scene.pause_menu_effects(paused);
    }
}
impl Gerokasu2App {
    pub(super) fn system_menu_requested(
        &mut self,
        input: &mut EnumMap<AppAction, ActionState>,
    ) -> bool {
        if self.suspended
            || self.backlog.is_some()
            || self.system.is_some()
            || self.loading_system.is_some()
            || self.backdrop.is_some()
            || input[AppAction::PointerClick].is_held
        {
            return false;
        }
        let Some(host) = self.host.as_mut() else {
            return false;
        };
        if !host.buttons_armed || !host.scene.playback().system_controls {
            return false;
        }
        let y = input[AppAction::SystemMenu].is_clicked;
        let modes = &host.scene.playback().modes;
        if y && (modes.autoplay || modes.skip) {
            //11650..11678 consumes Y to clear active playback modes before
            //L/R dispatch or menu eligibility. It must not open in this frame.
            host.scene.playback_mut().modes.reset();
            host.scene.play_system_cue("cancel");
            input[AppAction::ToggleAuto] = Default::default();
            input[AppAction::ToggleSkip] = Default::default();
            return false;
        }
        y && !input[AppAction::ToggleAuto].is_clicked
            && !input[AppAction::ToggleSkip].is_clicked
            && host.scene.system_menu_allowed()
    }
    pub(super) fn open_system_menu(&mut self, pre: &mut PreRenderContext) -> Result<()> {
        self.menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .system_player
            .play(
                &self.assets,
                "page",
                shin_core::vm::command::types::Volume(1.),
            );
        let host = self
            .host
            .as_mut()
            .context("Ger SystemMenu has no live ADV")?;
        let source = host
            .scene
            .backend_mut()
            .modal_capture_root_mut()
            .capture_without_ui(pre)?;
        let backdrop = Backdrop::capture(pre, source);
        self.modal.enter(|| Ok(()), host)?;
        host.scene.playback_mut().modes.reset();
        host.scene.playback_mut().hidden_ui = false;
        host.cancel_input();
        let title = host.scene.scene_title().to_owned();
        let assets = self.assets.clone();
        let audio = self.audio.clone();
        let settings = self.save_store.as_ref().unwrap().data.settings.clone();
        self.backdrop = Some(backdrop);
        self.loading_system = Some(shin_tasks::async_io::spawn_loader(system_menu::Host::load(
            assets, audio, settings, title,
        )));
        Ok(())
    }
    pub(super) fn poll_system_menu(&mut self) {
        if let Some(result) = self.loading_system.as_mut().and_then(|t| t.poll_naive()) {
            self.loading_system = None;
            match result {
                Ok(mut host) => {
                    host.cancel_input();
                    self.system = Some(host);
                }
                Err(error) => {
                    tracing::error!(%error,"Ger SystemMenu preparation failed; retaining ADV");
                    if let Some(host) = self.host.as_mut() {
                        self.modal.exit(host);
                        host.cancel_input();
                    }
                }
            }
        }
    }
    pub(super) fn finish_system_child(&mut self) {
        self.title_child = None;
        self.frozen_manual = None;
        self.waiting_for_child_save = false;
        if let Some(system) = self.system.as_mut() {
            system.child_returned();
        } else if let Some(title) = self.title.as_mut() {
            title.model.recover_child();
            title.cancel_input();
        }
    }
    pub(super) fn manual_write(&mut self, slot: u8, save: bool) {
        let replacement = if save {
            match self.frozen_manual.as_ref() {
                Some(frozen) => Some(GameData {
                    date: SaveDate::now(),
                    progress: frozen.progress.clone(),
                }),
                None => {
                    tracing::error!("Ger Save has no frozen live progress");
                    if let (Some(child), Some(store)) =
                        (self.title_child.as_mut(), self.save_store.as_ref())
                    {
                        child.manual_write_finished(false, &store.data);
                    }
                    return;
                }
            }
        } else {
            None
        };
        if let Err(error) = self.begin_write_wait(super::write_wait::Purpose::Manual) {
            tracing::error!(%error,"Ger manual write panel could not load");
            if let (Some(child), Some(store)) =
                (self.title_child.as_mut(), self.save_store.as_ref())
            {
                child.manual_write_finished(false, &store.data);
            }
            return;
        }
        let store = self.save_store.as_mut().unwrap();
        if let Err(error) = store.begin_manual_write(slot, replacement, save) {
            self.write_wait = None;
            tracing::error!(%error,"Ger manual save transaction could not start");
            if let Some(child) = self.title_child.as_mut() {
                child.manual_write_finished(false, &store.data);
            }
        } else if let Some(child) = self.title_child.as_mut() {
            child.manual_write_staged(&store.data);
        }
    }
    pub(super) fn handle_system_events(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::OpenChild(child) => {
                    let kind = match child {
                        Child::Save => {
                            let save = self.host.as_ref().unwrap().scene.save_session();
                            self.frozen_manual = Some(GameData {
                                date: SaveDate::default(),
                                progress: save.progress.clone(),
                            });
                            title_child::Kind::Save
                        }
                        Child::Load => title_child::Kind::Load,
                        Child::Config => title_child::Kind::Config,
                        Child::Manual => title_child::Kind::Manual,
                    };
                    self.start_title_child(kind);
                }
                Event::LeaveModal => {
                    self.system = None;
                    if let Some(host) = self.host.as_mut() {
                        self.modal.exit(host);
                        host.cancel_input();
                    }
                }
                Event::ReturnTitle => {
                    let store = self.save_store.as_mut().unwrap();
                    self.host
                        .as_ref()
                        .unwrap()
                        .scene
                        .export_save_globals(&mut store.data);
                    // Original SystemMenu callback enters145D0 directly. Script
                    // EXIT's30-tick/message-close/variable-commit path is separate.
                    self.system_title_exit = true;
                    self.waiting_for_exit_save = true;
                    store.mark_dirty();
                    if let Err(error) = self.begin_write_wait(super::write_wait::Purpose::Exit) {
                        tracing::error!(%error,"Ger SystemMenu Title save failed");
                        self.waiting_for_exit_save = false;
                        self.system_title_exit = false;
                        if let Some(system) = self.system.as_mut() {
                            system.recover_child();
                        }
                    }
                }
                Event::EnterModal | Event::ConfirmTitle => {}
            }
        }
    }
    /// Children poll before the scene. A final scope release resumes the VM
    /// and root in that frame; input stays blocked by the closing backdrop.
    pub(super) fn update_system_menu(
        &mut self,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        map: Viewport,
        seconds: f64,
    ) -> Result<()> {
        if self.suspended {
            if let Some(host) = self.host.as_mut() {
                host.cancel_input();
            }
            return Ok(());
        }
        let permission = self.title_child.is_none()
            && self.loading_child.is_none()
            && self.loading_game.is_none()
            && self.write_wait.is_none()
            && !self.waiting_for_exit_save;
        let events = if let Some(system) = self.system.as_mut() {
            system.update(
                delta.as_f32(),
                input,
                self.point,
                map,
                seconds,
                self.play_clock.ticks(),
                permission,
            )?
        } else {
            vec![]
        };
        self.handle_system_events(events);
        if self.loading_game.is_none() {
            let idle = EnumMap::default();
            let input = if self.write_wait.is_some() {
                &idle
            } else {
                input
            };
            if let Some(child) = self.title_child.as_mut() {
                let events = child.update(delta.as_f32(), input, self.point, map, seconds)?;
                self.handle_child_events(events);
            }
        }
        if let Some(host) = self.host.as_mut() {
            host.cancel_input();
            host.status =
                host.scene
                    .step_menu(self.frame, delta, false, self.modal.pause_depth != 0)?;
        }
        Ok(())
    }
    pub(super) fn reset_system_for_publication(&mut self) {
        self.backlog = None;
        self.system = None;
        self.backdrop = None;
        self.modal = Modal::default();
        self.frozen_manual = None;
        self.system_title_exit = false;
        self.menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .se_player
            .set_paused(false);
    }
    pub(super) fn retire_story_effects(&mut self) {
        // Ger ADV101E0→C38A0 destroys inline ADV+F0 SE sources. BGM and
        // system cues have separate app owners and survive this destruction.
        self.menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .se_player
            .retire_scene();
    }
    pub(super) fn poll_child(&mut self) {
        if let Some(result) = self
            .loading_child
            .as_mut()
            .and_then(|task| task.poll_naive())
        {
            self.loading_child = None;
            match result {
                Ok(mut child) => {
                    child.cancel_input();
                    self.title_child = Some(child);
                }
                Err(error) => {
                    tracing::error!(%error,"Ger child preparation failed; retaining parent");
                    self.recover_title_child();
                }
            }
        }
    }
}
