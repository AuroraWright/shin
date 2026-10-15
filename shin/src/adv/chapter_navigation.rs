//! Native12c80/12f80 generic confirmations and18510/182c0 final callbacks.
use super::{Adv, SaveMenuDialog, reconstruction::ReconstructedScene};
use crate::{app::AppAction, update::UpdateContext};
use enum_map::EnumMap;
use shin_input::ActionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Next,
    Previous,
}
impl Direction {
    fn prompt(self, disallow_unread: bool) -> &'static str {
        match (self, disallow_unread) {
            (Self::Next, true) => {
                "次のチャプター、未読メッセージ、選択肢まで@rスキップします。よろしいですか？"
            }
            (Self::Next, false) => "次のチャプターか選択肢までスキップします。@rよろしいですか？",
            (Self::Previous, _) => "前のチャプターに戻ります。@rよろしいですか？",
        }
    }
}
impl Adv {
    fn chapter_target(&self, direction: Direction) -> anyhow::Result<Option<ReconstructedScene>> {
        let mut game = self.vm_state.snapshot_game_data();
        let globals = self.save_menu_data();
        match direction {
            Direction::Next => ReconstructedScene::next_chapter_game_data(
                &self.scenario,
                &game,
                &globals,
                1_000_000,
            ),
            Direction::Previous => {
                game.entry.save_position = self.vm_state.previous_chapter_position();
                ReconstructedScene::from_game_data(&self.scenario, &game, &globals, 1_000_000)
                    .map(Some)
            }
        }
    }
    pub(super) fn has_next_chapter(&self) -> anyhow::Result<bool> {
        let blocked = matches!(
            self.current_command,
            Some(
                super::command::ExecutingCommand::SELECT(_)
                    | super::command::ExecutingCommand::QUIZ(_)
            )
        ) || (self.vm_state.current_event.is_none()
            && self.vm_state.settings.v11_disallowskipunread
            && self.vm_state.messagebox_state.text.is_some()
            && !self.vm_state.messagebox_state.was_read
            && !self
                .vm_state
                .save_vectors
                .seen_messages
                .contains(self.vm_state.messagebox_state.message_id.0 as usize));
        if blocked {
            return Ok(self.vm_state.chapter_skip_position > self.vm_state.save_position);
        }
        let mut state = self.vm_state.clone();
        let mut scripter = self.scripter.fork_for_simulation();
        // SELECT/QUIZ are handled above. All other yielded commands have a void
        // completion; a synchronous command may instead leave a pending result.
        let result = if self.current_command.is_some() {
            shin_core::vm::command::CommandResult::None
        } else {
            self.pending_result
        };
        ReconstructedScene::search_next_chapter(&mut scripter, &mut state, result, 1_000_000)?;
        Ok(state.chapter_skip_position > self.vm_state.save_position)
    }
    pub(super) fn update_chapter_navigation(
        &mut self,
        context: &mut UpdateContext,
        input: &EnumMap<AppAction, ActionState>,
    ) -> bool {
        if let Some(direction) = self.pending_chapter.take() {
            // Probe the continuation on private CPU state, avoiding a replay
            // of the whole episode before opening the prompt. Final Yes still
            // reconstructs from the current boundary, so events can advance.
            if direction == Direction::Next {
                match self.has_next_chapter() {
                    Ok(true) => {}
                    Ok(false) => {
                        self.adv_state.play_system_sound(
                            context.asset_server,
                            "error",
                            Default::default(),
                        );
                        return false;
                    }
                    Err(error) => {
                        self.report_scene_load_error(error, context);
                        return true;
                    }
                }
            }
            match SaveMenuDialog::load_prompt(
                context.asset_server,
                direction.prompt(self.vm_state.settings.v11_disallowskipunread),
                self.modal_pointer_held,
            ) {
                Ok(mut dialog) => {
                    dialog.set_pointer_extent(self.modal_pointer_extent, self.modal_pointer_held);
                    dialog.prepare(context.pre_render);
                    self.chapter_dialog = Some((direction, dialog));
                    self.adv_state.playback_modes.reset();
                    self.adv_state.play_system_sound(
                        context.asset_server,
                        "decide",
                        Default::default(),
                    );
                }
                Err(error) => {
                    self.report_scene_load_error(error, context);
                    return true;
                }
            }
            return false;
        }
        let Some((direction, dialog)) = &mut self.chapter_dialog else {
            return false;
        };
        let direction = *direction;
        if !self.vm_state.system_controls_allowed {
            // Native predicate18420 closes the child when SYS revokes it.
            dialog.cancel();
        }
        dialog.keyboard(input);
        let responses = dialog.update(context.delta_ticks);
        dialog.prepare(context.pre_render);
        for sound in dialog.take_sounds() {
            self.adv_state
                .play_system_sound(context.asset_server, sound, Default::default());
        }
        let Some(response) = responses.into_iter().find(|response| response.final_flag) else {
            return false;
        };
        self.chapter_dialog = None;
        // The native final callback guards the system flag and active loader;
        // an intermediate selection or cancellation never installs a scene.
        if response.button != 0
            || !self.vm_state.system_controls_allowed
            || self.is_loading_saved_scene()
        {
            return false;
        }
        match self.chapter_target(direction) {
            Ok(Some(scene)) => {
                let prior = Some(match direction {
                    Direction::Next => self.vm_state.save_position,
                    Direction::Previous => scene.state.save_position,
                });
                self.queue_reconstructed_scene(
                    Box::new(scene),
                    context.asset_server.clone(),
                    prior,
                );
                self.update_saved_scene_load(context);
                true
            }
            Ok(None) => {
                self.adv_state
                    .play_system_sound(context.asset_server, "error", Default::default());
                false
            }
            Err(error) => {
                self.report_scene_load_error(error, context);
                true
            }
        }
    }
}
