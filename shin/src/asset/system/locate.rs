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
    result.with_localized_file(path.join("localized.toml"));
    // First-mounted layers win. Optional mods override all standard layers.
    if let Ok(mod_rom) = path.join("mod.rom").canonicalize() {
        trace!("Trying mod ROM {:?}...", mod_rom);
        match result.try_with_rom(&mod_rom) {
            Ok(_) => trace!("Using mod ROM {:?}", mod_rom),
            Err(err) => trace!("Cannot use {:?} as assets directory: {}", path, err),
        }
    }
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
/// Optional `mod.rom` has highest priority, followed by the patch directory,
/// patch.rom, data directory and data.rom. Missing entries fall through to the
/// following layers. This can be useful for translation mods.
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
        Game::Gerokasu => return locate_gerokasu_assets(cli_assets),
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
    use anyhow::ensure;
    ensure!(
        path.is_dir(),
        "Higurashi asset directory does not exist: {}",
        path.display()
    );
    let mut result = LayeredAssetIo::new();
    result.with_localized_file(path.join("localized.toml"));
    // Resolve the selected scenario before append layers can affect it. Keep
    // mod ROM first, then the existing desktop order: patch directory/ROM,
    // data directory, eligible append directory/ROM, then base ROM. A modded
    // scenario also controls whether append content can be mounted.
    for name in ["mod.rom", "patch", "patch.rom", "data"] {
        mount_higurashi_layer(&mut result, path, name)?;
    }
    let mut base = LayeredAssetIo::new();
    mount_higurashi_layer(&mut base, path, "data.rom")?;
    let supports_origin = (|| -> anyhow::Result<bool> {
        use std::io::Read;
        let data = result
            .read_file("/main.snr")
            .or_else(|_| base.read_file("/main.snr"))?;
        let mut bytes = Vec::new();
        data.cursor()?.read_to_end(&mut bytes)?;
        let scenario =
            shin_core::format::scenario::higurashi::HigurashiScenario::new(bytes.into())?;
        Ok(higurashi_scenario_supports_origin(&scenario))
    })()
    .unwrap_or_else(|error| {
        // Startup retains responsibility for reporting its required SNR error.
        debug!("Append remains unmounted: selected Higurashi scenario unavailable: {error:#}");
        false
    });
    if supports_origin {
        for name in ["append", "append.rom"] {
            mount_higurashi_layer(&mut result, path, name)?;
        }
    }
    if !base.is_empty() {
        result.with(base.into());
    }
    ensure!(
        !result.is_empty(),
        "No Higurashi asset layers in {}",
        path.display()
    );
    Ok(result)
}

fn mount_higurashi_layer(
    result: &mut LayeredAssetIo,
    path: &Path,
    name: &str,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let source = path.join(name);
    if source.exists() {
        if name.ends_with(".rom") {
            result.try_with_rom(&source)
        } else {
            result.try_with_dir(&source)
        }
        .with_context(|| format!("Opening Higurashi asset layer {}", source.display()))?;
    }
    Ok(())
}

/// CPU-only content capability. A revision tag alone cannot establish Origin
/// support in a partial/older patch: require its catalog and authored dispatch.
pub(crate) fn higurashi_scenario_supports_origin(
    scenario: &shin_core::format::scenario::higurashi::HigurashiScenario,
) -> bool {
    use shin_core::{
        format::scenario::higurashi::HigurashiVariant,
        vm::{
            command::CommandResult,
            higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand},
        },
    };
    if scenario.variant != HigurashiVariant::Updated
        || !scenario
            .bgm_info
            .iter()
            .any(|row| row.name.as_str() == "org/2/it_moves")
        || !scenario
            .tips_info
            .iter()
            .any(|row| row.updated_field.is_some_and(|id| id > 9999))
    {
        return false;
    }
    let Ok(mut script) = HigurashiScripter::new(scenario, 10001, 0) else {
        return false;
    };
    for _ in 0..64 {
        let Ok(command) = script.run(CommandResult::None, 10_000) else {
            return false;
        };
        if let HigurashiRuntimeCommand::SAVEINFO { level: 0, info } = &command {
            return info.starts_with("ひぐらしオリジン");
        }
        // Inspect only inert prologue commands. Any host read or interactive
        // result before identity leaves this capability unproven.
        match command {
            HigurashiRuntimeCommand::MSGINIT { .. }
            | HigurashiRuntimeCommand::LAYERUNLOAD { .. }
            | HigurashiRuntimeCommand::SSET { .. } => {}
            _ => return false,
        }
    }
    false
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

    #[cfg(unix)]
    struct Layers(std::path::PathBuf);
    #[cfg(unix)]
    impl Layers {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "shin-patchless-mount-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn link(&self, root: &Path, name: &str) {
            std::os::unix::fs::symlink(root.join(name), self.0.join(name)).unwrap();
        }
    }
    #[cfg(unix)]
    impl Drop for Layers {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn patchless_mount_missing_invalid_or_header_only_scenario_keeps_append_unmounted() {
        use std::io::Read;
        for content in [None, Some(b"invalid SNR".as_slice())] {
            let layers = Layers::new();
            std::fs::create_dir(layers.0.join("data")).unwrap();
            std::fs::create_dir(layers.0.join("append")).unwrap();
            std::fs::write(layers.0.join("append/append.txt"), "append").unwrap();
            if let Some(content) = content {
                std::fs::write(layers.0.join("data/main.snr"), content).unwrap();
            }
            let io = higurashi_directory(&layers.0).unwrap();
            assert!(io.read_file("/append.txt").is_err());
            if let Some(content) = content {
                let mut actual = Vec::new();
                io.read_file("/main.snr")
                    .unwrap()
                    .cursor()
                    .unwrap()
                    .read_to_end(&mut actual)
                    .unwrap();
                assert_eq!(actual, content);
            }
        }
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "actual base/patch/append archive content capability and overlay precedence; CPU only"]
    fn patchless_mount_actual_scenario_controls_append_independently_of_patch_filename() {
        use shin_core::format::scenario::higurashi::HigurashiScenario;
        use std::io::Read;
        let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
        for patch in [false, true] {
            for append in [false, true] {
                let layers = Layers::new();
                layers.link(&root, "data.rom");
                if patch {
                    layers.link(&root, "patch.rom");
                }
                if append {
                    layers.link(&root, "append.rom");
                }
                let io = higurashi_directory(&layers.0).unwrap();
                assert_eq!(io.read_file("/append.txt").is_ok(), patch && append);
                let mut data = Vec::new();
                io.read_file("/main.snr")
                    .unwrap()
                    .cursor()
                    .unwrap()
                    .read_to_end(&mut data)
                    .unwrap();
                let mut scenario = HigurashiScenario::new(data.into()).unwrap();
                assert_eq!(higurashi_scenario_supports_origin(&scenario), patch);
                if patch {
                    scenario
                        .bgm_info
                        .retain(|row| row.name.as_str() != "org/2/it_moves");
                    assert!(!higurashi_scenario_supports_origin(&scenario));
                }
            }
        }
        // An Original directory override wins over full patch. A full updated
        // scenario in a directory supports append without a patch.rom filename.
        for selected_updated in [false, true] {
            let source = higurashi_directory(&root).unwrap();
            let layers = Layers::new();
            layers.link(&root, "data.rom");
            layers.link(&root, "append.rom");
            std::fs::create_dir(layers.0.join("patch")).unwrap();
            let data = if selected_updated {
                let mut data = Vec::new();
                source
                    .read_file("/main.snr")
                    .unwrap()
                    .cursor()
                    .unwrap()
                    .read_to_end(&mut data)
                    .unwrap();
                data
            } else {
                layers.link(&root, "patch.rom");
                let mut base = LayeredAssetIo::new();
                base.try_with_rom(root.join("data.rom")).unwrap();
                let mut data = Vec::new();
                base.read_file("/main.snr")
                    .unwrap()
                    .cursor()
                    .unwrap()
                    .read_to_end(&mut data)
                    .unwrap();
                data
            };
            std::fs::write(layers.0.join("patch/main.snr"), data).unwrap();
            let io = higurashi_directory(&layers.0).unwrap();
            assert_eq!(io.read_file("/append.txt").is_ok(), selected_updated);
        }
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

/// Gerokasu uses its own named root so it never picks up Gerokasu2 data.
fn locate_gerokasu_assets(cli_assets: Option<&Path>) -> anyhow::Result<LayeredAssetIo> {
    let explicit = cli_assets
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SHIN_GEROKASU_ASSETS").map(PathBuf::from));
    let paths = if let Some(path) = explicit {
        vec![path]
    } else {
        let mut paths = vec![std::env::current_dir()?.join("gerokasu")];
        if let Some(parent) = std::env::current_exe()?.parent() {
            paths.push(parent.join("gerokasu"));
        }
        paths
    };
    for path in &paths {
        if let Some(io) = try_assets_directory(path)? {
            return Ok(io);
        }
    }
    bail!("Failed to locate Gerokasu assets, tried {paths:?}. Use --assets-dir or SHIN_GEROKASU_ASSETS.");
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

#[cfg(test)]
mod mod_rom_tests;
