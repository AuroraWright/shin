# UI text overrides

Copy `umineko.en.toml` or `higurashi.en.toml` into that game's assets folder,
rename it `localized.toml`, then restart the game. If the folder already has a
`localized.toml`, merge the entries you want into it instead of replacing it.

To play with the fan translations' scenario patches as well, see the main
[README](../README.md#mods-and-translations).

The English profiles are taken from the `exefs_texts.txt` lists of the fan
translation projects
[umineko-catbox-english](https://github.com/andOlga/umineko-catbox-english/blob/rebirth/exefs_texts.txt)
(rebirth branch) and
[HigurashiENX](https://github.com/masagrator/HigurashiENX/blob/main/exefs_texts.txt).
They contain 60 Umineko and 39 Higurashi keys, respectively. These lists cover
only part of the UI, so the remaining keys keep their Japanese text. Scenario
text and text baked into images are not affected.

The conversion adapts printf time values to Rust placeholders, expands
completion prefixes into complete keys, combines the two pieces of Higurashi's
shortcut prompt, and preserves Umineko's leading color and size commands. The
save warning uses the engine's `ソフト` key instead of the source list's
`アプリケーション`. The English wording is unchanged from the source lists. The
hexadecimal addresses in the comments record where each string came from; they
are not lookup keys.

The `.localized.toml.example` files list every key with its Japanese text as
the value. Use them to translate keys the English profiles don't cover.
Gerokasu2 has only `gerokasu2.localized.toml.example`, without an English
profile. It covers dialogs, the Load screen's empty and loading rows, the
play-time clock and the backlog's fixed chapter markers. Story, choice,
chapter, scene and music titles come from the scenario and keep their original
text.
