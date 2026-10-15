//! CPU-only inspection before the window, audio, task pools or save system start.
use std::{collections::BTreeSet, io::Read};

use anyhow::{Context, Result};
use shin_core::format::scenario::{Scenario, higurashi::HigurashiScenario};

use crate::{
    asset::system::locate_assets_for_game,
    cli::{Cli, Game},
};

pub(crate) fn inspect(cli: &Cli) -> Result<()> {
    let io = locate_assets_for_game(cli.game, cli.assets_dir.as_deref())?;
    println!(
        "Game: {:?}\nAsset layers (first match wins): {io:#?}",
        cli.game
    );
    let mut data = Vec::new();
    io.read_file("/main.snr")?
        .cursor()?
        .read_to_end(&mut data)
        .context("Reading scenario for inspection")?;
    let mut paths = BTreeSet::new();
    match cli.game {
        Game::Higurashi => {
            let scenario = HigurashiScenario::new(data.into())?;
            println!(
                "Scenario: {:?}, {} bytes, entry 0x{:x}, {} dialogue IDs",
                scenario.variant,
                scenario.header.size,
                scenario.header.code_offset,
                scenario.header.dialogue_line_count
            );
            println!(
                "Tables: masks={}, pictures={}, bustups={}, BGM={}, SE={}, movies={}, voice mappings={}, CG gallery={}, music gallery={}, tips={}, chart records={} (raw payload)",
                scenario.mask_info.len(),
                scenario.picture_info.len(),
                scenario.bustup_info.len(),
                scenario.bgm_info.len(),
                scenario.se_info.len(),
                scenario.movie_info.len(),
                scenario.voice_mapping_info.len(),
                scenario.picture_box_info.len(),
                scenario.music_box_info.len(),
                scenario.tips_info.len(),
                scenario.chart_record_count
            );
            for item in &scenario.mask_info {
                paths.insert(format!(
                    "/mask/{}.msk",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.picture_info {
                paths.insert(format!(
                    "/picture/{}.pic",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.bustup_info {
                paths.insert(format!(
                    "/bustup/{}.bup",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.bgm_info {
                paths.insert(format!(
                    "/bgm/{}.nxa",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.se_info {
                paths.insert(format!(
                    "/se/{}.nxa",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.movie_info {
                paths.insert(format!(
                    "/movie/{}.mp4",
                    item.name.as_str().to_ascii_lowercase()
                ));
            }
            for item in &scenario.tips_info {
                paths.insert(format!(
                    "/{}",
                    item.texture_path.as_str().to_ascii_lowercase()
                ));
            }
            paths.insert("/newrodin.fnt".to_owned());
            println!(
                "Higurashi normal launch is available with --game higurashi; this inspection does not open a window or read/write progress."
            );
        }
        Game::Umineko => {
            let scenario = Scenario::new(data.into())?;
            let info = scenario.info_tables();
            println!(
                "Scenario: {} bytes, entry {}",
                scenario.raw().len(),
                scenario.entrypoint_address()
            );
            println!(
                "Tables: masks={}, pictures={}, bustups={}, BGM={}, SE={}, movies={}, voice mappings={}, tips={}",
                info.mask_info.len(),
                info.picture_info.len(),
                info.bustup_info.len(),
                info.bgm_info.len(),
                info.se_info.len(),
                info.movie_info.len(),
                info.voice_mapping_info.len(),
                info.tips_info.len()
            );
            paths.extend(info.mask_info.iter().map(|item| item.path()));
            paths.extend(info.picture_info.iter().map(|item| item.path()));
            paths.extend(info.bustup_info.iter().map(|item| item.path()));
            paths.extend(info.bgm_info.iter().map(|item| item.path()));
            paths.extend(info.se_info.iter().map(|item| item.path()));
            paths.extend(info.movie_info.iter().map(|item| item.path()));
        }
    }
    let templates: Vec<_> = paths.iter().filter(|path| path.contains('?')).collect();
    let missing: Vec<_> = paths
        .iter()
        .filter(|path| !path.contains('?'))
        .filter(|path| io.read_file(path).is_err())
        .collect();
    println!(
        "Asset references: {} unique paths, {} name templates, {} unresolved literal paths",
        paths.len(),
        templates.len(),
        missing.len()
    );
    for path in missing {
        println!("Unresolved: {path}");
    }
    println!(
        "Inspection checks metadata and literal path lookup. Native name substitutions, authored reachability, asset decoding and gameplay are not validated."
    );
    Ok(())
}
