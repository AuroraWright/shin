use super::*;
use crate::adv::higurashi::{
    checkpoints::CheckpointServices,
    effect_commands::EffectPlay,
    frame::tests::gpu::render_to_texture,
    host::HigurashiHost,
    message_glyphs::tests::gpu::gpu_assets,
    read_history::MessageReadHistory,
    read_state::ReadPolicy,
    replay::tests::{scenario, session},
};
use shin_core::primitives::color::UnormColor;

#[test]
#[ignore = "actual nested host modal scopes, retained textures and voice/SE/BGM on Metal/exact Loopback Audio"]
fn higurashi_host_modal_pauses_voice_se_keeps_bgm_and_restores_counted_owners() {
    for native in [false, true] {
        run_host_modal(native);
    }
}

fn run_host_modal(native: bool) {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let script = scenario(&[0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0, 0, 0]);
    let mut initial = session(script.clone(), 0, 0, 42);
    let mut history = MessageReadHistory::new();
    let mut read_services = CheckpointServices {
        memory: &mut initial.memory,
        progress: &mut initial.progress,
        history: &mut history,
    };
    let read = ReadPolicy {
        modes: 0,
        forced: false,
        read_only: true,
    };
    let runner = shin_tasks::block_on(HigurashiRunner::load(assets.clone(), script, 0, 42, || {
        [1; 4]
    }))
    .unwrap();
    let mut audio = HigurashiAudio::new(Arc::new(AudioManager::new_for_tests()), assets, authored);
    audio.set_preferences(100, u32::MAX, true, 100);
    audio.set_effects_volume(100);
    audio.set_paused(true);
    audio.play_bgm(114, 0, 0, 0.8).unwrap();
    audio.play_command("S10/40/430100047", 1.0, 1);
    let effect = |slot| EffectPlay {
        slot,
        id: 48,
        fade: 0,
        count: 0,
        volume: 0.8,
        pan: 0.0,
        speed: 1.0,
    };
    audio.play_effect(effect(0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while audio.bgm_status() & 1 != 0
        || audio.status_bits() & 1 != 0
        || audio.effect_status(0) & 1 != 0
    {
        assert!(Instant::now() < deadline, "modal audio loads timed out");
        audio.update(Ticks::ZERO, false);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(audio.bgm_status() & 2 != 0);
    assert!(audio.status_bits() & 2 != 0);
    assert!(audio.effect_status(0) & 2 != 0);
    let mut host = HigurashiHost::new(runner, audio);
    let p = policy();
    assert_eq!(
        if native {
            shin_tasks::block_on(host.runner.pump_native(
                p,
                read,
                &mut read_services,
                8,
                10000,
                || [1; 4],
                &mut host.audio,
            ))
            .unwrap()
        } else {
            shin_tasks::block_on(host.runner.pump(p, 8, 10000, || [1; 4], &mut host.audio)).unwrap()
        },
        Status::Waiting
    );
    host.runner
        .advance(Ticks::from_f32(15.0), p, &mut host.audio, None);
    let glyphs = host.runner.frame().message.glyphs().unwrap() as *const _;
    let position = host.runner.position();
    let audio_policy = AudioFramePolicy {
        focus_mode_active: false,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    };
    let mut update = |host: &mut HigurashiHost, ticks| {
        if native {
            return shin_tasks::block_on(host.update_native(
                Ticks::from_f32(ticks),
                p,
                read,
                &mut read_services,
                audio_policy,
                8,
                10000,
                || [1; 4],
            ))
            .unwrap();
        }
        shin_tasks::block_on(
            host.update(Ticks::from_f32(ticks), p, audio_policy, 8, 10000, || [1; 4]),
        )
        .unwrap()
    };
    let capture = |runner: &HigurashiRunner, size, expected_hidden| {
        assert_eq!(
            runner.frame().message.playback.hidden,
            expected_hidden,
            "capture precedes conceal"
        );
        let mut snapshot = None;
        render_to_texture(&device, &queue, size, |context, _| {
            let mut texture = context.new_render_texture("Modal scope capture fixture".into());
            let mut pass = context.begin_pass(
                texture.as_texture_target(),
                None,
                "Modal scope captured pattern",
            );
            pass.clear(Some(UnormColor::from_rgba(100, 50, 25, 255)), None, None);
            drop(pass);
            snapshot = Some(texture);
        });
        Ok(snapshot.unwrap())
    };
    // Failure occurs before replacing the current snapshot or acquiring scopes.
    assert!(
        host.enter_modal(|_| anyhow::bail!("capture failure"))
            .is_err()
    );
    assert_eq!(host.modal_pause_depth(), 0);
    assert!(!host.runner.frame().message.playback.hidden);
    assert!(host.modal_snapshot().is_none());
    host.audio.set_paused(false);
    std::thread::sleep(Duration::from_millis(80));
    assert!(host.audio.bgm_clock() > 0);
    assert!(host.audio.position_micros() > 0);
    assert!(host.audio.effects.position(0).unwrap() > 0);
    host.enter_modal(|runner| capture(runner, (1280, 720), false))
        .unwrap();
    assert_eq!(host.modal_pause_depth(), 1);
    assert_eq!(host.modal_snapshot().unwrap().dimensions(), (1280, 720));
    assert!(host.runner.frame().message.playback.hidden);
    assert!(host.ui.indicator.hidden);
    assert!(host.ui.menu.indicator.hidden);
    assert!(host.audio.modal_effects_paused());
    std::thread::sleep(Duration::from_millis(80));
    let voice = host.audio.position_micros();
    let se = host.audio.effects.position(0);
    let bgm = host.audio.bgm_clock();
    assert!(voice > 0);
    assert!(se.unwrap() > 0);
    // A newly completed SE worker remains unpublished while c3e70 is gated.
    host.audio.play_effect(effect(64)).unwrap();
    host.enter_modal(|runner| capture(runner, (1920, 1080), true))
        .unwrap();
    assert_eq!(host.modal_pause_depth(), 2);
    assert_eq!(host.modal_snapshot().unwrap().dimensions(), (1920, 1080));
    host.exit_modal();
    assert_eq!(host.modal_pause_depth(), 1);
    assert!(host.audio.modal_effects_paused());
    assert!(host.runner.frame().message.playback.hidden);
    for ticks in [3.0, 3.0, 3.0, 1.0] {
        assert_eq!(update(&mut host, ticks), Status::Waiting);
    }
    assert_eq!(host.runner.position(), position);
    assert_eq!(
        host.runner.frame().message.glyphs().unwrap() as *const _,
        glyphs
    );
    // The message reveal/modal clocks themselves are inside the scene gate.
    assert_eq!(host.runner.frame().message.playback.modal_slide, 1.0);
    assert_eq!(host.ui.indicator.opacity, 0.0);
    assert_eq!(host.modal_progress(), 0.0);
    assert!(host.modal_snapshot().is_none());
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(host.audio.position_micros(), voice);
    assert_eq!(host.audio.effects.position(0), se);
    assert!(host.audio.bgm_clock() > bgm);
    assert!(
        host.audio.effect_status(64) & 1 != 0,
        "modal SE gate must retain loader"
    );
    // Reopening replaces the capture and interrupts the closing fade.
    host.enter_modal(|runner| capture(runner, (1280, 720), true))
        .unwrap();
    for ticks in [3.0, 3.0, 3.0, 1.0] {
        update(&mut host, ticks);
    }
    assert_eq!(host.modal_progress(), 1.0);
    host.exit_modal();
    host.exit_modal();
    assert_eq!(host.modal_pause_depth(), 0);
    assert!(!host.audio.modal_effects_paused());
    assert!(!host.runner.frame().message.playback.hidden);
    assert!(!host.ui.indicator.hidden);
    assert!(!host.ui.menu.indicator.hidden);
    assert!(host.modal_snapshot().is_some());
    assert_eq!(host.modal_progress(), 1.0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.audio.effect_status(64) & 1 != 0 {
        assert!(Instant::now() < deadline);
        update(&mut host, 0.0);
        std::thread::sleep(Duration::from_millis(2));
    }
    std::thread::sleep(Duration::from_millis(80));
    assert!(host.audio.position_micros() > voice || host.audio.status_bits() & 2 == 0);
    assert!(host.audio.effects.position(0) > se);
    for ticks in [3.0, 3.0, 3.0, 1.0] {
        update(&mut host, ticks);
    }
    assert_eq!(host.modal_progress(), 0.0);
    assert!(host.modal_snapshot().is_none());
    assert_eq!(host.ui.indicator.opacity, 1.0);
    assert_eq!(
        host.runner.frame().message.glyphs().unwrap() as *const _,
        glyphs
    );
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    println!(
        "Higurashi host modal:exact Loopback Audio; native={native}, real voice/SE pause, global BGM clock continues, nested conceal/pause, independent UI/fade, retained VM/glyphs, gated SE loader, snapshot replacement/retirement at720p/1080p, restoration and failed capture pass; blur pixels remain outside this scope fixture"
    );
}
