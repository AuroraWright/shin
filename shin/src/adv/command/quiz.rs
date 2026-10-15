use shin_core::vm::command::types::{LayerId, LayerbankId, PlaneId};

use super::prelude::*;

#[derive(Debug)]
pub struct QUIZ {
    token: Option<command::token::QUIZ>,
    target: Option<(PlaneId, LayerbankId)>,
}

impl StartableCommand for command::runtime::QUIZ {
    type StateInfo = Option<(PlaneId, LayerbankId)>;

    fn apply_state(&self, state: &mut VmState) -> Self::StateInfo {
        // Like SELECT, QUIZ snapshots only without an active message (0xdcc0).
        if state.messagebox_state.text.is_none() {
            state.commit_persistent();
            state.record_interactive_boundary();
        }
        let plane = state.layers.current_plane;
        let layer = LayerId::try_new(u16::try_from(self.arg).ok()?)?;
        let bank = state
            .layers
            .layerbank_allocator
            .get_layerbank_id(plane, layer)?;
        Some((plane, bank))
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        target: Self::StateInfo,
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(
            QUIZ {
                token: Some(self.token),
                target,
            }
            .into(),
        )
    }
}

impl QUIZ {
    fn complete(&mut self, state: &mut VmState, result: i32) -> CommandResult {
        record_result(state, self.target, result);
        self.token.take().unwrap().finish(result)
    }
}

pub(super) fn record_result(
    state: &mut VmState,
    target: Option<(PlaneId, LayerbankId)>,
    result: i32,
) {
    if state.persistent_commits_enabled {
        state.route.record_quiz(result);
    }
    state.advance_save_position();
    state.capture_autosave();
    // Original completion (0xe23c) records the response on the layerbank.
    if let Some((_, bank)) = target {
        state.layers.layerbanks[bank].is_interation_completed = true;
        state.layers.layerbanks[bank].interaction_result = Some(result);
    }
}

impl UpdatableCommand for QUIZ {
    fn interactive_target(&self) -> Option<(PlaneId, LayerbankId)> {
        self.target
    }

    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        state: &mut VmState,
        adv: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // A missing/noninteractive layer has no overlay and returns zero (0xe1e8).
        let restored_result = self.target.and_then(|(_, bank)| {
            let bank = &state.layers.layerbanks[bank];
            bank.is_interation_completed
                .then_some(bank.interaction_result)
                .flatten()
        });
        let result = if let Some(result) = restored_result {
            result
        } else {
            match self
                .target
                .and_then(|(plane, bank)| adv.plane_layer_group(plane).get_layer(bank))
            {
                Some(layer) => match layer.interactive() {
                    Some(interactive) => interactive.result()?,
                    None => 0,
                },
                _ => 0,
            }
        };
        Some(self.complete(state, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::vm::command::types::LayerType;
    use shin_core::{
        format::scenario::instruction_elements::{NumberSpec, Register},
        vm::{IntoRuntimeForm, VmCtx},
    };

    #[test]
    fn targets_only_the_requested_layer_on_the_current_plane_and_records_result() {
        let mut state = VmState::new();
        let plane = PlaneId::new(2);
        let layer = LayerId::new(17);
        let bank = state
            .layers
            .layerbank_allocator
            .alloc_layerbank(plane, layer)
            .unwrap();
        state.layers.current_plane = plane;
        state.layers.layerbanks[bank].layer_type = Some(LayerType::Quiz);
        state.set_persistent(0, 65535);
        let command::RuntimeCommand::QUIZ(cmd) =
            command::CompiletimeCommand::QUIZ(command::compiletime::QUIZ {
                dest: Register::from_regular_register(1),
                arg: NumberSpec::constant(17),
            })
            .into_runtime_form(&VmCtx::new(0, 0))
        else {
            unreachable!()
        };
        let target = cmd.apply_state(&mut state);
        assert_eq!(target, Some((plane, bank)));
        assert_eq!(state.get_persistent(0), -1);
        let mut executing = QUIZ {
            token: Some(cmd.token),
            target,
        };
        let result = executing.complete(&mut state, 6);
        assert_eq!(state.save_position, 2); // No-text entry plus completion.
        assert_eq!(
            state
                .auto_save_slot
                .as_ref()
                .unwrap()
                .entry
                .route_data
                .reader()
                .read_quiz()
                .unwrap(),
            6
        );
        assert!(state.layers.layerbanks[bank].is_interation_completed);
        assert_eq!(state.layers.layerbanks[bank].interaction_result, Some(6));
        assert!(
            matches!(result, CommandResult::WriteMemory(register, 6) if register == Register::from_regular_register(1))
        );
    }
    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); tests quiz button responses through real script tokens"]
    fn reference_episode_quiz_buttons_return_original_indexes() {
        use crate::layer::user::{
            cake_layer::CakeModel,
            last_layer::LastModel,
            murderer_model::{MurdererModel, SUBMIT_ENTRY},
            quiz_layer::QuizModel,
        };
        enum Model {
            Buttons(QuizModel),
            Cake(CakeModel),
            Culprit(MurdererModel),
            Final(LastModel),
        }
        use shin_core::{
            format::{rom::RomReader, scenario::instruction_elements::CodeAddress},
            primitives::stateless_reader::StatelessFile,
            vm::Scripter,
        };
        use std::{collections::HashMap, fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut vm = Scripter::new(&scenario, 7, 42);
        let accusation_success = vm.add_breakpoint(CodeAddress(0x7d8b4b));
        let forced_success = vm.add_breakpoint(CodeAddress(0x7d890f));
        let mut state = VmState::new();
        let mut result = CommandResult::None;
        let mut models = HashMap::new();
        let mut answers = 0;
        let mut cakes = 0;
        let mut culprits = 0;
        let mut finals = 0;
        for _ in 0..500_000 {
            result = match vm.run(result).unwrap() {
                command::RuntimeCommand::PLANESELECT(cmd) => {
                    cmd.apply_state(&mut state);
                    cmd.token.finish()
                }
                command::RuntimeCommand::LAYERLOAD(cmd) if cmd.layer_type == LayerType::Quiz => {
                    cmd.apply_state(&mut state);
                    let VLayerIdRepr::Layer(id) = cmd.layer_id.repr() else {
                        panic!("Unexpected reference quiz layer");
                    };
                    if cmd.params.0 == 0 {
                        models.insert(
                            (state.layers.current_plane, id),
                            Model::Buttons(QuizModel::new(cmd.params.1)),
                        );
                    } else if cmd.params.0 == 1 {
                        models.insert(
                            (state.layers.current_plane, id),
                            Model::Cake(CakeModel::default()),
                        );
                    } else if cmd.params.0 == 2 {
                        models.insert(
                            (state.layers.current_plane, id),
                            Model::Culprit(MurdererModel::new(cmd.params.1)),
                        );
                    } else if cmd.params.0 == 3 {
                        models.insert(
                            (state.layers.current_plane, id),
                            Model::Final(LastModel::default()),
                        );
                    } else {
                        models.remove(&(state.layers.current_plane, id));
                    }
                    cmd.token.finish()
                }
                command::RuntimeCommand::QUIZ(cmd) => {
                    if let Some(model) =
                        models.get_mut(&(state.layers.current_plane, LayerId::new(cmd.arg as u16)))
                    {
                        let selected = match model {
                            Model::Buttons(model) => {
                                model.move_selection(-1);
                                model.confirm();
                                model.update(shin_core::time::Ticks::from_f32(7.0));
                                model.update(shin_core::time::Ticks::ZERO);
                                let selected =
                                    model.result.expect("No visible reference quiz button");
                                assert!(model.answers.contains(&(selected as usize)));
                                selected
                            }
                            Model::Final(model) => {
                                finals += 1;
                                model.navigate(-1);
                                assert_eq!(model.result, None);
                                model.update(7.0);
                                model.update(20.0);
                                model.confirm();
                                model.update(7.0);
                                assert_eq!(model.result, Some(0));
                                model.result.unwrap()
                            }
                            Model::Cake(model) => {
                                model.pointer(glam::Vec2::new(850.0, 700.0), true);
                                assert_eq!(model.result, Some(0));
                                cakes += 1;
                                0
                            }
                            Model::Culprit(model) => {
                                match culprits {
                                    0 => {
                                        assert!(model.open_menu(0));
                                        assert!(model.replay_chapter(5));
                                        assert_eq!(model.result, Some(5));
                                    }
                                    1 => {
                                        assert!(model.open_menu(4));
                                        assert_eq!(model.available_hints(), 1);
                                        assert!(model.request_hint(0));
                                        assert_eq!(model.result, Some(6));
                                    }
                                    2 => {
                                        assert!(model.open_menu(5));
                                        assert_eq!(model.available_hints(), 2);
                                        assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
                                        assert_eq!(model.result, Some(29));
                                    }
                                    3 => {
                                        assert!(model.open_menu(5));
                                        assert_eq!(model.available_hints(), 3);
                                        for entry in 34..37 {
                                            assert!(model.activate_accusation_entry(entry));
                                        }
                                        assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
                                        assert_eq!(model.result, Some(27));
                                    }
                                    _ => {
                                        panic!("Correct culprit response did not leave interaction")
                                    }
                                }
                                culprits += 1;
                                assert_eq!(model.poll_result(), None);
                                for _ in 0..128 {
                                    model.update(1.0);
                                }
                                model.poll_result().unwrap()
                            }
                        };
                        let target = cmd.apply_state(&mut state);
                        let mut executing = QUIZ {
                            token: Some(cmd.token),
                            target,
                        };
                        answers += 1;
                        executing.complete(&mut state, selected)
                    } else {
                        // A missing/noninteractive layer returns zero, like runtime.
                        cmd.token.finish(0)
                    }
                }
                other => match other.execute_dummy() {
                    Some(result) => result,
                    None => {
                        assert!(
                            answers > 0 && cakes > 0 && culprits == 4 && finals == 1,
                            "Episode did not exercise cake, quiz, culprit and final responses"
                        );
                        assert_eq!(accusation_success.hit_count(), 1);
                        assert_eq!(forced_success.hit_count(), 0);
                        return;
                    }
                },
            };
        }
        panic!("Reference episode did not reach EXIT");
    }
    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); verifies both final choice script branches"]
    fn reference_final_choice_routes_magic_and_trick_separately() {
        use crate::layer::user::last_layer::LastModel;
        use shin_core::{
            format::{rom::RomReader, scenario::instruction_elements::CodeAddress},
            primitives::stateless_reader::StatelessFile,
            vm::Scripter,
        };
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        for choice in 0..2 {
            let mut vm = Scripter::new(&scenario, 7, 42);
            vm.unsafe_set_position(CodeAddress(0x85e4cd));
            let magic = vm.add_breakpoint(CodeAddress(0x85e4f8));
            let trick = vm.add_breakpoint(CodeAddress(0x85e502));
            let mut state = VmState::new();
            let mut result = CommandResult::None;
            let mut answered = false;
            for _ in 0..1000 {
                result = match vm.run(result).unwrap() {
                    command::RuntimeCommand::LAYERLOAD(cmd) => {
                        assert_eq!(cmd.params.0, 3);
                        cmd.apply_state(&mut state);
                        cmd.token.finish()
                    }
                    command::RuntimeCommand::QUIZ(cmd) => {
                        assert!(!answered);
                        answered = true;
                        let mut model = LastModel::default();
                        model.navigate(if choice == 0 { -1 } else { 1 });
                        assert_eq!(model.result, None);
                        model.update(7.0);
                        model.update(20.0);
                        model.confirm();
                        model.update(7.0);
                        let target = cmd.apply_state(&mut state);
                        let mut executing = QUIZ {
                            token: Some(cmd.token),
                            target,
                        };
                        executing.complete(&mut state, model.result.unwrap())
                    }
                    command::RuntimeCommand::MSGCLOSE(_) if answered => break,
                    other => other
                        .execute_dummy()
                        .expect("Unexpected final choice command"),
                };
            }
            assert!(answered);
            assert_eq!(magic.hit_count(), u32::from(choice == 0));
            assert_eq!(trick.hit_count(), u32::from(choice == 1));
        }
    }
}
