This is an attempt at an unofficial re-implementation of game engine used by
some [visual novels](https://en.wikipedia.org/wiki/Visual_novel) released by [Entergram](http://www.entergram.co.jp/) on
Nintendo Switch, PS4 and PS Vita.

Currently known games using the engine on Switch and PS Vita are listed
in [this spreadsheet](https://docs.google.com/spreadsheets/d/1wGX9FOQq_iXcWMnY9qITCAV7hq1R7_gpWwjkT4_tKDI/edit?usp=sharing).
PS4 was not researched yet.

If you know more - let me know! (`dcnick3` on Discord)

## Status

The initial implementation is focused on running switch version of Umineko no Naku Koro ni Saku \~Nekobako to Musou no
Koukyoukyoku\~ (title id `01006A300BA2C000`), with the intention to support other games in the future.

As of writing, most of the basic game functionality works:

- Character sprites
- Backgrounds
- BGM & SFX
- Text

However, there are still a lot of missing advanced features. It's not fully playable yet.

The intent is to follow the engine as closely as possible, so that it can be used to run the original games.

![screenshot.png](screenshot.png)

## Try it

You can try out the engine by downloading a pre-built binary from
the [releases](https://github.com/dcnick3/shin/releases) page.

Select the latest release, and download the `shin` archive for your platform and extract it somewhere.

Then you would need to have romfs of Umineko no Naku Koro ni Saku \~Nekobako to Musou no Koukyoukyoku\~ game (title id
`01006A300BA2C000`).

It can be dumped from the switch using [nxdumptool](https://github.com/DarkMatterCore/nxdumptool).

You can use [hactoonet](https://github.com/Thealexbarney/LibHac) to dump the romfs from the game:

```bash
hactoolnet -t pfs0 "Umineko no Naku Koro ni Saku ~Nekobako to Musou no Koukyoukyoku~ [01006A300BA2C000].nsp" --romfs=assets
```

In the `assets` directory you should find a `data.rom` file. It's sha256sum should be
`6d90eb0bacacf769a7e4634407622b047acd711c47debb28136d7bab3fd0e591`.

You can put then put the `assets` directory next to the executable or another location
documented [here](https://dcnick3.github.io/shin/shin/asset/locate/fn.locate_assets.html)

Then you try running the game.

Controls are:

- Mouse click, space, enter - advance text
- Ctrl - fast-forward text while held, with the skip indicator
- S - toggle skip mode
- A - toggle auto mode
- Escape, Backspace - cancel skip and auto modes
- Controller L/R during story playback - toggle auto/skip independently (Switch manual pages 4 and 6)
- Event scenes allow mode selection while blocking skip/auto execution
- Waiting murderer menus clear skip/auto modes and block their selection
- Replay and hint responses wait for their native confirmation phases
- Replay: Up/Down switch rows; Left/Right wrap within the row
- Hints: Up/Down select unlocked hints without wrapping
- Hints: K/L scroll up/down; use the right stick or drag the list to scroll; releasing a drag preserves momentum
- Purple statements: person directions follow the grid and skip empty transcripts; chapter Up/Down stop at the ends and scroll the selection into view over four ticks; clicking a chapter heading scrolls to its section while preserving the selected statement; the right stick scrolls the transcript; drag the chapter list or transcript to scroll, then release to coast
  Person pointer presses select the transcript silently and brighten the portrait; release confirms with a cue. Empty and currently selected portraits ignore fresh presses.
- Accusation: directional controls follow the suspect grid, including diagonals and Submit; direction consumes simultaneous confirm/cancel even at an edge
  Pointer press selects and brightens; release toggles the suspect or submits. Hover retains selection, and leaving cancels the press.
- Murderer top menu: Up/Down wrap; movement consumes simultaneous confirm. Confirmation flashes before staged submenu wipes; input resumes after the wipe. Submenu Back flashes, wipes to Top, and restores its focus
  Pointer presses select and brighten a row; release confirms. Leaving the menu
  cancels the press. Passive hover keeps the current selection. Replay cards and
  submenu Back buttons also confirm on release and retain held brightness.
  Hint rows use the same release confirmation; changing selection animates focus
  and scrolls just enough to keep the row visible.
- While skip or auto is active, controller B/X/Y cancels the modes first
- Up/down arrows and enter, or mouse - choose an option
- F - toggle fullscreen
- H/J: previous/next in Character, Character sprite gallery, Help, Save/Load and Picture/Music galleries; Start tabs and Backlog navigation
- K/L: scroll Character descriptions, Tips, and Murderer text/hints
- Character screen: H/J switch pages; K/L scroll the description

With 「タッチ操作時の画面効果」 enabled in Config, left mouse presses and
held drags show the native trailing stars. Turning it off stops new stars;
existing stars fade after release.

During story movies and automatic event sequences, Escape/controller B skips to
the event's end. A short pointer tap shows 「ロングタップでスキップできます」
at the bottom left; hold for 1.15 seconds to skip. Dragging or losing input cancels
the hold. Enter/controller A shows the B-button hint. Title and gallery movies
retain their existing tap-to-skip controls.

Press keyboard B or controller X during the story to open the message browser.
Up/Down select a row,
Left/Right move four rows, H/J or controller L/R jump between choices,
quizzes and checkpoints, and the mouse wheel scrolls. Enter or V replays the
selected message's voice; its voice icon activates on mouse release. Moving off
a pressed row or close button cancels that click. Drag vertically to scroll;
a quick release continues scrolling with momentum. An active mouse gesture takes
priority over keyboard input. Escape or releasing the close button closes the
browser after its flash and slide. The story stays paused while it is open.

Y, releasing a message's text body, or a quick leftward fling opens a scene-jump
confirmation. The default answer is No. Select Yes to restore the selected
message; the browser stays paused until its closing animation and asset loading
finish. Messages later in a previously read route can also be selected.

G opens the chapter/scene-jump menu. Arrows select a marker, H/J select
the first/last marker, G returns to the cursor captured when the menu opened,
and Y opens confirmation for the selected message. Escape returns to the
browser with that selection. Controllers use X for this menu, Y for confirmation,
A for voice replay, B to cancel, and L/R for first/last markers. Click a marker
to select it; dragging scrolls with momentum. Pressing a moving row stops the
momentum without selecting it. A quick rightward fling or the close button
returns to the browser. Momentum continues under confirmation; the accepted
jump uses the browser row selected when the dialog finishes.

When the script opens the Characters screen, arrows move between portraits,
H/J change pages, Enter cycles characters sharing a portrait cell,
Tab cycles character states, and V changes the outfit. The state and outfit
buttons also accept mouse clicks. K/L, Home/End or the mouse wheel scroll the
character description; dragging the description supports momentum after release.
Mouse buttons activate on release, and dragging cancels the click. Escape or the close button closes the screen after its
flash and closing animation; the script waits until it has closed. Script-only
character grids keep their authored states and outfits.
Controllers use the Switch layout shown in the footer: D-pad/left stick navigates,
L/R changes pages, A cycles stacked characters, Y changes state, X changes outfit,
B closes, and the right stick scrolls descriptions.

Skip and auto mode pause at choices and during scripted events. Skip also pauses
at unread messages when the saved setting requires it (enabled by default).
Choice menus wait for their entry animation before accepting input. Use
Up/Down to navigate and Left/Right to select the first/last row. Enter or controller
A confirms; pointer presses highlight a row and releases confirm. The script
resumes after the confirmation flash and exit animation.
Completing a message records its read status. Saved text speed, auto-wait speed,
voice muting and the voice-on-click setting are applied. Saved BGM, effects,
voice and system-sound volumes multiply the script-controlled gains. System cues
capture that volume when they start and continue across scene restoration. When voice
focus is enabled in the save, BGM ramps to half volume during voice playback
and recovers afterward. The script can
also disable these system controls with `SYSCALL 0`.
Music and sound effects load asynchronously, and audio waits include pending loads.
Volume/pan changes survive loading. Matching music requests and repeating effect
requests reuse their original sources. Paused effects wait to begin playback until
resumed. Scene restoration keeps matching BGM and repeating effects at their
current playback positions and applies the restored volume, pan and speed.
One-shot effects continue, while positive effect slots absent from the save stop
with the native four-tick fade. New music titles and gallery unlocks appear when
a request is accepted.
Right-click or press H while a message or choice menu is visible to hide it and
the route/music overlays; on a controller, press B. Controller A/B/X/Y restore
the hidden UI. Click, press Enter, Escape, or H to restore them; restoring
the UI consumes that input without advancing the message.
Press M or controller Y to open the system menu. Manual, Characters, Information,
and Config return to the menu after closing. In Config, use controller L/R or keyboard K/L
to change tabs, and Y to reset settings. Changed settings are saved when Config closes.
Save and Load use the manual-slot screen, with Delete or controller Y to delete
a slot. Slots remain in memory unless a save file is configured. Title return
asks for confirmation and saves globals on Yes before opening Title.
No or Cancel returns to the system menu.
Chapter autosaves display the native bottom-left route label, and new music
tracks display their animated title. Both overlays honor the saved display settings.
Press Up (D-pad or left stick), Arrow Up, or Backspace during normal story playback
to return to the previous message. Holding Up repeats the request; menus retain
their Up navigation.
After text is closed, it returns to the last message instead. Quick return requires
at least two normal messages and uses a four-tick fade with a frozen scene image.
Backspace keeps its cancel behavior in menus and dismisses a hidden message first.

To load an existing Umineko engine-format save, start with
`shin --load-save save.dat`. This selects the autosave slot. Use
`--save-slot 1` through `--save-slot 100` for a manual slot. Loading reconstructs
the script from its recorded decisions and restores scene layers, masks, message
text, and repeating audio. It cannot be combined with the debugging entrypoint
or fast-forward options.

The script's `RESUMESET` records a return boundary. `RESUME` loads that boundary
through scene restoration, keeping current global progress and replaying the
recorded route up to the bookmark. Later reads and choices can replace the old
route suffix. An unset bookmark restarts at the scenario entrypoint. The backlog
stops at `RESUME` instead of collecting unreachable commands beyond it.
Active chapter/music labels and notifications survive `RESUME`; their animations
continue while loading blocks script input. A hidden UI remains dismissible after
restoration. Ordinary save/history loads retain active chapter and music labels
while they play their native exit animations. New labels created by the restored
command replace the old ones. Pending notifications are cleared on ordinary loads;
the active toast fades out over ten ticks. RESUME retains both active and queued
notifications.
While scene resources load or installation waits for audio capacity, the current
scene's layers, audio players, labels and notifications continue updating.
Script execution and input remain blocked while reconstruction is pending.
Ordinary save and backlog loads then fade the frozen previous scene out over
30 ticks. Script execution and input remain blocked through that fade;
`RESUME` installs immediately, and quick return uses four ticks.
Interrupted page wipes retain independently timed source transitions. Each page
and wipe advances once per frame; completed inner wipes collapse to their target
page, and the outer wipe releases its source textures when it finishes. Skipping
forwards both source and target pages while honoring property skip restrictions,
and completes nested wipes. The following update retires the finished transition.
Immediate page switches release the previous source tree. The engine's fresh-page
path starts with empty planes and default page properties.

Use `--save-file .shin/save.dat` to read global progress on startup and write
committed globals, choice history, gallery unlocks and autosave snapshots. To
resume from that file, also pass `--load-save .shin/save.dat`. Settings, manual
slots are preserved. Play time advances across story loading and menus, restores
from global save data, and is included in progress writes and normal shutdown.
Writes replace the file atomically. Failed writes are logged and retried at the
next progress change or shutdown. If the final shutdown write fails, the window
stays open with Retry and Cancel: Retry saves again before closing, while Cancel
returns to the same scene. Closing waits for an existing save task and its notice
to finish. Without `--save-file`, progress stays in memory.

Normal launches play the opening movie and then open Title. Enter, Escape,
controller A/B, or a pointer tap skips the opening with its native fade. Title's
Start opens episode selection. Manual opens the original paged viewer over Title
and returns to the same menu. Config edits global settings and saves changes
on close. Load opens the original manual-save menu and restores the selected
entry after confirmation. Explicit `--load-save`, `--unsafe-entry-point`,
and `--fast-forward-to` launches enter the requested story path directly.
FirstBoot loads common assets asynchronously in the native order while showing
black, then reads global progress before starting the movie. ADV's menu atlas
loads when entering a story scene. Native platform save fallback/error dialogs
remain incomplete.

Title's Characters and Tips entries show the unlocked episode chooser and open
the original viewer for the selected episode. Closing the viewer returns to the
chooser; Escape or a tap on its blank background returns to the Title menu.
Tips covers episodes 1–7; Characters also includes episode 8 when unlocked.

Title's CG entry opens the original gallery and returns with CG selected.
Arrow keys select entries; Enter opens unlocked variants. In the viewer, Enter
advances through unlocked variants, Escape closes, V fits the picture to the
screen, and Tab toggles variant markers. Arrow keys and the left stick pan;
pointer dragging also pans. Loading and capture fades gate viewer input.

Title's Movie entry opens the original ten-movie gallery. Arrow keys select a
thumbnail, Enter plays an unlocked movie, and Cancel skips playback or closes
the gallery. Pointer taps select/play thumbnails or use the close button. Locked
movies show the original logo overlay and give error feedback. Loading and fades
block gallery input during playback; returning to Title selects Movie again.

Title's BGM entry opens the original music gallery and returns with BGM selected.
It retains the title frame while artwork loads, then fades into the gallery.
Enter/controller A plays the selected unlocked track; Y/controller Y stops it.
Tapping a track toggles playback. Directional input, analog scrolling and pointer
dragging cover the two-column list; Escape/controller B or the close button
returns to Title. Global audio, volume settings and save progress survive the
roundtrip. Clipped rows, footer hints and completion text use the original assets.
Playback supports the gallery's numeric repeat counts: zero loops indefinitely,
one plays once, and larger counts finish with the file's full tail after the final
loop. Original and 1.03 native worker comparisons cover the count and boundary
behavior. Exact native completion-text glyph placement remains unverified.

Bounded original and 1.03 tests cover loading during unfinished movies, a default
wipe and pending picture, mask, BGM and SE requests. They verify input and VM
gating, retired owners and requests, and the frozen 30-tick load fade. Native
asynchronous timing and broader combinations remain unverified. See
`reverse-engineering/active-save-restoration-validation.json`.

The one- and two-entry save codec matches 128 complete native packets per
executable. Encrypted progress rewrites retain both entries. The live VM now
preserves the inactive entry through new autosaves, manual saves, resume and
load/rollback handoffs. Native copy/autosave routines pass 512 captures per
executable; regular regressions and six real-resource loads per ROM pass.
The saved-scene factory, native VM constructor, checkpoint clone, RESUME
preparation and destructor pass 2,176 packet comparisons per executable. Both
entries survive those tested transitions; RESUME changes the active boundary
without popping the inactive entry. See
`reverse-engineering/native-vm-save-lifecycle-validation.json`.

The complete native SaveGame worker produces 128 encrypted files per executable
that match our serializer byte for byte. Native pending/final callbacks and 256
manual-save success/failure callbacks per version also pass. Desktop explicit
Save/Delete, Config close and title/EXIT now capture the complete save before
opening the native no-button updating dialog. Its opening starts a background
writer; successful writes take at least one second of wall time. Control returns
only after the closing animation, cooldown and final owner callback. Shutdown
joins the writer before saving newer globals; failed manual writes restore both
entries and the remembered row. Successful writes retain thumbnails prepared
while saving. Native dialog updates and tween queues match 9,088 frames per
executable. Secondary-context origin and complete platform scheduling/error
handling remain unverified. See
`reverse-engineering/native-save-write-worker-validation.json` and
`reverse-engineering/native-save-dialog-timing-validation.json`.

Saved-scenario resource tasks pass 768 constructor/update/destructor cases per
executable, including publication before retiring the old VM and destruction
before completion. Hydration tasks pass 1,024 ownership cases per executable,
with 16,640 prepared layer/mask reference releases and empty/cancelled worker
paths. These checks execute native CPU bodies with guarded scheduler, resource
and condition interfaces. Desktop scene replacement and shutdown now wait for
loader cancellation to finish; real-resource replacement, shutdown and backlog
failure recovery pass for both ROMs. The four extra worker slots hold plane-mask
textures; audio is restored later by task update. See
`reverse-engineering/native-save-resource-task-validation.json` and
`reverse-engineering/native-save-hydration-task-validation.json`.

The complete native hydration worker and retained-layer lookup pass 1,280 cases
and 61,440 layer-slot comparisons per executable with Null/invalid Quiz requests
and guarded cached properties. Quick return and RESUME now consume the first
live layer with a matching load counter, retaining its drawable and property
owner. Actual Movie playback and Quiz selection/model ownership survive both
transitions in original/1.03 hardware checks; ordinary saved loads create fresh
resources. Real native asset loading, full replay integration, true concurrent scheduling
and secondary-context creation remain unverified. See
`reverse-engineering/native-save-hydration-worker-validation.json`.

The full native hydration update passes 1,280 task profiles and 1,981,440
property-value comparisons per executable with prepared scene/UI/audio services.
Seven container property owners keep their identifiers, counters, frame markers
and oscillator state while the update resets their 90 values. Loaded child
identifiers/counters, mask/audio service calls, Saved/History/RESUME gates and
publication before retiring the old VM are checked. The port now retains these
container property owners after validating the candidate command. Native asset
IO, full replay integration, concurrent scheduling and secondary-context origin remain
unverified. See `reverse-engineering/native-save-hydration-update-validation.json`.

Native saved replay against each ROM's actual script matches port reconstruction
for 48 cases per executable: Episodes 1/8, three seeds and eight boundaries up to
100. The comparisons cover 37,260 integer property values, loaded Tile/Picture/
Bustup descriptors, audio descriptors and saved command/input/message state.
Native checkpoint and VM cleanup balance; seven code mutations per executable
are detected. This CPU fixture prepares the scenario resource header externally
and does not load or decode assets, exercise positive plane masks, or run
concurrent workers. See `reverse-engineering/native-save-replay-validation.json`.

All 30 supplied MSK assets match the original and 1.03 native loaders byte for
byte: 11,911,392 decoded texels and 8,210 rectangles across both versions.
Native ROM lookup, construction, geometry, decompression, texture row upload
and destruction execute with checked host file reads and CPU storage services.
Native allocations and references balance; four code mutations per executable
are detected. This checks the production geometry builder without running a GPU
driver or integrating asynchronous saved-scene hydration. See
`reverse-engineering/native-mask-load-validation.json`.

Native saved replay now reaches actual mask hydration in 23 focused cases per
executable, using an isolated four-plane script with each ROM's real metadata.
The worker loads 40 real masks per version; plane IDs/flags and decoded pixels
match port reconstruction, and geometry matches the earlier byte comparisons.
Four threaded cases destroy the task while a file request is blocked. Native
destruction cancels and waits before retiring the worker's VM and masks. Native
plane ownership also retains masks through task destruction until plane clear.
Seven code mutations per executable are detected. The cache, string, pthread
wait and NVN services are checked host contracts; other resource loaders and
full platform scheduling remain unverified. See
`reverse-engineering/native-save-mask-hydration-validation.json`.

Desktop saved-mask publication now passes seven real loads and 28 plane-state
checks per ROM through `Adv::load_game_data`. The four installed R8 textures
contain 2,413,816 texels per version, all matching native decoded data byte for
byte through independent GPU readback. Exact prepared owners, pending/fade VM
gates, mask-free replacement and prepared-worker cancellation/shutdown pass.
This uses the same synthetic native script with actual metadata; complete
masked composites, mixed asset failures and native platform scheduling remain
outside the proof. See
`reverse-engineering/desktop-saved-mask-publication-validation.json`.

The manual release launches through `Shin.app`. On 2026-10-10 the user confirmed
that intermittent activation, AppKit hide/refocus and the collection-mutation
crash have not recurred since switching to app bundling; these issues are
resolved. Launch-activation retries, explicit AppKit fullscreen policy and their
opt-in feature have been removed. Earlier controlled checks are retained in
`reverse-engineering/macos-bundle-default-policy-validation.json` as history.
The F fullscreen shortcut responds to fresh key-down events and ignores held
repeats, so a missed release during a native transition cannot block the next
press. See `reverse-engineering/fullscreen-f-keydown-validation.json`.

If you encounter any problems, please open an [issue on GitHub](https://github.com/DCNick3/shin/issues).

## Docs

While there is no comprehensive documentation of the engine, as I reverse engineer it, I try to document the findings
mainly
in [this spreadsheet](https://docs.google.com/spreadsheets/d/1BGGZ1Wjnx5FXgjDjhKgwhRxSAY_HYX_6U0q679IVUi8/edit?usp=share_link)

The autogenerated docs can be found [here](https://dcnick3.github.io/shin/shin/).

## What else is in the box

Aside from the game engine, this repo also includes `shin-core` - a library for working with the game data.

There is also `sdu` - a CLI interface for the `shin-core` library. It can be used to extract game data, and to convert
it to other more conventional formats.

You can download it from the [releases](https://github.com/dcnick3/shin/releases) page.

For now, we have support for extracting the following data:

- `.rom` - Game data archive
- `.bup` - Character sprites
- `.pic` - Backgrounds & CGs
- `.nxa` - Game audio
- `.snr` - Limited support for game scenario (no decompilation, only tracing the execution)
- `.fnt` - Font data
- `.txa` - Texture archives (used mostly for UI)

Support for other formats used is planned.

If you encounter any errors/problems when using the CLI - please open
an [issue on GitHub](https://github.com/DCNick3/shin/issues).

## Building from source

Hardware audio tests always select the system output device named `Loopback Audio` (case insensitive). They fail if that device is unavailable; ordinary application playback uses the default output.

To build from source you would need to have [Rust](https://www.rust-lang.org/) installed.

Then you can build the engine with:

```bash
cargo build --release -p shin
```

And run it with:

```bash
cargo run --release -p shin
```

## Disclaimer

This project is not affiliated with Entergram or 07th Expansion. It is an unofficial fan project.

It does not contain copyrighted game data, so you can't play the game if you didn't buy it.

Complete native picture loading now matches all 2,604 supplied PIC assets
against the applicable original/1.03 executables, including geometry, indices,
decoded pixels, same-file cache sharing and cleanup. Native UV arithmetic uses
a reciprocal followed by multiplication; the port now matches its float bits.
The CPU harness adapts the dictionary leaf's reserved register for macOS and
provides guarded ROM/job/NVN storage contracts. Three reversed/signed-offset
profiles and eight mutations pass. Picture-ID alias caching, Bustup/Movie
loaders, mixed hydration and platform scheduling remain unverified. See
`reverse-engineering/native-picture-load-validation.json`.

The desktop Config setting 自動スリープの抑制 also controls background
playback. Enabled scenes update without a window surface while unfocused or
hidden; background controls are suppressed and audio continues. Disabled
retains focus-loss pausing, and OS lifecycle suspension always pauses.
The packaged opening passes enabled/disabled background and three hide/return
intervals, with seven window and three input tests. Both macOS workarounds
remain disabled. See
`reverse-engineering/background-playback-setting-validation.json`.
