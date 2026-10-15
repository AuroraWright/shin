//! Chapter search against unchanged native command implementations and SNR.
use super::{ReconstructedScene, native_replay_tests::scenario_bytes};
use shin_core::format::{
    save::{GameData, GameDataEntry, RouteData, SaveData},
    scenario::Scenario,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
#[ignore = "requires supplied original/1.03 ROMs and executable; CPU only, no saves/audio/window"]
fn reference_native_chapter_targets_and_read_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rom = std::path::PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap());
    let files = rom.parent().unwrap();
    let out = std::env::temp_dir().join(format!("shin-chapter-navigation-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("native");
    assert!(
        Command::new("clang")
            .args(["-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-chapter-navigation.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    for (version, image, rom) in [
        ("original", "uncompressed_executable", rom.clone()),
        (
            "1.03",
            "uncompressed_executable_update",
            files.join("patch.rom"),
        ),
    ] {
        let bytes = scenario_bytes(&rom);
        let snr = out.join(format!("{version}.snr"));
        std::fs::write(&snr, &bytes).unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        // Reuse the previously validated authored Episode8 route stream.
        let packet = std::fs::read_to_string(
            std::path::PathBuf::from("/private/tmp")
                .join("shin-episode8-recorded-movies")
                .join(if version == "original" {
                    "hydration-original"
                } else {
                    "hydration-updated"
                })
                .join("input.txt"),
        )
        .unwrap();
        let fields: Vec<_> = packet.lines().next().unwrap().split_whitespace().collect();
        let episode_hex = fields[3];
        let episode_bytes: Vec<u8> = episode_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        // Native saved construction grows an absent route with zero storage.
        // Give both interpreters that same historical stream explicitly.
        let opening_hex = "00".repeat(1024);
        let mut cases = Vec::new();
        let mut input = String::new();
        for (disallow, seen) in [(false, 0), (true, 0), (true, 50), (true, 1_000_000)] {
            let mut globals = SaveData::default();
            globals.settings.v11_disallowskipunread = disallow;
            for id in 0..seen {
                globals.save_vectors.seen_messages.add(id);
            }
            for (entry, boundary) in [
                (0, 1),
                (0, 54),
                (0, 55),
                (0, 56),
                (0, 57),
                (0, 58),
                (0, 59),
                (0, 324),
                (0, 325),
                (0, 326),
                (0, 7149),
                (0, 7150),
                (0, 7151),
                (7, 1570),
                (7, 6896),
            ] {
                let route_data: RouteData =
                    serde_json::from_value(serde_json::json!(if entry == 7 {
                        episode_bytes.clone()
                    } else {
                        vec![0u8; 1024]
                    }))
                    .unwrap();
                let mut game = GameData::from_entry(GameDataEntry {
                    scenario_id: entry,
                    random_seed: 12345,
                    save_position: boundary,
                    route_data,
                });
                // Entry-one installs must retain the independent inactive context.
                game.primary_entry = Some(GameDataEntry {
                    scenario_id: 3,
                    random_seed: 0xfeed1234,
                    save_position: 77,
                    route_data: serde_json::from_value(serde_json::json!([
                        0x93, 0x51, 0x72, 0xa8, 6
                    ]))
                    .unwrap(),
                });
                input.push_str(&format!(
                    "{entry} 3039 {boundary} {} {seen} {}\n",
                    u8::from(disallow),
                    if entry == 7 {
                        episode_hex
                    } else {
                        &opening_hex
                    }
                ));
                let before = globals.encode_raw();
                let source =
                    ReconstructedScene::from_game_data(&scenario, &game, &globals, 1_000_000)
                        .unwrap();
                let previous = source.state.previous_chapter_position();
                let candidate = source.state.chapter_skip_position;
                let next = ReconstructedScene::next_chapter_game_data(
                    &scenario, &game, &globals, 1_000_000,
                )
                .unwrap();
                let mut fork = source.scripter.fork_for_simulation();
                let mut shadow = source.state.clone();
                match source.stop {
                    super::ReconstructionStop::Command(command) => {
                        if !ReconstructedScene::chapter_search_blocks(&command, &shadow) {
                            ReconstructedScene::search_next_chapter(
                                &mut fork,
                                &mut shadow,
                                command.execute_dummy().unwrap(),
                                1_000_000,
                            )
                            .unwrap();
                        }
                    }
                    super::ReconstructionStop::Response(result) => {
                        ReconstructedScene::search_next_chapter(
                            &mut fork,
                            &mut shadow,
                            result,
                            1_000_000,
                        )
                        .unwrap();
                    }
                }
                assert_eq!(
                    (shadow.chapter_skip_position > boundary)
                        .then_some(shadow.chapter_skip_position),
                    next.as_ref().map(|n| n.state.save_position),
                    "Forked continuation agrees with canonical/native target"
                );
                assert_eq!(source.state.save_position, boundary);
                assert_eq!(
                    before,
                    globals.encode_raw(),
                    "{version} search publishes no globals"
                );
                if let Some(next) = &next {
                    assert_eq!(
                        serde_json::to_value(&next.state.primary_entry).unwrap(),
                        serde_json::to_value(&game.primary_entry).unwrap()
                    );
                    assert_eq!(next.state.save_revision, 0);
                }
                cases.push((
                    entry,
                    boundary,
                    previous,
                    next.map(|n| n.state.save_position),
                    candidate,
                ));
            }
        }
        std::fs::write(out.join(format!("{version}-input.txt")), &input).unwrap();
        let mut child = Command::new(&helper)
            .arg(files.join(image))
            .arg(&snr)
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
        std::fs::write(out.join(format!("{version}-native.txt")), &native.stdout).unwrap();
        assert!(
            native.status.success(),
            "{}",
            String::from_utf8_lossy(&native.stderr)
        );
        let output = String::from_utf8(native.stdout).unwrap();
        let rows: Vec<_> = output.lines().collect();
        assert_eq!(rows.len(), cases.len());
        for (index, (row, (entry, boundary, previous, next, candidate))) in
            rows.into_iter().zip(cases).enumerate()
        {
            let fields: Vec<u32> = row
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(
                &fields[..6],
                &[
                    entry as u32,
                    boundary,
                    previous,
                    u32::from(next.is_some()),
                    next.unwrap_or(0),
                    candidate
                ],
                "{version} case{index}: {row}"
            );
        }
        println!(
            "{version}: 60 native chapter target/read-state and forked continuation cases passed"
        );
    }
    println!("Evidence: {}", out.display());
}
