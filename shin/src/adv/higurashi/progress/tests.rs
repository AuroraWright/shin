use super::super::{
    autosave::{AutosaveCommand, AutosaveMemory, AutosaveServices, ProgressEntry},
    save_metadata::SaveMetadata,
};
use super::*;
use shin_core::format::scenario::instruction_elements::Register;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn bytes(n: usize, seed: u32) -> Vec<u8> {
    (0..n)
        .map(|i| seed.wrapping_add(i as u32 * 17) as u8)
        .collect()
}
fn progress(active: usize, seed: u32, lengths: [usize; 2], destination: bool) -> SaveProgress {
    SaveProgress::new(
        active,
        if destination {
            !seed
        } else {
            seed ^ 0x12345678
        },
        std::array::from_fn(|i| {
            let base = if destination {
                (!seed).wrapping_add(i as u32 * 7)
            } else {
                seed.wrapping_add(i as u32 * 31)
            };
            let step = if destination { 3 } else { 11 };
            ProgressEntry {
                scenario_id: base as i32,
                random_seed: base.wrapping_add(step),
                save_position: base.wrapping_add(step * 2),
                replay_target: base.wrapping_add(step * 3),
                response_data: bytes(
                    lengths[i],
                    if destination {
                        (!seed).wrapping_add(i as u32 * 23)
                    } else {
                        seed.wrapping_add(i as u32 * 19)
                    },
                ),
            }
        }),
    )
    .unwrap()
}
fn append_vector(out: &mut String, data: &[u8], element: usize) {
    out.push_str(&format!(" {} {:016x}", data.len() / element, hash(data)));
}
fn append_progress(out: &mut String, state: &SaveProgress) {
    out.push_str(&format!(
        " {} {:08x}",
        state.active_index(),
        state.extra_word
    ));
    for entry in &state.entries {
        out.push_str(&format!(
            " {:08x} {:08x} {:08x} {:08x}",
            entry.scenario_id as u32, entry.random_seed, entry.save_position, entry.replay_target
        ));
        append_vector(out, &entry.response_data, 1);
    }
}
struct Clock(usize);
impl AutosaveServices for Clock {
    fn packed_time(&mut self) -> Result<u64> {
        self.0 += 1;
        Ok(0x00123456789abcde + self.0 as u64)
    }
    fn notify(&mut self, text: &str) -> Result<()> {
        assert_eq!(text, " ");
        Ok(())
    }
}
struct Profile {
    state: ProgressMemory,
    memory: AutosaveMemory,
    clock: Clock,
    execution: Execution,
    boundary: u32,
    length: u32,
    seed: u32,
    previous: u32,
    input: String,
    expected: Vec<String>,
}
impl Profile {
    fn new(active: usize, seed: u32, lengths: [usize; 2], target: u32) -> Self {
        let mut source = progress(active, seed, lengths, false);
        source.entries[active].replay_target = target;
        let p = PersistData((0..64).map(|i| seed.wrapping_add(i * 17) as i16).collect());
        let mut profile = Self {
            state: ProgressMemory::new(p, source, bytes(11, seed)),
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            clock: Clock(0),
            execution: Execution::Live,
            boundary: 0,
            length: 0,
            seed: 42,
            previous: 0,
            input: format!(
                "N {active} {seed:x} {} {} {target:x}\n",
                lengths[0], lengths[1]
            ),
            expected: Vec::new(),
        };
        profile.snapshot(0);
        profile
    }
    fn snapshot(&mut self, result: i32) {
        let mut row = format!(
            "P {result} {} {} {} {:016x}",
            self.state.responses().1,
            self.clock.0,
            u8::from(self.memory.valid),
            self.memory.packed_time
        );
        let persistent: Vec<u8> = self
            .state
            .persistent()
            .0
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        append_vector(&mut row, &persistent, 2);
        append_vector(&mut row, self.state.bytes(), 1);
        for pairs in [self.state.pending_persistent(), self.state.pending_bytes()] {
            let raw: Vec<u8> = pairs
                .iter()
                .flat_map(|(slot, value)| slot.to_le_bytes().into_iter().chain(value.to_le_bytes()))
                .collect();
            append_vector(&mut row, &raw, 8);
        }
        append_progress(&mut row, self.state.progress());
        append_progress(&mut row, &self.memory.progress);
        row.push_str(&format!(
            " {} 0 {} {}",
            self.length, self.seed, self.previous
        ));
        self.expected.push(row);
    }
    fn mode(&mut self, execution: Execution, boundary: u32) {
        self.execution = execution;
        self.boundary = boundary;
        self.input.push_str(&format!(
            "E {} {boundary:x}\n",
            u8::from(execution == Execution::Replay)
        ));
        self.snapshot(0);
    }
    fn cursor(&mut self, cursor: usize) {
        self.input.push_str(&format!("R {cursor}\n"));
        self.state.restore_response_cursor(cursor).unwrap();
        self.snapshot(0);
    }
    fn commit(&mut self, enabled: bool) {
        self.input.push_str(&format!("C {}\n", u8::from(enabled)));
        self.state.commit_pending(enabled).unwrap();
        self.snapshot(0);
    }
    fn command(&mut self, byte: bool, slot: i32, value: Option<i32>) -> i32 {
        let dest = Register::from_regular_register(31);
        let command = match (byte, value) {
            (false, None) => Cmd::SGET { dest, slot },
            (true, None) => Cmd::OpcodeDA {
                dest,
                argument: slot,
            },
            (false, Some(v)) => Cmd::SSET { args: [slot, v] },
            (true, Some(v)) => Cmd::OpcodeDB { args: [slot, v] },
        };
        let op = match (byte, value.is_some()) {
            (false, false) => 'G',
            (true, false) => 'B',
            (false, true) => 'S',
            (true, true) => 'W',
        };
        self.input.push_str(&format!("{op} {slot}"));
        if let Some(v) = value {
            self.input.push_str(&format!(" {v}"));
        }
        self.input.push('\n');
        self.previous = self.seed;
        self.seed = self.seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        self.length = if value.is_some() { 3 } else { 4 };
        let response = self
            .state
            .dispatch_variables(&command, self.execution, self.boundary)
            .unwrap()
            .unwrap();
        let result = match response {
            CommandResult::None => 0,
            CommandResult::WriteMemory(_, value) => value,
        };
        self.snapshot(result);
        result
    }
    fn capture(&mut self) {
        self.input.push_str("A\n");
        AutosaveCommand::new()
            .poll_live(
                self.state.progress_mut(),
                &mut self.memory,
                self.boundary,
                &SaveMetadata::new(),
                &mut self.clock,
            )
            .unwrap();
        self.snapshot(0);
    }
}

#[test]
#[ignore = "native33168 active progress binding, actual variables/commit callbacks, cursor restore and11780 copy through one response vector"]
fn higurashi_progress_memory_matches_native_binding_replay_commits_and_autosave_copy() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for active in 0..2 {
        for seed in [0u32, 65535, u32::MAX] {
            for offset in 0..8 {
                for lengths in [[1, 7], [7, 1], [1023, 1024], [1024, 2048]] {
                    for target in [0, seed, u32::MAX] {
                        profiles += 1;
                        let mut p = Profile::new(active, seed, lengths, target);
                        let inactive = p.state.progress().entries[1 - active].clone();
                        p.cursor(offset);
                        p.mode(Execution::Replay, seed);
                        p.command(false, 1, Some(i32::MIN));
                        p.command(true, 20, Some(511));
                        p.command(false, 1, None);
                        p.command(true, 20, None);
                        p.commit(false);
                        p.mode(Execution::Live, seed);
                        p.command(false, 1, Some(65535));
                        p.command(false, 65, Some(1));
                        p.command(false, 1, Some(seed as i32));
                        p.command(false, 1, None);
                        p.command(true, 20, Some(511));
                        p.command(true, 20, None);
                        p.commit(false);
                        p.commit(true);
                        p.command(false, 1, None);
                        p.command(true, 20, None);
                        p.cursor(offset);
                        p.mode(Execution::Replay, seed);
                        p.command(false, 1, None);
                        p.command(true, 20, None);
                        p.command(false, 1, Some(-32768));
                        p.command(true, 20, Some(257));
                        p.commit(true);
                        p.mode(Execution::Live, seed);
                        p.capture();
                        let copied = p.memory.clone();
                        let route = p.state.responses().0.to_vec();
                        let cursor = p.state.responses().1;
                        p.command(false, 1, None);
                        p.command(true, 20, None);
                        assert_eq!(p.memory, copied);
                        assert_eq!(p.state.progress().entries[1 - active], inactive);
                        let cloned = p.state.clone();
                        assert_eq!(cloned.responses(), p.state.responses());
                        assert_ne!(
                            cloned.responses().0.as_ptr(),
                            p.state.responses().0.as_ptr()
                        );
                        p.cursor(p.state.responses().0.len() * 8);
                        p.mode(Execution::Replay, seed);
                        assert_eq!(p.command(false, 1, None), 0);
                        assert_eq!(p.command(true, 20, None), 0);
                        assert!(p.state.responses().0.len() > route.len());
                        p.capture();
                        p.cursor(cursor);
                        assert_eq!(p.state.responses().1, cursor);
                        input.push_str(&p.input);
                        expected.extend(p.expected);
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_PROGRESS").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (i, (line, want)) in native.lines().zip(&expected).enumerate() {
        assert_eq!(line, want, "Progress snapshot{i}");
    }
    println!(
        "Higurashi progress binding:{profiles} native profiles/{} snapshots match; active vector binding, pending variable order, replay target gates, signed/byte feedback, enabled/disabled commits, restored cursors, exhausted zero growth, retained tails and two autosave copies share one response owner; inactive entry/settings copies stay independent",
        expected.len()
    );
}

#[test]
fn progress_binding_rejects_invalid_cursor_and_preserves_unrelated_response_vector() {
    let mut state = ProgressMemory::new(
        PersistData::new(),
        progress(0, 42, [1, 3], false),
        Vec::new(),
    );
    let before = state.clone();
    assert!(state.restore_response_cursor(9).is_err());
    assert_eq!(state.responses(), before.responses());
    let data = state.responses().0.as_ptr();
    assert!(
        state
            .dispatch_variables(
                &Cmd::WAIT {
                    amount: 10,
                    allow_interrupt: 1
                },
                Execution::Live,
                0
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(state.responses().0.as_ptr(), data);
    let mut standalone = HigurashiVariables::new(PersistData::new(), Vec::new());
    standalone.restore_route(vec![0xff], 1).unwrap();
    let mut external = vec![42];
    assert!(
        standalone
            .dispatch_in_progress(
                &mut external,
                &Cmd::SGET {
                    dest: Register::from_regular_register(31),
                    slot: 0
                },
                variables::Execution::Live
            )
            .is_err()
    );
    assert_eq!(external, [42]);
    assert_eq!(standalone.route(), (&[0xff][..], 1));
}
