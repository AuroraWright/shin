use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};

pub mod ini;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Game {
    #[default]
    Umineko,
    Higurashi,
    Gerokasu2,
    /// 幻想牢獄のカレイドスコープ (the first game); runs on the Gerokasu2 engine path.
    Gerokasu,
}

static CURRENT: std::sync::OnceLock<Game> = std::sync::OnceLock::new();

impl Game {
    /// Record the game chosen on the command line, for render code that has no
    /// asset context. Unset (as in tests) reads as the default game.
    pub fn set_current(self) {
        let _ = CURRENT.set(self);
    }
    pub fn current() -> Game {
        CURRENT.get().copied().unwrap_or_default()
    }
    /// Gerokasu and Gerokasu2 share one engine generation and app path.
    pub fn is_gerokasu_family(self) -> bool {
        matches!(self, Game::Gerokasu | Game::Gerokasu2)
    }
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
    #[clap(long, conflicts_with_all = ["save_file", "text_websocket"])]
    pub inspect_assets: bool,
    /// Route audio to the Loopback Audio system device for testing (debug builds only).
    #[cfg(debug_assertions)]
    #[clap(long, conflicts_with = "inspect_assets")]
    pub loopback_audio: bool,
    /// Read global progress and write saves to this engine-format file.
    ///
    /// Defaults to "shin/<game>/SAVEDATA" in the OS application data directory
    /// (e.g. ~/Library/Application Support on macOS, %APPDATA% on Windows).
    #[clap(long)]
    pub save_file: Option<PathBuf>,
    /// Search this directory for assets
    ///
    /// The directory must contain either a directory named "data" or a file named "data.rom".
    /// Defaults to "assets" next to the executable (inside a macOS app bundle, also
    /// Contents/Resources/assets).
    #[clap(short, long)]
    pub assets_dir: Option<PathBuf>,
    /// Serve displayed text over WebSocket at HOST:PORT (e.g. 127.0.0.1:6677)
    ///
    /// Every connected client receives each message box text as plain text (all games), plus
    /// Umineko's character, tip and purple-statement descriptions.
    #[clap(long, value_name = "HOST:PORT")]
    pub text_websocket: Option<String>,
}

impl Cli {
    /// Parse the command line, filling options it omits from `shin.ini`.
    pub fn load() -> Self {
        Self::parse_from(ini::merged_args())
    }
    /// The selected assets directory, or the default next to the executable.
    pub fn assets_dir(&self) -> PathBuf {
        self.assets_dir.clone().unwrap_or_else(default_assets_dir)
    }
    /// The selected save file, or `shin/<game>/SAVEDATA` in the OS application
    /// data directory (the working directory when the OS has none).
    pub fn save_file(&self) -> PathBuf {
        self.save_file.clone().unwrap_or_else(|| {
            default_save_file(dirs_next::data_dir().unwrap_or_default(), self.game)
        })
    }
    /// Loopback Audio output is a debug-build testing option.
    pub fn loopback_audio(&self) -> bool {
        #[cfg(debug_assertions)]
        return self.loopback_audio;
        #[cfg(not(debug_assertions))]
        false
    }
}

pub const SAVE_FILE_NAME: &str = "SAVEDATA";

/// `<data_dir>/shin/<game id>/SAVEDATA`, with the game's `--game` value as its id.
pub fn default_save_file(data_dir: PathBuf, game: Game) -> PathBuf {
    let id = game.to_possible_value().expect("every game is selectable");
    data_dir
        .join("shin")
        .join(id.get_name())
        .join(SAVE_FILE_NAME)
}
pub const ASSETS_DIR_NAME: &str = "assets";

/// Folders searched for `shin.ini` and the default assets directory: the
/// executable's own folder, then, inside a macOS app bundle
/// (`Name.app/Contents/MacOS/<executable>`), `Contents/Resources`.
pub fn executable_dirs(executable: &Path) -> Vec<PathBuf> {
    let Some(directory) = executable.parent() else {
        return Vec::new();
    };
    let mut dirs = vec![directory.to_path_buf()];
    if directory.ends_with("Contents/MacOS")
        && let Some(contents) = directory.parent()
        && contents
            .parent()
            .and_then(Path::extension)
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    {
        dirs.push(contents.join("Resources"));
    }
    dirs
}

/// The first existing "assets" folder among [`executable_dirs`], else the
/// one next to the executable.
pub fn default_assets_dir() -> PathBuf {
    let Ok(executable) = std::env::current_exe() else {
        return PathBuf::from(ASSETS_DIR_NAME);
    };
    let candidates: Vec<_> = executable_dirs(&executable)
        .into_iter()
        .map(|dir| dir.join(ASSETS_DIR_NAME))
        .collect();
    candidates
        .iter()
        .find(|path| path.is_dir())
        .or(candidates.first())
        .cloned()
        .unwrap_or_else(|| PathBuf::from(ASSETS_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_game_and_keeps_inspection_separate_from_launch_options() {
        assert_eq!(Cli::try_parse_from(["shin"]).unwrap().game, Game::Umineko);
        let cli = Cli::try_parse_from(["shin", "--game", "gerokasu2", "--inspect-assets"]).unwrap();
        assert_eq!(cli.game, Game::Gerokasu2);
        assert!(cli.inspect_assets);
        assert!(cli.higurashi_launch.is_none());
        assert!(Cli::try_parse_from(["shin", "--game", "unknown"]).is_err());
        for extra in [
            &["--save-file", "SAVEDATA"][..],
            &["--text-websocket", "127.0.0.1:9001"],
            &["--loopback-audio"],
        ] {
            let mut args = vec!["shin", "--inspect-assets"];
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_err(), "{extra:?}");
        }
        let cli = Cli::try_parse_from(["shin", "--text-websocket", "127.0.0.1:9001"]).unwrap();
        assert_eq!(cli.text_websocket.as_deref(), Some("127.0.0.1:9001"));
        assert_eq!(Cli::try_parse_from(["shin"]).unwrap().text_websocket, None);
        assert!(
            Cli::try_parse_from(["shin", "--loopback-audio"])
                .unwrap()
                .loopback_audio()
        );
        for removed in [
            "--load-save",
            "--save-slot",
            "--fast-forward-to",
            "--unsafe-entry-point",
        ] {
            assert!(
                Cli::try_parse_from(["shin", removed, "1"]).is_err(),
                "{removed}"
            );
        }
    }
    #[test]
    fn save_file_defaults_to_savedata_in_the_app_data_directory() {
        let cli = Cli::try_parse_from(["shin", "--assets-dir", "/games/umineko"]).unwrap();
        assert_eq!(cli.assets_dir(), Path::new("/games/umineko"));
        let data = dirs_next::data_dir().unwrap();
        assert_eq!(cli.save_file(), data.join("shin/umineko/SAVEDATA"));
        for (game, id) in [
            (Game::Umineko, "umineko"),
            (Game::Higurashi, "higurashi"),
            (Game::Gerokasu2, "gerokasu2"),
            (Game::Gerokasu, "gerokasu"),
        ] {
            assert_eq!(
                default_save_file("/data".into(), game),
                Path::new("/data/shin").join(id).join("SAVEDATA")
            );
        }
        let cli =
            Cli::try_parse_from(["shin", "-a", "/games/umineko", "--save-file", "p.dat"]).unwrap();
        assert_eq!(cli.save_file(), Path::new("p.dat"));
        let cli = Cli::try_parse_from(["shin"]).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(cli.assets_dir(), exe.parent().unwrap().join("assets"));
        assert_eq!(
            executable_dirs(Path::new("/b/Shin.app/Contents/MacOS/shin")),
            [
                Path::new("/b/Shin.app/Contents/MacOS"),
                Path::new("/b/Shin.app/Contents/Resources")
            ]
        );
        assert_eq!(executable_dirs(Path::new("/b/shin")), [Path::new("/b")]);
        assert_eq!(
            executable_dirs(Path::new("/b/Contents/MacOS/shin")),
            [Path::new("/b/Contents/MacOS")]
        );
    }
    #[test]
    fn shin_ini_fills_omitted_options_and_command_line_wins() {
        let folder = Path::new("/games/shin");
        let text = "\u{feff}; comment\n# comment\n[shin]\ngame = gerokasu\nassets-dir = \"assets dir\"\n\
                    save_file=/abs/progress.dat\nloopback-audio = false\n--text-websocket = 127.0.0.1:9001\n\
                    save-file-unused-check =\n";
        assert!(ini::to_args(text, folder, &[]).is_err(), "unknown key");
        let text = text.replace("save-file-unused-check =\n", "");
        let args = ini::to_args(&text, folder, &[]).unwrap();
        let mut argv = vec![std::ffi::OsString::from("shin")];
        argv.extend(args);
        let cli = Cli::try_parse_from(&argv).unwrap();
        assert_eq!(cli.game, Game::Gerokasu);
        assert_eq!(cli.assets_dir(), folder.join("assets dir"));
        assert_eq!(cli.save_file(), Path::new("/abs/progress.dat"));
        assert!(!cli.loopback_audio());
        assert_eq!(cli.text_websocket.as_deref(), Some("127.0.0.1:9001"));

        // Options on the command line replace the ini's; empty values stay unset.
        let explicit = ["game".to_owned(), "assets-dir".to_owned()];
        let mut argv = vec![std::ffi::OsString::from("shin")];
        argv.extend(
            ini::to_args(
                "game = gerokasu\nassets-dir = a\nloopback-audio = yes\nsave-file =\n",
                folder,
                &explicit,
            )
            .unwrap(),
        );
        argv.extend(["--game", "higurashi", "-a", "b"].map(Into::into));
        let cli = Cli::try_parse_from(&argv).unwrap();
        assert_eq!(cli.game, Game::Higurashi);
        assert_eq!(cli.assets_dir(), Path::new("b"));
        assert_eq!(
            cli.save_file(),
            default_save_file(dirs_next::data_dir().unwrap(), Game::Higurashi)
        );
        assert!(cli.loopback_audio());

        for bad in [
            "unknown = 1",
            "help = true",
            "load-save = save.dat",
            "game",
            "loopback-audio = maybe",
            "game = umineko\ngame = higurashi",
        ] {
            assert!(ini::to_args(bad, folder, &[]).is_err(), "{bad}");
        }
        assert_eq!(
            ini::candidates(Path::new("/b/Shin.app/Contents/MacOS/shin")),
            [
                Path::new("/b/Shin.app/Contents/MacOS/shin.ini"),
                Path::new("/b/Shin.app/Contents/Resources/shin.ini")
            ]
        );
        assert_eq!(
            ini::candidates(Path::new("/b/shin")),
            [Path::new("/b/shin.ini")]
        );

        // The packaged example parses as-is.
        let example = include_str!("../../shin.ini.example");
        let mut argv = vec![std::ffi::OsString::from("shin")];
        argv.extend(ini::to_args(example, folder, &[]).unwrap());
        let cli = Cli::try_parse_from(&argv).unwrap();
        assert_eq!(cli.game, Game::Umineko);
        assert_eq!(cli.assets_dir(), folder.join("assets"));
        assert_eq!(cli.save_file, None);
    }
}
