This is an unofficial re-implementation of the game engine used by some
[visual novels](https://en.wikipedia.org/wiki/Visual_novel) released by
[Entergram](http://www.entergram.co.jp/) on Nintendo Switch, PS4 and PS Vita.

Currently known games using the engine on Switch and PS Vita are listed
in [this spreadsheet](https://docs.google.com/spreadsheets/d/1wGX9FOQq_iXcWMnY9qITCAV7hq1R7_gpWwjkT4_tKDI/edit?usp=sharing).
PS4 was not researched yet.

The intent is to follow the original engine as closely as possible, so that it
can run the original games' data unchanged.

![screenshot.png](screenshot.png)

## Supported games

The engine runs the Switch releases of these games. Select one with `--game`:

| `--game` | Game |
| --- | --- |
| `umineko` (default) | Umineko no Naku Koro ni Saku \~Nekobako to Musou no Koukyoukyoku\~ (`01006A300BA2C000`), original release or with the 1.03 update |
| `higurashi` | Higurashi no Naku Koro ni Hō (the updated release, including the Origin content) |
| `gerokasu` | 幻想牢獄のカレイドスコープ (Gensou Rougoku no Kaleidoscope) |
| `gerokasu2` | 幻想牢獄のカレイドスコープ 2 |

Story playback, title menus, saving and loading, the backlog, settings and the
in-game menus are implemented for each game, along with its own features and
extras: Umineko's character screen, Tips, murderer quiz and galleries,
Higurashi's Chart, TIPS, Kakera and Quiz screens and galleries, and the
Gerokasu card selection minigame and galleries.

Some details still differ from the original games. If you find a problem,
please open an [issue on GitHub](https://github.com/DCNick3/shin/issues).

## Running a game

You need the game's romfs, which you can dump from your Switch with
[nxdumptool](https://github.com/DarkMatterCore/nxdumptool). With
[hactoolnet](https://github.com/Thealexbarney/LibHac) you can extract the romfs
from a dumped package, for example:

```bash
hactoolnet -t pfs0 "Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~ [01006A300BA2C000].nsp" --romfs=assets
```

The resulting folder contains `data.rom`. For Umineko, its SHA-256 should be
`6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591`.
For Umineko 1.03 and Higurashi, also place the update's `patch.rom` (and, for
Higurashi, the `append.rom` add-on content) in the same folder.

By default, the engine reads the `assets` folder next to the executable. Inside
a macOS app bundle, `Contents/Resources/assets` is also accepted. Use
`--assets-dir` to read another folder:

```bash
shin --game higurashi --assets-dir /path/to/higurashi
```

### Command line options

| Option | Description |
| --- | --- |
| `--game <GAME>` | The game to run: `umineko` (default), `higurashi`, `gerokasu` or `gerokasu2`. |
| `-a`, `--assets-dir <DIR>` | Folder containing `data.rom` (or a `data` folder). Defaults to `assets` next to the executable. |
| `--save-file <FILE>` | Save file. Defaults to `shin/<game>/SAVEDATA` in the system application data folder. |
| `--text-websocket <HOST:PORT>` | Serve the displayed text over WebSocket, for example to a text hooker or translation tool. |
| `--inspect-assets` | Print the scenario and asset information without opening a window or touching saves. |

The default save file is in:

- macOS: `~/Library/Application Support/shin/<game>/SAVEDATA`
- Windows: `%APPDATA%\shin\<game>\SAVEDATA`
- Linux: `~/.local/share/shin/<game>/SAVEDATA`

Progress, settings, gallery unlocks and save slots are all stored in this one
file. It is written atomically. If a write fails, the game offers to retry.

### shin.ini

A `shin.ini` file next to the executable (or in `Contents/Resources` inside a
macOS app bundle) can set any of the options above, one per line, using the long
option name without `--`:

```ini
game = higurashi
assets-dir = higurashi
text-websocket = 127.0.0.1:9001
```

Lines starting with `;` or `#` are comments, an empty value leaves an option
unset, and relative paths are resolved from the folder containing `shin.ini`.
Options given on the command line take precedence. See
[`shin.ini.example`](shin.ini.example).

### Mods and translations

Every game loads an optional `mod.rom` from the assets folder before all other
archives, so a mod only needs to contain the files it replaces.

Every game also reads an optional UTF-8 `localized.toml` from the assets folder
to translate the engine's own UI text (dialogs, menus, labels). Each entry maps
an original string to its replacement:

```toml
"確認" = "Confirm"
"達成率:{percent}％" = "Completion: {percent}%"
```

The [localization](localization/) folder has the full list of strings for
[Umineko](localization/umineko.localized.toml.example),
[Higurashi](localization/higurashi.localized.toml.example) and
[Gerokasu2](localization/gerokasu2.localized.toml.example), plus English
translations for [Umineko](localization/umineko.en.toml) and
[Higurashi](localization/higurashi.en.toml). Scenario text is not affected.

### Text WebSocket

With `--text-websocket 127.0.0.1:9001`, every connected client receives each
displayed message as plain text. For Umineko, character, Tips and
purple-statement descriptions are sent as well. A client that connects later
first receives the most recent text.

## Controls

The games are played like on the Switch, with a controller, the mouse or
trackpad (taps, drags and swipes act as touch input), or the keyboard. The
keyboard maps to controller buttons the same way on every screen:

| Key | Controller |
| --- | --- |
| Arrow keys | D-pad / left stick |
| Enter | A (confirm, advance text) |
| Escape | B (cancel, back) |
| B | X |
| M | Y |
| H / J | L / R |
| K / L | Right stick up / down (scroll) |
| U / I | Minus / Plus |
| F | Toggle fullscreen |
| Left Ctrl (hold) | Skip text while held |

The mouse wheel and trackpad scroll wherever the right stick does.

## What else is in the box

Aside from the game engine, this repo also includes `shin-core`, a library for
working with the game data, and `sdu`, a command line interface to it. `sdu` can
extract game data and convert it to more conventional formats:

- `.rom` - Game data archive
- `.bup` - Character sprites
- `.pic` - Backgrounds & CGs
- `.nxa` - Game audio
- `.snr` - Game scenario (execution tracing, no decompilation)
- `.fnt` - Font data
- `.txa` - Texture archives (used mostly for UI)

Documentation of the engine's formats is collected in
[this spreadsheet](https://docs.google.com/spreadsheets/d/1BGGZ1Wjnx5FXgjDjhKgwhRxSAY_HYX_6U0q679IVUi8/edit?usp=share_link).
The autogenerated API docs can be found [here](https://dcnick3.github.io/shin/shin/).

## Building from source

Install [Rust](https://www.rust-lang.org/); the toolchain version is pinned in
`rust-toolchain.toml`. Then build and run the engine with:

```bash
cargo build --release -p shin
cargo run --release -p shin -- --game umineko --assets-dir /path/to/assets
```

Debug builds also accept `--loopback-audio`, which sends audio to a system
output device named `Loopback Audio`. The hardware audio tests require that
device.

## Disclaimer

This project is not affiliated with Entergram or 07th Expansion. It is an
unofficial fan project.

It does not contain copyrighted game data, so you can't play the games if you
didn't buy them.
