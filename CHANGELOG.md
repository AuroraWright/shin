# Changes

Downloads for every version are on the [releases page](https://github.com/AuroraWright/shin/releases).

## 1.0.7-fork (2026-10-28)

- `-f` / `--fullscreen` (or `fullscreen = true` in `shin.ini`) starts the game in fullscreen.
- Higurashi renders at the window's size like the other games, instead of always at 1080p and scaling the
  picture, so text and images stay sharp in large and fullscreen windows.
- A right click acts as B on every screen in all four games. In Umineko and the Gerokasu games it used to only
  hide the message window.
- Skipping no longer stalls when scenes change quickly (most noticeable in Gerokasu2): the images a scene no
  longer shows are released in the background. Drawing also takes less CPU time.

## 1.0.6-fork (2026-10-26)

- Sound is resampled with a band-limited filter, so the games' 48 kHz audio no longer distorts on a 44.1 kHz
  output device.
- Audio is mixed in the Switch's 5.3 ms blocks, so sounds start, stop and change with the original timing.
- Releases are no longer marked as pre-releases on GitHub.

## 1.0.5-fork (2026-10-25)

- The game keeps running and drawing while its window is in the background. Only a fully covered or
  minimised window stops drawing, and the game keeps playing meanwhile. Focus only decides whether input
  reaches the game.
- Waking the computer from sleep no longer makes the game jump ahead.
- The config's 自動スリープの抑制 option no longer changes anything (it is still saved).
- macOS: controllers are read through the system's GameController framework, so a PlayStation or Xbox
  controller keeps working after another app has used it.

## 1.0.4-fork (2026-10-20)

- Windows: Umineko no longer crashes with a stack overflow after the opening movie
  ([#2](https://github.com/AuroraWright/shin/issues/2)).
- Windows: the window opens centred in the space the taskbar leaves free.

## 1.0.3-fork (2026-10-18)

- Linux: fixed the crash at startup, "Unable to find entry point 'vertex_main'"
  ([#1](https://github.com/AuroraWright/shin/issues/1)).
- Linux: parts of the game, such as Higurashi's first-launch notice, are no longer drawn upside down.
- The window prefers an 8-bit colour format, and the format it picks is logged.
- Higurashi: a left swipe or mouse drag no longer closes the pause menu, as on the Switch.
- Gerokasu and Gerokasu2: a slow left mouse drag closes the pause menu, like the quick touch swipe already did.

## 1.0.2-fork (2026-10-17)

First fork release.

- Four games from their Switch data, chosen with `-g` / `--game`: Umineko no Naku Koro ni Saku (original
  release or with the 1.03 update), Higurashi no Naku Koro ni Hō (the updated release, with the Origin
  content), and 幻想牢獄のカレイドスコープ 1 and 2.
- Story playback, title menus, saving and loading, the backlog, the config, the in-game menus and each game's
  extras: Umineko's character screen, Tips, galleries and murderer quiz, Higurashi's Chart, TIPS, Kakera and
  Quiz screens and galleries, and the Gerokasu card selection minigame and galleries.
- Progress, settings, gallery unlocks and save slots are stored in one save file, by default in
  `shin/<game>/SAVEDATA` in the system's application data folder.
- Controller, mouse or trackpad (as touch) and keyboard controls. The keyboard maps to the same controller
  buttons on every screen; F toggles fullscreen and holding left Ctrl skips text.
- Mods and translations: an optional `mod.rom` replaces game files, and a `localized.toml` translates the
  engine's own UI text. English UI translations converted from the Umineko and Higurashi fan translations are
  included.
- `-t` / `--text-websocket` serves the displayed text to text hookers.
- Settings in a `shin.ini` next to the executable.
- Movies decode through the system (VideoToolbox, Media Foundation or GStreamer), with an `ffmpeg` executable
  as a fallback.
- Every shader is compiled in the background at startup, so menus don't stutter the first time they open.
- Builds for macOS, Windows and Linux.

The versions below are [DCNick3's releases](https://github.com/DCNick3/shin/releases) from before the fork.

## 0.8.1 (upstream)

This version only contains some improvements to savedata handling in sdu.

- Fix a bug where some savefiles were not decoded promerly due to several missing `.align` calls
- Update naming of some savefile fields to reflect latest RE
- Implement savefile encoding, accessible via `sdu save encode`. This allows one to edit the decoded savefile and to
  insert it back into the game.

## 0.8.0 (upstream)

- Implement wipes. Now instead of flickering on, the scene changes are animated, like in the original implementation.
- Implement novel mode messagebox.
- Implement LayerGroup rendering with masks (MASKLOAD command).
- Now we release builds for aarch64 linux (because why not).

- Add fallbacks from SPIR-V shaders to wgsl ones, allowing us to run on platforms not supporting SPIR-V shaders.
- Rewrite `shin-tasks`. Now it is no longer a fork of `bevy_tasks`, but a much simpler library setting up rayon &
  async-executor tasks pools. May support wasm in the future.

- Relax looping assertion in shin-core to allow more NXA files to be decoded.
- Add a new bustup extraction mode into `sdu`: `bake`. It will combine all the bustup parts, allowing to get a sprite
  that looks close to its in-game appearance without fiddling with files.
  This representation is, however, very inefficient in terms of storage space.

## 0.7.0 (upstream)

This is a release that contains big rewrites upder the hood. They might not improve compatibility that much initially (I
would expect the compat to be worse, actually), but will make implementing some engine features possible at all.

- Rewrite of the rendering engine to actually use the shaders/render passes in an equivalent to the way original game
  does it.
- Addition of `shin-window`: a common framework for starting a winit window, initializing wgpu and handling input. This
  makes test apps (like `shin-video`'s play example) easier to maintain. This is also where most of the web support will
  be contained.
- Rewrite of the input handling to somewhat resemble what the original game does.
- Rewrite of the `MessageLayer` to include all the features the original game has. Now we have messagebox sliding
  animations, overflow handling, keywait animations and voice support (though this needs still needs implementation from
  the audio engine side).
- Rewrite of the `LayerGroup`, `PageLayer` and `ScreenLayer` classes, which are much closer to what the game does.
- Stub implementation of `NewDrawableLayer` framework, paving the way for implementing various funny effects the scrip
  uses sometimes.
- Rewrite of `PictureLayer` and `BustupLayer` to no longer use stitched together textures (RIP), but do it the same way
  game does, by rendering in blocks. This indirectly fixes the ugly face seams we were getting due to some rounding
  bugs.

## 0.6.1 (upstream)

This release adds support for getting raw opus audio from nxa files to `sdu`. This allows preserving audio data without
additional re-encoding losses.

It also updates various dependencies of the engine, but this shouldn't change the visible functionality.

## 0.6.0 (upstream)

This is just an update of the changes accumulated over they year. This doesn't improve compat, mostly small internal
changes

- Add a very basic linear scenario disassembler. The output format is not stable yet, probably should work on making it
  compatible with what `shin-asm` expects.
- Implement & expose in sdu `shin-asm`: a way-too-much over-engineered assembler for SNR files. It is still largely
  unfinished and can assembly only very basic files.
- Add an optional gstreamer backend to `shin-video`, allowing for hardware-accelerated decoding. Not build by default,
  unsure on how to distribute on windows yet.
- Use typed `NumberSpec` values, which will get lowered to a more concrete type than `i32` when computed.
- Various dependency updates.

## Before that (upstream)

Before this, I did not keep any changelog, sorry. Looking at the commits is the best you will get
