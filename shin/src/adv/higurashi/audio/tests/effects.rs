use super::*;
use crate::adv::higurashi::effect_commands::{EffectPlay, EffectWait};
fn append(code: &mut Vec<u8>, opcode: u8, args: &[i32]) {
    code.push(opcode);
    for &v in args {
        assert!((-2048..=2047).contains(&v));
        if (-64..64).contains(&v) {
            code.push((v as u8) & 127);
        } else {
            code.extend([0x80 | ((v >> 8) & 15) as u8, v as u8]);
        }
    }
}
fn pump(r: &mut HigurashiRunner, p: Policy, a: &mut HigurashiAudio) -> Status {
    shin_tasks::block_on(r.pump(p, 1, 100, || [1, 2, 3, 4], a)).unwrap()
}
fn loaded(a: &mut HigurashiAudio, slot: i32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while a.effect_status(slot) & 1 != 0 {
        assert!(Instant::now() < deadline, "SE load timed out");
        a.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
#[ignore = "actual looping SE/exact Loopback Audio;idempotent skip bursts must not fill the audio queue or swallow later authored volume/pan/reuse changes"]
fn higurashi_repeated_effect_skip_coalesces_until_authored_parameters_change() {
    let (_, _, assets)=super::super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets=Arc::new(assets);
    let scenario=shin_tasks::block_on(assets.load::<HigurashiScenario,_>("/main.snr")).unwrap();
    let mut audio=HigurashiAudio::new(Arc::new(AudioManager::new_for_tests()),assets,scenario);
    audio.set_paused(true);
    let request=EffectPlay{slot:0,id:48,fade:0,count:0,volume:0.8,pan:0.0,speed:1.0};
    audio.play_effect(request).unwrap();
    loaded(&mut audio,0);
    assert_eq!(audio.effect_status(0),2);
    for _ in 0..4096 {audio.effects.fast_forward().unwrap();}
    audio.volume_effect(0,0.2,1200).unwrap();
    audio.pan_effect(0,0.5,1200).unwrap();
    let mut wait=|mask,expected|{
        let deadline=Instant::now()+Duration::from_secs(2);
        while audio.effect_status(0)&mask!=expected {
            assert!(Instant::now()<deadline,"authored effect status{:x},wanted{expected:x} under{mask:x}",audio.effect_status(0));
            audio.update(Ticks::ZERO,false);
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    wait(12,12);
    drop(wait);
    for _ in 0..4096 {audio.effects.fast_forward().unwrap();}
    let deadline=Instant::now()+Duration::from_secs(2);
    while audio.effect_status(0)&12!=0 {
        assert!(Instant::now()<deadline,"new volume/pan were not finished");
        audio.update(Ticks::ZERO,false);
        std::thread::sleep(Duration::from_millis(2));
    }
    audio.play_effect(EffectPlay{fade:1200,volume:0.6,pan:-0.25,speed:1.5,..request}).unwrap();
    let deadline=Instant::now()+Duration::from_secs(2);
    while audio.effect_status(0)&28!=28 {
        assert!(Instant::now()<deadline,"repeat reuse did not install all three tweens");
        audio.update(Ticks::ZERO,false);
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..4096 {audio.effects.fast_forward().unwrap();}
    let deadline=Instant::now()+Duration::from_secs(2);
    while audio.effect_status(0)&12!=0 {
        assert!(Instant::now()<deadline,"reused volume/pan were not finished");
        audio.update(Ticks::ZERO,false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_ne!(audio.effect_status(0)&16,0,"skip must preserve the authored speed tween");
    assert_eq!(audio.effects.position(0),Some(0));
    audio.stop_all_effects(0).unwrap();
    println!("Higurashi SE skip:12288 consecutive finish requests do not overflow;authored volume/pan and matching repeat invalidate coalescing,preserve FIFO completion/speed tween and paused source position on exact Loopback Audio");
}
#[test]
#[ignore = "actual authored SE through retained VM on offscreen Metal/exact Loopback Audio"]
fn higurashi_audio_runs_authored_se_with_signed_slots_and_retained_waits() {
    let (_, _, assets) = super::super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    assert_eq!(authored.se_info[48].name.as_str(), "Kyupirn");
    let mut audio = HigurashiAudio::new(
        Arc::new(AudioManager::new_for_tests()),
        assets.clone(),
        authored.clone(),
    );
    audio.set_preferences(100, u32::MAX, true, 100);
    audio.set_effects_volume(100);
    audio.set_paused(true);
    let mut raw = authored.raw().to_vec();
    let mut code = Vec::new();
    append(&mut code, 0x9a, &[0, 1]);
    append(&mut code, 0x9a, &[0, 2]);
    append(&mut code, 0x95, &[64, 48, 12, 0, 800, 500, 1500]);
    append(&mut code, 0x9a, &[-1, 2]);
    append(&mut code, 0x98, &[64, 600, 12]);
    append(&mut code, 0x99, &[64, -500, 12]);
    append(&mut code, 0x9a, &[64, 12]);
    append(&mut code, 0x96, &[64, 60]);
    append(&mut code, 0x95, &[64, 48, 6, 0, 750, 0, 1000]);
    append(&mut code, 0x9b, &[495, 2, 1000, 0, 0]);
    append(&mut code, 0x9b, &[495, 0, 1000, 0, 1000]);
    append(&mut code, 0x96, &[-1, 0]);
    append(&mut code, 0x97, &[0]);
    append(&mut code, 0x9a, &[-1, -1]);
    append(&mut code, 0, &[0, 0]);
    raw[0x30295..0x30295 + code.len()].copy_from_slice(&code);
    let mut scenario = HigurashiScenario::new(raw.into()).unwrap();
    scenario.header.code_offset = 0x3028b;
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets,
        Arc::new(scenario),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.effect_status(0), 3);
    assert_eq!(audio.effect_descriptor_id(0), -1);
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
    loaded(&mut audio, 0);
    assert_eq!(audio.effect_status(0), 2);
    assert_eq!(audio.effects.position(0), Some(0));
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
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.effect_status(0) != 0 {
        assert!(
            Instant::now() < deadline,
            "authored finite SE did not finish"
        );
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    audio.set_paused(true);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.effect_descriptor_id(64), 48);
    assert_eq!(audio.effect_status(64), 7);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    ); // Aggregate WAIT ignores this repeating descriptor.
    loaded(&mut audio, 64);
    assert_eq!(audio.effects.position(64), Some(0));
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.effect_status(64) & 12 != 12 {
        assert!(Instant::now() < deadline);
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.effect_status(64) & 12 != 0 {
        assert!(
            Instant::now() < deadline,
            "queued SE controls did not finish"
        );
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    let position = audio.effects.position(64);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.effect_status(64), 0);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert!(audio.effect_status(64) & 2 != 0);
    assert_eq!(audio.effects.position(64), position);
    assert_eq!(audio.effect_descriptor_id(64), 48);
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    assert_eq!(audio.effect_status(-1) & 3, 3);
    let retained = (
        runner.position(),
        runner.next_position(),
        runner.completed_commands(),
    );
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Waiting);
    assert_eq!(
        (
            runner.position(),
            runner.next_position(),
            runner.completed_commands()
        ),
        retained
    );
    loaded(&mut audio, -1);
    assert_eq!(audio.effect_status(-1), 2);
    assert_eq!(audio.effects.position(-1), Some(0));
    assert_eq!(pump(&mut runner, skip, &mut audio), Status::CommandBudget);
    assert_eq!(audio.effect_status(-1), 2); // Skip releases ONCE without stopping it.
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.effect_status(-2), 3);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(audio.effect_status(-1), 0);
    assert_eq!(audio.effect_status(-2), 3);
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert!(audio.active_effect_slots().is_empty());
    assert_eq!(
        pump(&mut runner, policy(), &mut audio),
        Status::CommandBudget
    );
    assert_eq!(pump(&mut runner, policy(), &mut audio), Status::Exited);
    assert_eq!(runner.completed_commands(), 16);
    // Same-ID loading retains the original worker. Suppressed finite PLAY
    // still changes the descriptor, so aggregate WAIT is independent of count.
    let request = EffectPlay {
        slot: -7,
        id: 48,
        fade: 60,
        count: 0,
        volume: 1.0,
        pan: 0.0,
        speed: 1.0,
    };
    audio.play_effect(request).unwrap();
    audio
        .play_effect(EffectPlay {
            volume: 0.5,
            pan: 0.5,
            speed: 1.5,
            ..request
        })
        .unwrap();
    assert_eq!(audio.effect_status(-7), 31);
    audio.stop_effect(-7, 0).unwrap();
    audio.update(Ticks::ZERO, false);
    assert_eq!(audio.effect_status(-7), 0);
    let mut bad = HigurashiScenario::new(authored.raw().to_vec().into()).unwrap();
    bad.se_info[48].name = U8String::new("missing-effect");
    audio.scenario = Arc::new(bad);
    audio
        .play_effect(EffectPlay {
            slot: i32::MIN,
            count: 1,
            ..request
        })
        .unwrap();
    assert_eq!(audio.effect_status(i32::MIN), 7);
    loaded(&mut audio, i32::MIN);
    assert_eq!(audio.effect_status(i32::MIN), 0);
    assert!(EffectWait::Status { slot: -1, mask: -1 }.complete(&audio, false));
    audio
        .play_effect(EffectPlay {
            id: i32::MIN,
            ..request
        })
        .unwrap();
    assert_eq!(audio.effect_status(-7), 0);
    audio.update(Ticks::ZERO, false);
    println!(
        "Higurashi SE audio: exact Loopback Audio; unchanged authored PLAY at3028b loads Kyupirn;16 real VM commands pass finite EOF, async paused loads, retained PC/status waits, dynamic signed slot64 and negative controls, aggregate descriptor filtering, queued volume/pan, stopping repeat reuse, ONCE reservation/fixed-speed/skip release, stop-all/cancellation and missing/invalid asset retirement; ordinary host policy remains pending"
    );
}
