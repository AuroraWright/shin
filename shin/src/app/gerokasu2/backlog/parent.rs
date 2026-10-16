//! ADV publication boundary shared by the window and offscreen story driver.
use super::super::{GerHost, Gerokasu2App, root_touch};
use super::*;
use crate::adv::gerokasu2::{modal::backdrop::Backdrop, runner::RunnerStatus};

impl Gerokasu2App {
    fn backlog_requested(&self, input: &EnumMap<AppAction, ActionState>) -> bool {
        !self.suspended
            && self.backlog.is_none()
            && self.system.is_none()
            && self.loading_system.is_none()
            && self.backdrop.is_none()
            && self.loading_game.is_none()
            && self.modal.pause_depth == 0
            && !input[AppAction::PointerClick].is_held
            && !input[AppAction::SystemMenu].is_clicked
            && !input[AppAction::ToggleAuto].is_clicked
            && !input[AppAction::ToggleSkip].is_clicked
            && input[AppAction::Backlog].is_clicked
            && self
                .host
                .as_ref()
                .is_some_and(|h| {
                    // 11650 runs first: X with an active mode only clears
                    // it (GerHost::update plays the cue), never opening here.
                    let modes = &h.scene.playback().modes;
                    h.buttons_armed
                        && !(modes.autoplay || modes.skip)
                        && h.scene.system_menu_allowed()
                })
    }
    fn start_backlog(&mut self) {
        let host = self.host.as_ref().unwrap();
        let source = GameData {
            date: Default::default(),
            progress: host.scene.save_session().progress.clone(),
        };
        let mut globals = self.save_store.as_ref().unwrap().data.clone();
        host.scene.export_save_globals(&mut globals);
        #[cfg(test)]
        let options = std::mem::take(&mut self.backlog_options);
        #[cfg(not(test))]
        let options = Options::default();
        self.backlog = Some(Slot::pending(
            self.assets.clone(),
            self.scenario.clone(),
            self.audio.clone(),
            source,
            globals,
            options,
        ));
    }
    fn backlog_cue(&self, cue: &str) {
        self.menu_bgm
            .lock()
            .shared_audio()
            .lock()
            .system_player
            .play(&self.assets, cue, Volume(1.));
    }
    fn poll_backlog(&mut self, pre: &mut PreRenderContext) {
        if self.suspended {
            return;
        }
        if self.backlog.as_ref().is_some_and(|slot| slot.retire) {
            // Native TaskGroup culls the done browser on its next update. Its
            // private voice focus remains authoritative on the done-setting frame.
            self.backlog = None;
        }
        let Some(result) = self.backlog.as_mut().and_then(Slot::poll) else {
            return;
        };
        self.backlog = None;
        let result = result.and_then(|browser| -> Result<Option<Host>> {
            let Some(browser) = browser else {
                return Ok(None);
            };
            let host = self
                .host
                .as_mut()
                .context("Ger backlog lost its source ADV")?;
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
            self.backdrop = Some(backdrop);
            Ok(Some(browser))
        });
        match result {
            Ok(Some(mut browser)) => {
                browser.cancel_input();
                self.backlog = Some(Slot::live(browser));
                self.backlog_cue("page");
            }
            Ok(None) => self.backlog_cue("error"),
            Err(error) => {
                tracing::error!(%error,"Ger backlog factory failed; retaining ADV");
                self.backlog_cue("error");
            }
        }
    }
    pub(in crate::app::gerokasu2) fn prepare_adv_menus(
        &mut self,
        input: &mut EnumMap<AppAction, ActionState>,
        pre: &mut PreRenderContext,
        map: Viewport,
        seconds: f64,
    ) -> Result<()> {
        self.poll_backlog(pre);
        if self.host.as_ref().is_some_and(|h| h.hidden_window) {
            // HiddenWindow is the ADV's top child: no menus open meanwhile.
            if self.hidden_window_released(input, map, seconds) {
                self.show_window();
            }
            return Ok(());
        }
        if let Some(request) = self.host.as_mut().and_then(|h| h.request.take()) {
            // Contacts (swipes and the toolbar) reach the same openings as
            // Y/X/B, behind the same gates.
            if self.contact_request_allowed() {
                match request {
                    root_touch::Request::SystemMenu => {
                        if let Err(error) = self.open_system_menu(pre) {
                            tracing::error!(%error,"Ger SystemMenu capture failed; retaining ADV");
                        }
                    }
                    root_touch::Request::Backlog => self.start_backlog(),
                    root_touch::Request::HideWindow => self.hide_window(),
                }
            }
            return Ok(());
        }
        // An open chapter Question blocks 115B8; Plus/Minus (bit8/bit9) are
        // dispatched before Y/X/B.
        if self.host.as_ref().is_some_and(|h| h.chapter.is_some())
            || input[AppAction::NextChapter].is_clicked
            || input[AppAction::PreviousChapter].is_clicked
        {
            return Ok(());
        }
        //115B8 gives Y (bit7) priority over X (bit6), including Y consumed to
        //clear active playback modes. Factory preparation holds the source VM.
        if self.system_menu_requested(input) {
            if let Err(error) = self.open_system_menu(pre) {
                tracing::error!(%error,"Ger SystemMenu capture failed; retaining ADV");
            }
        } else if self.backlog_requested(input) {
            self.start_backlog();
        } else if self.hide_window_requested(input) {
            self.hide_window();
        }
        Ok(())
    }
    /// 115B8 B (bit5) -> 1346C, gated by 13210 (the 12CD0 menu gate with the
    /// selection owner's hide slot). B with an active mode only clears it.
    fn hide_window_requested(&self, input: &EnumMap<AppAction, ActionState>) -> bool {
        !self.suspended
            && self.backlog.is_none()
            && self.system.is_none()
            && self.loading_system.is_none()
            && self.backdrop.is_none()
            && self.loading_game.is_none()
            && self.modal.pause_depth == 0
            && !input[AppAction::PointerClick].is_held
            && !input[AppAction::SystemMenu].is_clicked
            && !input[AppAction::Backlog].is_clicked
            && !input[AppAction::ToggleAuto].is_clicked
            && !input[AppAction::ToggleSkip].is_clicked
            && input[AppAction::Cancel].is_clicked
            && self.host.as_ref().is_some_and(|h| {
                let modes = &h.scene.playback().modes;
                h.buttons_armed
                    && !h.hidden_window
                    && !(modes.autoplay || modes.skip)
                    && h.scene.system_menu_allowed()
            })
    }
    fn contact_request_allowed(&self) -> bool {
        !self.suspended
            && self.backlog.is_none()
            && self.system.is_none()
            && self.loading_system.is_none()
            && self.backdrop.is_none()
            && self.loading_game.is_none()
            && self.modal.pause_depth == 0
            && self.host.as_ref().is_some_and(|h| {
                !h.hidden_window && h.chapter.is_none() && h.scene.system_menu_allowed()
            })
    }
    /// 1346C clears the mode word; 1BAE0 acquires conceal 14210(1).
    fn hide_window(&mut self) {
        let host = self.host.as_mut().unwrap();
        host.scene.playback_mut().modes.reset();
        host.scene.playback_mut().hidden_ui = true;
        host.hidden_window = true;
        self.modal.conceal(true, host);
        host.cancel_input();
        host.arm_hidden_window();
    }
    /// 1BBF0 finishes on pressed & 0x300000C0 (X, Y, A alias, B alias);
    /// 1BC90 finishes on a tap, recognized like the touch ripple: a drag or
    /// a contact held from before the window hid does not restore it. The
    /// press does nothing else.
    fn hidden_window_released(
        &mut self,
        input: &EnumMap<AppAction, ActionState>,
        map: Viewport,
        seconds: f64,
    ) -> bool {
        if self.suspended {
            self.host.as_mut().unwrap().arm_hidden_window();
            return false;
        }
        let point = self.point;
        let tapped = self
            .host
            .as_mut()
            .unwrap()
            .hidden_window_tapped(input, point, map, seconds);
        tapped
            || [
                AppAction::Enter,
                AppAction::Cancel,
                AppAction::Backlog,
                AppAction::SystemMenu,
            ]
            .into_iter()
            .any(|action| input[action].is_clicked)
    }
    /// The culled child's destructor 1BB50 releases conceal 14210(0).
    fn show_window(&mut self) {
        let host = self.host.as_mut().unwrap();
        host.hidden_window = false;
        host.scene.playback_mut().hidden_ui = false;
        self.modal.conceal(false, host);
        host.cancel_input();
    }
    pub(in crate::app::gerokasu2) fn update_adv_scene(
        &mut self,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        map: Viewport,
        seconds: f64,
    ) -> Result<RunnerStatus> {
        if self.backlog.is_some() {
            if self.suspended {
                return Ok(RunnerStatus::Waiting);
            }
            let events = if self.loading_game.is_none() {
                match self.backlog.as_mut().unwrap().host.as_mut() {
                    Some(browser) => {
                        browser.update(delta.as_f32(), input, self.point, map, seconds)?
                    }
                    None => return Ok(RunnerStatus::Waiting),
                }
            } else {
                vec![]
            };
            for event in events {
                match event {
                    Event::LeaveModal => {
                        if let Some(host) = self.host.as_mut() {
                            self.modal.exit(host);
                            host.cancel_input();
                        }
                    }
                    Event::Finished => self.backlog.as_mut().unwrap().retire = true,
                    Event::Jump(game) => self.start_backlog_game(game),
                }
            }
            let host = self.host.as_mut().unwrap();
            host.cancel_input();
            host.status =
                host.scene
                    .step_menu(self.frame, delta, false, self.modal.pause_depth != 0)?;
            return Ok(host.status);
        }
        if self.modal.pause_depth != 0 || self.backdrop.is_some() {
            self.update_system_menu(delta, input, map, seconds)?;
            Ok(self
                .host
                .as_ref()
                .map_or(RunnerStatus::Waiting, |h| h.status))
        } else if let Some(host) = self.host.as_mut() {
            if host.hidden_window {
                // The scene keeps running beneath; its buttons belong to
                // HiddenWindow until it is culled.
                return host.update(
                    self.frame,
                    delta,
                    &EnumMap::default(),
                    self.point,
                    map,
                    seconds,
                );
            }
            host.update(self.frame, delta, input, self.point, map, seconds)
        } else {
            Ok(RunnerStatus::Waiting)
        }
    }
    fn start_backlog_game(&mut self, game: GameData) {
        let assets = self.assets.clone();
        let scenario = self.scenario.clone();
        let audio = self.audio.clone();
        let data = self.save_store.as_ref().unwrap().data.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.loading_cancelled = cancelled.clone();
        self.loading_game = Some(shin_tasks::async_io::spawn_loader(async move {
            let mut host = GerHost::load(
                assets,
                scenario.clone(),
                audio,
                game.progress.active_entry().random_seed,
            )
            .await?;
            host.scene.defer_audio_publication();
            host.restore_memory_cancellable(scenario, &data, &game, cancelled)
                .await?;
            Ok(host)
        }));
    }
}
