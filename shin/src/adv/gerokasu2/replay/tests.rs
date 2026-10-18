use super::*;
use binrw::BinWrite;
use shin_core::format::scenario::instruction_elements::{NumberSpec, Register};
use std::io::Cursor;
pub(super) fn reg(n: u16) -> Register {
    Register::from_regular_register(n)
}
pub(super) fn num(d: &mut Vec<u8>, value: i32) {
    let mut c = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(value).write_le(&mut c).unwrap();
    d.extend(c.into_inner());
}
pub(super) fn text(d: &mut Vec<u8>, value: &str) {
    d.extend(((value.len() + 1) as u16).to_le_bytes());
    d.extend(value.as_bytes());
    d.push(0);
}
pub(super) fn scenario(code: impl FnOnce(&mut Vec<u8>)) -> Arc<Gerokasu2Scenario> {
    let mut d = b"SNR ".to_vec();
    for w in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        d.extend(w.to_le_bytes());
    }
    let mut tables = Vec::new();
    for i in 0..10 {
        d.extend((0x4c + tables.len() as u32).to_le_bytes());
        if i < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    d.extend(tables);
    let start = d.len() as u32;
    d[32..36].copy_from_slice(&start.to_le_bytes());
    code(&mut d);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(d.into()).unwrap())
}
pub(super) fn message(d: &mut Vec<u8>) {
    d.push(0x86);
    d.extend(&1u32.to_le_bytes()[..3]);
    d.push(1);
    num(d, 0);
    text(d, "日本@r復元");
}
fn selection(d: &mut Vec<u8>) {
    d.push(0x8d);
    d.extend(1u16.to_le_bytes());
    d.extend(1u16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    num(d, 5);
    text(d, "");
    let s = "A\0B\0C\0\0";
    d.extend((s.len() as u16).to_le_bytes());
    d.extend(s.as_bytes());
}
pub(super) fn saved(
    scenario: Arc<Gerokasu2Scenario>,
    target: u32,
    replay: u32,
    responses: Vec<u8>,
) -> ReplaySession {
    let mut progress = shin_core::format::save::gerokasu2::Progress::fresh(0, 42);
    let e = progress.active_entry_mut();
    e.save_position = target;
    e.replay_target = replay;
    e.responses = responses;
    ReplaySession::from_progress(scenario, progress).unwrap()
}
#[test]
fn ger_replay_resume_blocks_and_bookmark_fork_rewinds_both_targets() {
    let s = scenario(|d| {
        message(d);
        d.push(0xa4);
        message(d);
        d.push(0xa5);
        d.extend([0, 0]);
        num(d, 0);
    });
    let mut replay = saved(s.clone(), 100, 100, vec![]);
    assert_eq!(
        replay.pump(&IndexSet::new(), 64, 100000, || false).unwrap(),
        ReplayStatus::Ready
    );
    assert!(matches!(
        replay.pending.as_ref().unwrap().command,
        Cmd::Shared(RuntimeCommand::RESUME(_))
    ));
    let position = replay.save.timeline.resume_position;
    assert_eq!(position, 2);
    assert_eq!(replay.save.timeline.boundary, 3);
    let (save, checkpoint) = replay.save.fork_for_restore(position, position);
    assert_eq!(save.progress.active_entry().save_position, position);
    assert_eq!(save.progress.active_entry().replay_target, position);
    let mut restored = ReplaySession::new(s, save, checkpoint).unwrap();
    assert_eq!(
        restored
            .pump(&IndexSet::new(), 64, 100000, || false)
            .unwrap(),
        ReplayStatus::Ready
    );
    assert_eq!(restored.save.timeline.boundary, position);
    assert!(matches!(
        restored.pending.as_ref().unwrap().command,
        Cmd::Shared(RuntimeCommand::RESUMESET(_))
    ));
}
#[test]
#[ignore = "requires the Gerokasu2 ROM (SHIN_GEROKASU2_ROM)"]
fn ger_authored_layer_init_operands_inventory_has_no_message_minus5_address() {
    use shin_core::format::{
        rom::RomReader,
        scenario::{
            gerokasu2::bytecode::Instruction,
            instruction_elements::{CodeAddress, UntypedNumberSpec},
            instructions::Instruction as SharedInstruction,
        },
    };
    use shin_core::primitives::stateless_reader::StatelessFile;
    use shin_core::vm::command::CompiletimeCommand;
    use std::io::Read;
    let Some(path) = std::env::var_os("SHIN_GEROKASU2_ROM") else {
        return;
    };
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let mut data = Vec::new();
    let file = rom.find_file("/main.snr").unwrap();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    let scenario = Gerokasu2Scenario::new(data.into()).unwrap();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut counts = std::collections::BTreeMap::new();
    let mut total = 0;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let tail = &scenario.raw()[reader.position().0 as usize..];
        if tail.len() < 16 && tail.iter().all(|&b| b == 0) {
            break;
        }
        let instruction = reader.read().unwrap();
        total += 1;
        if let Instruction::Shared(SharedInstruction::Command(CompiletimeCommand::LAYERINIT(c))) =
            instruction
        {
            let operand = c.layer_id.into_untyped();
            assert_ne!(operand, UntypedNumberSpec::Constant(-5));
            *counts.entry(format!("{operand:?}")).or_insert(0usize) += 1;
        }
    }
    assert_eq!(total, 24187);
    println!(
        "GEROKASU2_AUTHORED_INIT_INVENTORY instructions={total} operands={counts:?} minus5_constant=absent"
    );
}
#[test]
fn ger_replay_select_first_target_and_consumed_response_publish_before_retention() {
    let s = scenario(|d| {
        selection(d);
        d.extend([0x81, 1, 0]);
        num(d, 7);
        message(d);
        d.extend([0, 0, 0]);
    });
    let mut responses = Vec::new();
    let mut cursor = 0;
    responses::write_bits(&mut responses, &mut cursor, 4, 2);
    responses::write_persistent(&mut responses, &mut cursor, -73);
    let seen = IndexSet::new();
    for (target, boundary, cursor, dest, stop) in
        [(1, 1, 0, 0, 0), (2, 2, 4, 2, 1), (3, 3, 21, 2, -1)]
    {
        let mut replay = saved(s.clone(), target, target, responses.clone());
        assert_eq!(
            replay.pump(&seen, 32, 1000, || false).unwrap(),
            ReplayStatus::Ready
        );
        assert_eq!(
            (
                replay.save.timeline.boundary,
                replay.save.response_bit_position,
                replay.vm.read_register(reg(0))
            ),
            (boundary, cursor, dest)
        );
        assert_eq!(replay.pending.as_ref().unwrap().replay_stop, stop);
        if target == 3 {
            assert_eq!(replay.vm.read_register(reg(1)), -73);
            assert_eq!(replay.completed_commands, 2);
            assert!(matches!(
                replay.pending.as_ref().unwrap().command,
                Cmd::MessageSet { .. }
            ));
        }
        assert_eq!(replay.save.progress.active_entry().responses, responses);
    }
}
#[test]
fn ger_replay_recorded_globals_descriptors_and_checkpoint_fork_preserve_current_progress() {
    let s = scenario(|d| {
        d.extend([0x81, 2, 0]);
        num(d, 7);
        d.push(0x82);
        num(d, 8);
        num(d, 123);
        d.push(0x85);
        num(d, 23);
        d.push(0x8b);
        num(d, 2);
        d.push(0x90);
        for v in [1, 0, 0, 1500] {
            num(d, v);
        }
        message(d);
        d.extend([0, 0, 0]);
    });
    let mut route = Vec::new();
    let mut cursor = 0;
    responses::write_persistent(&mut route, &mut cursor, -73);
    let mut replay = saved(s.clone(), 1, 0, route);
    let mut global = shin_core::format::save::PersistData::new();
    global.set(7, 444);
    replay.save.variables.replace_committed(global);
    assert_eq!(
        replay.pump(&IndexSet::new(), 32, 1000, || false).unwrap(),
        ReplayStatus::Ready
    );
    assert_eq!(replay.vm.read_register(reg(2)), -73);
    assert_eq!(replay.save.variables.committed().get(7), 444);
    assert_eq!(replay.save.variables.committed().get(8), 123);
    assert_eq!(replay.save.descriptors.music.id, 1);
    assert_eq!(replay.save.descriptors.music.volume.to_bits(), 0x3fc00001);
    assert_eq!(replay.save.descriptors.message.face, 2);
    let checkpoint = replay.save.checkpoints[0].clone();
    assert!(checkpoint.descriptors.message.text.is_none());
    assert_eq!(checkpoint.descriptors.message.style, 23);
    let current_routes = replay.save.progress.clone();
    replay.save.variables.set(8, 999);
    let (fork, state) = replay.save.fork_for_restore(1, 5);
    assert!(fork.variables.pending().is_empty());
    assert_eq!(fork.variables.committed().get(8), 123);
    assert_eq!(
        fork.progress.entries[0].responses,
        current_routes.entries[0].responses
    );
    assert_eq!(fork.progress.entries[1], current_routes.entries[1]);
    assert_eq!(fork.progress.active_entry().replay_target, 5);
    assert!(fork.checkpoints.is_empty());
    let mut restored = ReplaySession::new(s, fork, state).unwrap();
    assert_eq!(restored.vm.position(), checkpoint.vm.position());
    assert_eq!(
        restored.pump(&IndexSet::new(), 32, 1000, || false).unwrap(),
        ReplayStatus::Ready
    );
    assert!(restored.save.descriptors.message.text.is_some());
}
#[test]
fn ger_replay_event_block_reforks_last_skip_and_cancellation_is_after_completed_poll() {
    let s = scenario(|d| {
        message(d);
        d.push(0xa2);
        num(d, 7);
        d.extend([0, 0, 0]);
    });
    let mut replay = saved(s, 2, 2, Vec::new());
    assert_eq!(
        replay.pump(&IndexSet::new(), 32, 1000, || false).unwrap(),
        ReplayStatus::Ready
    );
    assert_eq!(replay.reforks, 1);
    assert_eq!(replay.save.timeline.event, -1);
    assert_eq!(replay.save.timeline.boundary, 1);
    let s = scenario(|d| {
        d.push(0x85);
        num(d, 3);
        message(d);
        d.extend([0, 0, 0]);
    });
    let mut replay = saved(s, 1, 1, Vec::new());
    assert_eq!(
        replay.pump(&IndexSet::new(), 32, 1000, || true).unwrap(),
        ReplayStatus::Cancelled
    );
    assert_eq!(replay.completed_commands, 1);
    assert_eq!(replay.save.descriptors.message.style, 3);
    assert!(replay.pending.is_none());
    assert_eq!(
        replay.pump(&IndexSet::new(), 32, 1000, || false).unwrap(),
        ReplayStatus::Cancelled
    );
}

#[test]
fn ger_desktop_replay_worker_observes_cancellation_after_a_full_command_chunk() {
    let script = scenario(|d| {
        for _ in 0..6000 {
            d.push(0x85);
            num(d, 3);
        }
        message(d);
    });
    let mut replay = saved(script, u32::MAX, u32::MAX, Vec::new());
    let polls = std::cell::Cell::new(0);
    let result = replay.pump_to_ready(&IndexSet::new(), || {
        polls.set(polls.get() + 1);
        polls.get() > 5000
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(replay.status(), ReplayStatus::Cancelled);
    assert!(replay.completed_commands > 4096 && replay.completed_commands < 6000);
}

#[test]
fn ger_replay_publication_preserves_feedback_and_transfers_history_once() {
    let script = scenario(|d| {
        selection(d);
        message(d);
        d.extend([0, 0, 0]);
    });
    let mut route = Vec::new();
    responses::write_bits(&mut route, &mut 0, 4, 2);
    let mut replay = saved(script.clone(), 2, 2, route);
    assert!(replay.check_publication(&script).is_err());
    replay.pump(&IndexSet::new(), 32, 1000, || false).unwrap();
    replay.check_publication(&script).unwrap();
    let other = scenario(|d| d.extend([0, 0, 0]));
    assert!(replay.check_publication(&other).is_err());
    let cursor = replay.save.response_bit_position;
    let checkpoints = replay.save.checkpoints.len();
    let (mut vm, layers, save, pending, response) = replay.take_live_parts();
    assert_eq!(pending.unwrap().replay_stop, 1);
    assert_eq!(save.response_bit_position, cursor);
    assert_eq!(save.checkpoints.len(), checkpoints);
    assert_eq!(vm.read_register(reg(0)), 2);
    assert!(matches!(
        vm.run(response, 1000).unwrap(),
        Cmd::MessageSet { .. }
    ));
    assert_eq!(vm.read_register(reg(0)), 2);
    assert_eq!(layers.next_initialization_id(), 0);
    assert_eq!(replay.status(), ReplayStatus::Published);
    assert!(replay.check_publication(&script).is_err());
    assert!(replay.pump(&IndexSet::new(), 32, 1000, || false).is_err());
}

#[test]
#[ignore = "requires supplied Ger ROM; memory replay only, no hardware or live effects"]
fn ger_replay_authored_boundaries_and_sparse_restore_redecode_without_hardware() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut rom = RomReader::new(
        StatelessFile::new(std::fs::File::open(root.join("game-data/gerokasu2/data.rom")).unwrap())
            .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap());
    let seen = IndexSet::new();
    let mut total = 0;
    let mut checkpoints = 0;
    for target in [1, 20, 64, 200, 383, 400, 1000] {
        let mut replay = saved(scenario.clone(), target, target, vec![0; 4096]);
        let mut slices = 0;
        while replay.status() != ReplayStatus::Ready {
            replay.pump(&seen, 4096, 100000, || false).unwrap();
            slices += 1;
            assert!(slices < 100, "authored replay did not stop at {target}");
        }
        assert!(replay.pending.is_some());
        assert!(replay.save.timeline.event < 0);
        assert_eq!(replay.save.descriptors.restart_required, 0);
        let boundary = replay.save.timeline.boundary;
        let current_progress = replay.save.progress.clone();
        let prior_steps = replay.completed_commands;
        checkpoints += replay.save.checkpoints.len();
        let (fork, state) = replay.save.fork_for_restore(boundary, boundary);
        let mut restored = ReplaySession::new(scenario.clone(), fork, state).unwrap();
        let mut slices = 0;
        while restored.status() != ReplayStatus::Ready {
            restored.pump(&seen, 4096, 100000, || false).unwrap();
            slices += 1;
            assert!(slices < 100);
        }
        assert_eq!(restored.save.timeline, replay.save.timeline);
        assert_eq!(restored.save.descriptors, replay.save.descriptors);
        assert_eq!(
            restored.layers.reference_bytes(),
            replay.layers.reference_bytes()
        );
        assert_eq!(
            restored.layers.reference_adv_bank_bytes(),
            replay.layers.reference_adv_bank_bytes(),
            "full ADV bank generations and headers must survive replay forks"
        );
        assert_eq!(restored.vm.next_position(), replay.vm.next_position());
        assert_eq!(restored.vm.random_state(), replay.vm.random_state());
        assert_eq!(
            restored.save.response_bit_position,
            replay.save.response_bit_position
        );
        assert_eq!(restored.save.progress, current_progress);
        for index in [0, 1, 2, 31, 63, 1023, 2047] {
            assert_eq!(
                restored.vm.read_register(reg(index)),
                replay.vm.read_register(reg(index))
            );
        }
        println!(
            "GEROKASU2_MEMORY_REPLAY_AUTHORED target={target} boundary={boundary} commands={prior_steps} restored_commands={} checkpoints={} response_bits={} no_live_effects=verified",
            restored.completed_commands,
            replay.save.checkpoints.len(),
            replay.save.response_bit_position
        );
        total += prior_steps + restored.completed_commands;
    }
    println!(
        "GEROKASU2_MEMORY_REPLAY_AUTHORED_MATCH targets=7 commands={total} checkpoints={checkpoints} integer_state_descriptors_PC_seed_responses_current_globals=verified native_whole_replay_parity=unclaimed"
    );
}
