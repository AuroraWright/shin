//! Real entry36 filtered choices, all remaining masks and visible responses.
use std::{
    collections::BTreeMap,
    path::Path,
    process::{Command, Stdio},
};

use shin_core::{
    format::{
        save::{GameDataEntry, SaveData},
        scenario::Scenario,
        text::write_sjis_string,
    },
    vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand, runtime},
    },
};

use super::{
    ReconstructedScene, ReconstructionStop,
    native_replay_tests::{assert_audio, expected_rows, scenario_bytes},
};
use crate::adv::{StartableCommand, VmState, choice::ChoiceModel, command};

struct Checkpoint {
    entry: GameDataEntry,
    globals: SaveData,
    pc: u32,
    rows: BTreeMap<String, String>,
    audio: Vec<String>,
    choice: String,
    label: String,
}

fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn route_hex(entry: &GameDataEntry) -> String {
    let bytes: Vec<u8> =
        serde_json::from_value(serde_json::to_value(&entry.route_data).unwrap()).unwrap();
    hex(&bytes)
}

fn choice_row(choice: &runtime::SELECT) -> String {
    let mut row = format!(
        "Q {} {} {} {}",
        choice.choice_set_base,
        choice.choice_index,
        choice.choice_visibility_mask,
        choice.variants.len()
    );
    for value in std::iter::once(&choice.choice_title).chain(choice.variants.iter()) {
        let mut bytes = Vec::new();
        write_sjis_string(value, &mut bytes).unwrap();
        row.push(' ');
        row.push_str(&hex(&bytes));
    }
    row
}

fn audio_rows(state: &VmState) -> Vec<String> {
    let mut rows = vec![state.audio.bgm.map_or_else(
        || "A 0 -1".into(),
        |b| format!("A 0 {} {:08x}", b.bgm_id.into_i32(), b.volume.0.to_bits()),
    )];
    for (index, item) in state.audio.se.iter().enumerate() {
        rows.push(item.map_or_else(
            || format!("A {} -1", index + 1),
            |s| {
                format!(
                    "A {} {} {:08x} {:08x} {:08x}",
                    index + 1,
                    s.se_id.into_i32(),
                    s.volume.0.to_bits(),
                    s.pan.0.to_bits(),
                    s.play_speed.to_bits()
                )
            },
        ));
    }
    rows
}

fn globals(state: &VmState) -> SaveData {
    let mut data = SaveData::default();
    data.persist_data = state.persist.clone();
    data.save_vectors = state.save_vectors.clone();
    data
}

fn checkpoint(state: &VmState, pc: u32, choice: String, label: String) -> Checkpoint {
    Checkpoint {
        entry: state.snapshot_entry(),
        globals: globals(state),
        pc,
        rows: expected_rows(state),
        audio: audio_rows(state),
        choice,
        label,
    }
}

struct Stage {
    base: u16,
    index: u16,
    count: usize,
    initial_mask: u32,
    prefix: Vec<usize>,
    filtered: bool,
}

fn discover_stages(scenario: &Scenario) -> (Vec<Stage>, GameDataEntry, u32) {
    let mut script = Scripter::new(scenario, 36, 42);
    let mut state = VmState::for_scenario(36, 42);
    let mut result = CommandResult::None;
    let mut stages: Vec<Stage> = Vec::new();
    let mut prefix = Vec::new();
    let mut previous: Option<(u32, usize)> = None;
    for _ in 0..100_000 {
        result = match script.run(result).unwrap() {
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut state);
                get.token.finish(value)
            }
            RuntimeCommand::SELECT(choice) => {
                choice.apply_state(&mut state);
                let count = choice.variants.len();
                assert!((1..=12).contains(&count));
                let mask = choice.choice_visibility_mask as u32 & ((1 << count) - 1);
                if stages.last().is_some_and(|s| {
                    s.base == choice.choice_set_base && s.index == choice.choice_index
                }) {
                    let (old_mask, old_value) = previous.unwrap();
                    assert_eq!(mask, old_mask & !(1 << old_value));
                    stages.last_mut().unwrap().filtered = true;
                } else {
                    eprintln!(
                        "Authored stage:base{} index{} mask{mask} labels{:?}",
                        choice.choice_set_base, choice.choice_index, choice.variants
                    );
                    stages.push(Stage {
                        base: choice.choice_set_base,
                        index: choice.choice_index,
                        count,
                        initial_mask: mask,
                        prefix: prefix.clone(),
                        filtered: false,
                    });
                }
                let visible = ChoiceModel::new(count, choice.choice_visibility_mask).indices;
                let value = *visible
                    .first()
                    .expect("Authored canonical menu has no choices");
                prefix.push(value);
                previous = Some((mask, value));
                command::replay_choice_result(
                    &mut state,
                    choice.choice_set_base,
                    choice.choice_index,
                    value as i32,
                );
                choice.token.finish(value as i32)
            }
            RuntimeCommand::EXIT(_) => {
                return (stages, state.snapshot_entry(), script.position().0);
            }
            current => {
                assert!(!matches!(
                    current,
                    RuntimeCommand::RESUME(_) | RuntimeCommand::QUIZ(_)
                ));
                command::apply_command_state_ref(&current, &mut state).unwrap();
                current.execute_dummy().unwrap()
            }
        };
    }
    panic!("Canonical entry36 completion exceeds command budget")
}

fn reach_mask(
    scenario: &Scenario,
    stage: &Stage,
    mask: u32,
) -> (Scripter, VmState, runtime::SELECT) {
    let mut script = Scripter::new(scenario, 36, 42);
    let mut state = VmState::for_scenario(36, 42);
    let mut result = CommandResult::None;
    let mut consumed = 0;
    for _ in 0..100_000 {
        result = match script.run(result).unwrap() {
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut state);
                get.token.finish(value)
            }
            RuntimeCommand::SELECT(choice) => {
                choice.apply_state(&mut state);
                let current =
                    choice.choice_visibility_mask as u32 & ((1 << choice.variants.len()) - 1);
                let value = if consumed < stage.prefix.len() {
                    let value = stage.prefix[consumed];
                    consumed += 1;
                    value
                } else {
                    assert_eq!(
                        (choice.choice_set_base, choice.choice_index),
                        (stage.base, stage.index)
                    );
                    assert_eq!(choice.variants.len(), stage.count);
                    assert_eq!(current & mask, mask, "Requested mask no longer reachable");
                    if current == mask {
                        return (script, state, choice);
                    }
                    assert!(stage.filtered);
                    (current & !mask).trailing_zeros() as usize
                };
                assert!(
                    ChoiceModel::new(choice.variants.len(), choice.choice_visibility_mask)
                        .indices
                        .contains(&value)
                );
                command::replay_choice_result(
                    &mut state,
                    choice.choice_set_base,
                    choice.choice_index,
                    value as i32,
                );
                choice.token.finish(value as i32)
            }
            current => {
                assert!(!matches!(
                    current,
                    RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_) | RuntimeCommand::QUIZ(_)
                ));
                command::apply_command_state_ref(&current, &mut state).unwrap();
                current.execute_dummy().unwrap()
            }
        };
    }
    panic!("Filtered mask {mask} unreachable within command budget")
}

#[test]
#[ignore = "requires actual original/1.03 executables/ROMs;native filtered SELECT,CPU only"]
fn reference_native_filtered_select_completion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = root.join("umineko_switch_files");
    let out = std::env::var_os("SHIN_FILTERED_SELECT_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("shin-native-filtered-select"));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("native-filtered-select-replay");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-save-filtered-select-replay.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    for (version, exe, rom) in [
        ("original", "uncompressed_executable", "data.rom"),
        ("1.03", "uncompressed_executable_update", "patch.rom"),
    ] {
        let dir = out.join(version);
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = scenario_bytes(&files.join(rom));
        let snr = dir.join("main.snr");
        std::fs::write(&snr, &bytes).unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut cases = Vec::new();
        let mut scans = Vec::new();
        let mut scan_input = String::new();
        let mut masks = BTreeMap::new();
        let mut completions = 0;
        let (stages, canonical_end, canonical_pc) = discover_stages(&scenario);
        let stage_json: Vec<_> = stages
            .iter()
            .map(|s| {
                serde_json::json!({
            "base":s.base,"index":s.index,"count":s.count,"initial_mask":s.initial_mask,
            "prefix":s.prefix,"filtered":s.filtered})
            })
            .collect();
        std::fs::write(
            dir.join("stages.json"),
            serde_json::to_vec_pretty(&stage_json).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("canonical-scan-input.txt"),
            format!(
                "36 {} 2a {}\n",
                canonical_end.save_position + 1,
                route_hex(&canonical_end)
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("canonical-end.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"boundary":canonical_end.save_position,"pc":canonical_pc}),
            )
            .unwrap(),
        )
        .unwrap();
        let mut expected_masks = 0;
        let mut expected_responses = 0;
        for (stage_id, stage) in stages.iter().enumerate() {
            let targets: Vec<u32> = if stage.filtered {
                (1..=stage.initial_mask)
                    .filter(|m| m & stage.initial_mask == *m)
                    .collect()
            } else {
                vec![stage.initial_mask]
            };
            for mask in targets {
                // Root first-pass mask127 already passed all visible responses.
                if stage_id == 0 && mask == 127 {
                    continue;
                }
                expected_masks += 1;
                expected_responses += mask.count_ones() as usize;
                let (script, state, choice) = reach_mask(&scenario, stage, mask);
                let visible =
                    ChoiceModel::new(choice.variants.len(), choice.choice_visibility_mask).indices;
                assert_eq!(
                    visible,
                    (0..stage.count)
                        .filter(|i| mask & (1 << i) != 0)
                        .collect::<Vec<_>>()
                );
                let expected_choice = choice_row(&choice);
                let before = checkpoint(
                    &state,
                    script.position().0,
                    expected_choice.clone(),
                    format!("stage{stage_id}-mask{mask}-entry"),
                );
                masks.insert(
                    format!("{stage_id}:{mask}"),
                    (state.save_position, expected_choice.clone()),
                );
                let entry = before.entry.clone();
                let data = before.globals.clone();
                cases.push(before);
                for value in visible {
                    let mut branch =
                        ReconstructedScene::from_save(&scenario, &entry, &data, 100_000).unwrap();
                    assert_eq!(expected_rows(&branch.state), expected_rows(&state));
                    assert_eq!(audio_rows(&branch.state), audio_rows(&state));
                    let ReconstructionStop::Command(RuntimeCommand::SELECT(active)) = branch.stop
                    else {
                        panic!("Later filtered menu must restore SELECT:mask{mask}");
                    };
                    assert_eq!(
                        choice_row(&active),
                        expected_choice,
                        "mask{mask}:restored menu"
                    );
                    command::replay_choice_result(
                        &mut branch.state,
                        active.choice_set_base,
                        active.choice_index,
                        value as i32,
                    );
                    let response = checkpoint(
                        &branch.state,
                        branch.scripter.position().0,
                        expected_choice.clone(),
                        format!("stage{stage_id}-mask{mask}-response{value}"),
                    );
                    let mut last = checkpoint(
                        &branch.state,
                        branch.scripter.position().0,
                        expected_choice.clone(),
                        format!("stage{stage_id}-mask{mask}-end{value}"),
                    );
                    cases.push(response);
                    let mut result = active.token.finish(value as i32);
                    let mut stopped = false;
                    for _ in 0..100_000 {
                        let next = branch.scripter.run(result).unwrap();
                        let pc = branch.scripter.position().0;
                        if matches!(next, RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_)) {
                            if stage.filtered {
                                assert_eq!(
                                    mask.count_ones(),
                                    1,
                                    "Premature filtered-menu completion"
                                );
                            }
                            assert!(
                                matches!(next, RuntimeCommand::EXIT(_)),
                                "Unexpected scene replacement"
                            );
                            completions += 1;
                            let end = branch.state.snapshot_entry();
                            scan_input.push_str(&format!(
                                "36 {} 2a {}\n",
                                end.save_position + 1,
                                route_hex(&end)
                            ));
                            scans.push(
                                serde_json::json!({"stage":stage_id,"mask":mask,"response":value,
                            "boundary":end.save_position,"pc":pc,"terminal":true}),
                            );
                            stopped = true;
                            break;
                        }
                        if let RuntimeCommand::SGET(get) = next {
                            let value = get.apply_state(&mut branch.state);
                            result = get.token.finish(value);
                            continue;
                        }
                        command::apply_command_state_ref(&next, &mut branch.state).unwrap();
                        let descriptor = match &next {
                            RuntimeCommand::SELECT(next_choice) => choice_row(next_choice),
                            _ => "Q -".into(),
                        };
                        if branch.state.save_position > last.entry.save_position {
                            last = checkpoint(
                                &branch.state,
                                pc,
                                descriptor,
                                format!("stage{stage_id}-mask{mask}-end{value}"),
                            );
                            if branch.state.layers.layerbanks.values().any(|l| {
                                l.layer_type
                                    == Some(shin_core::vm::command::types::LayerType::Movie)
                            }) {
                                cases.push(checkpoint(
                                    &branch.state,
                                    pc,
                                    "Q -".into(),
                                    format!("stage{stage_id}-mask{mask}-movie{value}"),
                                ));
                            }
                        }
                        if let RuntimeCommand::SELECT(next_choice) = next {
                            let remaining = mask & !(1 << value);
                            if stage.filtered && remaining != 0 {
                                assert_eq!(
                                    (next_choice.choice_set_base, next_choice.choice_index),
                                    (stage.base, stage.index)
                                );
                                assert_eq!(
                                    next_choice.choice_visibility_mask as u32
                                        & ((1 << stage.count) - 1),
                                    remaining
                                );
                            } else {
                                assert_ne!(
                                    (next_choice.choice_set_base, next_choice.choice_index),
                                    (stage.base, stage.index)
                                );
                            }
                            assert_eq!(branch.state.save_position, last.entry.save_position);
                            assert_eq!(pc, last.pc, "Menu shares an earlier message boundary");
                            let end = branch.state.snapshot_entry();
                            scan_input.push_str(&format!(
                                "36 {} 2a {}\n",
                                end.save_position,
                                route_hex(&end)
                            ));
                            scans.push(
                                serde_json::json!({"stage":stage_id,"mask":mask,"response":value,
                            "boundary":end.save_position,"pc":pc,"terminal":false}),
                            );
                            stopped = true;
                            break;
                        }
                        assert!(!matches!(next, RuntimeCommand::QUIZ(_)));
                        result = next.execute_dummy().unwrap();
                    }
                    assert!(stopped, "mask{mask}:branch command budget exhausted");
                    cases.push(last);
                }
            }
        }
        assert_eq!(masks.len(), expected_masks);
        assert_eq!(scans.len(), expected_responses);
        assert!(completions > 0);
        std::fs::write(
            dir.join("masks.json"),
            serde_json::to_vec_pretty(&masks).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("scans.json"),
            serde_json::to_vec_pretty(&scans).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("scan-input.txt"), scan_input).unwrap();
        let payload: String = cases
            .iter()
            .map(|c| format!("36 2a {} {}\n", c.entry.save_position, route_hex(&c.entry)))
            .collect();
        let input = dir.join("input.txt");
        std::fs::write(&input, payload).unwrap();
        let native = Command::new(&helper)
            .arg(files.join(exe))
            .arg(&snr)
            .stdin(Stdio::from(std::fs::File::open(&input).unwrap()))
            .output()
            .unwrap();
        std::fs::write(dir.join("native.txt"), &native.stdout).unwrap();
        std::fs::write(dir.join("native.stderr"), &native.stderr).unwrap();
        assert!(
            native.status.success(),
            "{version}:{}",
            String::from_utf8_lossy(&native.stderr)
        );
        let text = String::from_utf8(native.stdout).unwrap();
        let mut lines = text.lines();
        let mut scene_rows = 0;
        let mut descriptors = 0;
        for case in &cases {
            let label = format!(
                "{version}:{} boundary{}",
                case.label, case.entry.save_position
            );
            let scene =
                ReconstructedScene::from_save(&scenario, &case.entry, &case.globals, 100_000)
                    .unwrap();
            assert_eq!(
                expected_rows(&scene.state),
                case.rows,
                "{label}:continued state"
            );
            assert_eq!(
                audio_rows(&scene.state),
                case.audio,
                "{label}:continued audio"
            );
            assert_eq!(scene.scripter.position().0, case.pc, "{label}:continued PC");
            let header: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(header[0], "V");
            assert_eq!(header[2].parse::<u32>().unwrap(), case.entry.save_position);
            assert_eq!(header[7].parse::<u32>().unwrap(), scene.state.save_position);
            assert_eq!(
                header[8].parse::<usize>().unwrap(),
                scene.state.route.bit_position(),
                "{label}:route position"
            );
            assert_eq!(
                u32::from_str_radix(header[9], 16).unwrap(),
                case.pc,
                "{label}:native PC"
            );
            let mut expected = case.rows.clone();
            let mut audio = 0;
            let mut context = false;
            let mut choice = false;
            loop {
                let row = lines.next().unwrap();
                if row.starts_with("D ") {
                    break;
                }
                if row.starts_with("Q ") {
                    assert_eq!(row, case.choice, "{label}:native decoded choice");
                    descriptors += usize::from(row != "Q -");
                    choice = true;
                } else if row.starts_with("C ") {
                    let fields: Vec<u32> = row
                        .split_whitespace()
                        .skip(1)
                        .map(|s| s.parse().unwrap())
                        .collect();
                    assert_eq!(
                        fields[..4],
                        [0, 36, 42, case.entry.save_position],
                        "{label}:context"
                    );
                    context = true;
                } else if row.starts_with("A ") {
                    assert_audio(row, &scene.state, &label);
                    audio += 1;
                } else {
                    let key = if row.starts_with("S ") {
                        "S".into()
                    } else {
                        row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                    };
                    assert_eq!(row, expected.remove(&key).unwrap(), "{label}");
                    scene_rows += 1;
                }
            }
            assert!(expected.is_empty() && context && choice);
            assert_eq!(audio, 33);
        }
        assert!(lines.next().is_none());
        eprintln!(
            "{version}:{} saved cases,{scene_rows} native scene rows,{} audio descriptors,{descriptors} native SELECT descriptors;{} reachable masks,{} visible responses,{} completions;PC/route/state/context0/native ownership pass",
            cases.len(),
            cases.len() * 33,
            masks.len(),
            scans.len(),
            completions
        );
    }
    eprintln!("Evidence:{}", out.display());
}
