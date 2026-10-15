use super::*;
use crate::adv::higurashi::{
    audio::{AudioFramePolicy, HigurashiAudio},
    checkpoints::CheckpointServices,
    frame::tests::gpu::render_to_texture,
    host::HigurashiHost,
    input::{Buttons, Context, InputState},
    message_glyphs::tests::gpu::gpu_assets,
    read_history::MessageReadHistory,
    read_state::ReadPolicy,
    reconstruction_mode::Phase,
    replay::tests::{scenario, session},
    runner::{HigurashiRunner, Policy, Status},
    scene_textures::HigurashiSceneTextures,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "real host reconstruction UI/input clocks on offscreen Metal and exact Loopback Audio"]
fn higurashi_host_owns_reconstruction_resets_and_detached_vm_input() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let script = scenario(&[0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A', 0, 0, 0, 0]);
    let mut initial = session(script.clone(), 0, 0, 42);
    let replay = session(script.clone(), 0, 0, 42);
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        script.clone(),
        0,
        42,
        || [1; 4],
    ))
    .unwrap();
    let audio = HigurashiAudio::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        assets,
        script,
    );
    let mut host = HigurashiHost::new(runner, audio);
    let mut history = MessageReadHistory::new();
    let policy = Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 50,
        outline_mode: 0,
    };
    let read = ReadPolicy {
        modes: 0,
        forced: false,
        read_only: true,
    };
    let audio_policy = AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    };
    let mut services = CheckpointServices {
        memory: &mut initial.memory,
        progress: &mut initial.progress,
        history: &mut history,
    };
    assert_eq!(
        shin_tasks::block_on(host.runner.pump_native(
            policy,
            read,
            &mut services,
            8,
            10000,
            || [1; 4],
            &mut host.audio
        ))
        .unwrap(),
        Status::Waiting
    );
    host.ui.pulse_hint();
    host.ui.indicator.hidden = true;
    host.ui.menu.indicator.hidden = true;
    host.ui.menu.elapsed = 17.0;
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut screens = HigurashiSceneTextures::new(context);
        host.runner
            .capture_and_begin_reconstruction_mode(
                context,
                &mut screens,
                replay,
                1,
                services.history.clone(),
                || [2; 4],
            )
            .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while host.poll_reconstruction_mode().unwrap() == Phase::Preparing {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.runner.reconstruction_phase(), Some(Phase::Fading));
    assert!(host.runner.native_read_state().is_none());
    assert_eq!(host.ui.indicator.opacity, 1.0);
    assert!(!host.ui.indicator.hidden);
    assert_eq!(host.ui.menu.elapsed, 0.0);
    assert!(!host.ui.hint.is_waiting());
    let context = Context {
        permission: true,
        captured: false,
        disabled: false,
        system_locked: false,
        vm: None,
        paused: false,
        continuation_blocked: false,
        overlay_transition_busy: false,
        overlay_transition_value: 0.0,
        notification_busy: false,
        animations_enabled: true,
        overlay: None,
        hint_busy: false,
        scene_jump_available: false,
        wait_allow_interrupt: 1,
    };
    let mut input = InputState {
        modes: 0,
        temporary_indicator: 9,
    };
    let outcome = host
        .process_input_native(
            &mut input,
            context,
            Buttons {
                pressed: u32::MAX,
                ..Default::default()
            },
            &mut services,
        )
        .unwrap();
    assert_eq!(outcome.requests, [crate::adv::higurashi::input::Request::FlushInput]);
    assert!(!outcome.overlay_input_enabled);
    assert_eq!(input.modes, 0);
    let mut paused = policy;
    paused.paused = true;
    assert_eq!(
        shin_tasks::block_on(host.update_native(
            Ticks::from_f32(120.0),
            paused,
            read,
            &mut services,
            audio_policy,
            8,
            10000,
            || [3; 4]
        ))
        .unwrap(),
        Status::Waiting
    );
    assert_eq!(
        host.runner.reconstruction_overlay().unwrap().clock.elapsed,
        0.0
    );
    assert_eq!(host.ui.menu.elapsed, 3.0);
    assert_eq!(
        shin_tasks::block_on(host.update_native(
            Ticks::from_f32(3.0),
            policy,
            read,
            &mut services,
            audio_policy,
            8,
            10000,
            || [3; 4]
        ))
        .unwrap(),
        Status::Waiting
    );
    assert_eq!(host.poll_reconstruction_mode().unwrap(), Phase::Fading);
    assert_eq!(
        shin_tasks::block_on(host.update_native(
            Ticks::from_f32(1.0),
            policy,
            read,
            &mut services,
            audio_policy,
            8,
            10000,
            || [3; 4]
        ))
        .unwrap(),
        Status::Waiting
    );
    assert_eq!(host.poll_reconstruction_mode().unwrap(), Phase::Complete);
    assert!(host.runner.native_read_state().is_some());
    assert!(host.runner.take_reconstruction_result().is_some());
    assert!(host.has_modes());
    let blocked = host.process_input_native(
        &mut input, context,
        Buttons { pressed: 1 << 10, ..Default::default() }, &mut services,
    ).unwrap();
    assert_eq!(blocked.requests, [crate::adv::higurashi::input::Request::FlushInput]);
    assert_eq!(input.modes, 0);
    // Installing the VM did not retire its mode or grant permission within
    // that frame. The next native stack poll prunes it before logical input.
    assert!(host.poll_modes(Ticks::ZERO, 1).unwrap());
    assert!(!host.has_modes());
    let enabled = host.process_input_native(
        &mut input, context,
        Buttons { pressed: 1 << 10, ..Default::default() }, &mut services,
    ).unwrap();
    assert!(enabled.requests.contains(&crate::adv::higurashi::input::Request::CueEnabled));
    assert_eq!(input.modes, 1);
    assert!(!host.poll_modes(Ticks::ZERO, 2).unwrap());
    let blocked = host.process_input_native(&mut input, context, Buttons::default(), &mut services).unwrap();
    assert_eq!(blocked.requests, [crate::adv::higurashi::input::Request::FlushInput]);
    assert_eq!(input.modes, 1);
    println!(
        "Higurashi host UI:real resets/clocks and detached/completion-frame input; mode retirement, permission reopening and low-bit initial permission pass"
    );
}
