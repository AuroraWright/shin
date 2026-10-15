//! Actual original448c fresh Bonus fields, independent of the production helper.
//! Unwritten native date/dormant scalars are synthetic tags in the transport and
//! excluded from equality; deterministic Rust zeros have a separate assertion.
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[derive(serde::Deserialize)]
struct Profile {
    kind: String,
    seed: u32,
    active: u32,
    fill: u32,
    mode: u8,
    scenario: i32,
    observations: usize,
}
fn run_original(input: &[u8], run: usize) -> Output {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BONUS_EXIT").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(folder) = std::env::var("SHIN_HIGURASHI_BONUS_EXIT_CAPTURE") {
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
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
fn written_fields(data: &crate::app::higurashi::runtime_game_data::RuntimeGameData) -> [u32; 10] {
    let first = &data.progress.entries[0];
    [
        data.progress.active_index() as u32,
        data.progress.extra_word,
        first.scenario_id as u32,
        first.random_seed,
        first.save_position,
        first.replay_target,
        first.response_data.len() as u32,
        data.progress.entries[1].response_data.len() as u32,
        data.trailing_values.len() as u32,
        data.byte_variables.len() as u32,
    ]
}
fn deterministic_unwritten_adapter(
    data: &crate::app::higurashi::runtime_game_data::RuntimeGameData,
) {
    let d = data.date;
    assert_eq!(
        (d.year, d.month, d.day, d.hour, d.minute, d.second),
        (0, 0, 0, 0, 0, 0)
    );
    let dormant = &data.progress.entries[1];
    assert_eq!(
        (
            dormant.scenario_id,
            dormant.random_seed,
            dormant.save_position,
            dormant.replay_target
        ),
        (0, 0, 0, 0)
    );
    //These zeros are an explicit deterministic desktop adapter. Original448c
    //does not initialize them; do not compare them to native tagged scalars.
}

#[test]
#[ignore = "actual448c/11a94/17490/275b0 fresh Bonus/one-seed/gates/committed byte52;requires parent-frozen original runner/output, no GPU/audio/window/save"]
fn higurashi_bonus_fresh_assigned_data_matches_original_exit_factory() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../reverse-engineering/higurashi/bonus-exit-capture");
    let input = std::fs::read(folder.join("input.txt")).unwrap();
    let profiles: Vec<Profile> =
        serde_json::from_slice(&std::fs::read(folder.join("profiles.json")).unwrap()).unwrap();
    assert_eq!(profiles.len(), 62);
    let reference = PathBuf::from(std::env::var("SHIN_HIGURASHI_BONUS_EXIT_REFERENCE").unwrap());
    assert_eq!(std::fs::read(reference.join("input.txt")).unwrap(), input);
    let expected = std::fs::read(reference.join("output.txt")).unwrap();
    let expected_stderr = std::fs::read(reference.join("stderr.txt")).unwrap();
    let output = run_original(&input, 0);
    let repeat = run_original(&input, 1);
    assert_eq!(output.stdout, expected, "unchanged original stdout");
    assert_eq!(output.stderr, expected_stderr, "unchanged original stderr");
    assert_eq!(repeat.stdout, output.stdout, "second original stdout");
    assert_eq!(repeat.stderr, output.stderr, "second original stderr");
    let text = std::str::from_utf8(&output.stdout).unwrap();
    let mut lines = text.lines();
    let (mut observations, mut factory_observations, mut bonus_owners) = (0, 0, 0);
    for (index, profile) in profiles.iter().enumerate() {
        let mut saw_bonus = false;
        for _ in 0..profile.observations {
            let line = lines.next().unwrap();
            let w = line.split_whitespace().collect::<Vec<_>>();
            let label = format!(
                "profile{index}/{}/{}: {line}",
                profile.kind, profile.scenario
            );
            assert!(w.len() >= 35, "{label}");
            assert_eq!(w[0], "B");
            let n = |i: usize| w[i].parse::<u32>().unwrap();
            let phase = w[3].parse::<i32>().unwrap();
            let queued = n(7) != 0;
            assert_eq!(n(8), 1, "source alive while constructor/seed/queue run");
            assert_eq!(n(10), profile.active);
            assert_eq!(n(11), 0xdeadbeef, "source extra word stays independent");
            assert_eq!(u64::from_str_radix(w[12], 16).unwrap(), hash(&[0x39; 7]));
            assert_eq!([n(13), n(14), n(15), n(16)], [3, 4, 5, 64]);
            assert_eq!(
                n(6),
                u32::from(queued),
                "one seed and retained actual factory: {label}"
            );
            if matches!(w[1], "R" | "F") {
                assert_eq!(n(2), u32::from(profile.mode != 0));
                assert!(!queued);
            }
            if w[1] == "U" && phase == -1 && profile.mode == 0 {
                assert_eq!(
                    queued,
                    profile.scenario == 12049 && n(9) >= 2,
                    "committed-byte guard:{label}"
                );
            }
            if queued {
                assert_eq!(n(4), 4, "actual Bonus route");
                assert_eq!(n(17), 0, "actual17490 flags0");
                assert_eq!(
                    n(18),
                    1,
                    "native valid1,implicit in owned live RuntimeGameData request"
                );
                let native: [u32; 10] = std::array::from_fn(|i| n(19 + i));
                let actual = super::fresh(profile.seed);
                assert_eq!(
                    written_fields(&actual),
                    native,
                    "native-assigned data only:{label}"
                );
                deterministic_unwritten_adapter(&actual);
                assert_eq!(
                    u64::from_str_radix(w[29], 16).unwrap(),
                    hash(&[profile.fill as u8; 7])
                );
                for i in 0..4 {
                    assert_eq!(
                        n(30 + i),
                        0x7e000000 + profile.fill * 65537 + i as u32,
                        "synthetic dormant tag preserved by original factory copy"
                    );
                }
                assert_eq!(n(34), 2);
                assert_eq!(w.len(), 37);
                assert_eq!([n(35), n(36)], [u32::from(b'S'), u32::from(b'Q')]);
                factory_observations += 1;
                saw_bonus = true;
            } else {
                assert_eq!(n(34), 0);
                assert_eq!(w.len(), 35);
                assert!(w[17..29].iter().all(|v| *v == "0"));
            }
            observations += 1;
        }
        assert_eq!(
            lines.next(),
            Some("D 0"),
            "balanced factory/source fixture disposal"
        );
        bonus_owners += usize::from(saw_bonus);
    }
    assert_eq!(lines.next(), None);
    assert_eq!(
        (observations, factory_observations, bonus_owners),
        (422, 88, 44)
    );
    eprintln!(
        "Bonus fresh:62 actual original EXIT owners/422 states,44 Bonus owners/88 retained factory observations/880 assigned helper values;one seed after gates+actual commits before source disposal,flags0/valid1/fresh10010/reset vectors;date/dormant synthetic native tags excluded and desktopzero adapter separate;no real destination ctor/source death/entropy/GPU/audio/window/save claim"
    );
}
