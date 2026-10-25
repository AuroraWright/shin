//! Generic full playthroughs of the 17 routes that had no reviewed full-route
//! fixture in `inventory-after-yoigoshi-review.json` (10 standard + 7 Origin).
//! Exact supplied unlocked template; canonical held Ctrl, the original first
//! authored row of every choice and ordinary confirmations only. The pass
//! condition is the selected scenario reaching a common EXIT and the returned
//! Title with settings and manual saves unchanged. These are not per-route
//! feature acceptances: no earned progression, PCM or native-frame claim.
use super::*;

fn template() -> (SaveData, Vec<u8>) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/higurashi/chapter-playthrough-20261015/unlocked-input.dat"
    ))
    .unwrap();
    assert_eq!(bytes.len(), 31748);
    assert_eq!(
        bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100000001b3)),
        0x83007166ab0dfcf0
    );
    let data = SaveData::decode(&bytes).unwrap();
    assert_eq!(data.encode().unwrap(), bytes);
    (data, bytes)
}
fn scenario(r: &Runtime) -> Option<i32> {
    game(r)?
        .runtime_game_data()
        .ok()
        .map(|d| d.progress.active_entry().scenario_id)
}
fn picker_ready(r: &Runtime) -> bool {
    r.slot()
        .and_then(|s| s.scenario_select.as_ref())
        .is_some_and(|p| {
            let o = p.reference_owner();
            let s = o.reference_state();
            let m = o.reference_model();
            s.0 && s.2 && s.3.is_none() && m.ready && !m.closing
        })
}
fn picker(r: &Runtime) -> (u32, i32, usize, u32) {
    let m = r
        .slot()
        .unwrap()
        .scenario_select
        .as_ref()
        .unwrap()
        .reference_owner()
        .reference_model();
    (m.kind, m.selected, m.count, m.mask)
}
fn kakera_ready(r: &Runtime) -> bool {
    r.slot()
        .and_then(|s| s.kakera.as_ref())
        .is_some_and(|k| k.reference_input_ready())
}
fn vm_scenario(r: &Runtime) -> Option<i32> {
    game(r)
        .is_some_and(|g| g.host().is_some_and(|h| h.runner.native_vm_attached()))
        .then(|| scenario(r))
        .flatten()
}
fn top_selection(j: &Journey) -> Option<i32> {
    j.runtime
        .slot()
        .unwrap()
        .title
        .as_ref()
        .unwrap()
        .reference_top_selection()
}

/// Where the ordinary Title reaches a route: a standard category row (whose
/// category scenario opens the picker), or the Top8 Origin picker.
#[derive(Clone, Copy)]
enum Menu {
    Category(usize),
    Origin,
}

/// Grid picker: Left/Right switch the column, Up/Down move by one row of two.
fn navigate(j: &mut Journey, vp: InputViewport, want: i32) {
    for _ in 0..24 {
        let (_, selected, count, mask) = picker(&j.runtime);
        assert!(
            (want as usize) < count,
            "row{want} outside picker count{count}"
        );
        assert_ne!(mask & (1 << want), 0, "row{want} locked (mask {mask:#x})");
        if selected == want {
            return;
        }
        let code = if selected < 0 || (selected - want) % 2 != 0 {
            KeyCode::ArrowLeft
        } else if want > selected {
            KeyCode::ArrowDown
        } else {
            KeyCode::ArrowUp
        };
        j.tap(code, vp);
        assert!(picker_ready(&j.runtime));
    }
    panic!("picker did not reach row{want}: {:?}", picker(&j.runtime));
}

fn open_route(j: &mut Journey, vp: InputViewport, deadline: Instant, menu: Menu, row: i32) -> i32 {
    if top_selection(j) == Some(1) {
        j.tap(KeyCode::ArrowLeft, vp);
    }
    assert_eq!(top_selection(j), Some(0));
    let category = match menu {
        Menu::Origin => {
            j.tap(KeyCode::ArrowUp, vp);
            assert_eq!(top_selection(j), Some(8));
            j.tap(KeyCode::Enter, vp);
            j.until(vp, deadline, "Origin picker ready", picker_ready);
            None
        }
        Menu::Category(index) => {
            j.tap(KeyCode::Enter, vp);
            j.until(vp, deadline, "Category ready", |r| ready_menu(r, "cat"));
            for wanted in 1..=index {
                j.tap(KeyCode::ArrowDown, vp);
                assert_eq!(
                    j.runtime
                        .slot()
                        .unwrap()
                        .title
                        .as_ref()
                        .unwrap()
                        .reference_category(),
                    Some((0, wanted as i32, 2))
                );
            }
            j.tap(KeyCode::Enter, vp);
            j.until(vp, deadline, "category picker ready", |r| {
                picker_ready(r) || vm_scenario(r).is_some_and(|s| s >= 100)
            });
            let mut route = Route::new();
            while !picker_ready(&j.runtime) {
                route.step(j, vp, deadline);
            }
            Some(index as u32)
        }
    };
    let (kind, _, count, mask) = picker(&j.runtime);
    if let Some(index) = category {
        assert_eq!(kind, index);
    }
    eprintln!("ROUTE_PICKER kind={kind} count={count} mask={mask:#x} row={row}");
    navigate(j, vp, row);
    j.tap(KeyCode::Enter, vp);
    j.until(vp, deadline, "selected route VM", |r| {
        vm_scenario(r).is_some_and(|s| s >= 100 && s != kind as i32)
    });
    vm_scenario(&j.runtime).unwrap()
}

/// Canonical route input. Ctrl stays held except while an interactive owner
/// takes one released Enter/Down press; the press is always released next.
struct Route {
    released: bool,
    choices: Vec<(u32, usize, usize)>,
    last_choice: Option<(u32, i32)>,
    exits: Vec<(u32, u8)>,
    max_commands: u64,
    scenarios: Vec<i32>,
    kakera_cards: std::collections::BTreeSet<usize>,
    last_progress: (u64, Instant),
    last_log: Instant,
}
impl Route {
    fn new() -> Self {
        Self {
            released: true,
            choices: Vec::new(),
            last_choice: None,
            exits: Vec::new(),
            max_commands: 0,
            scenarios: Vec::new(),
            kakera_cards: Default::default(),
            last_progress: (0, Instant::now()),
            last_log: Instant::now(),
        }
    }
    /// First unvisited available Origin fragment, navigated with raw arrows.
    fn kakera(&mut self, j: &mut Journey, vp: InputViewport) -> Option<KeyCode> {
        let k = j.runtime.slot()?.kakera.as_ref()?;
        let m = &k.motion.model;
        let page = m.page;
        let wanted = m.pages[page]
            .iter()
            .position(|id| m.statuses[*id] > 0 && !self.kakera_cards.contains(id));
        let Some(cell) = wanted else {
            return Some(KeyCode::KeyJ);
        };
        if m.selected == cell {
            self.kakera_cards.insert(m.pages[page][cell]);
            eprintln!("ROUTE_KAKERA card={}", m.pages[page][cell]);
            return Some(KeyCode::Enter);
        }
        let _ = vp;
        Some(if m.selected / 3 != cell / 3 {
            KeyCode::ArrowDown
        } else {
            KeyCode::ArrowRight
        })
    }
    fn step(&mut self, j: &mut Journey, vp: InputViewport, deadline: Instant) {
        assert!(
            Instant::now() < deadline,
            "route deadline: {}",
            Driver::describe(&j.runtime)
        );
        let mut raw = key(KeyCode::ControlLeft);
        let mut press = None;
        let mut neutral = false;
        if kakera_ready(&j.runtime) {
            neutral = true;
            if self.released {
                press = self.kakera(j, vp);
            }
        } else if let Some(g) = game(&j.runtime) {
            if let Some(h) = g.host() {
                let commands = h.runner.completed_commands();
                self.max_commands = self.max_commands.max(commands);
                if commands != self.last_progress.0 {
                    self.last_progress = (commands, Instant::now());
                }
                if let Some(s) = scenario(&j.runtime) {
                    if self.scenarios.last() != Some(&s) {
                        eprintln!("ROUTE_SCENARIO {s} commands={commands}");
                        self.scenarios.push(s);
                    }
                }
                if let Some(Cmd::EXIT { arg1, .. }) = h.runner.pending_command() {
                    let exit = (h.runner.position().0, *arg1);
                    if !self.exits.contains(&exit) {
                        eprintln!("ROUTE_EXIT pc={:08x} arg1={arg1}", exit.0);
                        self.exits.push(exit);
                    }
                }
            }
            if g.reference_fake_select_state().is_some() {
                // FakeSelect runs its own automatic sequence.
                neutral = true;
            } else if let Some((phase, selected, rows)) = g.reference_story_choice() {
                neutral = true;
                if phase == 2 && g.input_permission && self.released {
                    let target = rows.iter().position(|(id, _)| *id == 0).unwrap_or(0);
                    let pc = g.host().map_or(0, |h| h.runner.position().0);
                    if selected == target as i32 {
                        if self.last_choice != Some((pc, selected)) {
                            eprintln!("ROUTE_CHOICE pc={pc:08x} row={target} rows={rows:?}");
                            self.choices.push((pc, target, rows.len()));
                            self.last_choice = Some((pc, selected));
                        }
                        press = Some(KeyCode::Enter);
                    } else {
                        press = Some(KeyCode::ArrowDown);
                    }
                }
            } else if let Some((phase, selected, originals)) = g.reference_story_character() {
                neutral = true;
                if phase == 2 && g.input_permission && self.released {
                    let target = originals.iter().position(|o| *o == 0).unwrap_or(0);
                    press = Some(if selected == target as i32 {
                        eprintln!("ROUTE_CHARACTER row={target} originals={originals:?}");
                        KeyCode::Enter
                    } else {
                        KeyCode::ArrowDown
                    });
                }
            } else if matches!(g.reference_story_notification(), Some((12..=14, 2))) {
                if self.released {
                    press = Some(KeyCode::Enter);
                }
            } else if g.input_permission
                && self.released
                && self.last_progress.1.elapsed() > Duration::from_secs(5)
            {
                // A wait that held Ctrl does not complete takes one ordinary A.
                press = Some(KeyCode::Enter);
            }
        }
        if neutral {
            raw = RawInputState::new();
        }
        if let Some(code) = press {
            raw.keyboard.insert(code);
        }
        self.released = press.is_none();
        if self.last_log.elapsed() >= Duration::from_secs(30) {
            eprintln!(
                "ROUTE_PROGRESS max_commands={} {}",
                self.max_commands,
                Driver::describe(&j.runtime)
            );
            self.last_log = Instant::now();
        }
        assert!(
            self.last_progress.1.elapsed() < Duration::from_secs(300),
            "route stalled for 5 minutes: {}",
            Driver::describe(&j.runtime)
        );
        assert!(
            matches!(j.step(&raw, vp), Frame::Advanced(_)),
            "route unexpectedly closing"
        );
    }
}

fn play(label: &str, menu: Menu, row: i32, expected: i32) {
    let (saved, initial) = template();
    let settings = serde_json::to_vec(&saved.settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3 * 60 * 60);
    let vp = viewport((1920, 1080));
    let mut j = Journey::new(label, &saved);
    j.boot_title(vp, deadline);
    let before = j
        .runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone();
    j.no_writes(Some(&initial));
    let started = Instant::now();
    let actual = open_route(&mut j, vp, deadline, menu, row);
    assert_eq!(actual, expected, "picker row{row} opened scenario{actual}");
    let mut route = Route::new();
    while !ready_menu(&j.runtime, "top") {
        route.step(&mut j, vp, deadline);
    }
    assert!(
        route.scenarios.contains(&expected),
        "route never ran scenario{expected}: {:?}",
        route.scenarios
    );
    assert!(
        route.exits.iter().any(|&(_, arg1)| arg1 == 0),
        "no common EXIT before Title: {:?}",
        route.exits
    );
    let g = j.runtime.globals().unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&g.data.settings).unwrap(), settings);
    for (i, (a, b)) in g
        .runtime_saves
        .unwrap()
        .manual
        .iter()
        .zip(&before.manual)
        .enumerate()
    {
        assert_eq!(a.valid, b.valid, "manual{i} validity");
        assert_eq!(a.data, b.data, "manual{i} complete payload");
    }
    let writes = j.storage.writes.lock().unwrap();
    assert!(!writes.is_empty(), "route completion writes progress");
    let actual_file = std::fs::read(j.storage.paths.current.as_ref().unwrap()).unwrap();
    assert_eq!(writes.last().unwrap(), &actual_file);
    let result = SaveData::decode(&actual_file).unwrap();
    assert_eq!(result.encode().unwrap(), actual_file);
    assert_eq!(serde_json::to_vec(&result.settings).unwrap(), settings);
    eprintln!(
        "ROUTE_PASS {label} scenario={expected} seconds={:.2} max_commands={} choices={:?} exits={:?} scenarios={:?} kakera={:?} writes={}",
        started.elapsed().as_secs_f64(),
        route.max_commands,
        route.choices,
        route.exits,
        route.scenarios,
        route.kakera_cards,
        writes.len()
    );
}

macro_rules! routes {
    ($($name:ident: $menu:expr, $row:expr, $id:expr;)*) => {$(
        #[test]
        #[ignore = "Generic full route from the exact unlocked template; offscreen Metal/the default audio output"]
        fn $name() {
            play(stringify!($name), $menu, $row, $id);
        }
    )*};
}
routes! {
    higurashi_route_204_kageboushi: Menu::Category(1), 4, 204;
    higurashi_route_304_tokihogushi: Menu::Category(2), 4, 304;
    higurashi_route_402_matsuribayashi: Menu::Category(3), 2, 402;
    higurashi_route_404_miotsukushi_ura: Menu::Category(3), 4, 404;
    higurashi_route_405_saikoroshi: Menu::Category(3), 5, 405;
    higurashi_route_406_kotohogushi: Menu::Category(3), 6, 406;
    higurashi_route_500_outbreak: Menu::Category(4), 0, 500;
    higurashi_route_502_hinamizawa_teiryujo: Menu::Category(4), 2, 502;
    higurashi_route_503_batsukoishi: Menu::Category(4), 3, 503;
    higurashi_route_10000_mehagashi: Menu::Category(4), 4, 10000;
    higurashi_route_10001_origin_onikakushi: Menu::Origin, 0, 10001;
    higurashi_route_10003_origin_tatarigoroshi: Menu::Origin, 2, 10003;
    higurashi_route_10004_origin_himatsubushi: Menu::Origin, 3, 10004;
    higurashi_route_10005_origin_meakashi: Menu::Origin, 4, 10005;
    higurashi_route_10006_origin_tsumihoroboshi: Menu::Origin, 5, 10006;
    higurashi_route_10007_origin_minagoroshi: Menu::Origin, 6, 10007;
    higurashi_route_10011_origin_saikoroshi: Menu::Origin, 8, 10011;
}
