use super::*;
use crate::adv::higurashi::{
    choice::tests::{append_progress, bytes, progress},
    input::ReadState,
    layers::replay_snapshot,
    read_state::{Checkpoint, CommandReadState, ReadServices},
};
use shin_core::format::{save::PersistData, scenario::instruction_elements::MessageId};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

fn hash(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn command(ids: &[i32]) -> Cmd {
    Cmd::TIPSGET {
        ids: ids.iter().copied().collect(),
    }
}
#[derive(Default)]
struct Services {
    ready: bool,
    selected: Option<i32>,
    requests: u32,
    polls: u32,
    attached: bool,
    clocks: u32,
    gates: u32,
    queues: u32,
    bits: u32,
    queued: Option<TipsSceneRequest>,
    events: String,
    fail_prepare: bool,
    fail_clock: bool,
    fail_gate: bool,
    fail_poll: bool,
    fail_queue: bool,
}
impl TipsGetServices for Services {
    fn prepare(
        &mut self,
        request: &TipsGetRequest,
        alternate: bool,
        target: Target,
        _: &mut HigurashiLayers,
    ) -> Result<bool> {
        ensure!(!self.fail_prepare, "producer service failed");
        assert_eq!(target.bank, 0);
        let params = request.producer_params(alternate);
        assert_eq!(params[0], i32::from(alternate));
        assert_eq!(params[1], request.ids.len() as i32);
        assert_eq!(&params[2..2 + request.ids.len()], request.ids);
        if self.requests == 0 {
            self.requests = 1;
            self.events.push('F');
        }
        self.polls += 1;
        if self.ready {
            assert!(!self.attached);
            self.attached = true;
            self.events.push_str("TWSU");
        }
        Ok(self.ready)
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        ensure!(!self.fail_poll, "callback service failed");
        Ok(self.selected)
    }
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock service failed");
        self.clocks += 1;
        self.events.push('C');
        Ok(0x00123456789abcde + u64::from(self.clocks))
    }
    fn opening_copied(&mut self, source: &ProgressMemory) -> Result<()> {
        ensure!(!self.fail_gate, "opening catalog service failed");
        // Original-side fixture supplies descriptors1/9999/10000/16: all except
        // 10000 pass(u16scenario>>4)<=624, so required evenbits are0/2/6.
        if source.progress().active_entry().scenario_id <= 9999 && self.bits & 69 == 69 {
            self.gates += 1;
            self.events.push('G');
        }
        Ok(())
    }
    fn queue_scene(&mut self, request: &TipsSceneRequest) -> Result<()> {
        ensure!(!self.fail_queue, "scene queue service failed");
        self.queues += 1;
        self.queued = Some(request.clone());
        self.events.push('Q');
        Ok(())
    }
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        assert_eq!(kind, Checkpoint::Interactive);
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
struct NoMessageCapture;
impl ReadServices for NoMessageCapture {
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
struct Profile {
    owner: TipsGetCommand,
    target: Target,
    layers: HigurashiLayers,
    read: CommandReadState,
    progress: ProgressMemory,
    memory: AutosaveMemory,
    services: Services,
    constructor_word: u32,
    input: String,
    expected: Vec<String>,
    released: bool,
}
impl Profile {
    #[allow(clippy::too_many_arguments)]
    fn new(
        active: usize,
        length: usize,
        cursor: usize,
        boundary: u32,
        saved: u32,
        text: bool,
        context: u32,
        used: usize,
        ids: &[i32],
        scenario: i32,
        bits: u32,
        tail: usize,
        variables: usize,
    ) -> Self {
        let seed = 0x5a;
        let mut lengths = [7; 2];
        lengths[active] = length;
        let mut source = progress(active, seed, lengths, false);
        source.entries[active].scenario_id = scenario;
        source.entries[active].save_position = saved;
        source.entries[active].replay_target = 0xabcdef01;
        let trailing = (0..tail)
            .map(|i| seed.wrapping_add(i as u32 * 17) as u16)
            .collect();
        let mut p = ProgressMemory::new(PersistData(vec![]), source, bytes(variables, seed + 1))
            .with_trailing_values(trailing);
        p.restore_response_cursor(cursor).unwrap();
        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        for i in 0..used {
            assert_eq!(layers.state.allocate((i * 2) as u16), Some(i as u8));
        }
        let mut read = CommandReadState::new();
        if text {
            read.prepare(
                &Cmd::MSGSET {
                    msg_id: MessageId(1),
                    auto_wait: 1,
                    argument: 0,
                    text: "old text".into(),
                },
                &mut NoMessageCapture,
            )
            .unwrap();
        }
        read.timeline_mut().boundary = boundary;
        read.timeline_mut().resume_position = 0x1234;
        read.timeline_mut().replay_target = 0x4567;
        let ctor = command(ids);
        let before = read.constructor_capture(&ctor);
        assert_eq!(before.timeline().boundary, boundary);
        assert_eq!(before.descriptors().message.text.is_some(), text);
        let mut services = Services {
            bits,
            ..Default::default()
        };
        read.prepare(&ctor, &mut services).unwrap();
        assert_eq!(read.timeline().boundary, boundary.wrapping_add(1));
        assert_eq!(read.timeline().resume_position, 0x1234);
        assert_eq!(read.timeline().replay_target, 0x4567);
        assert_eq!(read.descriptors().message.text.is_some(), text);
        let owner = TipsGetCommand::new(&ctor, &mut layers.state).unwrap();
        let target = owner.target().unwrap();
        let mut input = format!(
            "N {active} {seed:x} {length} {cursor} {boundary:x} {saved:x} {} {context:x} {used} {} {:x} {bits:x} {tail} {variables}",
            u8::from(text),
            ids.len(),
            scenario as u32
        );
        for id in ids {
            write!(input, " {:x}", *id as u32).unwrap();
        }
        input.push('\n');
        let mut out = Self {
            owner,
            target,
            layers,
            read,
            progress: p,
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            services,
            constructor_word: context,
            input,
            expected: vec![],
            released: false,
        };
        out.snapshot(-1);
        out
    }
    fn snapshot(&mut self, result: i32) {
        let t = self.read.timeline();
        let s = &self.services;
        let mut row = format!(
            "T {result} {:08x} {:08x} {:08x} {} {} {} {} {} {} {} {} {} {} {} {} {} {:016x}",
            t.boundary,
            t.resume_position,
            t.replay_target,
            self.owner.replay_stop(),
            u8::from(self.owner.alternate()),
            self.progress.responses().1,
            s.requests,
            s.polls,
            u8::from(s.attached),
            u8::from(s.attached),
            s.clocks,
            s.gates,
            s.queues,
            if s.attached { 3 } else { 0 },
            u8::from(self.read.descriptors().message.text.is_some()),
            u8::from(self.memory.valid),
            self.memory.packed_time
        );
        append_progress(&mut row, self.progress.progress());
        append_progress(&mut row, &self.memory.progress);
        write!(
            row,
            " {} {:016x} {} {:016x}",
            self.progress.trailing_bytes().len() / 2,
            hash(self.progress.trailing_bytes()),
            self.progress.bytes().len(),
            hash(self.progress.bytes())
        )
        .unwrap();
        write!(row, " {}", u8::from(s.queued.is_some())).unwrap();
        if let Some(q) = &s.queued {
            write!(row, " {} {:016x}", q.selected, 0x0011223344556677u64).unwrap();
            append_progress(&mut row, &q.progress);
            let trailing: Vec<_> = q
                .trailing_values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            write!(
                row,
                " {} {:016x} {} {:016x}",
                q.trailing_values.len(),
                hash(&trailing),
                q.byte_variables.len(),
                hash(&q.byte_variables)
            )
            .unwrap();
        }
        write!(
            row,
            " {} {} {} {} {} {} {} {:016x}",
            s.events,
            self.target.logical,
            self.target.slot,
            self.layers
                .state
                .target(self.target.logical)
                .map_or(32, |t| t.slot),
            u8::from(s.requests > 0 && !self.released),
            if s.attached && !self.released { 2 } else { 0 },
            if s.attached && !self.released { 2 } else { 0 },
            replay_snapshot(&self.layers.state)
        )
        .unwrap();
        self.expected.push(row);
    }
    fn live(&mut self) -> Result<bool> {
        self.owner.poll_live(
            &mut self.progress,
            &mut self.memory,
            &mut self.read.timeline_mut().boundary,
            &mut self.layers,
            &mut self.services,
        )
    }
    fn live_record(&mut self) {
        let result = self.live().unwrap();
        self.input.push_str("L\n");
        self.snapshot(i32::from(result));
    }
    fn replay_record(&mut self) -> bool {
        let result = self
            .owner
            .poll_replay(
                &self.progress,
                &mut self.read.timeline_mut().boundary,
                self.constructor_word,
            )
            .unwrap();
        self.input.push_str("R\n");
        self.snapshot(i32::from(result));
        result
    }
    fn forward_record(&mut self) {
        let result = self.owner.forward_complete();
        self.input.push_str("P\n");
        self.snapshot(i32::from(result));
    }
    fn ready(&mut self) {
        self.services.ready = true;
        self.input.push_str("Q 1\n");
        self.snapshot(-1);
    }
    fn select(&mut self, result: i32) {
        self.services.selected = Some(result);
        writeln!(self.input, "V {result}").unwrap();
        self.snapshot(-1);
    }
    fn context(&mut self, value: u32) {
        self.constructor_word = value;
        writeln!(self.input, "H {value:x}").unwrap();
        self.snapshot(-1);
    }
    fn mutate(&mut self) {
        let mut source = self.progress.progress().clone();
        for e in &mut source.entries {
            e.scenario_id ^= 255;
            e.save_position ^= 255;
            e.replay_target ^= 255;
            if let Some(v) = e.response_data.first_mut() {
                *v ^= 0x55;
            }
        }
        let mut trailing = self.progress.trailing_bytes().to_vec();
        if let Some(v) = trailing.first_mut() {
            *v ^= 0x55;
        }
        let mut vars = self.progress.bytes().to_vec();
        if let Some(v) = vars.first_mut() {
            *v ^= 0x55;
        }
        let cursor = self.progress.responses().1;
        self.progress = ProgressMemory::new(self.progress.persistent().clone(), source, vars)
            .with_trailing_bytes(trailing);
        self.progress.restore_response_cursor(cursor).unwrap();
        self.input.push_str("M\n");
        self.snapshot(-1);
    }
    fn release(&mut self) {
        // Native command identity retirement precedes mapping release. The
        // concrete app service must provide the same order before completion.
        if self.services.attached {
            self.services.events.push_str("us");
        }
        self.services.queued = None;
        self.owner.release(&mut self.layers.state);
        self.released = true;
        self.input.push_str("D\n");
        self.snapshot(-1);
    }
}

#[test]
#[ignore = "actual original TIPSGET command/11780/full scene-copy factories with checked asset/wrapper/clock services;no GPU/audio/user saves"]
fn higurashi_tips_get_commands_match_native_live_replay_and_scene_copies() {
    let mut input = String::new();
    let mut expected = vec![];
    let mut owners = 0;
    let lists: [&[i32]; 4] = [
        &[],
        &[17],
        &[17, 17, 2],
        &[-1, 65536, 17, 17, i32::MAX, 0x12345678],
    ];
    for active in 0..2 {
        for length in [0, 1, 1024] {
            for ids in lists {
                for result in [0, 1, 2, i32::MAX] {
                    for used in [0, 3, 31] {
                        let cursor = if length == 0 { 0 } else { (length - 1) * 8 + 3 };
                        let mut p = Profile::new(
                            active,
                            length,
                            cursor,
                            199,
                            400,
                            owners % 2 == 0,
                            [0, 1, 0x80000000][owners % 3],
                            used,
                            ids,
                            [-1, 9999, 10000][owners % 3],
                            [0, 69, u32::MAX][owners % 3],
                            [0, 3, 1024][owners % 3],
                            [0, 4, 1024][owners % 3],
                        );
                        assert!(!p.owner.alternate());
                        p.live_record();
                        p.live_record();
                        assert!(!p.memory.valid);
                        p.ready();
                        p.live_record();
                        let initial = p.memory.clone();
                        p.select(-1);
                        p.live_record();
                        p.select(result);
                        p.live_record();
                        assert_eq!(p.progress.responses().1, cursor);
                        assert_eq!(p.read.timeline().resume_position, 0x1234);
                        assert_eq!(p.read.timeline().replay_target, 0x4567);
                        if result == 1 {
                            assert_eq!(p.read.timeline().boundary, 200);
                            assert_eq!(p.services.clocks, 1);
                            let queued = p.services.queued.clone().unwrap();
                            assert_eq!(
                                queued.trailing_values,
                                ids.iter().map(|v| *v as u16).collect::<Vec<_>>()
                            );
                            assert_eq!(
                                queued.selected,
                                ids.first().map_or(0, |v| i32::from(*v as u16))
                            );
                            p.mutate();
                            assert_eq!(p.services.queued.as_ref(), Some(&queued));
                            p.live_record();
                            assert_eq!(p.services.queues, 2);
                            assert_eq!(p.services.clocks, 1);
                        } else {
                            assert_eq!(p.read.timeline().boundary, 201);
                            assert_eq!(p.services.clocks, 2);
                            assert_eq!(initial.progress.active_entry().save_position, 200);
                            assert_eq!(p.memory.progress.active_entry().save_position, 201);
                        }
                        p.release();
                        assert!(p.layers.state.target(p.target.logical).is_none());
                        input.push_str(&p.input);
                        expected.extend(p.expected);
                        owners += 1;
                    }
                }
            }
        }
    }
    for active in 0..2 {
        for context in [0, 1, 2, 10, 256, 0x80000000, u32::MAX, 0x7fffffff] {
            for (boundary, saved) in [
                (0, 0),
                (0, 2),
                (0, 5),
                (199, 200),
                (u32::MAX, u32::MAX),
                (u32::MAX, 0),
            ] {
                for text in [false, true] {
                    let mut p = Profile::new(
                        active,
                        7,
                        3,
                        boundary,
                        saved,
                        text,
                        context,
                        3,
                        &[17, 17, 2],
                        1,
                        69,
                        3,
                        4,
                    );
                    let before = p.progress.progress().clone();
                    for _ in 0..3 {
                        p.replay_record();
                        p.forward_record();
                    }
                    assert_eq!(p.progress.progress(), &before);
                    assert_eq!(p.progress.responses().1, 3);
                    if p.owner.replay_stop() > 0 {
                        p.live_record();
                        assert_eq!(p.services.requests, 0);
                    } else if p.owner.replay_stop() == 0 {
                        p.context(0);
                        p.replay_record();
                        assert!(!p.owner.alternate());
                        p.ready();
                        p.live_record();
                        p.select(0);
                        p.live_record();
                    }
                    p.release();
                    input.push_str(&p.input);
                    expected.extend(p.expected);
                    owners += 1;
                }
            }
        }
    }
    // A ready callback may finish/queue in the same publication poll.
    for result in [0, 1, 2, i32::MAX] {
        let mut p = Profile::new(1, 1, 3, 199, 400, true, 1, 0, &[17], 9999, 69, 3, 4);
        p.ready();
        p.select(result);
        p.live_record();
        p.release();
        input.push_str(&p.input);
        expected.extend(p.expected);
        owners += 1;
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIPSGET_COMMAND").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TIPSGET_COMMAND_CAPTURE") {
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
        assert_eq!(a, e, "original TIPSGET row{i}");
    }
    eprintln!(
        "TIPSGET commands:{owners} owners/{} exactstates;preserved constructor text/resume,fullcontext unsigned replay,retained readiness,initial/final11780,normal catalog gate,full source-preserving outer1/current factory copies and mapping/ref retirement",
        expected.len()
    );
}

#[test]
fn higurashi_tips_get_failures_preserve_only_pending_prefixes() {
    let mut p = Profile::new(0, 7, 3, 199, 400, true, 1, 0, &[17, 17, 2], 1, 69, 3, 4);
    p.ready();
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.requests, 1);
    assert_eq!(p.services.polls, 1);
    assert!(p.memory.valid);
    assert_eq!(p.progress.progress().active_entry().save_position, 200);
    p.services.fail_clock = false;
    p.services.fail_gate = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.clocks, 1);
    p.services.fail_gate = false;
    p.services.fail_poll = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.gates, 1);
    p.services.fail_poll = false;
    p.select(1);
    p.services.fail_queue = true;
    assert!(p.live().is_err());
    let pending = p.owner.scene_request.clone().unwrap();
    p.mutate();
    p.services.fail_queue = false;
    assert!(!p.live().unwrap());
    assert_eq!(p.services.queued, Some(pending));
    assert!(!p.live().unwrap());
    assert_eq!(
        p.services.queued.as_ref().unwrap().byte_variables,
        p.progress.bytes()
    );
    assert_eq!(p.services.clocks, 1);
    assert_eq!(p.read.timeline().boundary, 200);
    assert_eq!(p.progress.responses().1, 3);
    p.release();

    let mut p = Profile::new(1, 7, 3, u32::MAX, 400, true, 0, 0, &[], 10000, 0, 0, 0);
    p.ready();
    assert!(!p.live().unwrap());
    p.select(2);
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    assert_eq!(p.read.timeline().boundary, 1);
    assert_eq!(p.owner.result(), Some(2));
    p.services.fail_clock = false;
    assert!(p.live().unwrap());
    assert_eq!(p.read.timeline().boundary, 1);
    assert_eq!(p.services.requests, 1);
    assert_eq!(p.services.polls, 1);
    assert_eq!(p.services.clocks, 2);
    p.release();
}

#[test]
fn higurashi_tips_get_capacity_errors_do_not_mutate_other_mappings() {
    let mut banks = LayerBanks::default();
    for i in 0..32 {
        assert!(banks.allocate(i).is_some());
    }
    let before = replay_snapshot(&banks);
    assert!(TipsGetCommand::new(&command(&[17]), &mut banks).is_err());
    assert_eq!(replay_snapshot(&banks), before);
    let mut banks = LayerBanks::default();
    let before = replay_snapshot(&banks);
    assert!(TipsGetCommand::new(&command(&[1, 2, 3, 4, 5, 6, 7]), &mut banks).is_err());
    assert_eq!(replay_snapshot(&banks), before);
}

#[test]
fn higurashi_tips_get_scene_copy_preserves_pending_queues_and_source_banks() {
    let mut p = Profile::new(1, 7, 3, 199, 400, true, 0, 0, &[17, 17, -1], 1, 69, 3, 4);
    p.ready();
    assert!(!p.live().unwrap());
    for command in [
        Cmd::SSET {
            args: [3, 0x12345678],
        },
        Cmd::OpcodeDB { args: [2, 777] },
    ] {
        p.progress
            .dispatch_variables(&command, super::super::autosave::Execution::Live, 200)
            .unwrap();
    }
    let pending_persistent = p.progress.pending_persistent().to_vec();
    let pending_bytes = p.progress.pending_bytes().to_vec();
    let source = p.progress.progress().clone();
    let tail = p.progress.trailing_bytes().to_vec();
    let vars = p.progress.bytes().to_vec();
    let cursor = p.progress.responses().1;
    p.select(1);
    assert!(!p.live().unwrap());
    let copy = p.services.queued.as_ref().unwrap();
    assert_eq!(copy.progress, source);
    assert_eq!(copy.trailing_values, [17, 17, u16::MAX]);
    assert_eq!(copy.byte_variables, vars);
    assert_eq!(p.progress.progress(), &source);
    assert_eq!(p.progress.trailing_bytes(), tail);
    assert_eq!(p.progress.bytes(), vars);
    assert_eq!(p.progress.responses().1, cursor);
    assert_eq!(p.progress.pending_persistent(), pending_persistent);
    assert_eq!(p.progress.pending_bytes(), pending_bytes);
    p.release();
}
