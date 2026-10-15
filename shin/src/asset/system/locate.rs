use std::path::{Path, PathBuf};

use anyhow::bail;
use tracing::{debug, instrument, trace};

use crate::{asset::system::LayeredAssetIo, cli::Game};

#[instrument]
pub(super) fn try_assets_directory(path: &Path) -> anyhow::Result<Option<LayeredAssetIo>> {
    debug!("Trying assets directory {:?}...", path);
    if !path.is_dir() {
        debug!("Cannot use {:?} as assets directory, not a directory", path);
        return Ok(None);
    }
    let mut result = LayeredAssetIo::new();
    // try to use "data" directory first and them data.rom file
    if let Ok(patch_dir) = path.join("patch").canonicalize() {
        trace!("Trying patch directory {:?}...", patch_dir);
        match result.try_with_dir(&patch_dir) {
            Ok(_) => trace!("Using patch directory {:?}", patch_dir),
            Err(err) => trace!("Cannot use {:?} as assets directory: {}", path, err),
        }
    }
    if let Ok(patch_rom) = path.join("patch.rom").canonicalize() {
        trace!("Trying patch ROM {:?}...", patch_rom);
        match result.try_with_rom(&patch_rom) {
            Ok(_) => trace!("Using patch ROM {:?}", patch_rom),
            Err(err) => trace!("Cannot use {:?} as assets directory: {}", path, err),
        }
    }
    if let Ok(data_dir) = path.join("data").canonicalize() {
        trace!("Trying data directory {:?}...", data_dir);
        match result.try_with_dir(&data_dir) {
            Ok(_) => trace!("Using data directory {:?}", data_dir),
            Err(err) => trace!("Cannot use {:?} as assets directory: {}", path, err),
        }
    }
    if let Ok(data_rom) = path.join("data.rom").canonicalize() {
        trace!("Trying data ROM {:?}...", data_rom);
        match result.try_with_rom(&data_rom) {
            Ok(_) => trace!("Using data ROM {:?}", data_rom),
            Err(err) => trace!("Cannot use {:?} as assets directory: {}", path, err),
        }
    }

    if result.is_empty() {
        trace!("Cannot use {:?} as assets directory, no data found", path);
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

/// Implements the logic for locating game assets.
///
/// The asset directory is expected to contain a "data" directory or a "data.rom" file.
///
/// It can also contain both, in which case the assets are searched in the "data" directory first, then in the "data.rom" file. This can be useful for translation mods.
///
/// The candidate asset directories are (in order)
/// 1. The directory specified on the command line with the `--assets-dir` option
/// 2. The directory specified in the `SHIN_ASSETS` environment variable
/// 3. The directory "assets" next to the executable
/// 4. The directory "assets" in the current working directory
/// 5. The user's shared data directory (see [`dirs_next::data_dir`], `/home/alice/.local/share/shin/assets` / `C:\Users\Alice\AppData\Roaming\shin\assets` / `/Users/Alice/Library/Application Support/shin/assets`)
///
/// The used asset directory is the first one having a "data" directory or a "data.rom" file.
#[allow(clippy::match_result_ok)]
pub fn locate_assets(cli_assets: Option<&Path>) -> anyhow::Result<LayeredAssetIo> {
    // First, try the assets directory specified on the command line
    // Then, try the assets directory specified in the environment
    // Then, try the assets directory next to the executable
    // Then, try the assets directory in the current working directory
    // Then, try the user's shared data directory

    let mut try_list = Vec::new();

    if let Some(cli_assets) = cli_assets {
        try_list.push(cli_assets.to_path_buf());
    }

    if let Some(env_assets) = std::env::var_os("SHIN_ASSETS") {
        try_list.push(PathBuf::from(env_assets));
    }

    if let Some(exe_assets) = std::env::current_exe()?.parent().map(|p| p.join("assets")) {
        try_list.push(exe_assets);
    }

    if let Some(cwd_assets) = std::env::current_dir()?.join("assets").canonicalize().ok() {
        try_list.push(cwd_assets);
    }

    // |Platform | Value                                    | Example                                  |
    // | ------- | ---------------------------------------- | ---------------------------------------- |
    // | Linux   | `$XDG_DATA_HOME` or `$HOME`/.local/share | /home/alice/.local/share                 |
    // | macOS   | `$HOME`/Library/Application Support      | /Users/Alice/Library/Application Support |
    // | Windows | `{FOLDERID_RoamingAppData}`              | C:\Users\Alice\AppData\Roaming           |
    if let Some(shared_assets) = dirs_next::data_dir().map(|p| p.join("../../..").join("assets")) {
        try_list.push(shared_assets);
    }

    for path in try_list.iter() {
        if let Some(result) = try_assets_directory(path)? {
            return Ok(result);
        }
    }

    bail!("Failed to locate assets directory, tried: {:#?}", try_list);
}

/// Preserve Umineko's existing search policy. Other games use named roots
/// so they cannot silently pick up assets from an unrelated game.
pub fn locate_assets_for_game(
    game: Game,
    cli_assets: Option<&Path>,
) -> anyhow::Result<LayeredAssetIo> {
    match game {
        Game::Umineko => return locate_assets(cli_assets),
        Game::Gerokasu2 => return locate_gerokasu2_assets(cli_assets),
        Game::Higurashi => {}
    }
    if let Some(path) = cli_assets {
        return higurashi_directory(path);
    }
    if let Some(path) = std::env::var_os("SHIN_HIGURASHI_ASSETS") {
        return higurashi_directory(Path::new(&path));
    }
    let mut paths = vec![std::env::current_dir()?.join("higurashi")];
    if let Some(parent) = std::env::current_exe()?.parent() {
        paths.push(parent.join("higurashi"));
    }
    for path in &paths {
        if path.is_dir() {
            return higurashi_directory(path);
        }
    }
    bail!(
        "Failed to locate Higurashi assets, tried {paths:?}. Use --assets-dir or SHIN_HIGURASHI_ASSETS."
    );
}

fn higurashi_directory(path: &Path) -> anyhow::Result<LayeredAssetIo> {
    use anyhow::{Context, ensure};
    ensure!(
        path.is_dir(),
        "Higurashi asset directory does not exist: {}",
        path.display()
    );
    let mut result = LayeredAssetIo::new();
    // Mod overrides, updated scenario/art, append content, then base content.
    // This is a desktop overlay policy; native mount priority remains to trace.
    for name in [
        "patch",
        "patch.rom",
        "data",
        "append",
        "append.rom",
        "data.rom",
    ] {
        let source = path.join(name);
        if source.exists() {
            if name.ends_with(".rom") {
                result.try_with_rom(&source)
            } else {
                result.try_with_dir(&source)
            }
            .with_context(|| format!("Opening Higurashi asset layer {}", source.display()))?;
        }
    }
    ensure!(
        !result.is_empty(),
        "No Higurashi asset layers in {}",
        path.display()
    );
    Ok(result)
}

#[cfg(test)]
mod higurashi_tests {
    use super::*;

    #[test]
    fn explicit_missing_higurashi_directory_does_not_fall_back() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("missing-higurashi-assets");
        assert!(locate_assets_for_game(Game::Higurashi, Some(&path)).is_err());
    }

    #[test]
    #[ignore = "requires supplied Higurashi archives (SHIN_HIGURASHI_ASSETS)"]
    fn supplied_higurashi_patch_and_append_overlay() {
        use shin_core::{
            format::{
                rom::{IndexEntry, RomReader},
                scenario::higurashi::{HigurashiScenario, HigurashiVariant},
            },
            primitives::stateless_reader::StatelessFile,
        };
        use std::{collections::BTreeSet, io::Read};
        let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
        let io = locate_assets_for_game(Game::Higurashi, Some(&root)).unwrap();
        let mut patched = BTreeSet::new();
        for archive in ["patch.rom", "append.rom"] {
            let mut rom = RomReader::new(
                StatelessFile::new(std::fs::File::open(root.join(archive)).unwrap()).unwrap(),
            )
            .unwrap();
            let files: Vec<_> = rom
                .traverse()
                .filter_map(|(path, entry)| match entry {
                    IndexEntry::File(file) => Some((path, *file)),
                    _ => None,
                })
                .collect();
            for (path, file) in &files {
                // Every append path is visible, and all patch bytes win over
                // later layers. Sample append bytes without reading 1.3 GB.
                let accessor = io.read_file(path).unwrap();
                if archive == "patch.rom" || (path.ends_with(".pic") && !patched.contains(path)) {
                    let mut expected = Vec::new();
                    rom.open_file(*file)
                        .unwrap()
                        .read_to_end(&mut expected)
                        .unwrap();
                    let mut actual = Vec::new();
                    accessor.cursor().unwrap().read_to_end(&mut actual).unwrap();
                    assert_eq!(actual, expected, "{archive}: {path}");
                }
                if archive == "patch.rom" {
                    patched.insert(path.clone());
                }
            }
            println!("{archive}: {} overlaid paths checked", files.len());
        }
        let mut data = Vec::new();
        io.read_file("/main.snr")
            .unwrap()
            .cursor()
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        assert_eq!(
            HigurashiScenario::new(data.into()).unwrap().variant,
            HigurashiVariant::Updated
        );
        assert!(io.read_file("/newrodin.fnt").is_ok());
    }
}

fn locate_gerokasu2_assets(cli_assets: Option<&Path>) -> anyhow::Result<LayeredAssetIo> {
    let explicit = cli_assets
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SHIN_GEROKASU2_ASSETS").map(PathBuf::from));
    let paths = if let Some(path) = explicit {
        vec![path]
    } else {
        let mut paths = vec![std::env::current_dir()?.join("gerokasu2")];
        if let Some(parent) = std::env::current_exe()?.parent() {
            paths.push(parent.join("gerokasu2"));
        }
        paths
    };
    for path in &paths {
        if let Some(io) = try_assets_directory(path)? {
            return Ok(io);
        }
    }
    bail!(
        "Failed to locate Gerokasu2 assets, tried {paths:?}. Use --assets-dir or SHIN_GEROKASU2_ASSETS."
    );
}

#[cfg(test)]
mod gerokasu2_tests {
    use super::*;

    #[test]
    fn explicit_missing_directory_does_not_fall_back_to_another_game() {
        let path = std::env::temp_dir().join(format!("shin-no-gerokasu2-{}", std::process::id()));
        assert!(locate_assets_for_game(Game::Gerokasu2, Some(&path)).is_err());
    }
}
