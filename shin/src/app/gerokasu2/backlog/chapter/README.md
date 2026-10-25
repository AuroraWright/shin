# ChapterSelector app host

`ChapterHost::load(assets, &History, selected, current)` loads the cached native
log atlas's `chapter` entry and `/system.fnt`, then clones immutable marker
descriptors. It uses the native pure controller and artwork packets. The
parent retains history, browser scroll/flash, cues, private voice, confirmation
and modal/ADV transition ownership.

Call `update(delta, chapter::Input, mapped_point, held, elapsed_seconds)` once
per parent frame and apply returned events before advancing parent scroll,
flash and private voice. The host's `InputArea` runs through the additive pure
`update_with_contact` hook: phase first, permission/readiness, contact callbacks,
keyboard, inertia, then the literal1 scroll tick. A pointer close therefore
keeps progress1 on its initiating frame; a newly selected scroll tween receives
its first tick that same frame. `cancel_input()` returns any parent flash-release
events; controller accessors expose confirmation response and retirement state.
Desktop pointer samples suppress inherited holds and use the shared desktop
gesture vector policy; native touch remains the shared InputArea service.

Call `prepare()` after update. It creates visible GenericText resources and
retains insertion order while pruning invisible owners. Render draws panel
gradients, clears stencil0 while preserving depth/color, writes the y240..960
clip, draws text with Equal/ref1/Keep, then restores ordinary state for header,
gradient borders and scrollbar. The parent draws its chrome afterward. Native
stencil masks are FF/FF; text's Keep operations preserve stencil.

Native GenericText width1280 includes a3px border on each side: the reused
layouter gets width1274/font48, no soft breaks or ruby reservation. The shared
text renderer adds3px to glyph coordinates. The host anchors the padded extent
(`layout size+6`) at[1,0] and native position[1800,(index×60)|3], scale[1,1].
`Text::render_colored` supplies the exact float RGBA row multiplier while the
existing save/menu `render` contract remains alpha-only. Actual glyph atlas,
shader/filtering and framebuffer results are validated separately by root.

The retained original171-owner/6,747-line corpus still matches after the hook;
all532 quad and643 GenericText request/submission native packets are reused
unchanged. Nine pure tests include two new contact-order checks. Host tests
cover shared InputArea close origin/order, gradient channel conversion,
clip state and padded anchor. The ignored actual-assets test
`ger_backlog_chapter_host_renders_native_panel_clip_labels_and_retains_cache_order`
performs three320×180 captures with the default audio output:
labels must differ only within y40..160, the left panel must retain an alpha
gradient, and reverse scrolling must append recreated row0 after retained rows.
Root owns Cargo/hardware execution and records its result in the integration
ledger. No app/GPU equality is inferred solely from native packet comparisons.

No task-owned `/tmp` files were created.
