//! Discover saved Movie origins with actual Episode8 controller responses.
//! CPU metadata replay only; this does not replace live route validation.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::{Command, Stdio},
};

use super::{native_replay_tests::scenario_bytes, *};
use crate::adv::{StartableCommand, command};
use shin_core::{time::Ticks, vm::command::types::LayerType};

fn controller_response(kind: i32, arg: i32, last_choice: i32) -> i32 {
    use crate::layer::user::{
        cake_layer::CakeModel,
        last_layer::LastModel,
        murderer::model::{MurdererMenu, MurdererModel, SUBMIT_ENTRY},
        quiz_layer::QuizModel,
    };
    match kind {
        0 => {
            let mut quiz = QuizModel::new(arg);
            quiz.confirm();
            quiz.update(Ticks::from_f32(7.0));
            quiz.update(Ticks::ZERO);
            quiz.result.expect("Authored Quiz first visible response")
        }
        1 => {
            let mut cake = CakeModel::default();
            cake.pointer(glam::vec2(850.0, 700.0), true);
            cake.update(Ticks::from_f32(7.0));
            cake.update(Ticks::ZERO);
            cake.result.unwrap()
        }
        2 => {
            let mut murderer = Box::new(MurdererModel::new(arg));
            murderer.navigate(5);
            murderer.confirm();
            for _ in 0..240 {
                murderer.update(1.0);
            }
            assert_eq!(murderer.menu, MurdererMenu::Accusation);
            for suspect in 34..37 {
                assert!(murderer.activate_accusation_entry(suspect));
            }
            assert!(murderer.activate_accusation_entry(SUBMIT_ENTRY));
            for _ in 0..300 {
                if let Some(value) = murderer.poll_result() {
                    return value;
                }
                murderer.update(1.0);
            }
            panic!("Authored Murderer result did not complete")
        }
        3 => {
            let mut last = LastModel::default();
            last.navigate(if last_choice == 0 { -1 } else { 1 });
            // Last holds the slide until the choice flash has completed.
            last.update(7.0);
            last.update(20.0);
            last.confirm();
            last.update(7.0);
            assert_eq!(last.result, Some(last_choice));
            last.result.unwrap()
        }
        _ => panic!("Unexpected controller kind{kind}"),
    }
}

struct MovieScan {
    previous_boundary: u32,
    counters: [Option<u32>; 48],
    loads: Vec<(u32, u8, i32, u32)>,
    checkpoints: Vec<(u32, u8, i32)>,
    simultaneous: BTreeSet<(u32, Vec<(u8, i32)>)>,
    packets: BTreeMap<u32, Vec<u8>>,
}

impl MovieScan {
    fn new() -> Self {
        Self {
            previous_boundary: 0,
            counters: [None; 48],
            loads: Vec::new(),
            checkpoints: Vec::new(),
            simultaneous: BTreeSet::new(),
            packets: BTreeMap::new(),
        }
    }
    fn observe(&mut self, state: &VmState) {
        let mut active = Vec::new();
        for (bank, layer) in &state.layers.layerbanks {
            if layer.layer_type != Some(LayerType::Movie) {
                continue;
            }
            active.push((bank.raw(), layer.params.0));
            let counter = layer.layer_load_counter;
            if self.counters[bank.raw() as usize] != Some(counter) {
                self.loads
                    .push((state.save_position, bank.raw(), layer.params.0, counter));
                self.counters[bank.raw() as usize] = Some(counter);
            }
            if state.save_position != self.previous_boundary {
                self.checkpoints
                    .push((state.save_position, bank.raw(), layer.params.0));
                self.packets.entry(state.save_position).or_insert_with(|| {
                    let mut packet = SaveData {
                        persist_data: state.persist.clone(),
                        save_vectors: state.save_vectors.clone(),
                        auto_save_slot: state.auto_save_slot.clone(),
                        ..SaveData::default()
                    };
                    packet.manual_save_slots[99] =
                        Some(GameData::from_entry(state.snapshot_entry()));
                    packet.encode()
                });
            }
        }
        if active.len() > 1 {
            self.simultaneous.insert((state.save_position, active));
        }
        self.previous_boundary = state.save_position;
    }
}

fn scan(
    scenario: &Scenario,
    last_choice: i32,
) -> (VmState, u32, MovieScan, Vec<(u32, i32, i32, i32)>) {
    let mut script = Scripter::new(scenario, 7, 12345);
    let mut state = VmState::for_scenario(7, 12345);
    let mut result = CommandResult::None;
    let mut movies = MovieScan::new();
    let mut decisions = Vec::new();
    let mut seen = BTreeSet::new();
    for _ in 0..500_000 {
        let current = script.run(result).unwrap();
        result = match current {
            RuntimeCommand::QUIZ(quiz) => {
                let target = quiz.apply_state(&mut state).unwrap();
                movies.observe(&state);
                let params = state.layers.layerbanks[target.1].params;
                let response = controller_response(params.0, params.1, last_choice);
                let pc = script.position().0;
                assert!(seen.insert(pc), "Unexpected repeated controller at{pc:x}");
                decisions.push((pc, params.0, params.1, response));
                command::replay_quiz_result(&mut state, Some(target), response);
                quiz.token.finish(response)
            }
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut state);
                get.token.finish(value)
            }
            RuntimeCommand::EXIT(exit) => {
                exit.apply_state(&mut state);
                movies.observe(&state);
                if exit.arg1 == 0 {
                    assert_eq!(decisions.len(), 19);
                    return (state, script.position().0, movies, decisions);
                }
                exit.token.finish()
            }
            current => {
                assert!(!matches!(
                    current,
                    RuntimeCommand::SELECT(_) | RuntimeCommand::RESUME(_)
                ));
                command::apply_command_state_ref(&current, &mut state).unwrap();
                current.execute_dummy().unwrap()
            }
        };
        movies.observe(&state);
    }
    panic!("Authored Episode8 checkpoint discovery exceeded command budget")
}

#[test]
#[ignore = "requires original/1.03 supplied executables and ROMs; CPU only"]
fn reference_authored_episode8_recorded_movie_checkpoints() {
    std::thread::Builder::new()
        .name("reference-episode8-movie-checkpoints".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(verify)
        .unwrap()
        .join()
        .unwrap();
}

fn verify() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = root.join("game-data/umineko");
    let output =
        std::path::PathBuf::from(std::env::var_os("SHIN_EPISODE8_MOVIE_SCAN_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let helper = output.join("native-authored-movie-checkpoints");
    assert!(
        Command::new("clang")
            .args([
                "-DNATIVE_AUTHORED_MOVIE_ROUTE_TRACE",
                "-O0",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-unused-function"
            ])
            .arg(root.join("reverse-engineering/native-authored-movie-checkpoints.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut summaries = Vec::new();
    for (version, executable, rom) in [
        ("original", "uncompressed_executable", "data.rom"),
        ("1.03", "uncompressed_executable_update", "patch.rom"),
    ] {
        let bytes = scenario_bytes(&files.join(rom));
        let scenario = Scenario::new(bytes.into()).unwrap();
        let directory = output.join(version);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("main.snr");
        std::fs::write(&source, scenario.raw()).unwrap();
        for last_choice in 0..2 {
            let (state, pc, movies, decisions) = scan(&scenario, last_choice);
            for (boundary, packet) in &movies.packets {
                std::fs::write(
                    directory.join(format!("last{last_choice}-boundary{boundary}.dat")),
                    packet,
                )
                .unwrap();
            }
            let route: Vec<u8> =
                serde_json::from_value(serde_json::to_value(state.route.snapshot()).unwrap())
                    .unwrap();
            assert!(route.len() <= 8192);
            let hex: String = route.iter().map(|b| format!("{b:02x}")).collect();
            let input = directory.join(format!("last{last_choice}.input"));
            std::fs::write(
                &input,
                format!("7 {} 3039 {hex}\n", state.save_position + 1),
            )
            .unwrap();
            let native = Command::new(&helper)
                .arg(files.join(executable))
                .arg(&source)
                .arg("--recorded-route")
                .stdin(Stdio::from(std::fs::File::open(&input).unwrap()))
                .output()
                .unwrap();
            std::fs::write(
                directory.join(format!("last{last_choice}.native.txt")),
                &native.stdout,
            )
            .unwrap();
            std::fs::write(
                directory.join(format!("last{last_choice}.native.stderr")),
                &native.stderr,
            )
            .unwrap();
            assert!(
                native.status.success(),
                "{}",
                String::from_utf8_lossy(&native.stderr)
            );
            let text = String::from_utf8(native.stdout).unwrap();
            let mut loads = Vec::new();
            let mut checkpoints = Vec::new();
            let mut ended = false;
            for row in text.lines() {
                let fields = row.split_whitespace().collect::<Vec<_>>();
                match fields[0] {
                    "O" => loads.push((
                        fields[2].parse::<u32>().unwrap(),
                        fields[3].parse::<u8>().unwrap(),
                        fields[4].parse::<i32>().unwrap(),
                        fields[5].parse::<u32>().unwrap(),
                    )),
                    "C" => checkpoints.push((
                        fields[2].parse::<u32>().unwrap(),
                        fields[3].parse::<u8>().unwrap(),
                        fields[4].parse::<i32>().unwrap(),
                    )),
                    "R" => {
                        assert_eq!(fields[3].parse::<u32>().unwrap(), state.save_position);
                        assert_eq!(u32::from_str_radix(fields[4], 16).unwrap(), pc);
                        assert_eq!(fields[5], "0", "Canonical route uses primary context");
                    }
                    "T" => {
                        assert_eq!(
                            fields[2].parse::<usize>().unwrap(),
                            state.route.bit_position()
                        );
                    }
                    "S" => {
                        assert_eq!(fields[2].parse::<u32>().unwrap(), state.save_position);
                        assert!(fields[3].parse::<u32>().unwrap() < 1_000_000);
                        assert_eq!(fields[6], "1");
                        ended = true;
                    }
                    "D" | "X" => {}
                    other => panic!("Unexpected native trace row{other}"),
                }
            }
            assert!(ended);
            assert_eq!(
                loads, movies.loads,
                "{version} Last{last_choice} Movie load descriptors"
            );
            assert_eq!(
                checkpoints, movies.checkpoints,
                "{version} Last{last_choice} saved Movie origins"
            );
            let summary = serde_json::json!({"version":version,"last_choice":last_choice,"exit_pc":format!("{pc:x}"),"saved_boundary":state.save_position,"route_bits":state.route.bit_position(),"controller_decisions":decisions,"movie_loads":loads,"saved_movie_checkpoints":checkpoints,"simultaneous_movie_states":movies.simultaneous});
            std::fs::write(
                directory.join(format!("last{last_choice}.desktop.json")),
                serde_json::to_vec_pretty(&summary).unwrap(),
            )
            .unwrap();
            eprintln!(
                "{version} Last{last_choice}: EXIT{pc:x}, boundary{}, route{}bits,19 real controllers,{} Movie loads,{} saved Movie checkpoint rows; native state/route and final ownership agree.",
                state.save_position,
                state.route.bit_position(),
                loads.len(),
                checkpoints.len()
            );
            summaries.push(summary);
        }
    }
    std::fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&summaries).unwrap(),
    )
    .unwrap();
}
