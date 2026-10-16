//! Saved-route message browser data (native MessageBrowser::LogBuilder).
mod collection;
mod decoration;
mod feedback;
pub(super) mod gesture;
mod help;
pub(super) mod jump;
mod jump_pointer;
mod jump_view;
pub(super) mod modal;
mod model;
mod pointer;
pub(super) mod rollback;
pub(super) mod screen;
mod text;
mod view;
pub(super) mod voice;
use anyhow::{Result, bail};
use shin_core::{
    format::{
        save::{GameDataEntry, SaveData},
        scenario::Scenario,
    },
    vm::command::{RuntimeCommand, types::MessageboxType},
};

use super::reconstruction::ReconstructedScene;

#[derive(Debug, PartialEq)]
pub(super) enum BacklogItem {
    Message {
        message_id: u32,
        text: String,
        messagebox_type: MessageboxType,
    },
    Choice {
        title: String,
        text: String,
        response: i32,
    },
    Quiz {
        argument: i32,
        mode: Option<i32>,
        response: i32,
    },
    Checkpoint {
        title: String,
        text: String,
    },
}

#[derive(Debug, PartialEq)]
pub(super) struct BacklogEntry {
    pub save_position: u32,
    pub item: BacklogItem,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Backlog {
    pub entries: Vec<BacklogEntry>,
}

impl Backlog {
    /// Rebuild the recorded prefix, not current-global branch decisions. Entry
    /// includes the displayed boundary message; an open choice has no result.
    pub fn from_save(
        scenario: &Scenario,
        entry: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        let mut entries = Vec::new();
        let mut invalid_choice = None;
        ReconstructedScene::from_save_with_observer(
            scenario,
            entry,
            globals,
            command_limit,
            |command, state, response| {
                let item = match item_for_command(command, state, response) {
                    Ok(Some(item)) => item,
                    Ok(None) => return,
                    Err(error) => {
                        invalid_choice = Some(error);
                        return;
                    }
                };
                entries.push(BacklogEntry {
                    save_position: state.save_position,
                    item,
                });
            },
        )?;
        if let Some(error) = invalid_choice {
            return Err(error);
        }
        Ok(Self { entries })
    }
}

pub(super) fn item_for_command(
    command: &RuntimeCommand,
    state: &super::VmState,
    response: Option<i32>,
) -> Result<Option<BacklogItem>> {
    let item = match (command, response) {
        (RuntimeCommand::MSGSET(command), _) => BacklogItem::Message {
            message_id: command.msg_id.0,
            text: command.text.clone(),
            messagebox_type: state.messagebox_state.msginit.messagebox_type,
        },
        (RuntimeCommand::SELECT(command), Some(response)) => {
            let Some(text) = usize::try_from(response)
                .ok()
                .and_then(|i| command.variants.get(i))
            else {
                bail!("Saved SELECT response {response} has no authored variant");
            };
            BacklogItem::Choice {
                title: command.choice_title.clone(),
                text: strip_choice_prefix(text).to_owned(),
                response,
            }
        }
        (RuntimeCommand::QUIZ(command), Some(response)) => BacklogItem::Quiz {
            argument: command.arg,
            mode: u16::try_from(command.arg)
                .ok()
                .and_then(shin_core::vm::command::types::LayerId::try_new)
                .and_then(|layer| {
                    state
                        .layers
                        .layerbank_allocator
                        .get_layerbank_id(state.layers.current_plane, layer)
                })
                .map(|bank| state.layers.layerbanks[bank].params.0),
            response,
        },
        (RuntimeCommand::AUTOSAVE(_), _) => BacklogItem::Checkpoint {
            title: state.save_info.info[0].clone(),
            text: state.save_info.info[1].clone(),
        },
        _ => return Ok(None),
    };
    Ok(Some(item))
}

fn strip_choice_prefix(text: &str) -> &str {
    if text.starts_with('*') {
        if let Some((_, remainder)) = text.split_once('.') {
            return remainder;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::{StartableCommand, VmState, command};

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_saved_route_and_entry_collection() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, expected) in [
            (0xa31c4, 0x94012127),
            (0xa3204, 0x94012ec7),
            (0xab1f4, 0x97fe25bb),
            (0xab218, 0x97fe25b6),
            (0xab2e0, 0x11000508),
            (0xab398, 0x2900e51a),
            (0xab3a4, 0xf9001116),
            (0xab5a8, 0x97fe24fe),
            (0xab5e4, 0x7100a91f),
            (0xab5fc, 0x7100b93f),
            (0xab59c, 0x6b09011f),
            (0xab4a0, 0x11000508),
            (0xab5c4, 0xb900a668),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                expected,
                "native backlog {a:x}"
            );
        }
        let name = b"N4shin14MessageBrowser10LogBuilderE";
        assert!(exe.windows(name.len()).any(|bytes| bytes == name));
    }
    use binrw::{BinRead, BinWrite};
    use shin_core::{
        format::{
            save::RouteRecorder,
            scenario::{
                instruction_elements::{MessageId, NumberSpec, Register, U8Bool},
                instructions::Instruction,
                types::{StringArray, U16FixupString, U16String},
            },
        },
        vm::{
            Scripter,
            command::{CommandResult, CompiletimeCommand as C, compiletime as c},
        },
    };

    pub(in crate::adv) fn fixture(messages: bool) -> Scenario {
        let header: [u32; 47] = [
            0x20524e53, 0, 0, 6, 19, 0, 0, 0, 0xbc, 0x58, 0x60, 0x68, 0x70, 0x78, 0x80, 0x88, 0x90,
            0x94, 0x98, 0x9c, 0xa4, 0xa8, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 0, 0, 0, 4, 0,
            0, 4, 0, 0, 0, 0,
        ];
        let mut bytes: Vec<_> = header.into_iter().flat_map(u32::to_le_bytes).collect();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(0xbc);
        let mut params_bytes = std::io::Cursor::new(vec![1u8]);
        params_bytes.set_position(1);
        NumberSpec::<i32>::constant(2)
            .write_le(&mut params_bytes)
            .unwrap();
        params_bytes.set_position(0);
        let quiz_params =
            shin_core::format::scenario::instruction_elements::BitmaskNumberArray::read_le(
                &mut params_bytes,
            )
            .unwrap();
        for command in [
            C::SGET(c::SGET {
                dest: Register::from_regular_register(1),
                slot_number: NumberSpec::constant(0),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(12),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("First@vvoice_1."),
            }),
            C::SELECT(c::SELECT {
                choice_set_base: 1,
                choice_index: 1,
                dest: Register::from_regular_register(2),
                choice_visibility_mask: NumberSpec::register(Register::from_regular_register(1)),
                choice_title: U16String::new("Question"),
                variants: StringArray::new(["*voice.No", "*voice.Yes"]),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(13),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("Second"),
            }),
            C::LAYERLOAD(c::LAYERLOAD {
                layer_id: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
                layer_type: NumberSpec::new(NumberSpec::constant(9).into_untyped()),
                flags: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
                params: quiz_params,
            }),
            C::QUIZ(c::QUIZ {
                arg: NumberSpec::constant(0),
                dest: Register::from_regular_register(3),
            }),
            C::SAVEINFO(c::SAVEINFO {
                level: NumberSpec::constant(0),
                info: U16FixupString::new("Episode"),
            }),
            C::SAVEINFO(c::SAVEINFO {
                level: NumberSpec::constant(1),
                info: U16FixupString::new("Chapter"),
            }),
            C::AUTOSAVE(c::AUTOSAVE {}),
            C::EXIT(c::EXIT {
                arg1: 0,
                arg2: NumberSpec::constant(0),
            }),
        ] {
            if !messages && matches!(command, C::MSGSET(_)) {
                continue;
            }
            Instruction::Command(command).write_le(&mut cursor).unwrap();
        }
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        Scenario::new(bytes.into()).unwrap()
    }

    fn confirm_jump(jump: &mut crate::adv::backlog::rollback::BacklogRollback) {
        use shin_core::time::Ticks;
        jump.confirmation.update(Ticks::from_f32(12.0));
        jump.confirmation.update(Ticks::ZERO);
        assert!(jump.confirmation.select(0));
        assert!(jump.confirmation.confirm());
        jump.confirmation.update(Ticks::from_f32(7.0));
        jump.confirmation.update(Ticks::ZERO);
        jump.confirmation.update(Ticks::from_f32(12.0));
        jump.confirmation.update(Ticks::from_f32(5.0));
        assert!(jump.confirmation.is_done());
    }

    #[test]
    fn rollback_uses_saved_boundary_seed_and_historical_route_without_mutating_globals() {
        use super::super::{backlog::rollback::BacklogRollback, reconstruction::ReconstructionStop};
        let scenario = fixture(true);
        let mut globals = SaveData::default();
        globals.persist_data.set(0, 1);
        let before = globals.encode();
        let mut route = RouteRecorder::default();
        route.record_persistent(2);
        route.record_choice(1);
        route.record_quiz(27);
        let source = GameDataEntry {
            scenario_id: 7,
            random_seed: 42,
            save_position: 5,
            route_data: route.snapshot(),
        };
        let history = Backlog::from_save(&scenario, &source, &globals, 100).unwrap();
        let mut game = shin_core::format::save::GameData::from_entry(source.clone());
        game.primary_entry = Some(GameDataEntry {
            scenario_id: 19,
            random_seed: 99,
            save_position: 0xffff_fffe,
            route_data: route.snapshot(),
        });
        let retained = format!("{:?}", game.primary_entry);
        // Choice/QUIZ rows share the preceding message's native rollback boundary.
        for (index, position, message) in [(0, 1, 12), (1, 1, 12), (2, 3, 13), (3, 3, 13)] {
            let mut jump = BacklogRollback::from_game_data(&history, index, &game).unwrap();
            assert_eq!(jump.previous_position, 5);
            assert!(
                jump.reconstruct_confirmed(&scenario, &globals, 100)
                    .is_none()
            );
            confirm_jump(&mut jump);
            let scene = jump
                .reconstruct_confirmed(&scenario, &globals, 100)
                .unwrap()
                .unwrap();
            assert_eq!(scene.state.save_position, position);
            assert_eq!(format!("{:?}", scene.state.primary_entry), retained);
            let snapshot = scene.state.snapshot_entry();
            assert_eq!((snapshot.scenario_id, snapshot.random_seed), (7, 42));
            let ReconstructionStop::Command(RuntimeCommand::MSGSET(command)) = scene.stop else {
                panic!("Expected native message boundary");
            };
            assert_eq!(command.msg_id.0, message);
            assert_eq!(
                scene.state.route.bit_position(),
                if position == 1 { 17 } else { 21 }
            );
            assert!(
                jump.reconstruct_confirmed(&scenario, &globals, 100)
                    .is_none()
            );
        }
        let mut jump = BacklogRollback::new(&history, 4, &source).unwrap();
        confirm_jump(&mut jump);
        let scene = jump
            .reconstruct_confirmed(&scenario, &globals, 100)
            .unwrap()
            .unwrap();
        assert!(matches!(
            scene.stop,
            ReconstructionStop::Command(RuntimeCommand::AUTOSAVE(_))
        ));
        assert_eq!(scene.state.route.bit_position(), 27);
        assert_eq!(globals.encode(), before);
        assert_eq!(source.save_position, 5);
        assert_eq!(source.route_data.reader().read_persistent().unwrap(), 2);
    }

    #[test]
    fn canceled_and_invalid_rollbacks_never_reconstruct_a_scene() {
        use crate::adv::backlog::rollback::BacklogRollback;
        use shin_core::time::Ticks;
        let scenario = fixture(true);
        let globals = SaveData::default();
        let mut route = RouteRecorder::default();
        route.record_persistent(2);
        route.record_choice(1);
        route.record_quiz(27);
        let source = GameDataEntry {
            scenario_id: 7,
            random_seed: 42,
            save_position: 5,
            route_data: route.snapshot(),
        };
        let mut history = Backlog::from_save(&scenario, &source, &globals, 100).unwrap();
        assert!(BacklogRollback::new(&history, usize::MAX, &source).is_err());
        for cancel in [true, false] {
            let mut jump = BacklogRollback::new(&history, 0, &source).unwrap();
            jump.confirmation.update(Ticks::from_f32(12.0));
            jump.confirmation.update(Ticks::ZERO);
            if cancel {
                assert!(jump.confirmation.cancel());
            } else {
                assert!(jump.confirmation.confirm());
            }
            jump.confirmation.update(Ticks::from_f32(7.0));
            jump.confirmation.update(Ticks::ZERO);
            jump.confirmation.update(Ticks::from_f32(12.0));
            jump.confirmation.update(Ticks::from_f32(5.0));
            assert!(jump.confirmation.is_done());
            assert!(jump.reconstruct_confirmed(&scenario, &globals, 0).is_none());
        }
        history.entries[0].save_position = 6;
        assert!(BacklogRollback::new(&history, 0, &source).is_err());
        history.entries[0].save_position = 0;
        assert!(BacklogRollback::new(&history, 0, &source).is_err());
        history.entries[0].save_position = 3;
        let mut bad_source = source.clone();
        bad_source.route_data = Default::default();
        let mut jump = BacklogRollback::new(&history, 0, &bad_source).unwrap();
        confirm_jump(&mut jump);
        assert!(
            jump.reconstruct_confirmed(&scenario, &globals, 100)
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn rebuilds_recorded_prefix_and_does_not_invent_open_choice_response() {
        let scenario = fixture(true);
        let mut globals = SaveData::default();
        globals.persist_data.set(0, 1); // Historical visibility mask was2.
        globals.save_vectors.select_last_choices = vec![1]; // Historical response was1, not0.
        let before = globals.encode();
        let mut route = RouteRecorder::default();
        route.record_persistent(2);
        route.record_choice(1);
        route.record_quiz(27);
        let entry = |save_position| GameDataEntry {
            scenario_id: 7,
            random_seed: 42,
            save_position,
            route_data: route.snapshot(),
        };
        let message = BacklogEntry {
            save_position: 1,
            item: BacklogItem::Message {
                message_id: 12,
                text: "First@vvoice_1.".into(),
                messagebox_type: MessageboxType::Neutral,
            },
        };
        assert_eq!(
            Backlog::from_save(&scenario, &entry(1), &globals, 100)
                .unwrap()
                .entries,
            [message]
        );
        let backlog = Backlog::from_save(&scenario, &entry(2), &globals, 100).unwrap();
        assert_eq!(backlog.entries.len(), 2);
        assert_eq!(
            backlog.entries[1],
            BacklogEntry {
                save_position: 1,
                item: BacklogItem::Choice {
                    title: "Question".into(),
                    text: "Yes".into(),
                    response: 1
                }
            }
        );
        let backlog = Backlog::from_save(&scenario, &entry(3), &globals, 100).unwrap();
        assert_eq!(backlog.entries.len(), 3);
        assert_eq!(
            backlog.entries[2],
            BacklogEntry {
                save_position: 3,
                item: BacklogItem::Message {
                    message_id: 13,
                    text: "Second".into(),
                    messagebox_type: MessageboxType::Neutral
                }
            }
        );
        assert_eq!(globals.encode(), before);
        assert!(Backlog::from_save(&scenario, &entry(3), &globals, 1).is_err());
        let backlog = Backlog::from_save(&scenario, &entry(4), &globals, 100).unwrap();
        assert_eq!(backlog.entries.len(), 4);
        assert_eq!(
            backlog.entries[3],
            BacklogEntry {
                save_position: 3,
                item: BacklogItem::Quiz {
                    argument: 0,
                    mode: Some(2),
                    response: 27
                }
            }
        );
        let backlog = Backlog::from_save(&scenario, &entry(5), &globals, 100).unwrap();
        assert_eq!(backlog.entries.len(), 5);
        assert_eq!(
            backlog.entries[4],
            BacklogEntry {
                save_position: 5,
                item: BacklogItem::Checkpoint {
                    title: "Episode".into(),
                    text: "Chapter".into()
                }
            }
        );
        assert!(Backlog::from_save(&scenario, &entry(6), &globals, 100).is_err());
        let mut invalid = RouteRecorder::default();
        invalid.record_persistent(2);
        invalid.record_choice(5);
        assert!(
            Backlog::from_save(
                &scenario,
                &GameDataEntry {
                    route_data: invalid.snapshot(),
                    ..entry(2)
                },
                &globals,
                100
            )
            .is_err()
        );
        let no_text = fixture(false);
        assert!(
            Backlog::from_save(&no_text, &entry(1), &globals, 100)
                .unwrap()
                .entries
                .is_empty(),
            "Open no-text SELECT has no historical response"
        );
        let pending_quiz = Backlog::from_save(&no_text, &entry(3), &globals, 100).unwrap();
        assert_eq!(pending_quiz.entries.len(), 1);
        assert!(
            matches!(
                pending_quiz.entries[0].item,
                BacklogItem::Choice { response: 1, .. }
            ),
            "Open QUIZ must retain previous choice but invent no quiz response"
        );
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); verifies history replay, not UI"]
    fn reference_backlog_messages_match_recorded_supplied_script_prefix() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{fs::File, io::Read};
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let file = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut state = VmState::for_scenario(1, 12345);
        let mut scripter = Scripter::new(&scenario, 1, 12345);
        let mut result = CommandResult::None;
        let mut expected = Vec::new();
        for _ in 0..500_000 {
            let command = scripter.run(result).unwrap();
            result = match command {
                RuntimeCommand::SGET(command) => {
                    let value = command.apply_state(&mut state);
                    command.token.finish(value)
                }
                RuntimeCommand::SELECT(command) => {
                    command.apply_state(&mut state);
                    let value = (0..command.variants.len().min(12))
                        .find(|&i| command.choice_visibility_mask as u32 & (1 << i) != 0)
                        .unwrap() as i32;
                    command::replay_choice_result(
                        &mut state,
                        command.choice_set_base,
                        command.choice_index,
                        value,
                    );
                    command.token.finish(value)
                }
                RuntimeCommand::QUIZ(_) => panic!("Unexpected QUIZ in initial story prefix"),
                command => {
                    command::apply_command_state_ref(&command, &mut state).unwrap();
                    if let RuntimeCommand::MSGSET(message) = &command {
                        expected.push(BacklogEntry {
                            save_position: state.save_position,
                            item: BacklogItem::Message {
                                message_id: message.msg_id.0,
                                text: message.text.clone(),
                                messagebox_type: state.messagebox_state.msginit.messagebox_type,
                            },
                        });
                    }
                    command
                        .execute_dummy()
                        .expect("Story must contain80 messages")
                }
            };
            if expected.len() == 80 {
                break;
            }
        }
        assert_eq!(expected.len(), 80);
        let globals = SaveData {
            persist_data: state.persist.clone(),
            save_vectors: state.save_vectors.clone(),
            ..Default::default()
        };
        let entry = GameDataEntry {
            scenario_id: 1,
            random_seed: 12345,
            save_position: state.save_position,
            route_data: state.route.snapshot(),
        };
        let history = Backlog::from_save(&scenario, &entry, &globals, 500_000).unwrap();
        let messages: Vec<_> = history
            .entries
            .iter()
            .filter(|entry| matches!(entry.item, BacklogItem::Message { .. }))
            .collect();
        assert_eq!(messages, expected.iter().collect::<Vec<_>>());
        // Roll back to the40th boundary, but retain all80 seen message IDs.
        // The native builder should recover the following40 authored rows.
        let mut future_globals = globals.clone();
        for message in &expected {
            if let BacklogItem::Message { message_id, .. } = message.item {
                future_globals
                    .save_vectors
                    .seen_messages
                    .add(message_id as usize);
            }
        }
        let before_future = future_globals.encode();
        let mut earlier = entry.clone();
        earlier.save_position = messages[39].save_position;
        let collected = crate::adv::backlog::collection::BrowserHistory::collect(
            &scenario,
            &earlier,
            &future_globals,
            500_000,
        )
        .unwrap();
        let future_messages: Vec<_> = collected
            .history
            .entries
            .iter()
            .filter(|row| matches!(row.item, BacklogItem::Message { .. }))
            .collect();
        assert_eq!(future_messages, expected.iter().collect::<Vec<_>>());
        assert_eq!(future_globals.encode(), before_future);
        assert!(collected.jump_source.save_position > earlier.save_position);
        let selected = &collected.history.entries[collected.current_row.unwrap()];
        assert!(selected.save_position < earlier.save_position);
        let current_row = collected.current_row.unwrap();
        let jump = crate::adv::backlog::jump::SceneJump::new(
            &collected.history,
            current_row,
            current_row,
        )
        .unwrap();
        let checkpoint_rows: Vec<_> = collected
            .history
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                (!matches!(row.item, BacklogItem::Message { .. })).then_some(index)
            })
            .collect();
        assert_eq!(
            jump.markers
                .iter()
                .filter_map(|marker| marker.authored_row)
                .collect::<Vec<_>>(),
            checkpoint_rows
        );
        assert_eq!(jump.markers.len(), checkpoint_rows.len() + 3);
        assert_eq!(jump.selected_target(), current_row);
        let future_row = collected
            .history
            .entries
            .iter()
            .position(|row| std::ptr::eq(row, future_messages[79]))
            .unwrap();
        let future_jump =
            crate::adv::backlog::jump::SceneJump::new(&collected.history, current_row, future_row)
                .unwrap();
        assert_eq!(future_jump.markers.len(), checkpoint_rows.len() + 4);
        assert_eq!(future_jump.selected_target(), future_row);
        println!(
            "Native SceneJump markers: {} authored checkpoint rows, cursor/current merge and distinct future cursor match supplied80-message history",
            checkpoint_rows.len()
        );
        let mut future_target = collected.jump_source.clone();
        future_target.save_position = future_messages[79].save_position;
        let scene =
            ReconstructedScene::from_save(&scenario, &future_target, &future_globals, 500_000)
                .unwrap();
        let super::super::reconstruction::ReconstructionStop::Command(RuntimeCommand::MSGSET(msg)) =
            scene.stop
        else {
            panic!("Future authored message boundary");
        };
        let BacklogItem::Message {
            message_id,
            ref text,
            ..
        } = future_messages[79].item
        else {
            unreachable!()
        };
        assert_eq!(msg.msg_id.0, message_id);
        assert_eq!(&msg.text, text);
        println!(
            "Native future history: supplied40th boundary recovers all80 seen messages and reconstructs80th boundary"
        );
        let before = globals.encode();
        for message in [messages[0], messages[39], messages[79]] {
            let index = history
                .entries
                .iter()
                .position(|row| std::ptr::eq(row, message))
                .unwrap();
            let mut jump =
                crate::adv::backlog::rollback::BacklogRollback::new(&history, index, &entry)
                    .unwrap();
            confirm_jump(&mut jump);
            let scene = jump
                .reconstruct_confirmed(&scenario, &globals, 500_000)
                .unwrap()
                .unwrap();
            let super::super::reconstruction::ReconstructionStop::Command(RuntimeCommand::MSGSET(
                restored,
            )) = scene.stop
            else {
                panic!("Real message boundary must restore MSGSET");
            };
            let BacklogItem::Message {
                message_id, text, ..
            } = &message.item
            else {
                unreachable!()
            };
            assert_eq!(restored.msg_id.0, *message_id);
            assert_eq!(&restored.text, text);
            assert_eq!(scene.state.save_position, message.save_position);
        }
        assert_eq!(globals.encode(), before);

        let read_font = |rom: &mut RomReader<StatelessFile>, path: &str| {
            let file = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            shin_core::format::font::read_font_metrics(&mut std::io::Cursor::new(bytes)).unwrap()
        };
        let regular = read_font(&mut rom, "/newrodin-medium.fnt");
        let bold = read_font(&mut rom, "/newrodin-bold.fnt");
        let mut glyph_count = 0;
        let mut row_sizes = Vec::new();
        for entry in &history.entries {
            let (text, color) = crate::adv::backlog::text::display_text(&entry.item).unwrap();
            let (commands, lines, size) =
                crate::adv::backlog::text::layout_message(&text, color, &regular, &bold);
            assert!(size.is_finite());
            row_sizes.push((
                size.y,
                !crate::adv::backlog::model::voice_paths(&entry.item).is_empty(),
            ));
            assert!(
                lines
                    .iter()
                    .all(|line| line.width.is_finite() && line.line_height.is_finite())
            );
            for command in commands {
                if let shin_core::layout::commands::Command::Char(ch) = command {
                    assert!(
                        ch.position.is_finite()
                            && ch.scale.is_finite()
                            && ch.horizontal_scale.is_finite()
                    );
                    glyph_count += 1;
                }
            }
        }
        let count = history.entries.len();
        let document = crate::adv::backlog::model::BacklogModel::new(row_sizes, count - 1);
        assert_eq!(document.rows.len(), count);
        assert_eq!(document.selected, Some(count - 1));
        assert_eq!(document.scroll(), document.max_scroll());
        assert!(
            document
                .rows
                .windows(2)
                .all(|pair| pair[1].top == pair[0].top + pair[0].height + 14.0)
        );
        println!(
            "Recorded supplied-script backlog:80 raw messages, voice tags,box styles and saved boundaries match live prefix; {glyph_count} browser glyphs across {count} total entries (including choices/checkpoints) laid out with supplied fonts; document scroll={}",
            document.scroll()
        );
    }
}
