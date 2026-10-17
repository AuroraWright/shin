//! Source-only original item7 comparison. Parent registers/runs explicitly.
//! This executes the bounded native constructor/callback/full-factory oracle;
//! concrete Question/UI/Gameplay/source retirement are separate app tests.
use super::{Action, Destination, action, fragment_selected};
use crate::adv::higurashi::{browser_lifecycle::JUMP_BUTTONS, question};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/system-menu-fragment-corpus/input.txt"
));
#[derive(Clone, Debug, PartialEq, Eq)]
struct Record {
    scenario: i32,
    seed: u32,
    saved: u32,
    target: u32,
    responses: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Data {
    date: u64,
    active: usize,
    extra: u32,
    records: [Record; 2],
    words: Vec<u16>,
    bytes: Vec<u8>,
}
fn vector(count: usize, seed: u32) -> Vec<u32> {
    (0..count)
        .map(|i| seed.wrapping_add((i as u32).wrapping_mul(73)) ^ (i as u32).wrapping_mul(0x912d))
        .collect()
}
impl Data {
    fn new(scenario: i32, active: usize, seed: u32, lengths: [usize; 4], byte52: u8) -> Self {
        let mut bytes = vector(lengths[3], seed ^ 0xc35a)
            .into_iter()
            .map(|v| v as u8)
            .collect::<Vec<_>>();
        bytes[52] = byte52;
        Self {
            date: 0x0011_2233_4455_6677 ^ u64::from(seed),
            active,
            extra: seed ^ 0xdead_beef,
            records: std::array::from_fn(|r| Record {
                scenario: if r == active { scenario } else { 0x7fff_fffe },
                seed: seed.wrapping_add(r as u32 * 31),
                saved: 0x89ab_cdef + r as u32,
                target: 0xfedc_ba98 - r as u32,
                responses: vector(lengths[r], seed.wrapping_add(r as u32 * 17))
                    .into_iter()
                    .map(|v| v as u8)
                    .collect(),
            }),
            words: vector(lengths[2], seed ^ 0x5ac3)
                .into_iter()
                .map(|v| v as u16)
                .collect(),
            bytes,
        }
    }
    fn mutate_source(&mut self) {
        self.date ^= 0x33;
        self.extra ^= 0x55aa_55aa;
        for r in &mut self.records {
            r.seed ^= 0xf00d_f00d;
            r.saved ^= 0xf00d_f00d;
            r.target ^= 0xf00d_f00d;
            if let Some(first) = r.responses.first_mut() {
                *first ^= 0x55;
            }
        }
        if let Some(first) = self.words.first_mut() {
            *first ^= 0x55;
        }
        self.bytes[0] ^= 0x55;
    }
    fn wire(&self) -> Vec<u64> {
        // Full date, both records/all scalar bits, both response vectors,
        // high32 extra word, u16 and byte banks. No disk-save projection.
        let mut result = vec![1, self.date, self.active as u64, u64::from(self.extra)];
        for r in &self.records {
            result.extend([
                r.scenario as u32 as u64,
                u64::from(r.seed),
                u64::from(r.saved),
                u64::from(r.target),
                r.responses.len() as u64,
            ]);
            result.extend(r.responses.iter().map(|&v| u64::from(v)));
        }
        result.push(self.words.len() as u64);
        result.extend(self.words.iter().map(|&v| u64::from(v)));
        result.push(self.bytes.len() as u64);
        result.extend(self.bytes.iter().map(|&v| u64::from(v)));
        result
    }
}
fn native() -> std::process::Output {
    let mut child = Command::new(
        std::env::var("SHIN_HIGURASHI_NATIVE_MENU_FRAGMENT")
            .expect("Configure the NEW original item7 native runner explicitly"),
    )
    .args([
        std::env::var("SHIN_HIGURASHI_EXE").expect("updated executable required"),
        std::env::var("SHIN_HIGURASHI_FONT").expect("actual reviewed NewRodin font required"),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(INPUT.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    assert!(
        output.status.success(),
        "Original item7 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    output
}
fn words(line: &str, prefix: &str) -> Vec<u64> {
    line.strip_prefix(prefix)
        .unwrap_or_else(|| panic!("Expected {prefix:?}, received {line:?}"))
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect()
}
fn literal_string(value: &str) -> String {
    format!(
        "{} {}",
        value.len(),
        value
            .bytes()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

#[test]
#[ignore = "NEW original SystemMenu7 constructor/defaults/stage extremes/15e54 signed normalization/BGM1/full-data factories; requires explicit native runner+updatedEXE+FONT; CPU only"]
fn higurashi_system_menu_fragment_matches_original_378_owners_798_snapshots() {
    let first = native();
    let second = native();
    assert_eq!(
        first.stdout, second.stdout,
        "Original item7 stdout is nondeterministic"
    );
    assert_eq!(
        first.stderr, second.stderr,
        "Original item7 stderr/retirement drift"
    );
    let stdout = String::from_utf8(first.stdout).unwrap();
    let mut output = stdout.lines();
    let mut source: Option<Data> = None;
    let mut queued: Option<(i32, Data)> = None;
    let mut reopened = 0u64;
    let mut music = 0.375f32.to_bits();
    let mut owners = 0;
    let mut snapshots = 0;
    let mut copies = 0;
    let mut mutation_checks = 0;
    let mut signed_retargets = 0;
    for (line_number, input) in INPUT.lines().enumerate() {
        let mut fields = input.split_whitespace();
        let op = fields.next().unwrap();
        match op {
            "N" => {
                assert!(source.is_none());
                let scenario = fields.next().unwrap().parse::<i32>().unwrap();
                let active = fields.next().unwrap().parse::<usize>().unwrap();
                let seed = u32::from_str_radix(fields.next().unwrap(), 16).unwrap();
                let lengths = std::array::from_fn(|_| fields.next().unwrap().parse().unwrap());
                let byte52 = fields.next().unwrap().parse().unwrap();
                source = Some(Data::new(scenario, active, seed, lengths, byte52));
                queued = None;
                reopened = 0;
                music = 0.375f32.to_bits();
                owners += 1;
                assert_eq!(
                    output.next().unwrap(),
                    format!(
                        "N 1 1 {} {}",
                        literal_string(Destination::Kakera.prompt()),
                        literal_string(&JUMP_BUTTONS.join("|"))
                    ),
                    "Actual item7 Japanese prompt/defaultNo/cancellable at owner{owners}"
                );
            }
            "C" => {
                let stage = fields.next().unwrap().parse::<u32>().unwrap();
                let button_word = fields.next().unwrap().parse::<u32>().unwrap();
                let button = button_word as i32;
                assert_eq!(output.next().unwrap(), format!("C {stage} {button_word}"));
                match action(question::Callback { stage, button }) {
                    Action::Reopen => reopened += 1,
                    Action::Return => {
                        let data = source.as_ref().unwrap();
                        let selected = fragment_selected(data.records[data.active].scenario);
                        queued = Some((selected, data.clone()));
                        music = 1f32.to_bits();
                        copies += 1;
                    }
                    Action::None => {}
                }
            }
            "I" => {
                let scenario = fields.next().unwrap().parse().unwrap();
                let data = source.as_mut().unwrap();
                data.records[data.active].scenario = scenario;
                signed_retargets += 1;
            }
            "M" => {
                source.as_mut().unwrap().mutate_source();
                assert_ne!(&queued.as_ref().unwrap().1, source.as_ref().unwrap());
                mutation_checks += 1;
            }
            "D" => {
                assert_eq!(output.next().unwrap(), "X 0 0 1", "Balanced owner{owners}");
                source = None;
                queued = None;
                assert!(fields.next().is_none());
                continue;
            }
            _ => panic!("Unknown source-only corpus operation {op}"),
        }
        assert!(fields.next().is_none());
        let observed = output.next().unwrap();
        let prefix = format!("S {op} ");
        let state = words(observed, &prefix);
        assert_eq!(
            state.len(),
            7,
            "Native state schema at inputline{line_number}"
        );
        assert_eq!(
            &state[..4],
            &[
                reopened,
                u64::from(music),
                u64::from(queued.is_some()),
                queued
                    .as_ref()
                    .map_or(0, |(selected, _)| *selected as u32 as u64),
            ],
            "Actual callback/BGM/factory owner{owners} inputline{line_number} {input}"
        );
        assert_eq!(
            state[4], 1,
            "Question must not close/release the parent phase"
        );
        assert_eq!(state[5], 4, "Actual fragment roster item7 index");
        assert_eq!(
            words(output.next().unwrap(), "G"),
            source.as_ref().unwrap().wire(),
            "Direct helper mutated full source owner{owners} inputline{line_number}"
        );
        if let Some((_, data)) = &queued {
            assert_eq!(
                words(output.next().unwrap(), "Q"),
                data.wire(),
                "Original full four-vector copy owner{owners} inputline{line_number}"
            );
        }
        snapshots += 1;
    }
    assert!(output.next().is_none(), "Unconsumed native observations");
    assert!(source.is_none());
    assert_eq!(
        (owners, snapshots, copies, mutation_checks, signed_retargets),
        (378, 798, 54, 12, 30)
    );
    eprintln!(
        "SystemMenu7:378 original constructors/798 snapshots,twice identical stdout/stderr,literal prompt/defaultNo/cancel contract,real root staged action and signed normalization,54 actual BGM1/full-data factory returns,12 independent source-mutation copies,30 explicitly synthetic direct-helper signed boundaries,all native owners balanced; actual Question/media/scene/global lifetime acceptance remains separate"
    );
}
