//! Live ADV task joining saved-route history, browser screen and replay audio.
use super::{
    assets::AdvFonts, backlog_collection::BrowserHistory, backlog_screen::BacklogScreen,
    backlog_view::BacklogTextures, backlog_voice::BacklogVoiceReplay,
};
use crate::{app::AppAction, asset::system::AssetServer, update::UpdateContext};
use anyhow::Result;
use enum_map::EnumMap;
use shin_core::{
    format::{
        save::{GameData, GameDataEntry, SaveData},
        scenario::Scenario,
    },
    time::Ticks,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

struct RollbackTask {
    index: usize,
    backend: super::backlog_rollback::BacklogRollback,
    view: Option<super::dialog_view::DialogView>,
    input: super::dialog_input::DialogInput,
}

/// The browser remains the pause/UI owner while this child and its dialog run.
struct SceneJumpTask {
    model: super::backlog_jump::SceneJump,
    view: Option<super::backlog_jump_view::SceneJumpView>,
    progress: f32,
    closing: bool,
    pointer: super::backlog_jump_pointer::Pointer,
}
impl SceneJumpTask {
    fn is_open(&self) -> bool {
        self.progress == 1.0 && !self.closing && self.view.is_some()
    }
}

pub(super) struct BacklogModal {
    scenario: Arc<Scenario>,
    source: Box<GameData>,
    jump_source: Box<GameDataEntry>,
    globals: Box<SaveData>,
    rollback: Option<RollbackTask>,
    scene_jump: Option<SceneJumpTask>,
    current_row: usize,
    prepared_scene: Option<(Box<super::reconstruction::ReconstructedScene>, u32)>,
    installing: bool,
    loading: Option<
        shin_tasks::AsyncTask<
            Result<(
                BrowserHistory,
                Arc<BacklogTextures>,
                Arc<super::character_view::CharacterTextures>,
            )>,
        >,
    >,
    pub screen: Option<BacklogScreen>,
    replay: BacklogVoiceReplay,
    fonts: AdvFonts,
    outline_mode: u8,
    failed: bool,
    pointer: super::backlog_pointer::BacklogPointer,
    sounds: Vec<&'static str>,
    gesture: super::backlog_gesture::Gesture,
}
impl BacklogModal {
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.pointer.update(held, false, None);
        self.gesture.update(glam::Vec2::ZERO, held, false, 0.0);
        if let Some(screen) = &mut self.screen {
            screen.shell.cancel_close();
        }
        if let Some(task) = &mut self.rollback {
            task.input.ignore_pointer(held);
        }
        if let Some(menu) = &mut self.scene_jump {
            menu.pointer
                .update(&mut menu.model, glam::Vec2::ZERO, held, false, 0.0);
        }
    }
    pub fn new(
        scenario: Arc<Scenario>,
        game: GameData,
        globals: SaveData,
        assets: Arc<AssetServer>,
        audio: Arc<shin_audio::AudioManager>,
        fonts: AdvFonts,
    ) -> Self {
        let outline_mode = globals.settings.v8;
        let mut replay = BacklogVoiceReplay::new(audio, assets.clone(), scenario.clone());
        replay.set_settings_volume(globals.settings.v2_voicevol);
        let entry = game.entry.clone();
        let source = Box::new(game);
        let saved_globals = Box::new(globals.clone());
        let saved_scenario = scenario.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let history = BrowserHistory::collect(&scenario, &entry, &globals, 1_000_000)?;
            let textures = assets.load::<BacklogTextures, _>("/logview.txa").await?;
            let system = assets.load("/systex.txa").await?;
            Ok((history, textures, system))
        });
        Self {
            scenario: saved_scenario,
            jump_source: Box::new(source.entry.clone()),
            source,
            globals: saved_globals,
            rollback: None,
            scene_jump: None,
            current_row: 0,
            prepared_scene: None,
            installing: false,
            loading: Some(loading),
            screen: None,
            replay,
            fonts,
            outline_mode,
            failed: false,
            pointer: Default::default(),
            sounds: Vec::new(),
            gesture: Default::default(),
        }
    }
    pub fn input(&mut self, state: &EnumMap<AppAction, ActionState>, axis: f32) {
        if let Some(task) = &mut self.rollback {
            if let Some(view) = &task.view {
                task.input.keyboard(
                    &mut task.backend.confirmation,
                    view.geometry.vertical,
                    state,
                );
                self.sounds.extend(task.input.take_sounds());
            }
            return;
        }
        if let Some(menu) = &mut self.scene_jump {
            let Some(screen) = &mut self.screen else {
                return;
            };
            if !menu.is_open() || !menu.model.accepts_keyboard() || menu.pointer.take_consumed() {
                return;
            }
            let available = 0..=screen.model.rows.len() - 1;
            let delta = if state[AppAction::Up].is_clicked_or_repeated {
                -1
            } else if state[AppAction::AnyDown].is_clicked_or_repeated {
                1
            } else if state[AppAction::Left].is_clicked_or_repeated {
                -4
            } else if state[AppAction::Right].is_clicked_or_repeated {
                4
            } else if state[AppAction::CharacterPageUp].is_clicked_or_repeated {
                -(menu.model.markers.len() as i32)
            } else if state[AppAction::CharacterPageDown].is_clicked_or_repeated {
                menu.model.markers.len() as i32
            } else {
                0
            };
            let selected = if delta != 0 {
                menu.model.navigate(delta, available.clone())
            } else if state[AppAction::SceneJumpMenu].is_clicked {
                menu.model.reset_cursor(available)
            } else {
                None
            };
            if let Some(row) = selected {
                screen.model.select(row);
                self.sounds.push("move");
                return;
            }
            if state[AppAction::Cancel].is_clicked {
                screen.shell.flash_close();
                menu.closing = true;
                self.sounds.push("cancel");
                return;
            }
            if state[AppAction::ReplayVoice].is_clicked || state[AppAction::Enter].is_clicked {
                screen.replay_selected(&mut self.replay);
                return;
            }
            if state[AppAction::ConfirmSceneJump].is_clicked {
                self.request_rollback();
                return;
            }
            let wheel = if state[AppAction::TextScrollUp].is_clicked_or_repeated {
                -1.0
            } else if state[AppAction::TextScrollDown].is_clicked_or_repeated {
                1.0
            } else {
                axis
            };
            if let Some(row) = menu.model.axis(wheel) {
                screen.model.select(row);
            }
            return;
        }
        let Some(screen) = &mut self.screen else {
            return;
        };
        if !screen.shell.is_open()
            || self.gesture.take_consumed()
            || !screen.model.accepts_keyboard()
        {
            return;
        }
        let delta = if state[AppAction::Up].is_clicked_or_repeated {
            -1
        } else if state[AppAction::AnyDown].is_clicked_or_repeated {
            1
        } else if state[AppAction::Left].is_clicked_or_repeated {
            -4
        } else if state[AppAction::Right].is_clicked_or_repeated {
            4
        } else {
            0
        };
        if delta != 0 && screen.navigate(delta) {
            self.sounds.push("move");
            return;
        }
        if delta == 0 {
            let shoulder = if state[AppAction::CharacterPageDown].is_clicked_or_repeated {
                Some(true)
            } else if state[AppAction::CharacterPageUp].is_clicked_or_repeated {
                Some(false)
            } else {
                None
            };
            if shoulder.is_some_and(|forward| screen.navigate_checkpoint(forward)) {
                self.sounds.push("move");
                return;
            }
        }
        if state[AppAction::Cancel].is_clicked {
            self.replay.stop();
            self.sounds.push("cancel");
            screen.shell.request_close();
            return;
        }
        if state[AppAction::SceneJumpMenu].is_clicked {
            // Native a6d3c stops parent inertia for keyboard opening.
            screen.model.release(0.0);
            self.open_chapters();
            return;
        }
        if state[AppAction::ReplayVoice].is_clicked || state[AppAction::Enter].is_clicked {
            screen.replay_selected(&mut self.replay);
            return;
        }
        if state[AppAction::ConfirmSceneJump].is_clicked {
            self.request_rollback();
            return;
        }
        let wheel = if state[AppAction::TextScrollUp].is_clicked_or_repeated {
            -1.0
        } else if state[AppAction::TextScrollDown].is_clicked_or_repeated {
            1.0
        } else {
            axis
        };
        screen.axis(wheel);
    }
    pub(super) fn desktop_diagnostic_phase(&self) -> Option<&'static str> {
        if self.rollback.is_some() || self.installing {
            return None;
        }
        let screen = self.screen.as_ref()?;
        if !screen.shell.is_open() {
            return None;
        }
        if let Some(menu) = &self.scene_jump {
            (menu.is_open() && menu.model.accepts_keyboard()).then_some("chapters_ready")
        } else {
            screen.model.accepts_keyboard().then_some("ready")
        }
    }
    pub fn pointer(&mut self, position: glam::Vec2, clicked: bool) {
        if clicked {
            self.pointer_state(position, true, 1.0 / 60.0);
            self.pointer_state(position, false, 1.0 / 60.0);
        }
    }
    pub fn pointer_state(&mut self, position: glam::Vec2, held: bool, elapsed_seconds: f64) {
        self.pointer_state_with_mouse_release(position, held, elapsed_seconds, None)
    }
    pub fn pointer_state_with_mouse_release(
        &mut self,
        position: glam::Vec2,
        held: bool,
        elapsed_seconds: f64,
        mouse_release: Option<super::input_area::MouseRelease>,
    ) {
        use super::backlog_pointer::{Event, Target};
        if let Some(task) = &mut self.rollback {
            if let Some(menu) = &mut self.scene_jump {
                let events =
                    menu.pointer
                        .update(&mut menu.model, position, held, false, elapsed_seconds);
                if events
                    .iter()
                    .any(|e| matches!(e, super::backlog_jump_pointer::Event::CancelClose))
                {
                    if let Some(screen) = &mut self.screen {
                        screen.shell.cancel_close();
                    }
                }
            }
            self.pointer.update(held, false, None);
            self.gesture.update(position, held, false, elapsed_seconds);
            if let Some(view) = &mut task.view {
                task.input.pointer_timed(
                    &mut task.backend.confirmation,
                    &mut view.geometry,
                    position,
                    held,
                    elapsed_seconds,
                );
                self.sounds.extend(task.input.take_sounds());
            } else {
                task.input.ignore_pointer(held);
            }
            return;
        }
        if let (Some(menu), Some(screen)) = (&mut self.scene_jump, &mut self.screen) {
            self.pointer.update(held, false, None);
            self.gesture.update(position, held, false, elapsed_seconds);
            let eligible = menu.is_open();
            let events = menu.pointer.update_with_mouse_release(
                &mut menu.model,
                position,
                held,
                eligible,
                elapsed_seconds,
                mouse_release,
            );
            for event in events {
                use super::backlog_jump_pointer::Event as MenuEvent;
                match event {
                    MenuEvent::HoldClose => screen.shell.hold_close(),
                    MenuEvent::CancelClose => screen.shell.cancel_close(),
                    MenuEvent::CloseClick => {
                        screen.shell.flash_close();
                        menu.closing = true;
                        self.sounds.push("decide");
                    }
                    MenuEvent::Select(i) => {
                        if let Some(row) = menu
                            .model
                            .pointer_select(i, 0..=screen.model.rows.len() - 1)
                        {
                            screen.model.select(row);
                            self.sounds.push("move");
                        }
                    }
                    MenuEvent::Drag(dy) => {
                        if let Some(row) = menu.model.drag(dy) {
                            screen.model.select(row);
                        }
                    }
                    MenuEvent::Fling(v) => {
                        if menu.model.release(v) {
                            menu.closing = true;
                            self.sounds.push("cancel");
                            tracing::debug!(phase = "chapters_swipe_close", "UMINEKO_BROWSER");
                        }
                    }
                }
            }
            return;
        }
        let eligible = self
            .screen
            .as_ref()
            .is_some_and(|screen| screen.shell.is_open());
        let hit = self.screen.as_ref().and_then(|screen| {
            if screen.shell.close_at(position) {
                Some(Target::Close)
            } else {
                screen.model.row_at(position).map(Target::Row)
            }
        });
        let motion = self.gesture.update_browser_with_mouse_release(
            position,
            held,
            eligible,
            elapsed_seconds,
            mouse_release,
        );
        let cancelled = motion
            .cancel_click
            .then(|| self.pointer.cancel_capture())
            .flatten();
        let event = self.pointer.update(held, eligible, hit);
        let event = if motion.cancel_click {
            cancelled
        } else {
            event
        };
        // Disabled callbacks discard capture without retargeting the close task.
        if !eligible {
            return;
        }
        let Some(screen) = &mut self.screen else {
            return;
        };
        if let Some(delta) = motion.drag {
            if super::backlog_gesture::scroll_drag(delta) {
                screen.model.drag(delta.y);
            }
        }
        let mut request_rollback = false;
        let mut request_chapters = false;
        if let Some(velocity) = motion.fling {
            if super::backlog_gesture::scroll_fling(velocity) {
                screen.model.release(velocity.y);
            } else {
                request_chapters = true;
            }
        }
        match event {
            Some(Event::Press(Target::Close)) => screen.shell.hold_close(),
            Some(Event::Release(Target::Close)) => {
                self.sounds.push("decide");
                screen.shell.request_close();
            }
            Some(Event::Cancel(Target::Close)) => screen.shell.cancel_close(),
            Some(Event::Press(Target::Row(_))) => {
                screen.model.pointer_select(position);
                if position.x < 272.0 {
                    screen.hold_voice();
                }
            }
            Some(Event::Release(Target::Row(_))) => {
                screen.model.pointer_select(position);
                if position.x < 272.0 {
                    screen.replay_selected(&mut self.replay);
                } else {
                    request_rollback = true;
                }
            }
            Some(Event::Cancel(Target::Row(index))) => screen.cancel_voice(index),
            None => {}
        }
        if request_chapters {
            if self.open_chapters() {
                tracing::debug!(phase = "chapters_swipe_open", "UMINEKO_BROWSER");
            }
        } else if request_rollback {
            self.request_rollback();
        }
    }
    fn open_chapters(&mut self) -> bool {
        let Some(screen) = &self.screen else {
            return false;
        };
        // Original a7e94/a7ea4 and a6d24/a6d34 play the same confirm cue
        // before constructing the child for swipes and keyboard input.
        self.sounds.push("decide");
        match super::backlog_jump::SceneJump::new(
            screen.history(),
            self.current_row,
            screen.model.selected.unwrap_or(0),
        ) {
            Ok(model) => {
                self.scene_jump = Some(SceneJumpTask {
                    model,
                    view: None,
                    progress: 0.0,
                    closing: false,
                    pointer: Default::default(),
                });
                true
            }
            Err(error) => {
                tracing::warn!(%error, "Cannot open scene jump menu");
                self.sounds.push("error");
                false
            }
        }
    }
    fn request_rollback(&mut self) {
        if self.rollback.is_some() {
            return;
        }
        let Some(screen) = &mut self.screen else {
            return;
        };
        let mut game = (*self.source).clone();
        game.entry = (*self.jump_source).clone();
        match screen.prepare_rollback(&game) {
            Ok((index, mut backend)) => {
                backend.previous_position = self.source.entry.save_position;
                self.rollback = Some(RollbackTask {
                    index,
                    backend,
                    view: None,
                    input: Default::default(),
                });
            }
            Err(error) => {
                tracing::warn!(%error,"Cannot jump to backlog row");
                self.sounds.push("error");
            }
        }
    }
    #[cfg(test)]
    pub fn reference_render_scene_jump(&self, pass: &mut RenderPass) {
        if let (Some(menu), Some(screen)) = (&self.scene_jump, &self.screen) {
            if let Some(view) = &menu.view {
                view.render(pass, screen.textures(), &menu.model, menu.progress);
            }
        }
    }
    #[cfg(test)]
    pub fn reference_scene_jump(&self) -> Option<(&super::backlog_jump::SceneJump, f32, bool)> {
        self.scene_jump
            .as_ref()
            .map(|m| (&m.model, m.progress, m.is_open()))
    }
    #[cfg(test)]
    pub fn reference_requested_feedback(&self) -> Option<(bool, bool)> {
        let index = self.rollback.as_ref()?.index;
        let screen = self.screen.as_ref()?;
        Some((
            screen.has_cached_row(index),
            !screen.confirmation_feedback_finished(index),
        ))
    }
    #[cfg(test)]
    pub fn reference_dialog_created(&self) -> bool {
        self.rollback.as_ref().is_some_and(|t| t.view.is_some())
    }
    #[cfg(test)]
    pub fn reference_requested_jump_position(&self) -> Option<u32> {
        Some(
            self.screen
                .as_ref()?
                .history()
                .entries
                .get(self.rollback.as_ref()?.index)?
                .save_position,
        )
    }
    #[cfg(test)]
    pub fn reference_selected_jump_position(&self) -> Option<u32> {
        let screen = self.screen.as_ref()?;
        Some(
            screen
                .history()
                .entries
                .get(screen.model.selected?)?
                .save_position,
        )
    }
    #[cfg(test)]
    pub fn reference_prepared_jump_position(&self) -> Option<u32> {
        self.prepared_scene
            .as_ref()
            .map(|(scene, _)| scene.state.save_position)
    }
    #[cfg(test)]
    pub fn reference_confirmation(&self) -> Option<&super::backlog_rollback::Confirmation> {
        self.rollback
            .as_ref()
            .map(|task| &task.backend.confirmation)
    }
    pub fn take_prepared_scene(
        &mut self,
    ) -> Option<(Box<super::reconstruction::ReconstructedScene>, u32)> {
        if !self.screen.as_ref()?.shell.is_closed() {
            return None;
        }
        let scene = self.prepared_scene.take()?;
        self.installing = true;
        Some(scene)
    }
    pub fn recover_restore_failure(&mut self) {
        self.installing = false;
        if let Some(screen) = &mut self.screen {
            screen.shell.reopen();
        }
        self.sounds.push("error");
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn update(&mut self, context: &mut UpdateContext) -> bool {
        if let Some(task) = &mut self.loading {
            if let Some(result) = task.poll_naive() {
                self.loading = None;
                let result = result.and_then(|(collected, textures, system)| {
                    let current = collected
                        .current_row
                        .ok_or_else(|| anyhow::anyhow!("No available backlog entries"))?;
                    self.current_row = current;
                    self.jump_source = Box::new(collected.jump_source);
                    BacklogScreen::new(
                        collected.history,
                        self.fonts.clone(),
                        textures,
                        system,
                        current,
                        self.outline_mode,
                    )
                });
                match result {
                    Ok(screen) => self.screen = Some(screen),
                    Err(error) => {
                        tracing::warn!(%error,"Failed to open backlog");
                        self.failed = true;
                    }
                }
            }
        }
        self.replay.update();
        if let Some(screen) = &mut self.screen {
            screen.update(context.delta_ticks);
            screen.prepare(context.pre_render);
            if let Some(menu) = &mut self.scene_jump {
                if menu.view.is_none() {
                    match super::backlog_jump_view::SceneJumpView::load(
                        context.asset_server,
                        screen.history(),
                        &menu.model,
                    ) {
                        Ok(view) => menu.view = Some(view),
                        Err(error) => {
                            tracing::warn!(%error, "Failed to load scene jump menu");
                            self.sounds.push("error");
                            self.scene_jump = None;
                        }
                    }
                }
                if let Some(menu) = &mut self.scene_jump {
                    if menu.closing {
                        if screen.shell.close_feedback_finished() {
                            menu.progress =
                                (menu.progress - context.delta_ticks.as_f32() * 0.2).max(0.0);
                        }
                    } else {
                        menu.progress =
                            (menu.progress + context.delta_ticks.as_f32() * 0.2).min(1.0);
                    }
                    if let Some(row) = menu.model.update_with_delta(context.delta_ticks) {
                        screen.model.select(row);
                    }
                    if let Some(view) = &mut menu.view {
                        view.prepare(context.asset_server, &menu.model);
                    }
                    if menu.closing && menu.progress == 0.0 {
                        self.scene_jump = None;
                        tracing::debug!(
                            phase = "chapters_retired",
                            browser_open = screen.shell.is_open(),
                            "UMINEKO_BROWSER"
                        );
                    }
                }
            }

            if let Some(task) = &mut self.rollback {
                if task.view.is_some() || screen.current_confirmation_feedback_finished() {
                    if task.view.is_none() {
                        match super::dialog_view::DialogView::load(context.asset_server) {
                            Ok(view) => task.view = Some(view),
                            Err(error) => {
                                tracing::warn!(%error, "Failed to open scene jump dialog");
                                self.sounds.push("error");
                                self.rollback = None;
                            }
                        }
                    }
                    if let Some(task) = &mut self.rollback {
                        if let Some(view) = &mut task.view {
                            if let Some(scale) = super::dialog_view::DialogGeometry::pointer_scale(
                                context.pre_render.resize_source.canvas_handle().get(),
                            ) {
                                task.input.set_pointer_scale(scale);
                            }
                            task.backend.confirmation.update(context.delta_ticks);
                            task.input
                                .update_motion(&mut view.geometry, context.delta_ticks);
                            view.prepare(context.pre_render, &task.backend.confirmation);
                        }
                        if task.backend.confirmation.is_done() {
                            if let Some(result) =
                                task.backend.reconstruct_confirmed_at_current_boxed(
                                    screen.history(),
                                    screen.model.selected,
                                    &self.jump_source,
                                    &self.scenario,
                                    &self.globals,
                                    1_000_000,
                                )
                            {
                                match result {
                                    Ok(scene) => {
                                        self.prepared_scene =
                                            Some((scene, task.backend.previous_position));
                                        if let Some(menu) = &mut self.scene_jump {
                                            menu.closing = true;
                                        }
                                        screen.shell.begin_close();
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "Failed to reconstruct scene jump");
                                        self.sounds.push("error");
                                    }
                                }
                            }
                            self.rollback = None;
                        }
                    }
                }
            }
            if screen.shell.is_closed() {
                self.replay.stop();
                return self.prepared_scene.is_none() && !self.installing;
            }
        }
        self.failed
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some(screen) = &self.screen {
            screen.render_with_menu(pass, self.scene_jump.is_some());
        }
        if let (Some(menu), Some(screen)) = (&self.scene_jump, &self.screen) {
            if let Some(view) = &menu.view {
                view.render(pass, screen.textures(), &menu.model, menu.progress);
            }
        }
        if let Some(task) = &self.rollback {
            if let Some(view) = &task.view {
                view.render(pass, &task.backend.confirmation);
            }
        }
    }
    pub fn is_playing(&self) -> bool {
        self.replay.is_playing()
    }
    pub fn set_volume(&mut self, percent: u8) {
        self.replay.set_settings_volume(percent);
    }
}
