use super::*;
use crate::adv::higurashi::{
    choice::tests::{append_progress, bytes, progress},
    input::ReadState,
    layers::replay_snapshot,
    read_state::{Checkpoint, CommandReadState, ReadServices},
};
use shin_core::format::{
    save::{PersistData, index_set::IndexSet},
    scenario::instruction_elements::MessageId,
};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Default)]
pub(in crate::adv::higurashi) struct Services {
    pub ready: bool,
    pub selected: Option<i32>,
    pub requests: u32,
    polls: u32,
    attached: bool,
    pub clocks: u32,
    pub events: String,
    fail_poll: bool,
    fail_clock: bool,
}
impl CharacterServices for Services {
    fn prepare(
        &mut self,
        _: &CharacterRequest,
        target: Target,
        _: &mut HigurashiLayers,
    ) -> Result<bool> {
        assert_eq!(target.bank, 0);
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
        ensure!(!self.fail_poll, "result service failed");
        Ok(self.selected)
    }
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "clock service failed");
        self.clocks += 1;
        self.events.push('C');
        Ok(0x00123456789abcde + u64::from(self.clocks))
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
fn hash(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn command(index: u16, mask: u32) -> Cmd {
    Cmd::CHARSELECT {
        index,
        dest: Register::from_regular_register(31),
        argument: mask as i32,
    }
}
struct Profile {
    command: CharacterCommand,
    target: Target,
    layers: HigurashiLayers,
    read: CommandReadState,
    progress: ProgressMemory,
    memory: AutosaveMemory,
    history: ChoiceHistory,
    services: Services,
    register: i32,
    released: bool,
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
        index: u16,
        last: usize,
        mask: u32,
        used: usize,
    ) -> Self {
        let seed = 0x5a;
        let mut lengths = [7; 2];
        lengths[active] = length;
        let mut source = progress(active, seed, lengths, false);
        source.entries[active].save_position = saved;
        source.entries[active].replay_target = 0xabcdef01;
        let mut p = ProgressMemory::new(PersistData(vec![]), source, vec![]);
        p.restore_response_cursor(cursor).unwrap();
        let mut seen = IndexSet::new();
        seen.add(1);
        let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
        for i in 0..used {
            assert_eq!(layers.state.allocate((i * 2) as u16), Some(i as u8));
        }
        let mut read = CommandReadState::new();
        let mut services = Services::default();
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
        let ctor = command(index, mask);
        let capture = read.constructor_capture(&ctor);
        assert!(capture.descriptors().message.text.is_none());
        assert_eq!(capture.timeline().boundary, boundary);
        read.prepare(&ctor, &mut services).unwrap();
        let command = CharacterCommand::new(&ctor, &mut layers.state).unwrap();
        let target = command.target().unwrap();
        let mut out = Self {
            command,
            target,
            layers,
            read,
            progress: p,
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            history: ChoiceHistory::new(seen, bytes(last, seed)),
            services,
            register: -777,
            released: false,
            input: format!(
                "N {active} {seed:x} {length} {cursor} {boundary:x} {saved:x} {} {index} {last} {mask:x} {used}\n",
                u8::from(text)
            ),
            expected: vec![],
        };
        out.snapshot(-1);
        out
    }
    fn snapshot(&mut self, result: i32) {
        let t = self.read.timeline();
        let s = &self.services;
        let mut row = format!(
            "C {result} {:08x} {:08x} {:08x} {} {} {} {} {} {} {} {} {} {:016x}",
            t.boundary,
            t.resume_position,
            t.replay_target,
            self.command.replay_stop(),
            self.register,
            self.progress.responses().1,
            s.requests,
            s.polls,
            u8::from(s.attached),
            u8::from(s.attached),
            s.clocks,
            u8::from(self.memory.valid),
            self.memory.packed_time
        );
        let mut seen = vec![0u32; self.history.seen().size_in_words()];
        for bit in self.history.seen().iter() {
            seen[bit / 32] |= 1 << (bit % 32);
        }
        let seen = seen
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        write!(
            row,
            " {} {:016x} {} {:016x}",
            seen.len() / 4,
            hash(&seen),
            self.history.last().len(),
            hash(self.history.last())
        )
        .unwrap();
        append_progress(&mut row, self.progress.progress());
        append_progress(&mut row, &self.memory.progress);
        write!(
            row,
            " {} {} {} {} {} {} {} {} {} {} {} {:016x}",
            s.events,
            self.command.request.index,
            self.command.request.mask,
            self.target.logical,
            self.target.slot,
            self.layers
                .state
                .target(self.target.logical)
                .map_or(32, |t| t.slot),
            if s.attached { 3 } else { 0 },
            u8::from(self.read.descriptors().message.text.is_none()),
            u8::from(s.requests > 0 && !self.released),
            if s.attached && !self.released { 2 } else { 0 },
            if s.attached && !self.released { 2 } else { 0 },
            replay_snapshot(&self.layers.state)
        )
        .unwrap();
        self.expected.push(row);
    }
    fn live(&mut self) -> Result<bool> {
        self.command.poll_live(
            &mut self.progress,
            &mut self.memory,
            &mut self.history,
            &mut self.read.timeline_mut().boundary,
            &mut self.layers,
            &mut self.services,
            |dest, value| {
                assert_eq!(dest, Register::from_regular_register(31));
                self.register = value;
                Ok(())
            },
        )
    }
    fn live_record(&mut self) {
        let complete = self.live().unwrap();
        self.input.push_str("L\n");
        self.snapshot(i32::from(complete));
    }
    fn replay_record(&mut self) -> bool {
        let complete = self
            .command
            .poll_replay(
                &mut self.progress,
                &mut self.read.timeline_mut().boundary,
                |dest, value| {
                    assert_eq!(dest, Register::from_regular_register(31));
                    self.register = value;
                    Ok(())
                },
            )
            .unwrap();
        self.input.push_str("R\n");
        self.snapshot(i32::from(complete));
        complete
    }
    fn ready(&mut self) {
        self.services.ready = true;
        self.input.push_str("Q 1\n");
        self.snapshot(-1);
    }
    fn select(&mut self, value: i32) {
        self.services.selected = Some(value);
        writeln!(self.input, "V {value}").unwrap();
        self.snapshot(-1);
    }
    fn release(&mut self) {
        if self.services.attached {
            self.services.events.push_str("us");
        }
        self.command.release(&mut self.layers.state);
        self.released = true;
        self.input.push_str("D\n");
        self.snapshot(-1);
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
#[test]
#[ignore = "actual d3ec/d810/d8cc/d578 command, temporary mapping, last/response/progress copies with checked loader/UI services;no GPU/audio/saves"]
fn higurashi_character_commands_match_native_live_replay_loading_and_retirement() {
    let mut input = String::new();
    let mut expected = vec![];
    let mut owners = 0;
    for active in 0..2 {
        for length in [0, 1, 1024] {
            for offset in 0..if length == 0 { 1 } else { 8 } {
                let cursor = if length == 0 {
                    0
                } else {
                    (length - 1) * 8 + offset
                };
                for (index, last) in [(0, 0), (18, 0), (33, 33), (65535, 65504)] {
                    for value in [0, 2, 3, 16, 255, 256, i32::MAX] {
                        let mut p = Profile::new(
                            active,
                            length,
                            cursor,
                            199,
                            400,
                            owners % 2 == 0,
                            index,
                            last,
                            [7, 0x80000007, 0xffff0005][owners % 3],
                            [0, 3, 31][owners % 3],
                        );
                        p.live_record();
                        p.live_record();
                        assert!(!p.memory.valid);
                        assert_eq!(p.services.clocks, 0);
                        p.ready();
                        p.live_record();
                        let frozen = p.memory.clone();
                        p.select(-1);
                        p.live_record();
                        p.select(value);
                        p.live_record();
                        assert_eq!(p.register, value);
                        assert_eq!(p.progress.responses().1, cursor + 4);
                        assert_eq!(p.read.timeline().boundary, 201);
                        assert_eq!(p.memory.progress.active_entry().save_position, 201);
                        assert_eq!(frozen.progress.active_entry().save_position, 200);
                        assert_eq!(
                            frozen.progress.active_entry().response_data,
                            bytes(length, 0x5a + active as u32 * 19)
                        );
                        assert_eq!(p.history.seen().iter().collect::<Vec<_>>(), [1]);
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
        for (boundary, saved) in [
            (0, 0),
            (0, 1),
            (0, 2),
            (199, 200),
            (u32::MAX, u32::MAX),
            (u32::MAX, 0),
        ] {
            for length in [0, 1, 7] {
                for offset in [0, 3, 7].into_iter().take(if length == 0 { 1 } else { 3 }) {
                    let cursor = if length == 0 {
                        0
                    } else {
                        (length - 1) * 8 + offset
                    };
                    let mut p =
                        Profile::new(active, length, cursor, boundary, saved, true, 18, 0, 7, 3);
                    let complete = p.replay_record();
                    if !complete {
                        if p.command.replay_stop() > 0 {
                            p.live_record();
                            assert_eq!(p.services.requests, 0);
                            assert_eq!(p.services.clocks, 0);
                        } else {
                            p.ready();
                            p.live_record();
                            p.select(3);
                            p.live_record();
                        }
                    }
                    p.release();
                    input.push_str(&p.input);
                    expected.extend(p.expected);
                    owners += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHARACTER_COMMAND").unwrap())
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_CHARACTER_COMMAND_CAPTURE") {
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
        assert_eq!(a, e, "native character command row{i}");
    }
    eprintln!(
        "Character commands:{owners} native owners,{} exact observations;always-cleared text/boundaries,reserved mappings,retained loading,unchanged seen/shared last/full32 feedback/four response bits,both11780 copies,replay handoffs and balanced command/job/layer/wrapper retirement pass",
        expected.len()
    );
}
#[test]
fn higurashi_character_failures_retain_load_copy_feedback_and_response_prefixes() {
    let mut p = Profile::new(0, 7, 3, 199, 400, true, 18, 0, 7, 0);
    p.ready();
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.requests, 1);
    assert_eq!(p.services.polls, 1);
    assert!(p.memory.valid);
    assert_eq!(p.progress.progress().active_entry().save_position, 200);
    p.services.fail_clock = false;
    p.services.fail_poll = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.clocks, 1);
    assert_eq!(p.services.polls, 1);
    p.services.fail_poll = false;
    p.services.selected = Some(3);
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    let cursor = p.progress.responses().1;
    assert_eq!(cursor, 7);
    assert_eq!(p.read.timeline().boundary, 201);
    assert_eq!(p.history.last()[17], 4);
    assert_eq!(p.history.seen().iter().collect::<Vec<_>>(), [1]);
    p.services.fail_clock = false;
    assert!(p.live().unwrap());
    assert_eq!(p.progress.responses().1, cursor);
    assert_eq!(p.read.timeline().boundary, 201);
    assert_eq!(p.services.clocks, 2);
    let mut p = Profile::new(0, 7, 0, 0, 2, false, 18, 0, 7, 0);
    let cursor = p.progress.responses().1;
    let mut publishes = 0;
    assert!(
        p.command
            .poll_replay(
                &mut p.progress,
                &mut p.read.timeline_mut().boundary,
                |_, _| {
                    publishes += 1;
                    bail!("feedback unavailable")
                }
            )
            .is_err()
    );
    assert_eq!(p.progress.responses().1, cursor + 4);
    assert!(
        !p.command
            .poll_replay(
                &mut p.progress,
                &mut p.read.timeline_mut().boundary,
                |_, value| {
                    publishes += 1;
                    assert_eq!(value, 5);
                    Ok(())
                }
            )
            .unwrap()
    );
    assert_eq!(p.progress.responses().1, cursor + 4);
    assert_eq!(publishes, 2);
    let mut layers = LayerBanks::default();
    for id in 0..32 {
        layers.allocate(id).unwrap();
    }
    let before = replay_snapshot(&layers);
    assert!(CharacterCommand::new(&command(18, 7), &mut layers).is_err());
    assert_eq!(replay_snapshot(&layers), before);
}
