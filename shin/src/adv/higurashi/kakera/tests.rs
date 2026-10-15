use super::*;
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
    progress: SaveProgress,
    tail: Vec<u16>,
    variables: Vec<u8>,
}
#[derive(Default)]
pub(super) struct Services {
    captures: u32,
    capture_text: bool,
    clocks: u32,
    queues: u32,
    pending: Option<Snapshot>,
    events: String,
    fail_clock: bool,
    fail_queue: bool,
}
impl KakeraServices for Services {
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock unavailable");
        self.clocks += 1;
        self.events.push('C');
        Ok(0x00123456789abcde + u64::from(self.clocks))
    }
    fn queue_scene(&mut self, mode: i32, source: &ProgressMemory) -> Result<()> {
        assert_eq!(mode, 0);
        ensure!(!self.fail_queue, "queue unavailable");
        self.pending = Some(Snapshot {
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
    command: KakeraCommand,
    read: CommandReadState,
    progress: ProgressMemory,
    memory: AutosaveMemory,
    services: Services,
    input: String,
    expected: Vec<String>,
}
impl Profile {
    fn new(
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
        let before = read.constructor_capture(&Cmd::KAKERA {});
        assert_eq!(before.descriptors().message.text.is_some(), text);
        let mut services = Services {
            capture_text: before.descriptors().message.text.is_some(),
            ..Default::default()
        };
        read.prepare(&Cmd::KAKERA {}, &mut services).unwrap();
        let mut p = Self {
            command: KakeraCommand::new(),
            read,
            progress: memory,
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            services,
            input: format!(
                "N {active} {seed:x} {length} {cursor} {boundary:x} {saved:x} {} {tail} {variables}\n",
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
            "K {result} {:08x} {:08x} {:08x} {} {} {} {} {} {} {} {:016x} {}",
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
            row.push_str(" 0 1 0011223344556677");
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
#[test]
#[ignore = "real KAKERA constructor/replay/search/live/11780/656c0 full GameData/275b0 latest-writer queue/destruction with checked libc/capture/clock;no menu/GPU/audio/saves"]
fn higurashi_kakera_command_and_owned_scene_request_match_native() {
    let mut input = String::new();
    let mut expected = vec![];
    let mut owners = 0;
    for active in 0..2 {
        for length in [0, 1, 1024] {
            for offset in [0, 7].into_iter().take(if length == 0 { 1 } else { 2 }) {
                for text in [false, true] {
                    for tail in [0, 3, 25] {
                        let cursor = if length == 0 {
                            0
                        } else {
                            (length - 1) * 8 + offset
                        };
                        let mut p =
                            Profile::new(active, length, cursor, 199, 400, text, tail, tail + 3);
                        p.predicate();
                        p.record_live();
                        assert_eq!(p.progress.responses().1, cursor);
                        let frozen = p.services.pending.clone().unwrap();
                        p.mutate();
                        assert_eq!(
                            p.services.pending.as_ref().unwrap().progress,
                            frozen.progress
                        );
                        p.record_live();
                        assert_eq!(p.services.clocks, 2);
                        assert_eq!(p.services.queues, 2);
                        assert_eq!(p.progress.responses().1, cursor);
                        assert_eq!(p.read.timeline().boundary, 200);
                        p.retire();
                        input.push_str(&p.input);
                        expected.extend(p.expected);
                        owners += 1;
                    }
                }
            }
        }
    }
    for active in 0..2 {
        for (boundary, saved) in [
            (0, 0),
            (0, 1),
            (0, 2),
            (199, 200),
            (199, 400),
            (u32::MAX, 0),
            (u32::MAX, u32::MAX),
            (u32::MAX - 1, u32::MAX),
        ] {
            for text in [false, true] {
                let mut p = Profile::new(active, 7, 13, boundary, saved, text, 3, 7);
                let complete = p.replay();
                p.predicate();
                if !complete {
                    p.record_live();
                    assert_eq!(p.services.clocks, 0);
                    assert_eq!(p.services.queues, 0);
                }
                assert_eq!(p.progress.responses().1, 13);
                p.retire();
                input.push_str(&p.input);
                expected.extend(p.expected);
                owners += 1;
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_KAKERA_COMMAND").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_KAKERA_COMMAND_CAPTURE") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), input).unwrap();
        std::fs::write(dir.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (i, (a, e)) in actual.lines().zip(&expected).enumerate() {
        assert_eq!(a, e, "native Kakera row{i}");
    }
    eprintln!(
        "Kakera commands:{owners} native owners,{} exact observations;preserved text/resume/replay positions,one constructor boundary,no response bits,11780 and both full records/four independent vectors,factory0/latest-writer queue,replay-stop live skip and balanced closure retirement pass",
        expected.len()
    );
}
#[test]
fn higurashi_kakera_failures_retry_only_uncaptured_clock_or_unqueued_scene() {
    let mut p = Profile::new(1, 7, 13, 199, 400, true, 3, 7);
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
    p.services.fail_queue = false;
    assert!(!p.live().unwrap());
    assert_eq!(p.services.clocks, 1);
    assert_eq!(p.services.queues, 1);
    assert_eq!(p.progress.responses().1, 13);
    assert!(!p.live().unwrap());
    assert_eq!(p.services.clocks, 2);
    assert_eq!(p.services.queues, 2);
}
