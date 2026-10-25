use super::*;
use binrw::{BinRead, BinWrite};
use shin_core::{
    format::{
        save::RouteRecorder,
        scenario::{
            instruction_elements::{NumberSpec, Register},
            instructions::Instruction,
            types::{StringArray, U8SmallNumberList, U16String},
        },
    },
    vm::command::{CompiletimeCommand as C, compiletime as c},
};

#[test]
fn event_load_restart_preserves_normal_and_explicit_event_chapters() {
    use shin_core::format::scenario::{
        instruction_elements::{MessageId, U8Bool},
        types::U16FixupString,
    };
    for explicit_chapter in [false, true] {
        let mut bytes = scenario().raw()[..0xbc].to_vec();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(0xbc);
        let mut commands = vec![
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rBefore ending"),
            }),
            C::EVBEGIN(c::EVBEGIN {
                arg: NumberSpec::constant(10),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(2),
                auto_wait: U8Bool(false),
                text: U16FixupString::new("@rEnding event"),
            }),
        ];
        if explicit_chapter {
            commands.push(C::AUTOSAVE(c::AUTOSAVE {}));
        }
        commands.push(C::EVEND(c::EVEND {}));
        commands.push(C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }));
        for c in commands {
            Instruction::Command(c).write_le(&mut cursor).unwrap();
        }
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let scenario = Scenario::new(bytes.into()).unwrap();
        for (boundary, expected, event) in [
            (1, 1, None),
            (2, 1, None),
            (3, 3, explicit_chapter.then_some(10)),
        ] {
            let game = GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: boundary,
                route_data: RouteRecorder::default().snapshot(),
            });
            let raw =
                ReconstructedScene::from_game_data(&scenario, &game, &SaveData::default(), 100)
                    .unwrap();
            assert_eq!(
                raw.publishes_continue_checkpoint(),
                boundary == 1 || (explicit_chapter && boundary == 3)
            );
            let loaded = ReconstructedScene::from_game_data_for_load(
                &scenario,
                &game,
                &SaveData::default(),
                100,
            )
            .unwrap();
            assert_eq!(loaded.state.save_position, expected);
            assert_eq!(loaded.state.current_event, event);
            assert_eq!(game.entry.save_position, boundary);
        }
    }
}

#[test]
fn event_skip_replays_script_to_end_without_publishing_probe_side_effects() {
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    let r = Register::from_regular_register(1);
    for cmd in [
        C::EVBEGIN(c::EVBEGIN {
            arg: NumberSpec::constant(7),
        }),
        C::SGET(c::SGET {
            dest: r,
            slot_number: NumberSpec::constant(0),
        }),
        C::WAIT(c::WAIT {
            wait_amount: NumberSpec::constant(600),
            allow_interrupt: shin_core::format::scenario::instruction_elements::U8Bool(false),
        }),
        C::SAVEINFO(c::SAVEINFO {
            level: NumberSpec::constant(1),
            info: shin_core::format::scenario::types::U16FixupString::new("After the movie"),
        }),
        C::UNLOCK(c::UNLOCK {
            unlock_type: 2,
            unlock_indices: U8SmallNumberList::from_iter([NumberSpec::constant(4)]),
        }),
        C::EVEND(c::EVEND {}),
        C::SSET(c::SSET {
            slot_number: NumberSpec::constant(2),
            value: NumberSpec::register(r),
        }),
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }),
    ] {
        Instruction::Command(cmd).write_le(&mut cursor).unwrap();
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Scenario::new(bytes.into()).unwrap();
    let mut globals = SaveData::default();
    globals.persist_data.set(0, -37);
    let entry = GameDataEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 1,
        route_data: RouteRecorder::default().snapshot(),
    };
    let mut skipped = ReconstructedScene::skip_event(&scenario, &entry, &globals, 100).unwrap();
    assert_eq!(skipped.state.current_event, None);
    assert_eq!(skipped.state.save_position, 2);
    assert_eq!(skipped.state.save_info.info[1], "After the movie");
    assert_eq!(skipped.state.persist.get(0), -37);
    assert!(!skipped.state.save_vectors.unlocked_movies.contains(4));
    assert!(skipped.state.auto_save_slot.is_none());
    let ReconstructionStop::Command(RuntimeCommand::EVEND(end)) = skipped.stop else {
        panic!("Expected native event-end boundary")
    };
    let RuntimeCommand::SSET(next) = skipped.scripter.run(end.token.finish()).unwrap() else {
        panic!("Expected first post-event command")
    };
    assert_eq!(
        next.value, -37,
        "SGET result and VM register survive reconstruction"
    );
    assert_eq!(globals.persist_data.get(0), -37);
    let mut normal = entry.clone();
    normal.save_position = 2;
    normal.route_data = skipped.state.route.snapshot();
    assert!(ReconstructedScene::skip_event(&scenario, &normal, &globals, 100).is_err());
    assert!(ReconstructedScene::skip_event(&scenario, &entry, &globals, 1).is_err());
}

#[test]
fn saved_route_rebuilds_scene_title_at_autosave_not_later_saveinfo() {
    use shin_core::format::{
        save::{GameDataEntry, SaveData},
        scenario::{
            instruction_elements::{MessageId, U8Bool},
            types::U16FixupString,
        },
    };
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    for command in [
        C::SAVEINFO(c::SAVEINFO {
            level: NumberSpec::constant(0),
            info: U16FixupString::new("Episode A"),
        }),
        C::SAVEINFO(c::SAVEINFO {
            level: NumberSpec::constant(1),
            info: U16FixupString::new("Chapter A"),
        }),
        C::AUTOSAVE(c::AUTOSAVE {}),
        C::SAVEINFO(c::SAVEINFO {
            level: NumberSpec::constant(1),
            info: U16FixupString::new("Chapter B"),
        }),
        C::SAVEINFO(c::SAVEINFO {
            level: NumberSpec::constant(0),
            info: U16FixupString::new("Episode B"),
        }),
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(1),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("Restored message"),
        }),
        C::AUTOSAVE(c::AUTOSAVE {}),
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }),
    ] {
        Instruction::Command(command).write_le(&mut cursor).unwrap();
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Scenario::new(bytes.into()).unwrap();
    for (position, current, title) in [
        (1, "Chapter A", "Chapter A"),
        (2, "Chapter B", "Chapter A"),
        (3, "Chapter B", "Chapter B"),
    ] {
        let entry = GameDataEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: position,
            route_data: RouteRecorder::default().snapshot(),
        };
        let restored =
            ReconstructedScene::from_save(&scenario, &entry, &SaveData::default(), 100)
                .unwrap();
        let preview = crate::adv::save_menu::scene::SavedSlotScene::reconstruct(
            &scenario,
            &entry,
            &SaveData::default(),
            100,
        )
        .unwrap();
        assert_eq!(
            preview.labels,
            crate::adv::save_menu::text::SaveSlotLabels::Ready {
                title: if position < 3 {
                    "Episode A"
                } else {
                    "Episode B"
                }
                .into(),
                subtitle: title.into(),
            }
        );
        assert_eq!(preview.scene.state.save_info.info[1], current);
        assert_eq!(restored.state.save_info.info[1], current);
        assert_eq!(restored.state.scene_title, title);
        assert!(restored.state.persistent_commits_enabled);
        assert!(
            restored.state.auto_save_slot.is_none(),
            "Replay must not replace autosave slot"
        );
    }
}
fn scenario() -> Scenario {
    // Empty info-table layout shared with the core's minimal SNR example.
    let header: [u32; 47] = [
        0x20524e53, 0, 0, 6, 19, 0, 0, 0, 0xbc, 0x58, 0x60, 0x68, 0x70, 0x78, 0x80, 0x88, 0x90,
        0x94, 0x98, 0x9c, 0xa4, 0xa8, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 0, 0, 0, 4, 0,
        0, 4, 0, 0, 0, 0,
    ];
    let mut bytes: Vec<u8> = header.into_iter().flat_map(u32::to_le_bytes).collect();
    let register = Register::from_regular_register;
    let commands = [
        C::SSET(c::SSET {
            slot_number: NumberSpec::constant(10),
            value: NumberSpec::constant(32768),
        }),
        C::SGET(c::SGET {
            dest: register(1),
            slot_number: NumberSpec::constant(0),
        }),
        C::SELECT(c::SELECT {
            choice_set_base: 1,
            choice_index: 1,
            dest: register(2),
            choice_visibility_mask: NumberSpec::constant(3),
            choice_title: U16String::new(""),
            variants: StringArray::new(["A", "B"]),
        }),
        C::QUIZ(c::QUIZ {
            dest: register(3),
            arg: NumberSpec::constant(1),
        }),
        C::UNLOCK(c::UNLOCK {
            unlock_type: 0,
            unlock_indices: U8SmallNumberList::from_iter([NumberSpec::constant(4)]),
        }),
        C::AUTOSAVE(c::AUTOSAVE {}),
        C::SSET(c::SSET {
            slot_number: NumberSpec::constant(1),
            value: NumberSpec::register(register(1)),
        }),
        C::SSET(c::SSET {
            slot_number: NumberSpec::constant(2),
            value: NumberSpec::register(register(2)),
        }),
        C::SSET(c::SSET {
            slot_number: NumberSpec::constant(3),
            value: NumberSpec::register(register(3)),
        }),
    ];
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    for command in commands {
        Instruction::Command(command).write_le(&mut cursor).unwrap();
    }
    Instruction::rnd {
        dest: register(4),
        min: NumberSpec::constant(-100),
        max: NumberSpec::constant(100),
    }
    .write_le(&mut cursor)
    .unwrap();
    Instruction::Command(C::SSET(c::SSET {
        slot_number: NumberSpec::constant(4),
        value: NumberSpec::register(register(4)),
    }))
    .write_le(&mut cursor)
    .unwrap();
    Instruction::Command(C::EXIT(c::EXIT {
        arg1: 0,
        arg2: NumberSpec::constant(0),
    }))
    .write_le(&mut cursor)
    .unwrap();
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    Scenario::new(bytes.into()).unwrap()
}

fn entry(position: u32) -> GameDataEntry {
    let mut route = RouteRecorder::default();
    route.record_persistent(-14);
    route.record_choice(1);
    route.record_quiz(27);
    GameDataEntry {
        scenario_id: 7,
        random_seed: 42,
        save_position: position,
        route_data: route.snapshot(),
    }
}

fn resume_scenario() -> Scenario {
    use shin_core::format::scenario::{
        instruction_elements::{MessageId, U8Bool},
        types::U16FixupString,
    };
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    let register = Register::from_regular_register;
    for instruction in [
        Instruction::Command(C::SGET(c::SGET {
            dest: register(1),
            slot_number: NumberSpec::constant(0),
        })),
        Instruction::rnd {
            dest: register(4),
            min: NumberSpec::constant(-1000),
            max: NumberSpec::constant(1000),
        },
        Instruction::Command(C::RESUMESET(c::RESUMESET {})),
        Instruction::Command(C::SGET(c::SGET {
            dest: register(2),
            slot_number: NumberSpec::constant(1),
        })),
        Instruction::Command(C::SSET(c::SSET {
            slot_number: NumberSpec::constant(5),
            value: NumberSpec::register(register(2)),
        })),
        Instruction::Command(C::SSET(c::SSET {
            slot_number: NumberSpec::constant(6),
            value: NumberSpec::register(register(4)),
        })),
        Instruction::Command(C::MSGSET(c::MSGSET {
            msg_id: MessageId(1),
            auto_wait: U8Bool(false),
            text: U16FixupString::new("@rResume route message"),
        })),
        Instruction::Command(C::WAIT(c::WAIT {
            wait_amount: NumberSpec::new(NumberSpec::constant(10).into_untyped()),
            allow_interrupt: U8Bool(false),
        })),
        Instruction::Command(C::RESUME(c::RESUME {})),
        Instruction::Command(C::SSET(c::SSET {
            slot_number: NumberSpec::constant(99),
            value: NumberSpec::constant(999),
        })),
        Instruction::Command(C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        })),
    ] {
        instruction.write_le(&mut cursor).unwrap();
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    Scenario::new(bytes.into()).unwrap()
}

fn resume_source() -> GameDataEntry {
    let mut route = RouteRecorder::default();
    route.record_persistent(-14);
    route.record_persistent(-3);
    GameDataEntry {
        scenario_id: 7,
        random_seed: 42,
        save_position: 2,
        route_data: route.snapshot(),
    }
}

#[test]
fn resume_replays_prefix_preserves_globals_and_overwrites_abandoned_route() {
    let scenario = resume_scenario();
    let mut globals = SaveData::default();
    globals.persist_data.set(0, 999);
    globals.persist_data.set(1, 456);
    globals.save_vectors.seen_messages.add(1);
    let before = globals.encode();
    let source = resume_source();
    let original_route = format!("{:?}", source.route_data);
    let mut game = GameData::from_entry(source.clone());
    game.primary_entry = Some(entry(5));
    let retained = format!("{:?}", game.primary_entry);
    let original = ReconstructedScene::from_game_data(&scenario, &game, &globals, 100).unwrap();
    let random = original.state.get_persistent(6);
    assert_eq!(original.state.resume_position, Some(1));
    assert_eq!(original.state.route.bit_position(), 34);
    let target = original.state.resume_game_data();
    assert_eq!(target.entry.save_position, 1);
    assert_eq!(format!("{:?}", target.entry.route_data), original_route);
    assert_eq!(format!("{:?}", target.primary_entry), retained);
    let mut resumed =
        ReconstructedScene::from_game_data(&scenario, &target, &globals, 100).unwrap();
    assert_eq!(format!("{:?}", resumed.state.primary_entry), retained);
    assert_eq!(resumed.state.save_position, 1);
    assert_eq!(resumed.state.route.bit_position(), 17);
    assert_eq!(resumed.state.persist.get(0), 999);
    assert!(resumed.state.save_vectors.seen_messages.contains(1));
    let ReconstructionStop::Command(command @ RuntimeCommand::RESUMESET(_)) = resumed.stop
    else {
        panic!("resume must stop at its bookmark");
    };
    let mut result = command.execute_dummy().unwrap();
    loop {
        let command = resumed.scripter.run(result).unwrap();
        if let RuntimeCommand::SGET(get) = command {
            let value = get.apply_state(&mut resumed.state);
            assert_eq!(value, 456, "reads after bookmark use current globals");
            result = get.token.finish(value);
            continue;
        }
        command::apply_command_state_ref(&command, &mut resumed.state).unwrap();
        let is_message = matches!(command, RuntimeCommand::MSGSET(_));
        result = command.execute_dummy().unwrap();
        if is_message {
            break;
        }
    }
    assert_eq!(resumed.state.save_position, 2);
    assert_eq!(resumed.state.persist.get(5), 456);
    assert_eq!(
        resumed.state.persist.get(6),
        random,
        "replayed seed/PRNG restores register values"
    );
    let snapshot = resumed.state.route.snapshot();
    let mut route = snapshot.reader();
    assert_eq!(route.read_persistent().unwrap(), -14);
    assert_eq!(route.read_persistent().unwrap(), 456);
    assert_eq!(format!("{:?}", source.route_data), original_route);
    assert_eq!(globals.encode(), before);
    assert_eq!(VmState::for_scenario(7, 42).resume_entry().save_position, 0);
    let mut unset = source.clone();
    unset.save_position = 0;
    let initial = ReconstructedScene::from_save(&scenario, &unset, &globals, 100).unwrap();
    assert_eq!(initial.state.route.bit_position(), 0);
    assert_eq!(initial.state.resume_position, None);
    assert!(matches!(
        initial.stop,
        ReconstructionStop::Response(CommandResult::None)
    ));
}

#[test]
fn reconstruction_uses_historical_values_and_preserves_random_execution() {
    let scenario = scenario();
    let mut globals = SaveData::default();
    globals.persist_data.set(0, 999);
    globals.persist_data.set(10, 77);
    globals.save_vectors.select_seen_choices.add(5);
    let mut reconstructed =
        ReconstructedScene::from_save(&scenario, &entry(5), &globals, 100).unwrap();
    assert_eq!(reconstructed.state.save_position, 5);
    assert_eq!(reconstructed.state.persist.get(0), 999);
    assert_eq!(reconstructed.state.persist.get(10), 77);
    assert_eq!(reconstructed.state.get_persistent(10), 32768);
    assert!(!reconstructed.state.save_vectors.unlocked_cgs.contains(4));
    assert!(
        !reconstructed
            .state
            .save_vectors
            .select_seen_choices
            .contains(1)
    );
    assert!(
        reconstructed
            .state
            .save_vectors
            .select_seen_choices
            .contains(5)
    );
    assert_eq!(reconstructed.state.route.bit_position(), 27);
    assert!(reconstructed.state.persistent_commits_enabled);
    let ReconstructionStop::Command(RuntimeCommand::AUTOSAVE(command)) = reconstructed.stop
    else {
        panic!("Expected autosave entry boundary");
    };
    let mut reconstructed_result = command.token.finish();
    let mut original = Scripter::new(&scenario, 7, 42);
    let mut original_result = CommandResult::None;
    loop {
        original_result = match original.run(original_result).unwrap() {
            RuntimeCommand::SGET(command) => command.token.finish(-14),
            RuntimeCommand::SELECT(command) => command.token.finish(1),
            RuntimeCommand::QUIZ(command) => command.token.finish(27),
            RuntimeCommand::SSET(command) => command.token.finish(),
            RuntimeCommand::UNLOCK(command) => command.token.finish(),
            RuntimeCommand::AUTOSAVE(command) => {
                original_result = command.token.finish();
                break;
            }
            _ => unreachable!(),
        };
    }
    for expected in [Some(-14), Some(1), Some(27), None] {
        let a = original.run(original_result).unwrap();
        let b = reconstructed.scripter.run(reconstructed_result).unwrap();
        assert_eq!(original.position(), reconstructed.scripter.position());
        let (RuntimeCommand::SSET(a), RuntimeCommand::SSET(b)) = (a, b) else {
            unreachable!()
        };
        assert_eq!(a.value, b.value);
        if let Some(expected) = expected {
            assert_eq!(b.value, expected);
        }
        original_result = a.token.finish();
        reconstructed_result = b.token.finish();
    }
}

#[test]
fn reconstruction_stops_at_choice_entry_or_response_and_rejects_missing_routes() {
    let scenario = scenario();
    let globals = SaveData::default();
    let scene = ReconstructedScene::from_save(&scenario, &entry(1), &globals, 100).unwrap();
    assert!(matches!(
        scene.stop,
        ReconstructionStop::Command(RuntimeCommand::SELECT(_))
    ));
    assert_eq!(scene.state.route.bit_position(), 17);
    let scene = ReconstructedScene::from_save(&scenario, &entry(2), &globals, 100).unwrap();
    assert!(matches!(
        scene.stop,
        ReconstructionStop::Response(CommandResult::WriteMemory(_, 1))
    ));
    assert_eq!(scene.state.route.bit_position(), 21);
    assert!(ReconstructedScene::from_save(&scenario, &entry(5), &globals, 1).is_err());
    assert!(ReconstructedScene::from_save(&scenario, &entry(6), &globals, 100).is_err());
    let empty = GameDataEntry {
        route_data: Default::default(),
        ..entry(5)
    };
    assert!(ReconstructedScene::from_save(&scenario, &empty, &globals, 100).is_err());
}

#[test]
#[ignore = "requires supplied ROM; authored VM/state entry sweeps, without live rendering/audio"]
fn reference_authored_start_entries_reach_exit_or_interactive_boundary() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::{fs::File, io::Read};
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let mut total_messages = 0;
    let mut total_commands = 0;
    let mut exits = 0;
    let mut boundaries = 0;
    // Native Start's six groups expose IDs0..47 (records1736a8).
    for entry_id in 0..48 {
        let mut script = Scripter::new(&scenario, entry_id, 12345);
        let mut state = VmState::for_scenario(entry_id, 12345);
        let mut result = CommandResult::None;
        let mut messages = 0;
        let mut loads = 0;
        let mut choices = 0;
        let mut stopped = false;
        for count in 1..=500_000 {
            let current = script.run(result).unwrap_or_else(|error| {
                panic!(
                    "Authored entry {entry_id}, command {count}, pc {:?}: {error:#}",
                    script.position()
                )
            });
            match &current {
                RuntimeCommand::MSGSET(_) => messages += 1,
                RuntimeCommand::LAYERLOAD(_) => loads += 1,
                _ => {}
            }
            total_commands += 1;
            result = match current {
                RuntimeCommand::QUIZ(quiz) => {
                    let target = quiz.apply_state(&mut state);
                    assert!(
                        target.is_some(),
                        "Entry {entry_id}: QUIZ must address an allocated layer"
                    );
                    eprintln!(
                        "Authored entry {entry_id}: QUIZ boundary at {:?}, {count} commands, {messages} messages, {loads} loads, {choices} choices, target {target:?}",
                        script.position()
                    );
                    let (_, bank) = target.unwrap();
                    let layer = &state.layers.layerbanks[bank];
                    eprintln!(
                        "Authored QUIZ layer: {:?}, params {:?}",
                        layer.layer_type, layer.params
                    );
                    boundaries += 1;
                    stopped = true;
                    break;
                }
                RuntimeCommand::RESUME(_) => {
                    eprintln!(
                        "Authored entry {entry_id}: RESUME boundary at {:?}, {count} commands, {messages} messages, {loads} loads, {choices} choices",
                        script.position()
                    );
                    boundaries += 1;
                    stopped = true;
                    break;
                }
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut state);
                    get.token.finish(value)
                }
                RuntimeCommand::SELECT(select) => {
                    select.apply_state(&mut state);
                    let mut menu = crate::adv::choice::ChoiceModel::new(
                        select.variants.len(),
                        select.choice_visibility_mask,
                    );
                    menu.update(shin_core::time::Ticks::from_f32(100.0));
                    menu.update(shin_core::time::Ticks::ZERO);
                    if menu.selected.is_none() {
                        menu.move_selection(1);
                    }
                    menu.confirm();
                    menu.update(shin_core::time::Ticks::from_f32(7.0));
                    menu.update(shin_core::time::Ticks::ZERO);
                    menu.update(shin_core::time::Ticks::from_f32(100.0));
                    menu.update(shin_core::time::Ticks::ZERO);
                    let value = menu.result.expect("Authored visible choice completes");
                    command::replay_choice_result(
                        &mut state,
                        select.choice_set_base,
                        select.choice_index,
                        value,
                    );
                    choices += 1;
                    select.token.finish(value)
                }
                current => {
                    command::apply_command_state_ref(&current, &mut state).unwrap_or_else(|error|
                        panic!("Authored entry {entry_id}, command {count}, pc {:?}: {error:#}", script.position()));
                    match current.execute_dummy() {
                        Some(result) => result,
                        None => {
                            eprintln!(
                                "Authored entry {entry_id}: EXIT at {:?}, {count} commands, {messages} messages, {loads} loads, {choices} choices",
                                script.position()
                            );
                            exits += 1;
                            stopped = true;
                            break;
                        }
                    }
                }
            };
        }
        assert!(
            stopped,
            "Authored entry {entry_id} exhausted the command budget at {:?}",
            script.position()
        );
        total_messages += messages;
    }
    assert_eq!(exits + boundaries, 48);
    assert_eq!((exits, boundaries), (47, 1));
    assert!(
        total_commands > 1_000_000,
        "Sweep must cover the authored long-form stories"
    );
    assert!(
        total_messages > 60_000,
        "Early exits cannot stand in for script coverage"
    );
    eprintln!(
        "Authored Start sweep: {exits} exits, {boundaries} interactive/restore boundaries; {total_commands} commands and {total_messages} messages. First-visible choices only; no live render/audio or whole-route coverage claim."
    );
}

#[test]
#[ignore = "requires supplied ROM; actual Cake responses and authored save replay, CPU only"]
fn reference_authored_cake_responses_resume_identical_following_commands() {
    authored_episode7_response_replays((0..15).map(|cake| (cake, None)));
}

#[test]
#[ignore = "requires supplied ROM; actual Cake/Quiz response combinations and save replay, CPU only"]
fn reference_authored_cake_and_quiz_responses_resume_identical_following_commands() {
    authored_episode7_response_replays(
        (0..15).flat_map(|cake| (0..2).map(move |quiz| (cake, Some(quiz)))),
    );
}

#[test]
#[ignore = "requires supplied ROM; authored Episode7 with controller models and response save replay, CPU only"]
fn reference_authored_episode7_controller_models_reach_exit() {
    authored_episode7_controller_path(0);
}

#[test]
#[ignore = "requires supplied ROM; alternate Episode7 final choice with controller models and save replay, CPU only"]
fn reference_authored_episode7_alternate_final_choice_reaches_exit() {
    authored_episode7_controller_path(1);
}

// Version classification for supplied-script reference assertions only.
// Both complete script fingerprints are checked before using fixed addresses.
fn reference_updated_scenario(scenario: &Scenario) -> bool {
    let fingerprint = scenario
        .raw()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    match fingerprint {
        0x2308b0a38fba578a => false,
        0x7a006c4e2505e117 => true,
        other => panic!("Unknown supplied scenario fingerprint {other:016x}"),
    }
}

fn authored_episode7_controller_path(last_choice: i32) {
    use crate::layer::user::{
        cake_layer::CakeModel,
        last_layer::LastModel,
        murderer::model::{MurdererMenu, MurdererModel, SUBMIT_ENTRY},
        quiz_layer::QuizModel,
    };
    use shin_core::{
        format::rom::RomReader,
        primitives::stateless_reader::StatelessFile,
        vm::command::types::{LayerType, LayerbankId},
    };
    use std::{collections::BTreeMap, fs::File, io::Read};
    enum Controller {
        Quiz(QuizModel),
        Cake(CakeModel),
        Murderer(MurdererModel),
        Last(LastModel),
    }
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let updated = reference_updated_scenario(&scenario);
    let mut script = Scripter::new(&scenario, 7, 12345);
    let mut state = VmState::for_scenario(7, 12345);
    let mut result = CommandResult::None;
    let mut replay: Option<(ReconstructedScene, CommandResult)> = None;
    let mut controllers: enum_map::EnumMap<
        LayerbankId,
        Option<(
            shin_core::format::scenario::instruction_elements::UntypedNumberArray,
            Box<Controller>,
        )>,
    > = Default::default();
    let mut visits = BTreeMap::<u32, usize>::new();
    let mut responses = 0;
    let mut compared = 0;
    let mut finished = false;
    for count in 1..=1_000_000 {
        let current = script.run(result).unwrap();
        let mirrored = replay.as_mut().map(|(scene, response)| {
            let next = scene.scripter.run(*response).unwrap();
            assert_eq!(
                format!("{next:?}"),
                format!("{current:?}"),
                "Episode7 command {count} after response {responses}"
            );
            compared += 1;
            next
        });
        if let RuntimeCommand::QUIZ(quiz) = current {
            let target = quiz.apply_state(&mut state).unwrap();
            assert_eq!(
                state
                    .layers
                    .layerbanks
                    .values()
                    .filter(|layer| layer.layer_type == Some(LayerType::Quiz))
                    .count(),
                1,
                "This authored driver must not invent multi-bank clone publication semantics"
            );
            let params = state.layers.layerbanks[target.1].params;
            let visit = visits.entry(script.position().0).or_default();
            assert!(
                *visit < 50,
                "Repeated authored decision needs an explicit policy at {:?}",
                script.position()
            );
            let controller = &mut controllers[target.1]
                .get_or_insert_with(|| {
                    (
                        params,
                        Box::new(match params.0 {
                            0 => Controller::Quiz(QuizModel::new(params.1)),
                            1 => Controller::Cake(CakeModel::default()),
                            2 => Controller::Murderer(MurdererModel::new(params.1)),
                            3 => Controller::Last(LastModel::default()),
                            mode => panic!("Invalid authored controller {mode}"),
                        }),
                    )
                })
                .1;
            let value = match controller.as_mut() {
                Controller::Quiz(model) => {
                    if model.result.is_none() {
                        model.move_selection(*visit as i32);
                        model.confirm();
                        model.update(shin_core::time::Ticks::from_f32(7.0));
                        model.update(shin_core::time::Ticks::ZERO);
                    }
                    model.result.expect("Authored Quiz has a visible answer")
                }
                Controller::Cake(model) => {
                    if model.result.is_none() {
                        model.pointer(glam::Vec2::new(850.0, 700.0), true);
                        model.update(shin_core::time::Ticks::from_f32(7.0));
                        model.update(shin_core::time::Ticks::ZERO);
                    }
                    model.result.unwrap()
                }
                Controller::Murderer(model) => {
                    if model.result.is_none() {
                        model.navigate(5);
                        model.confirm();
                        for _ in 0..240 {
                            model.update(1.0);
                        }
                        assert_eq!(model.menu, MurdererMenu::Accusation);
                        for entry in 34..37 {
                            assert!(model.activate_accusation_entry(entry));
                        }
                        assert!(model.activate_accusation_entry(SUBMIT_ENTRY));
                    }
                    for _ in 0..300 {
                        if model.poll_result().is_some() {
                            break;
                        }
                        model.update(1.0);
                    }
                    model
                        .poll_result()
                        .expect("Native accusation finishes its response phases")
                }
                Controller::Last(model) => {
                    if model.result.is_none() {
                        model.navigate(if last_choice == 0 { -1 } else { 1 });
                    }
                    for _ in 0..150 {
                        if model.result.is_some() {
                            break;
                        }
                        model.update(1.0);
                        model.confirm();
                    }
                    model
                        .result
                        .expect("Native Last entry and confirmation phases finish")
                }
            };
            *visit += 1;
            responses += 1;
            eprintln!(
                "Authored Episode7 response {responses}: pc {:?}, command {count}, params {params:?}, value {value}, visit {visit}",
                script.position()
            );
            command::replay_quiz_result(&mut state, Some(target), value);
            result = quiz.token.finish(value);
            let globals = SaveData {
                persist_data: state.persist.clone(),
                save_vectors: state.save_vectors.clone(),
                auto_save_slot: state.auto_save_slot.clone(),
                ..SaveData::default()
            };
            let restored = ReconstructedScene::from_save(
                &scenario,
                &state.snapshot_entry(),
                &globals,
                1_000_000,
            )
            .unwrap();
            let ReconstructionStop::Response(response) = restored.stop else {
                panic!("Answered controller restores its response");
            };
            assert_eq!(format!("{response:?}"), format!("{result:?}"));
            assert_eq!(restored.state.save_position, state.save_position);
            assert_eq!(
                restored.state.route.bit_position(),
                state.route.bit_position()
            );
            assert_eq!(
                format!("{:?}", restored.state.layers.layerbanks),
                format!("{:?}", state.layers.layerbanks)
            );
            replay = Some((restored, response));
        } else {
            assert!(
                !matches!(
                    current,
                    RuntimeCommand::SELECT(_) | RuntimeCommand::RESUME(_)
                ),
                "Authored decision requires an explicit controller/scene policy at {:?}",
                script.position()
            );
            let terminal = matches!(&current, RuntimeCommand::EXIT(exit) if exit.arg1 == 0);
            let complete =
                |command: RuntimeCommand, state: &mut VmState| -> Option<CommandResult> {
                    match command {
                        RuntimeCommand::SGET(get) => {
                            let value = get.apply_state(state);
                            Some(get.token.finish(value))
                        }
                        command => {
                            command::apply_command_state_ref(&command, state).unwrap();
                            command.execute_dummy()
                        }
                    }
                };
            let next = complete(current, &mut state);
            if let Some(mirrored) = mirrored {
                let (scene, response) = replay.as_mut().unwrap();
                let mirrored_result = complete(mirrored, &mut scene.state);
                assert_eq!(format!("{mirrored_result:?}"), format!("{next:?}"));
                if let Some(next) = mirrored_result {
                    *response = next;
                }
            }
            if terminal {
                assert_eq!(responses, 19);
                assert!(count > 160_000);
                assert!(compared > 130_000);
                let expected_exit = match (updated, last_choice) {
                    (false, 0) => 0x867444,
                    (false, 1) => 0x86b4dc,
                    (true, 0) => 0x8674a0,
                    (true, 1) => 0x86b538,
                    _ => unreachable!(),
                };
                assert_eq!(script.position().0, expected_exit);
                eprintln!(
                    "Authored Episode7 ending {last_choice} EXIT: pc {:?}, {count} commands, {responses} controller responses, {compared} compared following commands",
                    script.position()
                );
                finished = true;
                break;
            }
            result = next.expect("Only EXIT terminates a command");
            // Drop model ownership when publication changes its descriptor or unloads it.
            for (bank, slot) in &mut controllers {
                let layer = &state.layers.layerbanks[bank];
                if slot.as_ref().is_some_and(|(params, _)| {
                    layer.layer_type != Some(LayerType::Quiz) || layer.params != *params
                }) {
                    *slot = None;
                }
            }
        }
    }
    assert!(finished, "Authored Episode7 must reach a real EXIT");
}

fn authored_episode7_response_replays(cases: impl IntoIterator<Item = (i32, Option<i32>)>) {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::{fs::File, io::Read};
    fn complete(current: RuntimeCommand, state: &mut VmState) -> CommandResult {
        match current {
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(state);
                get.token.finish(value)
            }
            current => {
                command::apply_command_state_ref(&current, state).unwrap();
                current
                    .execute_dummy()
                    .expect("Authored continuation must not exit unexpectedly")
            }
        }
    }
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let updated = reference_updated_scenario(&scenario);
    for (cake_choice, quiz_answer) in cases {
        let mut script = Scripter::new(&scenario, 7, 12345);
        let mut state = VmState::for_scenario(7, 12345);
        let mut result = CommandResult::None;
        let mut replay: Option<(ReconstructedScene, CommandResult)> = None;
        let mut answered = 0;
        let answers_to_complete = if quiz_answer.is_some() { 2 } else { 1 };
        let mut following = 0;
        let mut finished = false;
        for count in 1..=500_000 {
            let current = script.run(result).unwrap();
            let mirrored = replay.as_mut().map(|(scene, response)| {
                let next = scene.scripter.run(*response).unwrap();
                assert_eq!(format!("{next:?}"), format!("{current:?}"),
                    "Cake {cake_choice}: following command {following} differs after response restoration");
                next
            });
            if answered > 0 {
                following += 1;
            }
            if let RuntimeCommand::QUIZ(quiz) = current {
                let target = quiz.apply_state(&mut state).unwrap();
                let layer = &state.layers.layerbanks[target.1];
                if answered == answers_to_complete {
                    assert_eq!(
                        layer.layer_type,
                        Some(shin_core::vm::command::types::LayerType::Quiz)
                    );
                    assert!(following > 1, "Replay must follow the answered Quiz");
                    if quiz_answer.is_none() {
                        assert!(following > 2_000);
                        assert_eq!(
                            script.position().0,
                            if updated { 0x78d718 } else { 0x78d6da }
                        );
                        assert_eq!((layer.params.0, layer.params.1), (0, 3));
                    } else {
                        assert!(following > 1_400);
                        assert_eq!(
                            script.position().0,
                            if updated { 0x790621 } else { 0x7905e3 }
                        );
                        assert_eq!((layer.params.0, layer.params.1), (0, 7));
                    }
                    eprintln!(
                        "Authored Cake {cake_choice}, Quiz {quiz_answer:?}: next QUIZ at {:?}, {count} commands, {following} compared following commands, layer {:?}, params {:?}",
                        script.position(),
                        layer.layer_type,
                        layer.params
                    );
                    finished = true;
                    break;
                }
                assert_eq!(
                    layer.layer_type,
                    Some(shin_core::vm::command::types::LayerType::Quiz)
                );
                let value = if answered == 0 {
                    assert_eq!(layer.params.0, 1, "The first authored interaction is Cake");
                    let mut cake = crate::layer::user::cake_layer::CakeModel::default();
                    let point = (0..240)
                        .flat_map(|y| {
                            (0..240).map(move |x| {
                                glam::vec2(482.0 + x as f32 * 4.0, 2.0 + y as f32 * 4.0)
                            })
                        })
                        .find(|&point| {
                            crate::layer::user::cake_layer::CakeModel::hit_test(point)
                                == Some(cake_choice as usize)
                        })
                        .unwrap();
                    cake.pointer(point, true);
                    cake.update(shin_core::time::Ticks::from_f32(7.0));
                    cake.update(shin_core::time::Ticks::ZERO);
                    let value = cake.result.unwrap();
                    assert_eq!(value, cake_choice);
                    value
                } else {
                    assert!(following > 2_000);
                    assert_eq!(
                        script.position().0,
                        if updated { 0x78d718 } else { 0x78d6da }
                    );
                    assert_eq!((layer.params.0, layer.params.1), (0, 3));
                    let mut controller =
                        crate::layer::user::quiz_layer::QuizModel::new(layer.params.1);
                    assert_eq!(controller.answers, [0, 1]);
                    controller.move_selection(quiz_answer.unwrap());
                    controller.confirm();
                    controller.update(shin_core::time::Ticks::from_f32(7.0));
                    controller.update(shin_core::time::Ticks::ZERO);
                    let value = controller.result.unwrap();
                    assert_eq!(value, quiz_answer.unwrap());
                    value
                };
                command::replay_quiz_result(&mut state, Some(target), value);
                result = quiz.token.finish(value);
                let globals = SaveData {
                    persist_data: state.persist.clone(),
                    save_vectors: state.save_vectors.clone(),
                    auto_save_slot: state.auto_save_slot.clone(),
                    ..SaveData::default()
                };
                let restored = ReconstructedScene::from_save(
                    &scenario,
                    &state.snapshot_entry(),
                    &globals,
                    500_000,
                )
                .unwrap();
                let ReconstructionStop::Response(response) = restored.stop else {
                    panic!("Controller response must restore after the interactive completion");
                };
                assert_eq!(format!("{response:?}"), format!("{result:?}"));
                assert_eq!(restored.state.save_position, state.save_position);
                assert_eq!(
                    restored.state.route.bit_position(),
                    state.route.bit_position()
                );
                assert_eq!(
                    format!("{:?}", restored.state.layers.layerbanks),
                    format!("{:?}", state.layers.layerbanks)
                );
                replay = Some((restored, response));
                answered += 1;
                following = 0;
            } else {
                assert!(
                    !matches!(
                        current,
                        RuntimeCommand::SELECT(_) | RuntimeCommand::RESUME(_)
                    ),
                    "New authored decision needs its own controller policy"
                );
                result = complete(current, &mut state);
                if let Some(mirrored) = mirrored {
                    let (scene, response) = replay.as_mut().unwrap();
                    *response = complete(mirrored, &mut scene.state);
                    assert_eq!(format!("{response:?}"), format!("{result:?}"));
                }
            }
        }
        assert!(
            finished,
            "Cake {cake_choice} must reach the next authored interaction"
        );
    }
}

#[test]
#[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); verifies VM/state reconstruction, not rendering"]
fn reference_save_reconstruction_matches_following_script_commands() {
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
    let mut original = Scripter::new(&scenario, 1, 12345);
    let mut state = VmState::for_scenario(1, 12345);
    let mut result = CommandResult::None;
    let mut saved = false;
    for _ in 0..500_000 {
        let command = original.run(result).unwrap();
        result = match command {
            RuntimeCommand::SGET(command) => {
                let value = command.apply_state(&mut state);
                command.token.finish(value)
            }
            RuntimeCommand::SELECT(command) => {
                command.apply_state(&mut state);
                let mut model = crate::adv::choice::ChoiceModel::new(
                    command.variants.len(),
                    command.choice_visibility_mask,
                );
                model.update(shin_core::time::Ticks::from_f32(100.0));
                model.update(shin_core::time::Ticks::ZERO);
                model.move_selection(1);
                model.confirm();
                model.update(shin_core::time::Ticks::from_f32(7.0));
                model.update(shin_core::time::Ticks::ZERO);
                model.update(shin_core::time::Ticks::from_f32(100.0));
                model.update(shin_core::time::Ticks::ZERO);
                let value = model.result.unwrap();
                command::replay_choice_result(
                    &mut state,
                    command.choice_set_base,
                    command.choice_index,
                    value,
                );
                command.token.finish(value)
            }
            RuntimeCommand::QUIZ(_) => {
                panic!("Reference path reached QUIZ before its first autosave")
            }
            RuntimeCommand::AUTOSAVE(command) => {
                command.apply_state(&mut state);
                // The boot autosave is position one with no route. Exercise
                // a later boundary that also records historical reads.
                saved = state.save_position > 1 && state.route.bit_position() > 0;
                command.token.finish()
            }
            command => {
                command::apply_command_state_ref(&command, &mut state).unwrap();
                command
                    .execute_dummy()
                    .expect("Reference path exited before autosave")
            }
        };
        if saved {
            break;
        }
    }
    assert!(saved);
    assert!(state.save_position > 1);
    assert!(
        state.route.bit_position() > 0,
        "Reference path must record historical SGET"
    );
    let mut globals = SaveData {
        persist_data: state.persist.clone(),
        save_vectors: state.save_vectors.clone(),
        auto_save_slot: state.auto_save_slot.clone(),
        ..SaveData::default()
    };
    globals.persist_data.set(0, -1234);
    let globals = SaveData::decode(&globals.encode()).unwrap();
    let entry = &globals.auto_save_slot.as_ref().unwrap().entry;
    let mut restored =
        ReconstructedScene::from_save(&scenario, entry, &globals, 500_000).unwrap();
    assert_eq!(restored.state.save_position, state.save_position);
    assert_eq!(
        restored.state.route.bit_position(),
        state.route.bit_position()
    );
    assert_eq!(restored.state.persist.get(0), -1234);
    assert_eq!(restored.state.save_info.info, state.save_info.info);
    let preview = crate::adv::save_menu::scene::SavedSlotScene::reconstruct(
        &scenario, entry, &globals, 500_000,
    )
    .unwrap();
    assert_eq!(
        preview.labels,
        crate::adv::save_menu::text::SaveSlotLabels::Ready {
            title: state.save_info.info[0].clone(),
            subtitle: state.save_info.info[1].clone(),
        }
    );
    assert_eq!(
        preview.scene.state.route.bit_position(),
        state.route.bit_position()
    );
    assert_eq!(
        restored.state.messagebox_state.text,
        state.messagebox_state.text
    );
    assert_eq!(
        format!("{:?}", restored.state.audio),
        format!("{:?}", state.audio)
    );
    assert_eq!(
        format!("{:?}", restored.state.layers.layerbanks),
        format!("{:?}", state.layers.layerbanks)
    );
    assert_eq!(
        restored.state.layers.current_plane,
        state.layers.current_plane
    );
    let ReconstructionStop::Command(RuntimeCommand::AUTOSAVE(command)) = restored.stop else {
        panic!("Reference autosave boundary was not restored");
    };
    let mut restored_result = command.token.finish();
    for _ in 0..128 {
        let a = original.run(result).unwrap();
        let b = restored.scripter.run(restored_result).unwrap();
        assert_eq!(original.position(), restored.scripter.position());
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        result = a.execute_dummy().unwrap();
        restored_result = b.execute_dummy().unwrap();
    }
}

#[inline(never)]
fn resume_presentation_pixels(
    context: &mut crate::render::PreRenderContext,
    live: &crate::adv::Adv,
) -> Vec<u8> {
    snapshot_pixels(context, live, false)
}

fn snapshot_pixels(
    context: &mut crate::render::PreRenderContext,
    live: &crate::adv::Adv,
    include_scene: bool,
) -> Vec<u8> {
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("RESUME retained presentation"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            include_scene.then_some(context.depth_stencil),
            "RESUME retained presentation",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            None,
            None,
        );
        if include_scene {
            live.adv_state.render_scene(&mut pass);
        }
        live.adv_state.route_label.render(&mut pass);
        live.adv_state.bgm_title.render(&mut pass);
        live.adv_state.notifications.render(&mut pass);
    }
    crate::layer::user::readback_test_texture(context, &target)
}

fn verify_live_authored_interactive_checkpoint<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    scenario: &std::sync::Arc<Scenario>,
    kind: i32,
    last_choice: i32,
) {
    use crate::{app::AppAction, layer::user::InteractiveLayerRef, update::UpdateContext};
    use shin_core::{primitives::update::FrameId, time::Ticks};
    let updated = reference_updated_scenario(scenario);
    let (checkpoint_pc, mut expected_response) = match kind {
        0 => (0x78d6da, 1),
        1 => (0x789e38, 0),
        2 => (
            0x7c9aed,
            match last_choice {
                0 => 27,
                1 => 0,
                2 => 6,
                3 => 28,
                _ => panic!("Unsupported Murderer checkpoint variant"),
            },
        ),
        3 => {
            assert!((0..=1).contains(&last_choice));
            (0x85e4e0, last_choice)
        }
        _ => panic!("Unsupported authored checkpoint kind{kind}"),
    };
    let checkpoint_pc = if updated {
        match kind {
            0 => 0x78d718,
            1 => 0x789e76,
            2 => 0x7c9b2b,
            3 => 0x85e53c,
            _ => unreachable!(),
        }
    } else {
        checkpoint_pc
    };
    let mut script = Scripter::new(scenario, 7, 12345);
    let mut state = VmState::for_scenario(7, 12345);
    let mut result = CommandResult::None;
    let mut historical_reads = Vec::new();
    let mut found = false;
    let mut preceding_decisions = std::collections::BTreeSet::new();
    for _ in 0..250_000 {
        let current = script.run(result).unwrap();
        result = match current {
            RuntimeCommand::QUIZ(quiz) => {
                let target = quiz.apply_state(&mut state).unwrap();
                let params = state.layers.layerbanks[target.1].params;
                if params.0 == kind {
                    assert_eq!(script.position().0, checkpoint_pc);
                    if kind == 0 {
                        assert_eq!(params.1, 3);
                    }
                    if kind == 2 && last_choice == 3 {
                        expected_response = params.1.min(20).wrapping_add(28);
                    }
                    if kind == 2 && last_choice == 2 {
                        assert!(params.1 >= 0, "Authored hint0 must be available");
                    }
                    found = true;
                    break;
                }
                assert!(
                    preceding_decisions.insert(script.position().0),
                    "Repeated checkpoint prefix decision needs its retained controller policy"
                );
                let value = match params.0 {
                    1 => {
                        let mut cake = crate::layer::user::cake_layer::CakeModel::default();
                        cake.pointer(glam::Vec2::new(850.0, 700.0), true);
                        cake.update(shin_core::time::Ticks::from_f32(7.0));
                        cake.update(shin_core::time::Ticks::ZERO);
                        cake.result.unwrap()
                    }
                    0 => {
                        let mut quiz = crate::layer::user::quiz_layer::QuizModel::new(params.1);
                        quiz.confirm();
                        quiz.update(shin_core::time::Ticks::from_f32(7.0));
                        quiz.update(shin_core::time::Ticks::ZERO);
                        quiz.result
                            .expect("Authored preceding Quiz has a visible first answer")
                    }
                    2 => {
                        use crate::layer::user::murderer::model::{
                            MurdererMenu, MurdererModel, SUBMIT_ENTRY,
                        };
                        let mut murderer = Box::new(MurdererModel::new(params.1));
                        murderer.navigate(5);
                        murderer.confirm();
                        for _ in 0..240 {
                            murderer.update(1.0);
                        }
                        assert_eq!(murderer.menu, MurdererMenu::Accusation);
                        for suspect in 34..37 {
                            assert!(murderer.activate_accusation_entry(suspect));
                        }
                        assert!(murderer.activate_accusation_entry(SUBMIT_ENTRY));
                        for _ in 0..300 {
                            if murderer.poll_result().is_some() {
                                break;
                            }
                            murderer.update(1.0);
                        }
                        let value = murderer
                            .poll_result()
                            .expect("Preceding authored accusation completes");
                        assert_eq!(value, 27);
                        value
                    }
                    mode => panic!("Unexpected preceding controller kind{mode}"),
                };
                command::replay_quiz_result(&mut state, Some(target), value);
                historical_reads.push((true, value));
                quiz.token.finish(value)
            }
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut state);
                historical_reads.push((false, value));
                get.token.finish(value)
            }
            current => {
                assert!(!matches!(
                    current,
                    RuntimeCommand::SELECT(_) | RuntimeCommand::RESUME(_)
                ));
                command::apply_command_state_ref(&current, &mut state).unwrap();
                current.execute_dummy().unwrap()
            }
        };
    }
    assert!(found);
    let entry = state.snapshot_entry();
    let globals = SaveData {
        persist_data: state.persist.clone(),
        save_vectors: state.save_vectors.clone(),
        auto_save_slot: state.auto_save_slot.clone(),
        ..SaveData::default()
    };
    let mut authored_assets = assets.clone();
    authored_assets.scenario = scenario.clone();
    let mut live = crate::adv::Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        authored_assets,
        Scripter::new(scenario, 7, 12345),
    );
    live.load_saved_scene(&globals, &entry, server.clone())
        .unwrap();
    let mut frame = FrameId::default();
    let mut step = |live: &mut crate::adv::Adv,
                    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
                    input| {
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while live.is_loading_saved_scene() {
        assert!(
            std::time::Instant::now() < deadline,
            "Authored interactive restore exceeded deadline"
        );
        step(&mut live, context, enum_map::EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        live.saved_scene_load_error().is_none(),
        "Authored interactive restore: {:?}",
        live.saved_scene_load_error()
    );
    assert_eq!(live.vm_state.save_position, entry.save_position);
    match live.active_interactive_layer() {
        Some(InteractiveLayerRef::Cake(cake)) if kind == 1 => {
            assert_eq!(cake.reference_controller_state().2, -1);
            assert_eq!(cake.result(), None);
        }
        Some(InteractiveLayerRef::Quiz(quiz)) if kind == 0 => {
            assert_eq!(quiz.reference_controller_state().2, 0);
            assert_eq!(quiz.result(), None);
        }
        Some(InteractiveLayerRef::Murderer(view)) if kind == 2 => {
            assert_eq!(
                view.reference_menu_state(),
                (crate::layer::user::murderer::model::MurdererMenu::Top, false)
            );
            assert_eq!(view.reference_controller_state().2, 0);
            assert_eq!(view.result(), None);
        }
        Some(InteractiveLayerRef::Last(last)) if kind == 3 => {
            assert_eq!(last.reference_controller_state().2, -1);
            assert_eq!(last.result(), None);
            assert!(last.reference_accepts_input());
        }
        _ => panic!("Authored checkpoint must start its real QUIZ dispatcher"),
    }
    // Render the actual installed root, not a standalone atlas or synthetic layer.
    let pixels = snapshot_pixels(context, &live, true);
    assert!(
        pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count()
            > 10_000
    );
    if kind == 0 {
        // Physical pointer capture through the real restored ADV/QUIZ owner.
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        let rect = quiz.reference_button_rect(0);
        let point = glam::Vec2::new(rect.x + rect.z / 2.0, rect.y + rect.w / 2.0);
        let extent = (1280, 720);
        let pointer = |live: &mut crate::adv::Adv, point: glam::Vec2, held: bool| {
            live.sample_modal_pointer(point * (2.0 / 3.0), extent, held, 1.0 / 60.0);
            live.handle_pointer_state(point, false, held, 1.0 / 60.0);
        };
        pointer(&mut live, point, true);
        let mut competing = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        competing[AppAction::AnyDown].is_clicked = true;
        competing[AppAction::AnyDown].is_clicked_or_repeated = true;
        competing[AppAction::Enter].is_clicked = true;
        step(&mut live, context, competing);
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(quiz.reference_controller_state().2, 0);
        assert_eq!(quiz.reference_pointer_feedback(0), (Some(0), 0.0, None));
        assert_eq!(quiz.result(), None);
        for _ in 0..14 {
            pointer(&mut live, point, true);
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        let (captured, flash, pending) = quiz.reference_pointer_feedback(0);
        assert_eq!(captured, Some(0));
        assert!((flash - 0.7).abs() < 1e-6);
        assert_eq!(pending, None);
        assert_eq!(quiz.result(), None);
        assert_ne!(
            snapshot_pixels(context, &live, true),
            pixels,
            "Held white highlight changes real root pixels"
        );
        pointer(&mut live, point + glam::Vec2::new(20.0, 0.0), true);
        step(&mut live, context, enum_map::EnumMap::default());
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        let (captured, flash, pending) = quiz.reference_pointer_feedback(0);
        assert_eq!(captured, None);
        assert!((flash - 0.5).abs() < 1e-6);
        assert_eq!(pending, None);
        pointer(&mut live, point, false);
        for _ in 0..4 {
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(quiz.reference_pointer_feedback(0), (None, 0.0, None));
        assert_eq!(quiz.result(), None);
        assert_eq!(live.vm_state.save_position, entry.save_position);
        // The host samples idle pointer state before the next controller frame.
        pointer(&mut live, glam::Vec2::new(1900.0, 1000.0), false);
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        let count = quiz.reference_focus_values().len();
        assert!(count > 1);
        let mut opposite = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        opposite[AppAction::Up].is_clicked_or_repeated = true;
        opposite[AppAction::AnyDown].is_clicked_or_repeated = true;
        step(&mut live, context, opposite);
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(
            quiz.reference_controller_state().2,
            count as i32 - 1,
            "Native Quiz Up wins over Down through the actual ADV owner"
        );
        let focus = quiz.reference_focus_values();
        assert!((focus[0] - 0.95).abs() < 1e-6 && (focus[count - 1] - 0.25).abs() < 1e-6);
        let mut down = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        down[AppAction::AnyDown].is_clicked_or_repeated = true;
        step(&mut live, context, down);
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(quiz.reference_controller_state().2, 0);
        for _ in 0..20 {
            step(&mut live, context, enum_map::EnumMap::default());
        }
        assert_eq!(live.vm_state.save_position, entry.save_position);
    }
    if kind == 1 {
        // Real held-direction and signed two-axis movement from native default.
        live.set_interactive_axes(glam::vec2(-0.5, 0.25));
        step(&mut live, context, enum_map::EnumMap::default());
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().0, glam::vec2(1434.0, 903.0));
        let mut held = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        held[AppAction::DpadLeft].is_held = true;
        held[AppAction::DpadUp].is_held = true;
        step(&mut live, context, held);
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().0, glam::vec2(1420.0, 898.0));
        live.set_interactive_axes(glam::Vec2::ZERO);
        for (action, count) in [(AppAction::DpadLeft, 72), (AppAction::DpadUp, 25)] {
            for _ in 0..count {
                let mut held =
                    enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
                held[action].is_held = true;
                step(&mut live, context, held);
                assert_eq!(live.vm_state.save_position, entry.save_position);
            }
        }
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().0, glam::vec2(844.0, 698.0));
        assert_eq!(cake.reference_controller_state().2, 0);
        assert!(
            cake.take_sounds().is_empty(),
            "Native Cake movement has no move cue"
        );
        let point = glam::vec2(852.0, 702.0);
        let pointer = |live: &mut crate::adv::Adv, point: glam::Vec2, held: bool| {
            live.sample_modal_pointer(point * (2.0 / 3.0), (1280, 720), held, 1.0 / 60.0);
            live.handle_pointer_state(point, false, held, 1.0 / 60.0);
        };
        pointer(&mut live, point, true);
        let mut competing = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        competing[AppAction::DpadRight].is_held = true;
        competing[AppAction::Enter].is_clicked = true;
        step(&mut live, context, competing);
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().1, Some(0));
        assert_eq!(
            cake.result(),
            None,
            "Captured Cake touch consumes Enter and movement"
        );
        for _ in 0..14 {
            pointer(&mut live, point, true);
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().0, point);
        assert_eq!(cake.reference_feedback().5[0], 0.7);
        pointer(&mut live, point + glam::vec2(20.0, 0.0), true);
        step(&mut live, context, enum_map::EnumMap::default());
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().1, None);
        assert!((cake.reference_feedback().5[0] - 0.5).abs() < 1e-6);
        pointer(&mut live, point, false);
        for _ in 0..4 {
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Cake(cake)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(cake.reference_feedback().5[0], 0.0);
        assert_eq!(cake.result(), None);
        assert!(cake.take_sounds().is_empty());
        pointer(&mut live, glam::vec2(1900.0, 1000.0), false);
    }
    if kind == 3 {
        let point = glam::vec2(400.0, 900.0);
        let pointer = |live: &mut crate::adv::Adv, point: glam::Vec2, held: bool| {
            live.sample_modal_pointer(point * (2.0 / 3.0), (1280, 720), held, 1.0 / 60.0);
            live.handle_pointer_state(point, false, held, 1.0 / 60.0);
        };
        pointer(&mut live, point, true);
        let mut competing = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        for action in [AppAction::Left, AppAction::Right, AppAction::Enter] {
            competing[action].is_clicked = true;
            competing[action].is_clicked_or_repeated = true;
        }
        step(&mut live, context, competing);
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_controller_state().2, -1);
        assert_eq!(last.reference_pointer_feedback().0, Some(0));
        assert_eq!(last.result(), None);
        for _ in 0..14 {
            pointer(&mut live, point, true);
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert!((last.reference_pointer_feedback().2[3] - 0.7).abs() < 1e-6);
        assert_ne!(snapshot_pixels(context, &live, true), pixels);
        pointer(&mut live, point + glam::vec2(20.0, 0.0), true);
        step(&mut live, context, enum_map::EnumMap::default());
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_pointer_feedback().0, None);
        assert!((last.reference_pointer_feedback().2[3] - 0.5).abs() < 1e-6);
        pointer(&mut live, point, false);
        for _ in 0..4 {
            step(&mut live, context, enum_map::EnumMap::default());
        }
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_pointer_feedback().2[3], 0.0);
        assert!(last.take_sounds().is_empty());
        pointer(&mut live, glam::vec2(1900.0, 1000.0), false);
    }
    let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
    let navigation = match kind {
        2 => AppAction::Up,
        3 if last_choice == 0 => AppAction::Left,
        3 => AppAction::Right,
        _ => AppAction::AnyDown,
    };
    input[navigation].is_clicked_or_repeated = true;
    input[navigation].is_clicked = true;
    if kind == 3 && last_choice == 1 {
        let rect = crate::layer::user::last_layer::CHOICES[1];
        let point = glam::vec2(rect.x + rect.z / 2.0, rect.y + rect.w / 2.0);
        live.sample_modal_pointer(point * 2.0, (3840, 2160), true, 1.0 / 60.0);
        live.handle_pointer_state(point, false, true, 1.0 / 60.0);
        step(&mut live, context, input);
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_controller_state().2, -1);
        assert_eq!(last.reference_pointer_feedback().0, Some(1));
        let outside = glam::vec2(1900.0, 1000.0);
        live.sample_modal_pointer(outside * 2.0, (3840, 2160), false, 1.0 / 60.0);
        live.handle_pointer_state(outside, false, false, 1.0 / 60.0);
        step(&mut live, context, enum_map::EnumMap::default());
    } else if kind != 1 {
        step(&mut live, context, input);
    }
    match live.active_interactive_layer() {
        Some(InteractiveLayerRef::Cake(cake)) if kind == 1 => {
            assert_eq!(cake.reference_controller_state().2, 0);
            assert_eq!(cake.result(), None);
        }
        Some(InteractiveLayerRef::Quiz(quiz)) if kind == 0 => {
            assert_eq!(quiz.reference_controller_state().2, 1);
            assert_eq!(quiz.result(), None);
        }
        Some(InteractiveLayerRef::Murderer(view)) if kind == 2 => {
            assert_eq!(view.reference_controller_state().2, 5);
            assert_eq!(view.result(), None);
        }
        Some(InteractiveLayerRef::Last(last)) if kind == 3 => {
            assert_eq!(last.reference_controller_state().2, last_choice);
            assert_eq!(last.result(), None);
            assert!(!last.reference_accepts_input());
        }
        _ => panic!("Navigation keeps QUIZ pending"),
    }
    assert_eq!(live.vm_state.save_position, entry.save_position);
    assert_ne!(
        snapshot_pixels(context, &live, true),
        pixels,
        "Real interactive navigation changes authored root pixels"
    );
    if kind == 3 {
        // An early Enter must be consumed while the selected door slides.
        let mut early = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        early[AppAction::Enter].is_clicked = true;
        early[AppAction::Enter].is_clicked_or_repeated = true;
        step(&mut live, context, early);
        assert_eq!(live.vm_state.save_position, entry.save_position);
        let mut ready = false;
        for _ in 0..90 {
            let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
                panic!()
            };
            assert_eq!(last.reference_controller_state().2, last_choice);
            assert_eq!(last.result(), None);
            if last.reference_accepts_input() {
                ready = true;
                break;
            }
            step(&mut live, context, enum_map::EnumMap::default());
            assert_eq!(live.vm_state.save_position, entry.save_position);
        }
        assert!(ready, "Authored Last door entry must finish before confirm");
        assert_ne!(snapshot_pixels(context, &live, true), pixels);
    }
    if kind == 2 && matches!(last_choice, 1 | 2) {
        // Initial Up selected Accusation5; real controller navigation now
        // chooses Replay0 or Hints4 without replacing the checkpoint model.
        let action = if last_choice == 1 {
            AppAction::AnyDown
        } else {
            AppAction::Up
        };
        let mut extra = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        extra[action].is_clicked = true;
        extra[action].is_clicked_or_repeated = true;
        step(&mut live, context, extra);
        let Some(InteractiveLayerRef::Murderer(view)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(
            view.reference_controller_state().2,
            if last_choice == 1 { 0 } else { 4 }
        );
    }
    let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
    input[AppAction::Enter].is_clicked = true;
    input[AppAction::Enter].is_clicked_or_repeated = true;
    if kind == 0 && last_choice == 1 {
        input[AppAction::AnyDown].is_clicked_or_repeated = true;
        input[AppAction::Up].is_clicked_or_repeated = true;
        step(&mut live, context, input);
        let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(
            quiz.reference_controller_state().2,
            1,
            "Enter confirms current Quiz row before simultaneous Up and Down"
        );
        assert_eq!(quiz.reference_pointer_feedback(1).2, Some(1));
        assert!(quiz.result().is_none());
    } else if kind == 0 || kind == 1 {
        let point = if kind == 1 {
            glam::vec2(852.0, 702.0)
        } else {
            let Some(InteractiveLayerRef::Quiz(quiz)) = live.active_interactive_layer() else {
                panic!()
            };
            let rect = quiz.reference_button_rect(1);
            glam::Vec2::new(rect.x + rect.z / 2.0, rect.y + rect.w / 2.0)
        };
        let extent = (3840, 2160);
        live.sample_modal_pointer(point * 2.0, extent, true, 1.0 / 60.0);
        live.handle_pointer_state(point, false, true, 1.0 / 60.0);
        step(&mut live, context, input); // Enter is consumed by the captured mouse.
        match live.active_interactive_layer() {
            Some(InteractiveLayerRef::Quiz(quiz)) if kind == 0 => {
                assert_eq!(quiz.reference_pointer_feedback(1), (Some(1), 0.0, None));
                assert_eq!(quiz.result(), None);
            }
            Some(InteractiveLayerRef::Cake(cake)) if kind == 1 => {
                assert_eq!(cake.reference_feedback().1, Some(0));
                assert_eq!(cake.reference_feedback().2, false);
                assert_eq!(cake.result(), None);
            }
            _ => panic!(),
        }
        let outside = glam::Vec2::new(1900.0, 1000.0);
        live.sample_modal_pointer(outside * 2.0, extent, false, 1.0 / 60.0);
        live.handle_pointer_state(outside, false, false, 1.0 / 60.0);
        step(&mut live, context, enum_map::EnumMap::default());
        match live.active_interactive_layer() {
            Some(InteractiveLayerRef::Quiz(quiz)) if kind == 0 => {
                let (captured, flash, pending) = quiz.reference_pointer_feedback(1);
                assert_eq!(captured, None);
                assert!((flash - 0.2).abs() < 1e-6);
                assert_eq!(pending, Some(1));
            }
            Some(InteractiveLayerRef::Cake(cake)) if kind == 1 => {
                let (finger, captured, confirming, opacity, _, flash) =
                    cake.reference_feedback();
                assert_eq!(finger, point);
                assert_eq!(captured, None);
                assert!(confirming);
                assert!((flash[0] - 0.2).abs() < 1e-6);
                assert!((opacity - 0.9).abs() < 1e-6);
            }
            _ => panic!(),
        }
    } else if kind == 3 && last_choice == 1 {
        let rect = crate::layer::user::last_layer::confirmation_rects(1)[1];
        let point = glam::vec2(rect.x + rect.z / 2.0, rect.y + rect.w / 2.0);
        live.sample_modal_pointer(point * 2.0, (3840, 2160), true, 1.0 / 60.0);
        live.handle_pointer_state(point, false, true, 1.0 / 60.0);
        input[AppAction::Left].is_clicked_or_repeated = true;
        step(&mut live, context, input);
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_controller_state().2, 1);
        assert_eq!(last.reference_pointer_feedback().0, Some(0));
        assert!(!last.reference_pointer_feedback().1);
        let outside = glam::vec2(1900.0, 1000.0);
        live.sample_modal_pointer(outside * 2.0, (3840, 2160), false, 1.0 / 60.0);
        live.handle_pointer_state(outside, false, false, 1.0 / 60.0);
        step(&mut live, context, enum_map::EnumMap::default());
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(last.reference_pointer_feedback().0, None);
        assert!(last.reference_pointer_feedback().1);
    } else {
        if kind == 3 {
            input[AppAction::Left].is_clicked_or_repeated = true;
            input[AppAction::Right].is_clicked_or_repeated = true;
            input[AppAction::Cancel].is_clicked = true;
        }
        step(&mut live, context, input);
    }
    if kind == 0 || kind == 1 {
        assert_eq!(
            live.active_interactive_layer().unwrap().result(),
            None,
            "Native confirmation flash delays response"
        );
        assert_eq!(live.vm_state.save_position, entry.save_position);
        assert_ne!(snapshot_pixels(context, &live, true), pixels);
        let mut completed = false;
        for _ in 0..16 {
            step(&mut live, context, enum_map::EnumMap::default());
            if live.vm_state.save_position > entry.save_position {
                completed = true;
                break;
            }
        }
        assert!(
            completed,
            "Authored Cake/Quiz must publish its real delayed answer"
        );
    }
    if kind == 3 {
        let Some(InteractiveLayerRef::Last(last)) = live.active_interactive_layer() else {
            panic!()
        };
        assert_eq!(
            last.result(),
            None,
            "Last confirmation flash must delay QUIZ completion"
        );
        assert_eq!(live.vm_state.save_position, entry.save_position);
        let mut completed = false;
        for _ in 0..90 {
            step(&mut live, context, enum_map::EnumMap::default());
            if live.vm_state.save_position > entry.save_position {
                completed = true;
                break;
            }
        }
        assert!(
            completed,
            "Authored Last must publish its actual delayed response"
        );
    }
    if kind == 2 {
        use crate::layer::user::murderer::model::{
            MurdererMenu, MurdererWipeKind, SUBMIT_ENTRY, accusation_rect, replay_rect,
        };
        assert_eq!(live.vm_state.save_position, entry.save_position);
        let target_menu = match last_choice {
            1 => MurdererMenu::Replay,
            2 => MurdererMenu::Hints,
            _ => MurdererMenu::Accusation,
        };
        let target_wipe = if target_menu == MurdererMenu::Accusation {
            MurdererWipeKind::Glass
        } else {
            MurdererWipeKind::Default
        };
        let mut entered_accusation = false;
        let mut captured_glass = false;
        for tick in 0..360 {
            let Some(InteractiveLayerRef::Murderer(view)) = live.active_interactive_layer()
            else {
                panic!("Authored Murderer must remain pending through menu entry");
            };
            assert_eq!(view.result(), None);
            if !captured_glass && view.reference_wipe_kind() == Some(target_wipe) {
                let frozen = view.reference_frozen_pixels(context);
                assert_eq!(frozen.len(), 1920 * 1080 * 4);
                assert!(
                    frozen
                        .chunks_exact(4)
                        .filter(|pixel| pixel[..3] != [0, 0, 0])
                        .count()
                        > 10_000,
                    "Authored menu wipe retains its actual captured presentation"
                );
                captured_glass = true;
            }
            if view.reference_menu_state() == (target_menu, false) {
                entered_accusation = true;
                break;
            }
            assert_eq!(live.vm_state.save_position, entry.save_position);
            step(&mut live, context, enum_map::EnumMap::default());
            if tick % 30 == 0 {
                let _ = snapshot_pixels(context, &live, true);
            }
        }
        assert!(entered_accusation && captured_glass);
        assert_ne!(
            snapshot_pixels(context, &live, true),
            pixels,
            "Authored Top and selected submenu root presentations differ"
        );
        if target_menu == MurdererMenu::Accusation {
            if last_choice == 0 {
                for suspect in 34..37 {
                    let rect = accusation_rect(suspect).unwrap();
                    let point = glam::vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
                    live.handle_pointer_state(point, true, true, 0.0);
                    live.handle_pointer_state(point, false, false, 0.0);
                    step(&mut live, context, enum_map::EnumMap::default());
                    let Some(InteractiveLayerRef::Murderer(view)) =
                        live.active_interactive_layer()
                    else {
                        panic!()
                    };
                    assert!(
                        view.reference_accusation_selection(suspect).1,
                        "Normal authored accusation press/release selects suspect{suspect}"
                    );
                    assert_eq!(view.result(), None);
                    assert_eq!(live.vm_state.save_position, entry.save_position);
                }
            }
            let rect = accusation_rect(SUBMIT_ENTRY).unwrap();
            let point = glam::vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
            live.handle_pointer_state(point, true, true, 0.0);
            live.handle_pointer_state(point, false, false, 0.0);
            let Some(InteractiveLayerRef::Murderer(view)) = live.active_interactive_layer()
            else {
                panic!()
            };
            assert_eq!(
                view.result(),
                None,
                "Submit must wait for its native completion phases"
            );
        } else if target_menu == MurdererMenu::Replay {
            let rect = replay_rect(0).unwrap();
            let point = glam::vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
            live.handle_pointer_state(point, true, true, 0.0);
            live.handle_pointer_state(point, false, false, 0.0);
        } else {
            let Some(InteractiveLayerRef::Murderer(view)) = live.active_interactive_layer()
            else {
                panic!()
            };
            assert_eq!(
                view.reference_hint_selection().0,
                expected_response as usize - 6
            );
            let mut confirm =
                enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
            confirm[AppAction::Enter].is_clicked = true;
            confirm[AppAction::Enter].is_clicked_or_repeated = true;
            step(&mut live, context, confirm);
        }
        let mut completed = false;
        for tick in 0..360 {
            step(&mut live, context, enum_map::EnumMap::default());
            if live.vm_state.save_position > entry.save_position {
                completed = true;
                break;
            }
            if tick % 30 == 0 {
                let _ = snapshot_pixels(context, &live, true);
            }
        }
        assert!(
            completed,
            "Actual authored Murderer response must reach QUIZ completion"
        );
    }
    assert!(live.vm_state.save_position > entry.save_position);
    let route = live.vm_state.route.snapshot();
    let mut reader = route.reader();
    for (is_quiz, expected) in historical_reads {
        let actual = if is_quiz {
            reader.read_quiz().unwrap()
        } else {
            reader.read_persistent().unwrap()
        };
        assert_eq!(actual, expected);
    }
    assert_eq!(
        reader.read_quiz().unwrap(),
        expected_response,
        "Actual command completion records the controller response"
    );
    assert!(!live.is_finished());
    assert!(live.scripter.position().0 > checkpoint_pc);
    if matches!(kind, 0 | 1 | 2 | 3) {
        // The actual answer's route bytes and exact completion boundary
        // reconstruct an answered layer before the script unloads it.
        let mut completed_entry = entry.clone();
        completed_entry.save_position += 1;
        completed_entry.route_data = live.vm_state.snapshot_entry().route_data;
        let reconstructed =
            ReconstructedScene::from_save(scenario, &completed_entry, &globals, 250_000)
                .unwrap();
        assert!(matches!(
            reconstructed.stop,
            ReconstructionStop::Response(_)
        ));
        let (bank, plane) = reconstructed
            .state
            .layers
            .layerbanks
            .iter()
            .find_map(|(bank, layer)| {
                (layer.layer_type == Some(shin_core::vm::command::types::LayerType::Quiz)
                    && layer.params.0 == kind
                    && layer.is_interation_completed)
                    .then_some((bank, layer.plane))
            })
            .expect("Completed interactive layer persists at response boundary");
        assert_eq!(
            reconstructed.state.layers.layerbanks[bank].interaction_result,
            Some(expected_response)
        );
        live.load_saved_scene(&globals, &completed_entry, server.clone())
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while live.is_loading_saved_scene() {
            assert!(std::time::Instant::now() < deadline);
            step(&mut live, context, enum_map::EnumMap::default());
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            live.saved_scene_load_error().is_none(),
            "{:?}",
            live.saved_scene_load_error()
        );
        assert_eq!(live.vm_state.save_position, completed_entry.save_position);
        match &*live
            .adv_state
            .plane_layer_group(plane)
            .get_layer(bank)
            .unwrap()
        {
            crate::layer::user::UserLayer::Quiz(quiz) if kind == 0 => {
                assert_eq!(quiz.reference_controller_state().2, 1);
                assert_eq!(quiz.result(), Some(expected_response));
                assert_eq!(quiz.reference_focus_values(), [0.5, 1.0]);
                assert_eq!(quiz.reference_pointer_feedback(1), (None, 0.0, None));
                assert!(
                    quiz.take_sounds().is_empty(),
                    "Restoration must not play a decision cue"
                );
            }
            crate::layer::user::UserLayer::Cake(cake) if kind == 1 => {
                assert_eq!(cake.reference_controller_state().2, -1);
                assert_eq!(
                    cake.reference_visual_slice(),
                    Some(expected_response as usize)
                );
                assert_eq!(
                    cake.result(),
                    Some(-1),
                    "Native restores artwork while retaining constructor response index"
                );
            }
            crate::layer::user::UserLayer::Last(last) if kind == 3 => {
                assert_eq!(last.reference_controller_state().2, -1);
                assert_eq!(last.result(), Some(-1));
                assert_eq!(
                    last.reference_restored_tweens(),
                    [
                        if expected_response == 0 {
                            960.0
                        } else {
                            -960.0
                        },
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0
                    ]
                );
                assert!(!last.reference_accepts_input());
                assert!(last.take_sounds().is_empty());
            }
            crate::layer::user::UserLayer::Murderer(view) if kind == 2 => {
                use crate::layer::user::murderer::model::{MurdererMenu, SUBMIT_ENTRY};
                assert_eq!(
                    view.result(),
                    None,
                    "Native restored child does not re-publish the retained parent response"
                );
                if expected_response < 6 {
                    assert_eq!(view.reference_menu_state(), (MurdererMenu::Replay, true));
                    assert!(view.allows_playback_mode_changes());
                } else if expected_response < 27 {
                    assert_eq!(view.reference_menu_state(), (MurdererMenu::Hints, false));
                    assert_eq!(
                        view.reference_hint_selection(),
                        ((expected_response - 6) as usize, 0.0)
                    );
                } else {
                    assert_eq!(
                        view.reference_menu_state(),
                        (MurdererMenu::Accusation, false)
                    );
                    let (sprite, suspects, focus) = view.reference_completed_visuals();
                    assert_eq!(sprite, if expected_response == 27 { 1 } else { 2 });
                    assert_eq!(
                        suspects,
                        if expected_response == 27 {
                            vec![34, 35, 36]
                        } else {
                            vec![]
                        }
                    );
                    assert_eq!(focus, Some(SUBMIT_ENTRY));
                }
                assert!(view.take_sounds().is_empty());
                assert!(view.reference_wipe_kind().is_none());
            }
            _ => panic!("Completed interactive type matches its authored mode"),
        }
        let restored_pixels = snapshot_pixels(context, &live, true);
        assert_ne!(
            restored_pixels, pixels,
            "Completed answer visibly differs from the initial row"
        );
        // Repeated restore is idempotent: no second flash/cue or route write.
        live.load_saved_scene(&globals, &completed_entry, server.clone())
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while live.is_loading_saved_scene() {
            assert!(std::time::Instant::now() < deadline);
            step(&mut live, context, enum_map::EnumMap::default());
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(live.saved_scene_load_error().is_none());
        let snapshot = live.vm_state.snapshot_entry();
        assert_eq!(snapshot.scenario_id, completed_entry.scenario_id);
        assert_eq!(snapshot.random_seed, completed_entry.random_seed);
        assert_eq!(snapshot.save_position, completed_entry.save_position);
        assert_eq!(
            format!("{:?}", snapshot.route_data),
            format!("{:?}", completed_entry.route_data)
        );
        assert_eq!(
            snapshot_pixels(context, &live, true),
            restored_pixels,
            "Reloading the same completed interactive layer produces identical root pixels"
        );
        eprintln!(
            "Completed authored mode{kind} answer{expected_response} reload: native restored visual/retained response state, noflash/cue, exact saved boundary and idempotent real root pixels"
        );
    }
    eprintln!(
        "Live authored entry7 (Episode8) kind{kind} pc{checkpoint_pc:x} response{expected_response}: actual checkpoint restore/assets/root pixels, normal input focus/confirm and real QUIZ historical response/continuation pass on the default audio output"
    );
}

fn capture_authored_frame(
    context: &crate::render::PreRenderContext,
    target: &wgpu::Texture,
    directory: &std::path::Path,
    live: &crate::adv::Adv,
    entry: i32,
    frame: u64,
) {
    // Copy the already submitted frame. Re-rendering could observe or
    // initialize different native layer state and is not a capture.
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Authored frame capture"),
        size: 1920 * 1080 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut copy = context.device.create_command_encoder(&Default::default());
    copy.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1920 * 4),
                rows_per_image: Some(1080),
            },
        },
        wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
    );
    context.queue.submit([copy.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    context
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    receive.recv().unwrap().unwrap();
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .expect("Readback mapping must be ready");
    let name = format!(
        "entry{entry}-save{}-pc{:08x}-frame{frame}",
        live.vm_state.save_position,
        live.scripter.position().0
    );
    std::fs::write(directory.join(format!("{name}.rgba")), &pixels).unwrap();
    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
    for pixel in pixels.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write(directory.join(format!("{name}.ppm")), ppm).unwrap();
    std::fs::write(directory.join(format!("{name}.txt")), format!(
        "entry={entry}\nframe={frame}\nsave_position={}\npc=0x{:08x}\nwidth=1920\nheight=1080\nformat=RGBA8Unorm\ncommand={:?}\nscope=Already rendered original ADV frame; no re-render or forced command completion. This is a port frame, not an independent native reference.\n",
        live.vm_state.save_position, live.scripter.position().0, live.current_command)).unwrap();
    eprintln!(
        "Authored frame captured: {}",
        directory.join(format!("{name}.ppm")).display()
    );
    drop(pixels);
    readback.unmap();
}

fn verify_native_blend_fragment(
    context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        gpu_texture::GpuTexture,
        quad_vertices::build_quad_vertices,
        shaders::types::{
            buffer::VertexSource,
            texture::{TextureTarget, TextureTargetKind},
            vertices::BlendVertex,
        },
    };
    let first = [
        [17, 101, 233, 41],
        [207, 43, 71, 193],
        [83, 239, 19, 131],
        [251, 149, 113, 7],
    ];
    let second = [
        [211, 29, 89, 173],
        [47, 181, 227, 61],
        [137, 67, 151, 241],
        [31, 223, 53, 109],
    ];
    let texture = |label, pixels: &[[u8; 4]; 4]| {
        GpuTexture::new_static_with_data(
            context.device,
            context.queue,
            Some(label),
            winit::dpi::PhysicalSize::new(2, 2),
            wgpu::TextureFormat::Rgba8Unorm,
            &pixels.concat(),
        )
    };
    let texture1 = texture("Native Blend first", &first);
    let texture2 = texture("Native Blend second", &second);
    let tint = UnormColor::from_rgba(179, 107, 229, 151);
    let tint_values = tint.into_vec4().to_array();
    let mut checks = 0;
    for (blend, flash) in [
        ([0.0, 0.0, 0.0, 0.0], [0.2, 0.7, 0.4, 0.0]),
        ([1.0, 1.0, 1.0, 1.0], [0.8, 0.1, 0.6, 0.0]),
        ([0.13, 0.71, 0.37, 0.83], [0.91, 0.23, 0.59, 0.41]),
        ([-0.25, 1.25, 0.61, 0.19], [0.17, 0.83, 0.31, 1.2]),
    ] {
        for index in 0..4 {
            let other = (index + 1) % 4;
            let uv = |i| glam::vec2(0.25 + (i % 2) as f32 * 0.5, 0.25 + (i / 2) as f32 * 0.5);
            let a = uv(index);
            let b = uv(other);
            let vertices = build_quad_vertices(|corner| BlendVertex {
                position: glam::vec3(64.0 + corner.x * 128.0, 64.0 + corner.y * 128.0, 0.0),
                color: tint,
                texture_position: glam::vec4(a.x, a.y, b.x, b.y),
            });
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view,
                    },
                    None,
                    "Native Blend arithmetic",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .build(
                            RenderProgramWithArguments::Blend {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                texture1: texture1.as_source(),
                                texture2: texture2.as_source(),
                                transform: crate::render::top_left_projection_matrix(),
                                blend: glam::Vec4::from_array(blend),
                                flash: glam::Vec4::from_array(flash),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
            let pixels = crate::layer::user::readback_test_texture(context, target);
            for channel in 0..4 {
                let source = first[index][channel] as f32 / 255.0;
                let destination = second[other][channel] as f32 / 255.0;
                // Native FADD then FFMA; alpha bypasses the RGB flash.
                let mixed = (destination - source).mul_add(blend[channel], source);
                let flashed = if channel < 3 {
                    (flash[channel] - mixed).mul_add(flash[3], mixed)
                } else {
                    mixed
                };
                let expected = (flashed * tint_values[channel]).clamp(0.0, 1.0) * 255.0;
                for (x, y) in [(80, 80), (128, 128), (176, 176)] {
                    let actual = pixels[(y * 1920 + x) * 4 + channel] as f32;
                    assert!(
                        (actual - expected).abs() <= 1.1,
                        "Native Blend case {index}, channel {channel}: {actual} vs {expected}; blend={blend:?}, flash={flash:?}"
                    );
                    checks += 1;
                }
            }
        }
    }
    eprintln!(
        "Native Blend fragment: {checks} GPU channel checks pass for independent UVs, unequal RGBA weights, flash, tint, alpha and unclamped extrapolation"
    );
}

fn verify_native_button_fragment(
    context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        ColorBlendType, RenderRequestBuilder,
        gpu_texture::GpuTexture,
        quad_vertices::QuadVertices,
        shaders::types::texture::{TextureTarget, TextureTargetKind},
    };
    let colors = [
        [23u8, 197, 83, 47],
        [211, 59, 137, 181],
        [157, 31, 229, 113],
        [67, 241, 19, 223],
    ];
    let texture = GpuTexture::new_static_with_data(
        context.device,
        context.queue,
        Some("Native Button flash"),
        winit::dpi::PhysicalSize::new(2, 2),
        wgpu::TextureFormat::Rgba8Unorm,
        &colors.concat(),
    );
    let tint = UnormColor::from_rgba(193, 127, 79, 167);
    let tint_values = tint.into_vec4().to_array();
    let mut checks = 0;
    for (flash1, flash2) in [
        ([0.83, 0.17, 0.51, 0.0], [0.29, 0.91, 0.43, 0.0]),
        ([0.83, 0.17, 0.51, 1.0], [0.29, 0.91, 0.43, 0.0]),
        ([0.83, 0.17, 0.51, 0.37], [0.29, 0.91, 0.43, 0.61]),
        ([0.83, 0.17, 0.51, -0.25], [0.29, 0.91, 0.43, 1.25]),
    ] {
        for (index, source) in colors.iter().enumerate() {
            let uv = glam::vec2(
                0.25 + (index % 2) as f32 * 0.5,
                0.25 + (index / 2) as f32 * 0.5,
            );
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view,
                    },
                    None,
                    "Native Button arithmetic",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                QuadVertices::new()
                    .with_box(64.0, 64.0, 192.0, 192.0)
                    .with_tex_box(uv.x, uv.y, uv.x, uv.y)
                    .with_color(tint)
                    .render_button(
                        &mut pass,
                        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                        texture.as_source(),
                        crate::render::top_left_projection_matrix(),
                        glam::Vec4::from_array(flash1),
                        glam::Vec4::from_array(flash2),
                    );
            }
            let pixels = crate::layer::user::readback_test_texture(context, target);
            for channel in 0..4 {
                let sampled = source[channel] as f32 / 255.0;
                let flashed = if channel < 3 {
                    let first = (flash1[channel] - sampled).mul_add(flash1[3], sampled);
                    (flash2[channel] - first).mul_add(flash2[3], first)
                } else {
                    sampled
                };
                let expected = (flashed * tint_values[channel]).clamp(0.0, 1.0) * 255.0;
                for (x, y) in [(80, 80), (128, 128), (176, 176)] {
                    let actual = pixels[(y * 1920 + x) * 4 + channel] as f32;
                    assert!(
                        (actual - expected).abs() <= 1.1,
                        "Native Button texel {index}, channel {channel}: {actual} vs {expected}; flash1={flash1:?}, flash2={flash2:?}"
                    );
                    checks += 1;
                }
            }
        }
    }
    eprintln!(
        "Native Button fragment: {checks} GPU channel checks pass for ordered RGB flashes, extrapolation, tint and flash-independent alpha"
    );
}

fn verify_native_font_fragments(
    context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
) {
    use shin_core::primitives::color::{FloatColor4, UnormColor};
    use shin_render::{
        ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        gpu_texture::GpuTexture,
        quad_vertices::build_quad_vertices,
        shaders::types::{
            buffer::VertexSource,
            texture::{TextureTarget, TextureTargetKind},
            vertices::TextVertex,
        },
    };
    let vertices = |weight| {
        build_quad_vertices(|corner: glam::Vec2| TextVertex {
            position: glam::vec4(64.0 + corner.x * 128.0, 64.0 + corner.y * 128.0, 0.5, 0.5),
            color: weight,
        })
    };
    let mut checks = 0;
    let mut compare = |context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
                       expected: [f32; 4],
                       label: &str| {
        let pixels = crate::layer::user::readback_test_texture(context, target);
        for (x, y) in [(80, 80), (128, 128), (176, 176)] {
            for channel in 0..4 {
                let actual = pixels[(y * 1920 + x) * 4 + channel] as f32;
                let expected = expected[channel].clamp(0.0, 1.0) * 255.0;
                assert!(
                    (actual - expected).abs() <= 1.2,
                    "{label}, channel{channel}: {actual} vs {expected}"
                );
                checks += 1;
            }
        }
    };
    for coverage in [0u8, 31, 173, 255] {
        let glyph = GpuTexture::new_static_with_data(
            context.device,
            context.queue,
            Some("Native font coverage"),
            winit::dpi::PhysicalSize::new(1, 1),
            wgpu::TextureFormat::R8Unorm,
            &[coverage],
        );
        for (first, second) in [
            ([0.13, 0.71, 0.37, 0.23], [0.83, 0.19, 0.59, 0.91]),
            ([0.91, 0.29, 0.61, 0.87], [0.07, 0.79, 0.17, 0.31]),
            ([0.27, 0.63, 0.41, 0.57], [0.27, 0.63, 0.41, 0.57]),
        ] {
            for weight in [-0.25f32, 0.0, 0.13, 0.5, 0.83, 1.0, 1.25] {
                let quad = vertices(weight);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view,
                        },
                        None,
                        "Native font interpolation",
                    );
                    pass.clear(Some(UnormColor::BLACK), None, None);
                    pass.run(
                        RenderRequestBuilder::new()
                            .color_blend_type(ColorBlendType::Opaque)
                            .build(
                                RenderProgramWithArguments::Font {
                                    vertices: VertexSource::VertexData { vertices: &quad },
                                    glyph: glyph.as_source(),
                                    transform: crate::render::top_left_projection_matrix(),
                                    color1: FloatColor4::from_rgba(
                                        first[0], first[1], first[2], first[3],
                                    ),
                                    color2: FloatColor4::from_rgba(
                                        second[0], second[1], second[2], second[3],
                                    ),
                                },
                                DrawPrimitive::TrianglesStrip,
                            ),
                    );
                }
                let mut expected =
                    std::array::from_fn(|c| (second[c] - first[c]).mul_add(weight, first[c]));
                expected[3] *= coverage as f32 / 255.0;
                compare(
                    context,
                    expected,
                    &format!(
                        "Native font coverage{coverage} weight{weight} endpoints{first:?}/{second:?}"
                    ),
                );
            }
        }
    }
    let offsets = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    let distances = offsets.map(|(x, y)| glam::vec2(x as f32 / 3.0, y as f32 / 3.0));
    let tint = [0.31, 0.59, 0.83, 0.43];
    for coverage in [0u8, 31, 91, 173, 237, 255] {
        for selected in 0..8 {
            // Bright center must be excluded; independently make each of
            // the eight surrounding taps the unique maximum when nonzero.
            let mut data = [0u8; 9];
            data[4] = 255;
            let (x, y) = offsets[selected];
            data[((y + 1) * 3 + x + 1) as usize] = coverage;
            let glyph = GpuTexture::new_static_with_data(
                context.device,
                context.queue,
                Some("Native font outline taps"),
                winit::dpi::PhysicalSize::new(3, 3),
                wgpu::TextureFormat::R8Unorm,
                &data,
            );
            let quad = vertices(0.73);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view,
                    },
                    None,
                    "Native font outline",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .build(
                            RenderProgramWithArguments::FontBorder {
                                vertices: VertexSource::VertexData { vertices: &quad },
                                glyph: glyph.as_source(),
                                transform: crate::render::top_left_projection_matrix(),
                                distances,
                                color: FloatColor4::from_rgba(
                                    tint[0], tint[1], tint[2], tint[3],
                                ),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                );
            }
            let expected = [
                tint[0],
                tint[1],
                tint[2],
                (coverage as f32 / 255.0 * 1.5).min(1.0) * tint[3],
            ];
            compare(
                context,
                expected,
                &format!("Native font outline coverage{coverage} tap{selected}"),
            );
        }
    }
    eprintln!(
        "Native Font/FontBorder: {checks} GPU channel checks pass for fused RGBA interpolation, extrapolation, glyph alpha, all eight outline taps, excluded center and saturated1.5coverage"
    );
}

fn verify_native_texture_color_operations(
    context: &mut crate::render::PreRenderContext<'_, '_, '_, '_>,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
    operations: &[shin_render::LayerFragmentShader],
) {
    use shin_core::primitives::color::{FloatColor4, UnormColor};
    use shin_render::{
        ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
        RenderProgramWithArguments, RenderRequestBuilder,
        gpu_texture::GpuTexture,
        quad_vertices::build_quad_vertices,
        shaders::types::{
            buffer::VertexSource,
            texture::{TextureTarget, TextureTargetKind},
            vertices::{MaskVertex, PosTexVertex},
        },
    };
    let colors = [
        [23u8, 197, 83, 0],
        [211, 59, 137, 181],
        [157, 31, 229, 113],
        [67, 241, 19, 223],
    ];
    let mask_colors = [
        [31u8, 0, 0, 255],
        [91, 0, 0, 255],
        [173, 0, 0, 255],
        [237, 0, 0, 255],
    ];
    let texture = |name, data: &[[u8; 4]; 4]| {
        GpuTexture::new_static_with_data(
            context.device,
            context.queue,
            Some(name),
            winit::dpi::PhysicalSize::new(2, 2),
            wgpu::TextureFormat::Rgba8Unorm,
            &data.concat(),
        )
    };
    let image = texture("Native Negative texture", &colors);
    let mask = texture("Native Negative mask", &mask_colors);
    let tint = FloatColor4::from_rgba(0.73, 0.41, 0.83, 0.67);
    let tint_values = tint.into_array();
    let mut checks = 0;
    for &operation in operations {
        for param in [
            [0.5, 0.25, 0.75, 0.0],
            [0.13, 0.71, 0.37, 1.0],
            [0.31, 0.83, 0.59, 0.41],
            [1.2, 0.7, 0.2, 1.25],
        ] {
            for (index, sampled) in colors.iter().enumerate() {
                let uv =
                    |i| glam::vec2(0.25 + (i % 2) as f32 * 0.5, 0.25 + (i / 2) as f32 * 0.5);
                let position = |corner: glam::Vec2| {
                    glam::vec2(64.0 + corner.x * 128.0, 64.0 + corner.y * 128.0)
                };
                let vertices = build_quad_vertices(|corner| PosTexVertex {
                    position: position(corner),
                    texture_position: uv(index),
                });
                let mask_index = (index + 1) % 4;
                let mask_vertices = build_quad_vertices(|corner| MaskVertex {
                    position: position(corner),
                    texture_position: uv(index),
                    mask_position: uv(mask_index),
                });
                for mode in 0..4 {
                    {
                        let mut pass = context.begin_pass(
                            TextureTarget {
                                kind: TextureTargetKind::Screen,
                                view,
                            },
                            None,
                            "Native textured Negative",
                        );
                        pass.clear(Some(UnormColor::BLACK), None, None);
                        let program = if mode == 3 {
                            RenderProgramWithArguments::Mask {
                                vertices: VertexSource::VertexData {
                                    vertices: &mask_vertices,
                                },
                                texture: image.as_source(),
                                mask: mask.as_source(),
                                fragment_shader: operation,
                                transform: crate::render::top_left_projection_matrix(),
                                color_multiplier: tint,
                                fragment_shader_param: glam::Vec4::from_array(param),
                                minmax: glam::vec2(0.2, 0.9),
                            }
                        } else {
                            RenderProgramWithArguments::Layer {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                texture: image.as_source(),
                                fragment_shader: operation,
                                output_kind: [
                                    LayerShaderOutputKind::Layer,
                                    LayerShaderOutputKind::LayerPremultiply,
                                    LayerShaderOutputKind::LayerDiscard,
                                ][mode],
                                transform: crate::render::top_left_projection_matrix(),
                                color_multiplier: tint,
                                fragment_shader_param: glam::Vec4::from_array(param),
                            }
                        };
                        pass.run(
                            RenderRequestBuilder::new()
                                .color_blend_type(ColorBlendType::Opaque)
                                .build(program, DrawPrimitive::TrianglesStrip),
                        );
                    }
                    let pixels = crate::layer::user::readback_test_texture(context, target);
                    let mask_weight = if mode == 3 {
                        (0.9f32 - 0.2).mul_add(mask_colors[mask_index][0] as f32 / 255.0, 0.2)
                    } else {
                        1.0
                    };
                    let alpha = (sampled[3] as f32 / 255.0 * mask_weight) * tint_values[3];
                    for channel in 0..4 {
                        let expected = if mode == 2 && alpha < 0.00100000005 {
                            if channel == 3 { 255.0 } else { 0.0 }
                        } else if channel == 3 {
                            alpha * 255.0
                        } else {
                            let source =
                                sampled.map(|value| value as f32 / 255.0 * mask_weight);
                            let tinted =
                                std::array::from_fn::<_, 4, _>(|c| source[c] * tint_values[c]);
                            let color = tinted[channel];
                            let processed = match operation {
                                LayerFragmentShader::Mono => {
                                    let red_luma = tinted[0] * f32::from_bits(0x3e991687);
                                    let green_luma =
                                        tinted[1].mul_add(f32::from_bits(0x3f1645a2), red_luma);
                                    let luma = tinted[2]
                                        .mul_add(f32::from_bits(0x3de978d5), green_luma);
                                    let delta =
                                        source[channel].mul_add(-tint_values[channel], luma);
                                    delta.mul_add(param[3], color) * param[channel]
                                }
                                LayerFragmentShader::Fill => {
                                    let delta = source[channel]
                                        .mul_add(-tint_values[channel], param[channel]);
                                    delta.mul_add(param[3], color)
                                }
                                LayerFragmentShader::Negative => {
                                    let delta = (color * param[3]).mul_add(-2.0, param[3]);
                                    (color + delta) * param[channel]
                                }
                                LayerFragmentShader::Gamma => {
                                    (color.log2() * (1.0 / param[channel])).exp2()
                                }
                                _ => unreachable!(),
                            };
                            let result = if mode == 1 {
                                processed * alpha
                            } else {
                                processed
                            };
                            result.clamp(0.0, 1.0) * 255.0
                        };
                        for (x, y) in [(80, 80), (128, 128), (176, 176)] {
                            let actual = pixels[(y * 1920 + x) * 4 + channel] as f32;
                            assert!(
                                (actual - expected).abs() <= 1.2,
                                "Native {operation:?} texel {index}, mode {mode}, channel {channel}: {actual} vs {expected}, param={param:?}"
                            );
                            checks += 1;
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "Native texture color operations {operations:?}: {checks} GPU channel checks pass for RGB gains, fused arithmetic, extrapolation, tint, mask, premultiplication and transparent discard"
    );
}

fn verify_live_authored_skip_playback<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    scenario: &std::sync::Arc<Scenario>,
    scenario_id: i32,
) {
    use crate::{app::AppAction, layer::user::InteractiveLayerRef, update::UpdateContext};
    use shin_core::{primitives::update::FrameId, time::Ticks};
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    if updated {
        let fingerprint = scenario
            .raw()
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(
            fingerprint, 0x7a006c4e2505e117,
            "Updated live assertions require the exact supplied 1.03 scenario"
        );
        // Original 0x338377 and updated 0x338396 have the same 28-byte
        // credits wait boundary; the patch inserts 31 bytes before this point.
        assert_eq!(
            &scenario.raw()[0x33838a..0x3383a6],
            &[
                0x03, 0x01, 0x03, 0x82, 0x1c, 0xb2, 0xc3, 0x03, 0x01, 0x06, 0xb3, 0x02, 0xc4,
                0x03, 0x01, 0x01, 0x83, 0x00, 0x81, 0xe0, 0x8a, 0x00, 0x83, 0x00, 0x0a, 0x4f,
                0x69, 0x2d,
            ]
        );
    }
    let seconds: u64 = std::env::var("SHIN_REFERENCE_LIVE_SECONDS")
        .unwrap_or_else(|_| "300".into())
        .parse()
        .unwrap();
    let last_choice: i32 = std::env::var("SHIN_REFERENCE_LAST_CHOICE")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    assert!((0..=1).contains(&last_choice));
    let capture_directory =
        std::env::var_os("SHIN_REFERENCE_LIVE_FRAME_DIR").map(std::path::PathBuf::from);
    if let Some(directory) = &capture_directory {
        std::fs::create_dir_all(directory).unwrap();
    }
    let capture_step: u32 = std::env::var("SHIN_REFERENCE_LIVE_CAPTURE_STEP")
        .unwrap_or_else(|_| "1000".into())
        .parse()
        .unwrap();
    assert!(capture_step > 0, "Frame capture interval must be positive");
    let mut next_capture_save = capture_step;
    let mut authored_assets = assets.clone();
    authored_assets.scenario = scenario.clone();
    let mut live = crate::adv::Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        authored_assets,
        Scripter::new(scenario, scenario_id, 12345),
    );
    // Use the engine's supported unread-skip setting and normal held-skip
    // input. No commands, asset tasks, clocks or results are dummy-completed.
    live.vm_state.settings.v11_disallowskipunread = false;
    let mut frame = FrameId::default();
    let start_save = std::env::var("SHIN_REFERENCE_LIVE_START_SAVE")
        .ok()
        .map(|value| value.parse::<u32>().unwrap());
    if let Some(start_save) = start_save {
        // Prepare only the historical route on the CPU. The measured
        // segment below uses the normal saved-scene loader and actual ADV.
        assert_eq!(scenario_id, 2, "Ending regression is scoped to Episode 3");
        assert_eq!(start_save, 7603, "Restore the boundary before the credits");
        let mut script = Scripter::new(scenario, scenario_id, 12345);
        let mut state = VmState::for_scenario(scenario_id, 12345);
        state.settings.v11_disallowskipunread = false;
        let mut result = CommandResult::None;
        for _ in 0..250_000 {
            let current = script.run(result).unwrap();
            result = match current {
                RuntimeCommand::SGET(get) => {
                    let value = get.apply_state(&mut state);
                    get.token.finish(value)
                }
                current => {
                    assert!(!matches!(
                        current,
                        RuntimeCommand::QUIZ(_)
                            | RuntimeCommand::SELECT(_)
                            | RuntimeCommand::RESUME(_)
                    ));
                    command::apply_command_state_ref(&current, &mut state).unwrap();
                    if state.save_position >= start_save {
                        eprintln!(
                            "Ending checkpoint: pc{:?}, saved{}, {current:?}",
                            script.position(),
                            state.save_position
                        );
                        break;
                    }
                    current.execute_dummy().unwrap()
                }
            };
        }
        assert_eq!(state.save_position, start_save);
        let entry = state.snapshot_entry();
        let globals = SaveData {
            settings: state.settings.clone(),
            persist_data: state.persist.clone(),
            save_vectors: state.save_vectors.clone(),
            auto_save_slot: state.auto_save_slot.clone(),
            ..SaveData::default()
        };
        live.load_saved_scene(&globals, &entry, server.clone())
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while live.is_loading_saved_scene() {
            assert!(
                std::time::Instant::now() < deadline,
                "Ending scene restore exceeded deadline"
            );
            frame.advance();
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(1.0),
                    asset_server: server,
                    pre_render: context,
                },
                enum_map::EnumMap::default(),
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            live.saved_scene_load_error().is_none(),
            "Ending restore: {:?}",
            live.saved_scene_load_error()
        );
        assert_eq!(live.vm_state.save_position, start_save);
        assert!(
            live.scripter.position().0 < if updated { 0x338396 } else { 0x338377 },
            "Ending restore must precede the credits wait"
        );
        eprintln!(
            "Ending restored: pc{:?}, saved{}",
            live.scripter.position(),
            live.vm_state.save_position
        );
    }
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Continuous authored playback"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let begin = std::time::Instant::now();
    let mut logged = begin;
    let mut frames = 0u64;
    let mut decisions = std::collections::BTreeSet::new();
    let mut submitted = std::collections::BTreeSet::new();
    let mut credits_wait_checked = false;
    let write_progress =
        |live: &crate::adv::Adv, frames, decisions: &std::collections::BTreeSet<(u32, i32)>| {
            let command: String = format!("{:?}", live.current_command)
                .chars()
                .take(200)
                .collect();
            let movies: Vec<_> = (0..48)
                .filter_map(|bank| {
                    let layer = live
                        .adv_state
                        .plane_layer_group(live.vm_state.layers.current_plane)
                        .get_layer(shin_core::vm::command::types::LayerbankId::new(bank))?;
                    let crate::layer::user::UserLayer::Movie(movie) = &*layer else {
                        return None;
                    };
                    Some(format!("bank{bank}: {}", movie.reference_playback_status()))
                })
                .collect();
            let scope = if start_save.is_some() {
                "Episode 3 final part from restored save7603; historical prefix prepared on CPU, then actual ADV update/assets/render/audio/commands through EXIT with no dummy completions or checkpoint jumps in the measured segment. This does not prove native timing/framebuffer equivalence."
            } else {
                "Continuous actual ADV update/assets/render/audio/commands under normal held skip and simulated six-tick frames; no dummy completions or saved checkpoint jumps. This does not prove native timing/framebuffer equivalence."
            };
            let status = format!(
                "start_save={start_save:?}\nentry={scenario_id}\nlast_choice={last_choice}\nfinished={}\nframes={frames}\nwall_seconds={:.3}\npc=0x{:08x}\nsave_position={}\ncommand={command}\ninteractive_checkpoints={decisions:?}\naudio_device=the default audio output\nupdated_103={updated}\nscope={scope}\n",
                live.is_finished(),
                begin.elapsed().as_secs_f64(),
                live.scripter.position().0,
                live.vm_state.save_position
            );
            eprintln!(
                "Continuous authored entry{scenario_id}: frame{frames}, pc{:?}, saved{}, finished{}, {:.1}s, {command}",
                live.scripter.position(),
                live.vm_state.save_position,
                live.is_finished(),
                begin.elapsed().as_secs_f64()
            );
            if !movies.is_empty() {
                eprintln!("Live movies: {movies:?}");
            }
            let layer_wait = match &live.current_command {
                Some(command::ExecutingCommand::LAYERWAIT(wait)) => {
                    Some(wait.reference_wait_status(&live.adv_state))
                }
                _ => None,
            };
            if let Some(wait) = &layer_wait {
                eprintln!("Live layer wait: {wait}");
            }
            if let Ok(path) = std::env::var("SHIN_REFERENCE_LIVE_REPORT") {
                std::fs::write(
                    path,
                    format!("{status}movies={movies:?}\nlayer_wait={layer_wait:?}\n"),
                )
                .unwrap();
            }
        };
    while !live.is_finished() && begin.elapsed().as_secs() < seconds {
        let pc = live.scripter.position().0;
        let mut input = enum_map::EnumMap::<AppAction, shin_input::ActionState>::default();
        let mut action = Some(AppAction::Enter);
        let mut pointer = None;
        if let Some(layer) = live.active_interactive_layer() {
            action = match layer {
                InteractiveLayerRef::Quiz(_) => {
                    decisions.insert((pc, 0));
                    Some(AppAction::Enter)
                }
                InteractiveLayerRef::Cake(cake) => {
                    decisions.insert((pc, 1));
                    let finger = cake.reference_feedback().0;
                    Some(if finger.x > 852.0 {
                        AppAction::DpadLeft
                    } else if finger.x < 844.0 {
                        AppAction::DpadRight
                    } else if finger.y > 704.0 {
                        AppAction::DpadUp
                    } else if finger.y < 696.0 {
                        AppAction::DpadDown
                    } else {
                        AppAction::Enter
                    })
                }
                InteractiveLayerRef::Murderer(murderer) => {
                    use crate::layer::user::murderer::model::{
                        MurdererMenu, SUBMIT_ENTRY, accusation_rect,
                    };
                    decisions.insert((pc, 2));
                    match murderer.reference_menu_state() {
                        (_, true) => None,
                        (MurdererMenu::Top, false) => {
                            Some(if murderer.reference_controller_state().2 == 5 {
                                AppAction::Enter
                            } else {
                                AppAction::Up
                            })
                        }
                        (MurdererMenu::Accusation, false) if !submitted.contains(&pc) => {
                            let suspect = (34..37).find(|&entry| {
                                !murderer.reference_accusation_selection(entry).1
                            });
                            let entry = suspect.unwrap_or(SUBMIT_ENTRY);
                            let rect = accusation_rect(entry).unwrap();
                            pointer =
                                Some(glam::vec2(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5));
                            if suspect.is_none() {
                                submitted.insert(pc);
                            }
                            None
                        }
                        _ => None,
                    }
                }
                InteractiveLayerRef::Last(last) => {
                    decisions.insert((pc, 3));
                    if !last.reference_accepts_input() {
                        None
                    } else if last.reference_controller_state().2 < 0 {
                        Some(if last_choice == 0 {
                            AppAction::Left
                        } else {
                            AppAction::Right
                        })
                    } else {
                        Some(AppAction::Enter)
                    }
                }
            };
        } else if let Some(menu) = &live.adv_state.choice_menu {
            action = Some(if menu.model.selected.is_none() {
                AppAction::AnyDown
            } else {
                AppAction::Enter
            });
        } else if live.adv_state.character_screen.is_some() {
            action = Some(AppAction::Cancel);
        } else {
            input[AppAction::HoldSkip].is_held = true;
        }
        if let Some(action) = action {
            input[action].is_held = true;
            input[action].is_clicked = true;
            input[action].is_clicked_or_repeated = true;
        }
        if let Some(point) = pointer {
            live.handle_pointer_state(point, true, true, begin.elapsed().as_secs_f64());
            live.handle_pointer_state(point, false, false, begin.elapsed().as_secs_f64());
        }
        frame.advance();
        frames += 1;
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(6.0),
                    asset_server: server,
                    pre_render: context,
                },
                input,
            );
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Continuous authored playback",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                live.render(&mut pass);
            }
            let mut upload = context.device.create_command_encoder(&Default::default());
            context.dynamic_buffer.finish(&mut upload);
            let encoder = std::mem::replace(
                context.encoder,
                context.device.create_command_encoder(&Default::default()),
            );
            context.queue.submit([upload.finish(), encoder.finish()]);
            context
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("GPU work must complete before readback or retirement");
        }));
        if let Err(error) = step {
            // Preserve the exact live failure position, rather than only
            // the last five-second progress sample. Do not recover/skip it.
            write_progress(&live, frames, &decisions);
            std::panic::resume_unwind(error);
        }
        if start_save.is_some()
            && !credits_wait_checked
            && live.scripter.position().0 == if updated { 0x338396 } else { 0x338377 }
        {
            let Some(command::ExecutingCommand::LAYERWAIT(wait)) = &live.current_command else {
                panic!("Credits must execute the actual LAYERWAIT");
            };
            let status = wait.reference_wait_status(&live.adv_state);
            assert_eq!(
                status.matches("/bank").count(),
                1,
                "Credits must target its layer once: {status}"
            );
            assert_eq!(
                status.matches("Linear").count(),
                1,
                "Credits must queue one scroll: {status}"
            );
            assert_eq!(
                status.matches("SineOut").count(),
                1,
                "Credits must queue one final motion: {status}"
            );
            eprintln!("Verified single-target credits tween queue: {status}");
            credits_wait_checked = true;
        }
        if let Some(directory) = &capture_directory {
            // Held skip can finish MSGSET/MSGWAIT in the same update.
            // Capture the displayed message even after its token resumes
            // the script; keep the actual transition/loading frame intact.
            if live.vm_state.save_position >= next_capture_save
                && live.vm_state.messagebox_state.messagebox_shown
                && live.vm_state.messagebox_state.text.is_some()
            {
                capture_authored_frame(context, &target, directory, &live, scenario_id, frames);
                next_capture_save =
                    (live.vm_state.save_position / capture_step + 1) * capture_step;
            }
        }
        if logged.elapsed().as_secs() >= 5 {
            write_progress(&live, frames, &decisions);
            logged = std::time::Instant::now();
        }
    }
    write_progress(&live, frames, &decisions);
    assert!(
        live.is_finished(),
        "Continuous authored playback reached its time budget; inspect the real pending command/report instead of claiming EXIT"
    );
    if start_save.is_some() {
        assert!(
            credits_wait_checked,
            "Ending regression must observe the actual credits scroll"
        );
    }
    if scenario_id == 7 {
        assert_eq!(decisions.len(), 19);
        let expected_exit = match (updated, last_choice) {
            (false, 0) => 0x867444,
            (false, 1) => 0x86b4dc,
            (true, 0) => 0x8674a0,
            (true, 1) => 0x86b538,
            _ => unreachable!(),
        };
        assert_eq!(live.scripter.position().0, expected_exit);
    } else {
        // Recorded actual original-script EXIT PCs from the authored CPU
        // entry sweep (default global values and first displayed choices).
        // EXIT itself must now be reached by the actual ADV pipeline.
        const REFERENCE_EXIT_PCS: [u32; 48] = [
            0x001598d1, 0x00232f38, 0x003383cf, 0x00455dca, 0x00552cd4, 0x00670c48, 0x0072c690,
            0x00000000, 0x001639cb, 0x00239595, 0x0033ba80, 0x004700f0, 0x00556957, 0x00676698,
            0x00761e6d, 0x0086edd0, 0x00165a1b, 0x0023d7f6, 0x00341b89, 0x00473caf, 0x00571a1b,
            0x0067ad4f, 0x00764081, 0x008780a9, 0x00879f80, 0x0088094e, 0x0088e32a, 0x00891eae,
            0x00897ce8, 0x0089be3d, 0x008a372b, 0x008ade21, 0x008b87c0, 0x008c66c1, 0x008d1606,
            0x008d31dd, 0x008d84d6, 0x008db39f, 0x008e193e, 0x0093092b, 0x00931fa6, 0x00933bff,
            0x0093df6c, 0x008e22bf, 0x008ebc40, 0x008f189a, 0x009064d5, 0x0092c2af,
        ];
        // Independently recorded patched CPU sweep, guarded by full scenario fingerprint above.
        const UPDATED_REFERENCE_EXIT_PCS: [u32; 48] = [
            0x001598e4, 0x00232f4c, 0x003383ee, 0x00455de9, 0x00552cfb, 0x00670c7d, 0x0072c6ca,
            0x00000000, 0x001639de, 0x002395a9, 0x0033ba9f, 0x0047010d, 0x0055697e, 0x006766ce,
            0x00761eab, 0x0086ee2c, 0x00165a2e, 0x0023d80a, 0x00341ba8, 0x00473ccc, 0x00571a42,
            0x0067ad85, 0x007640bf, 0x00878105, 0x00879fdc, 0x008809aa, 0x0088e386, 0x00891f0a,
            0x00897d44, 0x0089be99, 0x008a3787, 0x008ade7d, 0x008b881c, 0x008c671e, 0x008d1663,
            0x008d323a, 0x008d8535, 0x008db3fe, 0x008e199d, 0x00930992, 0x0093200d, 0x00933c66,
            0x0093dfd4, 0x008e231e, 0x008ebca1, 0x008f18fc, 0x00906539, 0x0092c316,
        ];
        let entry = usize::try_from(scenario_id).unwrap();
        let expected = if updated {
            UPDATED_REFERENCE_EXIT_PCS[entry]
        } else {
            REFERENCE_EXIT_PCS[entry]
        };
        assert_eq!(
            live.scripter.position().0,
            expected,
            "Actual playback must reach the recorded authored entry EXIT"
        );
    }
}

fn verify_live_root_back_selection<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    audio: std::sync::Arc<shin_audio::AudioManager>,
) {
    use crate::{
        layer::{DrawableLayer, user::TileLayer},
        update::{Updatable, UpdateContext},
    };
    use shin_core::{
        primitives::{color::FloatColor4, update::FrameId},
        time::Ticks,
        vm::command::types::{LayerProperty, LayerbankId, PlaneId},
    };
    let mut live = crate::adv::Adv::new(
        audio,
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let plane = PlaneId::new(0);
    let bank = LayerbankId::new(47);
    live.adv_state.plane_layer_group_mut(plane).add_layer(
        bank,
        TileLayer::new(
            FloatColor4::BLUE,
            glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
        )
        .into(),
    );
    for (property, value) in [
        (LayerProperty::WobbleXMode, 4.0),
        (LayerProperty::WobbleXPeriod, 10.0),
        (LayerProperty::WobbleXAmplitude, 100.0),
    ] {
        live.adv_state
            .root_layer_group
            .properties_mut()
            .property_tweener_mut(property)
            .fast_forward_to(value);
    }
    let mut frame = FrameId::default();
    frame.advance();
    Updatable::update(
        &mut live.adv_state,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: context,
        },
    );
    live.adv_state
        .create_back_layer_group_if_needed(&mut context.render_clone_ctx());
    let back_identity = live
        .adv_state
        .back_layer_group
        .as_ref()
        .unwrap()
        .properties()
        .identity();
    live.adv_state
        .create_back_layer_group_if_needed(&mut context.render_clone_ctx());
    assert_eq!(
        live.adv_state
            .back_layer_group
            .as_ref()
            .unwrap()
            .properties()
            .identity(),
        back_identity,
        "A later loader must retain the first back root"
    );
    live.adv_state.plane_layer_group_mut(plane).add_layer(
        bank,
        TileLayer::new(FloatColor4::RED, glam::vec4(-960.0, -540.0, 1920.0, 1080.0)).into(),
    );
    live.adv_state.allow_running_animations = false;
    let before = live
        .adv_state
        .root_layer_group
        .properties()
        .get_transform_params()
        .compute_final_transform();
    frame.advance();
    Updatable::update(
        &mut live.adv_state,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        },
    );
    let front = live
        .adv_state
        .root_layer_group
        .properties()
        .get_transform_params()
        .compute_final_transform();
    let back = live
        .adv_state
        .back_layer_group
        .as_ref()
        .unwrap()
        .properties()
        .get_transform_params()
        .compute_final_transform();
    assert_ne!(
        front, before,
        "Native wobble advances during a load animation pause"
    );
    assert_eq!(
        back, front,
        "Both independent root property clocks must update"
    );
    let pixels = snapshot_pixels(context, &live, true);
    assert_eq!(
        &pixels[(540 * 1920 + 960) * 4..(540 * 1920 + 960) * 4 + 4],
        &[0, 0, 255, 255],
        "Retained back scene must render while the front is being replaced"
    );
    live.adv_state.acquire_audio_pause();
    live.adv_state.allow_running_animations = true;
    frame.advance();
    Updatable::update(
        &mut live.adv_state,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        },
    );
    assert!(
        live.adv_state.back_layer_group.is_some(),
        "Audio/modal pause retains the back root"
    );
    live.adv_state.release_audio_pause();
    frame.advance();
    Updatable::update(
        &mut live.adv_state,
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        },
    );
    assert!(live.adv_state.back_layer_group.is_none());
    let pixels = snapshot_pixels(context, &live, true);
    assert_eq!(
        &pixels[(540 * 1920 + 960) * 4..(540 * 1920 + 960) * 4 + 4],
        &[255, 0, 0, 255],
        "Resumed animation releases the back root and reveals the front"
    );
    eprintln!(
        "Native back root: first snapshot retained, independent front/back wobble updates, blue back rendered during load, audio pause retains it, resumed animation reveals red front"
    );
}

pub(super) fn verify_live_scene_load_fade<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    frame: shin_core::primitives::update::FrameId,
    kind: super::super::SceneLoadKind,
) {
    use crate::{adv::Adv, app::AppAction, layer::DrawableLayer, update::UpdateContext};
    use shin_core::{
        primitives::color::FloatColor4,
        time::Ticks,
        vm::command::types::{LayerProperty, LayerbankId, PlaneId},
    };
    let mut assets = assets.clone();
    assets.scenario = std::sync::Arc::new(resume_scenario());
    let scenario = assets.scenario.clone();
    let mut live = Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets,
        Scripter::new(&scenario, 0, 42),
    );
    live.adv_state
        .plane_layer_group_mut(PlaneId::new(0))
        .add_layer(
            LayerbankId::new(0),
            crate::layer::user::TileLayer::new(
                FloatColor4::from_rgba(1.0, 0.0, 0.0, 1.0),
                glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
            )
            .into(),
        );
    let mut state = VmState::for_scenario(0, 42);
    state.save_position = 99;
    live.queue_reconstructed_scene(
        Box::new(ReconstructedScene {
            scripter: Scripter::new(&scenario, 0, 42),
            state,
            stop: ReconstructionStop::Response(CommandResult::None),
        }),
        server.clone(),
        Some(7),
    );
    live.pending_restore.as_mut().unwrap().kind = kind;
    // Capture on the first loader update, before advancing the old scene.
    live.update_saved_scene_load(&mut UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::ZERO,
        asset_server: server,
        pre_render: context,
    });
    if live.pending_restore.is_some() {
        live.adv_state
            .root_layer_group_mut()
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(1920.0);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live.pending_restore.is_some() {
        live.update_saved_scene_load(&mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            pre_render: context,
        });
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert!(live.is_loading_saved_scene());
    let installed_position = live.scripter.position();
    let mask = (kind == super::super::SceneLoadKind::Saved).then(|| {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(StatelessFile::new(std::fs::File::open(
            std::env::var_os("SHIN_REFERENCE_ROM").unwrap(),
        ).unwrap()).unwrap()).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/default.msk").unwrap()).unwrap()
            .read_to_end(&mut bytes).unwrap();
        shin_core::format::mask::read_mask(&bytes).unwrap().texels
    });
    let duration = if kind == super::super::SceneLoadKind::History {
        4.0
    } else {
        30.0
    };
    for (phase, expected) in [(0, 255u8), (1, 191), (2, 128), (3, 64), (4, 0)] {
        if phase != 0 {
            let mut input = enum_map::EnumMap::default();
            input[AppAction::Act] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                is_held: true,
                ..Default::default()
            };
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(duration / 4.0),
                    asset_server: server,
                    pre_render: context,
                },
                input,
            );
        }
        let pixels = snapshot_pixels(context, &live, true);
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            let expected = if let Some(mask) = &mask {
                let x = (index % 1920) as f32 + 0.5;
                let y = (index / 1920) as f32 + 0.5;
                let sx = x / 1920.0 * mask.width() as f32 - 0.5;
                let sy = y / 1080.0 * mask.height() as f32 - 0.5;
                let at = |x: i32, y: i32| f32::from(mask.get_pixel(
                    x.clamp(0, mask.width() as i32 - 1) as u32,
                    y.clamp(0, mask.height() as i32 - 1) as u32,
                ).0[0]) / 255.0;
                let tx = sx - sx.floor();
                let ty = sy - sy.floor();
                let ix = sx.floor() as i32;
                let iy = sy.floor() as i32;
                let a = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
                let b = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
                let texel = a * (1.0 - ty) + b * ty;
                ((1.0 - phase as f32 * 0.25 * (1.0 + 1.0 / 0.3) + texel / 0.3)
                    .clamp(0.0, 1.0) * 255.0).round() as u8
            } else { expected };
            assert!(
                pixel[0].abs_diff(expected) <= 1,
                "{kind:?} fade phase{phase}: {pixel:?}"
            );
            assert_eq!(&pixel[1..3], &[0, 0]);
        }
        if mask.is_some() && phase == 2 {
            assert_eq!(pixels[(540 * 1920 + 100) * 4], 0, "Left side reveals first");
            assert_eq!(pixels[(540 * 1920 + 1820) * 4], 255, "Right side still covered");
            if let Some(output) = std::env::var_os("SHIN_LOAD_FEEDBACK_OUTPUT") {
                std::fs::write(std::path::Path::new(&output).join("saved-load-halfway.rgba"), &pixels).unwrap();
            }
        }
        assert_eq!(live.vm_state.save_position, 99);
        assert_eq!(live.scripter.position(), installed_position);
        assert_eq!(live.is_loading_saved_scene(), phase < 4);
    }
    assert_eq!(live.previous_rollback_position(), Some(7));
    eprintln!(
        "Native {kind:?} load fade: mask={}, frozen source,2073600 pixels at five {duration}tick phases, held-input/script gate and expiry pass", mask.is_some()
    );
}

fn verify_live_pending_saved_scene<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    original_scenario: &Scenario,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::{adv::Adv, app::AppAction, layer::DrawableLayer, update::UpdateContext};
    use shin_core::{
        time::{Ticks, Tween},
        vm::command::types::{LayerProperty, Pan, Volume},
    };
    let effect_path = original_scenario
        .info_tables()
        .se_info(shin_core::format::scenario::instruction_elements::FromNumber::from_number(0))
        .path()
        .to_owned();
    let mut assets = assets.clone();
    assets.scenario = std::sync::Arc::new(resume_scenario());
    let scenario = assets.scenario.clone();
    let audio = std::sync::Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        assets,
        Scripter::new(&scenario, 0, 42),
    );
    live.adv_state.bgm_title.show(server, "Pending load music");
    live.adv_state
        .route_label
        .show(server, "Pending load chapter");
    live.adv_state
        .notifications
        .enqueue(super::super::information::NotificationKind::Characters, 1);
    live.adv_state
        .root_layer_group_mut()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .enqueue_now(600.0, Tween::linear(Ticks::from_f32(60.0)));
    live.adv_state.audio.se_player.request(
        server.clone(),
        effect_path,
        0,
        shin_core::format::scenario::instruction_elements::FromNumber::from_number(0),
        true,
        Volume::default(),
        Pan::default(),
        1.0,
        Tween::IMMEDIATE,
    );
    let blockers = {
        let mut manager = audio.kira_manager().lock();
        let available = manager.sub_track_capacity() - manager.num_sub_tracks();
        (0..available)
            .map(|_| {
                manager
                    .add_sub_track(kira::track::TrackBuilder::new())
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let (release, gate) = futures::channel::oneshot::channel::<()>();
    let mut state = VmState::for_scenario(0, 42);
    state.save_position = 7;
    let scene = Box::new(ReconstructedScene {
        scripter: Scripter::new(&scenario, 0, 42),
        state,
        stop: ReconstructionStop::Response(CommandResult::None),
    });
    let worker_server = server.clone();
    live.pending_restore = Some(super::super::PendingSceneRestore {
        task: Some(shin_tasks::async_io::spawn(async move {
            gate.await.unwrap();
            let loaded = super::super::restoration::LoadedScene::load(
                &worker_server,
                &audio,
                &scenario,
                &scene.state,
            )
            .await?;
            Ok((loaded, scene))
        })),
        ready: None,
        retirement: None,
        kind: super::super::SceneLoadKind::Saved,
        source: None,
        present_source: false,
    });
    let source_position = live.scripter.position();
    let source_route = live.vm_state.route.bit_position();
    let mut step_frame = frame;
    let mut step = |live: &mut Adv,
                    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
                    ticks: f32| {
        let mut input = enum_map::EnumMap::default();
        input[AppAction::Act] = shin_input::ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            is_held: true,
            ..Default::default()
        };
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: step_frame,
                delta_ticks: Ticks::from_f32(ticks),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        step_frame.advance();
    };
    step(&mut live, context, 10.0);
    assert!(live.pending_restore.as_ref().unwrap().ready.is_none());
    assert_eq!(live.adv_state.bgm_title.reference_motion().2, 10.0);
    assert!((live.adv_state.route_label.reference_motion().0 - 0.3).abs() < 0.00001);
    assert!((live.adv_state.notifications.reference_motion().3 - 1.0 / 3.0).abs() < 0.00001);
    assert!(
        (live
            .adv_state
            .root_layer_group()
            .properties()
            .get_value(LayerProperty::TranslateX)
            - 100.0)
            .abs()
            < 0.0001
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live
        .adv_state
        .audio
        .se_player
        .get_wait_status(0)
        .contains(shin_core::vm::command::types::AudioWaitStatus::LOADING)
    {
        step(&mut live, context, 0.0);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        live.adv_state
            .audio
            .se_player
            .get_wait_status(0)
            .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING)
    );
    release.send(()).unwrap();
    while live.pending_restore.as_ref().unwrap().ready.is_none() {
        step(&mut live, context, 0.0);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    step(&mut live, context, 4.0);
    assert!(live.is_loading_saved_scene());
    assert_eq!(live.adv_state.bgm_title.reference_motion().2, 14.0);
    assert!((live.adv_state.route_label.reference_motion().0 - 0.42).abs() < 0.00001);
    assert!((live.adv_state.notifications.reference_motion().3 - 14.0 / 30.0).abs() < 0.00001);
    assert!(
        (live
            .adv_state
            .root_layer_group()
            .properties()
            .get_value(LayerProperty::TranslateX)
            - 140.0)
            .abs()
            < 0.0001
    );
    assert_eq!(live.scripter.position(), source_position);
    assert_eq!(live.vm_state.route.bit_position(), source_route);
    assert_eq!(live.vm_state.save_position, 0);
    drop(blockers);
    while live.is_loading_saved_scene() {
        step(&mut live, context, 1.0);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.vm_state.save_position, 7);
    eprintln!(
        "Pending ordinary save: gated IO and saturated audio capacity advance layers, labels, notifications and pending SE; held advance leaves script/route unchanged; successful installation pass"
    );
}

fn verify_live_restored_audio<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    scenario: &std::sync::Arc<Scenario>,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::{adv::Adv, update::UpdateContext};
    use shin_core::vm::IntoRuntimeForm;
    use shin_core::{
        format::scenario::{
            info::{BgmId, SeId},
            instruction_elements::{FromNumber, U8Bool},
        },
        time::{Ticks, Tween},
        vm::command::types::{AudioWaitStatus, Pan, Volume},
    };
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let mut live = Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets,
        Scripter::new(scenario, 0, 42),
    );
    let (index, bgm_info) = scenario
        .info_tables()
        .bgm_info
        .iter()
        .enumerate()
        .find(|(_, info)| info.linked_bgm_id.repr().is_some())
        .unwrap();
    let bgm_id = BgmId::from_number(index as i32);
    let se_id = SeId::from_number(0);
    let se_path = scenario.info_tables().se_info(se_id).path();
    let bgm: std::sync::Arc<shin_audio::AudioFile> = server.load_sync(bgm_info.path()).unwrap();
    let effect: std::sync::Arc<shin_audio::AudioFile> = server.load_sync(&se_path).unwrap();
    let se_threshold =
        (effect.info().num_samples as u64 * 1000 / u64::from(effect.info().sample_rate) / 4)
            .clamp(1, 30) as u32;
    live.adv_state
        .audio
        .bgm_player
        .play(bgm_id, bgm, "", true, Volume(0.8), Tween::IMMEDIATE);
    live.adv_state.audio.se_player.play(
        0,
        se_id,
        effect.clone(),
        true,
        Volume(0.7),
        Pan(0.0),
        1.0,
        Tween::IMMEDIATE,
    );
    live.adv_state.audio.se_player.play(
        1,
        se_id,
        effect,
        false,
        Volume::default(),
        Pan::default(),
        1.0,
        Tween::IMMEDIATE,
    );
    live.adv_state
        .audio
        .se_player
        .reference_pause_source(1, true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while live.adv_state.audio.bgm_player.reference_source().1 < 30
        || live.adv_state.audio.se_player.reference_source(0).1 < se_threshold
    {
        assert!(
            std::time::Instant::now() < deadline,
            "reference audio did not advance"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    live.adv_state.audio.bgm_player.reference_pause_source(true);
    live.adv_state
        .audio
        .se_player
        .reference_pause_source(0, true);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let music_before = live.adv_state.audio.bgm_player.reference_source();
    let effect_before = live.adv_state.audio.se_player.reference_source(0);
    let once = live.adv_state.audio.se_player.request_once(
        server.clone(),
        se_path.clone(),
        se_id,
        Volume::default(),
        Pan::default(),
    );
    while live
        .adv_state
        .audio
        .se_player
        .get_once_wait_status(once)
        .contains(AudioWaitStatus::LOADING)
    {
        live.adv_state.audio.se_player.update();
        live.adv_state
            .audio
            .se_player
            .reference_pause_once(once, true);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        live.adv_state
            .audio
            .se_player
            .get_once_wait_status(once)
            .contains(AudioWaitStatus::PLAYING)
    );
    let mut state = VmState::for_scenario(0, 42);
    state.audio.bgm = Some(super::super::vm_state::audio::BgmState {
        bgm_id,
        volume: Volume(0.3),
    });
    state.audio.se[0] = Some(super::super::vm_state::audio::SeState {
        se_id,
        volume: Volume(0.4),
        pan: Pan(0.5),
        play_speed: 1.5,
    });
    let finish =
        |live: &mut Adv, context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
            while live.pending_restore.is_some() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "audio restoration timed out"
                );
                live.update_saved_scene_load(&mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    pre_render: context,
                });
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
    live.queue_reconstructed_scene(
        Box::new(ReconstructedScene {
            scripter: Scripter::new(scenario, 0, 42),
            state,
            stop: ReconstructionStop::Response(CommandResult::None),
        }),
        server.clone(),
        None,
    );
    finish(&mut live, context);
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(
        live.adv_state.audio.bgm_player.reference_source(),
        music_before,
        "same BGM must retain source position and routing"
    );
    assert_eq!(
        live.adv_state.audio.se_player.reference_source(0),
        effect_before,
        "same repeat must retain source position and routing"
    );
    assert!(live.adv_state.audio.se_player.get_wait_status(1).is_empty());
    assert!(
        live.adv_state
            .audio
            .se_player
            .get_once_wait_status(once)
            .contains(AudioWaitStatus::PLAYING)
    );
    assert_eq!(live.vm_state.audio.bgm.unwrap().volume, Volume(0.3));
    assert_eq!(live.vm_state.audio.se[0].unwrap().play_speed, 1.5);
    // A failed command-start candidate must not clear live audio, even
    // when its reconstructed state requests silence.
    let rejected = C::WAIT(c::WAIT {
        wait_amount: NumberSpec::new(NumberSpec::constant(1).into_untyped()),
        allow_interrupt: U8Bool(false),
    })
    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
    live.queue_reconstructed_scene(
        Box::new(ReconstructedScene {
            scripter: Scripter::new(scenario, 0, 42),
            state: VmState::for_scenario(0, 42),
            stop: ReconstructionStop::Command(rejected),
        }),
        server.clone(),
        None,
    );
    finish(&mut live, context);
    assert!(
        live.saved_scene_load_error()
            .unwrap()
            .contains("Unsupported reconstruction stop")
    );
    assert_eq!(
        live.adv_state.audio.bgm_player.reference_source(),
        music_before
    );
    assert_eq!(
        live.adv_state.audio.se_player.reference_source(0),
        effect_before
    );
    assert!(
        live.adv_state
            .audio
            .se_player
            .get_once_wait_status(once)
            .contains(AudioWaitStatus::PLAYING)
    );
    live.queue_reconstructed_scene(
        Box::new(ReconstructedScene {
            scripter: Scripter::new(scenario, 0, 42),
            state: VmState::for_scenario(0, 42),
            stop: ReconstructionStop::Response(CommandResult::None),
        }),
        server.clone(),
        None,
    );
    finish(&mut live, context);
    assert!(live.saved_scene_load_error().is_none());
    assert!(live.adv_state.audio.bgm_player.get_wait_status().is_empty());
    assert!(live.adv_state.audio.se_player.get_wait_status(0).is_empty());
    assert!(
        live.adv_state
            .audio
            .se_player
            .get_once_wait_status(once)
            .contains(AudioWaitStatus::PLAYING),
        "negative slots are outside the restore loop"
    );
    live.adv_state.audio.se_player.stop_all(Tween::IMMEDIATE);
    eprintln!(
        "Restored native audio: exact BGM/repeat source positions and routing, absent positive slot removal, negative one-shot continuity, command-start failure atomicity and explicit silence restore pass"
    );
}

fn verify_live_saved_label_exit<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::{adv::Adv, update::UpdateContext};
    use shin_core::time::Ticks;
    let mut assets = assets.clone();
    assets.scenario = std::sync::Arc::new(resume_scenario());
    let mut live = Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 7, 42),
    );
    let blank = resume_presentation_pixels(context, &live);
    live.adv_state.bgm_title.show(server, "ab-sinthe");
    live.adv_state
        .route_label
        .show(server, "10/4（土）8:00 新島空港到着");
    live.adv_state.bgm_title.update(Ticks::from_f32(100.0));
    live.adv_state.route_label.update(Ticks::from_f32(100.0));
    live.adv_state
        .notifications
        .enqueue(super::super::information::NotificationKind::Characters, 1);
    assert_eq!(
        live.adv_state.notifications.update(Ticks::from_f32(100.0)),
        Some(0)
    );
    live.adv_state.notifications.load_textures(server);
    live.adv_state
        .notifications
        .enqueue(super::super::information::NotificationKind::Tips, 2);
    live.adv_state.play_system_sound(
        server,
        "notify1",
        shin_core::vm::command::types::Volume(0.5),
    );
    live.adv_state.audio.system_player.sounds[0].set_paused(true);
    let cue_track = live.adv_state.audio.system_player.track.id();
    let bank = live.adv_state.audio.system_player.bank.clone().unwrap();
    let music_before = live.adv_state.bgm_title.reference_motion();
    let chapter_before = live.adv_state.route_label.reference_motion();
    let before = resume_presentation_pixels(context, &live);
    assert_ne!(
        before, blank,
        "actual supplied-font labels must be visible before loading"
    );
    let globals = SaveData::default();
    live.load_saved_scene(&globals, &resume_source(), server.clone())
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while live.pending_restore.is_some() {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            enum_map::EnumMap::default(),
        );
        assert!(
            std::time::Instant::now() < deadline,
            "saved label handoff timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert!(live.adv_state.audio.system_player.sounds.is_empty());
    assert_eq!(live.adv_state.audio.system_player.retired.len(), 1);
    assert_eq!(
        live.adv_state.audio.system_player.retired[0]._track.id(),
        cue_track
    );
    assert!(std::sync::Arc::ptr_eq(
        live.adv_state.audio.system_player.bank.as_ref().unwrap(),
        &bank
    ));
    assert!(
        live.adv_state.audio.system_player.retired[0].sounds[0]
            .get_wait_status()
            .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING)
    );
    assert_eq!(live.adv_state.bgm_title.reference_motion(), music_before);
    assert_eq!(
        live.adv_state.route_label.reference_motion(),
        chapter_before
    );
    assert_eq!(
        live.adv_state.notifications.reference_motion(),
        (Some(0), Vec::new(), 1.0, 1.0, false)
    );
    assert_eq!(
        resume_presentation_pixels(context, &live),
        before,
        "load handoff must retain all2073600 label pixels at zero additional delta"
    );
    live.adv_state.update_presentation(&mut UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::from_f32(5.0),
        asset_server: server,
        pre_render: context,
    });
    assert_eq!(live.adv_state.notifications.reference_motion().3, 0.5);
    assert!(live.adv_state.notifications.reference_motion().1.is_empty());
    assert_ne!(resume_presentation_pixels(context, &live), before);
    live.adv_state.update_presentation(&mut UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::from_f32(5.0),
        asset_server: server,
        pre_render: context,
    });
    assert_eq!(
        live.adv_state.notifications.reference_motion(),
        (Some(0), Vec::new(), 1.0, 0.0, true)
    );
    let music = live.adv_state.bgm_title.reference_motion();
    let chapter = live.adv_state.route_label.reference_motion();
    assert!((music.0 - (music_before.0 - 20.0)).abs() < 0.0001);
    assert!((chapter.0 - 0.7).abs() < 0.0001);
    assert_ne!(resume_presentation_pixels(context, &live), before);
    live.adv_state.update_presentation(&mut UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::from_f32(24.0),
        asset_server: server,
        pre_render: context,
    });
    assert!(live.adv_state.bgm_title.reference_motion().3);
    assert!(live.adv_state.route_label.reference_motion().2);
    assert_eq!(resume_presentation_pixels(context, &live), blank);
    // A label started by the reconstructed boundary supersedes its old
    // counterpart; handoff must not cancel or overwrite that new motion.
    let mut music = super::super::bgm_title::BgmTitle::new();
    music.show(server, "new music");
    music.update(Ticks::from_f32(10.0));
    let motion = music.reference_motion();
    music.restore_previous_for_exit(&mut live.adv_state.bgm_title);
    assert_eq!(music.reference_motion(), motion);
    let mut chapter = super::super::route_label::RouteLabel::new();
    chapter.show(server, "new chapter");
    chapter.update(Ticks::from_f32(10.0));
    let motion = chapter.reference_motion();
    chapter.restore_previous_for_exit(&mut live.adv_state.route_label);
    assert_eq!(chapter.reference_motion(), motion);
    // Repeated loads keep the old global cue bus. Saturate the actual
    // manager to hold the loaded result and verify EOF cleanup still
    // runs while scene installation remains blocked on audio capacity.
    while live
        .adv_state
        .audio_manager
        .kira_manager()
        .lock()
        .num_sub_tracks()
        > 38
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let held_tracks = {
        let mut manager = live.adv_state.audio_manager.kira_manager().lock();
        let available = manager.sub_track_capacity() - manager.num_sub_tracks();
        (0..available)
            .map(|_| {
                manager
                    .add_sub_track(kira::track::TrackBuilder::new())
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    live.load_saved_scene(&globals, &resume_source(), server.clone())
        .unwrap();
    while live.pending_restore.as_ref().unwrap().ready.is_none() {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            enum_map::EnumMap::default(),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.is_loading_saved_scene());
    assert_eq!(live.adv_state.audio.system_player.retired.len(), 1);
    assert_eq!(
        live.adv_state.audio.system_player.retired[0]._track.id(),
        cue_track
    );
    live.adv_state.audio.system_player.retired[0].sounds[0].set_paused(false);
    while !live.adv_state.audio.system_player.retired.is_empty() {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            enum_map::EnumMap::default(),
        );
        assert!(
            live.is_loading_saved_scene(),
            "cue cleanup must not bypass capacity ownership"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "retired cue never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(held_tracks);
    while live.pending_restore.is_some() {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: server,
                pre_render: context,
            },
            enum_map::EnumMap::default(),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert!(live.adv_state.audio.system_player.retired.is_empty());
    eprintln!(
        "Saved/SceneJump label cancellation: exact2073600 pixel handoff, music20pixel/10tick exit, chapter0.3/10tick exit, expiry, ten-tick toast exit/old queue discard and reconstructed-label priority and retained global system cue bus/bank/repeated-load/capacity-wait EOF cleanup pass"
    );
}

fn verify_live_resume<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::{adv::Adv, update::UpdateContext};
    use enum_map::EnumMap;
    use shin_core::time::Ticks;
    let mut live_assets = assets.clone();
    live_assets.scenario = std::sync::Arc::new(resume_scenario());
    let mut live = Adv::new(
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        live_assets.clone(),
        Scripter::new(&live_assets.scenario, 7, 42),
    );
    let mut globals = SaveData::default();
    globals.persist_data.set(0, 999);
    globals.persist_data.set(1, 456);
    globals.settings.v19 = 0;
    globals.save_vectors.seen_messages.add(1);
    let source = resume_source();
    let original_route = format!("{:?}", source.route_data);
    let save_path =
        std::env::temp_dir().join(format!("shin-resume-live-{}.dat", std::process::id()));
    assert!(!save_path.exists());
    live.configure_save_file(save_path.clone(), Some(globals.clone()))
        .unwrap();
    let capture = std::cell::Cell::new(0);
    let captured = std::cell::RefCell::new(Vec::new());
    let mut step = |live: &mut Adv, delta| {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: context,
            },
            EnumMap::default(),
        );
        if capture.get() == 1 || capture.get() == 2 && !live.is_loading_saved_scene() {
            captured
                .borrow_mut()
                .push(resume_presentation_pixels(context, live));
            capture.set(0);
        }
    };
    let mut finish_load = |live: &mut Adv| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while live.is_loading_saved_scene() {
            assert!(std::time::Instant::now() < deadline);
            step(live, 1.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            live.saved_scene_load_error().is_none(),
            "{:?}",
            live.saved_scene_load_error()
        );
    };
    live.load_saved_scene(&globals, &source, server.clone())
        .unwrap();
    finish_load(&mut live);
    drop(finish_load);
    let random = live.vm_state.get_persistent(6);
    assert_eq!(live.vm_state.save_position, 2);
    // The setup uses an ordinary save load; finish its native notification
    // reset before constructing the independent Resume continuity case.
    assert_eq!(
        live.adv_state.notifications.update(Ticks::from_f32(10.0)),
        None
    );
    live.adv_state.bgm_title.show(server, "Resume song");
    live.adv_state.route_label.show(server, "Resume chapter");
    live.adv_state
        .notifications
        .enqueue(super::super::information::NotificationKind::Characters, 1);
    live.adv_state
        .notifications
        .enqueue(super::super::information::NotificationKind::Tips, 2);
    step(&mut live, 0.0);
    assert_eq!(live.adv_state.notifications.reference_motion().0, Some(0));
    let cue_track = live.adv_state.audio.system_player.track.id();
    assert!(matches!(
        live.current_command,
        Some(super::super::ExecutingCommand::WAIT(_))
    ));
    live.vm_state.set_persistent(98, 123);
    live.vm_state.commit_persistent();
    let reclamation_deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live
        .adv_state
        .audio_manager
        .kira_manager()
        .lock()
        .num_sub_tracks()
        > super::super::AdvState::AUDIO_TRACK_COUNT
    {
        assert!(std::time::Instant::now() < reclamation_deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let blockers = {
        let mut manager = live.adv_state.audio_manager.kira_manager().lock();
        let spare = manager.sub_track_capacity() - manager.num_sub_tracks();
        (0..spare)
            .map(|_| {
                manager
                    .add_sub_track(kira::track::TrackBuilder::new())
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    step(&mut live, 10.0);
    assert!(live.is_loading_saved_scene());
    let on_disk = SaveData::decode(&std::fs::read(&save_path).unwrap()).unwrap();
    assert_eq!(
        on_disk.persist_data.get(98),
        123,
        "committed globals flushed before early RESUME return"
    );
    assert!(matches!(
        live.current_command,
        Some(super::super::ExecutingCommand::RESUME(_))
    ));
    assert_eq!(
        live.vm_state.save_position, 2,
        "old VM retained until installation"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live.pending_restore.as_ref().unwrap().ready.is_none() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 0.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..3 {
        step(&mut live, 4.0);
    }
    assert!(live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.vm_state.route.bit_position(), 34);
    assert_eq!(live.vm_state.persist.get(99), 0);
    let title_motion = live.adv_state.bgm_title.reference_motion();
    let label_motion = live.adv_state.route_label.reference_motion();
    let notification_motion = live.adv_state.notifications.reference_motion();
    assert_eq!(
        title_motion.2, 22.0,
        "presentation clock advances while script remains frozen"
    );
    assert!(!title_motion.3);
    assert!((label_motion.0 - 0.66).abs() < 0.00001);
    assert_eq!(notification_motion.0, Some(0));
    assert_eq!(notification_motion.1, [17]);
    assert!((notification_motion.3 - 22.0 / 30.0).abs() < 0.00001);
    capture.set(1);
    step(&mut live, 0.0);
    drop(blockers);
    capture.set(2);
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 0.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    {
        let pixels = captured.borrow();
        assert_eq!(pixels.len(), 2);
        assert_eq!(
            pixels[0], pixels[1],
            "RESUME must preserve every presentation pixel through atomic installation"
        );
        for (top, bottom) in [(0, 96), (100, 164), (980, 1080)] {
            let count = pixels[0][top * 1920 * 4..bottom * 1920 * 4]
                .chunks_exact(4)
                .filter(|p| p[..3] != [0, 0, 0])
                .count();
            assert!(
                count > 100,
                "title, notification and chapter regions must contain visible ROM glyph/atlas pixels"
            );
        }
    }
    assert_eq!(live.vm_state.save_position, 1);
    assert_eq!(live.vm_state.resume_position, Some(1));
    assert_eq!(live.adv_state.bgm_title.reference_motion(), title_motion);
    assert_eq!(live.adv_state.route_label.reference_motion(), label_motion);
    assert_eq!(
        live.adv_state.notifications.reference_motion(),
        notification_motion
    );
    assert_eq!(
        live.adv_state.audio.system_player.track.id(),
        cue_track,
        "existing notification audio keeps its routing track"
    );
    assert_eq!(live.vm_state.route.bit_position(), 17);
    assert_eq!(live.vm_state.persist.get(0), 999);
    assert_eq!(live.vm_state.persist.get(98), 123);
    assert_eq!(live.vm_state.settings.v19, 0);
    assert!(live.vm_state.save_vectors.seen_messages.contains(1));
    assert!(!live.finished);
    step(&mut live, 0.0);
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.vm_state.persist.get(5), 456);
    assert_eq!(live.vm_state.persist.get(6), random);
    let snapshot = live.vm_state.route.snapshot();
    let mut route = snapshot.reader();
    assert_eq!(route.read_persistent().unwrap(), -14);
    assert_eq!(route.read_persistent().unwrap(), 456);
    assert_eq!(live.vm_state.persist.get(99), 0);
    assert_eq!(format!("{:?}", source.route_data), original_route);

    // An injected unreachable bookmark must not let RESUME fall through
    // or continually enqueue replacement attempts after a failure.
    live.vm_state.resume_position = Some(3);
    step(&mut live, 10.0);
    assert!(!live.is_loading_saved_scene());
    assert!(
        live.saved_scene_load_error()
            .unwrap()
            .contains("beyond RESUME")
    );
    live.vm_state.resume_position = Some(1);
    for _ in 0..3 {
        step(&mut live, 100.0);
    }
    assert!(!live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.vm_state.persist.get(99), 0);
    assert!(matches!(
        live.current_command,
        Some(super::super::ExecutingCommand::RESUME(_))
    ));

    // Explicit loading replaces the failed task; an unset bookmark then
    // restarts at boundary zero rather than continuing after RESUME.
    live.load_saved_scene(&globals, &source, server.clone())
        .unwrap();
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    live.vm_state.resume_position = None;
    live.adv_state.hidden_ui_active = true;
    live.adv_state.acquire_ui_hide();
    step(&mut live, 0.0);
    step(&mut live, 10.0);
    assert!(live.is_loading_saved_scene());
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 0.0);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.vm_state.save_position, 0);
    assert_eq!(live.vm_state.route.bit_position(), 0);
    assert_eq!(live.vm_state.resume_position, None);
    assert!(live.adv_state.hidden_ui_active);
    assert_eq!(live.adv_state.ui_hide_count, 1);
    let mut dismiss = EnumMap::default();
    dismiss[crate::app::AppAction::Enter] = shin_input::ActionState {
        is_clicked: true,
        ..Default::default()
    };
    live.handle_input(dismiss, true);
    assert!(!live.adv_state.hidden_ui_active);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    step(&mut live, 0.0);
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.vm_state.resume_position, Some(1));
    let snapshot = live.vm_state.route.snapshot();
    let mut route = snapshot.reader();
    assert_eq!(route.read_persistent().unwrap(), 999);
    assert_eq!(route.read_persistent().unwrap(), 456);
    assert_eq!(live.vm_state.persist.get(99), 0);
    std::fs::remove_file(save_path).unwrap();
    eprintln!(
        "Native live RESUME: boundary2->bookmark1, audio-capacity deferral, disk/global preservation, route prefix retained/suffix rewritten, deterministic PRNG, one-shot failure, explicit recovery and unset bookmark0 restart pass"
    );
}

fn verify_live_scene_jump_late_target<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    audio: std::sync::Arc<shin_audio::AudioManager>,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::{adv::Adv, app::AppAction, update::UpdateContext};
    use enum_map::EnumMap;
    use shin_core::{
        format::scenario::{
            instruction_elements::{MessageId, U8Bool},
            types::U16FixupString,
        },
        time::Ticks,
        vm::Scripter,
    };
    use shin_input::ActionState;
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    for i in 1..=25 {
        Instruction::Command(C::MSGSET(c::MSGSET {
            msg_id: MessageId(i),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("@rScrolling history message"),
        }))
        .write_le(&mut cursor)
        .unwrap();
        Instruction::Command(C::AUTOSAVE(c::AUTOSAVE {}))
            .write_le(&mut cursor)
            .unwrap();
    }
    Instruction::Command(C::EXIT(c::EXIT {
        arg1: 0,
        arg2: NumberSpec::constant(0),
    }))
    .write_le(&mut cursor)
    .unwrap();
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let mut live_assets = assets.clone();
    live_assets.scenario = std::sync::Arc::new(Scenario::new(bytes.into()).unwrap());
    let mut live = Adv::new(
        audio,
        server.clone(),
        live_assets.clone(),
        Scripter::new(&live_assets.scenario, 0, 12345),
    );
    let mut globals = SaveData::default();
    for i in 1..=25 {
        globals.save_vectors.seen_messages.add(i);
    }
    let source = GameDataEntry {
        scenario_id: 0,
        random_seed: 12345,
        save_position: 49,
        route_data: RouteRecorder::default().snapshot(),
    };
    live.load_saved_scene(&globals, &source, server.clone())
        .unwrap();
    let press = |action| {
        let key = match action {
            AppAction::CharacterPageUp => Some(winit::keyboard::KeyCode::KeyH),
            AppAction::CharacterPageDown => Some(winit::keyboard::KeyCode::KeyJ),
            AppAction::SceneJumpMenu => Some(winit::keyboard::KeyCode::KeyG),
            _ => None,
        };
        if let Some(key) = key {
            let mut raw = shin_input::RawInputState::new();
            raw.keyboard.insert(key);
            return <AppAction as shin_input::Action>::lower(&raw).map(|_, held| ActionState {
                is_clicked: held,
                is_clicked_or_repeated: held,
                is_held: held,
                ..Default::default()
            });
        }
        let mut states = EnumMap::default();
        states[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            is_held: true,
            ..Default::default()
        };
        states
    };
    let mut step = |live: &mut Adv, delta: f32, input: EnumMap<AppAction, ActionState>| {
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(live.vm_state.save_position, 49);
    let mut raw_x = shin_input::RawInputState::new();
    raw_x.gamepads.unified.buttons[shin_input::inputs::GamepadButton::X] = true;
    let lowered_x = <AppAction as shin_input::Action>::lower(&raw_x);
    let controller_x = EnumMap::from_fn(|action| ActionState {
        is_clicked: lowered_x[action],
        is_clicked_or_repeated: lowered_x[action],
        is_held: lowered_x[action],
        ..Default::default()
    });
    raw_x.gamepads.unified.buttons[shin_input::inputs::GamepadButton::Up] = true;
    let lowered_x_up = <AppAction as shin_input::Action>::lower(&raw_x);
    let controller_x_up = EnumMap::from_fn(|action| ActionState {
        is_clicked: lowered_x_up[action],
        is_clicked_or_repeated: lowered_x_up[action],
        is_held: lowered_x_up[action],
        ..Default::default()
    });
    step(&mut live, 1.0, controller_x_up);
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .is_some_and(|s| s.shell.is_open())
    {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let selected = |live: &Adv| {
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected
    };
    assert_eq!(selected(&live), Some(47));
    for key in [
        winit::keyboard::KeyCode::KeyK,
        winit::keyboard::KeyCode::KeyL,
    ] {
        let mut raw = shin_input::RawInputState::new();
        raw.keyboard.insert(key);
        let input = <AppAction as shin_input::Action>::lower(&raw).map(|_, held| ActionState {
            is_clicked: held,
            is_clicked_or_repeated: held,
            is_held: held,
            ..Default::default()
        });
        step(&mut live, 1.0, input);
        assert_eq!(selected(&live), Some(47), "K/L no longer jump in Backlog");
    }
    step(&mut live, 1.0, press(AppAction::CharacterPageDown));
    assert_eq!(selected(&live), Some(49));
    // The native parent scroll tween owns input until its fit finishes.
    step(&mut live, 1.0, press(AppAction::Cancel));
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    step(&mut live, 1.0, press(AppAction::CharacterPageUp));
    assert_eq!(selected(&live), Some(49));
    for _ in 0..4 {
        step(&mut live, 1.0, EnumMap::default());
    }
    for (action, expected) in [
        (AppAction::CharacterPageUp, 47),
        (AppAction::CharacterPageUp, 45),
        (AppAction::CharacterPageDown, 47),
    ] {
        step(&mut live, 1.0, press(action));
        assert_eq!(selected(&live), Some(expected));
        for _ in 0..4 {
            step(&mut live, 1.0, EnumMap::default());
        }
    }
    step(&mut live, 1.0, controller_x);
    step(&mut live, 5.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .0
            .max_scroll()
            > 720.0
    );
    step(&mut live, 1.0, press(AppAction::CharacterPageUp));
    for _ in 0..4 {
        step(&mut live, 1.0, EnumMap::default());
    }
    let p = glam::vec2(1000.0, 500.0);
    live.handle_pointer_state(p, true, true, 1.0 / 60.0);
    live.handle_pointer_state(p - glam::Vec2::Y * 40.0, false, true, 1.0 / 60.0);
    live.handle_pointer_state(p - glam::Vec2::Y * 140.0, false, true, 1.0 / 60.0);
    live.handle_pointer_state(
        p - glam::Vec2::Y * 140.0,
        false,
        false,
        0.016683333333333335,
    );
    live.handle_pointer_state(p - glam::Vec2::Y * 140.0, false, false, 1.0 / 60.0);
    step(&mut live, 1.0, press(AppAction::ConfirmSceneJump));
    let requested = live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .reference_requested_jump_position()
        .unwrap();
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_requested_feedback(),
        Some((true, true))
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_dialog_created(),
        "Current row ready/absent starts the dialog while the original row still flashes"
    );
    let mut evicted_original = false;
    let mut ticks = 0;
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .reference_confirmation()
        .unwrap()
        .is_open()
    {
        ticks += 1;
        assert!(ticks < 40);
        step(&mut live, 1.0, EnumMap::default());
        evicted_original |= live
            .adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_requested_feedback()
            .is_some_and(|(cached, _)| !cached);
    }
    assert!(
        evicted_original,
        "Original glyph/feedback row must be evicted while the dialog continues opening"
    );
    assert_ne!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_selected_jump_position(),
        Some(requested),
        "Inertia must continue while the child dialog opens"
    );
    step(&mut live, 1.0, press(AppAction::Left));
    step(&mut live, 1.0, press(AppAction::Enter));
    let mut ticks = 0;
    while live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .reference_prepared_jump_position()
        .is_none()
    {
        ticks += 1;
        assert!(ticks < 40);
        step(&mut live, 1.0, EnumMap::default());
    }
    let browser = live.adv_state.backlog.as_ref().unwrap();
    let accepted = browser.reference_prepared_jump_position().unwrap();
    assert_eq!(Some(accepted), browser.reference_selected_jump_position());
    assert_ne!(
        accepted, requested,
        "Accepted Yes must use the live cursor, not the opening flash row"
    );
    assert_eq!(live.vm_state.save_position, 49);
    assert_eq!(live.adv_state.audio_pause_count, 1);
    for _ in 0..10 {
        step(&mut live, 1.0, EnumMap::default());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.adv_state.backlog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert_eq!(live.previous_rollback_position(), Some(49));
    // AUTOSAVE checkpoint is restored exactly, then its immediate completion
    // resumes at the following MSGSET, one save boundary later.
    assert_eq!(live.vm_state.save_position, accepted);
    step(&mut live, 1.0, EnumMap::default());
    assert_eq!(live.vm_state.save_position, accepted + 1);
    eprintln!(
        "Native live SceneJump late target: flash boundary{requested}, accepted cursor boundary{accepted}, resumed MSG boundary{}",
        live.vm_state.save_position
    );
}

#[inline(never)]
fn verify_live_backlog<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a std::sync::Arc<crate::asset::system::AssetServer>,
    assets: &crate::adv::assets::AdvAssets,
    audio: std::sync::Arc<shin_audio::AudioManager>,
    frame: shin_core::primitives::update::FrameId,
) {
    use crate::app::AppAction;
    use crate::{
        adv::Adv,
        layer::{DrawableLayer, Layer},
        update::UpdateContext,
    };
    use enum_map::EnumMap;
    use shin_core::{
        format::scenario::{
            instruction_elements::{MessageId, U8Bool},
            types::U16FixupString,
        },
        time::Ticks,
        vm::Scripter,
    };
    use shin_input::ActionState;
    use std::sync::Arc;
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    for command in [
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(1),
            auto_wait: U8Bool(true),
            text: U16FixupString::new(&format!(
                "戦人@rBacklog live message@v19/11900001.{}",
                "@rA tall history row for scrolling".repeat(25)
            )),
        }),
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(2),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("@rFollowing story message"),
        }),
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }),
    ] {
        Instruction::Command(command).write_le(&mut cursor).unwrap();
    }
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let mut live_assets = assets.clone();
    live_assets.scenario = Arc::new(Scenario::new(bytes.into()).unwrap());
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        live_assets.clone(),
        Scripter::new(&live_assets.scenario, 0, 12345),
    );
    let mut checked_jump_render = false;
    let mut step = |live: &mut Adv, delta: f32, input: EnumMap<AppAction, ActionState>| {
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(delta),
            asset_server: &server,
            pre_render: context,
        };
        live.update(&mut update, input);
        if !checked_jump_render
            && live
                .adv_state
                .backlog
                .as_ref()
                .is_some_and(|b| b.reference_scene_jump().is_some_and(|(_, _, open)| open))
        {
            let target = context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("SceneJump reference render"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            {
                use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
                let mut pass = shin_render::render_pass::RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "SceneJump reference",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .reference_render_scene_jump(&mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(context, &target);
            let mut samples = 0;
            for y in [32, 100, 1000, 1050] {
                for x in 0..1920 {
                    let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 3];
                    let alpha = (192.0 * (x as f32 + 0.5) / 420.0).min(192.0);
                    let expected = (128.0 * alpha / 255.0).round() as i32;
                    assert!(
                        actual.iter().all(|&v| (i32::from(v) - expected).abs() <= 1),
                        "SceneJump native pane at{x},{y}: {actual:?} != {expected}"
                    );
                    samples += 1;
                }
            }
            let text_pixels = (243..420)
                .flat_map(|y| (800..1800).map(move |x| (y * 1920 + x) * 4))
                .filter(|&i| pixels[i..i + 3].iter().any(|&v| v > 160))
                .count();
            assert!(
                text_pixels > 1000,
                "Real system-font labels must draw inside the clipped viewport"
            );
            eprintln!(
                "SceneJump native pane: {samples} scalar pixels; {text_pixels} label pixels"
            );
            checked_jump_render = true;
        }
    };
    step(&mut live, 20.0, EnumMap::default());
    let position = live.vm_state.save_position;
    assert_eq!(position, 1);
    let press = |action: AppAction| {
        let mut state = EnumMap::default();
        state[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            is_held: true,
            ..Default::default()
        };
        state
    };
    live.vm_state.system_controls_allowed = false;
    step(&mut live, 0.0, press(AppAction::Backlog));
    assert!(live.adv_state.backlog.is_none());
    live.vm_state.system_controls_allowed = true;
    // Native browser stops at an unread source message. Simulate a message
    // already read on a prior playthrough so this modal fixture has history.
    live.vm_state.save_vectors.seen_messages.add(1);
    live.adv_state
        .root_layer_group_mut()
        .properties_mut()
        .property_tweener_mut(shin_core::vm::command::types::LayerProperty::TranslateX)
        .enqueue(
            100.0,
            shin_core::time::Tween::linear(Ticks::from_f32(100.0)),
        );
    live.adv_state.playback_modes.skip = true;
    step(&mut live, 0.0, press(AppAction::Backlog));
    assert!(
        live.adv_state.backlog.is_none(),
        "First X cancels skip without opening backlog"
    );
    assert!(!live.adv_state.playback_modes.skip);
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    step(&mut live, 1.0, press(AppAction::Backlog));
    assert!(live.adv_state.backlog.is_some());
    assert_eq!(live.adv_state.audio_pause_count, 1);
    assert_eq!(live.adv_state.ui_hide_count, 1);
    assert!(!live.adv_state.playback_modes.skip);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .is_some_and(|screen| screen.shell.is_open())
    {
        assert!(
            std::time::Instant::now() < deadline,
            "Live backlog did not open"
        );
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    step(&mut live, 400.0, press(AppAction::Enter));
    step(&mut live, 400.0, press(AppAction::ToggleSkip));
    assert_eq!(
        live.vm_state.save_position, position,
        "Modal input must not advance story"
    );
    assert!(!live.adv_state.playback_modes.skip);
    assert_eq!(
        live.adv_state
            .root_layer_group()
            .properties()
            .get_value(shin_core::vm::command::types::LayerProperty::TranslateX),
        0.0,
        "Backlog must freeze root animation across loading and long frames"
    );
    // Pressing highlights a voiced row without starting audio. Leaving
    // the captured row cancels the gesture, even if release returns to it.
    let voice_point = glam::vec2(200.0, 220.0);
    live.handle_pointer_state(voice_point, true, true, 1.0 / 60.0);
    step(&mut live, 3.5, EnumMap::default());
    assert!(!live.adv_state.backlog.as_ref().unwrap().is_playing());
    live.handle_pointer_state(glam::vec2(135.0, 220.0), false, true, 1.0 / 60.0);
    // Stationary sample cancels leftward fling momentum; a fast leftward
    // release now correctly starts the rollback child task.
    live.handle_pointer_state(glam::vec2(135.0, 220.0), false, true, 1.0 / 60.0);
    live.handle_pointer_state(voice_point, false, false, 1.0 / 60.0);
    step(&mut live, 3.5, EnumMap::default());
    assert!(!live.adv_state.backlog.as_ref().unwrap().is_playing());
    live.handle_pointer_state(voice_point, true, true, 1.0 / 60.0);
    live.handle_pointer_state(voice_point, false, false, 1.0 / 60.0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !live.adv_state.backlog.as_ref().unwrap().is_playing() {
        assert!(
            std::time::Instant::now() < deadline,
            "Backlog voice did not load"
        );
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    step(&mut live, 1.0, press(AppAction::Enter));
    assert!(
        !live.adv_state.backlog.as_ref().unwrap().is_playing(),
        "Native confirm restarts selected replay with its ten-frame startup"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !live.adv_state.backlog.as_ref().unwrap().is_playing() {
        assert!(
            std::time::Instant::now() < deadline,
            "Backlog voice did not load"
        );
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..20 {
        step(&mut live, 0.0, press(AppAction::TextScrollDown));
    }
    let before_drag = live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .unwrap()
        .model
        .scroll();
    assert!(before_drag > 200.0, "Tall live history must be scrollable");
    let drag_origin = glam::vec2(500.0, 500.0);
    live.handle_pointer_state(drag_origin, true, true, 1.0 / 60.0);
    live.handle_pointer_state(drag_origin + glam::vec2(0.0, 80.0), false, true, 1.0 / 60.0);
    let dragged = live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .unwrap()
        .model
        .scroll();
    assert_eq!(dragged, before_drag - 80.0);
    live.handle_pointer_state(
        drag_origin + glam::vec2(0.0, 80.0),
        false,
        false,
        0.016683333333333335,
    );
    step(&mut live, 0.5, EnumMap::default());
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .scroll(),
        dragged - 40.0
    );
    // Zero tick cancels inertia via the native no-displacement branch.
    step(&mut live, 0.0, EnumMap::default());
    let close_point = glam::vec2(1800.0, 70.0);
    live.handle_pointer_state(close_point, true, true, 1.0 / 60.0);
    step(&mut live, 3.5, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    live.handle_pointer_state(glam::vec2(100.0, 100.0), false, true, 1.0 / 60.0);
    live.handle_pointer_state(close_point, false, false, 1.0 / 60.0);
    step(&mut live, 3.5, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    assert_eq!(live.adv_state.audio_pause_count, 1);
    live.handle_pointer_state(close_point, true, true, 1.0 / 60.0);
    live.handle_pointer_state(close_point, false, false, 1.0 / 60.0);
    step(&mut live, 7.0, EnumMap::default());
    assert!(
        live.adv_state.backlog.is_some(),
        "Flash completion must not resume story"
    );
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 5.0, EnumMap::default());
    assert_eq!(
        live.adv_state.audio_pause_count, 1,
        "Closing slide retains pause"
    );
    step(&mut live, 5.0, EnumMap::default());
    assert!(live.adv_state.backlog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert_eq!(live.vm_state.save_position, position);
    assert!(!live.adv_state.audio.se_player.reference_is_paused());
    assert!(
        !live
            .adv_state
            .message_layer()
            .voice_player()
            .reference_pause_state()
            .0
    );
    // Keyboard cancel stops replay immediately, while the browser retains
    // its story pause until the close task completes.
    step(&mut live, 0.0, press(AppAction::Backlog));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .is_some_and(|screen| screen.shell.is_open())
    {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    step(&mut live, 1.0, press(AppAction::ReplayVoice));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !live.adv_state.backlog.as_ref().unwrap().is_playing() {
        assert!(
            std::time::Instant::now() < deadline,
            "Backlog voice did not load"
        );
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    live.handle_pointer_state(close_point, true, true, 1.0 / 60.0);
    step(&mut live, 0.0, press(AppAction::Cancel));
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open(),
        "Active pointer owns input before keyboard cancel"
    );
    assert!(live.adv_state.backlog.as_ref().unwrap().is_playing());
    live.handle_pointer_state(glam::vec2(100.0, 100.0), false, true, 1.0 / 60.0);
    live.handle_pointer_state(glam::vec2(100.0, 100.0), false, false, 1.0 / 60.0);
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 0.0, press(AppAction::Cancel));
    assert!(!live.adv_state.backlog.as_ref().unwrap().is_playing());
    assert_eq!(live.adv_state.audio_pause_count, 1);
    step(&mut live, 3.5, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 1.0, EnumMap::default());
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .progress(),
        1.0,
        "Canceled pointer feedback must not shorten the keyboard close flash"
    );
    step(&mut live, 2.5, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 10.0, EnumMap::default());
    assert!(live.adv_state.backlog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert_eq!(live.vm_state.save_position, position);
    // Reach a later boundary, then use a text-body release to jump back.
    live.adv_state.root_layer_group_mut().fast_forward();
    step(&mut live, 400.0, EnumMap::default());
    step(&mut live, 0.0, press(AppAction::Enter));
    step(&mut live, 0.0, EnumMap::default());
    assert_eq!(live.vm_state.save_position, 2);
    // Both menu positions must be available under the native unread gate.
    live.vm_state.save_vectors.seen_messages.add(2);
    // A retained future route payload must survive cursor reconstruction.
    live.vm_state.route.record_choice(9);
    let source = live.vm_state.snapshot_entry();
    let source_route = format!("{:?}", source.route_data);
    let persistent = format!("{:?}", live.vm_state.persist);
    let vectors = format!("{:?}", live.vm_state.save_vectors);
    step(&mut live, 0.0, press(AppAction::Backlog));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .is_some_and(|screen| screen.shell.is_open())
    {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    step(&mut live, 0.0, press(AppAction::Up));
    // Native current marker is the last row strictly before the boundary.
    // Move the browser cursor to the available row at the boundary first.
    step(&mut live, 0.0, press(AppAction::AnyDown));
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .1,
        0.0
    );
    step(&mut live, 5.0, EnumMap::default());
    {
        let (menu, progress, open) = live
            .adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap();
        assert!(open);
        assert_eq!(progress, 1.0);
        assert_eq!(menu.selected_target(), 1);
        assert!(menu
            .markers
            .iter()
            .any(|m| m.kind == crate::adv::backlog::jump::Kind::Current && m.target_row == 0));
    }
    step(&mut live, 0.0, press(AppAction::Up));
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected,
        Some(0)
    );
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected,
        Some(1)
    );
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    step(&mut live, 0.0, press(AppAction::Up));
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    // Menu clicks select markers on release, without opening rollback.
    let marker_point = glam::vec2(1000.0, 450.0); // end marker, index3
    live.handle_pointer_state(marker_point, true, true, 1.0 / 60.0);
    step(&mut live, 0.0, press(AppAction::Cancel));
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .2
    );
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected,
        Some(0)
    );
    live.handle_pointer_state(marker_point, false, false, 1.0 / 60.0);
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected,
        Some(1)
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_confirmation()
            .is_none()
    );
    for _ in 0..6 {
        step(&mut live, 0.0, EnumMap::default());
    }
    step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    step(&mut live, 0.0, press(AppAction::Up));
    for _ in 0..4 {
        step(&mut live, 0.0, EnumMap::default());
    }
    // A nested default-No dialog returns to this menu with its cursor intact.
    step(&mut live, 0.0, press(AppAction::ConfirmSceneJump));
    step(&mut live, 3.5, EnumMap::default());
    step(&mut live, 3.5, EnumMap::default());
    step(&mut live, 12.0, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_confirmation()
            .unwrap()
            .is_open()
    );
    step(&mut live, 0.0, press(AppAction::Enter));
    step(&mut live, 7.0, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 12.0, EnumMap::default());
    step(&mut live, 4.0, EnumMap::default());
    step(&mut live, 1.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_confirmation()
            .is_none()
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .2
    );
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.adv_state.audio_pause_count, 1);
    step(&mut live, 0.0, press(AppAction::Cancel));
    step(&mut live, 3.5, EnumMap::default());
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .1,
        1.0
    );
    step(&mut live, 3.5, EnumMap::default());
    step(&mut live, 5.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .is_none()
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    assert_eq!(live.adv_state.audio_pause_count, 1);
    assert_eq!(live.adv_state.ui_hide_count, 1);
    // Dispatch a stationary released sample, as the app does every frame.
    live.handle_pointer_state(marker_point, false, false, 1.0 / 60.0);
    // A rightward fling dismisses just the menu, without button flash.
    step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
    step(&mut live, 5.0, EnumMap::default());
    let fling_origin = glam::vec2(900.0, 500.0);
    live.handle_pointer_state(fling_origin, true, true, 1.0 / 60.0);
    live.handle_pointer_state(
        fling_origin + glam::vec2(100.0, 0.0),
        true,
        true,
        1.0 / 60.0,
    );
    live.handle_pointer_state(
        fling_origin + glam::vec2(100.0, 0.0),
        false,
        false,
        0.016683333333333335,
    );
    assert!(
        !live
            .adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .2
    );
    step(&mut live, 5.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .is_none()
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
    step(&mut live, 5.0, EnumMap::default());
    let menu_close = glam::vec2(1800.0, 80.0);
    live.handle_pointer_state(menu_close, true, true, 1.0 / 60.0);
    live.handle_pointer_state(menu_close, false, false, 1.0 / 60.0);
    step(&mut live, 3.5, EnumMap::default());
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .unwrap()
            .1,
        1.0
    );
    step(&mut live, 3.5, EnumMap::default());
    step(&mut live, 5.0, EnumMap::default());
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .reference_scene_jump()
            .is_none()
    );
    assert!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .shell
            .is_open()
    );
    assert_eq!(live.adv_state.audio_pause_count, 1);
    step(&mut live, 0.0, EnumMap::default());
    // Ensure the selected tall first row is visible at a body coordinate.
    for _ in 0..40 {
        step(&mut live, 0.0, press(AppAction::TextScrollUp));
    }
    let body = glam::vec2(500.0, 220.0);
    for attempt in 0..3 {
        if attempt == 2 {
            live.handle_pointer_state(body, false, false, 1.0 / 60.0);
            step(&mut live, 0.0, press(AppAction::SceneJumpMenu));
            step(&mut live, 5.0, EnumMap::default());
            assert!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .reference_scene_jump()
                    .unwrap()
                    .2
            );
            step(&mut live, 0.0, press(AppAction::ConfirmSceneJump));
        } else {
            live.handle_pointer_state(body, true, true, 1.0 / 60.0);
            if attempt == 1 {
                let left = body - glam::vec2(100.0, 0.0);
                live.handle_pointer_state(left, false, true, 1.0 / 60.0);
                live.handle_pointer_state(left, false, false, 0.016683333333333335);
            } else {
                live.handle_pointer_state(body, false, false, 1.0 / 60.0);
            }
        }
        assert!(
            live.adv_state
                .backlog
                .as_ref()
                .unwrap()
                .reference_confirmation()
                .is_some()
        );
        step(&mut live, 3.5, EnumMap::default());
        assert!(
            !live
                .adv_state
                .backlog
                .as_ref()
                .unwrap()
                .reference_confirmation()
                .unwrap()
                .is_open()
        );
        step(&mut live, 3.5, EnumMap::default());
        step(&mut live, 12.0, EnumMap::default());
        step(&mut live, 0.0, EnumMap::default());
        assert!(
            live.adv_state
                .backlog
                .as_ref()
                .unwrap()
                .reference_confirmation()
                .unwrap()
                .is_open()
        );
        assert_eq!(
            live.adv_state
                .backlog
                .as_ref()
                .unwrap()
                .reference_confirmation()
                .unwrap()
                .selected(),
            Some(1)
        );
        if attempt != 0 {
            step(&mut live, 0.0, press(AppAction::Left));
            assert_eq!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .reference_confirmation()
                    .unwrap()
                    .selected(),
                Some(0)
            );
        }
        step(&mut live, 0.0, press(AppAction::Enter));
        step(&mut live, 7.0, EnumMap::default());
        step(&mut live, 0.0, EnumMap::default());
        step(&mut live, 12.0, EnumMap::default());
        assert_eq!(
            live.vm_state.save_position, 2,
            "No installation during dialog close"
        );
        step(&mut live, 4.0, EnumMap::default());
        step(&mut live, 1.0, EnumMap::default());
        assert_eq!(live.adv_state.audio_pause_count, 1);
        assert_eq!(live.adv_state.ui_hide_count, 1);
        if attempt == 0 {
            assert!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .reference_confirmation()
                    .is_none()
            );
            assert!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .screen
                    .as_ref()
                    .unwrap()
                    .shell
                    .is_open()
            );
            assert!(!live.is_loading_saved_scene());
            assert_eq!(
                live.vm_state.save_position, 2,
                "Default No retains live scene"
            );
            continue;
        }
        if attempt == 2 {
            let (_, progress, open) = live
                .adv_state
                .backlog
                .as_ref()
                .unwrap()
                .reference_scene_jump()
                .unwrap();
            assert!(!open);
            assert!(progress > 0.0);
        }
        step(&mut live, 5.0, EnumMap::default());
        if attempt == 2 {
            assert!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .reference_scene_jump()
                    .is_none()
            );
        }
        assert_eq!(live.adv_state.audio_pause_count, 1);
        assert!(
            !live.is_loading_saved_scene(),
            "Closing slide precedes restore handoff"
        );
        step(&mut live, 5.0, EnumMap::default());
        assert!(live.is_loading_saved_scene());
        assert_eq!(live.vm_state.save_position, 2);
        assert_eq!(
            live.adv_state.audio_pause_count, 1,
            "Asset loading retains modal pause"
        );
        if attempt == 1 {
            // Deterministic failure of the asset-loading stage must return
            // to the existing browser without installing reconstructed VM.
            live.pending_restore = Some(super::super::PendingSceneRestore {
                task: Some(shin_tasks::async_io::spawn(async {
                    anyhow::bail!("injected scene asset failure")
                })),
                ready: None,
                retirement: None,
                kind: super::super::SceneLoadKind::Saved,
                source: None,
                present_source: false,
            });
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while live.is_loading_saved_scene() {
            assert!(std::time::Instant::now() < deadline);
            step(&mut live, 1.0, EnumMap::default());
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if attempt == 1 {
            assert!(
                live.saved_scene_load_error()
                    .unwrap()
                    .contains("injected scene asset failure")
            );
            assert_eq!(live.vm_state.save_position, 2);
            assert!(
                live.adv_state
                    .backlog
                    .as_ref()
                    .unwrap()
                    .screen
                    .as_ref()
                    .unwrap()
                    .shell
                    .is_open()
            );
            assert_eq!(live.adv_state.audio_pause_count, 2);
            assert!(live.restore_error_dialog.is_some());
            for _ in 0..100 {
                step(&mut live, 1.0, press(AppAction::Enter));
                assert_eq!(live.vm_state.save_position, 2);
                if live.restore_error_dialog.is_none() {
                    break;
                }
            }
            assert!(live.restore_error_dialog.is_none());
            assert_eq!(live.adv_state.audio_pause_count, 1);
            assert_eq!(live.previous_rollback_position(), None);
        }
    }
    assert!(live.saved_scene_load_error().is_none());
    assert!(live.adv_state.backlog.is_none());
    assert_eq!(live.vm_state.save_position, 1);
    assert_eq!(live.previous_rollback_position(), Some(2));
    assert_eq!(
        live.vm_state.resume_position, None,
        "Rollback metadata is independent of RESUMESET"
    );
    assert_eq!(
        live.vm_state.snapshot_entry().random_seed,
        source.random_seed
    );
    assert_eq!(
        format!("{:?}", live.vm_state.route.snapshot()),
        source_route
    );
    assert_eq!(live.vm_state.route.bit_position(), 0);
    assert_eq!(
        live.vm_state
            .route
            .snapshot()
            .reader()
            .read_choice()
            .unwrap(),
        9
    );
    assert_eq!(format!("{:?}", live.vm_state.persist), persistent);
    assert_eq!(format!("{:?}", live.vm_state.save_vectors), vectors);
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    // Previously seen future history must allow a live jump forward even
    // after rollback reduced the current boundary. Preserve source1 as the
    // native previous position, independent of the extended jump route.
    live.vm_state.save_vectors.seen_messages.add(2);
    step(&mut live, 0.0, press(AppAction::Backlog));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .is_some_and(|screen| screen.shell.is_open())
    {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .rows
            .len(),
        2
    );
    assert_eq!(
        live.adv_state
            .backlog
            .as_ref()
            .unwrap()
            .screen
            .as_ref()
            .unwrap()
            .model
            .selected,
        Some(0)
    );
    step(&mut live, 0.0, press(AppAction::AnyDown));
    // Browser cursor scrolling advances one native tick per frame,
    // regardless of the host frame's delta_ticks.
    for _ in 0..4 {
        step(&mut live, 1.0, EnumMap::default());
    }
    let model = &live
        .adv_state
        .backlog
        .as_ref()
        .unwrap()
        .screen
        .as_ref()
        .unwrap()
        .model;
    assert_eq!(model.selected, Some(1));
    let row = model.rows[model.selected.unwrap()];
    let point = glam::vec2(500.0, 188.0 + row.top - model.scroll() + row.height * 0.5);
    assert!((188.0..918.0).contains(&point.y));
    live.handle_pointer_state(point, true, true, 1.0 / 60.0);
    live.handle_pointer_state(point, false, false, 1.0 / 60.0);
    step(&mut live, 7.0, EnumMap::default());
    step(&mut live, 12.0, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 0.0, press(AppAction::Left));
    step(&mut live, 0.0, press(AppAction::Enter));
    step(&mut live, 7.0, EnumMap::default());
    step(&mut live, 0.0, EnumMap::default());
    step(&mut live, 12.0, EnumMap::default());
    step(&mut live, 5.0, EnumMap::default());
    assert_eq!(live.vm_state.save_position, 1);
    step(&mut live, 10.0, EnumMap::default());
    assert!(live.is_loading_saved_scene());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.vm_state.save_position, 2);
    assert_eq!(live.previous_rollback_position(), Some(1));
    assert!(live.adv_state.backlog.is_none());
    assert_eq!(live.adv_state.audio_pause_count, 0);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert_eq!(live.vm_state.quick_return_position(), Some(1));
    live.previous_rollback_position = Some(0);
    live.adv_state.hidden_ui_active = true;
    live.adv_state.acquire_ui_hide();
    let mut raw_up = shin_input::RawInputState::new();
    raw_up.gamepads.unified.buttons[shin_input::inputs::GamepadButton::Up] = true;
    let lowered = <AppAction as shin_input::Action>::lower(&raw_up);
    let repeating_up = EnumMap::from_fn(|action| shin_input::ActionState {
        is_held: lowered[action],
        is_clicked_or_repeated: lowered[action],
        ..Default::default()
    });
    step(&mut live, 0.0, repeating_up);
    assert!(live.adv_state.hidden_ui_active);
    assert_eq!(live.adv_state.ui_hide_count, 1);
    assert!(!live.is_loading_saved_scene());
    let mut backspace = press(AppAction::PreviousScene);
    backspace[AppAction::Cancel] = press(AppAction::Cancel)[AppAction::Cancel];
    step(&mut live, 0.0, backspace);
    assert!(!live.adv_state.hidden_ui_active);
    assert_eq!(live.adv_state.ui_hide_count, 0);
    assert!(!live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, 2);
    live.vm_state.system_controls_allowed = false;
    step(&mut live, 0.0, repeating_up);
    assert!(!live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, 2);
    live.vm_state.system_controls_allowed = true;
    let held_up = EnumMap::from_fn(|action| shin_input::ActionState {
        is_held: lowered[action],
        ..Default::default()
    });
    step(&mut live, 0.0, held_up);
    assert!(!live.is_loading_saved_scene());
    step(&mut live, 0.0, repeating_up);
    assert!(live.is_loading_saved_scene());
    assert_eq!(
        live.pending_restore.as_ref().unwrap().kind,
        super::super::SceneLoadKind::History
    );
    assert!(!live.load_previous_scene(server.clone()).unwrap());
    while live.is_loading_saved_scene() {
        assert!(std::time::Instant::now() < deadline);
        step(&mut live, 1.0, EnumMap::default());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(live.saved_scene_load_error().is_none());
    assert_eq!(live.vm_state.save_position, 1);
    assert_eq!(live.previous_rollback_position(), Some(2));
    assert!(!live.load_previous_scene(server.clone()).unwrap());
    step(&mut live, 0.0, press(AppAction::PreviousScene));
    assert!(!live.is_loading_saved_scene());
    assert_eq!(live.vm_state.save_position, 1);
    println!(
        "Live backlog: confirm-to-replay, pointer/fling/direct-button confirmation, default No, load failure recovery, atomic rollback, seen-future forward jump and native message-based mode1 quick-return input/system-gate/first-message rejection pass"
    );
}

mod active_restore;
mod scene_reuse;
mod saved_masks;
mod mixed_restore;
mod authored_restore;
mod authored_movie_load;
mod blocked_read;
#[cfg(target_os = "macos")]
mod saved_io_errors;
#[cfg(target_os = "macos")]
mod live_layer_io_errors;
#[cfg(target_os = "macos")]
mod live_mask_io_errors;
mod chapter_navigation_live;
mod published_loop;
mod save_context;

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires supplied ROMs, explicit syscall interposer, Metal and the default audio output"]
fn reference_asset_io_loader_failure_recovery() {
    assert!(std::env::var_os("SHIN_REFERENCE_ASSET_IO_ERRORS_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-asset-io-loaders".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM, Metal, the default audio output and SHIN_REFERENCE_POST_INSTALL_FAILURE_ONLY"]
fn reference_post_install_movie_command_failure_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_POST_INSTALL_FAILURE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-post-install-failure".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal and the default audio output; temporary saves only"]
fn reference_scene_load_error_title_mouse_resume_and_retry() {
    assert!(std::env::var_os("SHIN_REFERENCE_RESTORE_ERROR_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-scene-load-errors".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, the default audio output and SHIN_REFERENCE_MIXED_RESTORE_ONLY"]
fn reference_mixed_saved_scene_publication_and_retirement() {
    assert!(std::env::var_os("SHIN_REFERENCE_MIXED_RESTORE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-mixed-restore".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, the default audio output and native mask hydration output"]
fn reference_desktop_saved_mask_publication() {
    assert!(std::env::var_os("SHIN_REFERENCE_MASK_RESTORE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-mask-restore".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, the default audio output and SHIN_REFERENCE_SCENE_REUSE_ONLY"]
fn reference_saved_scene_drawable_reuse() {
    assert!(std::env::var_os("SHIN_REFERENCE_SCENE_REUSE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-scene-reuse".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal, the default audio output and SHIN_REFERENCE_ACTIVE_RESTORE_ONLY"]
fn reference_save_load_during_active_movie_wipe_and_requests() {
    assert!(std::env::var_os("SHIN_REFERENCE_ACTIVE_RESTORE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-active-restore".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM); restores live scene and verifies rendering/input"]
fn reference_quiz_layer_saved_scene_restores_with_supplied_assets() {
    // Several simultaneous ADV instances and save snapshots exceed the default
    // test thread stack in debug builds. Keep this fixture self-contained.
    std::thread::Builder::new()
        .name("reference-live-scene".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied original/updated ROM, Metal, the default audio output and SHIN_REFERENCE_BACKLOG_ONLY"]
fn reference_live_backlog_input_replay_and_rollback() {
    assert!(std::env::var_os("SHIN_REFERENCE_BACKLOG_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-backlog".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied ROM, GPU, the default audio output and SHIN_REFERENCE_LIVE_ENTRY"]
fn reference_live_authored_entry_skip_playback() {
    std::env::var("SHIN_REFERENCE_LIVE_ENTRY").expect("Set the actual authored entry ID");
    std::thread::Builder::new()
        .name("reference-live-authored".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires supplied patch ROM, GPU, the default audio output and SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY"]
fn reference_updated_authored_interactive_checkpoint_restoration() {
    assert!(std::env::var_os("SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_LIVE_ENTRY").is_none());
    std::thread::Builder::new()
        .name("reference-updated-controllers".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original ROM, Metal, the default audio output and SHIN_REFERENCE_ORIGINAL_INTERACTIVE_ONLY"]
fn reference_original_authored_interactive_checkpoint_restoration() {
    assert!(std::env::var_os("SHIN_REFERENCE_ORIGINAL_INTERACTIVE_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_none());
    assert!(std::env::var_os("SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY").is_none());
    assert!(std::env::var_os("SHIN_REFERENCE_LIVE_ENTRY").is_none());
    std::thread::Builder::new()
        .name("reference-original-controllers".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original ROM, Metal, the default audio output and SHIN_REFERENCE_INTERACTIVE_PUBLICATION_ONLY"]
fn reference_interactive_layer_publication_and_ownership() {
    assert!(std::env::var_os("SHIN_REFERENCE_INTERACTIVE_PUBLICATION_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_none());
    std::thread::Builder::new()
        .name("reference-interactive-publication".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "requires original/1.03 ROM, Metal, the default audio output; temporary storage only"]
fn reference_automatic_save_failure_visible_and_preserves_latest_progress() {
    assert!(std::env::var_os("SHIN_REFERENCE_SAVE_PENDING_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_AUTO_SAVE_FAILURE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-auto-save-failure".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal and the default audio output; temporary save only"]
fn reference_scenario_return_save_notification_and_long_fade() {
    assert!(std::env::var_os("SHIN_REFERENCE_SAVE_PENDING_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_TITLE_RETURN_FADE_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-title-return-fade".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "requires original/1.03 ROM, Metal and the default audio output; no save writes"]
fn reference_episode_two_character_scrollbar_and_keys() {
    assert!(std::env::var_os("SHIN_REFERENCE_SAVE_PENDING_ONLY").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_CHARACTER_SCROLL_ONLY").is_some());
    std::thread::Builder::new()
        .name("reference-character-scroll".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets)
        .unwrap()
        .join()
        .unwrap();
}

fn assert_reference_quiz_layer_saved_scene_restores_with_supplied_assets() {
    use crate::{
        adv::{Adv, assets::AdvAssets},
        asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
        layer::DrawableLayer,
        render::PreRenderContext,
        update::UpdateContext,
    };
    use shin_core::{
        format::scenario::instruction_elements::{MessageId, U8Bool},
        format::scenario::types::U16FixupString,
        primitives::update::FrameId,
        time::Ticks,
        vm::command::types::{LayerId, LayerProperty, PlaneId},
    };
    use shin_render::{
        dynamic_buffer::DynamicBuffer,
        pipelines::PipelineStorage,
        resize::{SurfaceResizeSource, ViewportParams},
        shaders::types::{
            buffer::BytesAddress,
            texture::{
                DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
            },
        },
    };
    use std::sync::Arc;

    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
            } else {
                wgpu::Features::empty()
            },
            ..Default::default()
        }))
        .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        assert!(
            std::env::var_os("SHIN_REFERENCE_LIVE_ENTRY").is_some()
                || std::env::var_os("SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_BACKLOG_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_ACTIVE_RESTORE_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_SAVE_PENDING_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_TITLE_AUDIO_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_SCENE_REUSE_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_MIXED_RESTORE_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_MASK_RESTORE_ONLY").is_some()
                || std::env::var_os("SHIN_REFERENCE_AUTHORED_RESTORE_ONLY").is_some(),
            "Updated overlay here is scoped to authored live routes/checkpoints, not original-address synthetic fixtures"
        );
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let mut assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let original_scenario = assets.scenario.clone();
    let mut bytes = scenario().raw()[..0xbc].to_vec();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(0xbc);
    // Tile with a non-default position, followed by a message boundary.
    0xc1u8.write_le(&mut cursor).unwrap();
    for value in [1, 1, 0] {
        NumberSpec::<i32>::constant(value)
            .write_le(&mut cursor)
            .unwrap();
    }
    0x1fu8.write_le(&mut cursor).unwrap();
    for value in [0xf900, -200, -100, 400, 200] {
        NumberSpec::<i32>::constant(value)
            .write_le(&mut cursor)
            .unwrap();
    }
    0xc3u8.write_le(&mut cursor).unwrap();
    for value in [1, 0] {
        NumberSpec::<i32>::constant(value)
            .write_le(&mut cursor)
            .unwrap();
    }
    1u8.write_le(&mut cursor).unwrap();
    NumberSpec::<i32>::constant(300)
        .write_le(&mut cursor)
        .unwrap();
    // Root translateY: absolute20 then relative10, with no extra save boundary.
    for (value, flags) in [(20, 0), (10, 128)] {
        0xc3u8.write_le(&mut cursor).unwrap();
        for value in [-1, 1] {
            NumberSpec::<i32>::constant(value)
                .write_le(&mut cursor)
                .unwrap();
        }
        7u8.write_le(&mut cursor).unwrap();
        for value in [value, 0, flags] {
            NumberSpec::<i32>::constant(value)
                .write_le(&mut cursor)
                .unwrap();
        }
    }
    for command in [
        C::MSGSET(c::MSGSET {
            msg_id: MessageId(1),
            auto_wait: U8Bool(true),
            text: U16FixupString::new("@rRestored message."),
        }),
        C::AUTOSAVE(c::AUTOSAVE {}),
        C::EXIT(c::EXIT {
            arg1: 0,
            arg2: NumberSpec::constant(0),
        }),
    ] {
        Instruction::Command(command).write_le(&mut cursor).unwrap();
    }
    // Keep real global stage music metadata for the App/title handoff,
    // while the small authored script isolates save reconstruction.
    let original = original_scenario.raw();
    let bgm_offset = u32::from_le_bytes(original[48..52].try_into().unwrap()) as usize;
    let bgm_size =
        u32::from_le_bytes(original[bgm_offset..bgm_offset + 4].try_into().unwrap()) as usize;
    let new_bgm_offset = bytes.len() as u32;
    bytes.extend_from_slice(&original[bgm_offset..bgm_offset + 4 + bgm_size]);
    bytes[48..52].copy_from_slice(&new_bgm_offset.to_le_bytes());
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    assets.scenario = Arc::new(Scenario::new(bytes.into()).unwrap());
    assert_eq!(
        assets.scenario.info_tables().bgm_info,
        original_scenario.info_tables().bgm_info
    );
    let mut adv = Adv::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets.clone(),
        Scripter::new(&assets.scenario, 0, 42),
    );
    let disk_path =
        std::env::temp_dir().join(format!("shin-live-restoration-{}.dat", std::process::id()));
    assert!(!disk_path.exists());
    let imported = SaveData {
        play_seconds: 123,
        ..Default::default()
    };
    adv.configure_save_file(disk_path.clone(), Some(imported))
        .unwrap();
    assert_eq!(adv.play_ticks(), 7380.0);
    adv.advance_play_clock(Ticks::from_f32(59.5));
    assert_eq!(adv.play_ticks(), 7439.5);
    assert_eq!(adv.persistence.as_ref().unwrap().data.play_seconds, 123);
    let entry = GameDataEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: 1,
        route_data: Default::default(),
    };
    adv.load_saved_scene(&SaveData::default(), &entry, server.clone())
        .unwrap();

    let size = wgpu::Extent3d {
        width: 1920,
        height: 1080,
        depth_or_array_layers: 1,
    };
    let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
        1920, 1080,
    )));
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Restore depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::DEPTH_STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Restore target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let samplers = TextureSamplerStore::new(&device);
    let mut pipelines = PipelineStorage::new(device.clone(), format);
    let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
    let mut encoder = device.create_command_encoder(&Default::default());
    // The production App binds effects.breakup before ADV playback. The
    // continuous branch returns before the later isolated dissolve fixture,
    // so it must own and bind the original archive here as well.
    let live_effect_textures: Option<Arc<crate::asset::effects::EffectTextures>> =
        std::env::var("SHIN_REFERENCE_LIVE_ENTRY")
            .ok()
            .or_else(|| std::env::var("SHIN_REFERENCE_LIVE_LAYER_IO_ONLY").ok())
            .map(|_| shin_tasks::block_on(server.load("/effects.txa")).unwrap());
    let mut context = PreRenderContext {
        breakup_texture: live_effect_textures
            .as_ref()
            .map(|effects| effects.required("breakup")),
        device: &device,
        queue: &queue,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    if std::env::var_os("SHIN_REFERENCE_AUTHORED_RESTORE_ONLY").is_some() {
        #[cfg(target_os = "macos")]
        if std::env::var_os("SHIN_REFERENCE_LIVE_LAYER_IO_ONLY").is_some() {
            if std::env::var_os("SHIN_REFERENCE_LIVE_MASK_IO_ONLY").is_some() {
                live_mask_io_errors::verify(&mut context, &assets, &original_scenario, &view);
            } else {
                live_layer_io_errors::verify(&mut context, &assets, &original_scenario, &view);
            }
            assert!(
                !disk_path.exists(),
                "Live layer IO checks must not write progress"
            );
            return;
        }
        if std::env::var_os("SHIN_REFERENCE_CHAPTER_NAVIGATION_ONLY").is_some() {
            chapter_navigation_live::verify(
                &mut context,
                &server,
                &assets,
                &original_scenario,
                &view,
            );
            assert!(
                !disk_path.exists(),
                "Chapter checks must not write progress"
            );
            return;
        }
        #[cfg(target_os = "macos")]
        if std::env::var_os("SHIN_REFERENCE_SAVED_IO_ERRORS_ONLY").is_some() {
            saved_io_errors::verify(&mut context, &assets, &original_scenario, &view);
            assert!(
                !disk_path.exists(),
                "Saved IO error checks must not write progress"
            );
            return;
        }
        #[cfg(target_os = "macos")]
        if std::env::var_os("SHIN_REFERENCE_ASSET_IO_ERRORS_ONLY").is_some() {
            crate::asset::system::verify_asset_load_failures(
                server.context(),
                &original_scenario,
            );
            assert!(
                !disk_path.exists(),
                "Asset IO checks must not write progress"
            );
            return;
        }
        if std::env::var_os("SHIN_REFERENCE_RESTORE_ERROR_ONLY").is_some() {
            mixed_restore::verify_scene_load_errors(
                &mut context,
                &server,
                &assets,
                &original_scenario,
                &target,
            );
        } else if std::env::var_os("SHIN_REFERENCE_PUBLISHED_LOOP_ONLY").is_some() {
            published_loop::verify(&mut context, &server, &assets, &original_scenario);
        } else if std::env::var_os("SHIN_REFERENCE_POST_INSTALL_FAILURE_ONLY").is_some() {
            mixed_restore::verify_post_install_failure(
                &mut context,
                &server,
                &assets,
                &original_scenario,
            );
        } else if std::env::var_os("SHIN_REFERENCE_BLOCKED_READ_ONLY").is_some() {
            blocked_read::verify(&mut context, &assets, &original_scenario);
        } else if std::env::var_os("SHIN_REFERENCE_AUTHORED_MOVIE_AFTER_LOAD").is_some() {
            authored_movie_load::verify(
                &mut context,
                &server,
                &assets,
                &original_scenario,
                &view,
            );
        } else {
            authored_restore::verify(&mut context, &server, &assets, &original_scenario, &view);
        }
        assert!(
            !disk_path.exists(),
            "Authored restoration must not write progress"
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_SCENE_REUSE_ONLY").is_some() {
        scene_reuse::verify(&mut context, &server, &assets, &original_scenario);
        assert!(
            !disk_path.exists(),
            "retained-scene fixture must not write working progress"
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MASK_RESTORE_ONLY").is_some() {
        saved_masks::verify(&mut context, &server, &assets, &original_scenario);
        assert!(!disk_path.exists(), "Mask fixture must not write progress");
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_MIXED_RESTORE_ONLY").is_some() {
        mixed_restore::verify(&mut context, &server, &assets, &original_scenario);
        assert!(!disk_path.exists(), "Mixed fixture must not write progress");
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_TITLE_AUDIO_ONLY").is_some() {
        crate::adv::reference_tests::live_save::assert_title_return_retires_story_effects(
            &server,
            &assets,
            &mut context,
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_SAVE_PENDING_ONLY").is_some() {
        if std::env::var_os("SHIN_REFERENCE_TITLE_RETURN_FADE_ONLY").is_some() {
            crate::adv::reference_tests::live_save::assert_title_return_fade(
                &server,
                &assets,
                &mut context,
                &target,
            );
            return;
        }
        if std::env::var_os("SHIN_REFERENCE_CHARACTER_SCROLL_ONLY").is_some() {
            crate::adv::characters::screen::assert_reference_character_scroll(
                &server,
                &original_scenario,
                &mut context,
                &target,
            );
            return;
        }
        #[cfg(unix)]
        if std::env::var_os("SHIN_REFERENCE_AUTO_SAVE_FAILURE_ONLY").is_some() {
            crate::adv::reference_tests::live_save::assert_automatic_save_failure(
                &server,
                &assets,
                &mut context,
                &target,
                false,
            );
            return;
        }
        crate::adv::reference_tests::live_save::assert_live_save_menu(
            &server,
            &assets,
            &mut context,
            &target,
        );
        crate::title::reference_pending_config_save(&server, &mut context, &target);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_ACTIVE_RESTORE_ONLY").is_some() {
        active_restore::verify(&mut context, &server, &assets, &original_scenario);
        assert!(
            !disk_path.exists(),
            "Active restoration test must not write working progress"
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_BACKLOG_ONLY").is_some() {
        verify_live_backlog(
            &mut context,
            &server,
            &assets,
            adv.adv_state.audio_manager.clone(),
            FrameId::default(),
        );
        assert!(
            !disk_path.exists(),
            "Focused backlog fixture must not write progress"
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_FONT_ONLY").is_some() {
        verify_native_font_fragments(&mut context, &target, &view);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_TEXTURE_COLOR_ONLY").is_some() {
        verify_native_texture_color_operations(
            &mut context,
            &target,
            &view,
            &[
                shin_render::LayerFragmentShader::Mono,
                shin_render::LayerFragmentShader::Fill,
                shin_render::LayerFragmentShader::Negative,
                shin_render::LayerFragmentShader::Gamma,
            ],
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_TEXTURE_NEGATIVE_ONLY").is_some() {
        verify_native_texture_color_operations(
            &mut context,
            &target,
            &view,
            &[shin_render::LayerFragmentShader::Negative],
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_BLEND_ONLY").is_some() {
        verify_native_blend_fragment(&mut context, &target, &view);
        verify_native_button_fragment(&mut context, &target, &view);
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_AUDIO_SKIP_ONLY").is_some() {
        crate::adv::command::reference_live_audio_skip_dispatch(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: FrameId::default(),
                delta_ticks: Ticks::from_f32(6.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            &original_scenario,
            &mut adv.adv_state,
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_INTERACTIVE_PUBLICATION_ONLY").is_some() {
        crate::adv::command::reference_live_interactive_publication(
            &mut context,
            &server,
            &original_scenario,
            &mut adv.adv_state,
        );
        return;
    }
    if std::env::var_os("SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY").is_some()
        || std::env::var_os("SHIN_REFERENCE_ORIGINAL_INTERACTIVE_ONLY").is_some()
    {
        assert_eq!(
            reference_updated_scenario(&original_scenario),
            std::env::var_os("SHIN_REFERENCE_UPDATED_INTERACTIVE_ONLY").is_some()
        );
        drop(adv);
        assert!(!disk_path.exists());
        for (kind, last_choice) in [
            (1, 0),
            (0, 0),
            (0, 1),
            (2, 0),
            (2, 1),
            (2, 2),
            (2, 3),
            (3, 0),
            (3, 1),
        ] {
            verify_live_authored_interactive_checkpoint(
                &mut context,
                &server,
                &assets,
                &original_scenario,
                kind,
                last_choice,
            );
        }
        return;
    }
    if let Ok(entry) = std::env::var("SHIN_REFERENCE_LIVE_ENTRY") {
        crate::layer::message_layer::reference_live_msgclose(
            &mut context,
            &server,
            &original_scenario,
            &mut adv.adv_state,
        );
        drop(adv);
        assert!(
            !disk_path.exists(),
            "The unused restore fixture must not write a save before its first update"
        );
        verify_live_authored_skip_playback(
            &mut context,
            &server,
            &assets,
            &original_scenario,
            entry.parse().unwrap(),
        );
        return;
    }
    for (kind, last_choice) in [
        (1, 0),
        (0, 0),
        (0, 1),
        (2, 0),
        (2, 1),
        (2, 2),
        (2, 3),
        (3, 0),
        (3, 1),
    ] {
        verify_live_authored_interactive_checkpoint(
            &mut context,
            &server,
            &assets,
            &original_scenario,
            kind,
            last_choice,
        );
    }
    crate::adv::command::reference_live_layer_transitions(
        &mut context,
        &server,
        &original_scenario,
        &mut adv.adv_state,
    );
    crate::layer::LayerGroup::<crate::layer::user::TileLayer>::reference_whole_layer_update_guards(
        &mut context,
        &mut crate::update::AdvUpdateContext {
            frame_id: FrameId::default(), delta_ticks: Ticks::ZERO,
            asset_server: &server, device: &device, queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
        },
    );
    crate::layer::LayerGroup::<crate::layer::user::TileLayer>::reference_delayed_removal(
        &mut context,
        &mut crate::update::AdvUpdateContext {
            frame_id: FrameId::default(),
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
        },
    );
    crate::layer::RootLayerGroup::new(
        assets.fonts.clone(),
        assets.messagebox_textures.clone(),
        crate::audio::VoicePlayer::new(adv.adv_state.audio_manager.clone(), server.clone()),
    )
    .reference_retained_message_ownership(
        &mut context,
        &mut crate::update::AdvUpdateContext {
            frame_id: FrameId::default(),
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
        },
        &original_scenario,
    );
    verify_live_root_back_selection(
        &mut context,
        &server,
        &assets,
        adv.adv_state.audio_manager.clone(),
    );
    {
        let mut mask_live = Adv::new(
            adv.adv_state.audio_manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        crate::adv::command::reference_live_publication_constructors(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        {
            let mut movie_live = super::super::AdvState::new(
                Arc::new(shin_audio::AudioManager::new_for_tests()),
                server.clone(),
                assets.clone(),
            );
            crate::adv::command::reference_live_movie_publication(
                &mut context,
                &server,
                &original_scenario,
                &mut movie_live,
            );
        }
        crate::layer::user::murderer::accusation::reference_retained_wipe_ownership(
            &mut context,
            &server,
        );
        crate::adv::command::reference_live_interactive_publication(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::layer::message_layer::reference_live_frame_guard(
            &mut mask_live.adv_state.root_layer_group.message_layer_mut(),
            &mut crate::update::AdvUpdateContext {
                frame_id: FrameId::default(),
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                device: &device,
                queue: &queue,
                are_animations_allowed: true,
                voice_callback: None,
                movie_volume_settings: crate::audio::MovieVolumeSettings::default(),
            },
        );
        crate::adv::command::reference_live_maskload_policy(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::adv::command::reference_live_maskunload(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::adv::command::reference_live_layerswap(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::layer::message_layer::reference_live_msgsync(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::layer::message_layer::reference_live_msgclose(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
        crate::adv::command::reference_live_delayed_layerunload(
            &mut context,
            &server,
            &original_scenario,
            &mut mask_live.adv_state,
        );
    }
    let mut frame = FrameId::default();
    for _ in 0..2000 {
        frame.advance();
        adv.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            enum_map::EnumMap::default(),
        );
        if !adv.is_loading_saved_scene() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!adv.is_loading_saved_scene());
    assert_eq!(adv.saved_scene_load_error(), None);
    assert_eq!(adv.vm_state.save_position, 1);
    assert_eq!(
        adv.adv_state
            .root_layer_group()
            .properties()
            .get_value(LayerProperty::TranslateY),
        30.0
    );
    assert!(!adv.vm_state.save_vectors.seen_messages.contains(1));
    assert!(!adv.vm_state.can_skip_current_message());
    assert!(adv.adv_state.message_layer().recv_sync_is_waiting(-1));
    let plane = PlaneId::new(0);
    let bank = adv
        .vm_state
        .layers
        .layerbank_allocator
        .get_layerbank_id(plane, LayerId::new(1))
        .unwrap();
    assert_eq!(
        adv.adv_state
            .plane_layer_group(plane)
            .get_layer(bank)
            .unwrap()
            .properties()
            .get_value(LayerProperty::TranslateX),
        300.0
    );
    let mut skip_input =
        enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
    skip_input[crate::app::AppAction::HoldSkip].is_held = true;
    crate::adv::reference_tests::live_save::assert_live_save_menu(
        &server,
        &assets,
        &mut context,
        &target,
    );
    frame.advance();
    adv.update(
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: &server,
            pre_render: &mut context,
        },
        skip_input,
    );
    assert_eq!(adv.vm_state.save_position, 1);
    assert!(!adv.vm_state.save_vectors.seen_messages.contains(1));
    assert!(!adv.vm_state.can_skip_current_message());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: TextureTargetKind::Screen,
            },
            Some(DepthStencilTarget { view: &depth_view }),
            "Restored scene",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        adv.render(&mut pass);
    }
    drop(context);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 1920 * 1080 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1920 * 4),
                rows_per_image: Some(1080),
            },
        },
        size,
    );
    let mut uploads = device.create_command_encoder(&Default::default());
    dynamic.finish(&mut uploads);
    queue.submit([uploads.finish(), encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    receive.recv().unwrap().unwrap();
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .expect("Readback mapping must be ready");
    let marker = &pixels[(540 * 1920 + 1260) * 4..][..4];
    assert!(
        marker[0] > 100 && marker[1] < 10 && marker[2] < 10,
        "Restored translated tile: {marker:?}"
    );
    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
    for pixel in pixels.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write("/tmp/shin-restored-scene.ppm", ppm).unwrap();
    assert!(
        pixels[(780 * 1920) * 4..]
            .chunks_exact(4)
            .filter(|p| p[0] > 150 && p[1] > 150 && p[2] > 150)
            .count()
            > 100,
        "Restored message must have visible glyphs"
    );
    assert!(adv.adv_state.message_layer_mut().try_advance());
    assert!(adv.can_skip_current_message());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut context = PreRenderContext {
        breakup_texture: None,
        device: &device,
        queue: &queue,
        resize_source: &resize,
        sampler_store: &samplers,
        depth_stencil: DepthStencilTarget { view: &depth_view },
        pipeline_storage: &mut pipelines,
        dynamic_buffer: &mut dynamic,
        encoder: &mut encoder,
    };
    // Capture the real reconstructed scene while its message is still visible.
    // Compare independently rendered ScreenLayer, and prove root/message state
    // and prepared draw caches are unchanged after temporary removal.
    {
        use crate::layer::{DrawableLayer as _, Layer as _, render_params::TransformParams};
        use shin_core::primitives::color::UnormColor;
        use shin_render::{ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices};
        let render_root = |adv: &Adv, context: &mut PreRenderContext, screen_only: bool| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "Modal capture comparison",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                if screen_only {
                    crate::layer::render_layer_without_bg(
                        &mut pass,
                        &adv.adv_state
                            .root_layer_group
                            .properties()
                            .get_composed_transform_params(&TransformParams::default()),
                        adv.adv_state.screen_layer(),
                        0,
                    );
                } else {
                    adv.adv_state.render_scene(&mut pass);
                }
            }
            crate::layer::user::readback_test_texture(context, &target)
        };
        adv.adv_state
            .root_layer_group
            .pre_render(&mut context, &TransformParams::default());
        let before = render_root(&adv, &mut context, false);
        let snapshot = adv
            .adv_state
            .root_layer_group
            .capture_modal_snapshot(&mut context);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                None,
                "Captured modal scene",
            );
            QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_tex_box(0.0, 0.0, 1.0, 1.0)
                .with_color(UnormColor::WHITE)
                .render_sprite(
                    &mut pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                    snapshot.as_texture_source(),
                    crate::render::top_left_projection_matrix(),
                );
        }
        let captured = crate::layer::user::readback_test_texture(&mut context, &target);
        let after = render_root(&adv, &mut context, false);
        assert!(
            before == after,
            "Modal capture changed live message/root rendering"
        );
        let screen = render_root(&adv, &mut context, true);
        let mismatch: Vec<_> = captured
            .chunks_exact(4)
            .zip(screen.chunks_exact(4))
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(12)
            .map(|(i, (a, b))| (i % 1920, i / 1920, a, b))
            .collect();
        let max_capture_error = captured
            .iter()
            .zip(&screen)
            .map(|(&a, &b)| (a as i32 - b as i32).abs())
            .max()
            .unwrap();
        assert!(
            max_capture_error == 0,
            "Modal snapshot/screen error{max_capture_error}, samples{mismatch:?}"
        );
        let excluded = before
            .chunks_exact(4)
            .zip(captured.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            excluded > 100,
            "Fixture must show excluded message artwork: {excluded} pixels"
        );
        let mut backdrop = adv.adv_state.capture_modal_backdrop(&mut context);
        backdrop.update(Ticks::from_f32(10.0));
        assert!(!backdrop.needs_scene());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                None,
                "Actual captured ADV backdrop",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            backdrop.render(&mut pass);
        }
        let dimmed = crate::layer::user::readback_test_texture(&mut context, &target);
        let mut checked = 0;
        for y in (10..1070).step_by(17) {
            for x in (10..1910).step_by(17) {
                let at = (y * 1920 + x) * 4;
                let center = &screen[at..at + 4];
                let constant = (-7..=7).all(|dy| {
                    (-7..=7).all(|dx| {
                        let pos = (((y as i32 + dy) * 1920 + x as i32 + dx) as usize) * 4;
                        &screen[pos..pos + 4] == center
                    })
                });
                if constant {
                    for c in 0..3 {
                        let expected = (center[c] as f32 * 128.0 / 255.0).round() as i32;
                        assert!(
                            (dimmed[at + c] as i32 - expected).abs() <= 1,
                            "Actual ADV captured backdrop pixel({x},{y}) c{c}"
                        );
                    }
                    assert_eq!(dimmed[at + 3], 255);
                    checked += 1;
                }
            }
        }
        assert!(checked > 100);
        assert!(before == render_root(&adv, &mut context, false));
        eprintln!(
            "Actual modal capture: excluded {excluded} overlay pixels; {checked} constant-region blur/tint checks; full snapshot/screen and before/after root byte equality"
        );
    }
    // Exercise native Y opening, actual ADV ownership and real child dispatch.
    {
        use crate::adv::system_menu::SystemMenuPhase as Phase;
        use crate::app::AppAction as A;
        let mut live = Adv::new(
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        // The authored character tables accompany the synthetic test bytecode.
        live.scenario = original_scenario.clone();
        live.vm_state.scene_title = "10/4（土）8:00 新島空港到着".into();
        live.vm_state.information.select_character_grid(0, 0);
        live.vm_state.information.acquire_tips(&[7, 2, 1]);
        let button = |action| {
            let mut input = enum_map::EnumMap::<A, shin_input::ActionState>::default();
            input[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
            input
        };
        let config_save_path =
            std::env::temp_dir().join(format!("shin-live-config-{}.dat", std::process::id()));
        assert!(!config_save_path.exists());
        live.configure_save_file(config_save_path.clone(), None)
            .unwrap();
        let mut config_pixels_checked = false;
        let mut tips_pixels_checked = false;
        let mut step = |live: &mut Adv, delta: f32, input, position: glam::Vec2, held: bool| {
            live.sample_modal_pointer(position, (1920, 1080), held, delta as f64 / 60.0);
            live.handle_pointer_state(position, false, held, delta as f64 / 60.0);
            frame.advance();
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: &server,
                    pre_render: &mut context,
                },
                input,
            );
            live.advance_play_clock(Ticks::from_f32(delta));
            // Exercise the actual ADV composition during opening and both
            // real children, rather than rendering only the returned scene.
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Live system-menu host frame",
            );
            live.render(&mut pass);
            drop(pass);
            if !config_pixels_checked
                && live
                    .system_menu
                    .as_ref()
                    .and_then(|menu| menu.reference_config_if_any())
                    .is_some_and(|child| child.reference_dialog_is_open())
            {
                let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
                let mut glyph_pixels = 0;
                for y in 475..525 {
                    for x in 430..1500 {
                        let at = (y * 1920 + x) * 4;
                        if pixels[at..at + 3].iter().all(|c| *c > 245) {
                            glyph_pixels += 1;
                        }
                    }
                }
                assert!(
                    glyph_pixels > 100,
                    "Live Config reset prompt was not rendered ({glyph_pixels})"
                );
                std::fs::write("/private/tmp/shin-live-config-reset.rgba", &pixels).unwrap();
                config_pixels_checked = true;
            }
            if !tips_pixels_checked
                && live
                    .system_menu
                    .as_ref()
                    .and_then(|menu| menu.reference_tips_if_any())
                    .and_then(|child| child.model())
                    .is_some_and(|model| model.progress() == 1.0 && model.scroll() == 32.0)
            {
                let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
                let mut glyph_pixels = 0;
                for y in 220..880 {
                    for x in 620..1810 {
                        let at = (y * 1920 + x) * 4;
                        let rgb = &pixels[at..at + 3];
                        if rgb.iter().all(|c| *c > 245) {
                            glyph_pixels += 1;
                        }
                    }
                }
                assert!(
                    glyph_pixels > 100,
                    "Actual Information body has no visible outlined glyphs ({glyph_pixels})"
                );
                eprintln!(
                    "Live Information: {glyph_pixels} white body glyph pixels at native scroll32"
                );
                tips_pixels_checked = true;
            }
        };
        let p = glam::vec2(30.0, 72.0 + 5.0 * 128.0);
        // Let the real asynchronous Tile load complete before measuring
        // message/VM ownership; opening over an unfinished loader is not an
        // adequate oracle for pausing a running story.
        for _ in 0..2000 {
            step(&mut live, 1.0, Default::default(), p, false);
            if live.vm_state.save_position == 1
                && live.adv_state.message_layer().recv_sync_is_waiting(-1)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(live.vm_state.save_position, 1);
        step(&mut live, 100.0, Default::default(), p, false);
        assert!(live.adv_state.message_layer().is_interested_in_input());
        assert!(!live.is_finished());
        let save_position = live.vm_state.save_position;
        let message_clock = live.adv_state.message_layer().reference_modal_clock().1;
        // Lower real Y and simultaneous X/Up: Y opens before other story controls.
        let mut raw = shin_input::RawInputState::new();
        raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::Y] = true;
        raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::X] = true;
        raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::Up] = true;
        let lowered = <A as shin_input::Action>::lower(&raw);
        let open = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
            is_clicked: lowered[action],
            is_clicked_or_repeated: lowered[action],
            ..Default::default()
        });
        live.vm_state.system_controls_allowed = false;
        step(&mut live, 0.0, open, p, false);
        assert!(live.system_menu.is_none());
        live.vm_state.system_controls_allowed = true;
        live.adv_state.playback_modes.skip = true;
        step(&mut live, 0.0, open, p, false);
        assert!(live.system_menu.is_none() && !live.adv_state.playback_modes.skip);
        step(&mut live, 10.0, open, p, false);
        assert!(live.system_menu.is_some() && live.adv_state.backlog.is_none());
        assert!(!live.pending_previous_scene);
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        assert_eq!(
            live.adv_state.modal_backdrop.as_ref().unwrap().progress(),
            1.0
        );
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .screen
                .model
                .render_rect(0)
                .unwrap()
                .x,
            -480.0
        );
        assert_eq!(
            live.adv_state.message_layer().reference_modal_clock().1,
            message_clock
        );
        assert_eq!(live.vm_state.save_position, save_position);
        let captured_title = live
            .system_menu
            .as_ref()
            .unwrap()
            .screen
            .snapshot()
            .scene_title
            .clone();
        live.vm_state.scene_title = "Next pending chapter".into();
        step(&mut live, 22.0, button(A::Enter), p, false); // Opening ignores confirm.
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Opening
        );
        // The first idle frame accepts input before advancing clocks.
        step(&mut live, 0.0, button(A::AnyDown), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            1
        );
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .screen
                .snapshot()
                .scene_title,
            captured_title
        );
        // A desktop viewport resize cancels a captured row before replacing
        // its physical transform. The subsequent Up must not confirm it.
        step(&mut live, 0.0, Default::default(), p, true);
        step(&mut live, 10.0, Default::default(), p, true);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            5
        );
        live.sample_modal_pointer(p * 0.5, (960, 540), true, 0.0);
        step(&mut live, 1.0, Default::default(), p, false);
        step(&mut live, 7.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Active
        );
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        // Native contact capture selects Manual and blocks simultaneous Cancel/Act.
        let mut simultaneous = button(A::Cancel);
        simultaneous[A::Act].is_clicked = true;
        step(&mut live, 0.0, simultaneous, p, true);
        step(&mut live, 10.0, simultaneous, p, true);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            5
        );
        step(
            &mut live,
            1.0,
            simultaneous,
            glam::vec2(1800.0, 80.0),
            false,
        );
        step(&mut live, 7.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert!(live.pending_system_menu_request().is_none());
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Child
        );
        assert_eq!(
            live.system_menu
                .as_mut()
                .unwrap()
                .wait_reference_manual()
                .model()
                .unwrap()
                .progress(),
            0.0
        );
        step(&mut live, 22.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu
                .as_mut()
                .unwrap()
                .wait_reference_manual()
                .model()
                .unwrap()
                .progress(),
            1.0
        );
        assert_eq!(
            live.adv_state.message_layer().reference_modal_clock().1,
            message_clock
        );
        step(&mut live, 7.0, button(A::Cancel), p, false);
        step(&mut live, 0.0, Default::default(), p, false); // Manual response0.
        assert!(live.system_menu.as_ref().unwrap().has_child());
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Child
        );
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        step(&mut live, 5.0, Default::default(), p, false);
        step(&mut live, 5.0, Default::default(), p, false); // Manual response1.
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Opening
        );
        step(&mut live, 22.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            5
        );
        for _ in 0..2 {
            step(&mut live, 0.0, button(A::Up), p, false);
        }
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            3
        );
        step(&mut live, 7.0, button(A::Enter), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert!(live.system_menu.as_ref().unwrap().has_child());
        step(&mut live, 10.0, Default::default(), p, false);
        step(&mut live, 7.0, button(A::Cancel), p, false);
        for _ in 0..4 {
            if !live.system_menu.as_ref().unwrap().has_child() {
                break;
            }
            step(&mut live, 10.0, Default::default(), p, false);
        }
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        step(&mut live, 22.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Active
        );
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        // Information dispatch retains acquired IDs in scenario order,
        // loads authored Tips/system atlases, and returns through callbacks0/1.
        step(&mut live, 0.0, button(A::AnyDown), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            4
        );
        step(&mut live, 7.0, button(A::Enter), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        let child = live.system_menu.as_mut().unwrap().wait_reference_tips();
        let tips = child.model().unwrap();
        assert_eq!(tips.progress(), 0.0);
        assert_eq!(
            tips.rows()
                .iter()
                .map(|r| r.scenario_index)
                .collect::<Vec<_>>(),
            [1, 2, 7]
        );
        assert!(tips.content_height() > 730.0);
        live.vm_state.information.acquire_tips(&[0, 3]); // Child keeps captured list.
        step(&mut live, 10.0, Default::default(), p, false);
        live.set_character_scroll_axis(1.0);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_tips()
                .model()
                .unwrap()
                .scroll(),
            32.0
        );
        let mut down = button(A::AnyDown);
        down[A::Cancel].is_clicked = true;
        step(&mut live, 0.0, down, p, false);
        let tips = live
            .system_menu
            .as_ref()
            .unwrap()
            .reference_tips()
            .model()
            .unwrap();
        assert_eq!(tips.selected(), 1);
        assert_eq!(tips.scroll(), 0.0);
        assert_eq!(tips.phase(), crate::adv::tips::TipsPhase::Active);
        assert_eq!(tips.rows().len(), 3);
        live.set_character_scroll_axis(0.0);
        step(&mut live, 0.0, button(A::Up), p, false);
        let body = glam::vec2(700.0, 500.0);
        step(&mut live, 0.0, button(A::Cancel), body, true); // Pointer capture consumes Cancel.
        step(
            &mut live,
            1.0,
            Default::default(),
            glam::vec2(700.0, 350.0),
            true,
        );
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_tips()
                .model()
                .unwrap()
                .scroll(),
            150.0
        );
        step(
            &mut live,
            1.0,
            Default::default(),
            glam::vec2(1800.0, 40.0),
            false,
        );
        assert!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_tips()
                .model()
                .unwrap()
                .scroll()
                > 150.0
        );
        let row = glam::vec2(100.0, 380.0);
        step(&mut live, 0.0, Default::default(), row, true);
        step(&mut live, 10.0, Default::default(), row, true); // Press silently selects captured row.
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_tips()
                .model()
                .unwrap()
                .selected(),
            2
        );
        step(
            &mut live,
            0.0,
            Default::default(),
            glam::vec2(1800.0, 40.0),
            false,
        );
        assert_eq!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_tips()
                .model()
                .unwrap()
                .phase(),
            crate::adv::tips::TipsPhase::Active
        );
        let close = glam::vec2(1764.0, 32.0);
        step(&mut live, 0.0, Default::default(), close, true);
        step(&mut live, 10.0, Default::default(), close, true);
        step(&mut live, 0.0, Default::default(), row, false);
        step(&mut live, 4.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false); // Response0 retains modal scopes.
        assert!(live.system_menu.as_ref().unwrap().has_child());
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Child
        );
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        step(&mut live, 5.0, Default::default(), p, false);
        step(&mut live, 5.0, Default::default(), p, false);
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        step(&mut live, 22.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            4
        );
        // Actual Config factory publishes VM/audio settings, owns its
        // reset dialog, writes changed Settings and returns through0/1.
        for _ in 0..2 {
            step(&mut live, 0.0, button(A::Up), p, false);
        }
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            2
        );
        let revision = live.vm_state.save_revision;
        let original_volume = live.vm_state.settings.v3_sysvol;
        step(&mut live, 7.0, button(A::Enter), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        let child = live.system_menu.as_mut().unwrap().wait_reference_config();
        assert_eq!(child.model().unwrap().progress(), 0.0);
        step(&mut live, 10.0, Default::default(), p, false);
        step(&mut live, 0.0, button(A::Right), p, false);
        assert_eq!(live.vm_state.settings.v3_sysvol, original_volume + 5);
        assert_eq!(
            live.adv_state.audio.system_player.volume_percent,
            original_volume + 5
        );
        step(&mut live, 7.0, button(A::ConfigReset), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        step(&mut live, 12.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert!(
            live.system_menu
                .as_ref()
                .unwrap()
                .reference_config()
                .reference_dialog_is_open()
        );
        step(&mut live, 0.0, button(A::Cancel), p, false);
        for _ in 0..30 {
            step(&mut live, 1.0, Default::default(), p, false);
        }
        assert_eq!(live.vm_state.settings.v3_sysvol, original_volume + 5);
        step(&mut live, 7.0, button(A::Cancel), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert!(live.system_menu.as_ref().unwrap().has_child());
        assert!(live.pending_save.is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while live.pending_save.is_some() {
            step(&mut live, 1.0, button(A::Cancel), p, false);
            assert_eq!(live.vm_state.save_position, save_position);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let saved = SaveData::decode(&std::fs::read(&config_save_path).unwrap()).unwrap();
        assert!(saved.settings.serialized_eq(&live.vm_state.settings));
        assert_eq!(live.vm_state.save_revision, revision);
        assert_eq!(live.vm_state.save_position, save_position);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Child
        );
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (1, 1)
        );
        step(&mut live, 10.0, Default::default(), p, false);
        assert!(!live.system_menu.as_ref().unwrap().has_child());
        step(&mut live, 22.0, Default::default(), p, false);
        step(&mut live, 0.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.selected(),
            2
        );
        step(&mut live, 0.0, button(A::Cancel), p, false);
        step(&mut live, 22.0, Default::default(), p, false);
        assert_eq!(
            live.system_menu.as_ref().unwrap().screen.model.phase(),
            Phase::Closing
        );
        step(&mut live, 0.0, Default::default(), p, false);
        assert!(live.system_menu.is_none());
        assert_eq!(
            (
                live.adv_state.ui_hide_count,
                live.adv_state.audio_pause_count
            ),
            (0, 0)
        );
        assert_eq!(
            live.adv_state.modal_backdrop.as_ref().unwrap().progress(),
            1.0
        );
        assert_eq!(live.vm_state.save_position, save_position);
        let mut during_fade = button(A::SystemMenu);
        during_fade[A::Act].is_clicked = true;
        step(&mut live, 5.0, during_fade, p, false);
        assert!(live.system_menu.is_none());
        assert_eq!(
            live.adv_state.modal_backdrop.as_ref().unwrap().progress(),
            0.5
        );
        assert!(!live.is_finished());
        step(&mut live, 5.0, Default::default(), p, false);
        assert!(live.adv_state.modal_backdrop.is_none());
        assert_eq!(live.vm_state.save_position, save_position);
        assert!(live.play_ticks() > 100.0 * 60.0 / 60.0);
        drop(step);
        assert!(
            config_pixels_checked,
            "Config dialog readback was not exercised"
        );
        std::fs::remove_file(config_save_path).unwrap();
        assert!(
            tips_pixels_checked,
            "Information readback was not exercised"
        );
        // Actual ADV render path returns to the live story after fade-out.
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Native menu host return",
            );
            live.render(&mut pass);
        }
        let returned = crate::layer::user::readback_test_texture(&mut context, &target);
        assert!(
            returned
                .chunks_exact(4)
                .filter(|p| p[0] > 150 && p[1] > 150 && p[2] > 150)
                .count()
                > 100
        );
        eprintln!(
            "Native ADV menu host: real Y/X/Up priority, script/mode gate, preloaded artwork, retained title, paused VM/message, pointer Manual dispatch, response0/1 ownership, Characters round trip, final release before backdrop fade and story return pass"
        );
    }
    frame.advance();
    adv.update(
        &mut UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: &server,
            pre_render: &mut context,
        },
        enum_map::EnumMap::default(),
    );
    assert!(adv.is_finished());
    assert_eq!(adv.vm_state.save_position, 2);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while adv.pending_title_setup().is_none() {
        frame.advance();
        adv.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            Default::default(),
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let disk_save = SaveData::decode(&std::fs::read(&disk_path).unwrap()).unwrap();
    assert_eq!(disk_save.play_seconds, 123);
    assert_eq!(
        adv.play_ticks(),
        7439.5,
        "scene reconstruction must preserve global play time"
    );
    assert!(disk_save.save_vectors.seen_messages.contains(1));
    let saved_entry = &disk_save.auto_save_slot.as_ref().unwrap().entry;
    assert_eq!(saved_entry.save_position, 2);
    assert_eq!(saved_entry.random_seed, 42);
    let restored =
        ReconstructedScene::from_save(&assets.scenario, saved_entry, &disk_save, 100).unwrap();
    assert_eq!(restored.state.save_position, 2);
    // Layout timing uses the saved speed, while inline @s remains authoritative.
    let params = crate::layer::message_layer::MsgsetParams {
        flags: crate::layer::message_layer::MessageFlags::empty(),
        messagebox_type: shin_core::vm::command::types::MessageboxType::Neutral,
        text_layout: shin_core::vm::command::types::MessageTextLayout::Justify,
        message_id: MessageId(2),
    };
    let mut message = adv.adv_state.message_layer_mut();
    let mut settings = disk_save.settings.clone();
    let mut times = Vec::new();
    for speed in [0, 50, 100] {
        settings.v9_msgspeed = speed;
        message.set_settings(&settings);
        message.on_msgset(
            &context,
            &assets.scenario,
            "@rRestored message.",
            params,
            false,
        );
        times.push(message.final_wait_time());
    }
    assert!(times[0] > times[1] && times[1] > times[2]);
    assert!(((times[0] - times[2]) / (times[1] - times[2]) - 2.0).abs() < 0.01);
    settings.v9_msgspeed = 0;
    message.set_settings(&settings);
    message.on_msgset(
        &context,
        &assets.scenario,
        "@r@s100.Restored message.",
        params,
        false,
    );
    assert!((message.final_wait_time() - times[2]).abs() < 0.0001);
    let no_text = crate::layer::message_layer::MsgsetParams {
        messagebox_type: shin_core::vm::command::types::MessageboxType::NoText,
        ..params
    };
    message.on_msgset(
        &context,
        &assets.scenario,
        "@rRestored message.",
        no_text,
        false,
    );
    assert!((message.final_wait_time() - times[2]).abs() < 0.0001);
    let mut auto_frames = Vec::new();
    for speed in [0, 100] {
        settings.v9_msgspeed = 100;
        settings.v10_skipspeed = speed;
        message.set_settings(&settings);
        message.on_msgset(
            &context,
            &assets.scenario,
            "@rRestored message.",
            params,
            false,
        );
        frame.advance();
        let update = crate::update::AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(60.0),
            asset_server: &server,
            device: &device,
            queue: &queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: (&settings).into(),
        };
        crate::update::AdvUpdatable::update(&mut *message, &update);
        assert!(message.recv_sync_is_waiting(-1));
        let mut update = crate::update::AdvUpdateContext {
            delta_ticks: Ticks::from_f32(1.0),
            ..update
        };
        let mut elapsed = 0;
        while message.recv_sync_is_waiting(-1) && elapsed < 300 {
            frame.advance();
            update.frame_id = frame;
            message.request_autoplay();
            crate::update::AdvUpdatable::update(&mut *message, &update);
            elapsed += 1;
        }
        assert!(elapsed < 300);
        auto_frames.push(elapsed);
    }
    assert!(auto_frames[0] > auto_frames[1] + 30);
    drop(message);
    // Exercise the actual LAYERWAIT poll against live root properties.
    use crate::adv::UpdatableCommand as _;
    use shin_core::vm::IntoRuntimeForm as _;
    for (options, fast_forward, expected_done) in [
        (
            shin_core::time::TweenOptions {
                prohibit_fast_forward: true,
                ignore_wait: false,
            },
            true,
            false,
        ),
        (
            shin_core::time::TweenOptions {
                prohibit_fast_forward: true,
                ignore_wait: true,
            },
            true,
            true,
        ),
        (shin_core::time::TweenOptions::default(), false, false),
        (shin_core::time::TweenOptions::default(), true, true),
    ] {
        let mut property = adv
            .adv_state
            .root_layer_group_mut()
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX);
        property.fast_forward_to(0.0);
        property.enqueue_with_options(
            100.0,
            shin_core::time::Tween::linear(Ticks::from_f32(10.0)),
            options,
        );
        drop(property);
        let runtime = C::LAYERWAIT(c::LAYERWAIT {
            layer_id: NumberSpec::new(NumberSpec::constant(-1).into_untyped()),
            wait_properties: shin_core::format::scenario::types::U8SmallNumberList::from_iter(
                [NumberSpec::new(NumberSpec::constant(0).into_untyped())],
            ),
        })
        .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: &server,
            pre_render: &mut context,
        };
        let start = crate::adv::command::apply_command_state_and_start(
            runtime,
            &mut update,
            &assets.scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
        );
        let crate::adv::CommandStartResult::Yield(mut wait) = start else {
            panic!("LAYERWAIT must yield");
        };
        let result = wait.update(
            &mut update,
            &assets.scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
            fast_forward,
        );
        assert_eq!(result.is_some(), expected_done);
        let value = adv
            .adv_state
            .root_layer_group()
            .properties()
            .get_value(LayerProperty::TranslateX);
        assert_eq!(
            value,
            if fast_forward && !options.prohibit_fast_forward {
                100.0
            } else {
                0.0
            }
        );
    }
    // Layer gallery links commit after loading; native BGM links commit
    // when the asynchronous request is accepted.
    let info = original_scenario.info_tables();
    let (picture_id, picture) = info
        .picture_info
        .iter()
        .enumerate()
        .find(|(_, item)| item.linked_cg_id.repr().is_some())
        .unwrap();
    let mut encoded = std::io::Cursor::new(Vec::new());
    0xc1u8.write_le(&mut encoded).unwrap();
    for value in [2, 2, 0] {
        NumberSpec::constant(value).write_le(&mut encoded).unwrap();
    }
    1u8.write_le(&mut encoded).unwrap();
    NumberSpec::constant(picture_id as i32)
        .write_le(&mut encoded)
        .unwrap();
    encoded.set_position(0);
    let runtime = Instruction::read_le(&mut encoded).unwrap();
    let Instruction::Command(runtime) = runtime else {
        panic!("Expected layer command")
    };
    let runtime = runtime.into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
    let mut update = UpdateContext {
        frame_started_at: std::time::Instant::now(),
        frame_id: frame,
        delta_ticks: Ticks::from_f32(1.0),
        asset_server: &server,
        pre_render: &mut context,
    };
    let start = crate::adv::command::apply_command_state_and_start(
        runtime,
        &mut update,
        &original_scenario,
        &mut adv.vm_state,
        &mut adv.adv_state,
    );
    let crate::adv::CommandStartResult::Yield(mut load) = start else {
        panic!("LAYERLOAD must yield");
    };
    assert!(!adv.vm_state.save_vectors.unlocked_cgs.contains(picture_id));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if load
            .update(
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
                false,
            )
            .is_some()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Picture load timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(adv.vm_state.save_vectors.unlocked_cgs.contains(picture_id));
    assert!(
        adv.vm_state
            .save_vectors
            .unlocked_cgs
            .contains(picture.linked_cg_id.repr().unwrap().into_i32() as usize)
    );
    // Exercise real replacement and cached LAYERLOAD paths, including
    // the nonzero flags present in the supplied scenario.
    let plane = adv.vm_state.layers.current_plane;
    let target = shin_core::vm::command::types::LayerId::new(2);
    let bank = adv
        .vm_state
        .layers
        .layerbank_allocator
        .get_layerbank_id(plane, target)
        .unwrap();
    adv.vm_state.layers.layerbanks[bank]
        .properties
        .set_property(LayerProperty::TranslateX, 345);
    adv.adv_state
        .plane_layer_group_mut(plane)
        .get_layer_mut(bank)
        .unwrap()
        .properties_mut()
        .property_tweener_mut(LayerProperty::TranslateX)
        .fast_forward_to(345.0);
    let initial_counter = adv.vm_state.layers.layer_load_counter;
    for (index, (flags, replacement)) in [
        (2, 0),
        (3, picture_id as i32),
        (3, picture_id as i32),
        (0, picture_id as i32),
    ]
    .into_iter()
    .enumerate()
    {
        let mut encoded = std::io::Cursor::new(Vec::new());
        0xc1u8.write_le(&mut encoded).unwrap();
        for value in [2, 2, flags] {
            NumberSpec::constant(value).write_le(&mut encoded).unwrap();
        }
        1u8.write_le(&mut encoded).unwrap();
        NumberSpec::constant(replacement)
            .write_le(&mut encoded)
            .unwrap();
        encoded.set_position(0);
        let Instruction::Command(runtime) = Instruction::read_le(&mut encoded).unwrap() else {
            panic!("Layer command")
        };
        let runtime = runtime.into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
        let crate::adv::CommandStartResult::Yield(mut load) =
            crate::adv::command::apply_command_state_and_start(
                runtime,
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
            )
        else {
            panic!("Layer load must yield")
        };
        assert_eq!(adv.adv_state.allow_running_animations, flags != 2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if load
                .update(
                    &mut update,
                    &original_scenario,
                    &mut adv.vm_state,
                    &mut adv.adv_state,
                    false,
                )
                .is_some()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Flagged layer load timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let layer = adv
            .adv_state
            .plane_layer_group(plane)
            .get_layer(bank)
            .unwrap();
        assert_eq!(
            layer.properties().get_value(LayerProperty::TranslateX),
            if flags == 0 { 0.0 } else { 345.0 }
        );
        assert_eq!(layer.properties().get_layer_id(), target);
        assert_eq!(
            adv.vm_state.layers.layer_load_counter,
            initial_counter + if index == 0 { 1 } else { 2 },
            "Only changed assets advance load order"
        );
        assert!(adv.adv_state.allow_running_animations);
    }
    let (bgm_id, bgm) = info
        .bgm_info
        .iter()
        .enumerate()
        .find(|(_, item)| item.linked_bgm_id.repr().is_some())
        .unwrap();
    let runtime = C::BGMPLAY(c::BGMPLAY {
        bgm_data_id: NumberSpec::new(NumberSpec::constant(bgm_id as i32).into_untyped()),
        fade_in_time: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
        no_repeat: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
        volume: NumberSpec::new(NumberSpec::constant(1000).into_untyped()),
    })
    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
    let start = crate::adv::command::apply_command_state_and_start(
        runtime,
        &mut update,
        &original_scenario,
        &mut adv.vm_state,
        &mut adv.adv_state,
    );
    assert!(matches!(start, crate::adv::CommandStartResult::Continue(_)));
    assert_eq!(adv.adv_state.audio.bgm_player.get_wait_status().bits(), 3);
    assert_eq!(adv.adv_state.audio.bgm_player.position(), Ticks::ZERO);
    adv.collect_gallery_unlocks();
    assert!(adv.vm_state.save_vectors.unlocked_bgms.contains(bgm_id));
    assert!(
        adv.vm_state
            .save_vectors
            .unlocked_bgms
            .contains(bgm.linked_bgm_id.repr().unwrap().into_i32() as usize)
    );
    if !bgm.display_name.as_str().is_empty() {
        assert!(
            !adv.adv_state.bgm_title.reference_motion().3,
            "new loading BGM must show its title immediately"
        );
    }
    adv.adv_state.bgm_title.update(Ticks::from_f32(5.0));
    let title_before_reuse = adv.adv_state.bgm_title.reference_motion();
    let repeated = C::BGMPLAY(c::BGMPLAY {
        bgm_data_id: NumberSpec::new(NumberSpec::constant(bgm_id as i32).into_untyped()),
        fade_in_time: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
        no_repeat: NumberSpec::new(NumberSpec::constant(0).into_untyped()),
        volume: NumberSpec::new(NumberSpec::constant(1000).into_untyped()),
    })
    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
    let repeated = crate::adv::command::apply_command_state_and_start(
        repeated,
        &mut update,
        &original_scenario,
        &mut adv.vm_state,
        &mut adv.adv_state,
    );
    assert!(matches!(
        repeated,
        crate::adv::CommandStartResult::Continue(_)
    ));
    assert_eq!(
        adv.adv_state.bgm_title.reference_motion(),
        title_before_reuse
    );
    assert!(
        adv.adv_state.pending_gallery_unlocks.is_empty(),
        "identity reuse must not enqueue gallery unlocks"
    );
    // Actual BGMWAIT and BGMSYNC commands must yield while loading.
    let mut pending_waits = Vec::new();
    for command in [
        C::BGMWAIT(c::BGMWAIT {
            unwanted_statuses: NumberSpec::new(NumberSpec::constant(1).into_untyped()),
        }),
        C::BGMSYNC(c::BGMSYNC {
            sync_time: NumberSpec::constant(60),
        }),
    ] {
        let start = crate::adv::command::apply_command_state_and_start(
            command.into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
            &mut update,
            &original_scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
        );
        let crate::adv::CommandStartResult::Yield(mut wait) = start else {
            panic!("music waits must yield");
        };
        assert!(
            wait.update(
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
                false
            )
            .is_none()
        );
        pending_waits.push(wait);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while adv
        .adv_state
        .audio
        .bgm_player
        .get_wait_status()
        .contains(shin_core::vm::command::types::AudioWaitStatus::LOADING)
    {
        adv.adv_state.apply_audio_settings(&adv.vm_state.settings);
        assert!(
            std::time::Instant::now() < deadline,
            "command BGM load timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        pending_waits[0]
            .update(
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
                false
            )
            .is_some()
    );
    // Complete the real source-clock wait too: loading alone cannot prove
    // that BGMSYNC uses the installed source clock or preserves pause.
    adv.adv_state.audio.bgm_player.reference_pause_source(true);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let paused_position = adv.adv_state.audio.bgm_player.position_micros();
    assert!(paused_position < 1_000_000);
    assert!(
        pending_waits[1]
            .update(
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
                false,
            )
            .is_none()
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        adv.adv_state.audio.bgm_player.position_micros(),
        paused_position
    );
    adv.adv_state.audio.bgm_player.reference_pause_source(false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if pending_waits[1]
            .update(
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
                false,
            )
            .is_some()
        {
            assert!(adv.adv_state.audio.bgm_player.position_micros() >= 1_000_000);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Actual BGMSYNC source-clock deadline timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    eprintln!(
        "BGMSYNC integer microsecond clock: real loading wait, paused source clock, resumed one-second deadline and actual token completion pass on the default audio output"
    );
    let movie_id = info
        .movie_info
        .iter()
        .position(|item| item.name.as_str().eq_ignore_ascii_case("door"))
        .unwrap();
    let movie_audio = adv.adv_state.audio_manager.clone();
    let load_movie = |repeat| {
        let device = device.clone();
        let server = server.clone();
        let audio = movie_audio.clone();
        let scenario = original_scenario.clone();
        shin_tasks::async_io::spawn(async move {
            crate::layer::user::UserLayer::load(
                &device,
                &server,
                &audio,
                &scenario,
                Default::default(),
                shin_core::vm::command::types::LayerType::Movie,
                (movie_id as i32, 1000, repeat, 0, 0, 0, 0, 0),
            )
            .await
        })
    };
    crate::layer::user::MovieLayer::assert_reference_mismatched_movie_durations(
        &mut context,
        &movie_audio,
    );
    crate::layer::user::MovieLayer::assert_reference_audio_restart_publication(
        &mut context,
        &server,
        &movie_audio,
    );
    let movie = shin_tasks::block_on(load_movie(0));
    let crate::layer::user::UserLayer::Movie(mut movie) = movie else {
        panic!("Movie layer")
    };
    movie.assert_reference_door_completion(&mut context, &server);
    movie.assert_reference_backend_retirement_with_clones(&mut context, &server);
    // Native skip releases a nonrepeating movie but retains repeating ones.
    for (repeat, preparation_failure) in [(0, false), (1, false), (1, true)] {
        let movie = shin_tasks::block_on(load_movie(repeat));
        let crate::layer::user::UserLayer::Movie(mut movie) = movie else {
            panic!("Movie layer")
        };
        if preparation_failure {
            movie = movie.reference_failed_preparation_with_still(&mut context, &server);
        }
        crate::layer::Layer::fast_forward(&mut movie);
        assert_eq!(movie.is_finished(), repeat == 0 || preparation_failure);
        if repeat == 1 && !preparation_failure {
            movie.assert_reference_loop_restarts(&mut context, &server);
        }
        let plane = adv.vm_state.layers.current_plane;
        let bank = adv
            .vm_state
            .layers
            .layerbank_allocator
            .alloc_layerbank(plane, LayerId::new(2))
            .unwrap();
        adv.adv_state
            .plane_layer_group_mut(plane)
            .add_layer(bank, movie.into());
        let runtime = C::MOVIEWAIT(c::MOVIEWAIT {
            layer_id: NumberSpec::new(NumberSpec::constant(2).into_untyped()),
            unwanted_statuses: NumberSpec::new(NumberSpec::constant(2).into_untyped()),
        })
        .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42));
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: &server,
            pre_render: &mut context,
        };
        let start = crate::adv::command::apply_command_state_and_start(
            runtime,
            &mut update,
            &original_scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
        );
        let crate::adv::CommandStartResult::Yield(mut wait) = start else {
            panic!("MOVIEWAIT must yield")
        };
        // A pending wait follows the plane captured at command creation.
        adv.vm_state.layers.current_plane = PlaneId::new(1);
        let result = wait.update(
            &mut update,
            &original_scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
            false,
        );
        assert_eq!(result.is_some(), repeat == 0 || preparation_failure);
        adv.vm_state.layers.current_plane = plane;
    }
    let alpha_id = info
        .movie_info
        .iter()
        .position(|item| item.name.as_str().eq_ignore_ascii_case("no03"))
        .unwrap();
    let alpha = shin_tasks::block_on(crate::layer::user::UserLayer::load(
        &device,
        &server,
        &adv.adv_state.audio_manager,
        &original_scenario,
        (&adv.vm_state.settings).into(),
        shin_core::vm::command::types::LayerType::Movie,
        (alpha_id as i32, 1000, 0, 0, 0, 0, 0, 0),
    ));
    let crate::layer::user::UserLayer::Movie(alpha) = alpha else {
        panic!("Transparent movie layer")
    };
    alpha.assert_reference_alpha_rendering(&mut context);
    for mode in 0..4 {
        let rain = shin_tasks::block_on(crate::layer::user::UserLayer::load(
            &device,
            &server,
            &adv.adv_state.audio_manager,
            &original_scenario,
            (&adv.vm_state.settings).into(),
            shin_core::vm::command::types::LayerType::Rain,
            (mode, 100, 1500, 0, 0, 0, 0, 0),
        ));
        let crate::layer::user::UserLayer::Rain(mut rain) = rain else {
            panic!("Rain layer")
        };
        rain.assert_reference_render(&mut context, &server);
    }
    let focus = shin_tasks::block_on(crate::layer::user::UserLayer::load(
        &device,
        &server,
        &adv.adv_state.audio_manager,
        &original_scenario,
        (&adv.vm_state.settings).into(),
        shin_core::vm::command::types::LayerType::FocusLine,
        (-200, 100, 0, 0, 0, 0, 0, 0),
    ));
    let crate::layer::user::UserLayer::FocusLine(mut focus) = focus else {
        panic!("FocusLine layer")
    };
    focus.assert_reference_render(&mut context, &server);
    let effect = shin_tasks::block_on(crate::layer::user::UserLayer::load(
        &device,
        &server,
        &adv.adv_state.audio_manager,
        &original_scenario,
        (&adv.vm_state.settings).into(),
        shin_core::vm::command::types::LayerType::Effect,
        (123, -99, 42, 0, 0, 0, 0, 0),
    ));
    let crate::layer::user::UserLayer::Effect(mut effect) = effect else {
        panic!("Effect layer")
    };
    effect.assert_reference_capture(&mut context, &server);
    for mode in 0..6 {
        let animation = shin_tasks::block_on(crate::layer::user::UserLayer::load(
            &device,
            &server,
            &adv.adv_state.audio_manager,
            &original_scenario,
            (&adv.vm_state.settings).into(),
            shin_core::vm::command::types::LayerType::Animation,
            (mode, 0, 0, 0, 0, 0, 0, 0),
        ));
        let crate::layer::user::UserLayer::Animation(mut animation) = animation else {
            panic!("Animation layer")
        };
        animation.assert_reference_render(&mut context, &server, mode);
    }
    let effect_textures: std::sync::Arc<crate::asset::effects::EffectTextures> =
        shin_tasks::block_on(server.load("/effects.txa")).unwrap();
    context.breakup_texture = Some(effect_textures.required("breakup"));
    let mut ghost_picture = shin_tasks::block_on(crate::layer::user::UserLayer::load(
        &device,
        &server,
        &adv.adv_state.audio_manager,
        &original_scenario,
        (&adv.vm_state.settings).into(),
        shin_core::vm::command::types::LayerType::Picture,
        (picture_id as i32, 0, 0, 0, 0, 0, 0, 0),
    ));
    crate::layer::user::TileLayer::assert_reference_ghosting(&mut context, &mut ghost_picture);
    crate::layer::user::TileLayer::assert_reference_dissolve(&mut context);
    crate::layer::user::TileLayer::assert_reference_mosaic(&mut context);
    crate::layer::user::TileLayer::assert_reference_raster(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_ripple(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_blur(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_zoom_blur(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_clipping(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_containers(&mut context, &server);
    crate::layer::ScreenLayer::assert_reference_nested_transitions(&mut context, &server);
    // Run information commands through the real start dispatcher. Reconstruction applies
    // the same VM state but must not enqueue historical notifications.
    {
        let retained = adv.vm_state.information.clone();
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            pre_render: &mut context,
        };
        for compiletime in [
            C::NOTIFYSET(c::NOTIFYSET {
                arg: NumberSpec::constant(2),
            }),
            C::CHARS(c::CHARS {
                arg1: NumberSpec::constant(169),
                arg2: NumberSpec::constant(1),
            }),
            C::CHARS(c::CHARS {
                arg1: NumberSpec::constant(169),
                arg2: NumberSpec::constant(1),
            }),
            C::TIPSGET(c::TIPSGET {
                tip_ids: shin_core::format::scenario::types::U8SmallNumberList::from_iter([
                    NumberSpec::constant(0),
                    NumberSpec::constant(0),
                    NumberSpec::constant(-1),
                ]),
            }),
        ] {
            let start = crate::adv::command::apply_command_state_and_start(
                compiletime.into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
            );
            assert!(matches!(start, crate::adv::CommandStartResult::Continue(_)));
        }
        assert_eq!(
            (
                adv.vm_state.information.character_grid,
                adv.vm_state.information.character_grid_argument
            ),
            (169, 1)
        );
        assert_eq!(adv.adv_state.notifications.begin_next(), Some(16));
        assert_eq!(adv.adv_state.notifications.begin_next(), Some(17));
        assert_eq!(adv.adv_state.notifications.begin_next(), None);
        let commits = adv.vm_state.persistent_commits_enabled;
        adv.vm_state.persistent_commits_enabled = false;
        crate::adv::command::apply_command_state_and_start(
            C::CHARS(c::CHARS {
                arg1: NumberSpec::constant(8),
                arg2: NumberSpec::constant(0),
            })
            .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
            &mut update,
            &original_scenario,
            &mut adv.vm_state,
            &mut adv.adv_state,
        );
        assert_eq!(adv.vm_state.information.character_grid, 8);
        assert_eq!(adv.adv_state.notifications.begin_next(), None);
        adv.vm_state.persistent_commits_enabled = commits;
        for style in [1, 2] {
            adv.adv_state.notifications = Default::default();
            adv.vm_state.information.notification_style = style;
            let before = adv.adv_state.audio.system_player.sounds.len();
            crate::adv::command::apply_command_state_and_start(
                C::CHARS(c::CHARS {
                    arg1: NumberSpec::constant(169),
                    arg2: NumberSpec::constant(0),
                })
                .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                &mut update,
                &original_scenario,
                &mut adv.vm_state,
                &mut adv.adv_state,
            );
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            assert_eq!(
                adv.adv_state.audio.system_player.sounds.len(),
                before + 1,
                "Native notify{style} must load and play once"
            );
            assert_eq!(
                adv.adv_state.notifications.update(Ticks::ZERO),
                None,
                "The active toast must not start its sound again"
            );
        }
        adv.adv_state.notifications = Default::default();
        adv.vm_state.information = retained;
    }
    crate::layer::user::TileLayer::assert_reference_notifications(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_character_portraits(&mut context, &server);
    crate::layer::user::TileLayer::assert_reference_character_buttons(&mut context, &server);
    crate::adv::characters::text::assert_reference_character_text(
        &mut context,
        &server,
        &original_scenario,
    );
    crate::layer::user::TileLayer::assert_reference_character_chrome(
        &mut context,
        &server,
        &original_scenario,
    );
    crate::layer::user::TileLayer::assert_reference_character_grid(
        &mut context,
        &server,
        &original_scenario,
    );
    crate::layer::user::TileLayer::assert_reference_whirl(
        &mut context,
        &server,
        &original_scenario,
    );
    crate::layer::user::TileLayer::assert_reference_glass(
        &mut context,
        &server,
        &original_scenario,
    );
    {
        let mut message = crate::layer::message_layer::MessageLayer::new(
            assets.fonts.clone(),
            assets.messagebox_textures.clone(),
            crate::audio::VoicePlayer::new(adv.adv_state.audio_manager.clone(), server.clone()),
        );
        message.assert_reference_outline(&mut context, &original_scenario);
        message.assert_reference_window_opacity(&mut context);
        let mut label_vm = crate::adv::VmState::new();
        // First timed chapter label in supplied main.snr (SAVEINFO0x9c102).
        label_vm
            .save_info
            .set_save_info(0, "Episode1 Legend of the golden witch".into());
        label_vm
            .save_info
            .set_save_info(1, "10/4（土）8:00 新島空港到着".into());
        let RuntimeCommand::AUTOSAVE(auto) =
            C::AUTOSAVE(c::AUTOSAVE {}).into_runtime_form(&shin_core::vm::VmCtx::new(0, 0))
        else {
            unreachable!()
        };
        auto.apply_state(&mut label_vm);
        assert_eq!(label_vm.scene_title, "10/4（土）8:00 新島空港到着");
        // Constructor has already copied VM9110. A subsequent SAVEINFO
        // must not change the chapter artwork used by the first poll.
        label_vm
            .save_info
            .set_save_info(1, "Pending next chapter".into());
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            pre_render: &mut context,
        };
        let _ = auto.start(
            &mut update,
            &original_scenario,
            &label_vm,
            (),
            &mut adv.adv_state,
        );
        drop(update);
        let label = std::mem::replace(
            &mut adv.adv_state.route_label,
            crate::adv::route_label::RouteLabel::new(),
        );
        crate::adv::route_label::RouteLabel::assert_reference_render(
            label,
            &mut context,
            &server,
        );
        crate::adv::bgm_title::BgmTitle::assert_reference_render(
            &mut context,
            &server,
            &original_scenario,
        );
    }
    // Exercise live hide-UI input, balanced modal scopes, and retained message clocks.
    {
        let mut live = crate::adv::Adv::new(
            adv.adv_state.audio_manager.clone(),
            server.clone(),
            assets.clone(),
            shin_core::vm::Scripter::new(&original_scenario, 0, 42),
        );
        // Hold authored VM dispatch with a real pending command. Finished
        // scenes deliberately reject input, so they cannot exercise live UI.
        {
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            };
            let crate::adv::CommandStartResult::Yield(command) =
                crate::adv::command::apply_command_state_and_start(
                    C::WAIT(c::WAIT {
                        wait_amount: NumberSpec::constant(1_000_000),
                        allow_interrupt: U8Bool(false),
                    })
                    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                    &mut update,
                    &original_scenario,
                    &mut live.vm_state,
                    &mut live.adv_state,
                )
            else {
                panic!("Live UI fixture WAIT must yield");
            };
            live.current_command = Some(command);
        }
        live.vm_state.settings.v11_disallowskipunread = false;
        live.adv_state.message_layer_mut().on_msgset(
            &context,
            &original_scenario,
            "@r@s100.Hidden UI reference.",
            crate::layer::message_layer::MsgsetParams {
                flags: crate::layer::message_layer::MessageFlags::empty(),
                messagebox_type: shin_core::vm::command::types::MessageboxType::Neutral,
                text_layout: shin_core::vm::command::types::MessageTextLayout::Justify,
                message_id: shin_core::format::scenario::instruction_elements::MessageId(2),
            },
            true,
        );
        live.adv_state.bgm_title.show(&server, "ab-sinthe");
        live.adv_state
            .route_label
            .show(&server, "10/4（土）8:00 新島空港到着");
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Live native UI hiding"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let (choice, mut select_command) = {
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            };
            let crate::adv::CommandStartResult::Yield(command) =
                crate::adv::command::apply_command_state_and_start(
                    C::SELECT(c::SELECT {
                        choice_set_base: 1,
                        choice_index: 1,
                        dest: Register::from_regular_register(0),
                        choice_visibility_mask: NumberSpec::constant(7),
                        choice_title: U16String::new("REFERENCE"),
                        variants: StringArray::new(["ルシファー", "レヴィアタン", "サタン"]),
                    })
                    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                    &mut update,
                    &original_scenario,
                    &mut live.vm_state,
                    &mut live.adv_state,
                )
            else {
                panic!("SELECT must yield");
            };
            (live.adv_state.choice_menu.take().unwrap(), command)
        };
        let quiz_layer =
            shin_tasks::block_on(crate::layer::user::quiz_layer::QuizLayer::load(&server, 5));
        let quiz_bank = live
            .vm_state
            .layers
            .layerbank_allocator
            .alloc_layerbank(PlaneId::new(0), LayerId::new(0))
            .unwrap();
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            pre_render: &mut context,
        };
        let crate::adv::CommandStartResult::Yield(quiz_command) =
            crate::adv::command::apply_command_state_and_start(
                C::QUIZ(c::QUIZ {
                    arg: NumberSpec::constant(0),
                    dest: Register::from_regular_register(0),
                })
                .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                &mut update,
                &original_scenario,
                &mut live.vm_state,
                &mut live.adv_state,
            )
        else {
            panic!("QUIZ must yield")
        };
        drop(update);
        let mut step = |live: &mut crate::adv::Adv, ticks: f32| {
            frame.advance();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(ticks),
                asset_server: &server,
                pre_render: &mut context,
            };
            if live
                .adv_state
                .choice_menu
                .as_ref()
                .is_some_and(|m| m.model.result.is_none())
            {
                assert!(
                    crate::adv::UpdatableCommand::update(
                        &mut select_command,
                        &mut update,
                        &original_scenario,
                        &mut live.vm_state,
                        &mut live.adv_state,
                        true
                    )
                    .is_none(),
                    "Live SELECT must keep yielding through opening,flash andexit even during fast-forward"
                );
            }
            let mut input =
                enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
            input[crate::app::AppAction::HoldSkip].is_held = live.adv_state.hidden_ui_active;
            live.update(&mut update, input);
            assert!(!live.is_finished());
            assert!(matches!(
                live.current_command,
                Some(crate::adv::ExecutingCommand::WAIT(_))
            ));
            drop(update);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Live native UI hiding",
                );
                live.render(&mut pass);
            }
            crate::layer::user::readback_test_texture(&mut context, &target)
        };
        let shown = step(&mut live, 100.0);
        assert!(live.adv_state.message_layer().is_interested_in_input());
        let controller_press = |button, up| {
            let mut raw = shin_input::RawInputState::new();
            raw.gamepads.unified.buttons[button] = true;
            raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::Up] = up;
            let lowered = <crate::app::AppAction as shin_input::Action>::lower(&raw);
            enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                is_clicked: lowered[action],
                is_clicked_or_repeated: lowered[action],
                is_held: lowered[action],
                ..Default::default()
            })
        };
        // Native story controls prioritize B hiding over simultaneous Up return.
        let hide = controller_press(shin_input::inputs::GamepadButton::B, true);
        let position = live.vm_state.save_position;
        let clock_before_modes = live.adv_state.message_layer().reference_modal_clock();
        live.vm_state.system_controls_allowed = false;
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::L, true),
            true,
        );
        assert!(!live.adv_state.playback_modes.skip && !live.pending_previous_scene);
        live.vm_state.system_controls_allowed = true;
        live.vm_state.current_event = Some(24);
        assert!(live.vm_state.allows_playback_mode_changes());
        assert!(!live.vm_state.allows_system_controls());
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::L, true),
            true,
        );
        assert!(live.adv_state.playback_modes.autoplay);
        assert!(!live.adv_state.playback_modes.skip && !live.pending_previous_scene);
        assert_eq!(live.pending_playback_sound, Some("son"));
        // Exercise the real cue bank without advancing the story under skip.
        live.adv_state
            .play_system_sound(&server, "son", Default::default());
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::R, false),
            true,
        );
        assert!(live.adv_state.playback_modes.skip && live.adv_state.playback_modes.autoplay);
        assert!(
            !live
                .adv_state
                .playback_modes
                .is_skipping(live.vm_state.allows_system_controls(), false)
        );
        live.vm_state.current_event = None;
        let mut both = controller_press(shin_input::inputs::GamepadButton::L, false);
        both[crate::app::AppAction::ToggleSkip] =
            controller_press(shin_input::inputs::GamepadButton::R, false)
                [crate::app::AppAction::ToggleSkip];
        live.handle_input(both, true);
        assert!(live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::R, false),
            true,
        );
        assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        let held = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
            is_held: controller_press(shin_input::inputs::GamepadButton::L, false)[action]
                .is_held,
            ..Default::default()
        });
        live.handle_input(held, true);
        assert!(!live.adv_state.playback_modes.skip);
        for button in [
            shin_input::inputs::GamepadButton::B,
            shin_input::inputs::GamepadButton::X,
            shin_input::inputs::GamepadButton::Y,
        ] {
            live.adv_state.playback_modes.skip = true;
            live.adv_state.playback_modes.autoplay = true;
            live.handle_input(controller_press(button, false), true);
            assert!(
                !live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay
            );
            assert!(!live.adv_state.hidden_ui_active);
            assert!(live.adv_state.backlog.is_none());
            assert!(!live.pending_backlog && !live.pending_previous_scene);
            assert_eq!(live.pending_playback_sound, Some("cancel"));
        }
        live.adv_state.playback_modes.skip = true;
        let mut backspace = enum_map::EnumMap::default();
        for action in [
            crate::app::AppAction::Cancel,
            crate::app::AppAction::PreviousScene,
        ] {
            backspace[action] = shin_input::ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        live.handle_input(backspace, true);
        assert!(!live.adv_state.playback_modes.skip && !live.pending_previous_scene);
        assert_eq!(live.vm_state.save_position, position);
        assert_eq!(
            live.adv_state.message_layer().reference_modal_clock(),
            clock_before_modes
        );
        let _ = step(&mut live, 0.0);
        assert!(live.pending_playback_sound.is_none());
        live.vm_state.system_controls_allowed = false;
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "Script gate blocks opening hidden UI"
        );
        live.vm_state.system_controls_allowed = true;
        live.vm_state.current_event = Some(24);
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "Event gate blocks opening hidden UI"
        );
        live.vm_state.current_event = None;
        live.adv_state.playback_modes.skip = true;
        live.adv_state.playback_modes.autoplay = true;
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "First B cancels active playback modes"
        );
        assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        live.handle_input(hide, true);
        assert!(live.adv_state.hidden_ui_active);
        assert_eq!(live.adv_state.ui_hide_count, 1);
        assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        let clock = live.adv_state.message_layer().reference_modal_clock().1;
        let halfway = step(&mut live, 5.0);
        assert_eq!(
            live.adv_state.message_layer().reference_modal_clock(),
            (0.5, clock)
        );
        assert_ne!(halfway, shown);
        let hidden = step(&mut live, 5.0);
        assert!(
            hidden.chunks_exact(4).all(|p| p == [0, 0, 0, 255]),
            "Hidden UI retains unobstructed background"
        );
        // A second modal owner must prevent the first release from exposing UI.
        live.adv_state.acquire_ui_hide();
        let mut dismiss = enum_map::EnumMap::default();
        dismiss[crate::app::AppAction::Enter] = shin_input::ActionState {
            is_clicked: true,
            ..Default::default()
        };
        live.handle_input(dismiss, true);
        assert!(!live.adv_state.hidden_ui_active);
        assert_eq!(live.adv_state.ui_hide_count, 1);
        assert_eq!(
            live.adv_state.message_layer().reference_modal_clock(),
            (0.0, clock),
            "Dismiss must not advance the message"
        );
        assert_eq!(step(&mut live, 400.0), hidden);
        live.adv_state.release_ui_hide();
        let restored = step(&mut live, 10.0);
        assert_eq!(live.adv_state.ui_hide_count, 0);
        assert!(live.adv_state.message_layer().reference_modal_clock().0 == 1.0);
        assert!(
            restored
                .chunks_exact(4)
                .filter(|p| *p != [0, 0, 0, 255])
                .count()
                > 1000
        );
        assert!(
            restored[..1920 * 96 * 4]
                .chunks_exact(4)
                .filter(|p| *p != [0, 0, 0, 255])
                .count()
                > 100,
            "Music title must resume after400 hidden ticks"
        );
        assert!(
            restored[1920 * 984 * 4..]
                .chunks_exact(4)
                .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245)
                .count()
                > 100,
            "Chapter label must resume after400 hidden ticks"
        );
        // Every supported dismissal consumes its input, even a confirm click.
        for button in [
            shin_input::inputs::GamepadButton::A,
            shin_input::inputs::GamepadButton::B,
            shin_input::inputs::GamepadButton::X,
            shin_input::inputs::GamepadButton::Y,
        ] {
            live.handle_input(hide, true);
            assert!(live.adv_state.hidden_ui_active);
            let before = live.adv_state.message_layer().reference_modal_clock().1;
            let position = live.vm_state.save_position;
            live.handle_input(controller_press(button, false), true);
            assert!(!live.adv_state.hidden_ui_active);
            assert_eq!(live.adv_state.ui_hide_count, 0);
            assert_eq!(
                live.adv_state.message_layer().reference_modal_clock().1,
                before
            );
            assert_eq!(live.vm_state.save_position, position);
            assert!(live.adv_state.backlog.is_none());
        }
        for action in [
            crate::app::AppAction::Cancel,
            crate::app::AppAction::PointerClick,
            crate::app::AppAction::HideMessage,
        ] {
            live.handle_input(hide, true);
            let _ = step(&mut live, 10.0);
            let before = live.adv_state.message_layer().reference_modal_clock().1;
            let mut input = enum_map::EnumMap::default();
            input[action] = shin_input::ActionState {
                is_clicked: true,
                ..Default::default()
            };
            if matches!(action, crate::app::AppAction::PointerClick) {
                let position = glam::vec2(800.0, 500.0);
                live.sample_modal_pointer(position, (1920, 1080), true, 0.016);
                live.handle_input(input, true);
                assert!(
                    live.adv_state.hidden_ui_active,
                    "Press does not restore hidden text"
                );
                live.sample_modal_pointer(position, (1920, 1080), false, 0.016);
                live.handle_input(Default::default(), true);
            } else {
                live.handle_input(input, true);
            }
            assert!(!live.adv_state.hidden_ui_active);
            assert_eq!(
                live.adv_state.message_layer().reference_modal_clock().1,
                before
            );
            let _ = step(&mut live, 10.0);
        }
        live.adv_state.message_layer_mut().close(false);
        live.adv_state.choice_menu = Some(choice);
        let _ = step(&mut live, 0.0);
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "Opening SELECT cannot hide"
        );
        let _ = step(&mut live, 100.0);
        let _ = step(&mut live, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            None
        );
        live.handle_pointer_state(glam::vec2(960.0, 400.0), false, false, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            None,
            "Passive pointer does not replace keyboard selection"
        );
        live.handle_pointer_state(glam::vec2(960.0, 400.0), true, true, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            Some(1)
        );
        let _ = step(&mut live, 3.5);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "Live pointer press must not finish SELECT"
        );
        live.handle_pointer_state(glam::Vec2::ZERO, false, true, 0.0);
        let _ = step(&mut live, 3.5);
        live.handle_pointer_state(glam::Vec2::ZERO, false, false, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "Live pointer cancellation must retain waiting SELECT"
        );
        let mut down = enum_map::EnumMap::default();
        down[crate::app::AppAction::AnyDown] = shin_input::ActionState {
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        let mut left_confirm = enum_map::EnumMap::default();
        left_confirm[crate::app::AppAction::Left] = shin_input::ActionState {
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        left_confirm[crate::app::AppAction::Enter] = shin_input::ActionState {
            is_clicked: true,
            ..Default::default()
        };
        live.handle_input(left_confirm, true);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            Some(0)
        );
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "Changed horizontal focus consumes simultaneous confirmation"
        );
        // The prior pressed row takes10ticks to lose its full focus. Capture
        // a settled baseline: full-opacity restoration resumes tweeners
        // with that frame's delta, as the native update does.
        let choices = step(&mut live, 10.0);
        live.handle_input(hide, true);
        assert!(
            live.adv_state.hidden_ui_active,
            "Waiting SELECT is independently hideable"
        );
        let halfway = step(&mut live, 5.0);
        assert_ne!(
            &halfway[1920 * 250 * 4..1920 * 570 * 4],
            &choices[1920 * 250 * 4..1920 * 570 * 4],
            "SELECT rows and text must fade"
        );
        let background_pixel = (400 * 1920 + 500) * 4;
        for channel in 0..3 {
            assert!(
                (halfway[background_pixel + channel] as f32
                    - choices[background_pixel + channel] as f32 * 0.5)
                    .abs()
                    <= 1.0,
                "SELECT background opacity must follow the native0.1/tick fade"
            );
        }
        let hidden_choices = step(&mut live, 5.0);
        assert!(hidden_choices.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        let mut input = enum_map::EnumMap::default();
        input[crate::app::AppAction::AnyDown] = shin_input::ActionState {
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        live.handle_input(input, true);
        live.handle_pointer(glam::vec2(960.0, 600.0), true);
        live.handle_pointer_state(glam::vec2(960.0, 400.0), true, true, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            Some(0)
        );
        live.handle_input(dismiss, true);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "SELECT restore must consume confirm"
        );
        let restored_choices = step(&mut live, 10.0);
        live.handle_pointer_state(glam::vec2(960.0, 400.0), false, true, 0.0);
        live.handle_pointer_state(glam::vec2(960.0, 400.0), false, false, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.selected,
            Some(0),
            "Held restore click cannot acquire another row after restoration"
        );
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "Held restore click release cannot confirm SELECT"
        );
        assert!(
            restored_choices[1920 * 250 * 4..1920 * 570 * 4]
                == choices[1920 * 250 * 4..1920 * 570 * 4],
            "SELECT labels and selection survive hiding"
        );
        live.handle_input(dismiss, true);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            None,
            "SELECT must wait for native flash and exit animations"
        );
        let _ = step(&mut live, 7.0);
        let _ = step(&mut live, 0.0);
        let _ = step(&mut live, 100.0);
        let _ = step(&mut live, 0.0);
        assert_eq!(
            live.adv_state.choice_menu.as_ref().unwrap().model.result,
            Some(0)
        );
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "Completed SELECT is not hideable"
        );
        drop(step);
        let before_completion = live.vm_state.save_position;
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::ZERO,
            asset_server: &server,
            pre_render: &mut context,
        };
        assert!(
            crate::adv::UpdatableCommand::update(
                &mut select_command,
                &mut update,
                &original_scenario,
                &mut live.vm_state,
                &mut live.adv_state,
                true
            )
            .is_some()
        );
        drop(update);
        assert!(live.adv_state.choice_menu.is_none());
        assert_eq!(live.vm_state.save_position, before_completion + 1);
        assert!(live.vm_state.save_vectors.select_seen_choices.contains(0));
        assert_eq!(live.vm_state.save_vectors.select_last_choices, [1]);
        live.adv_state
            .plane_layer_group_mut(PlaneId::new(0))
            .add_layer(quiz_bank, quiz_layer.into());
        live.current_command = Some(quiz_command);
        assert!(live.active_interactive_layer().is_some());
        live.handle_input(hide, true);
        assert!(
            !live.adv_state.hidden_ui_active,
            "QUIZ overlay does not grant hide eligibility"
        );
        println!(
            "Live native hide UI: system/event gates,10tick fade,nested scopes,400tick clock pause and consumed dismissal pass"
        );
        println!(
            "Native SELECT hide: independent eligibility,5/10tick fade,retained rows,consumed confirm,and QUIZ rejection pass"
        );
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::R, false),
            true,
        );
        assert!(live.adv_state.playback_modes.skip);
        let murderer = shin_tasks::block_on(
            crate::layer::user::murderer::accusation::MurdererAccusationView::load_menu(
                &server, 20,
            ),
        );
        live.adv_state
            .plane_layer_group_mut(PlaneId::new(0))
            .add_layer(quiz_bank, murderer.into());
        let top_point = glam::vec2(1010.0, 450.0);
        live.handle_pointer_state(top_point, true, true, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            view.advance_animation(2.0);
            assert!(view.take_sounds().is_empty());
        } else {
            unreachable!();
        }
        live.handle_pointer_state(glam::Vec2::ZERO, false, true, 0.0);
        live.handle_pointer_state(top_point, false, false, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            view.advance_animation(2.0);
            assert!(view.take_sounds().is_empty());
            assert!(view.result().is_none());
        } else {
            unreachable!();
        }
        live.handle_pointer_state(top_point, true, true, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            view.advance_animation(10.0);
            assert!(view.take_sounds().is_empty());
            assert!(view.result().is_none());
        } else {
            unreachable!();
        }
        live.handle_pointer_state(top_point, false, false, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            assert_eq!(view.take_sounds(), ["decide"]);
            view.advance_animation(3.5);
            assert!(view.take_sounds().is_empty());
            view.advance_animation(0.0);
            view.advance_animation(0.0);
            assert!(view.take_sounds().is_empty()); // Only accusation entry cues execute.
            view.advance_animation(30.0);
            assert!(view.result().is_none());
            assert!(view.result().is_none()); // Replay is installed; verify held input below.
        } else {
            unreachable!();
        }
        let replay_point = glam::vec2(130.0, 200.0);
        live.handle_pointer_state(replay_point, true, true, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            view.advance_animation(10.0);
            assert!(view.take_sounds().is_empty());
            assert!(view.result().is_none());
        } else {
            unreachable!();
        }
        live.handle_pointer_state(replay_point, false, false, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            assert_eq!(view.take_sounds(), ["decide"]);
            for delta in [3.5, 0.0, 90.0, 0.0] {
                view.advance_animation(delta);
            }
            assert_eq!(view.result(), Some(0));
            view.reset();
        } else {
            unreachable!();
        }
        println!(
            "Native murderer Top/Replay pointer: held input highlights without confirming, Top leave cancels capture, long holds leave3.5 flash ticks, Replay release publishes original chapter0 after its90tick exit"
        );
        live.pending_playback_sound = None;
        live.adv_state.playback_modes.autoplay = true;
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::R, false),
            true,
        );
        assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        assert!(live.pending_playback_sound.is_none());
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, true),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["move"]);
        assert!(view.result().is_none());
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]);
        view.advance_animation(7.0);
        view.advance_animation(0.0);
        view.advance_animation(30.0);
        view.advance_animation(0.0);
        assert_eq!(view.take_sounds(), ["execute"]);
        view.advance_animation(60.0);
        assert!(view.result().is_none());
        println!(
            "Native murderer Top controller: raw Up+A wraps0->5 without confirmation; second A waits flash/preliminary wipe/hold then stages Accusation with delayed execute cue"
        );
        for menu in 0..6 {
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                panic!("Expected murderer overlay");
            };
            view.reset();
            view.navigate(menu);
            view.confirm();
            for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
                view.advance_animation(delta);
            }
            for button in [
                shin_input::inputs::GamepadButton::L,
                shin_input::inputs::GamepadButton::R,
            ] {
                live.handle_input(controller_press(button, false), true);
                assert!(
                    !live.adv_state.playback_modes.skip
                        && !live.adv_state.playback_modes.autoplay
                );
                assert!(live.pending_playback_sound.is_none());
            }
            let back_point = glam::vec2(1700.0, 60.0);
            live.handle_pointer_state(back_point, true, true, 0.0);
            if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            {
                view.take_sounds(); // Entry cue only for accusation.
                view.advance_animation(10.0);
                assert!(view.take_sounds().is_empty());
                assert!(view.result().is_none());
            } else {
                unreachable!();
            }
            live.handle_pointer_state(back_point, false, false, 0.0);
            if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            {
                assert_eq!(view.take_sounds(), ["decide"]);
                for delta in [3.5, 0.0, 30.0] {
                    view.advance_animation(delta);
                }
                assert!(view.result().is_none());
                view.confirm(); // Returned Top entry accepts confirmation.
                assert_eq!(view.take_sounds(), ["decide"]);
            } else {
                unreachable!();
            }
        }
        println!(
            "Native murderer Back pointer: allsix submenu long holds stay live until release, returnflash takes3.5 remaining ticks and restored Top accepts confirmation after30tick wipe"
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.reset();
        view.confirm(); // Top's first entry opens Replay.
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        let mut diagonal = controller_press(shin_input::inputs::GamepadButton::Right, true);
        diagonal[crate::app::AppAction::Enter] =
            controller_press(shin_input::inputs::GamepadButton::A, false)
                [crate::app::AppAction::Enter];
        live.handle_input(diagonal, true);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert!(view.result().is_none()); // Movement consumes simultaneous A.
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(7.0);
        view.advance_animation(0.0);
        view.advance_animation(90.0);
        view.advance_animation(0.0);
        assert_eq!(view.result(), Some(4)); // Row1, column1.
        view.reset();
        view.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::B, true),
            true,
        );
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(7.0);
        view.advance_animation(0.0);
        view.advance_animation(90.0);
        view.advance_animation(0.0);
        assert_eq!(view.result(), Some(3)); // Up moves, consuming simultaneous B.
        view.reset();
        view.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        view.take_sounds();
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::B, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["cancel"]);
        for delta in [3.5, 3.5, 0.0, 15.0] {
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            view.advance_animation(delta);
            live.handle_input(
                controller_press(shin_input::inputs::GamepadButton::A, false),
                true,
            );
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert!(view.take_sounds().is_empty());
            assert!(view.result().is_none());
        }
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(15.0);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]); // Confirm restored Top's Replay entry.
        for delta in [7.0, 0.0, 30.0] {
            view.advance_animation(delta);
        }
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        for delta in [7.0, 0.0, 90.0, 0.0] {
            view.advance_animation(delta);
        }
        assert_eq!(view.result(), Some(0));
        println!(
            "Native murderer Back: raw B starts cancel flash; A is consumed through flash/staging/wipe; restored Top reopens Replay and later A publishes chapter0"
        );
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        let up_and_confirm = controller_press(shin_input::inputs::GamepadButton::A, true);
        live.handle_input(up_and_confirm, true);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert!(view.result().is_none()); // Movement and busy scroll consume A.
        view.advance_animation(2.0);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert!(view.result().is_none());
        view.advance_animation(2.0);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(60.0);
        view.advance_animation(0.0);
        assert_eq!(view.result(), Some(25)); // Hint19 +6.
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        view.navigate(-20); // Stable first-row setup.
        for _ in 0..8 {
            live.handle_input(
                controller_press(shin_input::inputs::GamepadButton::Down, false),
                true,
            );
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            view.advance_animation(4.0);
        }
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(60.0);
        view.advance_animation(0.0);
        assert_eq!(view.result(), Some(14)); // Hint8 +6.
        println!(
            "Native hint controller: raw Up+A and busy scroll consume confirm until4ticks; eight Down steps preserve index8 through delayed response14"
        );
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0] {
            view.advance_animation(delta);
        }
        view.take_sounds();
        live.set_character_scroll_axis(-0.5);
        live.handle_input(Default::default(), true);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_hint_selection(), (19, 1154.0));
        assert!(view.take_sounds().is_empty());
        live.set_character_scroll_axis(0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0] {
            view.advance_animation(delta);
        }
        view.take_sounds();
        let origin = glam::vec2(300.0, 400.0);
        let dragged = origin + glam::Vec2::Y * 40.0;
        let native_tick = 0.016683333333333335;
        live.handle_pointer_state(origin, true, true, native_tick);
        live.handle_pointer_state(dragged, false, true, native_tick);
        live.handle_pointer_state(dragged, false, false, native_tick);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_hint_selection(), (19, 1130.0));
        view.advance_animation(2.5);
        assert_eq!(view.reference_hint_selection(), (18, 1030.0));
        assert!(view.take_sounds().is_empty());
        assert!(view.result().is_none());
        let stop = glam::vec2(400.0, 200.0);
        live.handle_pointer_state(stop, true, true, native_tick);
        live.handle_pointer_state(stop, false, false, native_tick);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(1.0);
        assert_eq!(view.reference_hint_selection(), (18, 1030.0));
        assert!(view.take_sounds().is_empty());
        assert!(view.result().is_none());
        println!(
            "Native Hint scrolling: axis-.5 moves1170->1154 and selection20->19; captured drag/release1130->1030 at2.5ticks selects18; next press stops inertia without response or cue"
        );
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0] {
            view.advance_animation(delta);
        }
        view.take_sounds();
        let coast_point = origin + glam::Vec2::Y * 100.0;
        live.handle_pointer_state(origin, true, true, native_tick);
        live.handle_pointer_state(coast_point, false, true, native_tick);
        live.handle_pointer_state(coast_point, false, false, native_tick);
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::A, false),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]);
        view.advance_animation(2.0);
        assert_eq!(view.reference_hint_selection(), (16, 870.0));
        assert!(view.result().is_none());
        view.advance_animation(58.0);
        assert_eq!(view.reference_hint_selection(), (7, 0.0));
        assert!(view.result().is_none());
        view.advance_animation(0.0);
        assert_eq!(view.result(), Some(13));
        println!(
            "Native coasting Hint confirm: A requests18, inertia selects16 then7 during timer, completion publishes current7+6=13 after timer phase check"
        );
        view.reset();
        view.navigate(4);
        view.confirm();
        for delta in [7.0, 0.0, 30.0] {
            view.advance_animation(delta);
        }
        view.take_sounds(); // Discard Top navigation/confirmation cues.
        let hint_point = glam::vec2(330.0, 190.0); // Initial viewport row13.
        live.handle_pointer_state(hint_point, true, true, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            view.advance_animation(10.0);
            assert!(view.take_sounds().is_empty());
            assert!(view.result().is_none());
        } else {
            unreachable!();
        }
        live.handle_pointer_state(hint_point, false, false, 0.0);
        if let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        {
            assert_eq!(view.take_sounds(), ["decide"]);
            view.advance_animation(3.5);
            view.advance_animation(0.0);
            assert!(view.result().is_none());
            view.advance_animation(56.5);
            assert!(view.result().is_none());
            view.advance_animation(0.0);
            assert_eq!(view.result(), Some(19));
        } else {
            unreachable!();
        }
        println!(
            "Native hint pointer: row13 press stays silent through10tick hold, release retains hotflash and independent60tick timer before publishing original response19"
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.reset();
        view.navigate(2);
        view.confirm();
        for ticks in [7.0, 0.0, 30.0] {
            view.advance_animation(ticks);
        }
        view.take_sounds();
        // Down skips native empty person transcripts3/6/9, not linear IDs.
        for (button, up, index, cues) in [
            (shin_input::inputs::GamepadButton::A, true, 0, vec![]),
            (
                shin_input::inputs::GamepadButton::Down,
                false,
                14,
                vec!["move"],
            ),
            (shin_input::inputs::GamepadButton::B, true, 0, vec!["move"]),
        ] {
            live.handle_input(controller_press(button, up), true);
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(view.reference_purple_selection().0, Some(index));
            assert_eq!(view.take_sounds(), cues);
            assert!(view.result().is_none());
        }
        let purple_point = glam::vec2(440.0, 210.0);
        live.handle_pointer_state(purple_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_purple_selection().0, Some(0));
        live.handle_pointer_state(purple_point, true, true, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_purple_selection().0, Some(1));
        view.advance_animation(10.0);
        assert!(view.take_sounds().is_empty());
        assert!(view.result().is_none());
        live.handle_pointer_state(purple_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]);
        assert_eq!(view.reference_purple_selection().0, Some(1));
        view.advance_animation(3.5);
        let mut purple_text_max = 0.0f32;
        let mut longest_person = 1usize;
        for person in 0..17 {
            let current = view.reference_purple_selection().0.unwrap();
            view.navigate(person as i32 - current as i32);
            view.scroll_text(10000.0);
            let maximum = view.reference_purple_selection().2;
            if maximum > purple_text_max {
                purple_text_max = maximum;
                longest_person = person;
            }
        }
        let current = view.reference_purple_selection().0.unwrap();
        view.navigate(longest_person as i32 - current as i32);
        view.scroll_text(-10000.0);
        view.take_sounds();
        live.set_character_scroll_axis(0.5);
        live.handle_input(Default::default(), true);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(
            view.reference_purple_selection().2,
            16.0f32.min(purple_text_max)
        );
        live.set_character_scroll_axis(-0.25);
        live.handle_input(Default::default(), true);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(
            view.reference_purple_selection().2,
            (16.0f32.min(purple_text_max) - 8.0).max(0.0)
        );
        assert!(view.take_sounds().is_empty());
        view.navigate(1 - longest_person as i32);
        view.take_sounds();
        live.set_character_scroll_axis(0.0);
        println!(
            "Native Purple analog transcript: signed right-stick axis adds32*axis with loaded-font bounds; longest person{longest_person},maximum{purple_text_max}; no axis cue or selection change"
        );
        live.handle_pointer_state(purple_point, true, true, 0.0); // Current person ignored.
        live.handle_pointer_state(purple_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert!(view.take_sounds().is_empty());
        println!(
            "Native Purple person pointer: hover retains0, held press silently selects original1 and its transcript, release emits decide without QUIZ response, fresh current-person hit ignored"
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.reset();
        view.navigate(3);
        view.confirm();
        for ticks in [7.0, 0.0, 30.0] {
            view.advance_animation(ticks);
        }
        view.take_sounds();
        for expected in [2, 3, 4, 5, 7, 8] {
            live.handle_input(
                controller_press(shin_input::inputs::GamepadButton::Down, false),
                true,
            );
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(view.reference_purple_selection().0, Some(expected));
            assert_eq!(view.take_sounds(), ["move"]);
            view.advance_animation(2.0);
            if expected == 8 {
                assert_eq!(view.reference_purple_selection().1, 45.0);
            }
            live.handle_input(
                controller_press(shin_input::inputs::GamepadButton::B, false),
                true,
            );
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(view.reference_purple_selection().0, Some(expected));
            assert!(view.take_sounds().is_empty()); // Four-tick scroll gate.
            view.advance_animation(2.0);
        }
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_purple_selection().1, 90.0);
        view.reset();
        view.navigate(3);
        view.confirm();
        for ticks in [7.0, 0.0, 30.0] {
            view.advance_animation(ticks);
        }
        view.take_sounds();
        let chapter_point = glam::vec2(200.0, 390.0);
        live.handle_pointer_state(chapter_point, true, true, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_purple_selection().0, Some(2));
        view.advance_animation(10.0);
        assert!(view.take_sounds().is_empty());
        live.handle_pointer_state(chapter_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]);
        view.advance_animation(3.5);
        view.reference_set_purple_list_scroll(495.0);
        let heading_point = glam::vec2(200.0, 200.0);
        live.handle_pointer_state(heading_point, true, true, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(2.0);
        assert_eq!(view.reference_purple_selection().0, Some(2));
        assert_eq!(view.reference_purple_selection().1, 495.0);
        assert!(view.take_sounds().is_empty());
        live.handle_pointer_state(heading_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["decide"]);
        view.advance_animation(2.0);
        assert!(
            (view.reference_purple_selection().1
                - 495.0 * (1.0 - std::f32::consts::FRAC_1_SQRT_2))
                .abs()
                < 0.001
        );
        view.advance_animation(2.0);
        assert_eq!(view.reference_purple_selection().0, Some(2));
        assert_eq!(view.reference_purple_selection().1, 0.0);
        assert!(view.result().is_none());
        println!(
            "Native Purple chapter pointer: held row silently selects original2; heading wins overlapping row, release scrolls495->0 with four-tick SineOut and preserves transcript selection"
        );
        let drag_origin = glam::vec2(200.0, 390.0);
        let drag_point = drag_origin - glam::Vec2::Y * 40.0;
        let drag_tick = 0.016683333333333335;
        live.handle_pointer_state(drag_origin, true, true, drag_tick);
        live.handle_pointer_state(drag_point, false, true, drag_tick);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_purple_selection().1, 40.0);
        assert_eq!(view.reference_purple_selection().0, Some(2));
        live.handle_pointer_state(drag_point, false, false, drag_tick);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(2.5);
        assert_eq!(view.reference_purple_selection().1, 140.0);
        assert_eq!(view.reference_purple_selection().0, Some(3));
        assert!(view.take_sounds().is_empty());
        assert!(view.result().is_none());
        let stop_point = glam::vec2(200.0, 800.0);
        live.handle_pointer_state(stop_point, true, true, drag_tick);
        live.handle_pointer_state(stop_point, false, false, drag_tick);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(2.0);
        assert_eq!(view.reference_purple_selection().1, 140.0);
        assert_eq!(view.reference_purple_selection().0, Some(3));
        assert!(view.take_sounds().is_empty());
        println!(
            "Native Purple chapter drag: actual held pointer scrolls0->40, releases velocity-40px/tick, advances140 at2.5ticks, reconciles statement2->3, stops inertia on next press without click cue"
        );
        view.reset();
        view.navigate(3);
        view.confirm();
        for ticks in [7.0, 0.0, 30.0] {
            view.advance_animation(ticks);
        }
        view.take_sounds();
        live.handle_input(
            controller_press(shin_input::inputs::GamepadButton::B, true),
            true,
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["cancel"]); // Stationary chapter Up permits Back.
        assert_eq!(view.reference_purple_selection().0, Some(1));
        println!(
            "Native Purple controller: person directions skip empty transcripts and suppress A/B; chapter Down skips heading6, scrolls0->45->90 over4ticks, gates B while scrolling, and stationary Up permits Back"
        );

        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.reset();
        view.navigate(5);
        view.confirm();
        for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
            view.advance_animation(delta);
        }
        view.take_sounds();
        // Lower actual multi-button gamepad state through ADV. Direction
        // consumes A/B, including a stationary Up at the top grid edge.
        use shin_input::inputs::GamepadButton as AccButton;
        for (buttons, expected, selected, cue) in [
            (
                vec![AccButton::Right, AccButton::Down, AccButton::A],
                32,
                false,
                vec!["move"],
            ),
            (
                vec![AccButton::Right, AccButton::Up, AccButton::B],
                30,
                false,
                vec!["move"],
            ),
            (vec![AccButton::Up, AccButton::A], 30, false, vec![]),
            (vec![AccButton::Down], 33, false, vec!["move"]),
            (vec![AccButton::A], 33, true, vec!["decide"]),
            (vec![AccButton::A], 33, false, vec!["decide"]),
        ] {
            let mut raw = shin_input::RawInputState::new();
            for button in buttons {
                raw.gamepads.unified.buttons[button] = true;
            }
            let lowered = <crate::app::AppAction as shin_input::Action>::lower(&raw);
            live.handle_input(
                enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                    is_clicked: lowered[action],
                    is_clicked_or_repeated: lowered[action],
                    is_held: lowered[action],
                    ..Default::default()
                }),
                true,
            );
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(
                view.reference_accusation_selection(33),
                (Some(expected), selected)
            );
            assert_eq!(view.take_sounds(), cue);
            assert!(view.result().is_none());
        }
        println!(
            "Native accusation controller: raw diagonals consume A/B, stationary Up consumes A, and confirm toggles geometric target33"
        );
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        let point = glam::vec2(580.0, 240.0); // Suspect29.
        live.handle_pointer_state(point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.reference_accusation_selection(29), (Some(33), false));
        for (held_ticks, selected) in [(2.0, true), (10.0, false)] {
            live.handle_pointer_state(point, true, true, 0.0);
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(
                view.reference_accusation_selection(29),
                (Some(29), !selected)
            );
            view.advance_animation(held_ticks);
            assert!(view.take_sounds().is_empty());
            live.handle_pointer_state(point, false, false, 0.0);
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            assert_eq!(
                view.reference_accusation_selection(29),
                (Some(29), selected)
            );
            assert_eq!(view.take_sounds(), ["decide"]);
            view.advance_animation(10.0);
        }
        let submit_point = glam::vec2(500.0, 850.0);
        live.handle_pointer_state(submit_point, true, true, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        view.advance_animation(10.0);
        assert!(view.take_sounds().is_empty());
        assert!(view.result().is_none());
        live.handle_pointer_state(submit_point, false, false, 0.0);
        let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
            live.active_interactive_layer()
        else {
            unreachable!();
        };
        assert_eq!(view.take_sounds(), ["attack"]);
        assert!(!view.allows_playback_mode_changes());
        view.advance_animation(3.5);
        assert!(!view.allows_playback_mode_changes()); // Phase check precedes row clocks.
        view.advance_animation(0.0);
        println!(
            "Native accusation pointer: hover retains controller selection, held rows stay silent until release toggles29, held Submit releases attack and waits3.5remaining flash ticks plus phase check before completion modes"
        );
        assert!(view.allows_playback_mode_changes());
        assert!(view.result().is_none());
        for button in [
            shin_input::inputs::GamepadButton::L,
            shin_input::inputs::GamepadButton::R,
        ] {
            live.handle_input(controller_press(button, false), true);
        }
        assert!(live.adv_state.playback_modes.skip && live.adv_state.playback_modes.autoplay);
        assert_eq!(live.pending_playback_sound, Some("son"));
        println!(
            "Native murderer playback policy: all waiting screens reject L/R and clear modes; accusation completion permits both while QUIZ still waits"
        );
        for (menu, exit_ticks, response) in [(0, 90.0, 0), (4, 53.0, 26)] {
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            };
            // Each command gets its own real token. Do not advance the
            // unrelated scenario used to build this isolated live scene.
            let crate::adv::CommandStartResult::Yield(command) =
                crate::adv::command::apply_command_state_and_start(
                    C::QUIZ(c::QUIZ {
                        arg: NumberSpec::constant(0),
                        dest: Register::from_regular_register(0),
                    })
                    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                    &mut update,
                    &original_scenario,
                    &mut live.vm_state,
                    &mut live.adv_state,
                )
            else {
                unreachable!();
            };
            live.current_command = Some(command);
            live.vm_state.layers.layerbanks[quiz_bank].is_interation_completed = false;
            live.vm_state.layers.layerbanks[quiz_bank].interaction_result = None;
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            view.reset();
            view.navigate(menu);
            view.confirm();
            for delta in [7.0, 0.0, 30.0, 0.0, 60.0] {
                view.advance_animation(delta);
            }
            view.confirm();
            assert!(view.result().is_none());
            view.advance_animation(7.0);
            view.advance_animation(0.0);
            assert!(view.allows_playback_mode_changes());
            assert!(view.result().is_none());
            let position = live.vm_state.save_position;
            let mut command = live.current_command.take().unwrap();
            assert!(
                crate::adv::UpdatableCommand::update(
                    &mut command,
                    &mut update,
                    &original_scenario,
                    &mut live.vm_state,
                    &mut live.adv_state,
                    true,
                )
                .is_none()
            );
            assert_eq!(live.vm_state.save_position, position);
            live.current_command = Some(command);
            let Some(crate::layer::user::InteractiveLayerRef::Murderer(view)) =
                live.active_interactive_layer()
            else {
                unreachable!();
            };
            view.advance_animation(exit_ticks);
            assert!(view.result().is_none());
            view.advance_animation(0.0);
            assert_eq!(view.result(), Some(response));
            let mut command = live.current_command.take().unwrap();
            assert!(
                crate::adv::UpdatableCommand::update(
                    &mut command,
                    &mut update,
                    &original_scenario,
                    &mut live.vm_state,
                    &mut live.adv_state,
                    true,
                )
                .is_some()
            );
            assert_eq!(live.vm_state.save_position, position + 1);
            assert_eq!(
                live.vm_state.layers.layerbanks[quiz_bank].interaction_result,
                Some(response)
            );
        }
        println!(
            "Native murderer replay/hint responses: real QUIZ stays pending in phase2, publishes original response only after90/53 remaining ticks and records completion once"
        );
    }
    // Saved character mask reaches the live player. Muting affects only
    // requests carrying the flag and a muted replacement stops prior loading.
    {
        use crate::audio::VoicePlayFlags;
        use shin_core::vm::command::types::{AudioWaitStatus, Volume};
        let saved = adv.vm_state.settings.clone();
        let mut muted = saved.clone();
        muted.v19 = 0;
        adv.adv_state.apply_audio_settings(&muted);
        let mut message = adv.adv_state.message_layer_mut();
        let voice = message.voice_player_mut();
        assert!(voice.play(
            &original_scenario,
            "19/11900001",
            0,
            0,
            VoicePlayFlags::empty(),
            Volume::default()
        ));
        assert_eq!(
            voice.get_wait_status(),
            AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING
        );
        assert!(!voice.play(
            &original_scenario,
            "19/11900001|19/11900001",
            0,
            0,
            VoicePlayFlags::ENABLE_CHARACTER_MUTING,
            Volume::default()
        ));
        assert!(voice.get_wait_status().is_empty());
        drop(message);
        adv.adv_state.apply_audio_settings(&saved);
        let mut message = adv.adv_state.message_layer_mut();
        let voice = message.voice_player_mut();
        assert!(voice.play(
            &original_scenario,
            "19/11900001",
            0,
            0,
            VoicePlayFlags::ENABLE_CHARACTER_MUTING,
            Volume::default()
        ));
        voice.stop();
    }
    crate::audio::BgmPlayer::reference_verify_async_loading(server.clone(), &original_scenario);
    crate::audio::SePlayer::reference_verify_async_loading(server.clone(), &original_scenario);
    verify_live_scene_load_fade(
        &mut context,
        &server,
        &assets,
        frame,
        super::super::SceneLoadKind::History,
    );
    verify_live_scene_load_fade(
        &mut context,
        &server,
        &assets,
        frame,
        super::super::SceneLoadKind::Saved,
    );
    verify_live_pending_saved_scene(&mut context, &server, &assets, &original_scenario, frame);
    verify_live_restored_audio(&mut context, &server, &assets, &original_scenario, frame);
    verify_live_saved_label_exit(&mut context, &server, &assets, frame);
    verify_live_resume(&mut context, &server, &assets, frame);
    verify_live_scene_jump_late_target(
        &mut context,
        &server,
        &assets,
        std::sync::Arc::new(shin_audio::AudioManager::new_for_tests()),
        frame,
    );
    verify_live_backlog(
        &mut context,
        &server,
        &assets,
        adv.adv_state.audio_manager.clone(),
        frame,
    );
    // Browser replay owns its voice player independently of story speech.
    {
        use crate::adv::backlog::voice::BacklogVoiceReplay;
        use crate::audio::VoicePlayFlags;
        use shin_core::vm::command::types::{AudioWaitStatus, Volume};
        let mut message = adv.adv_state.message_layer_mut();
        let story = message.voice_player_mut();
        assert!(story.play(
            &original_scenario,
            "19/11900001",
            0,
            0,
            VoicePlayFlags::empty(),
            Volume::default()
        ));
        let status = story.get_wait_status();
        drop(message);
        adv.adv_state.acquire_audio_pause();
        adv.adv_state.acquire_audio_pause();
        assert_eq!(adv.adv_state.audio_pause_count, 2);
        assert!(adv.adv_state.audio.se_player.reference_is_paused());
        assert_eq!(
            adv.adv_state.ui_hide_count, 0,
            "Audio pause must not acquire UI-hide scope"
        );
        let mut replay = BacklogVoiceReplay::new(
            adv.adv_state.audio_manager.clone(),
            server.clone(),
            original_scenario.clone(),
        );
        replay.set_settings_volume(70);
        replay.play_message("@v19/11900001.");
        for _ in 0..9 {
            replay.update();
            assert!(!replay.is_playing());
        }
        replay.update();
        assert!(replay.is_playing());
        replay.play_message("@o25.@v19/11900001|19/11900001.");
        assert!(
            !replay.is_playing(),
            "Replacement must release old replay wait state"
        );
        for _ in 0..10 {
            replay.update();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while replay.reference_voice_metadata().len() != 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "Replay voice assets did not load"
            );
            replay.update();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(replay.is_playing());
        assert!(
            replay
                .reference_voice_metadata()
                .iter()
                .all(|(speakers, pan)| speakers.is_empty() && pan.0 == 0.0),
            "Replay must disable lipsync and character pan"
        );
        assert_eq!(
            adv.adv_state
                .message_layer()
                .voice_player()
                .get_wait_status(),
            status,
            "Browser playback cannot replace story speech"
        );
        assert!(status.contains(AudioWaitStatus::LOADING));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while adv
            .adv_state
            .message_layer()
            .voice_player()
            .reference_pause_state()
            .1
            .is_empty()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "Paused story voice did not load"
            );
            adv.adv_state
                .message_layer_mut()
                .voice_player_mut()
                .update(Ticks::from_f32(1.0));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        let (paused, positions) = adv
            .adv_state
            .message_layer()
            .voice_player()
            .reference_pause_state();
        assert!(
            paused && positions.iter().all(|&position| position == 0),
            "Late-loaded paused story voice must retain source position0"
        );
        adv.adv_state.release_audio_pause();
        assert_eq!(adv.adv_state.audio_pause_count, 1);
        assert!(adv.adv_state.audio.se_player.reference_is_paused());
        assert!(
            adv.adv_state
                .message_layer()
                .voice_player()
                .reference_pause_state()
                .0
        );
        adv.adv_state.release_audio_pause();
        assert_eq!(adv.adv_state.audio_pause_count, 0);
        assert!(!adv.adv_state.audio.se_player.reference_is_paused());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while adv
            .adv_state
            .message_layer()
            .voice_player()
            .reference_pause_state()
            .1
            .iter()
            .all(|&position| position == 0)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "Resumed story voice did not advance"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        println!(
            "Native modal audio pause: nested scopes, late-loaded voice position0, final-release resume and independent replay pass"
        );
        replay.stop();
        assert!(!replay.is_playing());
        for _ in 0..5 {
            replay.update();
        }
        adv.adv_state.message_layer_mut().voice_player_mut().stop();
        println!(
            "Native browser replay:10-frame startup, replacement, simultaneous supplied voices, no lipsync/pan, separate story ownership and stop pass"
        );
    }
    // Real-ROM bustup metadata survives upload, and the displayed screen
    // projects its own properties for the speaking character pan.
    {
        use crate::{
            asset::bustup::{Bustup, BustupArgs, CharacterId},
            audio::VoicePlayFlags,
            layer::{ScreenLayer, user::BustupLayer},
        };
        use shin_core::vm::command::types::{AudioWaitStatus, LayerbankId, Volume};
        let info = original_scenario
            .info_tables()
            .bustup_info
            .iter()
            .find(|info| info.lipsync_character_id == 19)
            .unwrap();
        let bustup = shin_tasks::block_on(server.load_with_args::<Bustup, _>(
            info.path(),
            BustupArgs {
                expression: info.emotion.to_string(),
                character_id: CharacterId::new(19),
                disable_animations: false,
            },
        ))
        .unwrap();
        assert_eq!(bustup.character_id.raw(), 19);
        BustupLayer::assert_reference_lipsync(&mut context, bustup.clone(), &server);
        let live_plane = PlaneId::new(0);
        let live_bank = LayerbankId::new(47);
        assert!(
            adv.adv_state
                .plane_layer_group(live_plane)
                .get_layer(live_bank)
                .is_none()
        );
        let mut live_bustup = BustupLayer::new(bustup.clone(), Some(info.name.to_string()));
        live_bustup
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(480.0);
        adv.adv_state
            .plane_layer_group_mut(live_plane)
            .add_layer(live_bank, live_bustup.into());
        let mut screen = ScreenLayer::new(1);
        screen
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(120.0);
        screen
            .page_layer_mut()
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(240.0);
        let plane = screen.page_layer_mut().get_plane_mut(PlaneId::new(0));
        plane
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(120.0);
        let mut pan_bustup = BustupLayer::new(bustup.clone(), Some(info.name.to_string()));
        pan_bustup
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(480.0);
        plane.add_layer(LayerbankId::new(0), pan_bustup.into());
        let mut positions = Vec::new();
        screen.collect_voice_positions(&Default::default(), &mut positions);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].0, 19);
        assert!((positions[0].1 - 0.5).abs() < 0.00001);
        let saved = adv.vm_state.settings.clone();
        for (flags, enabled, expected) in [
            (VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC, true, 0.5),
            (VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC, false, 0.0),
            (VoicePlayFlags::ENABLE_CHARACTER_MUTING, true, 0.0),
        ] {
            let mut settings = saved.clone();
            settings.v19 = u32::MAX;
            settings.v5_voicepanapot = enabled;
            adv.adv_state.apply_audio_settings(&settings);
            let mut message = adv.adv_state.message_layer_mut();
            let voice = message.voice_player_mut();
            assert!(voice.play(
                &original_scenario,
                "19/11900001|19/11900001",
                0,
                0,
                flags,
                Volume::default()
            ));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while voice.get_wait_status().contains(AudioWaitStatus::LOADING) {
                assert!(std::time::Instant::now() < deadline, "voice load timed out");
                voice.update(Ticks::ZERO);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            for &(id, pan) in &positions {
                voice.pan_for_character(id, pan);
            }
            let voices = voice.reference_voice_metadata();
            assert_eq!(voices.len(), 2);
            for (speakers, pan) in voices {
                assert_eq!(
                    speakers,
                    if flags.contains(VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC) {
                        vec![19]
                    } else {
                        vec![]
                    }
                );
                assert!((pan.0 - expected).abs() < 0.00001);
            }
            let live_level = voice.lipsync_for_character(19);
            drop(message);
            frame.advance();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            let crate::layer::user::UserLayer::Bustup(live) = &*adv
                .adv_state
                .plane_layer_group(live_plane)
                .get_layer(live_bank)
                .unwrap()
            else {
                panic!("Expected live bustup")
            };
            let metadata = adv
                .adv_state
                .message_layer()
                .voice_player()
                .reference_voice_metadata();
            assert_eq!(metadata.len(), 2);
            assert!(
                metadata
                    .iter()
                    .all(|(_, pan)| (pan.0 - expected).abs() < 0.00001),
                "Actual ADV bustup callback must apply the native local pan: {metadata:?}"
            );
            // The supplied default Nanjo bustup has four authored mouth
            // blocks, with the first two sharing the same source block.
            assert_eq!(
                live.reference_mouth_state(),
                u32::from(live_level.unwrap_or(0)) * 4 / 3
            );
            if enabled && flags == VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC {
                adv.adv_state.allow_running_animations = false;
                adv.adv_state
                    .create_back_layer_group_if_needed(&mut context.render_clone_ctx());
                let mut front_bustup =
                    BustupLayer::new(bustup.clone(), Some(info.name.to_string()));
                front_bustup
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to(-480.0);
                adv.adv_state
                    .plane_layer_group_mut(live_plane)
                    .add_layer(live_bank, front_bustup.into());
                for (back_enabled, expected_pan) in [(false, -0.5), (true, 0.5)] {
                    adv.adv_state
                        .back_layer_group
                        .as_mut()
                        .unwrap()
                        .screen_layer_mut()
                        .page_layer_mut()
                        .get_plane_mut(live_plane)
                        .get_layer_mut(live_bank)
                        .unwrap()
                        .properties_mut()
                        .property_tweener_mut(LayerProperty::Prop27)
                        .fast_forward_to(if back_enabled { 1.0 } else { 0.0 });
                    frame.advance();
                    let mut update = UpdateContext {
                        frame_started_at: std::time::Instant::now(),
                        frame_id: frame,
                        delta_ticks: Ticks::ZERO,
                        asset_server: &server,
                        pre_render: &mut context,
                    };
                    crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                    let metadata = adv
                        .adv_state
                        .message_layer()
                        .voice_player()
                        .reference_voice_metadata();
                    assert_eq!(metadata.len(), 2);
                    assert!(
                        metadata
                            .iter()
                            .all(|(_, pan)| (pan.0 - expected_pan).abs() < 0.00001),
                        "Front then back callbacks must share the real voice owner: {metadata:?}"
                    );
                }
                adv.adv_state
                    .message_layer_mut()
                    .voice_player_mut()
                    .pan_for_character(19, -0.75);
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(10.0),
                    asset_server: &server,
                    pre_render: &mut context,
                };
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert!(
                    adv.adv_state
                        .message_layer()
                        .voice_player()
                        .reference_voice_metadata()
                        .iter()
                        .all(|(_, pan)| pan.0 == -0.75),
                    "Duplicate frame must skip voice callbacks"
                );
                adv.adv_state.back_layer_group = None;
                adv.adv_state.allow_running_animations = true;
                adv.adv_state
                    .plane_layer_group_mut(live_plane)
                    .get_layer_mut(live_bank)
                    .unwrap()
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to(480.0);
                println!(
                    "Actual ADV audio voice: front/back shared-owner pan order, disabled back mouth gate and duplicate frame callback suppression pass"
                );
            }
            adv.adv_state.message_layer_mut().voice_player_mut().stop();
            let message = adv.adv_state.message_layer();
            let voice = message.voice_player();
            assert!(
                voice.get_wait_status().is_empty(),
                "Stopping tails must not block VOICEWAIT"
            );
            assert_eq!(voice.lipsync_for_character(19), None);
            assert!(
                voice.reference_stopping_volumes().len() >= 2,
                "Simultaneous voices remain owned during their stop fades"
            );
            drop(message);
            frame.advance();
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            let crate::layer::user::UserLayer::Bustup(live) = &*adv
                .adv_state
                .plane_layer_group(live_plane)
                .get_layer(live_bank)
                .unwrap()
            else {
                panic!("Expected live bustup")
            };
            assert_eq!(
                live.reference_mouth_state(),
                0,
                "Stopped voice must close the live mouth"
            );
        }
        frame.advance();
        let mut update = UpdateContext {
            frame_started_at: std::time::Instant::now(),
            frame_id: frame,
            delta_ticks: Ticks::from_f32(5.0),
            asset_server: &server,
            pre_render: &mut context,
        };
        crate::update::Updatable::update(&mut adv.adv_state, &mut update);
        assert!(
            adv.adv_state
                .message_layer()
                .voice_player()
                .reference_stopping_volumes()
                .is_empty(),
            "Five native ticks finish all full-volume stop tails"
        );
        adv.adv_state
            .plane_layer_group_mut(live_plane)
            .remove_layer(live_bank, Ticks::ZERO);
        adv.adv_state.apply_audio_settings(&saved);
    }
    // The persisted owner above has reached EXIT. Use a live owner for
    // Characters input checks, keeping the finished owner's shutdown clock.
    {
        let mut adv = Adv::new(
            adv.adv_state.audio_manager.clone(),
            server.clone(),
            assets.clone(),
            shin_core::vm::Scripter::new(&original_scenario, 0, 42),
        );
        assert!(!adv.is_finished());
        // Exercise the actual SHOWCHARS dispatcher, ADV input routing and modal gate.
        {
            use crate::adv::UpdatableCommand;
            let live_target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Live SHOWCHARS target"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let live_view = live_target.create_view(&Default::default());
            let information = adv.vm_state.information.clone();
            let original_permission = adv.vm_state.system_controls_allowed;
            let save_position = adv.vm_state.save_position;
            adv.vm_state.information.notification_style = 0;
            for permission in [true, false] {
                adv.vm_state.system_controls_allowed = permission;
                adv.adv_state.playback_modes.skip = true;
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: &server,
                    pre_render: &mut context,
                };
                crate::adv::command::apply_command_state_and_start(
                    C::CHARS(c::CHARS {
                        arg1: NumberSpec::constant(0),
                        arg2: NumberSpec::constant(0),
                    })
                    .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                    &mut update,
                    &original_scenario,
                    &mut adv.vm_state,
                    &mut adv.adv_state,
                );
                let crate::adv::CommandStartResult::Yield(mut command) =
                    crate::adv::command::apply_command_state_and_start(
                        C::SHOWCHARS(c::SHOWCHARS {})
                            .into_runtime_form(&shin_core::vm::VmCtx::new(0, 42)),
                        &mut update,
                        &original_scenario,
                        &mut adv.vm_state,
                        &mut adv.adv_state,
                    )
                else {
                    panic!("SHOWCHARS must suspend the script")
                };
                assert!(!adv.vm_state.system_controls_allowed);
                assert!(!adv.adv_state.playback_modes.skip);
                assert!(
                    command
                        .update(
                            &mut update,
                            &original_scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            true
                        )
                        .is_none(),
                    "Fast-forward must not dismiss character screen"
                );
                update.delta_ticks = Ticks::from_f32(10.0);
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                adv.adv_state
                    .character_screen
                    .as_mut()
                    .unwrap()
                    .wait_reference_full_sprite(&server);
                let screen = adv.adv_state.character_screen.as_ref().unwrap();
                assert!(screen.model.transition.is_open());
                assert_eq!(screen.model.characters.page, 0);
                drop(update);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &live_view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Live SHOWCHARS screen",
                    );
                    adv.render(&mut pass);
                }
                let pixels = crate::layer::user::TileLayer::readback_reference_texture(
                    &mut context,
                    &live_target,
                );
                assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
                // Character caption and enabled Execute artwork must reach ADV's real pass.
                assert!(
                    pixels[(32 * 1920 + 500) * 4..(110 * 1920 + 1420) * 4]
                        .chunks_exact(4)
                        .filter(|p| p[0] > 230 && p[1] > 230 && p[2] > 230)
                        .count()
                        > 1000
                );
                if permission {
                    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                    for pixel in pixels.chunks_exact(4) {
                        ppm.extend_from_slice(&pixel[..3]);
                    }
                    std::fs::write("/tmp/shin-live-showchars-reference.ppm", ppm).unwrap();
                }
                let initial = adv
                    .adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .characters
                    .selected_index()
                    .unwrap();
                let mut input = enum_map::EnumMap::default();
                input[crate::app::AppAction::Right] = shin_input::ActionState {
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
                input[crate::app::AppAction::ToggleSkip] = shin_input::ActionState {
                    is_clicked: true,
                    ..Default::default()
                };
                adv.handle_input(input, true);
                assert!(!adv.adv_state.playback_modes.skip);
                assert_ne!(
                    adv.adv_state
                        .character_screen
                        .as_ref()
                        .unwrap()
                        .model
                        .characters
                        .selected_index(),
                    Some(initial)
                );
                adv.adv_state.audio.system_player.sounds.clear();
                let before = adv
                    .adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .characters
                    .selected_cell()
                    .unwrap()
                    .definition()
                    .state()
                    .unwrap()
                    .index;
                adv.handle_pointer(glam::vec2(150.0, 800.0), true);
                assert_eq!(
                    adv.adv_state
                        .character_screen
                        .as_ref()
                        .unwrap()
                        .model
                        .characters
                        .selected_cell()
                        .unwrap()
                        .definition()
                        .state()
                        .unwrap()
                        .index,
                    before,
                    "State must remain unchanged during its button flash"
                );
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(7.0),
                    asset_server: &server,
                    pre_render: &mut context,
                };
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert_eq!(
                    adv.adv_state
                        .character_screen
                        .as_ref()
                        .unwrap()
                        .model
                        .characters
                        .selected_cell()
                        .unwrap()
                        .definition()
                        .state()
                        .unwrap()
                        .index,
                    before,
                    "Native edit task cannot observe its flash becoming idle until the following poll"
                );
                update.delta_ticks = Ticks::ZERO;
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert_eq!(
                    adv.adv_state
                        .character_screen
                        .as_ref()
                        .unwrap()
                        .model
                        .characters
                        .selected_cell()
                        .unwrap()
                        .definition()
                        .state()
                        .unwrap()
                        .index,
                    before + 1
                );
                assert_eq!(
                    adv.adv_state.audio.system_player.sounds.len(),
                    2,
                    "Directional move and Execute must each play once"
                );
                drop(update);
                if permission {
                    let mut input = enum_map::EnumMap::default();
                    input[crate::app::AppAction::Cancel] = shin_input::ActionState {
                        is_clicked: true,
                        ..Default::default()
                    };
                    adv.handle_input(input, true);
                } else {
                    adv.handle_pointer(glam::vec2(1800.0, 64.0), true);
                }
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(7.0),
                    asset_server: &server,
                    pre_render: &mut context,
                };
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert!(
                    command
                        .update(
                            &mut update,
                            &original_scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            true
                        )
                        .is_none()
                );
                assert!(!adv.vm_state.system_controls_allowed);
                update.delta_ticks = Ticks::ZERO;
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                update.delta_ticks = Ticks::from_f32(9.0);
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert!(
                    command
                        .update(
                            &mut update,
                            &original_scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            true
                        )
                        .is_none()
                );
                update.delta_ticks = Ticks::from_f32(1.0);
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                assert!(
                    command
                        .update(
                            &mut update,
                            &original_scenario,
                            &mut adv.vm_state,
                            &mut adv.adv_state,
                            true
                        )
                        .is_some()
                );
                assert!(adv.adv_state.character_screen.is_none());
                assert_eq!(adv.vm_state.system_controls_allowed, permission);
                assert_eq!(adv.vm_state.save_position, save_position);
            }
            adv.vm_state.information = information;
            adv.vm_state.system_controls_allowed = original_permission;
        }
        // Drag a real long character description using the desktop pointer route.
        {
            use crate::adv::{
                characters::screen::CharacterScreen, characters::text::CharacterTextLayout,
                characters::CharacterModel,
            };
            let font: Arc<crate::asset::font::GpuFontLazy> =
                server.load_sync("/system.fnt").unwrap();
            let tables = original_scenario.info_tables();
            let mut candidate = None;
            for (id, grid) in tables.chars_grid_info.iter().enumerate() {
                let model = CharacterModel::new(grid, &tables.chars_sprite_info, id, 0);
                for cell in &model.cells {
                    let Some(state) = cell.definition().state() else {
                        continue;
                    };
                    let layout = CharacterTextLayout::new(
                        &state.name,
                        &state.description,
                        font.as_ref(),
                    );
                    if layout.max_scroll() > 150.0 && cell.is_selectable() {
                        candidate = Some((
                            id,
                            cell.page,
                            glam::vec2(
                                104.0 + cell.x as f32 * 24.0,
                                204.0 + cell.y as f32 * 26.0,
                            ),
                        ));
                        break;
                    }
                }
                if candidate.is_some() {
                    break;
                }
            }
            let (id, page, portrait) =
                candidate.expect("Supplied ROM must include a long character description");
            adv.adv_state.character_screen = Some(
                CharacterScreen::new(&server, &original_scenario, id as i32, page as i32)
                    .unwrap(),
            );
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(10.0),
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            drop(update);
            adv.handle_pointer(portrait, true);
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(10.0),
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            drop(update);
            adv.adv_state
                .character_screen
                .as_mut()
                .unwrap()
                .wait_reference_full_sprite(&server);
            let drag_target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Native character drag"),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let drag_view = drag_target.create_view(&Default::default());
            let native_tick = 0.016683333333333335;
            adv.handle_pointer_state(glam::vec2(600.0, 400.0), true, true, native_tick);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &drag_view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Character drag initial viewport",
                );
                adv.render(&mut pass);
            }
            let before = crate::layer::user::TileLayer::readback_reference_texture(
                &mut context,
                &drag_target,
            );
            // Native ADV draws the title before modal tasks. Compare both orders
            // with the real title and an open Characters screen so overlap is tested.
            let title_name = original_scenario
                .info_tables()
                .bgm_info
                .iter()
                .find(|b| !b.display_name.as_str().is_empty())
                .unwrap()
                .display_name
                .as_str();
            adv.adv_state.bgm_title.show(&server, title_name);
            adv.adv_state.bgm_title.update(Ticks::from_f32(23.0));
            let mut frames = Vec::new();
            for order in 0..3 {
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &drag_view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Native title/modal compositing",
                    );
                    if order == 0 {
                        adv.render(&mut pass);
                    } else {
                        pass.clear(
                            Some(shin_core::primitives::color::UnormColor::BLACK),
                            Some(0),
                            Some(1.0),
                        );
                        crate::adv::render_layer_without_bg(
                            &mut pass,
                            &crate::adv::TransformParams::default(),
                            adv.adv_state.root_layer_group(),
                            0,
                        );
                        adv.adv_state.notifications.render(&mut pass);
                        adv.adv_state.route_label.render(&mut pass);
                        if order == 1 {
                            adv.adv_state.bgm_title.render(&mut pass);
                        }
                        adv.adv_state
                            .character_screen
                            .as_ref()
                            .unwrap()
                            .render(&mut pass);
                        if order == 2 {
                            adv.adv_state.bgm_title.render(&mut pass);
                        }
                    }
                }
                frames.push(crate::layer::user::TileLayer::readback_reference_texture(
                    &mut context,
                    &drag_target,
                ));
            }
            assert_eq!(
                frames[0], frames[1],
                "Live ADV must draw titles before modal tasks"
            );
            assert_ne!(
                frames[1], frames[2],
                "Real Characters artwork must overlap the title"
            );
            adv.adv_state.bgm_title = crate::adv::bgm_title::BgmTitle::new();
            println!(
                "Native title/modal compositing matches with real Characters artwork; reversed order differs"
            );
            adv.handle_pointer_state(glam::vec2(600.0, 390.0), false, true, native_tick);
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                0.0,
                "Motion below native16-pixel threshold must remain a click"
            );
            adv.handle_pointer_state(glam::vec2(600.0, 383.0), false, true, native_tick);
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                17.0
            );
            adv.handle_pointer_state(glam::vec2(600.0, 320.0), false, true, native_tick);
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                80.0
            );
            let selected = adv
                .adv_state
                .character_screen
                .as_ref()
                .unwrap()
                .model
                .characters
                .selected_index();
            let mut input = enum_map::EnumMap::default();
            input[crate::app::AppAction::Right] = shin_input::ActionState {
                is_clicked_or_repeated: true,
                ..Default::default()
            };
            adv.handle_input(input, true);
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .characters
                    .selected_index(),
                selected,
                "Native pointer processing consumes simultaneous keyboard navigation"
            );
            adv.handle_pointer_state(glam::vec2(600.0, 320.0), false, false, native_tick);
            for expected in [111.5, 111.5 + 31.5 * 0.95f32.sqrt()] {
                let mut update = UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(0.5),
                    asset_server: &server,
                    pre_render: &mut context,
                };
                crate::update::Updatable::update(&mut adv.adv_state, &mut update);
                drop(update);
                assert!(
                    (adv.adv_state
                        .character_screen
                        .as_ref()
                        .unwrap()
                        .model
                        .text_scroll
                        .position
                        - expected)
                        .abs()
                        < 0.0001
                );
            }
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &drag_view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Character drag released viewport",
                );
                adv.render(&mut pass);
            }
            let after = crate::layer::user::TileLayer::readback_reference_texture(
                &mut context,
                &drag_target,
            );
            let mut changed = 0usize;
            for y in 0..1080 {
                for x in 0..1920 {
                    let i = (y * 1920 + x) * 4;
                    if (544..1520).contains(&x) && (188..918).contains(&y) {
                        changed += usize::from(before[i..i + 4] != after[i..i + 4]);
                    } else {
                        assert_eq!(
                            &before[i..i + 4],
                            &after[i..i + 4],
                            "Text drag must retain other screen artwork at{x},{y}"
                        );
                    }
                }
            }
            assert!(
                changed > 5000,
                "Live scroll must visibly change clipped text, changed{changed}"
            );
            println!(
                "Native live character drag grid{id} page{page}: changed{changed} viewport pixels; all outside pixels retained"
            );
            // A sub-threshold release must stop without inertia, even after dragging.
            adv.handle_pointer_state(glam::vec2(600.0, 400.0), true, true, native_tick);
            adv.handle_pointer_state(glam::vec2(600.0, 420.0), false, true, native_tick);
            adv.handle_pointer_state(glam::vec2(600.0, 421.0), false, true, native_tick);
            let stopped = adv
                .adv_state
                .character_screen
                .as_ref()
                .unwrap()
                .model
                .text_scroll
                .position;
            adv.handle_pointer_state(glam::vec2(600.0, 421.0), false, false, native_tick);
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            drop(update);
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                stopped
            );
            // A pressed Close button waits for release; leaving its bounds cancels.
            adv.handle_pointer_state(glam::vec2(1800.0, 64.0), true, true, native_tick);
            assert!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .transition
                    .is_open()
            );
            adv.handle_pointer_state(glam::vec2(1700.0, 64.0), false, true, native_tick);
            adv.handle_pointer_state(glam::vec2(1700.0, 64.0), false, false, native_tick);
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(20.0),
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            drop(update);
            assert!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .transition
                    .is_open()
            );
            // Real controller lowering/dispatch: signed right-stick scrolling and
            // A wins over B in the same native input snapshot.
            use shin_input::{
                Action, ActionsState, RawInputState,
                inputs::{GamepadAxis, GamepadButton},
            };
            let mut raw = RawInputState::new();
            let mut actions = ActionsState::<crate::app::AppAction>::new();
            adv.handle_pointer_state(glam::vec2(1700.0, 64.0), false, false, native_tick);
            adv.adv_state
                .character_screen
                .as_mut()
                .unwrap()
                .model
                .text_scroll
                .position = 80.0;
            raw.gamepads.unified.axes[GamepadAxis::RightStickY] = 0.5;
            adv.set_character_scroll_axis(-raw.gamepads.unified.axes[GamepadAxis::RightStickY]);
            adv.handle_input(
                actions.update(
                    crate::app::AppAction::lower(&raw),
                    std::time::Duration::from_secs_f64(native_tick),
                ),
                true,
            );
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                72.0
            );
            raw.gamepads.unified.axes[GamepadAxis::RightStickY] = -0.25;
            adv.set_character_scroll_axis(-raw.gamepads.unified.axes[GamepadAxis::RightStickY]);
            adv.handle_input(
                actions.update(
                    crate::app::AppAction::lower(&raw),
                    std::time::Duration::from_secs_f64(native_tick),
                ),
                true,
            );
            assert_eq!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .text_scroll
                    .position,
                76.0
            );
            raw.gamepads.unified.buttons[GamepadButton::A] = true;
            raw.gamepads.unified.buttons[GamepadButton::B] = true;
            adv.handle_input(
                actions.update(
                    crate::app::AppAction::lower(&raw),
                    std::time::Duration::from_secs_f64(native_tick),
                ),
                true,
            );
            assert!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .transition
                    .is_open()
            );
            raw.gamepads.unified.buttons[GamepadButton::A] = false;
            raw.gamepads.unified.buttons[GamepadButton::B] = false;
            actions.update(
                crate::app::AppAction::lower(&raw),
                std::time::Duration::from_secs_f64(native_tick),
            );
            raw.gamepads.unified.buttons[GamepadButton::B] = true;
            adv.handle_input(
                actions.update(
                    crate::app::AppAction::lower(&raw),
                    std::time::Duration::from_secs_f64(native_tick),
                ),
                true,
            );
            let mut update = UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(7.0),
                asset_server: &server,
                pre_render: &mut context,
            };
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            update.delta_ticks = Ticks::ZERO;
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            update.delta_ticks = Ticks::from_f32(10.0);
            crate::update::Updatable::update(&mut adv.adv_state, &mut update);
            assert!(
                adv.adv_state
                    .character_screen
                    .as_ref()
                    .unwrap()
                    .model
                    .is_closed()
            );
            adv.set_character_scroll_axis(0.0);
            adv.adv_state.character_screen = None;
        }
    }

    // Run SHOWCHARS through ADV's real VM loop, with a command after it that
    // proves the script cannot advance until the close callback is observed.
    {
        let mut bytes = original_scenario.raw().to_vec();
        let code_offset = bytes.len() as u32;
        let mut writer = std::io::Cursor::new(&mut bytes);
        writer.set_position(code_offset as u64);
        for compiletime in [
            C::CHARS(c::CHARS {
                arg1: NumberSpec::constant(0),
                arg2: NumberSpec::constant(0),
            }),
            C::SHOWCHARS(c::SHOWCHARS {}),
            C::SSET(c::SSET {
                slot_number: NumberSpec::constant(9),
                value: NumberSpec::constant(23),
            }),
            C::EXIT(c::EXIT {
                arg1: 0,
                arg2: NumberSpec::constant(0),
            }),
        ] {
            Instruction::Command(compiletime)
                .write_le(&mut writer)
                .unwrap();
        }
        drop(writer);
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        bytes[0x20..0x24].copy_from_slice(&code_offset.to_le_bytes());
        let mut live_assets = assets.clone();
        live_assets.scenario = Arc::new(Scenario::new(bytes.into()).unwrap());
        let mut live = Adv::new(
            adv.adv_state.audio_manager.clone(),
            server.clone(),
            live_assets.clone(),
            Scripter::new(&live_assets.scenario, 0, 42),
        );
        for delta in [1.0, 9.0] {
            frame.advance();
            live.update(
                &mut UpdateContext {
                    frame_started_at: std::time::Instant::now(),
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: &server,
                    pre_render: &mut context,
                },
                enum_map::EnumMap::default(),
            );
            assert!(!live.is_finished());
            assert_eq!(live.vm_state.get_persistent(9), 0);
            assert!(!live.vm_state.system_controls_allowed);
            assert!(live.current_command.is_some());
        }
        assert!(
            live.adv_state
                .character_screen
                .as_ref()
                .unwrap()
                .model
                .transition
                .is_open()
        );
        let mut input = enum_map::EnumMap::default();
        input[crate::app::AppAction::HoldSkip] = shin_input::ActionState {
            is_held: true,
            ..Default::default()
        };
        input[crate::app::AppAction::Enter] = shin_input::ActionState {
            is_clicked: true,
            ..Default::default()
        };
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(1.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            input,
        );
        assert_eq!(live.vm_state.get_persistent(9), 0);
        let mut input = enum_map::EnumMap::default();
        input[crate::app::AppAction::Cancel] = shin_input::ActionState {
            is_clicked: true,
            ..Default::default()
        };
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(7.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            input,
        );
        assert_eq!(live.vm_state.get_persistent(9), 0);
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            },
            enum_map::EnumMap::default(),
        );
        assert_eq!(
            live.adv_state
                .character_screen
                .as_ref()
                .unwrap()
                .model
                .transition
                .progress(),
            1.0
        );
        assert_eq!(live.vm_state.get_persistent(9), 0);
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::from_f32(10.0),
                asset_server: &server,
                pre_render: &mut context,
            },
            enum_map::EnumMap::default(),
        );
        assert_eq!(live.vm_state.get_persistent(9), 0);
        assert!(
            live.adv_state
                .character_screen
                .as_ref()
                .unwrap()
                .model
                .is_closed()
        );
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: std::time::Instant::now(),
                frame_id: frame,
                delta_ticks: Ticks::ZERO,
                asset_server: &server,
                pre_render: &mut context,
            },
            enum_map::EnumMap::default(),
        );
        assert!(live.is_finished());
        assert_eq!(live.vm_state.get_persistent(9), 23);
        assert!(live.vm_state.system_controls_allowed);
        assert!(live.adv_state.character_screen.is_none());
    }
    adv.advance_play_clock(Ticks::from_f32(0.5));
    assert_eq!(adv.play_ticks(), 7440.0);
    drop(adv);
    let final_save = SaveData::decode(&std::fs::read(&disk_path).unwrap()).unwrap();
    assert_eq!(
        final_save.play_seconds, 124,
        "Adv shutdown must persist clock without a new script boundary"
    );
    eprintln!(
        "Live play clock: restore123s, fractional59.5 ticks survive scene loads, shutdown persists124s"
    );
    std::fs::remove_file(disk_path).unwrap();
}
