use super::*;
use crate::adv::higurashi::autosave::{Execution, ProgressEntry, SaveProgress};
use shin_core::{
    format::save::PersistData, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Default)]
struct Services {
    starts: [u32; 9],
    gates: [u32; 4],
    busy: [bool; 4],
    achievements: [u32; 2],
    destination: u32,
    selected: i32,
    exiting: bool,
    pause: u32,
    fail_begin: Option<usize>,
    fail_destination: bool,
}
impl ExitServices for Services {
    fn begin(&mut self, action: Begin) -> Result<()> {
        let i = BEGIN.iter().position(|&a| a == action).unwrap();
        if self.fail_begin == Some(i) {
            self.fail_begin = None;
            anyhow::bail!("injected EXIT action failure");
        }
        self.starts[i] += 1;
        if i == 0 {
            self.exiting = true;
        }
        if i == 1 {
            self.pause = 0;
        }
        Ok(())
    }
    fn wait_busy(&mut self) -> Result<bool> {
        self.gates[0] += 1;
        Ok(self.busy[0])
    }
    fn message_busy(&mut self) -> Result<bool> {
        self.gates[1] += 1;
        Ok(self.busy[1])
    }
    fn view_child_busy(&mut self) -> Result<bool> {
        self.gates[2] += 1;
        Ok(self.busy[2])
    }
    fn modes_ready(&mut self) -> Result<bool> {
        self.gates[3] += 1;
        Ok(self.busy[3])
    }
    fn achievement(&mut self, id: u32) -> Result<()> {
        self.achievements[usize::from(id == 37)] += 1;
        Ok(())
    }
    fn destination(&mut self, d: Destination, _: &mut ProgressMemory) -> Result<()> {
        if std::mem::take(&mut self.fail_destination) {
            anyhow::bail!("injected EXIT destination failure");
        }
        match d {
            Destination::SaveThenTitle => self.destination = 1,
            Destination::Tips => self.destination = 2,
            Destination::Kakera { selected } => {
                self.destination = 3;
                self.selected = selected;
            }
            Destination::Bonus => self.destination = 4,
        }
        Ok(())
    }
}
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn vector(out: &mut String, bytes: &[u8], width: usize) {
    *out += &format!(" {} {:016x}", bytes.len() / width, hash(bytes));
}
fn snapshot(
    command: &ExitCommand,
    progress: &ProgressMemory,
    s: &Services,
    result: bool,
) -> String {
    let mut out = format!(
        "E {} {} {} {} {}",
        u8::from(result),
        command.phase(),
        u8::from(command.blocks_fast_forward()),
        u8::from(s.exiting),
        s.pause
    );
    for count in s.starts[2..].iter().chain(&s.gates) {
        out += &format!(" {count}");
    }
    out += &format!(
        " {} {} {} {} 0",
        s.achievements[0], s.achievements[1], s.destination, s.selected as u32
    );
    for pairs in [progress.pending_persistent(), progress.pending_bytes()] {
        let raw: Vec<_> = pairs
            .iter()
            .flat_map(|(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes()))
            .collect();
        vector(&mut out, &raw, 8);
    }
    vector(
        &mut out,
        &progress
            .persistent()
            .0
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        2,
    );
    vector(&mut out, progress.bytes(), 1);
    out
}
fn progress(scenario: i32, active: usize, persistent: Vec<i16>, bytes: Vec<u8>) -> ProgressMemory {
    ProgressMemory::new(
        PersistData(persistent),
        SaveProgress::new(
            active,
            0xdeadbeef,
            std::array::from_fn(|i| ProgressEntry {
                scenario_id: if i == active { scenario } else { 2084 },
                random_seed: 42 + i as u32,
                save_position: 0,
                replay_target: 0,
                response_data: vec![0x23, 0xee],
            }),
        )
        .unwrap(),
        bytes,
    )
}
#[test]
#[ignore = "executes updated420/NOPE/447c/4484/448c against guarded native image;ordered close gates,commits,completion predicates and all EXIT destinations"]
fn higurashi_exit_owner_gates_commits_and_routes_match_native() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for scenario in [
        i32::MIN,
        -1,
        0,
        999,
        1000,
        1999,
        2000,
        2999,
        3000,
        9999,
        10000,
        10999,
        11000,
        11999,
        12000,
        12048,
        12049,
        12050,
        12051,
        12999,
        13000,
        i32::MAX,
    ] {
        for mode in [0, 1, 255] {
            for argument in [0, 1, -1] {
                for kind in 0..6 {
                    for active in [0, 1] {
                        let persistent: Vec<i16> = (0..if kind == 0 { 0 } else { 192 })
                            .map(|i| match kind {
                                1 => 0,
                                2 => 1,
                                3 => 2,
                                4 => {
                                    if i == 18 || i == 161 {
                                        1
                                    } else {
                                        2
                                    }
                                }
                                5 => {
                                    if i % 2 == 0 {
                                        i16::MIN
                                    } else {
                                        i16::MAX
                                    }
                                }
                                _ => unreachable!(),
                            })
                            .collect();
                        let bytes: Vec<u8> = vec![
                            if kind == 3 {
                                2
                            } else if kind == 4 {
                                1
                            } else {
                                0
                            };
                            if kind == 0 { 0 } else { 64 }
                        ];
                        input += &format!(
                            "N {mode} {argument} {scenario} {active} {} {}\n",
                            persistent.len(),
                            bytes.len()
                        );
                        for v in &persistent {
                            input += &format!("{v} ");
                        }
                        input += "\n";
                        for v in &bytes {
                            input += &format!("{v} ");
                        }
                        input += "\n";
                        let mut progress = progress(scenario, active, persistent, bytes);
                        let mut command = ExitCommand::new(mode, argument);
                        let mut services = Services {
                            pause: 7,
                            ..Default::default()
                        };
                        expected.push(snapshot(&command, &progress, &services, false));
                        input += "R\nF\n";
                        expected.push(snapshot(
                            &command,
                            &progress,
                            &services,
                            command.poll_replay(),
                        ));
                        expected.push(snapshot(
                            &command,
                            &progress,
                            &services,
                            command.forward_complete(),
                        ));
                        // Both queues remain pending through all UI gates. Completion sees
                        // committed,16/8-bit truncated values only when8d18 enables11a94.
                        for (bank, slot, value) in [(0, 18, 2), (0, 161, 2), (1, 52, 258)] {
                            input += &format!("P {bank} {slot} {value}\n");
                            let cmd = if bank == 0 {
                                Cmd::SSET {
                                    args: [slot, value],
                                }
                            } else {
                                Cmd::OpcodeDB {
                                    args: [slot, value],
                                }
                            };
                            progress
                                .dispatch_variables(&cmd, Execution::Live, 0)
                                .unwrap();
                            expected.push(snapshot(&command, &progress, &services, false));
                        }
                        for busy in [
                            [true, false, false, true],
                            [false, true, false, true],
                            [false, false, true, true],
                            [false, false, false, false],
                            [false, false, false, true],
                            [false, false, false, true],
                        ] {
                            services.busy = busy;
                            let enabled = kind % 2 == 1;
                            input += &format!(
                                "U {} {} {} {} {}\n",
                                u8::from(busy[0]),
                                u8::from(busy[1]),
                                u8::from(busy[2]),
                                u8::from(busy[3]),
                                u8::from(enabled)
                            );
                            let result = command
                                .poll_live(&mut progress, enabled, &mut services)
                                .unwrap();
                            expected.push(snapshot(&command, &progress, &services, result));
                        }
                        assert_eq!(progress.responses(), (&[0x23, 0xee][..], 0));
                        assert_eq!(progress.progress().extra_word, 0xdeadbeef);
                        input += "D\n";
                        expected.push("D".into());
                        owners += 1;
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_EXIT").unwrap())
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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (i, (actual, expected)) in text.lines().zip(&expected).enumerate() {
        assert_eq!(actual, *expected, "EXIT native observation{i}");
    }
    let path = "/tmp/higurashi-exit-command-capture";
    std::fs::create_dir_all(path).unwrap();
    for (name, data) in [
        ("input.txt", input.as_bytes()),
        ("output.txt", text.as_bytes()),
        ("stderr.txt", &output.stderr),
    ] {
        std::fs::write(format!("{path}/{name}"), data).unwrap();
    }
    println!(
        "Higurashi EXIT:{owners} native owners/{} exact observations;NOPE modes,replay/forward barriers,ordered nine close actions/four gates,signed scenario ranges,committed completion/unlock predicates,all four destinations and unchanged responses/full extra word pass",
        expected.len() - owners
    );
}
#[test]
fn exit_service_failures_retry_only_unacknowledged_close_or_destination() {
    let mut p = progress(0, 0, vec![2; 192], vec![0; 64]);
    let mut c = ExitCommand::new(0, 1);
    let mut s = Services {
        fail_begin: Some(0),
        busy: [false, false, false, true],
        ..Default::default()
    };
    assert!(c.poll_live(&mut p, true, &mut s).is_err());
    assert!(s.starts.iter().all(|&v| v == 0));
    s.fail_begin = Some(7);
    assert!(c.poll_live(&mut p, true, &mut s).is_err());
    assert_eq!(s.starts, [1, 1, 1, 1, 1, 1, 1, 0, 0]);
    s.fail_destination = true;
    assert!(c.poll_live(&mut p, true, &mut s).is_err());
    assert_eq!(s.starts, [1; 9]);
    assert_eq!(s.achievements, [1, 0]);
    assert_eq!(s.destination, 0);
    assert!(!c.poll_live(&mut p, true, &mut s).unwrap());
    assert_eq!(s.starts, [1; 9]);
    assert_eq!(s.achievements, [1, 0]);
    assert_eq!(s.destination, 1);
    assert_eq!(c.phase(), -1);
}
