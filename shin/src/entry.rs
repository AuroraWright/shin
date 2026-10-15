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

// Both binary entrypoints share bootstrap code; their only difference is mode.
fn run(window_probe: bool) -> anyhow::Result<()> {
    shin_window::init_tracing();
    #[cfg(feature = "higurashi-window-probe")]
    if window_probe {
        shin_tasks::create_task_pools();
        return app::run_higurashi_window_probe();
    }
    #[cfg(not(feature = "higurashi-window-probe"))]
    assert!(!window_probe, "The window probe requires its Cargo feature");
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
