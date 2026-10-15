// this is noisy & not well-supported by IDEs
#![allow(clippy::uninlined_format_args)]
// Retained scene/asset futures include the recursive wgpu resource graph when
// Rust proves Send/Sync for asynchronous scene loading.
#![recursion_limit = "256"]

include!("entry.rs");

fn main() -> anyhow::Result<()> {
    run(false)
}
