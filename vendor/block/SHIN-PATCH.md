# Local compatibility patch: block 0.1.6

Source: the existing crates.io block 0.1.6 package, by Steven Sheldon, MIT.
The private opaque `Class` representation and stack-block static address
expression change: use inhabited `c_void` and `ptr::addr_of!`, preserving the
symbol and pointer ABI. This fixes Rust 1.99's future-incompatible uninhabited
static warning without upgrading the stable dependency baseline. Bare `extern`
declarations now explicitly name the existing C ABI to satisfy Rust 1.99.

This legacy dependency is used by mesh-vis → iced/wgpu27 → metal, not the
main engine's modern block2 bindings. The local manifest disables upstream
unit tests requiring the unpublished Objective-C helper; the added integration
regression checks native heap copy, invocation, shared ownership and exactly
one captured-value destruction. Run it explicitly with:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0 \
  cargo +1.99.0 test --manifest-path vendor/block/Cargo.toml --target-dir target --release --test abi --offline
```

The workspace excludes this private patch crate from member-wide targets and
selects it through `[patch.crates-io]`; package versions remain unchanged.
