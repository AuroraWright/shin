//! Complete authored prologue, opening Movie0 and Days1/2 through ordinary
//! Runtime callbacks. Day3 is a bounded checkpoint, not whole-book completion.
use super::*;
use crate::adv::higurashi::read_state::native_message_id;
use crate::app::higurashi::runtime_game_data::RuntimeGameData;
use shin_core::format::save::higurashi::{GameData, GameDataEntry, SaveDate};

fn manual_rows(runtime: &Runtime) -> Vec<(bool, RuntimeGameData)> {
    runtime
        .globals()
        .unwrap()
        .unwrap()
        .runtime_saves
        .unwrap()
        .manual
        .iter()
        .map(|row| (row.valid, row.data.clone()))
        .collect()
}

fn identities(runtime: &Runtime) -> [usize; 5] {
    let globals = runtime.globals().unwrap().unwrap();
    [
        Arc::as_ptr(globals.common) as usize,
        Arc::as_ptr(globals.server) as usize,
        Arc::as_ptr(globals.manager) as usize,
        Arc::as_ptr(globals.sounds) as usize,
        Arc::as_ptr(globals.storage.unwrap()) as *const () as usize,
    ]
}

fn pending(runtime: &Runtime, pc: u32) -> bool {
    game(runtime).is_some_and(|g| g.host().is_some_and(|host| host.runner.position().0 == pc))
}

fn message(runtime: &Runtime, pc: u32, id: u32) -> bool {
    game(runtime).is_some_and(|g| {
        g.host().is_some_and(|host| {
            host.runner.position().0 == pc
                && host.runner.frame().message.playback.message_id == native_message_id(id)
                && matches!(
                    host.runner.pending_command(),
                    Some(Cmd::MSGSET { msg_id, .. }) if msg_id.0 == id
                )
        })
    })
}

#[test]
#[ignore = "actual normal Runtime/new game/prologue/opening Movie0 EOF/complete Days1+2/real TIPSGET Continue/Day3;real assets/offscreen Metal/the default audio output;temporary progress only"]
fn higurashi_runtime_sustained_fresh_main_reaches_day3_through_authored_tips_choice() {
    let mut saved = initial();
    // This unrelated preexisting disk row detects loss of either runtime
    // record or any vector. Fresh story persist slots remain untouched/zero.
    saved.manual_save_slots[99] = Some(GameData {
        date: SaveDate {
            year: 2026,
            month: 10,
            day: 13,
            hour: 18,
            minute: 19,
            second: 20,
        },
        extra_word: 5,
        primary_entry: Some(GameDataEntry {
            scenario_id: 0,
            random_seed: 0x1234_5678,
            save_position: 0,
            replay_target: 0,
            response_data: vec![0x55, 0x80, 0xee],
        }),
        entry: GameDataEntry {
            scenario_id: 2000,
            random_seed: 0x8765_4321,
            save_position: 0,
            replay_target: 0,
            response_data: vec![0x91, 0x23, 0xfe, 0x80],
        },
        trailing_values: vec![7, 1023, 0x123],
        byte_variables: vec![9, 0, 255, 1, 0x80],
    });
    assert_eq!(saved.persist_data.get(1), 0);
    assert_eq!(saved.persist_data.get(2), 0);
    assert_eq!(saved.persist_data.get(3), 0);
    let disk = saved.encode().unwrap();
    let mut journey = Journey::new("sustained-main", &saved);
    let viewport = viewport((1920, 1080));
    let deadline = Instant::now() + Duration::from_secs(600);
    journey.boot_title(viewport, deadline);
    let original_manual = manual_rows(&journey.runtime);
    assert_eq!(original_manual.len(), 100);
    assert!(original_manual[99].0);
    let owners = identities(&journey.runtime);
    let clock = journey.runtime.globals().unwrap().unwrap().clock.ticks();
    journey.start_story(viewport, deadline);
    let source = game(&journey.runtime).unwrap().runtime_game_data().unwrap();
    assert_eq!(source.progress.active_index(), 0);
    assert_eq!(source.progress.active_entry().scenario_id, 0);
    let host = game(&journey.runtime).unwrap().host().unwrap();
    assert_eq!(
        host.runner.frame().message.playback.message_id,
        native_message_id(1579)
    );
    assert_eq!(
        host.runner.native_save_metadata().unwrap().current()[1].as_ref(),
        "prologue"
    );
    pure_frame(&mut journey);
    journey.no_writes(Some(&disk));

    let mut driver = Driver::new();
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "complete prologue and authored opening Movie0 wait",
        |r| pending(r, 0x0022_2e72),
    );
    assert!(matches!(
        game(&journey.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MOVIEWAIT { args: [13, 2] })
    ));
    {
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert_eq!(globals.data.persist_data.get(1), 1);
        assert_eq!(globals.data.persist_data.get(2), 1);
        assert_eq!(globals.data.persist_data.get(3), 0);
    }
    pure_frame(&mut journey); // A real decoding frame may still be black.
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "actual opening Movie0 EOF and Day1 first message",
        |r| message(r, 0x0022_32d4, 18867),
    );
    assert!(driver.movie_waits.contains(&0x0022_2e72));
    assert_eq!(
        game(&journey.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .native_save_metadata()
            .unwrap()
            .current()[0]
            .as_ref(),
        "1日目"
    );
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "complete authored Day1 and Day2 first message",
        |r| message(r, 0x0022_4ef8, 18944),
    );
    pure_frame(&mut journey);
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "complete authored Day2 and actual TIPSGET choice",
        |r| {
            pending(r, 0x0023_0ded)
                && game(r)
                    .unwrap()
                    .reference_story_choice()
                    .is_some_and(|(phase, _, _)| phase == 2)
        },
    );
    let (_, selected, rows) = game(&journey.runtime)
        .unwrap()
        .reference_story_choice()
        .unwrap();
    assert_eq!(rows, [(0, "本編を続ける".into()), (1, "TIPSを見る".into())]);
    assert!(selected == -1 || selected == 0);
    {
        let globals = journey.runtime.globals().unwrap().unwrap();
        assert_eq!(globals.data.persist_data.get(3), 0);
        assert_eq!(globals.data.persist_data.get(55), 1);
        assert_eq!(globals.data.save_vectors.extra_words_0[0] & 0x3f, 0x15);
    }
    assert_eq!(manual_rows(&journey.runtime), original_manual);
    assert_eq!(identities(&journey.runtime), owners);
    pure_frame(&mut journey);
    journey.no_writes(Some(&disk));

    // The shared driver accepts the first visible real Choice row using only
    // fresh Down/A; no direct command/response/temporary-owner mutation.
    driver.until(
        &mut journey,
        viewport,
        deadline,
        "actual TIPS Continue closing and Day3 MSGSET19497",
        |r| message(r, 0x0023_0efb, 19497),
    );
    assert!(driver.pending_pcs.contains(&0x0023_0ded));
    assert!(!driver.pending_pcs.contains(&0x0022_ed33));
    assert!(
        driver.exits.is_empty(),
        "no EXIT belongs to the pristine route before Day3"
    );
    let game = game(&journey.runtime).unwrap();
    assert!(game.reference_story_choice().is_none());
    let host = game.host().unwrap();
    let metadata = host.runner.native_save_metadata().unwrap();
    assert_eq!(metadata.current()[0].as_ref(), "3日目");
    assert_eq!(metadata.current()[1].as_ref(), "#1 レナたちと散歩");
    let final_data = game.runtime_game_data().unwrap();
    assert_eq!(final_data.progress.active_entry().scenario_id, 0);
    assert_eq!(final_data.progress.active_index(), 0);
    assert!(host.runner.completed_commands() > 1000);
    assert!(driver.max_commands > 1000);
    assert!(journey.runtime.globals().unwrap().unwrap().clock.ticks() > clock);
    assert_eq!(manual_rows(&journey.runtime), original_manual);
    assert_eq!(identities(&journey.runtime), owners);
    pure_frame(&mut journey);
    journey.no_writes(Some(&disk));
    assert!(journey.runtime.failure().is_none());

    // Persist the state generated by this entire authored route through the
    // actual Runtime shutdown owner, not a direct SaveData/storage call.
    journey.runtime.request_close().unwrap();
    let close_deadline = Instant::now() + Duration::from_secs(30);
    while journey.step(&RawInputState::new(), viewport) != Frame::Exit {
        assert!(Instant::now() < close_deadline, "sustained main close");
    }
    let writes = journey.storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    let persisted = SaveData::decode(&writes[0]).unwrap();
    assert_eq!(persisted.persist_data.get(1), 1);
    assert_eq!(persisted.persist_data.get(2), 1);
    assert_eq!(persisted.persist_data.get(3), 0);
    assert_eq!(persisted.persist_data.get(55), 1);
    assert_eq!(persisted.save_vectors.extra_words_0[0] & 0x3f, 0x15);
    assert_eq!(
        persisted.manual_save_slots,
        SaveData::decode(&disk).unwrap().manual_save_slots
    );
    assert_eq!(
        journey
            .storage
            .paths
            .read(Source::Current)
            .unwrap()
            .as_deref(),
        Some(writes[0].as_slice())
    );
    println!(
        "Higurashi sustained normal Runtime: authored prologue + Movie0 EOF + complete Days1/2 + real TIPSGET[0,1,2] Continue -> Day3 MSG19497; {} callbacks/{} commands;100 full manual rows retained;pure720p/1080p captures;actual close writes generated flags/acquisitions;no VM/scene/policy/clock patches",
        driver.callbacks, driver.max_commands
    );
}
