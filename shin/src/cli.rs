use std::path::PathBuf;

use clap::Parser;
use clap_num::maybe_hex;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
/// A visual novel engine
pub struct Cli {
    /// Route audio to the Loopback Audio system device for testing.
    #[clap(long)]
    pub loopback_audio: bool,
    /// Read global progress and write autosaves to this engine-format file.
    #[clap(long)]
    pub save_file: Option<PathBuf>,
    /// Load an engine-format save file when starting the game.
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
    /// Automatically fast-forward the scenario to the specified address (useful for debugging)
    #[clap(long, value_parser=maybe_hex::<u32>)]
    pub fast_forward_to: Option<u32>,
    /// Start scenario execution from the specified address (useful for debugging)
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
