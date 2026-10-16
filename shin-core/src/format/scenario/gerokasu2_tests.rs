use super::*;
use crate::format::rom::RomReader;
use shin_primitives::stateless_reader::StatelessFile;
use std::{fs::File, io::Read};

fn empty_scenario() -> Bytes {
    let mut data = b"SNR ".to_vec();
    for word in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        data.extend(word.to_le_bytes());
    }
    let mut tables = Vec::new();
    for index in 0..10 {
        data.extend((0x4cu32 + tables.len() as u32).to_le_bytes());
        if index < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        // Sized tables contain a count; character gallery is an empty segment list.
        tables.extend(0u32.to_le_bytes());
    }
    data.extend(tables);
    let code = data.len() as u32;
    data[32..36].copy_from_slice(&code.to_le_bytes());
    data.extend([0, 0, 0]);
    let length = data.len() as u32;
    data[4..8].copy_from_slice(&length.to_le_bytes());
    data.into()
}

#[test]
fn rejects_wrong_dialects_and_unbounded_table_spans() {
    let valid = empty_scenario();
    assert!(Gerokasu2Scenario::new(valid.clone()).is_ok());
    for (at, value) in [
        (4, 1),
        (12, 6),
        (16, 19),
        (32, u32::MAX),
        (36, 0),
        (40, 0x4c),
        (44, u32::MAX),
        (0x4c, u32::MAX),
        (0x50, u32::MAX),
    ] {
        let mut bad = valid.to_vec();
        bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Gerokasu2Scenario::new(bad.into()).is_err(),
            "accepted mutation at {at:x}"
        );
    }
}

#[test]
#[ignore = "requires supplied Gerokasu2 data.rom (SHIN_GEROKASU2_ROM)"]
fn gerokasu2_supplied_scenario_metadata() {
    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    let scenario = Gerokasu2Scenario::new(data.into()).unwrap();
    assert_eq!(scenario.header.code_offset, 0xc280);
    assert_eq!(scenario.header.dialogue_line_count, 5927);
    assert_eq!(
        [
            scenario.mask_info.len(),
            scenario.picture_info.len(),
            scenario.bustup_info.len(),
            scenario.bgm_info.len(),
            scenario.se_info.len(),
            scenario.movie_info.len(),
            scenario.voice_mapping_info.len(),
            scenario.picture_box_info.len(),
            scenario.music_box_info.len()
        ],
        [14, 262, 644, 21, 48, 3, 7, 29, 21]
    );
    assert_eq!(scenario.bustup_info[0].name.as_str(), "レモン立ち絵A_制服");
    assert_eq!(scenario.bustup_info[0].emotion.as_str(), "通常_なし");
    assert_eq!(scenario.bustup_info[0].raw_placement, [-164, 80, 800]);
    let paths = scenario.asset_paths();
    let missing = paths
        .iter()
        .filter(|path| rom.find_file(path).is_err())
        .collect::<Vec<_>>();
    println!(
        "Gerokasu2: {} unique resource paths; {} missing references: {:?}; {} character-gallery segments",
        paths.len(),
        missing.len(),
        missing,
        scenario.character_box_info.len()
    );
    assert!(paths.contains("/bustup/m/レモン立ち絵a_制服.bup"));
    assert!(rom.find_file("/bustup/m/レモン立ち絵a_制服.bup").is_ok());
    assert_eq!(paths.len(), 325);
    assert_eq!(
        missing.iter().map(|path| path.as_str()).collect::<Vec<_>>(),
        [
            "/picture/cg/?.pic",
            "/picture/cutin/?.pic",
            "/picture/cutin/手札_ツーペア.pic"
        ]
    );
    assert_eq!(scenario.character_box_info.len(), 712);
}

#[test]
#[ignore = "requires supplied Gerokasu2 data.rom (SHIN_GEROKASU2_ROM); syntax inventory only"]
fn gerokasu2_supplied_bytecode_inventory() {
    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    let scenario = Gerokasu2Scenario::new(data.into()).unwrap();
    let mut reader = scenario.instruction_reader(super::super::instruction_elements::CodeAddress(
        scenario.header.code_offset,
    ));
    let mut opcodes = std::collections::BTreeMap::new();
    let mut recent = std::collections::VecDeque::new();
    let mut starts = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut message_ids = BTreeSet::new();
    let mut trailing_zeros = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let start = reader.position().0 as usize;
        let remaining = &scenario.raw()[start..];
        // The supplied SNR ends with fourteen zero bytes after its last
        // retsub and is aligned to16bytes. These are not authored EXITs.
        if remaining.len() < 16 && remaining.iter().all(|&byte| byte == 0) {
            assert_eq!(scenario.raw().len() % 16, 0);
            trailing_zeros = remaining.len();
            break;
        }
        let instruction = reader.read().unwrap_or_else(|error| {
            panic!(
                "{error:#}; recent={recent:x?}; bytes={:x?}",
                &scenario.raw()[start..scenario.raw().len().min(start + 32)]
            )
        });
        let end = reader.position().0 as usize;
        assert!(end > start && end <= scenario.raw().len());
        *opcodes.entry(scenario.raw()[start]).or_insert(0usize) += 1;
        starts.insert(start as u32);
        recent.push_back((start, end, scenario.raw()[start]));
        if recent.len() > 5 {
            recent.pop_front();
        }
        match instruction {
            bytecode::Instruction::MessageSet { text, msg_id, .. } => {
                assert!(!text.as_str().contains('\0'));
                if msg_id.0 > 0 {
                    message_ids.insert(msg_id.0);
                }
            }
            bytecode::Instruction::CallTable { targets: table, .. } => {
                targets.extend(table.0.iter().map(|address| address.0))
            }
            bytecode::Instruction::Shared(
                super::super::instructions::Instruction::j { target }
                | super::super::instructions::Instruction::jc { target, .. }
                | super::super::instructions::Instruction::gosub { target }
                | super::super::instructions::Instruction::call { target, .. },
            ) => {
                targets.insert(target.0);
            }
            bytecode::Instruction::Shared(super::super::instructions::Instruction::jt {
                table,
                ..
            }) => targets.extend(table.0.iter().map(|address| address.0)),
            _ => {}
        }
    }
    assert_eq!(trailing_zeros, 14);
    assert!(
        targets.is_subset(&starts),
        "Non-instruction branch targets: {:?}",
        targets.difference(&starts).collect::<Vec<_>>()
    );
    assert_eq!(opcodes.values().sum::<usize>(), 24187);
    assert_eq!(opcodes.len(), 48);
    assert_eq!(targets.len(), 81);
    assert_eq!(message_ids.len(), 5926);
    assert_eq!(message_ids.last().copied(), Some(5926));
    println!(
        "Gerokasu2 complete syntax inventory: {} instructions, {} opcode values, {} aligned direct branch targets, {} dialogue IDs (max {:?}), {trailing_zeros} trailing zero bytes; counts={opcodes:x?}",
        opcodes.values().sum::<usize>(),
        opcodes.len(),
        targets.len(),
        message_ids.len(),
        message_ids.last()
    );
}

fn supplied_gerokasu() -> Gerokasu2Scenario {
    let path = std::env::var_os("SHIN_GEROKASU_ROM").expect("SHIN_GEROKASU_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    Gerokasu2Scenario::new(data.into()).unwrap()
}

#[test]
#[ignore = "requires supplied Gerokasu data.rom (SHIN_GEROKASU_ROM); syntax inventory only"]
fn gerokasu_supplied_bytecode_inventory() {
    let scenario = supplied_gerokasu();
    assert_eq!(scenario.dialect, Dialect::Gerokasu);
    let mut reader = scenario.instruction_reader(super::super::instruction_elements::CodeAddress(
        scenario.header.code_offset,
    ));
    let mut opcodes = std::collections::BTreeMap::new();
    let mut recent = std::collections::VecDeque::new();
    let mut starts = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut message_ids = BTreeSet::new();
    let mut samples = 0;
    let mut trailing_zeros = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let start = reader.position().0 as usize;
        let remaining = &scenario.raw()[start..];
        if remaining.len() < 16 && remaining.iter().all(|&byte| byte == 0) {
            trailing_zeros = remaining.len();
            break;
        }
        let instruction = reader.read().unwrap_or_else(|error| {
            panic!(
                "{error:#}; recent={recent:x?}; bytes={:x?}",
                &scenario.raw()[start..scenario.raw().len().min(start + 32)]
            )
        });
        let end = reader.position().0 as usize;
        if std::env::var_os("SHIN_GEROKASU_LISTING").is_some() {
            println!("L {start:06x} {instruction:?}");
        }
        *opcodes.entry(scenario.raw()[start]).or_insert(0usize) += 1;
        starts.insert(start as u32);
        recent.push_back((start, end, scenario.raw()[start]));
        if recent.len() > 5 {
            recent.pop_front();
        }
        match &instruction {
            bytecode::Instruction::MessageSet { text, msg_id, .. } => {
                if samples < 40 || std::env::var_os("SHIN_GEROKASU_ALL_TEXT").is_some() {
                    samples += 1;
                    println!("MSG {start:#x} {:?} {}", msg_id.0, text.as_str());
                }
                if msg_id.0 > 0 {
                    message_ids.insert(msg_id.0);
                }
            }
            bytecode::Instruction::CallTable { targets: table, .. } => {
                targets.extend(table.0.iter().map(|address| address.0))
            }
            bytecode::Instruction::Shared(
                super::super::instructions::Instruction::j { target }
                | super::super::instructions::Instruction::jc { target, .. }
                | super::super::instructions::Instruction::gosub { target }
                | super::super::instructions::Instruction::call { target, .. },
            ) => {
                targets.insert(target.0);
            }
            bytecode::Instruction::Shared(super::super::instructions::Instruction::jt {
                table,
                ..
            }) => targets.extend(table.0.iter().map(|address| address.0)),
            bytecode::Instruction::Shared(super::super::instructions::Instruction::Command(
                command,
            )) => {
                if std::env::var_os("SHIN_GEROKASU_COMMANDS").is_some() {
                    println!("CMD {start:#x} {command:?}");
                }
            }
            _ => {}
        }
    }
    assert!(
        targets.is_subset(&starts),
        "Non-instruction branch targets: {:?}",
        targets.difference(&starts).collect::<Vec<_>>()
    );
    println!(
        "Gerokasu complete syntax inventory: {} instructions, {} opcode values, {} direct branch targets, {} dialogue IDs (max {:?}), {trailing_zeros} trailing zero bytes; counts={opcodes:x?}",
        opcodes.values().sum::<usize>(),
        opcodes.len(),
        targets.len(),
        message_ids.len(),
        message_ids.last()
    );
}
