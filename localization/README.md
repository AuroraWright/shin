# UI text overrides

Copy `higurashi.en.toml` or `umineko.en.toml` into that game's selected asset
folder, rename it `localized.toml`, then restart the game. For the standard
manual launchers, use `higurashi/localized.toml` or
`umineko_switch_files/localized.toml` in the workspace. Preserve any existing
custom dictionary by merging the desired entries instead of replacing it.

The English profiles use the translations supplied by
[HigurashiENX](https://github.com/masagrator/HigurashiENX/blob/main/exefs_texts.txt)
and [umineko-catbox-english, rebirth branch](https://github.com/andOlga/umineko-catbox-english/blob/rebirth/exefs_texts.txt).
They contain 39 Higurashi and 60 Umineko code keys, respectively. These lists
cover only part of the UI: remaining keys keep Japanese. Scenario `.snr` content
and text baked into images are excluded.

The original hexadecimal addresses are provenance comments, not lookup keys.
The conversion adapts printf time values to Rust placeholders, expands completion
prefixes into complete code keys, combines the Higurashi shortcut prompt's two
literal pieces, and preserves Umineko's native leading color/size controls.
The equivalent save-warning wording uses the code's `ソフト` key instead of the
source list's `アプリケーション`. English wording remains from the supplied lists.

The `.localized.toml.example` files list all code keys with Japanese identity
values. Use them to add translations not supplied by the English lists.
Gerokasu2 has only `gerokasu2.localized.toml.example` (no English profile). Put
its translated copy in `gerokasu2/localized.toml`. It covers dialogs, Load's
empty and loading rows, the play-time clock and the backlog chapter's fixed
markers. Story, choice, chapter, scene and music titles come from the scenario
and keep their original text.
See the main README for font, markup and placeholder limitations. Reproduction
inputs, mappings and checks are under
`reverse-engineering/higurashi/english-localization-20261015/`.
