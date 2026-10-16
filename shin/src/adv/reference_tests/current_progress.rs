//! Inspect actual copied progress without asset preparation, playback or writes.
use crate::adv::reconstruction::ReconstructedScene;
use shin_core::{
    format::{rom::RomReader, save::SaveData, scenario::Scenario},
    primitives::stateless_reader::StatelessFile,
    vm::command::types::LayerType,
};
use std::{fs::File, io::Read, path::PathBuf};

#[test]
#[ignore = "requires copied progress and supplied ROM;CPU only,no devices orworking save writes"]
fn reference_current_progress_saved_scene_origins() {
    let path = PathBuf::from(std::env::var_os("SHIN_PROGRESS_AUDIT_SAVE").unwrap());
    assert!(
        path.starts_with("/private/tmp"),
        "Use a copied progress file"
    );
    let bytes = std::fs::read(&path).unwrap();
    let globals = SaveData::decode(&bytes).unwrap();
    let rom_path = PathBuf::from(std::env::var_os("SHIN_PROGRESS_AUDIT_ROM").unwrap());
    let mut rom =
        RomReader::new(StatelessFile::new(File::open(rom_path).unwrap()).unwrap()).unwrap();
    let mut snr = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut snr)
        .unwrap();
    let scenario = Scenario::new(snr.into()).unwrap();
    let entries = globals
        .auto_save_slot
        .iter()
        .map(|game| ("auto".to_owned(), game))
        .chain(
            globals
                .manual_save_slots
                .iter()
                .enumerate()
                .filter_map(|(index, game)| {
                    game.as_ref().map(|game| (format!("manual{index}"), game))
                }),
        );
    let mut records = Vec::new();
    for (slot, game) in entries {
        let scene = ReconstructedScene::from_game_data(&scenario, game, &globals, 1_000_000)
            .unwrap_or_else(|error| panic!("{slot} reconstruction: {error:#}"));
        let movies: Vec<_> = scene
            .state
            .layers
            .layerbanks
            .iter()
            .filter(|(_, bank)| bank.layer_type == Some(LayerType::Movie))
            .map(|(id, bank)| {
                serde_json::json!({
                    "bank": id.raw(), "source": bank.params.0,
                    "repeat": bank.params.2 != 0, "counter": bank.layer_load_counter,
                })
            })
            .collect();
        records.push(serde_json::json!({
            "slot": slot, "scenario_id": game.entry.scenario_id,
            "seed": game.entry.random_seed, "saved_boundary": game.entry.save_position,
            "reconstructed_boundary": scene.state.save_position,
            "secondary_entry": game.primary_entry.is_some(),
            "movies": movies,
        }));
    }
    let output = PathBuf::from(std::env::var_os("SHIN_PROGRESS_AUDIT_OUTPUT").unwrap());
    assert!(output.starts_with("/private/tmp"));
    std::fs::write(output, serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "Copied progress was mutated"
    );
    println!(
        "Copied current progress: {} saved snapshots reconstructed,without devices orwrites",
        records.len()
    );
}
