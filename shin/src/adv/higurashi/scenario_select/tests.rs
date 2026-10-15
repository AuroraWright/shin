use super::*;
use anyhow::ensure;
use crate::adv::higurashi::{
    choice::tests::{append_progress, bytes, progress},
    input::ReadState,
    read_state::{Checkpoint, CommandReadState, ReadServices},
};
use shin_core::{
    format::{save::PersistData, scenario::instruction_elements::MessageId},
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
#[derive(Clone)]
struct Snapshot {
    kind: i32,
    progress: SaveProgress,
    tail: Vec<u16>,
    variables: Vec<u8>,
}
#[derive(Default)]
pub(super) struct Services {
    kind: i32,
    captures: u32,
    capture_text: bool,
    clocks: u32,
    queues: u32,
    pending: Option<Snapshot>,
    events: String,
    fail_clock: bool,
    fail_queue: bool,
}
impl ScenarioSelectServices for Services {
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock unavailable");
        self.clocks += 1;
        self.events.push('C');
        Ok(0x00123456789abcde + u64::from(self.clocks))
    }
    fn queue_scene(&mut self, kind: i32, source: &ProgressMemory) -> Result<()> {
        assert_eq!(kind, self.kind);
        ensure!(!self.fail_queue, "queue unavailable");
        self.pending = Some(Snapshot {
            kind,
            progress: source.progress().clone(),
            tail: source.trailing_values()?,
            variables: source.bytes().to_vec(),
        });
        self.queues += 1;
        self.events.push('Q');
        Ok(())
    }
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        assert_eq!(kind, Checkpoint::Interactive);
        self.captures += 1;
        self.events.push('K');
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}
struct Reads;
impl ReadServices for Reads {
    fn checkpoint(&mut self, _: Checkpoint, _: ReadState) -> Result<()> {
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}
fn hash(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn vectors(row: &mut String, tail: &[u16], variables: &[u8]) {
    let data = tail
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    write!(
        row,
        " {} {:016x} {} {:016x}",
        tail.len(),
        hash(&data),
        variables.len(),
        hash(variables)
    )
    .unwrap();
}
struct Profile {
    command: ScenarioSelectCommand,
    read: CommandReadState,
    progress: ProgressMemory,
    memory: AutosaveMemory,
    services: Services,
    input: String,
    expected: Vec<String>,
}
impl Profile {
    fn new(
        kind: i32,
        active: usize,
        length: usize,
        cursor: usize,
        boundary: u32,
        saved: u32,
        text: bool,
        tail: usize,
        variables: usize,
    ) -> Self {
        let seed = 0x5a;
        let mut lengths = [7; 2];
        lengths[active] = length;
        let mut source = progress(active, seed, lengths, false);
        source.entries[active].save_position = saved;
        source.entries[active].replay_target = 0xabcdef01;
        let mut memory =
            ProgressMemory::new(PersistData(vec![]), source, bytes(variables, seed + 1))
                .with_trailing_values((0..tail).map(|i| (seed + i as u32 * 17) as u16).collect());
        memory.restore_response_cursor(cursor).unwrap();
        let mut read = CommandReadState::new();
        if text {
            read.prepare(
                &Cmd::MSGSET {
                    msg_id: MessageId(1),
                    auto_wait: 1,
                    argument: 0,
                    text: "old text".into(),
                },
                &mut Reads,
            )
            .unwrap();
        }
        read.timeline_mut().boundary = boundary;
        read.timeline_mut().resume_position = 0x1234;
        read.timeline_mut().replay_target = 0x4567;
        let cmd = Cmd::SNRSEL { argument: kind };
        let before = read.constructor_capture(&cmd);
        assert_eq!(before.descriptors().message.text.is_some(), text);
        let mut services = Services {
            kind,
            capture_text: before.descriptors().message.text.is_some(),
            ..Default::default()
        };
        read.prepare(&cmd, &mut services).unwrap();
        let mut p = Self {
            command: ScenarioSelectCommand::new(&cmd).unwrap(),
            read,
            progress: memory,
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            services,
            input: format!(
                "N {kind} {active} {seed:x} {length} {cursor} {boundary:x} {saved:x} {} {tail} {variables}\n",
                u8::from(text)
            ),
            expected: vec![],
        };
        p.snapshot(-1);
        p
    }
    fn snapshot(&mut self, result: i32) {
        let t = self.read.timeline();
        let s = &self.services;
        let mut row = format!(
            "S {result} {:08x} {:08x} {:08x} {} {} {} {} {} {} {} {:016x} {}",
            t.boundary,
            t.resume_position,
            t.replay_target,
            self.command.replay_stop(),
            self.progress.responses().1,
            s.captures,
            u8::from(s.capture_text),
            u8::from(self.read.descriptors().message.text.is_some()),
            s.clocks,
            s.queues,
            self.memory.packed_time,
            u8::from(self.memory.valid)
        );
        append_progress(&mut row, self.progress.progress());
        append_progress(&mut row, &self.memory.progress);
        vectors(
            &mut row,
            &self.progress.trailing_values().unwrap(),
            self.progress.bytes(),
        );
        write!(row, " {}", u8::from(s.pending.is_some())).unwrap();
        if let Some(p) = &s.pending {
            write!(row, " {} 1 0011223344556677", p.kind).unwrap();
            append_progress(&mut row, &p.progress);
            vectors(&mut row, &p.tail, &p.variables);
        }
        write!(row, " {}", s.events).unwrap();
        self.expected.push(row);
    }
    fn live(&mut self) -> Result<bool> {
        self.command.poll_live(
            &mut self.progress,
            &mut self.memory,
            self.read.timeline().boundary,
            &mut self.services,
        )
    }
    fn record_live(&mut self) {
        let complete = self.live().unwrap();
        self.input.push_str("L\n");
        self.snapshot(i32::from(complete));
    }
    fn replay(&mut self) -> bool {
        let complete = self
            .command
            .poll_replay(self.progress.progress(), self.read.timeline().boundary)
            .unwrap();
        self.input.push_str("R\n");
        self.snapshot(i32::from(complete));
        complete
    }
    fn predicate(&mut self) {
        assert!(!self.command.forward_complete());
        self.input.push_str("P\n");
        self.snapshot(0);
    }
    fn mutate(&mut self) {
        let mut source = self.progress.progress().clone();
        for entry in &mut source.entries {
            entry.scenario_id ^= 255;
            if let Some(v) = entry.response_data.first_mut() {
                *v ^= 0x55;
            }
        }
        let mut tail = self.progress.trailing_values().unwrap();
        if let Some(v) = tail.first_mut() {
            *v ^= 0x55;
        }
        let mut data = self.progress.bytes().to_vec();
        if let Some(v) = data.first_mut() {
            *v ^= 0x55;
        }
        let cursor = self.progress.responses().1;
        self.progress =
            ProgressMemory::new(PersistData(vec![]), source, data).with_trailing_values(tail);
        self.progress.restore_response_cursor(cursor).unwrap();
        self.input.push_str("M\n");
        self.snapshot(-1);
    }
    fn retire(&mut self) {
        self.services.pending = None;
        self.input.push_str("D\n");
        self.snapshot(-1);
    }
}

const CAPTURE_INPUT: &str = include_str!(
    "../../../../../reverse-engineering/higurashi/scenario-select-command-capture/input.txt"
);
const CAPTURE_OUTPUT: &str = include_str!(
    "../../../../../reverse-engineering/higurashi/scenario-select-command-capture/original-stdout.txt"
);
const CAPTURE_STDERR: &str = include_str!(
    "../../../../../reverse-engineering/higurashi/scenario-select-command-capture/original-stderr.txt"
);

#[test]
fn reference_updated_scenario_select_command_original144_owners972_observations() {
    let mut current: Option<Profile> = None;
    let mut actual = Vec::new();
    let mut regenerated_input = String::new();
    let mut owners = 0;
    for line in CAPTURE_INPUT.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if words[0] == "N" {
            assert!(current.is_none());
            let decimal = |i: usize| words[i].parse::<usize>().unwrap();
            let hex = |i: usize| u32::from_str_radix(words[i], 16).unwrap();
            assert_eq!(hex(3), 0x5a);
            let kind = words[1].parse().unwrap();
            current = Some(Profile::new(
                kind,
                decimal(2),
                decimal(4),
                decimal(5),
                hex(6),
                hex(7),
                decimal(8) != 0,
                decimal(9),
                decimal(10),
            ));
            assert_eq!(current.as_ref().unwrap().command.kind(), kind);
            owners += 1;
        } else {
            let p = current.as_mut().unwrap();
            match words[0] {
                "P" => p.predicate(),
                "R" => {
                    p.replay();
                }
                "L" => p.record_live(),
                "M" => p.mutate(),
                "D" => p.retire(),
                other => panic!("Unknown frozen operation {other}"),
            }
        }
        actual.push(current.as_mut().unwrap().expected.pop().unwrap());
        if words[0] == "D" {
            regenerated_input.push_str(&current.take().unwrap().input);
        }
    }
    assert!(current.is_none());
    assert_eq!(regenerated_input, CAPTURE_INPUT);
    assert_eq!(owners, 144);
    assert_eq!(actual.len(), 972);
    for (i, (actual, native)) in actual.iter().zip(CAPTURE_OUTPUT.lines()).enumerate() {
        assert_eq!(actual, native, "original SNRSEL observation {i}");
    }
    assert_eq!(CAPTURE_OUTPUT.lines().count(), actual.len());
    eprintln!(
        "SNRSEL:144 actual native owners/972 exact observations;all six kinds/both records/four owned vectors/replay stop/constructor wrap/cursor and source preservation/latest-writer queue"
    );
}

#[test]
#[ignore = "actual original AArch64 command reexecution;requires native runner and updated executable;no GPU/audio/window/disk"]
fn reference_updated_scenario_select_command_frozen_original_reexecution() {
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SCENARIO_SELECT_COMMAND").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(CAPTURE_INPUT.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, CAPTURE_OUTPUT.as_bytes());
    assert_eq!(output.stderr, CAPTURE_STDERR.as_bytes());
}

#[test]
fn scenario_select_io_retry_preserves_kind_and_completed_capture() {
    for kind in 0..6 {
        let mut p = Profile::new(kind, 1, 7, 13, 199, 400, true, 3, 7);
        p.services.fail_clock = true;
        assert!(p.live().is_err());
        assert!(p.memory.valid);
        assert_eq!(p.progress.progress().active_entry().save_position, 200);
        assert_eq!(p.services.clocks, 0);
        p.services.fail_clock = false;
        p.services.fail_queue = true;
        assert!(p.live().is_err());
        assert_eq!(p.services.clocks, 1);
        assert_eq!(p.services.queues, 0);
        let captured = p.memory.progress.clone();
        p.services.fail_queue = false;
        assert!(!p.live().unwrap());
        assert_eq!(p.services.clocks, 1);
        assert_eq!(p.services.pending.as_ref().unwrap().kind, kind);
        assert_eq!(p.services.queues, 1);
        assert_eq!(p.progress.responses().1, 13);
        let queued = p.services.pending.clone().unwrap();
        p.mutate();
        assert_eq!(p.memory.progress, captured);
        assert_eq!(
            p.services.pending.as_ref().unwrap().progress,
            queued.progress
        );
        assert_eq!(p.services.pending.as_ref().unwrap().tail, queued.tail);
        assert_eq!(
            p.services.pending.as_ref().unwrap().variables,
            queued.variables
        );
        assert!(!p.live().unwrap());
        assert_eq!(p.services.clocks, 2);
        assert_eq!(p.services.queues, 2);
    }
    for kind in [i32::MIN, -1, 6, i32::MAX] {
        let mut p = Profile::new(kind, 0, 0, 0, 0, 0, false, 0, 0);
        assert_eq!(p.command.kind(), kind);
        assert!(!p.live().unwrap());
        assert_eq!(p.services.pending.as_ref().unwrap().kind, kind);
    }
}
