//! Strict item5 original-function transport/model comparison. This test does not
//! execute real Question/writer/audio/platform source replacement services.
use super::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[derive(serde::Deserialize)]
struct Profile {
    kind: String,
    scenario: i32,
    active: u32,
    salt: u32,
    stage: u32,
    button: i32,
}
#[derive(Debug)]
struct Observation {
    op: char,
    reopens: u32,
    questions: u32,
    save: u32,
    music: u32,
    titles: u32,
    selected: u32,
    mode: u32,
    save_phase: i32,
    save_flag: u32,
    menu_phase: u32,
    children: u32,
    globals: u64,
    current: u64,
    frozen: u64,
}
impl Observation {
    fn parse(line: &str, op: char) -> Self {
        let words = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words.len(), 16, "{line}");
        assert_eq!(words[0], "O");
        assert_eq!(words[1], op.to_string());
        let value = |i: usize| words[i].parse::<u32>().unwrap();
        Self {
            op,
            reopens: value(2),
            questions: value(3),
            save: value(4),
            music: value(5),
            titles: value(6),
            selected: value(7),
            mode: value(8),
            save_phase: words[9].parse().unwrap(),
            save_flag: value(10),
            menu_phase: value(11),
            children: value(12),
            globals: u64::from_str_radix(words[13], 16).unwrap(),
            current: u64::from_str_radix(words[14], 16).unwrap(),
            frozen: u64::from_str_radix(words[15], 16).unwrap(),
        }
    }
    fn parent(&self, initial: &Self, saved: bool, reopens: u32) {
        assert_eq!(self.questions, 1, "{self:?}");
        assert_eq!(self.save, u32::from(saved), "{self:?}");
        assert_eq!(self.reopens, reopens, "{self:?}");
        assert_eq!(self.menu_phase, 1, "{self:?}");
        assert_eq!(self.children, if saved { 2 } else { 1 }, "{self:?}");
        assert_eq!(
            self.current, initial.current,
            "live full GameData/date changed"
        );
        if saved {
            assert_eq!(self.save_flag, 1, "15b6c/ad4e0 save flag");
            assert_eq!(
                self.frozen, initial.globals,
                "full101-row/two-record snapshot changed"
            );
        } else {
            assert_eq!(self.frozen, 0);
            assert_eq!(self.save_flag, 0);
        }
    }
}
fn question(line: &str) {
    let mut words = line.split_whitespace();
    assert_eq!(words.next(), Some("Q"));
    assert_eq!(words.next(), Some("1"), "native default No");
    assert_eq!(words.next(), Some("1"), "native cancellation enabled");
    let mut text = || {
        let length = words.next().unwrap().parse::<usize>().unwrap();
        let bytes = (0..length)
            .map(|_| words.next().unwrap().parse::<u8>().unwrap())
            .collect::<Vec<_>>();
        String::from_utf8(bytes).unwrap()
    };
    assert_eq!(text(), Destination::Title.prompt());
    assert_eq!(
        text(),
        crate::adv::higurashi::browser_lifecycle::JUMP_BUTTONS.join("|")
    );
    assert_eq!(words.next(), None);
}
fn run_original(input: &[u8], run: usize) -> Output {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MENU_RETURN").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    //Preserve failures before assertions; never regenerate frozen expected data.
    if let Ok(folder) = std::env::var("SHIN_HIGURASHI_MENU_RETURN_CAPTURE") {
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Path::new(&folder);
        std::fs::write(folder.join("input.txt"), input).unwrap();
        std::fs::write(folder.join(format!("output-{run}.txt")), &output.stdout).unwrap();
        std::fs::write(folder.join(format!("stderr-{run}.txt")), &output.stderr).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    output
}

#[test]
#[ignore = "strict actual item5 prompt/callback/full native SaveGame snapshot/Title(-1,-1);requires parent-validated original runner+frozen output, no GPU/audio/window/user saves"]
fn higurashi_system_menu_title_prompt_save_and_staged_return_match_native() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../reverse-engineering/higurashi/system-menu-return-capture");
    let input = std::fs::read(folder.join("input.txt")).unwrap();
    let profiles: Vec<Profile> =
        serde_json::from_slice(&std::fs::read(folder.join("profiles.json")).unwrap()).unwrap();
    assert_eq!(profiles.len(), 350);
    //Reference outputs must be captured and frozen by the parent BEFORE this
    //test runs. Missing reference/environment is a failure, never a skip/pass.
    let reference = PathBuf::from(std::env::var("SHIN_HIGURASHI_MENU_RETURN_REFERENCE").unwrap());
    assert_eq!(std::fs::read(reference.join("input.txt")).unwrap(), input);
    let expected = std::fs::read(reference.join("output.txt")).unwrap();
    let expected_stderr = std::fs::read(reference.join("stderr.txt")).unwrap();
    let mut outputs = Vec::new();
    for run in 0..2 {
        let output = run_original(&input, run);
        assert_eq!(
            output.stdout, expected,
            "unchanged original stdout rerun{run}"
        );
        assert_eq!(
            output.stderr, expected_stderr,
            "unchanged original stderr rerun{run}"
        );
        outputs.push(output);
    }
    assert_eq!(outputs[0].stdout, outputs[1].stdout);
    assert_eq!(outputs[0].stderr, outputs[1].stderr);
    let output = std::str::from_utf8(&outputs[0].stdout).unwrap();
    let mut lines = output.lines();
    let mut observations = 0;
    for (index, profile) in profiles.iter().enumerate() {
        let label = format!(
            "profile{index}/{}/{}/{}/{}",
            profile.kind, profile.scenario, profile.active, profile.salt
        );
        question(lines.next().expect(&label));
        let initial = Observation::parse(lines.next().expect(&label), 'N');
        initial.parent(&initial, false, 0);
        assert_eq!(
            (
                initial.music,
                initial.titles,
                initial.selected,
                initial.mode
            ),
            (0, 0, 0, 0)
        );
        observations += 1;
        match profile.kind.as_str() {
            "callback" | "complete" | "cancel-save" => {
                let actual = Observation::parse(lines.next().expect(&label), 'C');
                let expected = action(question::Callback {
                    stage: profile.stage,
                    button: profile.button,
                });
                let saved = expected == Action::Return;
                actual.parent(&initial, saved, u32::from(expected == Action::Reopen));
                assert_eq!(
                    actual.globals, initial.globals,
                    "callback changed global data: {label}"
                );
                assert_eq!((actual.music, actual.titles, actual.save_phase), (0, 0, 0));
                observations += 1;
                if profile.kind == "complete" {
                    assert!(saved);
                    let mutated = Observation::parse(lines.next().expect(&label), 'M');
                    mutated.parent(&initial, true, 0);
                    assert_ne!(mutated.globals, initial.globals);
                    assert_eq!(
                        (mutated.music, mutated.titles, mutated.save_phase),
                        (0, 0, 0)
                    );
                    let before_stage = Observation::parse(lines.next().expect(&label), 'W');
                    before_stage.parent(&initial, true, 0);
                    assert_eq!(before_stage.globals, mutated.globals);
                    assert_eq!(
                        (
                            before_stage.music,
                            before_stage.titles,
                            before_stage.save_phase
                        ),
                        (0, 0, 0)
                    );
                    let completed = Observation::parse(lines.next().expect(&label), 'W');
                    completed.parent(&initial, true, 0);
                    assert_eq!(
                        (completed.music, completed.titles, completed.save_phase),
                        (0, 0, 2)
                    );
                    for _ in 0..2 {
                        let queued = Observation::parse(lines.next().expect(&label), 'U');
                        queued.parent(&initial, true, 0);
                        assert_eq!(queued.op, 'U');
                        assert_eq!(queued.globals, mutated.globals);
                        assert_eq!((queued.music, queued.titles, queued.save_phase), (1, 1, -1));
                        assert_eq!((queued.selected, queued.mode), (u32::MAX, u32::MAX));
                    }
                    observations += 5;
                }
            }
            "cancel-question" => {}
            _ => panic!("Unknown {label}"),
        }
        assert_eq!(lines.next(), Some("D"), "{label}");
    }
    assert_eq!(lines.next(), None);
    assert_eq!(observations, 759);
    eprintln!(
        "SystemMenu item5:350 original owners/759 observations,336 callback extremes,12 retained complete SaveGame deep-snapshot/Title(-1,-1) routes,2 pending disposals;original stdout/stderr twice exact before model comparison;checked Question/writer-completion/audio/queue services,no app/storage/codec/GPU/window claim"
    );
}
