// this is noisy & not well-supported by IDEs
#![allow(clippy::uninlined_format_args)]
// Retained scene/asset futures include the recursive wgpu resource graph when
// Rust proves Send/Sync for asynchronous scene loading.
#![recursion_limit = "256"]

extern crate self as shin;

use clap::Parser;
use tracing::error;

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
// mod window;

#[cfg(test)]
pub(crate) fn init_test_task_pools() {
    static POOLS: std::sync::Once = std::sync::Once::new();
    POOLS.call_once(shin_tasks::create_task_pools);
}

fn main() -> anyhow::Result<()> {
    shin_window::init_tracing();
    let mut cli = cli::Cli::parse();

    if cli.inspect_assets {
        return game_inspection::inspect(&cli);
    }
    if cli.game == cli::Game::Higurashi {
        app::prepare_higurashi_launch(&mut cli)?;
    }

    // Create a background thread which checks for deadlocks every 10s
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let deadlocks = parking_lot::deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }

            error!("{} deadlocks detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                error!("Deadlock #{}", i);
                for t in threads {
                    error!("Thread Id {:#?}", t.thread_id());
                    error!("{:#?}", t.backtrace());
                }
            }
        }
    });

    shin_tasks::create_task_pools();

    match cli.game {
        cli::Game::Umineko => shin_window::run_window::<app::App>(cli),
        cli::Game::Higurashi => shin_window::run_window::<app::HigurashiApp>(cli),
    }
    Ok(())
}
