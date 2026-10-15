//! Internal automated window acceptance. The ordinary executable keeps its
//! launch gate until all reachable game destinations are implemented.
#![allow(clippy::uninlined_format_args)]
#![recursion_limit = "256"]
extern crate self as shin;
#[expect(unused)]
mod adv;
mod app;
mod asset;
mod audio;
mod cli;
mod engine_random;
#[expect(unused)]
mod fps_counter;
mod game_inspection;
mod layer;
mod render;
mod time;
#[expect(unused)]
mod title;
mod update;
mod wiper;

fn main() -> anyhow::Result<()> {
    shin_window::init_tracing();
    shin_tasks::create_task_pools();
    app::run_higurashi_window_probe()
}
