//! Numeric whole-scenario route exploration for the supplied script.
//!
//! The walker follows every SELECT option and every possible CardSelect/QUIZ
//! face, carries persistent SSET values into later New Game playthroughs and
//! prunes branches whose complete VM state was already explored. It records
//! every evaluated command so the shin crate can audit live owner acceptance
//! and resources. This is numeric control-flow evidence only: no ADV owner,
//! timing, rendering or audio runs here.
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    hash::{Hash, Hasher},
    io::{Read, Write},
};

use super::*;
use crate::{
    format::{rom::RomReader, scenario::gerokasu2::bytecode},
    vm::command::RuntimeCommand,
};

fn supplied_scenario() -> Arc<Gerokasu2Scenario> {
    use shin_primitives::stateless_reader::StatelessFile;
    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap())
}

#[derive(Clone)]
struct Branch {
    vm: Gerokasu2Scripter,
    result: CommandResult,
    persist: BTreeMap<i32, i32>,
    /// CardSelect masks by layer ID, from the latest type-9 LAYERLOAD.
    cards: BTreeMap<i32, i32>,
    choices: Vec<(u32, i32)>,
    /// QUIZ sites resolved by an automatic (0x1000) CardSelect without input.
    automatic: Vec<u32>,
    playthrough: u32,
    commands: u64,
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Possible QUIZ results for the native CardSelect constructor at2ed60.
fn card_faces(raw_mask: i32) -> Vec<i32> {
    if raw_mask & 0x1000 != 0 {
        // Automatic variant: fixed mask/selected index.
        let (mask, selected) = match raw_mask & 15 {
            1 => (11, 1),
            2 => (9, 0),
            _ => (15, 2),
        };
        let faces: Vec<i32> = (0..4).filter(|face| mask & (1 << face) != 0).collect();
        return vec![faces[selected]];
    }
    (0..4).filter(|face| raw_mask & (1 << face) != 0).collect()
}

fn selection_options(count: usize, mask: i32) -> Vec<i32> {
    (0..count.min(12) as i32)
        .filter(|i| mask as u32 & (1 << i) != 0)
        .collect()
}

#[test]
#[ignore = "requires supplied Gerokasu2 data.rom (SHIN_GEROKASU2_ROM); numeric routes only"]
fn gerokasu2_supplied_route_exploration_covers_script() {
    let scenario = supplied_scenario();
    let seed: u32 = std::env::var("SHIN_GEROKASU2_ROUTE_SEED")
        .map(|value| value.parse().unwrap())
        .unwrap_or(42);
    let max_playthroughs: u32 = std::env::var("SHIN_GEROKASU2_ROUTE_PLAYTHROUGHS")
        .map(|value| value.parse().unwrap())
        .unwrap_or(8);

    // Static inventory: every command-yielding instruction and dialogue ID.
    let mut static_commands = BTreeSet::new();
    let mut static_text = BTreeMap::new();
    let mut static_messages = BTreeSet::new();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    while (reader.position().0 as usize) < scenario.raw().len() {
        let remaining = &scenario.raw()[reader.position().0 as usize..];
        if remaining.len() < 16 && remaining.iter().all(|&byte| byte == 0) {
            break;
        }
        let start = reader.position().0;
        let instruction = reader.read().unwrap();
        static_text.insert(start, format!("{instruction:?}").chars().take(240).collect::<String>());
        match instruction {
            bytecode::Instruction::MessageSet { msg_id, .. } => {
                static_commands.insert(start);
                if msg_id.0 > 0 {
                    static_messages.insert(msg_id.0);
                }
            }
            bytecode::Instruction::Wait { .. }
            | bytecode::Instruction::MessageFace { .. }
            | bytecode::Instruction::GerMessageFace { .. }
            | bytecode::Instruction::Shared(Instruction::Command(_)) => {
                static_commands.insert(start);
            }
            _ => {}
        }
    }

    let mut covered_commands = BTreeSet::new();
    let mut covered_messages = BTreeSet::new();
    let mut seen_choice_states = HashSet::new();
    let mut seen_playthrough_starts = HashSet::new();
    let mut endings: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut ending_routes = Vec::new();
    let mut inventory: BTreeSet<String> = BTreeSet::new();
    let mut faults = Vec::new();
    let mut total_commands = 0u64;
    let mut longest = 0u64;
    let mut branches_run = 0usize;

    let start = |persist: BTreeMap<i32, i32>, playthrough| Branch {
        vm: Gerokasu2Scripter::new(scenario.clone(), 0, seed),
        result: CommandResult::None,
        persist,
        cards: BTreeMap::new(),
        choices: Vec::new(),
        automatic: Vec::new(),
        playthrough,
        commands: 0,
    };
    seen_playthrough_starts.insert(hash_of(&BTreeMap::<i32, i32>::new()));
    let mut stack = vec![start(BTreeMap::new(), 0)];
    'branches: while let Some(mut branch) = stack.pop() {
        branches_run += 1;
        assert!(branches_run < 200_000, "route explosion");
        loop {
            let result = std::mem::replace(&mut branch.result, CommandResult::None);
            let command = match branch.vm.run(result, 10_000_000) {
                Ok(command) => command,
                Err(error) => {
                    faults.push(format!(
                        "playthrough{} choices={:?} after {} commands: {error:#}",
                        branch.playthrough, branch.choices, branch.commands
                    ));
                    continue 'branches;
                }
            };
            let pc = branch.vm.position().0;
            branch.commands += 1;
            total_commands += 1;
            covered_commands.insert(pc);
            match &command {
                Gerokasu2RuntimeCommand::MessageSet { msg_id, .. } => {
                    covered_messages.insert(msg_id.0.wrapping_add(1));
                }
                Gerokasu2RuntimeCommand::LayerLoad {
                    layer_id,
                    layer_type: 9,
                    params,
                    ..
                } => {
                    branch.cards.insert(*layer_id, params.1);
                }
                Gerokasu2RuntimeCommand::LayerUnload { layer_id, .. } => {
                    branch.cards.remove(layer_id);
                }
                _ => {}
            }
            inventory.insert(match &command {
                // Message text is covered by the separate layout corpus.
                Gerokasu2RuntimeCommand::MessageSet {
                    auto_wait,
                    message_arg,
                    ..
                } => format!("MessageSet auto_wait={auto_wait} message_arg={message_arg}"),
                Gerokasu2RuntimeCommand::Selection {
                    visibility,
                    variants,
                    ..
                } => format!("Selection count={} visibility={visibility:#x}", variants.len()),
                Gerokasu2RuntimeCommand::Wait { allow_interrupt, .. } => {
                    format!("Wait allow_interrupt={allow_interrupt}")
                }
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SGET(c)) => {
                    format!("SGET slot={}", c.slot_number)
                }
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SSET(c)) => {
                    format!("SSET slot={} value={}", c.slot_number, c.value)
                }
                other => format!("{other:?}"),
            });

            let options: Option<Vec<i32>> = match &command {
                Gerokasu2RuntimeCommand::Selection {
                    visibility,
                    variants,
                    ..
                } => Some(selection_options(variants.len(), *visibility)),
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::QUIZ(c)) => Some(
                    branch
                        .cards
                        .get(&c.arg)
                        .map(|&mask| card_faces(mask))
                        // Missing/noninteractive drawable: native zero result.
                        .unwrap_or_else(|| vec![0]),
                ),
                _ => None,
            };
            if let Gerokasu2RuntimeCommand::Shared(RuntimeCommand::QUIZ(c)) = &command
                && branch.cards.get(&c.arg).is_some_and(|mask| mask & 0x1000 != 0)
            {
                branch.automatic.push(pc);
            }
            if let Some(options) = options {
                let destination = branch.vm.pending_destination().unwrap();
                if options.is_empty() {
                    faults.push(format!(
                        "playthrough{} empty choice at {pc:x} after {:?}",
                        branch.playthrough, branch.choices
                    ));
                    continue 'branches;
                }
                let key = hash_of(&(
                    branch.vm.ctx.fingerprint(),
                    pc,
                    &branch.persist,
                    &branch.cards,
                    branch.playthrough.min(1),
                ));
                if !seen_choice_states.insert(key) {
                    continue 'branches;
                }
                for &option in options.iter().rev() {
                    let mut next = branch.clone();
                    next.result = CommandResult::WriteMemory(destination, option);
                    next.choices.push((pc, option));
                    stack.push(next);
                }
                continue 'branches;
            }
            match command {
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SGET(c)) => {
                    let value = branch.persist.get(&c.slot_number).copied().unwrap_or(0);
                    branch.result =
                        CommandResult::WriteMemory(branch.vm.pending_destination().unwrap(), value);
                }
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SSET(c)) => {
                    branch.persist.insert(c.slot_number, c.value);
                }
                Gerokasu2RuntimeCommand::Shared(RuntimeCommand::EXIT(_))
                    if branch.vm.is_halted() =>
                {
                    longest = longest.max(branch.commands);
                    *endings.entry((pc, branch.playthrough)).or_default() += 1;
                    ending_routes.push(format!(
                        "playthrough{} EXIT@{pc:x} commands={} choices={:x?} automatic={:x?} persist={:?}",
                        branch.playthrough,
                        branch.commands,
                        branch.choices,
                        branch.automatic,
                        branch.persist
                    ));
                    if branch.playthrough + 1 < max_playthroughs
                        && seen_playthrough_starts.insert(hash_of(&branch.persist))
                    {
                        stack.push(start(branch.persist.clone(), branch.playthrough + 1));
                    }
                    continue 'branches;
                }
                _ => {}
            }
        }
    }

    let unreached_messages: Vec<u32> = static_messages
        .difference(&covered_messages)
        .copied()
        .collect();
    let unreached_commands: Vec<u32> = static_commands
        .difference(&covered_commands)
        .copied()
        .collect();
    let compress = |values: &[u32]| {
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for &value in values {
            match ranges.last_mut() {
                Some((_, end)) if *end + 1 == value => *end = value,
                _ => ranges.push((value, value)),
            }
        }
        ranges
    };
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("gerokasu2/.local/playability");
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut file = std::fs::File::create(out_dir.join(format!("routes-seed{seed}.txt"))).unwrap();
    writeln!(file, "seed={seed} branches={branches_run} total_commands={total_commands} longest={longest}").unwrap();
    writeln!(file, "endings={endings:x?}").unwrap();
    for route in &ending_routes {
        writeln!(file, "{route}").unwrap();
    }
    writeln!(file, "faults={faults:#?}").unwrap();
    writeln!(
        file,
        "unreached_messages={} {:?}",
        unreached_messages.len(),
        compress(&unreached_messages)
    )
    .unwrap();
    writeln!(
        file,
        "unreached_command_pcs={} {:x?}",
        unreached_commands.len(),
        unreached_commands
    )
    .unwrap();
    for &pc in &unreached_commands {
        writeln!(file, "  {pc:x}: {}", static_text[&pc]).unwrap();
    }
    std::fs::write(
        out_dir.join(format!("command-inventory-seed{seed}.txt")),
        inventory.iter().cloned().collect::<Vec<_>>().join("\n") + "\n",
    )
    .unwrap();
    println!(
        "GEROKASU2_ROUTES seed={seed} branches={branches_run} endings={} routes={} longest={longest} messages={}/{} commands={}/{} faults={} inventory={}",
        endings.len(),
        ending_routes.len(),
        covered_messages.intersection(&static_messages).count(),
        static_messages.len(),
        covered_commands.len(),
        static_commands.len(),
        faults.len(),
        inventory.len()
    );
    assert!(faults.is_empty(), "{faults:#?}");
}
