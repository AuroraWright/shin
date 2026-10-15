use super::*;
use shin_core::format::scenario::instruction_elements::Register;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn get(byte: bool, slot: i32) -> Cmd {
    let dest = Register::from_regular_register(31);
    if byte {
        Cmd::OpcodeDA {
            dest,
            argument: slot,
        }
    } else {
        Cmd::SGET { dest, slot }
    }
}
fn set(byte: bool, slot: i32, value: i32) -> Cmd {
    if byte {
        Cmd::OpcodeDB {
            args: [slot, value],
        }
    } else {
        Cmd::SSET {
            args: [slot, value],
        }
    }
}
fn value(state: &mut HigurashiVariables, byte: bool, slot: i32, mode: Execution) -> i32 {
    let Some(CommandResult::WriteMemory(_, value)) =
        state.dispatch(&get(byte, slot), mode).unwrap()
    else {
        panic!()
    };
    value
}
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
fn snapshot(
    op: char,
    value: i32,
    state: &HigurashiVariables,
    length: u32,
    seed: u32,
    previous: u32,
) -> String {
    let persistent: Vec<_> = state
        .persistent
        .0
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let pending_p: Vec<_> = state
        .pending_persistent
        .iter()
        .flat_map(|(s, v)| s.to_le_bytes().into_iter().chain(v.to_le_bytes()))
        .collect();
    let pending_b: Vec<_> = state
        .pending_bytes
        .iter()
        .flat_map(|(s, v)| s.to_le_bytes().into_iter().chain(v.to_le_bytes()))
        .collect();
    let mut row = format!("{op} {value} {}", state.position);
    for (bytes, element) in [
        (&persistent[..], 2),
        (&state.bytes[..], 1),
        (&pending_p[..], 8),
        (&pending_b[..], 8),
        (&state.route[..], 1),
    ] {
        row.push_str(&format!(" {} {:016x}", bytes.len() / element, hash(bytes)));
    }
    row.push_str(&format!(" {length} 0 {seed} {previous}"));
    row
}

struct Profile {
    state: HigurashiVariables,
    mode: Execution,
    seed: u32,
    previous: u32,
    length: u32,
    input: String,
    expected: Vec<String>,
    getters: Vec<(bool, i32)>,
}
impl Profile {
    fn new(pn: usize, bn: usize, seed: u32) -> Self {
        let p = PersistData(
            (0..pn)
                .map(|i| seed.wrapping_add(i as u32 * 17) as i16)
                .collect(),
        );
        let b = (0..bn)
            .map(|i| seed.wrapping_add(i as u32 * 17) as u8)
            .collect();
        let state = HigurashiVariables::new(p, b);
        let expected = vec![snapshot('N', 0, &state, 0, 42, 0)];
        Self {
            state,
            mode: Execution::Live,
            seed: 42,
            previous: 0,
            length: 0,
            input: format!("N {pn} {bn} {seed:x}\n"),
            expected,
            getters: Vec::new(),
        }
    }
    fn command(&mut self, byte: bool, slot: i32, write: Option<i32>) {
        let op = match (byte, write.is_some()) {
            (false, false) => 'G',
            (false, true) => 'S',
            (true, false) => 'B',
            (true, true) => 'W',
        };
        self.input.push_str(&format!("{op} {slot}"));
        if let Some(v) = write {
            self.input.push_str(&format!(" {v}"));
        }
        self.input.push('\n');
        self.previous = self.seed;
        self.seed = self.seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        self.length = if write.is_some() { 3 } else { 4 };
        let result = if let Some(v) = write {
            self.state.dispatch(&set(byte, slot, v), self.mode).unwrap();
            0
        } else {
            let v = value(&mut self.state, byte, slot, self.mode);
            if matches!(self.mode, Execution::Live) {
                self.getters.push((byte, v));
            }
            v
        };
        self.expected.push(snapshot(
            op,
            result,
            &self.state,
            self.length,
            self.seed,
            self.previous,
        ));
    }
    fn commit(&mut self, capture: bool, enabled: bool) {
        let op = if capture { 'K' } else { 'C' };
        self.input
            .push_str(&format!("{op} {}\n", u8::from(enabled)));
        self.state.commit_pending(enabled).unwrap();
        self.expected.push(snapshot(
            op,
            0,
            &self.state,
            self.length,
            self.seed,
            self.previous,
        ));
    }
    fn replay(&mut self, boundary: u32, target: u32) {
        self.input
            .push_str(&format!("E 1 {boundary:x} {target:x}\n"));
        self.mode = Execution::Replay { boundary, target };
        self.expected.push(snapshot(
            'E',
            0,
            &self.state,
            self.length,
            self.seed,
            self.previous,
        ));
    }
    fn rewind(&mut self) {
        self.seek(0);
    }
    fn seek(&mut self, position: usize) {
        self.input.push_str(&format!("R {position}\n"));
        self.state
            .restore_route(self.state.route.clone(), position)
            .unwrap();
        self.expected.push(snapshot(
            'R',
            0,
            &self.state,
            self.length,
            self.seed,
            self.previous,
        ));
    }
}

#[test]
#[ignore = "original updated variable decoder/constructors/live-replay polls, queues, bit codec and both commit paths"]
fn higurashi_variables_match_native_pending_storage_commits_and_replay() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for pn in [0, 64] {
        for bn in [0, 4] {
            for seed in [0, 0x8000, u32::MAX] {
                for slot in [0, 3, 63, 64, 127, 128, 1023] {
                    for v in [0, 1, 2, -1, 32768, 65535, 65536, i32::MIN, i32::MAX] {
                        let mut p = Profile::new(pn, bn, seed);
                        for byte in [false, true] {
                            p.command(byte, slot, None);
                            p.command(byte, -1, None);
                            p.command(byte, slot, Some(v));
                            p.command(byte, slot, None);
                            p.command(byte, slot, Some(v ^ 0x13579bdf));
                            p.command(byte, slot, None);
                            p.command(byte, 7, Some(v));
                        }
                        p.commit(false, false);
                        p.commit(true, false);
                        p.command(false, slot, None);
                        p.command(true, slot, None);
                        p.commit(profiles % 2 == 0, true);
                        p.command(false, slot, None);
                        p.command(true, slot, None);
                        // Low-byte zero must not allocate beyond an existing vector.
                        p.command(true, 4095, Some(256));
                        p.commit(profiles % 2 != 0, true);
                        let getters = p.getters.clone();
                        let saved = p.state.clone();
                        p.rewind();
                        p.replay(0, 1);
                        p.command(false, slot, Some(-999));
                        p.command(true, slot, Some(-999));
                        assert!(
                            p.state.pending_persistent.is_empty()
                                && p.state.pending_bytes.is_empty()
                        );
                        for (byte, v) in getters {
                            p.command(byte, slot, None);
                            let expected_v = if byte {
                                v as u8 as i32
                            } else {
                                v as i16 as i32
                            };
                            assert_eq!(
                                p.expected
                                    .last()
                                    .unwrap()
                                    .split_whitespace()
                                    .nth(1)
                                    .unwrap()
                                    .parse::<i32>()
                                    .unwrap(),
                                expected_v
                            );
                        }
                        assert_eq!(p.state.route(), saved.route());
                        for (boundary, target) in [(1, 1), (u32::MAX, 1), (0, u32::MAX)] {
                            p.replay(boundary, target);
                            p.command(false, slot, Some(v));
                            p.command(true, slot, Some(v));
                        }
                        p.commit(true, false);
                        p.commit(false, true);
                        profiles += 1;
                        input.push_str(&p.input);
                        expected.extend(p.expected);
                    }
                }
            }
        }
    }
    // Grow and replay across native1024-byte blocks with mixed unaligned values.
    let mut p = Profile::new(0, 0, 0);
    for i in 0..900i32 {
        let byte = i % 3 == 0;
        p.command(byte, i % 7, Some(if i % 5 == 0 { 65536 } else { -1 }));
        p.command(byte, i % 7, None);
    }
    let saved = p.state.clone();
    p.rewind();
    p.replay(0, 1);
    for (byte, _) in p.getters.clone() {
        p.command(byte, 9999, None);
    }
    assert_eq!(p.state.route(), saved.route());
    profiles += 1;
    input.push_str(&p.input);
    expected.extend(p.expected);
    // Resume live recording over a populated tail at an unaligned cursor.
    let mut p = Profile::new(0, 0, 0);
    p.command(false, 2, Some(-1));
    p.command(false, 2, None);
    p.command(true, 3, Some(255));
    p.command(true, 3, None);
    p.rewind();
    p.command(false, 2, Some(0));
    p.command(true, 3, Some(0));
    p.command(false, 2, None);
    p.command(true, 3, None);
    p.command(false, 2, Some(65536));
    p.command(false, 2, None);
    profiles += 1;
    input.push_str(&p.input);
    expected.extend(p.expected);
    // Native false/int prefix masks differ from the shared later bit buffer.
    for offset in 0..8 {
        for v in [0, 1, 2, -1, 32768, 65535, 65536, i32::MIN, i32::MAX] {
            let mut p = Profile::new(0, 0, 0);
            p.command(false, 2, Some(-1));
            p.command(false, 2, None);
            p.command(true, 3, Some(-1));
            p.command(true, 3, None);
            p.seek(offset);
            p.command(false, 2, Some(v));
            p.command(false, 2, None);
            p.command(true, 3, Some(v));
            p.command(true, 3, None);
            profiles += 1;
            input.push_str(&p.input);
            expected.extend(p.expected);
        }
    }
    let mut p = Profile::new(0, 0, 0);
    p.replay(0, 1);
    p.command(false, 2, None);
    p.command(true, 3, None);
    profiles += 1;
    input.push_str(&p.input);
    expected.extend(p.expected);
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_VARIABLES").unwrap())
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
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (row, (actual, expected)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "native variable row{row}");
    }
    println!(
        "Higurashi variables:{profiles} native profiles/{} complete storage/queue/route/PC/PRNG snapshots match signed16/byte commits, full32 pending overrides, duplicate order, disabled commits, sparse capture commits and unsigned replay targets",
        expected.len()
    );
}

#[test]
fn exhausted_replay_zero_extends_and_invalid_commit_retains_unacknowledged_state() {
    let mut state = HigurashiVariables::new(PersistData::new(), Vec::new());
    state.restore_route(vec![0], 0).unwrap();
    let mode = Execution::Replay {
        boundary: 0,
        target: 1,
    };
    assert_eq!(value(&mut state, false, 2, mode), 0);
    assert_eq!(state.route(), (&vec![0; 1024][..], 17));
    assert!(state.restore_route(vec![], 1).is_err());
    state
        .dispatch(&set(false, 2, 65535), Execution::Live)
        .unwrap();
    state.dispatch(&set(false, -1, 1), Execution::Live).unwrap();
    state.dispatch(&set(true, 3, 255), Execution::Live).unwrap();
    assert!(state.commit_pending(true).is_err());
    assert_eq!(state.persistent.get(2), -1);
    assert_eq!(state.pending_persistent, vec![(u32::MAX, 1)]);
    assert_eq!(state.pending_bytes, vec![(3, 255)]);
    assert!(state.bytes.is_empty());
    let mut snapshot = state.clone();
    snapshot.pending_persistent.clear();
    snapshot.commit_pending(true).unwrap();
    assert_eq!(snapshot.bytes, vec![0, 0, 0, 255]);
    assert!(state.bytes.is_empty());
}
