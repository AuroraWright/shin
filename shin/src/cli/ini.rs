//! `shin.ini`: the command line options as `key = value` lines, read from the
//! executable's folder (and, inside a macOS `.app`, `Contents/Resources`).
//! Options given on the actual command line take precedence.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::CommandFactory;

use super::Cli;

pub const FILE_NAME: &str = "shin.ini";

/// Options whose relative values resolve against the ini's folder rather than
/// the working directory.
const PATH_OPTIONS: &[&str] = &["assets-dir", "save-file"];

/// `shin.ini` candidates, most specific first.
pub fn candidates(executable: &Path) -> Vec<PathBuf> {
    super::executable_dirs(executable)
        .into_iter()
        .map(|dir| dir.join(FILE_NAME))
        .collect()
}

/// Long names of the options present on the command line.
fn explicit_options(args: &[OsString]) -> Vec<String> {
    let command = Cli::command();
    let mut names = Vec::new();
    for arg in args {
        let Some(arg) = arg.to_str() else { continue };
        if arg == "--" {
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            names.push(long.split('=').next().unwrap_or(long).to_owned());
        } else if let Some(short) = arg.strip_prefix('-').and_then(|s| s.chars().next())
            && let Some(option) = command
                .get_arguments()
                .find(|a| a.get_short() == Some(short))
            && let Some(long) = option.get_long()
        {
            names.push(long.to_owned());
        }
    }
    names
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Convert ini text to command line arguments, skipping options in `explicit`.
pub fn to_args(text: &str, folder: &Path, explicit: &[String]) -> anyhow::Result<Vec<OsString>> {
    let command = Cli::command();
    let mut args = Vec::new();
    let mut seen = Vec::<String>::new();
    for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let number = number + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("line {number}: expected `option = value`, found `{line}`");
        };
        let key = key
            .trim()
            .trim_start_matches("--")
            .replace('_', "-")
            .to_ascii_lowercase();
        let mut value = value.trim();
        for quote in ['"', '\''] {
            if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
                value = &value[1..value.len() - 1];
            }
        }
        let Some(option) = command
            .get_arguments()
            .find(|a| a.get_long() == Some(key.as_str()))
            .filter(|a| !matches!(a.get_long(), Some("help" | "version")))
        else {
            bail!("line {number}: unknown option `{key}`");
        };
        if seen.contains(&key) {
            bail!("line {number}: `{key}` is set more than once");
        }
        seen.push(key.clone());
        if explicit.contains(&key) {
            continue;
        }
        if option.get_action().takes_values() {
            if value.is_empty() {
                continue;
            }
            args.push(format!("--{key}").into());
            if PATH_OPTIONS.contains(&key.as_str()) {
                args.push(folder.join(value).into_os_string());
            } else {
                args.push(value.into());
            }
        } else {
            match parse_bool(value) {
                Some(true) => args.push(format!("--{key}").into()),
                Some(false) => {}
                None => bail!("line {number}: `{key}` takes true or false, found `{value}`"),
            }
        }
    }
    Ok(args)
}

/// Program name, then the first `shin.ini` options, then the command line.
pub fn merged_args() -> Vec<OsString> {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    let program = if args.is_empty() {
        "shin".into()
    } else {
        args.remove(0)
    };
    let mut merged = vec![program];
    let ini = std::env::current_exe()
        .ok()
        .into_iter()
        .flat_map(|exe| candidates(&exe))
        .find(|path| path.is_file());
    if let Some(path) = ini {
        let loaded = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))
            .and_then(|text| {
                let folder = path.parent().unwrap_or(Path::new("."));
                to_args(&text, folder, &explicit_options(&args))
                    .with_context(|| format!("Invalid {}", path.display()))
            });
        match loaded {
            Ok(options) => {
                tracing::info!("Using {} ({} arguments)", path.display(), options.len());
                merged.extend(options);
            }
            Err(error) => {
                eprintln!("error: {error:#}");
                std::process::exit(2);
            }
        }
    }
    merged.extend(args);
    merged
}
