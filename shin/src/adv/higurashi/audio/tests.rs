use super::super::{
    music_commands,
    runner::{HigurashiRunner, Policy, Status},
};
use super::*;
use shin_core::{
    format::text::U8String,
    vm::{command::CommandResult, higurashi::command::HigurashiRuntimeCommand as Cmd},
};
use std::time::{Duration, Instant};

mod modal;

fn policy() -> Policy {
    Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 50,
        outline_mode: 0,
    }
}
#[test]
#[ignore = "loads actual authored BGM through retained VM and shared audio on offscreen Metal/the default audio output"]
fn higurashi_audio_runs_authored_bgm_and_retains_music_waits() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut audio = HigurashiAudio::new(manager, assets.clone(), authored.clone());
    audio.set_preferences(100, u32::MAX, true, 100);
    audio.set_paused(true);
    // Keep first authored BGMPLAY at2545e unchanged; replace only its following
    // control stream with a bounded wait/volume/stop fixture. No input is written.
    let mut raw = authored.raw().to_vec();
    let code = [
        0x93, 1, 0x94, 1, 0x92, 0x81, 0xf4, 12, 0x92, 0, 12, 0x93, 4, 0x90, 0x80, 114, 6, 1, 0x82,
        0xee, 0x91, 12, 0x93, 0x7f, 0x94, 0x7f, 0, 0, 0,
    ];
    raw[0x25465..0x25465 + code.len()].copy_from_slice(&code);
    let mut scenario = HigurashiScenario::new(raw.into()).unwrap();
    // The file's code offset also bounds its final metadata table. Keep that
    // boundary intact while parsing, then select this fixture's VM entry.
    scenario.header.code_offset = 0x2545e;
    let scenario = Arc::new(scenario);
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario,
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let pump = |runner: &mut HigurashiRunner, p: Policy, audio: &mut HigurashiAudio| {
        shin_tasks::block_on(runner.pump(p, 1, 100, || [1, 2, 3, 4], audio)).unwrap()
    };
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.bgm_status(), 3);
    assert_eq!(audio.bgm_clock(), 0);
    assert_eq!(audio.bgm.reference_loading_loop_count(), Some(0));
    assert_eq!(
        audio.pop_music_event(),
        Some(MusicEvent::Started(BgmStarted {
            id: 114,
            display_name: authored.bgm_info[114].display_name.as_str().into(),
            linked_id: authored.bgm_info[114].linked_bgm_id
        }))
    );
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    let retained = (
        runner.position(),
        runner.next_position(),
        runner.completed_commands(),
    );
    assert!(runner.complete_external(CommandResult::None).is_err());
    for _ in 0..3 {
        assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
        assert_eq!(
            (
                runner.position(),
                runner.next_position(),
                runner.completed_commands()
            ),
            retained
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.bgm_status() & 1 != 0 {
        assert!(Instant::now() < deadline, "BGM load timed out");
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(audio.bgm_status(), 2);
    assert_eq!(audio.bgm_clock(), 0);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    let mut skip = policy();
    skip.fast_skip = true;
    skip.suppress_transient_audio = true;
    assert_eq!(pump(&mut runner, skip, &mut audio), Status::Waiting);
    let mut paused = policy();
    paused.paused = true;
    assert_eq!(pump(&mut runner, paused, &mut audio), Status::Waiting);
    audio.set_paused(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.bgm_clock() < music_commands::sync_target(1) {
        assert!(Instant::now() < deadline, "music clock did not advance");
        audio.update(Ticks::from_f32(1.0), false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.bgm_status() & 4 == 0 {
        assert!(Instant::now() < deadline);
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.bgm_status() & 4 != 0 {
        assert!(Instant::now() < deadline, "volume queue did not finish");
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    let before = audio.bgm_clock();
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert!(audio.bgm_clock() >= before);
    assert_eq!(audio.pop_music_event(), None);
    assert_eq!(audio.bgm.reference_loading_loop_count(), None);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.bgm_status(), 0);
    assert_eq!(audio.bgm_clock(), 0);
    assert_eq!(audio.pop_music_event(), Some(MusicEvent::Stopped));
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Exited);
    assert_eq!(runner.completed_commands(), 11);
    // Same-ID loading keeps the initial infinite repeat policy. A missing new
    // file retires its loader and releases waits without reviving the old entry.
    audio.play_bgm(0, 60, 0, 1.0).unwrap();
    audio.play_bgm(0, 12, 3, 0.5).unwrap();
    assert_eq!(audio.bgm.reference_loading_loop_count(), Some(0));
    assert!(matches!(
        audio.pop_music_event(),
        Some(MusicEvent::Started(BgmStarted { id: 0, .. }))
    ));
    assert_eq!(audio.pop_music_event(), None);
    audio.stop_bgm(0);
    assert_eq!(audio.bgm_status(), 0);
    audio.pop_music_event();
    let mut bad = HigurashiScenario::new(authored.raw().to_vec().into()).unwrap();
    bad.bgm_info[1].name = U8String::new("missing-file");
    audio.scenario = Arc::new(bad);
    audio.play_bgm(1, 0, 1, 1.0).unwrap();
    assert_eq!(audio.bgm_status(), 3);
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.bgm_status() & 1 != 0 {
        assert!(Instant::now() < deadline);
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(audio.bgm_status(), 0);
    assert!(music_commands::MusicWait::Status(-1).complete(&audio));
    audio.play_bgm(i32::MIN, 0, 0, 1.0).unwrap();
    assert_eq!(audio.bgm_status(), 0);
    audio.update(Ticks::from_f32(100.0), false);
    println!(
        "Higurashi BGM audio: the default audio output; unchanged authored PLAY at2545e loads HM20_A5;11 real VM commands, async load/paused zero clock, native microsecond SYNC, queued volumes/status waits, pause/skip/PC retention, same-ID reuse/original loop policy, stop notifications, canceled load, missing asset and invalid-ID retirement pass; native title policy remains host work"
    );
}

mod effects;
mod reconstruction;
