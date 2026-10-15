use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use clap_num::maybe_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Game {
    #[default]
    Umineko,
    Higurashi,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
/// A visual novel engine
pub struct Cli {
    /// Select the game to launch (Umineko by default).
    #[clap(long, value_enum, default_value_t = Game::Umineko)]
    pub game: Game,
    /// Resources resolved before the Higurashi window starts. Direct app callers
    /// leave this empty and resolve the same resources during initialization.
    #[clap(skip)]
    pub(crate) higurashi_launch: Option<crate::app::PreparedLaunch>,
    /// Inspect scenario metadata and asset references without opening a window or writing saves.
    #[clap(long, conflicts_with_all = ["save_file", "load_save", "unsafe_entry_point", "fast_forward_to", "loopback_audio"])]
    pub inspect_assets: bool,
    /// Route audio to the Loopback Audio system device for testing.
    #[clap(long)]
    pub loopback_audio: bool,
    /// Read global progress and write saves to this engine-format file.
    /// Higurashi otherwise uses the desktop data directory's shin/higurashi/progress.dat.
    #[clap(long)]
    pub save_file: Option<PathBuf>,
    /// Load an engine-format save file when starting Umineko.
    #[clap(long, conflicts_with_all = ["unsafe_entry_point", "fast_forward_to"])]
    pub load_save: Option<PathBuf>,
    /// Load slot 0 (autosave) or a manual slot from 1 to 100.
    #[clap(long, default_value_t = 0, requires = "load_save", value_parser = clap::value_parser!(u8).range(0..=100))]
    pub save_slot: u8,
    /// Search this directory for assets
    ///
    /// The directory must contain either a directory named "data" or a file named "data.rom".
    /// Consult the README for more information.
    #[clap(short, long)]
    pub assets_dir: Option<PathBuf>,
    /// Automatically fast-forward Umineko to the specified address (useful for debugging)
    #[clap(long, value_parser=maybe_hex::<u32>)]
    pub fast_forward_to: Option<u32>,
    /// Start Umineko scenario execution from the specified address (useful for debugging)
    ///
    /// Called "unsafe" because the VM is not designed to be started from arbitrary addresses.
    /// But in lieu of proper scene loading this helps a lot with testing later parts of the episodes.
    #[clap(long, value_parser=maybe_hex::<u32>)]
    pub unsafe_entry_point: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_game_and_keeps_inspection_separate_from_save_launches() {
        assert_eq!(Cli::try_parse_from(["shin"]).unwrap().game, Game::Umineko);
        let cli = Cli::try_parse_from(["shin", "--game", "higurashi", "--inspect-assets"]).unwrap();
        assert_eq!(cli.game, Game::Higurashi);
        assert!(cli.inspect_assets);
        assert!(cli.higurashi_launch.is_none());
        assert!(Cli::try_parse_from(["shin", "--game", "unknown"]).is_err());
        assert!(
            Cli::try_parse_from(["shin", "--inspect-assets", "--save-file", "progress.dat"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["shin", "--inspect-assets", "--load-save", "progress.dat"])
                .is_err()
        );
    }
    #[test]
    fn save_loading_validates_slot_and_conflicting_entry_options() {
        assert!(Cli::try_parse_from(["shin"]).is_ok());
        let progress = Cli::try_parse_from(["shin", "--save-file", "progress.dat"]).unwrap();
        assert_eq!(progress.save_file, Some("progress.dat".into()));
        assert!(progress.load_save.is_none());
        let cli =
            Cli::try_parse_from(["shin", "--load-save", "save.dat", "--save-slot", "100"]).unwrap();
        assert_eq!(cli.save_slot, 100);
        assert!(Cli::try_parse_from(["shin", "--save-slot", "1"]).is_err());
        assert!(
            Cli::try_parse_from(["shin", "--load-save", "save.dat", "--save-slot", "101"]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "shin",
                "--load-save",
                "save.dat",
                "--unsafe-entry-point",
                "1"
            ])
            .is_err()
        );
    }
}
