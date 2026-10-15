use super::super::autosave::{ProgressEntry, SaveProgress};
use super::*;
use shin_core::format::save::PersistData;
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
pub(in crate::adv::higurashi) fn bytes(n: usize, seed: u32) -> Vec<u8> {
    (0..n)
        .map(|i| seed.wrapping_add(i as u32 * 17) as u8)
        .collect()
}
pub(in crate::adv::higurashi) fn progress(active: usize, seed: u32, lengths: [usize; 2], destination: bool) -> SaveProgress {
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
pub(in crate::adv::higurashi) fn append_progress(row: &mut String, p: &SaveProgress) {
    row.push_str(&format!(" {} {:08x}", p.active_index(), p.extra_word));
    for e in &p.entries {
        row.push_str(&format!(
            " {:08x} {:08x} {:08x} {:08x} {} {:016x}",
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.response_data.len(),
            hash(&e.response_data)
        ));
    }
}
#[derive(Default)]
pub(in crate::adv::higurashi) struct Services {
    pub(in crate::adv::higurashi) selected: Option<i32>,
    pub(in crate::adv::higurashi) opens: u32,
    pub(in crate::adv::higurashi) clocks: u32,
    pub(in crate::adv::higurashi) mask: u32,
    pub(in crate::adv::higurashi) events: String,
    pub(in crate::adv::higurashi) fail_open: bool,
    pub(in crate::adv::higurashi) fail_poll: bool,
    pub(in crate::adv::higurashi) fail_clock: bool,
}
impl ChoiceServices for Services {
    fn open(&mut self, r: &ChoiceRequest, mask: u32) -> Result<()> {
        ensure!(!self.fail_open, "choice UI unavailable");
        assert_eq!(r.visibility, 7);
        assert_eq!(r.variants, ["A", "B", "C"]);
        self.opens += 1;
        self.mask = mask;
        self.events.push_str("OSU");
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<i32>> {
        ensure!(!self.fail_poll, "choice result unavailable");
        Ok(self.selected)
    }
    fn packed_time(&mut self) -> Result<u64> {
        ensure!(!self.fail_clock, "choice clock unavailable");
        self.clocks += 1;
        self.events.push('C');
        Ok(0x00123456789abcde + u64::from(self.clocks))
    }
}
fn command(base: u16, index: u16) -> Cmd {
    Cmd::SELECT {
        base,
        index,
        dest: Register::from_regular_register(31),
        visibility: 7,
        title: String::new(),
        variants: vec!["A".into(), "B".into(), "C".into()],
    }
}
struct Profile {
    choice: ChoiceCommand,
    progress: ProgressMemory,
    memory: AutosaveMemory,
    history: ChoiceHistory,
    services: Services,
    boundary: u32,
    register: i32,
    input: String,
    expected: Vec<String>,
}
impl Profile {
    fn new(
        active: usize,
        length: usize,
        cursor: usize,
        boundary: u32,
        target: u32,
        starts: bool,
        base: u16,
        index: u16,
        last_len: usize,
    ) -> Self {
        let seed = 0x5a;
        let mut lengths = [7; 2];
        lengths[active] = length;
        let mut source = progress(active, seed, lengths, false);
        source.entries[active].save_position = target;
        source.entries[active].replay_target = 0xabcdef01;
        let mut state = ProgressMemory::new(PersistData(Vec::new()), source, Vec::new());
        state.restore_response_cursor(cursor).unwrap();
        let mut seen = IndexSet::new();
        if base > 0 {
            seen.add(usize::from(base));
        }
        let mut profile = Self {
            choice: ChoiceCommand::new(&command(base, index), starts).unwrap(),
            progress: state,
            memory: AutosaveMemory {
                valid: false,
                packed_time: 0,
                progress: progress(1 - active, seed, [3, 4], true),
            },
            history: ChoiceHistory::new(seen, bytes(last_len, seed)),
            services: Services {
                events: if starts { "K".into() } else { String::new() },
                ..Default::default()
            },
            boundary: boundary.wrapping_add(u32::from(starts)),
            register: -777,
            input: format!(
                "N {active} {seed:x} {length} {cursor} {boundary:x} {target:x} {} {base} {index} {last_len}\n",
                u8::from(starts)
            ),
            expected: Vec::new(),
        };
        profile.snapshot(-1);
        profile
    }
    fn snapshot(&mut self, result: i32) {
        let mut row = format!(
            "Q {result} {:08x} {} {} {} {} {} {} {} {:016x}",
            self.boundary,
            u8::from(self.choice.starts_resume),
            self.choice.replay_stop(),
            self.register,
            self.progress.responses().1,
            self.services.opens,
            self.services.clocks,
            u8::from(self.memory.valid),
            self.memory.packed_time
        );
        let mut seen = vec![0u32; self.history.seen().size_in_words()];
        for bit in self.history.seen().iter() {
            seen[bit / 32] |= 1 << (bit % 32);
        }
        let seen_bytes = seen
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        row.push_str(&format!(
            " {} {:016x} {} {:016x}",
            seen.len(),
            hash(&seen_bytes),
            self.history.last().len(),
            hash(self.history.last())
        ));
        append_progress(&mut row, self.progress.progress());
        append_progress(&mut row, &self.memory.progress);
        row.push_str(&format!(
            " {} {:08x} {} {}",
            if self.services.events.is_empty() {
                "-"
            } else {
                &self.services.events
            },
            self.services.mask,
            self.choice.request.base,
            self.choice.request.index
        ));
        self.expected.push(row);
    }
    fn live(&mut self) -> Result<bool> {
        self.choice.poll_live(
            &mut self.progress,
            &mut self.memory,
            &mut self.history,
            &mut self.boundary,
            &mut self.services,
            |dest, value| {
                assert_eq!(dest, Register::from_regular_register(31));
                self.register = value;
                Ok(())
            },
        )
    }
    fn replay(&mut self) -> bool {
        self.choice
            .poll_replay(&mut self.progress, &mut self.boundary, |dest, value| {
                assert_eq!(dest, Register::from_regular_register(31));
                self.register = value;
                Ok(())
            })
            .unwrap()
    }
    fn record_live(&mut self) {
        let result = self.live().unwrap();
        self.input.push_str("L\n");
        self.snapshot(i32::from(result));
    }
    fn select(&mut self, value: i32) {
        self.services.selected = Some(value);
        self.input.push_str(&format!("V {value}\n"));
        self.snapshot(-1);
    }
}

#[test]
#[ignore = "original updated SELECT decoder/constructor, replay/live callbacks, settings setters, four-bit codec and 11780 copies with checked UI/clock contracts"]
fn higurashi_choices_match_native_live_replay_history_and_progress_copies() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for active in [0, 1] {
        for length in [0, 1, 7, 1023, 1024, 2048] {
            for offset in 0..8 {
                let cursor = if length == 0 {
                    0
                } else {
                    (length - 1) * 8 + offset
                };
                let mut cases = Vec::new();
                for (base, index, last) in [(0, 0, 0), (1, 1, 0), (33, 33, 1), (65535, 65535, 33)] {
                    for value in [0, 1, 2] {
                        cases.push((base, index, last, value));
                    }
                }
                for value in [11, 16, 255, 256, i32::MAX] {
                    cases.push((0, 0, 0, value));
                }
                for (base, index, last, value) in cases {
                    let mut p =
                        Profile::new(active, length, cursor, 199, 400, true, base, index, last);
                    p.record_live();
                    let frozen = p.memory.clone();
                    p.record_live();
                    p.select(-1);
                    p.record_live();
                    p.select(value);
                    p.record_live();
                    assert_eq!(p.memory.progress.active_entry().save_position, 201);
                    assert_eq!(p.register, value);
                    assert_eq!(frozen.progress.active_entry().save_position, 200);
                    assert_ne!(
                        frozen.progress.active_entry().response_data.as_ptr(),
                        p.memory.progress.active_entry().response_data.as_ptr()
                    );
                    assert_eq!(
                        frozen.progress.active_entry().response_data,
                        bytes(length, 0x5a + active as u32 * 19)
                    );
                    let (data, end) = p.progress.responses();
                    assert_eq!(end, cursor + 4);
                    assert!(!data.is_empty());
                    input.push_str(&p.input);
                    expected.extend(p.expected);
                    profiles += 1;
                }
            }
        }
    }
    for active in [0, 1] {
        for starts in [false, true] {
            for (boundary, target) in [
                (0, 0),
                (0, 1),
                (1, 2),
                (199, 200),
                (u32::MAX, u32::MAX),
                (u32::MAX, 0),
            ] {
                for length in [0, 1, 7, 1023, 1024] {
                    for offset in 0..8 {
                        let cursor = if length == 0 {
                            0
                        } else {
                            (length - 1) * 8 + offset
                        };
                        let mut p = Profile::new(
                            active, length, cursor, boundary, target, starts, 33, 33, 31,
                        );
                        let continued = p.replay();
                        p.input.push_str("R\n");
                        p.snapshot(i32::from(continued));
                        let saved = p.memory.clone();
                        let history_len = p.history.last().len();
                        if !continued {
                            p.record_live();
                            if p.choice.replay_stop() == 0 {
                                p.select(2);
                                p.record_live();
                            } else {
                                assert_eq!(p.memory, saved);
                                assert_eq!(p.history.last().len(), history_len);
                                assert_eq!(p.services.opens, 0);
                            }
                        }
                        input.push_str(&p.input);
                        expected.extend(p.expected);
                        profiles += 1;
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHOICE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let actual = String::from_utf8(output.stdout).unwrap();
    let rows = actual.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), expected.len());
    for (i, (a, e)) in rows.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "native choice snapshot {i}");
    }
    println!(
        "Higurashi choices: {profiles} native profiles/{} snapshots; normalized metadata, all cursor offsets, loaded-vector growth/tails, full32 feedback versus low4 responses, seen/last history, two immutable progress copies, unsigned replay saved-boundary stops and UI-free live handoff",
        expected.len()
    );
}

#[test]
fn higurashi_choice_failures_retain_ui_copy_response_and_clock_prefixes() {
    let mut p = Profile::new(1, 7, 5, 199, 400, true, 33, 33, 1);
    p.services.fail_open = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.opens, 0);
    assert!(!p.memory.valid);
    p.services.fail_open = false;
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.opens, 1);
    assert!(p.memory.valid);
    assert_eq!(p.services.clocks, 0);
    assert_eq!(p.progress.progress().active_entry().save_position, 200);
    let original = p.memory.progress.clone();
    assert_ne!(original, p.progress.progress().clone());
    assert!(p.live().is_err());
    assert_eq!(p.memory.progress, original);
    assert_eq!(p.services.opens, 1);
    p.services.fail_clock = false;
    p.services.fail_poll = true;
    assert!(p.live().is_err());
    assert_eq!(p.services.clocks, 1);
    assert_eq!(p.progress.responses().1, 5);
    p.services.fail_poll = false;
    p.services.selected = Some(2);
    p.services.fail_clock = true;
    assert!(p.live().is_err());
    assert_eq!(p.register, 2);
    assert_eq!(p.boundary, 201);
    assert_eq!(p.progress.responses().1, 9);
    assert_eq!(p.history.last()[32], 3);
    assert!(p.history.seen().contains(34));
    let recorded = p.progress.responses().0.to_vec();
    assert!(p.live().is_err());
    assert_eq!(p.progress.responses(), (recorded.as_slice(), 9));
    assert_eq!(p.boundary, 201);
    assert_eq!(p.services.opens, 1);
    p.services.fail_clock = false;
    assert!(p.live().unwrap());
    assert_eq!(p.services.clocks, 2);
    assert_eq!(p.memory.progress, p.progress.progress().clone());
    assert!(p.live().unwrap());
    assert_eq!(p.services.clocks, 2);
    assert!(
        p.choice
            .poll_replay(&mut p.progress, &mut p.boundary, |_, _| Ok(()))
            .is_err()
    );
    let mut invalid = command(0, 0);
    if let Cmd::SELECT { variants, .. } = &mut invalid {
        variants.resize(13, "A".into());
    }
    assert!(ChoiceCommand::new(&invalid, true).is_err());
    let mut replay = Profile::new(0, 7, 0, 0, 200, false, 0, 0, 0);
    assert!(
        replay
            .choice
            .poll_replay(
                &mut replay.progress,
                &mut replay.boundary,
                |_, _| anyhow::bail!("publication unavailable")
            )
            .is_err()
    );
    assert_eq!(replay.progress.responses().1, 4);
    assert_eq!(replay.boundary, 0);
    let value = replay.choice.result().unwrap();
    assert!(
        replay
            .choice
            .poll_replay(&mut replay.progress, &mut replay.boundary, |_, actual| {
                assert_eq!(actual, value);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(replay.progress.responses().1, 4);
    assert_eq!(replay.boundary, 1);
    println!(
        "Higurashi choice failure prefixes: UI/open clock/poll/final clock retries; one response/boundary/feedback/history publication and two acknowledged copies; completed owner idempotent"
    );
}
