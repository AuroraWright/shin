//! Full authored Bonus entry via ordinary Title Load, not a command/VM patch.
use super::*;
use crate::app::higurashi::runtime_saves::Slots;
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};
use std::cell::RefCell;

fn saved_bonus() -> SaveData {
    let mut saved = initial();
    saved.play_seconds = 17;
    saved.settings.common.v3_sysvol = 37;
    let entry = |scenario_id, random_seed| GameDataEntry {
        scenario_id,
        random_seed,
        save_position: 0,
        replay_target: 0,
        response_data: vec![],
    };
    let date = SaveDate {
        year: 2026,
        month: 10,
        day: 13,
        hour: 18,
        minute: 19,
        second: 20,
    };
    //4738..47e0's genuine Bonus producer makes fresh record0/scenario10010.
    // A controlled initial disk row enters that same unmodified init branch.
    // Date is fixture bookkeeping; native fresh date is proven unwritten.
    saved.manual_save_slots[0] = Some(GameData {
        date,
        extra_word: 0,
        entry: entry(10010, 42),
        primary_entry: None,
        trailing_values: vec![],
        byte_variables: vec![],
    });
    saved.manual_save_slots[99] = Some(GameData {
        date,
        extra_word: 5,
        entry: entry(0, 17),
        primary_entry: Some(GameDataEntry {
            response_data: vec![0xf1, 0x55, 0x23, 0xee],
            ..entry(2084, 0x8765_4321)
        }),
        trailing_values: vec![7, 1023],
        byte_variables: vec![2, 255],
    });
    saved
}
fn rows(runtime: &Runtime) -> Slots {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .clone()
}
fn same_manual(actual: &Slots, expected: &Slots) {
    for (index, (a, e)) in actual.manual.iter().zip(&expected.manual).enumerate() {
        assert_eq!(a.valid, e.valid, "manual{index} validity");
        assert_eq!(a.data, e.data, "manual{index} full two-record payload");
    }
}
fn identities(runtime: &Runtime) -> [usize; 5] {
    let g = runtime.globals().unwrap().unwrap();
    [
        Arc::as_ptr(g.common) as usize,
        Arc::as_ptr(g.server) as usize,
        Arc::as_ptr(g.manager) as usize,
        Arc::as_ptr(g.sounds) as usize,
        Arc::as_ptr(g.storage.unwrap()) as *const () as usize,
    ]
}
fn acquired_tip314(data: &SaveData) -> bool {
    let bit = 314 * 2;
    data.save_vectors
        .extra_words_0
        .get(bit / 32)
        .is_some_and(|word| word & (1 << (bit % 32)) != 0)
}

#[test]
#[ignore = "full unmodified authored Bonus10010 via Runtime TitleLoad/raw J/actual clocks+credits EOF/TIPS314/OTSUGET42/EXIT SaveThenTitle;real assets/offscreen Metal/the default audio output;temporary initial disk only"]
fn higurashi_runtime_bonus_10010_reaches_natural_exit_save_and_title() {
    let saved = saved_bonus();
    let initial_file = saved.encode().unwrap();
    let mut j = Journey::new("sustained-bonus-10010", &saved);
    let viewport = viewport((1280, 720));
    // The authored ending plays BGM235 once for 221.205 real seconds.
    // Allow the complete story and natural credits EOF without changing clocks.
    let deadline = Instant::now() + Duration::from_secs(1500);
    j.boot_title(viewport, deadline);
    let before = rows(&j.runtime);
    let shared = identities(&j.runtime);
    let clock = j.runtime.globals().unwrap().unwrap().clock.ticks();
    j.no_writes(Some(&initial_file));
    load(&mut j, viewport, deadline);
    let data = game(&j.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(data.progress.active_index(), 0);
    assert_eq!(data.progress.active_entry().scenario_id, 10010);
    let scenario = &j.runtime.globals().unwrap().unwrap().common.scenario;
    assert_eq!(
        &scenario.raw()[0x1e279..0x1e283],
        &[0x46, 0x00, 0xb0, 0x90, 0x27, 0x1a, 0xa4, 0xf8, 0x74, 0x01]
    );
    assert_eq!(
        &scenario.raw()[0x18236d4..0x18236db],
        &[0x90, 0x80, 0xeb, 0x00, 0x01, 0x83, 0xe8]
    );
    assert_eq!(
        &scenario.raw()[0x18236fb..0x18236ff],
        &[0xd0, 0x01, 0x81, 0x3a]
    );
    assert_eq!(&scenario.raw()[0x1823731..0x1823734], &[0, 0, 0]);
    pure_frame(&mut j);
    let final_source_rows = RefCell::new(None::<Slots>);
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "Bonus10010 natural EXIT/save/ready Top",
        |runtime| {
            if let Some(g) = game(runtime) {
                let h = g.host().unwrap();
                if matches!(
                    h.runner.pending_command(),
                    Some(Cmd::EXIT { arg1: 0, arg2: 0 })
                ) && h.runner.position().0 == 0x1823731
                    && g.has_save_task()
                    && final_source_rows.borrow().is_none()
                {
                    // Capture the real full101 rows only after EXIT has committed
                    // and created its retained save. Autosave legitimately evolves
                    // throughout the story; manual owners never change here.
                    *final_source_rows.borrow_mut() = Some(rows(runtime));
                }
            }
            ready_menu(runtime, "top")
        },
    );
    assert!(
        game(&j.runtime).is_none(),
        "ready Title must retire the actual ADV source"
    );
    assert!(
        driver.max_commands > 20_000,
        "go beyond the old bounded CPU prefix"
    );
    assert!(
        driver.exits.iter().any(|(pc, _)| *pc == 0x1823731),
        "observe the actual authored main EXIT, not an injected return"
    );
    assert!(
        driver.pending_pcs.contains(&0x18236db),
        "actual credits BGMSYNC1230 observed"
    );
    assert!(
        driver.pending_pcs.contains(&0x18236e5),
        "actual credits BGMWAIT2 observed"
    );
    let final_source_rows = final_source_rows
        .into_inner()
        .expect("no retained natural EXIT SaveGame boundary observed");
    let after = rows(&j.runtime);
    same_manual(&after, &before);
    assert_eq!(after.auto.valid, final_source_rows.auto.valid);
    assert_eq!(after.auto.data, final_source_rows.auto.data);
    same_manual(&after, &final_source_rows);
    assert!(after.auto.valid);
    assert_eq!(after.auto.data.progress.active_entry().scenario_id, 10010);
    assert_eq!(identities(&j.runtime), shared);
    let globals = j.runtime.globals().unwrap().unwrap();
    assert!(globals.clock.ticks() > clock);
    assert_eq!(globals.data.play_seconds, globals.clock.saved_seconds());
    assert_ne!(globals.data.persist_data.get(176) & 2, 0);
    assert_ne!(globals.data.persist_data.get(177) & 1, 0);
    assert!(acquired_tip314(globals.data));
    let writes = j.storage.writes.lock().unwrap().clone();
    assert!(
        !writes.is_empty(),
        "natural EXIT must complete its real flag1 save"
    );
    let written = writes.last().unwrap();
    assert_eq!(
        j.storage
            .paths
            .read(Source::Current)
            .unwrap()
            .as_ref()
            .unwrap(),
        written
    );
    let decoded = SaveData::decode(written).unwrap();
    assert_eq!(decoded.manual_save_slots, saved.manual_save_slots);
    assert_ne!(decoded.persist_data.get(176) & 2, 0);
    assert_ne!(decoded.persist_data.get(177) & 1, 0);
    assert!(acquired_tip314(&decoded));
    assert_eq!(
        decoded.auto_save_slot.as_ref().unwrap().entry.scenario_id,
        10010
    );
    assert!(j.runtime.failure().is_none());
    pure_frame(&mut j);
    println!(
        "Runtime Bonus10010 complete: {} commands/{} callbacks;actual credits sync+EOF/TIPS314/OTSUGET42/EXIT01823731→{} real writes→ready Title;full101 owners and Globals retained;unmodified scenario/normal TitleLoad/raw controls only",
        driver.max_commands,
        driver.callbacks,
        writes.len()
    );
}
