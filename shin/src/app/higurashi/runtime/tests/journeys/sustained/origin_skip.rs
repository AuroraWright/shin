//! Controlled unlocked disk -> real Origin Matsuribayashi -> authored quote
//! and its two WAIT0 helpers. This is a synthetic bug fixture, not earned play.
use super::*;
use crate::{
    adv::higurashi::{input, read_state::native_message_id},
    app::higurashi::{runtime_game_data::RuntimeGameData, scenario_select::owner::Owner},
};
use shin_core::format::{
    save::higurashi::{GameData, GameDataEntry, SaveDate},
    scenario::instruction_elements::Register,
};

const QUOTE: u32 = 183284;
fn fixture() -> SaveData {
    let mut saved = initial();
    // Explicit synthetic initial-disk unlock. No live scene, script, response,
    // timer or policy is assigned; the real Origin picker checks this field.
    saved.persist_data.set(176, 3);
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
    saved
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
fn manuals(runtime: &Runtime) -> Vec<(bool, RuntimeGameData)> {
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
fn origin(runtime: &Runtime) -> Option<&Owner> {
    Some(runtime.slot()?.scenario_select.as_ref()?.reference_owner())
}
fn origin_ready(runtime: &Runtime) -> bool {
    origin(runtime).is_some_and(|owner| {
        let state = owner.reference_state();
        let model = owner.reference_model();
        // First has been pruned once the live Input task exists, so its old
        // completion bool is not a readiness requirement.
        state.0 && state.2 && state.3.is_none() && model.ready && !model.closing
    })
}
fn pending(runtime: &Runtime, pc: u32) -> bool {
    game(runtime).is_some_and(|g| {
        g.host()
            .is_some_and(|h| h.runner.position().0 == pc && h.runner.pending_command().is_some())
    })
}
fn neutral(j: &mut Journey, viewport: InputViewport) {
    assert!(matches!(
        j.step(&RawInputState::new(), viewport),
        Frame::Advanced(_)
    ));
    assert!(j.runtime.failure().is_none());
}
fn modes(runtime: &Runtime) -> u32 {
    game(runtime)
        .expect("actual Origin ADV must remain alive")
        .reference_play_modes()
}
fn eligible(runtime: &Runtime) -> bool {
    let g = game(runtime).expect("actual Origin ADV must remain alive");
    input::suppress_transient_audio(
        g.host().unwrap().runner.native_read_state(),
        g.reference_play_modes(),
        false,
        false,
    )
}
fn remaining(runtime: &Runtime, amount: i32, sync: u32) -> (f32, u64) {
    let g = game(runtime).expect("left actual Origin ADV before its scripted pause");
    let host = g.host().unwrap();
    assert_eq!(host.runner.position().0, 0x57825);
    assert!(matches!(host.runner.pending_command(),
        Some(Cmd::WAIT { allow_interrupt: 0, amount: actual }) if *actual == amount));
    let (clock, interrupt) = host
        .runner
        .reference_wait()
        .expect("real retained WAIT clock");
    assert_eq!(interrupt, 0);
    let p = &host.runner.frame().message.playback;
    assert_eq!(p.message_id, native_message_id(QUOTE));
    assert_eq!(
        p.syncs, sync,
        "no fabricated MSGSIGNAL before helper completion"
    );
    assert_eq!(
        host.runner
            .reference_register(Register::from_regular_register(100)),
        Some(0)
    );
    (clock, host.runner.completed_commands())
}
fn signal_after_one_callback(
    j: &mut Journey,
    viewport: InputViewport,
    old_amount: i32,
    commands: u64,
    sync: u32,
    temporary: bool,
) {
    let frames = j.runtime.frames;
    skip_frame(j, viewport, temporary);
    assert_eq!(j.runtime.frames, frames + 1);
    let g = game(&j.runtime).unwrap();
    let h = g.host().unwrap();
    assert!(
        h.runner.completed_commands() > commands,
        "WAIT retires on the following actual poll"
    );
    assert!(!matches!(h.runner.pending_command(),
        Some(Cmd::WAIT { amount, .. }) if *amount == old_amount));
    let p = &h.runner.frame().message.playback;
    assert_eq!(p.message_id, native_message_id(QUOTE));
    assert_eq!(
        p.syncs, sync,
        "actual authored MSGSIGNAL ran after helper return"
    );
    assert_eq!(modes(&j.runtime) & 2, if temporary { 0 } else { 2 });
}

fn skip_frame(j: &mut Journey, viewport: InputViewport, temporary: bool) {
    if temporary {
        assert!(matches!(
            j.step(&key(KeyCode::ControlLeft), viewport),
            Frame::Advanced(_)
        ));
    } else {
        neutral(j, viewport);
    }
}

#[test]
#[ignore = "normal Runtime/Top8/actual Origin7 Matsuribayashi10008/unmodified quote183284/actual WAIT0(42,48) helpers/canonical J skip;real assets/offscreen Metal/exact Loopback Audio;synthetic initial unlock/temp writes only"]
fn higurashi_runtime_origin_skip_retires_both_authored_quote_pauses_on_following_poll() {
    origin_quote_skip(false);
}

#[test]
#[ignore = "real Runtime raw Ctrl/Origin Matsuribayashi quote WAIT0, release and unchanged latched modes; offscreen Metal/exact Loopback Audio"]
fn higurashi_runtime_ctrl_skip_retires_authored_pauses_without_latching() {
    origin_quote_skip(true);
}

fn origin_quote_skip(temporary: bool) {
    let saved = fixture();
    let disk = saved.encode().unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let viewport = viewport((1920, 1080));
    let mut j = Journey::new(
        if temporary {
            "origin-ctrl-skip-pauses"
        } else {
            "origin-authored-skip-pauses"
        },
        &saved,
    );
    j.boot_title(viewport, deadline);
    let shared = identities(&j.runtime);
    let manual = manuals(&j.runtime);
    assert_eq!(manual.len(), 100);
    assert!(manual[99].0);
    j.no_writes(Some(&disk));
    // Fresh Title defaults0 because the synthetic fixture contains no auto.
    // Native Up wraps to the actual bottom Origin row8 in the plus layout.
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(0)
    );
    j.tap(KeyCode::ArrowUp, viewport);
    assert_eq!(
        j.runtime
            .slot()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .reference_top_selection(),
        Some(8)
    );
    j.tap(KeyCode::Enter, viewport);
    j.until(
        viewport,
        deadline,
        "actual Origin picker Input",
        origin_ready,
    );
    let m = origin(&j.runtime).unwrap().reference_model();
    assert_eq!((m.kind, m.selected, m.count), (5, 0, 10));
    assert_ne!(m.mask & (1 << 7), 0, "real native persist176 unlock lookup");
    // Original two-column controller geometry: Left0→1, Down1→3→5→7.
    for (code, selected) in [
        (KeyCode::ArrowLeft, 1),
        (KeyCode::ArrowDown, 3),
        (KeyCode::ArrowDown, 5),
        (KeyCode::ArrowDown, 7),
    ] {
        j.tap(code, viewport);
        assert!(origin_ready(&j.runtime));
        assert_eq!(
            origin(&j.runtime).unwrap().reference_model().selected,
            selected
        );
    }
    j.tap(KeyCode::Enter, viewport);
    let mut driver = Driver::new();
    driver.until(
        &mut j,
        viewport,
        deadline,
        "real Origin Matsuribayashi prologue",
        |r| {
            game(r).is_some_and(|g| {
                g.host().is_some_and(|h| {
                    h.runner.native_save_metadata().is_some_and(|m| {
                        m.saved()[0].as_ref() == "ひぐらしオリジン・祭囃し・プロローグ"
                    })
                })
            })
        },
    );
    let g = game(&j.runtime).unwrap();
    assert_eq!(g.flags, 0);
    assert_eq!(
        g.runtime_game_data()
            .unwrap()
            .progress
            .active_entry()
            .scenario_id,
        10008
    );
    let common = j.runtime.globals().unwrap().unwrap().common.clone();
    let raw = common.scenario.raw();
    for (at, bytes) in [
        (
            0x1e265,
            &[0x46, 0x00, 0xb0, 0x90, 0x27, 0x18, 0xbe, 0x5d, 0x71, 0x01][..],
        ),
        (
            0x57813,
            &[
                0x42, 0x00, 0x10, 0x00, 0xd0, 0x00, 0x3c, 0x19, 0xff, 0x46, 0x01, 0xc0, 0x64, 0x00,
                0x2d, 0x78, 0x05, 0x00, 0x83, 0x00, 0xd0, 0x47, 0x30, 0x78, 0x05, 0x00, 0x83, 0x01,
                0xd0, 0x50,
            ][..],
        ),
        (
            0x17179a8,
            &[0x86, 0xf4, 0xcb, 0x02, 0x00, 0x7f, 0x83, 0x01][..],
        ),
        (
            0x1717b33,
            &[
                0x87, 0x00, 0x41, 0x00, 0x64, 0x00, 0x00, 0x4f, 0x13, 0x78, 0x05, 0x00, 0x01, 0x82,
                0xbc, 0x41, 0x00, 0x64, 0x00, 0x01, 0x88, 0x87, 0x01, 0x41, 0x00, 0x64, 0x00, 0x00,
                0x4f, 0x13, 0x78, 0x05, 0x00, 0x01, 0x83, 0x20, 0x41, 0x00, 0x64, 0x00, 0x01, 0x88,
                0x87, 0x7f, 0x8a, 0x00,
            ][..],
        ),
    ] {
        assert_eq!(
            &raw[at..at + bytes.len()],
            bytes,
            "actual authored bytes at{at:08x}"
        );
    }
    driver.until(
        &mut j,
        viewport,
        deadline,
        "quote's actual first MSGWAIT0 before helper",
        |r| {
            assert!(
                game(r).is_some(),
                "left Origin prologue before quote: {}",
                Driver::describe(r)
            );
            pending(r, 0x1717b33)
        },
    );
    assert_eq!(
        modes(&j.runtime) & 2,
        2,
        "canonical J has actually enabled skip"
    );
    assert!(eligible(&j.runtime));
    assert!(matches!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command(),
        Some(Cmd::MSGWAIT { signal: 0 })
    ));
    let clock = j.runtime.globals().unwrap().unwrap().clock.ticks();
    // A normal-input negative control needs no separate route. At this real
    // section boundary, fresh J disables skip before the first helper polls.
    assert!(matches!(
        j.step(&key(KeyCode::KeyJ), viewport),
        Frame::Advanced(_)
    ));
    assert_eq!(modes(&j.runtime) & 2, 0);
    assert!(!eligible(&j.runtime));
    let ordinary = remaining(&j.runtime, 42, 0).0;
    assert!(
        ordinary > 0.0 && ordinary <= 42.0,
        "non-skipped WAIT0 retains its real clock"
    );
    neutral(&mut j, viewport); // Release J and admit one ordinary timed tick.
    let later = remaining(&j.runtime, 42, 0).0;
    assert!(
        later > 0.0 && later < ordinary,
        "ordinary pause advances without an injected time"
    );
    // Use held Ctrl or a genuine R/J edge. This same retained WAIT0 polls
    // false, then eligible native post-poll skip must clear its clock only.
    assert!(matches!(
        j.step(
            &key(if temporary {
                KeyCode::ControlLeft
            } else {
                KeyCode::KeyJ
            }),
            viewport
        ),
        Frame::Advanced(_)
    ));
    assert_eq!(modes(&j.runtime) & 2, if temporary { 0 } else { 2 });
    assert_eq!(
        game(&j.runtime).unwrap().reference_effective_play_modes() & 2,
        2
    );
    let (remaining_first, first_commands) = remaining(&j.runtime, 42, 0);
    assert_eq!(
        remaining_first, 0.0,
        "eligible skip must clear real non-interruptible700ms pause after failed poll"
    );
    signal_after_one_callback(&mut j, viewport, 42, first_commands, 1, temporary);
    // No full42/48-tick delay is allowed between the actual signal barriers.
    // All intervening instructions are original MSGWAIT/CALL/assignments.
    let between = j.runtime.frames;
    for _ in 0..4 {
        let ready = game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .pending_command()
            .is_some_and(|c| {
                matches!(
                    c,
                    Cmd::WAIT {
                        allow_interrupt: 0,
                        amount: 48
                    }
                )
            });
        if ready {
            break;
        }
        skip_frame(&mut j, viewport, temporary);
    }
    assert!(j.runtime.frames - between <= 4);
    assert_eq!(
        game(&j.runtime).unwrap().reference_effective_play_modes() & 2,
        2
    );
    let (remaining_second, second_commands) = remaining(&j.runtime, 48, 1);
    assert_eq!(
        remaining_second, 0.0,
        "eligible skip clears the second actual800ms WAIT0"
    );
    signal_after_one_callback(&mut j, viewport, 48, second_commands, 2, temporary);
    let after_signal = j.runtime.frames;
    for _ in 0..4 {
        if game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id
            == native_message_id(QUOTE + 1)
        {
            break;
        }
        skip_frame(&mut j, viewport, temporary);
    }
    assert!(j.runtime.frames - after_signal <= 4);
    assert_eq!(
        game(&j.runtime)
            .unwrap()
            .host()
            .unwrap()
            .runner
            .frame()
            .message
            .playback
            .message_id,
        native_message_id(QUOTE + 1),
        "actual next message after both authored @y barriers"
    );
    if temporary {
        neutral(&mut j, viewport);
        assert_eq!(modes(&j.runtime), 0, "Ctrl never latches native skip");
        assert_eq!(
            game(&j.runtime).unwrap().reference_effective_play_modes(),
            0,
            "physical release stops temporary skip"
        );
    }
    assert_eq!(identities(&j.runtime), shared);
    assert_eq!(manuals(&j.runtime), manual);
    assert!(j.runtime.globals().unwrap().unwrap().clock.ticks() > clock);
    assert!(j.runtime.failure().is_none());
    j.no_writes(Some(&disk));
    println!(
        "Origin7/10008 real quote183284: normal-input skip-off clock advances; J/Ctrl re-enable clears actual WAIT0(42),following callback runsMSG SIGNAL1; WAIT0(48) clears and following callback runsMSG SIGNAL2; actual nextMSG183285;no clock/script/policy injection,all100manuals/shared owners preserved/no writes;{}Runtimeframes/{}driven callbacks",
        j.runtime.frames, driver.callbacks
    );
}
