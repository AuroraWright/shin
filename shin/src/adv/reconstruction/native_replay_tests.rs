use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use enum_map::Enum;
use shin_core::{
    format::{
        rom::RomReader,
        save::{GameDataEntry, RouteRecorder, SaveData},
        scenario::Scenario,
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::types::LayerProperty,
};

use super::ReconstructedScene;
use crate::{adv::VmState, layer::LayerPropertiesState};

pub(super) fn scenario_bytes(path: &Path) -> Vec<u8> {
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
#[ignore = "requires supplied executable/ROMs; native saved replay and actual mask hydration, no hardware"]
fn reference_native_saved_mask_hydration() {
    use binrw::BinWrite;
    use shin_core::format::scenario::{
        instruction_elements::{MessageId, NumberSpec, U8Bool},
        instructions::Instruction,
        types::U16FixupString,
    };
    use shin_core::vm::command::{CompiletimeCommand as C, compiletime as c};
    fn n<T>(value: i32) -> NumberSpec<T> {
        NumberSpec::new(NumberSpec::constant(value).into_untyped())
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let data = Path::new(&std::env::var("SHIN_REFERENCE_ROM").unwrap()).to_path_buf();
    let files = data.parent().unwrap();
    let out = std::env::temp_dir().join(format!(
        "shin-native-saved-mask-hydration-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("hydrate");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-save-mask-hydration.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&data).unwrap()).unwrap()).unwrap();
    for (version, executable, source) in [
        ("original", "uncompressed_executable", data.clone()),
        (
            "1.03",
            "uncompressed_executable_update",
            files.join("patch.rom"),
        ),
    ] {
        let mut bytes = scenario_bytes(&source);
        let original = Scenario::new(bytes.clone().into()).unwrap();
        let table = &original.info_tables().mask_info;
        assert!(table.len() >= 4);
        let ids = [0, table.len() / 3, table.len() * 2 / 3, table.len() - 1];
        let metadata = out.join(format!("{version}-metadata.txt"));
        std::fs::write(
            &metadata,
            format!(
                "{}\n{}\n",
                table.len(),
                table
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )
        .unwrap();
        let offset = bytes.len();
        bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(offset as u64);
        // Preserve actual metadata and isolate only a four-plane saved script.
        // Each message provides one native saved boundary after MASKLOAD.
        for (plane, id) in ids.into_iter().enumerate() {
            for command in [
                C::PLANESELECT(c::PLANESELECT {
                    plane_id: n(plane as i32),
                }),
                C::MASKLOAD(c::MASKLOAD {
                    mask_id: n(id as i32),
                    mask_flags: n((plane + 1) as i32),
                    smth_smth_transition: n(1),
                }),
                C::MSGSET(c::MSGSET {
                    msg_id: MessageId(plane as u32 + 1),
                    auto_wait: U8Bool(true),
                    text: U16FixupString::new("Saved mask boundary"),
                }),
            ] {
                Instruction::Command(command).write_le(&mut cursor).unwrap();
            }
        }
        Instruction::Command(C::EXIT(c::EXIT {
            arg1: 0,
            arg2: n(0),
        }))
        .write_le(&mut cursor)
        .unwrap();
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let snr = out.join(format!("{version}.snr"));
        std::fs::write(&snr, &bytes).unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let cases = (0..=4)
            .flat_map(|boundary| {
                [0, 5, 6]
                    .into_iter()
                    .map(move |profile| (boundary, profile))
            })
            .chain((1..=4).map(|profile| (4, profile)))
            .chain((7..=10).map(|profile| (4, profile)))
            .collect::<Vec<_>>();
        let input = cases
            .iter()
            .map(|(b, p)| format!("{b} {p}\n"))
            .collect::<String>();
        let dir = out.join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), &input).unwrap();
        let mut child = Command::new(&helper)
            .arg(files.join(executable))
            .arg(&data)
            .arg(&snr)
            .arg(&metadata)
            .arg(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let native = child.wait_with_output().unwrap();
        std::fs::write(dir.join("trace.txt"), &native.stdout).unwrap();
        std::fs::write(dir.join("stderr.txt"), &native.stderr).unwrap();
        assert!(
            native.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        let text = String::from_utf8(native.stdout).unwrap();
        let mut rows = text.lines();
        for (case, (boundary, profile)) in cases.into_iter().enumerate() {
            let scene = ReconstructedScene::from_save(
                &scenario,
                &GameDataEntry {
                    scenario_id: 0,
                    random_seed: 0,
                    save_position: boundary,
                    route_data: RouteRecorder::default().snapshot(),
                },
                &SaveData::default(),
                100,
            )
            .unwrap();
            let expected_loads = if profile == 5 {
                0
            } else if profile >= 7 {
                profile - 6
            } else if (1..=4).contains(&profile) {
                profile
            } else {
                boundary
            };
            let mut loaded = 0;
            for plane in 0..4 {
                let row = rows.next().unwrap().split_whitespace().collect::<Vec<_>>();
                assert_eq!(row[..3], ["P", &case.to_string(), &plane.to_string()]);
                if profile != 5 {
                    let group = &scene.state.layers.plane_layergroups
                        [shin_core::vm::command::types::PlaneId::new(plane as u8)];
                    assert_eq!(
                        row[3].parse::<i32>().unwrap(),
                        group.mask_id.into_i32(),
                        "{version} case{case} plane{plane}"
                    );
                    assert_eq!(row[4].parse::<i32>().unwrap(), group.mask_flags.bits());
                }
                let present = plane < expected_loads as usize;
                assert_eq!(row[5], if present { "1" } else { "0" });
                if present {
                    let row = rows.next().unwrap().split_whitespace().collect::<Vec<_>>();
                    assert_eq!(row[0], "M");
                    assert_eq!(row[1].parse::<usize>().unwrap(), case);
                    assert_eq!(row[2].parse::<usize>().unwrap(), loaded);
                    let path = table[ids[plane]].path();
                    assert_eq!(row[3], path);
                    let entry = rom.find_file(&path).unwrap();
                    let mut bytes = Vec::new();
                    rom.open_file(entry)
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    let mask = shin_core::format::mask::read_mask(&bytes).unwrap();
                    assert_eq!(row[4].parse::<u32>().unwrap(), mask.texels.width());
                    assert_eq!(row[5].parse::<u32>().unwrap(), mask.texels.height());
                    assert_eq!(
                        std::fs::read(dir.join(format!("{}-pixels.bin", case * 4 + loaded)))
                            .unwrap(),
                        *mask.texels.as_raw()
                    );
                    loaded += 1;
                }
            }
            let row = rows.next().unwrap().split_whitespace().collect::<Vec<_>>();
            assert_eq!(row[0], "W");
            assert_eq!(row[1].parse::<usize>().unwrap(), case);
            assert_eq!(row[2].parse::<u32>().unwrap(), profile);
            assert_eq!(row[3].parse::<u32>().unwrap(), boundary);
            assert_eq!(
                row[4],
                if profile == 0 || profile == 6 {
                    "1"
                } else {
                    "0"
                }
            );
            assert_eq!(
                row[5].parse::<u32>().unwrap(),
                if profile == 5 { 0 } else { boundary }
            );
            assert_eq!(row[6].parse::<usize>().unwrap(), loaded);
            assert_eq!(
                row[7],
                if profile == 0 || profile == 6 {
                    "0"
                } else {
                    "1"
                }
            );
            let row = rows.next().unwrap().split_whitespace().collect::<Vec<_>>();
            assert_eq!(row[0], "D");
            assert_eq!(row[1].parse::<usize>().unwrap(), case);
        }
        assert!(rows.next().is_none());
        eprintln!(
            "{version}: 23 saved replay/mask hydration cases pass, including four injected and four threaded native cancellation boundaries, plane retention and cleanup"
        );
    }
    eprintln!("Evidence: {}", out.display());
}

fn property_row(id: usize, properties: &LayerPropertiesState) -> String {
    let values = (0..90)
        .map(|i| {
            properties
                .get_property(LayerProperty::from_usize(i))
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("P {id} {values}")
}

pub(super) fn expected_rows(state: &VmState) -> BTreeMap<String, String> {
    let mut rows = Vec::new();
    rows.push(property_row(0, &state.layers.root_layer_group));
    rows.push(property_row(1, &state.layers.screen_layer));
    rows.push(property_row(2, &state.layers.page_layer));
    for (plane, group) in &state.layers.plane_layergroups {
        let id = plane.raw() as usize;
        rows.push(property_row(3 + id, &group.properties));
        rows.push(format!(
            "M {id} {} {}",
            group.mask_id.into_i32(),
            group.mask_flags.bits()
        ));
    }
    for (bank, layer) in &state.layers.layerbanks {
        let id = bank.raw() as usize;
        let mut row = format!("L {id} {}", layer.layer_type.map_or(-1, |ty| ty as i32));
        if layer.layer_type.is_some() {
            row.push_str(&format!(
                " {} {} {}",
                layer.plane.raw(),
                layer.layer_id.raw(),
                layer.layer_load_counter
            ));
            let p = layer.params;
            for value in [p.0, p.1, p.2, p.3, p.4, p.5, p.6, p.7] {
                row.push_str(&format!(" {value}"));
            }
            // Native e680 leaves inactive bank properties uninitialized. First
            // LAYERLOAD resets all 90 values even with KEEP_PREVIOUS_PROPERTIES
            // (8adc/8b24); hydration skips negative bank types (1b43c).
            rows.push(property_row(7 + id, &layer.properties));
        }
        rows.push(row);
    }
    rows.push(format!(
        "S {} {} {} {} {} {}",
        u8::from(state.system_controls_allowed),
        state.current_event.unwrap_or(-1),
        u8::from(state.messagebox_state.close_inhibited),
        (state.messagebox_state.msginit.messagebox_type as i32)
            | ((state.messagebox_state.msginit.text_layout as i32) << 4),
        u8::from(state.messagebox_state.text.is_some()),
        state.resume_position.unwrap_or(0)
    ));
    rows.into_iter()
        .map(|row| {
            let key = if row.starts_with("S ") {
                "S".into()
            } else {
                row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
            };
            (key, row)
        })
        .collect()
}

pub(super) fn assert_audio(row: &str, state: &VmState, label: &str) {
    let mut fields = row.split_whitespace().skip(1);
    let slot: usize = fields.next().unwrap().parse().unwrap();
    let id: i32 = fields.next().unwrap().parse().unwrap();
    let expected = if slot == 0 {
        state
            .audio
            .bgm
            .map(|bgm| (bgm.bgm_id.into_i32(), vec![bgm.volume.0]))
    } else {
        state.audio.se[slot - 1].map(|se| {
            (
                se.se_id.into_i32(),
                vec![se.volume.0, se.pan.0, se.play_speed],
            )
        })
    };
    assert_eq!(
        id,
        expected.as_ref().map_or(-1, |value| value.0),
        "{label}: {row}"
    );
    if let Some((_, values)) = expected {
        for value in values {
            let native = f32::from_bits(u32::from_str_radix(fields.next().unwrap(), 16).unwrap());
            assert!(
                (native - value).abs() <= 1e-6,
                "{label}: {row}: {native} != {value}"
            );
        }
    }
    assert!(fields.next().is_none(), "{label}: {row}");
}

#[test]
#[ignore = "requires supplied original/1.03 executables and ROMs; executes native saved replay, no hardware"]
fn reference_native_authored_saved_vm_replay() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let data = Path::new(&std::env::var("SHIN_REFERENCE_ROM").unwrap()).to_path_buf();
    let directory = data.parent().unwrap();
    let out = std::env::temp_dir().join(format!("shin-native-save-replay-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("replay");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-save-replay.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut route = RouteRecorder::default();
    for _ in 0..512 {
        route.record_persistent(0);
    }
    // Short-form zero is 10 in the MSB-first native route bitstream.
    let route_hex = "aa".repeat(128);
    let cases = [0, 7]
        .into_iter()
        .flat_map(|entry| {
            [0, 42, u32::MAX].into_iter().flat_map(move |seed| {
                [0, 1, 2, 3, 4, 10, 50, 100]
                    .into_iter()
                    .map(move |boundary| (entry, seed, boundary))
            })
        })
        .collect::<Vec<_>>();
    let payload = cases
        .iter()
        .map(|(entry, seed, boundary)| format!("{entry} {seed:08x} {boundary} {route_hex}\n"))
        .collect::<String>();
    let mut differences = Vec::new();
    for (version, executable, rom) in [
        ("original", "uncompressed_executable", data.clone()),
        (
            "1.03",
            "uncompressed_executable_update",
            directory.join("patch.rom"),
        ),
    ] {
        let bytes = scenario_bytes(&rom);
        let snr = out.join(format!("{version}.snr"));
        std::fs::write(&snr, &bytes).unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut child = Command::new(&helper)
            .arg(directory.join(executable))
            .arg(&snr)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = payload.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let native = child.wait_with_output().unwrap();
        writer.join().unwrap().unwrap();
        std::fs::write(out.join(format!("{version}.stderr")), &native.stderr).unwrap();
        std::fs::write(out.join(format!("{version}.txt")), &native.stdout).unwrap();
        assert!(
            native.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        let output = String::from_utf8(native.stdout).unwrap();
        let mut lines = output.lines();
        for &(entry, seed, boundary) in &cases {
            let label = format!("{version} entry{entry} seed{seed:x} boundary{boundary}");
            let scene = ReconstructedScene::from_save(
                &scenario,
                &GameDataEntry {
                    scenario_id: entry,
                    random_seed: seed,
                    save_position: boundary,
                    route_data: route.snapshot(),
                },
                &SaveData::default(),
                1_000_000,
            )
            .unwrap();
            let header = lines.next().unwrap().split_whitespace().collect::<Vec<_>>();
            assert_eq!(header[0], "V", "{label}");
            assert_eq!(header[1].parse::<i32>().unwrap(), entry);
            assert_eq!(header[2].parse::<u32>().unwrap(), boundary);
            assert_eq!(
                header[7].parse::<u32>().unwrap(),
                scene.state.save_position,
                "{label}"
            );
            assert_eq!(
                header[8].parse::<usize>().unwrap(),
                scene.state.route.bit_position(),
                "{label}"
            );
            if u32::from_str_radix(header[9], 16).unwrap() != scene.scripter.position().0 {
                differences.push(format!(
                    "{label}: command position native={} port={:x}",
                    header[9],
                    scene.scripter.position().0
                ));
            }
            let mut expected = expected_rows(&scene.state);
            let mut audio_rows = 0;
            loop {
                let row = lines.next().unwrap();
                if row.starts_with("D ") {
                    let fields = row.split_whitespace().collect::<Vec<_>>();
                    assert_eq!(fields.len(), 4, "{label}: {row}");
                    assert_eq!(fields[1].parse::<i32>().unwrap(), entry, "{label}");
                    assert_eq!(fields[2].parse::<u32>().unwrap(), boundary, "{label}");
                    // The native allocation ledger checks full cleanup. Only
                    // checkpoint-producing commands (e.g. AUTOSAVE), rather
                    // than every saved boundary, allocate checkpoint owners.
                    let releases = fields[3].parse::<u32>().unwrap();
                    assert!(
                        releases <= header[3].parse::<u32>().unwrap(),
                        "{label}: {row}"
                    );
                    break;
                }
                if row.starts_with("A ") {
                    assert_audio(row, &scene.state, &label);
                    audio_rows += 1;
                    continue;
                }
                let key = if row.starts_with("S ") {
                    "S".into()
                } else {
                    row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                };
                let port = expected.remove(&key).unwrap();
                if row != port {
                    differences.push(format!("{label}: {key}\nnative: {row}\nport:   {port}"));
                }
            }
            assert!(expected.is_empty(), "{label}: missing rows {expected:?}");
            assert_eq!(audio_rows, 33, "{label}");
        }
        assert!(lines.next().is_none());
        eprintln!(
            "{version}: {} authored saved replays,seven container/active-bank property sets,48 layers,four masks,33 audio descriptors and input/message/RESUME gates; native VM/checkpoint ownership balanced",
            cases.len()
        );
    }
    eprintln!("Evidence: {}", out.display());
    assert!(
        differences.is_empty(),
        "{} replay state differences:\n{}",
        differences.len(),
        differences
            .iter()
            .take(50)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "requires supplied executables/ROMs; authored SELECT replay, CPU only"]
fn reference_native_authored_select_saved_replay() {
    use super::ReconstructionStop;
    use crate::adv::{StartableCommand, choice::ChoiceModel, command};
    use shin_core::vm::{
        Scripter,
        command::{CommandResult, RuntimeCommand},
    };

    fn route_hex(entry: &GameDataEntry) -> String {
        let bytes: Vec<u8> =
            serde_json::from_value(serde_json::to_value(&entry.route_data).unwrap()).unwrap();
        if bytes.is_empty() {
            return "-".into();
        }
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = root.join("game-data/umineko");
    let out = std::env::var_os("SHIN_SELECT_REPLAY_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("shin-native-select-replay"));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("native-select-replay");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-save-select-replay.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    for (version, executable, rom) in [
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
        let mut authored = String::new();
        for entry_id in [36, 46] {
            for seed in [0, 42, u32::MAX] {
                let mut script = Scripter::new(&scenario, entry_id, seed);
                let mut state = VmState::for_scenario(entry_id, seed);
                let mut result = CommandResult::None;
                let mut found = false;
                let mut checkpoint = (
                    state.snapshot_entry(),
                    expected_rows(&state),
                    script.position().0,
                );
                for _ in 0..100_000 {
                    match script.run(result).unwrap() {
                        RuntimeCommand::SGET(get) => {
                            let value = get.apply_state(&mut state);
                            result = get.token.finish(value);
                        }
                        RuntimeCommand::SELECT(select) => {
                            select.apply_state(&mut state);
                            let before = state.snapshot_entry();
                            if before.save_position > checkpoint.0.save_position {
                                checkpoint =
                                    (before.clone(), expected_rows(&state), script.position().0);
                            }
                            let displayed = ChoiceModel::new(
                                select.variants.len(),
                                select.choice_visibility_mask,
                            )
                            .indices;
                            assert!(!displayed.is_empty());
                            authored.push_str(&format!(
                                "entry{entry_id} seed{seed:x} SELECT pc{:x} boundary{} variants{:?} visible{:?}\n",
                                script.position().0, before.save_position, select.variants, displayed));
                            eprintln!(
                                "{version}: entry{entry_id} SELECT boundary{} pc{:x} variants{:?}",
                                before.save_position,
                                script.position().0,
                                select.variants
                            );
                            cases.push((
                                checkpoint.0.clone(),
                                checkpoint.1.clone(),
                                checkpoint.2,
                                "choice-entry".to_owned(),
                            ));
                            for value in displayed {
                                // Resume the real choice entry, then record the same
                                // response as live SELECT. No script bytes changed.
                                let mut branch = ReconstructedScene::from_save(
                                    &scenario,
                                    &before,
                                    &SaveData::default(),
                                    100_000,
                                )
                                .unwrap();
                                branch.state.persistent_commits_enabled = true;
                                // SELECT while message text is active reuses the
                                // preceding message boundary. Loading resumes that
                                // message; advance it to reach the live choice.
                                let mut stop = branch.stop;
                                let mut commands = 0;
                                let choice = loop {
                                    let response = match stop {
                                        ReconstructionStop::Command(RuntimeCommand::SELECT(
                                            choice,
                                        )) => break choice,
                                        ReconstructionStop::Command(command) => {
                                            command.execute_dummy().unwrap()
                                        }
                                        ReconstructionStop::Response(result) => result,
                                    };
                                    let current = branch.scripter.run(response).unwrap();
                                    command::apply_command_state_ref(&current, &mut branch.state)
                                        .unwrap();
                                    stop = ReconstructionStop::Command(current);
                                    commands += 1;
                                    assert!(
                                        commands < 1000,
                                        "Choice unreachable from preceding boundary"
                                    );
                                };
                                assert_eq!(expected_rows(&branch.state), expected_rows(&state));
                                assert_eq!(branch.scripter.position().0, script.position().0);
                                assert_eq!(choice.variants, select.variants);
                                command::replay_choice_result(
                                    &mut branch.state,
                                    choice.choice_set_base,
                                    choice.choice_index,
                                    value as i32,
                                );
                                cases.push((
                                    branch.state.snapshot_entry(),
                                    expected_rows(&branch.state),
                                    branch.scripter.position().0,
                                    format!("response{value}"),
                                ));
                                let mut branch_result = choice.token.finish(value as i32);
                                let mut next_boundary = false;
                                let response_position = branch.state.save_position;
                                let mut last_movie_boundary = response_position;
                                let mut stopped = false;
                                let mut branch_checkpoint = (
                                    branch.state.snapshot_entry(),
                                    expected_rows(&branch.state),
                                    branch.scripter.position().0,
                                );
                                for _ in 0..100_000 {
                                    let next = branch.scripter.run(branch_result).unwrap();
                                    if matches!(
                                        next,
                                        RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_)
                                    ) {
                                        authored.push_str(&format!("entry{entry_id} seed{seed:x} response{value} stops {:?} pc{:x} boundary{}\n", next, branch.scripter.position().0, branch.state.save_position));
                                        stopped = true;
                                        break;
                                    }
                                    if let RuntimeCommand::SGET(get) = next {
                                        let value = get.apply_state(&mut branch.state);
                                        branch_result = get.token.finish(value);
                                        continue;
                                    }
                                    command::apply_command_state_ref(&next, &mut branch.state)
                                        .unwrap();
                                    if branch.state.save_position
                                        > branch_checkpoint.0.save_position
                                    {
                                        branch_checkpoint = (
                                            branch.state.snapshot_entry(),
                                            expected_rows(&branch.state),
                                            branch.scripter.position().0,
                                        );
                                    }
                                    if branch.state.save_position > response_position
                                        && !next_boundary
                                    {
                                        cases.push((
                                            branch.state.snapshot_entry(),
                                            expected_rows(&branch.state),
                                            branch.scripter.position().0,
                                            format!("after-response{value}"),
                                        ));
                                        next_boundary = true;
                                    }
                                    if matches!(
                                        next,
                                        RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_)
                                    ) {
                                        authored.push_str(&format!("entry{entry_id} seed{seed:x} response{value} next interactive {:?} pc{:x} boundary{}\n", next, branch.scripter.position().0, branch.state.save_position));
                                        stopped = true;
                                        break;
                                    }
                                    if branch.state.save_position > last_movie_boundary {
                                        last_movie_boundary = branch.state.save_position;
                                        if branch.state.layers.layerbanks.values().any(|bank| {
                                            bank.layer_type
                                                == Some(
                                                    shin_core::vm::command::types::LayerType::Movie,
                                                )
                                        }) {
                                            authored.push_str(&format!("entry{entry_id} seed{seed:x} response{value} Movie checkpoint{} pc{:x}\n", branch.state.save_position, branch.scripter.position().0));
                                            cases.push((
                                                branch.state.snapshot_entry(),
                                                expected_rows(&branch.state),
                                                branch.scripter.position().0,
                                                format!("movie-response{value}"),
                                            ));
                                        }
                                    }
                                    branch_result = next.execute_dummy().unwrap();
                                }
                                assert!(
                                    next_boundary,
                                    "entry{entry_id} response{value}: next saved boundary"
                                );
                                assert!(
                                    stopped,
                                    "entry{entry_id} response{value}: branch command budget exceeded"
                                );
                                cases.push((
                                    branch_checkpoint.0,
                                    branch_checkpoint.1,
                                    branch_checkpoint.2,
                                    format!("branch-end-response{value}"),
                                ));
                            }
                            found = true;
                            break;
                        }
                        current => {
                            assert!(!matches!(
                                current,
                                RuntimeCommand::EXIT(_)
                                    | RuntimeCommand::RESUME(_)
                                    | RuntimeCommand::QUIZ(_)
                            ));
                            command::apply_command_state_ref(&current, &mut state).unwrap();
                            if state.save_position > checkpoint.0.save_position {
                                checkpoint = (
                                    state.snapshot_entry(),
                                    expected_rows(&state),
                                    script.position().0,
                                );
                            }
                            result = current.execute_dummy().unwrap();
                        }
                    }
                }
                assert!(found, "entry{entry_id}: no SELECT");
            }
        }
        std::fs::write(dir.join("authored.txt"), authored).unwrap();
        let payload: String = cases
            .iter()
            .map(|(entry, _, _, _)| {
                format!(
                    "{} {:08x} {} {}\n",
                    entry.scenario_id,
                    entry.random_seed,
                    entry.save_position,
                    route_hex(entry)
                )
            })
            .collect();
        let input = dir.join("input.txt");
        std::fs::write(&input, payload).unwrap();
        let native = Command::new(&helper)
            .arg(files.join(executable))
            .arg(&snr)
            .stdin(Stdio::from(std::fs::File::open(&input).unwrap()))
            .output()
            .unwrap();
        std::fs::write(dir.join("native.txt"), &native.stdout).unwrap();
        std::fs::write(dir.join("native.stderr"), &native.stderr).unwrap();
        assert!(
            native.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        let text = String::from_utf8(native.stdout).unwrap();
        let mut lines = text.lines();
        let mut total_rows = 0;
        for (entry, live_rows, live_pc, phase) in &cases {
            let label = format!(
                "{version} entry{} seed{:x} {phase} boundary{}",
                entry.scenario_id, entry.random_seed, entry.save_position
            );
            let scene =
                ReconstructedScene::from_save(&scenario, entry, &SaveData::default(), 100_000)
                    .unwrap();
            assert_eq!(
                &expected_rows(&scene.state),
                live_rows,
                "{label}: uninterrupted state"
            );
            assert_eq!(
                scene.scripter.position().0,
                *live_pc,
                "{label}: uninterrupted PC"
            );
            let header: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(
                &header[..3],
                &[
                    "V",
                    &entry.scenario_id.to_string(),
                    &entry.save_position.to_string()
                ]
            );
            assert_eq!(
                u32::from_str_radix(header[9], 16).unwrap(),
                *live_pc,
                "{label}: native PC"
            );
            assert_eq!(
                header[7].parse::<u32>().unwrap(),
                scene.state.save_position,
                "{label}"
            );
            assert_eq!(
                header[8].parse::<usize>().unwrap(),
                scene.state.route.bit_position(),
                "{label}: route position"
            );
            let mut expected = live_rows.clone();
            let mut audio_rows = 0;
            let mut context = false;
            loop {
                let row = lines.next().unwrap();
                if row.starts_with("D ") {
                    break;
                }
                if row.starts_with("C ") {
                    let f: Vec<u32> = row
                        .split_whitespace()
                        .skip(1)
                        .map(|s| s.parse().unwrap())
                        .collect();
                    assert_eq!(
                        f[..4],
                        [
                            0,
                            entry.scenario_id as u32,
                            entry.random_seed,
                            entry.save_position
                        ],
                        "{label}: native GameData context"
                    );
                    context = true;
                } else if row.starts_with("A ") {
                    assert_audio(row, &scene.state, &label);
                    audio_rows += 1;
                } else {
                    let key = if row.starts_with("S ") {
                        "S".to_owned()
                    } else {
                        row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
                    };
                    assert_eq!(row, expected.remove(&key).unwrap(), "{label}");
                    total_rows += 1;
                }
            }
            assert!(expected.is_empty() && context);
            assert_eq!(audio_rows, 33);
        }
        assert!(lines.next().is_none());
        eprintln!(
            "{version}: {} real SELECT saved entry/response/following-boundary cases, {total_rows} native scene rows, {} audio descriptors, PC/route consumption, uninterrupted/reconstructed state and native context0/ownership pass",
            cases.len(),
            cases.len() * 33
        );
    }
    eprintln!("Evidence: {}", out.display());
}

#[test]
#[ignore = "requires supplied ROMs/executables and prior recorded Episode8 route; CPU only, no GPU/audio/save writes"]
fn reference_native_saved_movie_load_checkpoint_selection() {
    use shin_core::format::save::{GameData, RouteData};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = root.join("game-data/umineko");
    let out = std::path::PathBuf::from(std::env::var_os("SHIN_MOVIE_LOAD_CHECKPOINT_OUTPUT").unwrap());
    assert!(out.starts_with("/private/tmp"));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("native-event-load-restart");
    assert!(Command::new("clang")
        .args(["-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
        .arg(root.join("reverse-engineering/native-event-load-restart.c"))
        .arg("-o").arg(&helper).status().unwrap().success());
    let mut zero_route = RouteRecorder::default();
    for _ in 0..16384 { zero_route.record_persistent(0); }
    for (version, image, rom, recorded) in [
        ("original", "uncompressed_executable", "data.rom", "hydration-original"),
        ("1.03", "uncompressed_executable_update", "patch.rom", "hydration-updated"),
    ] {
        let bytes = scenario_bytes(&files.join(rom));
        let snr = out.join(format!("{version}.snr"));
        std::fs::write(&snr, &bytes).unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut games: Vec<GameData> = [(2,2364),(3,7733),(5,3852),(25,93),(45,55)].into_iter()
            .map(|(scenario_id, save_position)| GameData::from_entry(GameDataEntry {
                scenario_id, random_seed: 0, save_position, route_data: zero_route.snapshot(),
            })).collect();
        let prior = std::fs::read_to_string(
            Path::new("/private/tmp/shin-episode8-recorded-movies").join(recorded).join("input.txt")
        ).unwrap();
        for line in prior.lines() {
            let f: Vec<_> = line.split_whitespace().collect();
            let bytes: Vec<u8> = f[3].as_bytes().chunks_exact(2)
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect();
            let route_data: RouteData = serde_json::from_value(serde_json::json!(bytes)).unwrap();
            games.push(GameData::from_entry(GameDataEntry {
                scenario_id: f[0].parse().unwrap(), random_seed: u32::from_str_radix(f[1],16).unwrap(),
                save_position: f[2].parse().unwrap(), route_data,
            }));
        }
        assert_eq!(games.len(),7);
        let payload: String = games.iter().map(|g| {
            let bytes: Vec<u8> = serde_json::from_value(serde_json::to_value(&g.entry.route_data).unwrap()).unwrap();
            let hex: String = bytes.iter().map(|b|format!("{b:02x}")).collect();
            format!("{} {:x} {} {hex}\n",g.entry.scenario_id,g.entry.random_seed,g.entry.save_position)
        }).collect();
        let input = out.join(format!("{version}.input"));std::fs::write(&input, payload).unwrap();
        let native = Command::new(&helper).arg(files.join(image)).arg(&snr)
            .stdin(Stdio::from(std::fs::File::open(input).unwrap())).output().unwrap();
        std::fs::write(out.join(format!("{version}.native")), &native.stdout).unwrap();
        std::fs::write(out.join(format!("{version}.stderr")), &native.stderr).unwrap();
        assert!(native.status.success(), "{}",String::from_utf8_lossy(&native.stderr));
        let native = String::from_utf8(native.stdout).unwrap();
        let mut lines = native.lines().peekable();
        let mut summary = Vec::new();
        for game in games {
            let label = format!("{version} entry{} saved{}",game.entry.scenario_id,game.entry.save_position);
            let raw = ReconstructedScene::from_game_data(&scenario, &game, &SaveData::default(), 1_000_000).unwrap();
            let loaded = ReconstructedScene::from_game_data_for_load(&scenario, &game, &SaveData::default(), 1_000_000).unwrap();
            let header: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(header[0],"E");assert_eq!(header[1].parse::<i32>().unwrap(),game.entry.scenario_id);
            assert_eq!(header[2].parse::<u32>().unwrap(),game.entry.save_position);
            assert_eq!(header[3].parse::<u32>().unwrap(),loaded.state.save_position,"{label}");
            assert_eq!(header[4].parse::<i32>().unwrap(),loaded.state.current_event.unwrap_or(-1),"{label}");
            assert_eq!(u32::from_str_radix(header[5],16).unwrap(),loaded.scripter.position().0,"{label}");
            assert_eq!(header[6].parse::<u32>().unwrap(),loaded.state.chapter_skip_position,"{label}");
            let mut expected = expected_rows(&loaded.state);let state_rows = expected.len();let mut audio=0;
            while lines.peek().is_some_and(|row|!row.starts_with("E ")) {
                let row=lines.next().unwrap();
                if row.starts_with("A "){assert_audio(row,&loaded.state,&label);audio+=1;continue;}
                let key=if row.starts_with("S "){"S".into()}else{row.split_whitespace().take(2).collect::<Vec<_>>().join(" ")};
                assert_eq!(row, expected.remove(&key).unwrap(),"{label} {key}");
            }
            assert!(expected.is_empty());assert_eq!(audio,33);
            let movies: Vec<_> = loaded.state.layers.layerbanks.iter()
                .filter(|(_,l)|l.layer_type==Some(shin_core::vm::command::types::LayerType::Movie))
                .map(|(bank,l)|(bank.raw(),l.params.0,l.layer_load_counter)).collect();
            assert!(!movies.is_empty(),"{label}: known saved Movie must remain present");
            assert_eq!(loaded.state.save_position,game.entry.save_position,"{label}: native control should retain ordinary Movie checkpoint");
            summary.push(serde_json::json!({"entry":game.entry.scenario_id,"saved_boundary":game.entry.save_position,
                "raw_event":raw.state.current_event,"prepared_boundary":loaded.state.save_position,
                "chapter_skip_position":loaded.state.chapter_skip_position,"state_rows":state_rows,"audio_rows":audio,"movies":movies}));
        }
        assert!(lines.next().is_none());
        std::fs::write(out.join(format!("{version}.summary.json")),serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
        println!("{version}: seven saved Movie checkpoint selections and complete CPU layer/property/mask/audio state match native worker prefix");
    }
}
