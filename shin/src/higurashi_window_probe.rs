//! Internal automated acceptance entry sharing the ordinary application bootstrap.
#![allow(clippy::uninlined_format_args)]
#![recursion_limit = "256"]

include!("entry.rs");

fn main() -> anyhow::Result<()> {
    run(true)
}
