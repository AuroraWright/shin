//! Native LogBuilder: recorded route first, then current-global/last-choice
//! fallback until an unread message, unavailable choice, future quiz or EXIT.
use super::{
    StartableCommand, VmState,
    backlog::{Backlog, BacklogEntry, item_for_command},
    command,
};
use anyhow::{Context, Result, bail};
use shin_core::{
    format::{
        save::{GameDataEntry, RouteRecorder, SaveData},
        scenario::Scenario,
    },
    vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand},
    },
};

pub(super) struct BrowserHistory {
    pub history: Backlog,
    /// Extended route suitable for reconstruction of any collected row.
    pub jump_source: GameDataEntry,
    pub current_row: Option<usize>,
}

impl BrowserHistory {
    pub fn collect(
        scenario: &Scenario,
        source: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        if source.save_position == 0 {
            return Ok(Self {
                history: Backlog::default(),
                jump_source: source.clone(),
                current_row: None,
            });
        }
        let mut scripter = Scripter::new(scenario, source.scenario_id, source.random_seed);
        let mut state = VmState::for_scenario(source.scenario_id, source.random_seed);
        state.settings = globals.settings.clone();
        state.persist = globals.persist_data.clone();
        state.save_vectors = globals.save_vectors.clone();
        state.auto_save_slot = globals.auto_save_slot.clone();
        state.persistent_commits_enabled = false;
        let mut historical = Some(source.route_data.reader());
        let mut future: Option<RouteRecorder> = None;
        let mut entries = Vec::new();
        let mut current_row = None;
        let mut result = CommandResult::None;
        let mut done = false;
        for _ in 0..command_limit {
            let command = scripter.run(result).context("Executing browser history")?;
            let mut quiz_target = None;
            let response = match &command {
                RuntimeCommand::SGET(get) => {
                    let value = if state.save_position < source.save_position {
                        historical
                            .as_mut()
                            .context("History moved behind its source")?
                            .read_persistent()
                            .context("Reading historical browser SGET")?
                    } else {
                        let value = state.persist.get(get.slot_number as u32 as usize);
                        future
                            .get_or_insert_with(|| historical.take().unwrap().into_recorder())
                            .record_persistent(value);
                        value
                    };
                    Some(value)
                }
                RuntimeCommand::SSET(set) => {
                    // ab260 suppresses historical writes. Future writes change
                    // only the builder's private, signed16-bit persistent copy.
                    if state.save_position >= source.save_position {
                        state
                            .persist
                            .set(set.slot_number as u32 as usize, set.value);
                    }
                    None
                }
                RuntimeCommand::SELECT(select) => {
                    select.apply_state(&mut state);
                    let value = if state.save_position < source.save_position {
                        historical
                            .as_mut()
                            .context("History moved behind its source")?
                            .read_choice()
                            .context("Reading historical browser SELECT")?
                    } else {
                        let Some(index) = select.choice_index.checked_sub(1) else {
                            done = true;
                            break;
                        };
                        let remembered = globals
                            .save_vectors
                            .select_last_choices
                            .get(index as usize)
                            .copied()
                            .unwrap_or(0);
                        if remembered == 0 {
                            done = true;
                            break;
                        }
                        let value = i32::from(remembered) - 1;
                        if (select.choice_visibility_mask as u32 & 1u32.wrapping_shl(value as u32))
                            == 0
                        {
                            done = true;
                            break;
                        }
                        future
                            .get_or_insert_with(|| historical.take().unwrap().into_recorder())
                            .record_choice(value);
                        value
                    };
                    Some(value)
                }
                RuntimeCommand::QUIZ(quiz) => {
                    quiz_target = quiz.apply_state(&mut state);
                    if state.save_position >= source.save_position {
                        done = true;
                        break;
                    }
                    let value = historical
                        .as_mut()
                        .context("History moved behind its source")?
                        .read_quiz()
                        .context("Reading historical browser QUIZ")?;
                    Some(value)
                }
                RuntimeCommand::EXIT(exit) if exit.arg1 == 0 => {
                    done = true;
                    break;
                }
                // LogBuilder abcc0 sets its done byte rather than following
                // the live VM's scene replacement back to RESUMESET.
                RuntimeCommand::RESUME(_) => {
                    done = true;
                    break;
                }
                other => {
                    command::apply_command_state_ref(other, &mut state)?;
                    if let RuntimeCommand::MSGSET(message) = other {
                        // ab328..ab354: equality already belongs to the future
                        // gate, and negative message IDs bypass read history.
                        if state.save_position >= source.save_position
                            && globals.settings.v11_disallowskipunread
                            && (message.msg_id.0 as i32) >= 0
                            && !globals
                                .save_vectors
                                .seen_messages
                                .contains(message.msg_id.0 as usize)
                        {
                            done = true;
                            break;
                        }
                    }
                    None
                }
            };
            if let Some(item) = item_for_command(&command, &state, response)? {
                if state.save_position < source.save_position {
                    current_row = Some(entries.len());
                }
                entries.push(BacklogEntry {
                    save_position: state.save_position,
                    item,
                });
            }
            result = match command {
                RuntimeCommand::SGET(get) => get.token.finish(response.unwrap()),
                RuntimeCommand::SSET(set) => set.token.finish(),
                RuntimeCommand::SELECT(select) => {
                    command::replay_choice_result(
                        &mut state,
                        select.choice_set_base,
                        select.choice_index,
                        response.unwrap(),
                    );
                    select.token.finish(response.unwrap())
                }
                RuntimeCommand::QUIZ(quiz) => {
                    command::replay_quiz_result(&mut state, quiz_target, response.unwrap());
                    quiz.token.finish(response.unwrap())
                }
                other => other
                    .execute_dummy()
                    .context("Browser command cannot continue")?,
            };
        }
        if !done {
            bail!("Browser history exceeded command limit");
        }
        let route = future.unwrap_or_else(|| historical.unwrap().into_recorder());
        let mut jump_source = source.clone();
        jump_source.route_data = route.snapshot();
        jump_source.save_position = entries
            .iter()
            .map(|entry| entry.save_position)
            .max()
            .unwrap_or(0)
            .max(source.save_position);
        if current_row.is_none() && !entries.is_empty() {
            current_row = Some(0);
        }
        Ok(Self {
            history: Backlog { entries },
            jump_source,
            current_row,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        backlog::{BacklogItem, tests::fixture},
        reconstruction::{ReconstructedScene, ReconstructionStop},
    };
    use super::*;
    use binrw::BinWrite;
    use shin_core::{
        format::scenario::{
            instruction_elements::{MessageId, NumberSpec, Register, U8Bool},
            instructions::Instruction,
            types::U16FixupString,
        },
        vm::command::{CompiletimeCommand as C, compiletime as c},
    };
    fn script(commands: impl IntoIterator<Item = C>) -> Scenario {
        let mut bytes = fixture(true).raw()[..0xbc].to_vec();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(0xbc);
        for command in commands {
            Instruction::Command(command).write_le(&mut cursor).unwrap();
        }
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        Scenario::new(bytes.into()).unwrap()
    }
    fn message(id: u32) -> C {
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(id),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("Message"),
        })
    }
    fn exit() -> C {
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        })
    }
    #[test]
    fn resume_stops_browser_without_appending_unreachable_messages() {
        let scenario = script([
            C::RESUMESET(c::RESUMESET {}),
            message(12),
            C::RESUME(c::RESUME {}),
            message(13),
            exit(),
        ]);
        let globals = globals();
        let before = globals.encode();
        let collected = BrowserHistory::collect(&scenario, &source(2), &globals, 100).unwrap();
        assert_eq!(collected.history.entries.len(), 1);
        assert_eq!(collected.history.entries[0].save_position, 2);
        assert_eq!(collected.jump_source.save_position, 2);
        assert_eq!(globals.encode(), before);
        let impossible = GameDataEntry {
            save_position: 3,
            ..collected.jump_source
        };
        assert!(
            ReconstructedScene::from_save(&scenario, &impossible, &globals, 100)
                .err()
                .unwrap()
                .to_string()
                .contains("beyond RESUME")
        );
    }
    #[test]
    fn future_persistent_reads_use_globals_and_private_signed_writes() {
        let scenario = script([
            C::SSET(c::SSET {
                slot_number: NumberSpec::constant(0),
                value: NumberSpec::constant(111),
            }),
            message(12),
            C::SGET(c::SGET {
                dest: Register::from_regular_register(1),
                slot_number: NumberSpec::constant(0),
            }),
            C::SSET(c::SSET {
                slot_number: NumberSpec::constant(0),
                value: NumberSpec::constant(65539),
            }),
            C::SGET(c::SGET {
                dest: Register::from_regular_register(2),
                slot_number: NumberSpec::constant(0),
            }),
            message(13),
            exit(),
        ]);
        let globals = globals();
        let before = globals.encode();
        let source = source(1);
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert_eq!(collected.history.entries.len(), 2);
        let mut route = collected.jump_source.route_data.reader();
        assert_eq!(
            route.read_persistent().unwrap(),
            1,
            "Historical write111 is suppressed"
        );
        assert_eq!(
            route.read_persistent().unwrap(),
            3,
            "Future write truncates to signed16bits privately"
        );
        let mut target = collected.jump_source.clone();
        target.save_position = collected.history.entries[1].save_position;
        let restored = ReconstructedScene::from_save(&scenario, &target, &globals, 100).unwrap();
        assert!(
            matches!(restored.stop, ReconstructionStop::Command(RuntimeCommand::MSGSET(ref msg)) if msg.msg_id.0 == 13)
        );
        assert_eq!(globals.encode(), before);
    }
    #[test]
    fn largest_serialized_24bit_message_id_still_obeys_unread_guard() {
        let scenario = script([message(u32::MAX), message(13), exit()]);
        let globals = SaveData::default();
        let collected = BrowserHistory::collect(&scenario, &source(1), &globals, 100).unwrap();
        assert!(collected.history.entries.is_empty());
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_backlog_future_history_fallback_and_stop_gates() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, expected) in [
            (0xab210, 0x540000a2u32),
            (0xab224, 0xb9400661),
            (0xab23c, 0x97fe25c1),
            (0xab278, 0x54000042),
            (0xab288, 0x14009832),
            (0xab2fc, 0x6b0a011f),
            (0xab338, 0x3942ed2b),
            (0xab33c, 0x7a402964),
            (0xab340, 0x540000c0),
            (0xab348, 0x37f80081),
            (0xab350, 0x940099c8),
            (0xab354, 0x36000b80),
            (0xab59c, 0x6b09011f),
            (0xab5a0, 0x54000482),
            (0xab648, 0x72001c1f),
            (0xab654, 0x12001c08),
            (0xab658, 0x51000516),
            (0xab660, 0x1ad6210a),
            (0xab664, 0x6a0a013f),
            (0xab668, 0x54000420),
            (0xab684, 0x97fe24cb),
            (0xac03c, 0x320003e8),
            (0xac040, 0x39022268),
            (0xa34c4, 0x35000068),
            (0xa34cc, 0x39022268),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                expected,
                "Native browser fallback {a:x}"
            );
        }
    }
    fn source(position: u32) -> GameDataEntry {
        let mut route = RouteRecorder::default();
        route.record_persistent(2);
        route.record_choice(0);
        route.record_quiz(27);
        GameDataEntry {
            scenario_id: 7,
            random_seed: 42,
            save_position: position,
            route_data: route.snapshot(),
        }
    }
    fn globals() -> SaveData {
        let mut globals = SaveData::default();
        globals.persist_data.set(0, 1);
        globals.save_vectors.seen_messages.add(12);
        globals.save_vectors.seen_messages.add(13);
        globals.save_vectors.select_last_choices = vec![2];
        globals
    }
    #[test]
    fn future_uses_remembered_choice_and_stops_at_quiz_without_mutating_globals() {
        let scenario = fixture(true);
        let globals = globals();
        let before = globals.encode();
        let source = source(1);
        let original = format!("{:?}", source.route_data);
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert_eq!(collected.history.entries.len(), 3);
        assert_eq!(collected.current_row, Some(0));
        assert_eq!(collected.jump_source.save_position, 3);
        assert!(matches!(
            collected.history.entries[1].item,
            BacklogItem::Choice { response: 1, .. }
        ));
        let mut route = collected.jump_source.route_data.reader();
        assert_eq!(
            route.read_persistent().unwrap(),
            2,
            "SGET before boundary uses recorded value"
        );
        assert_eq!(
            route.read_choice().unwrap(),
            1,
            "Future choice overwrites stale recorded answer0"
        );
        let mut target = collected.jump_source.clone();
        target.save_position = collected.history.entries[2].save_position;
        let restored = ReconstructedScene::from_save(&scenario, &target, &globals, 100).unwrap();
        assert!(
            matches!(restored.stop, ReconstructionStop::Command(RuntimeCommand::MSGSET(ref msg)) if msg.msg_id.0 == 13)
        );
        assert_eq!(globals.encode(), before);
        assert_eq!(format!("{:?}", source.route_data), original);
    }
    #[test]
    fn current_boundary_obeys_unread_guard() {
        let scenario = fixture(true);
        let source = source(1);
        let mut globals = globals();
        globals.save_vectors.seen_messages = shin_core::format::save::index_set::IndexSet::new();
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert!(
            collected.history.entries.is_empty(),
            "Equality belongs to unread gate"
        );
        globals.settings.v11_disallowskipunread = false;
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert_eq!(collected.history.entries.len(), 3);
        globals.settings.v11_disallowskipunread = true;
        globals.save_vectors.seen_messages.add(12);
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert_eq!(
            collected.history.entries.len(),
            2,
            "Unseen future message13 is not included"
        );
    }
    #[test]
    fn unavailable_or_hidden_remembered_choices_stop_before_result_row() {
        let scenario = fixture(true);
        let source = source(1);
        for remembered in [0, 1] {
            let mut globals = globals();
            globals.save_vectors.select_last_choices[0] = remembered;
            let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
            assert_eq!(collected.history.entries.len(), 1);
        }
    }
    #[test]
    fn recorded_choices_and_quizzes_ignore_current_last_choice_and_seen_guard() {
        let scenario = fixture(true);
        let source = source(5);
        let globals = SaveData::default();
        let before = globals.encode();
        let collected = BrowserHistory::collect(&scenario, &source, &globals, 100).unwrap();
        assert_eq!(collected.history.entries.len(), 5);
        assert!(matches!(
            collected.history.entries[1].item,
            BacklogItem::Choice { response: 0, .. }
        ));
        assert!(matches!(
            collected.history.entries[3].item,
            BacklogItem::Quiz { response: 27, .. }
        ));
        assert_eq!(
            collected.current_row,
            Some(3),
            "Native current row is last row strictly before source"
        );
        assert_eq!(globals.encode(), before);
    }
    #[test]
    fn zero_boundary_is_empty_and_command_limit_is_recoverable() {
        let scenario = fixture(true);
        let globals = globals();
        let collected = BrowserHistory::collect(&scenario, &source(0), &globals, 0).unwrap();
        assert!(collected.history.entries.is_empty());
        assert_eq!(collected.current_row, None);
        assert!(BrowserHistory::collect(&scenario, &source(1), &globals, 1).is_err());
    }
}
