//! Sustained authored playback through ordinary Runtime/input/resource owners.
//! The driver presses approved controls; it never assigns VM, playback, scene,
//! policy, clock, response or resource state to make progress.
use super::*;
use crate::app::higurashi::gameplay::Gameplay;
use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;

mod bonus;
mod first_book;
mod himatsubushi;
mod hirukowashi;
mod hajisarashi_features;
mod hinamizawa_features;
mod kamikanshi_features;
mod main;
mod meakashi;
mod minagoroshi_features;
mod miotsukushi_omote_features;
mod origin10008_features;
mod origin_watanagashi_features;
mod origin_skip;
mod remaining_routes;
mod someutsushi;
mod taraimawashi;
mod tatarigoroshi;
mod tsukiotoshi;
mod tsumihoroboshi_features;
mod watanagashi;
mod yoigoshi_features;

fn initial() -> SaveData {
    let mut saved = SaveData::default();
    // Legitimate persisted user preferences, before the ordinary startup read.
    saved.settings.common.v11_disallowskipunread = false;
    saved.settings.common.v10_skipspeed = 100;
    saved.settings.common.v9_msgspeed = 100;
    saved
}
fn game(runtime: &Runtime) -> Option<&Gameplay> {
    runtime.slot()?.game.as_ref()
}
fn pure_frame(j: &mut Journey) {
    let frames = j.runtime.frames;
    let ticks = j.runtime.globals().unwrap().unwrap().clock.ticks();
    for extent in [(1280, 720), (1920, 1080)] {
        let first = j.gpu.pixels(&j.runtime, extent, false);
        let capture = j.gpu.pixels(&j.runtime, extent, true);
        let repeated = j.gpu.pixels(&j.runtime, extent, false);
        assert_eq!(first, capture, "pure completed capture");
        assert_eq!(first, repeated, "repeat presentation is pure");
    }
    assert_eq!(j.runtime.frames, frames);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(), ticks);
}
fn load(j: &mut Journey, viewport: InputViewport, deadline: Instant) {
    assert!(ready_menu(&j.runtime, "top"));
    j.tap(KeyCode::ArrowRight, viewport);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(1)
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "normal Title Load menu not ready",
        |r| {
            r.slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_save_menu()
                .is_some_and(|menu| menu.reference_input_ready())
        },
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "Title Load default-No Question not ready",
        |r| {
            r.slot()
                .unwrap()
                .title
                .as_ref()
                .unwrap()
                .reference_save_menu()
                .is_some_and(|menu| menu.reference_question() == Some((2, 1)))
        },
    );
    j.tap(KeyCode::ArrowLeft, viewport);
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "Title Load did not construct authored ADV",
        |r| game(r).is_some_and(|g| g.host().is_some_and(|h| h.runner.completed_commands() > 0)),
    );
}

struct Driver {
    pub max_commands: u64,
    pub callbacks: u64,
    pub exits: Vec<(u32, u64)>,
    pub pending_pcs: std::collections::BTreeSet<u32>,
    pub movie_waits: std::collections::BTreeSet<u32>,
    released: bool,
    last_log: Instant,
    last_choice: Option<(u32, i32)>,
}
impl Driver {
    fn new() -> Self {
        Self {
            max_commands: 0,
            callbacks: 0,
            exits: Vec::new(),
            pending_pcs: Default::default(),
            movie_waits: Default::default(),
            released: true,
            last_log: Instant::now(),
            last_choice: None,
        }
    }
    fn describe(runtime: &Runtime) -> String {
        if let Some(g) = game(runtime) {
            if let Some(h) = g.host() {
                return format!(
                    "ADV pc={:08x} next={:08x} completed={} pending={:?} modes={} input={} read={:?} choice={:?}",
                    h.runner.position().0,
                    h.runner.next_position().0,
                    h.runner.completed_commands(),
                    h.runner.pending_command(),
                    g.reference_play_modes(),
                    g.input_permission,
                    h.runner.native_read_state(),
                    g.reference_story_choice()
                );
            }
            return "ADV First".into();
        }
        format!(
            "scene={:?}",
            runtime.slot().and_then(|s| s.active_scene().ok())
        )
    }
    fn step(&mut self, j: &mut Journey, viewport: InputViewport, deadline: Instant) {
        assert!(
            Instant::now() < deadline,
            "sustained playback deadline: {}",
            Self::describe(&j.runtime)
        );
        assert!(
            self.callbacks < 1_000_000,
            "sustained callback limit: {}",
            Self::describe(&j.runtime)
        );
        let mut press = None;
        if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host() {
                self.max_commands = self.max_commands.max(h.runner.completed_commands());
                if h.runner.pending_command().is_some() {
                    self.pending_pcs.insert(h.runner.position().0);
                }
                if matches!(h.runner.pending_command(), Some(Cmd::MOVIEWAIT { .. })) {
                    self.movie_waits.insert(h.runner.position().0);
                }
                if matches!(h.runner.pending_command(), Some(Cmd::EXIT { .. })) {
                    let exit = (h.runner.position().0, h.runner.completed_commands());
                    if !self.exits.contains(&exit) {
                        self.exits.push(exit);
                    }
                }
                if self.released {
                    if let Some((2, selected, rows)) = g.reference_story_choice() {
                        let key = (h.runner.position().0, selected);
                        if self.last_choice != Some(key) {
                            println!(
                                "authored choice pc={:08x} selected={selected} rows={rows:?}",
                                key.0
                            );
                            self.last_choice = Some(key);
                        }
                        press = Some(if selected < 0 {
                            KeyCode::ArrowDown
                        } else {
                            KeyCode::Enter
                        });
                    } else if matches!(g.reference_story_notification(), Some((12..=14, 2))) {
                        // Normal type12 first waits for A on the acquisition
                        // notice, then creates a separate Choice. Types13/14
                        // similarly wait for A; their VOID result stays native.
                        press = Some(KeyCode::Enter);
                    } else if g.input_permission && g.reference_play_modes() & 2 == 0 {
                        press = Some(KeyCode::KeyJ);
                    }
                }
            }
        }
        if self.last_log.elapsed() >= Duration::from_secs(15) {
            println!(
                "sustained callback{} max_commands{} {}",
                self.callbacks,
                self.max_commands,
                Self::describe(&j.runtime)
            );
            self.last_log = Instant::now();
        }
        let raw = press.map(key).unwrap_or_else(RawInputState::new);
        self.released = press.is_none();
        assert!(
            matches!(j.step(&raw, viewport), Frame::Advanced(_)),
            "sustained route unexpectedly closing"
        );
        self.callbacks += 1;
    }
    fn until(
        &mut self,
        j: &mut Journey,
        viewport: InputViewport,
        deadline: Instant,
        description: &str,
        ready: impl Fn(&Runtime) -> bool,
    ) {
        println!(
            "sustained begin {description}: {}",
            Self::describe(&j.runtime)
        );
        while !ready(&j.runtime) {
            self.step(j, viewport, deadline);
        }
        println!(
            "sustained reached {description}: callbacks{} max_commands{} {}",
            self.callbacks,
            self.max_commands,
            Self::describe(&j.runtime)
        );
    }
}
