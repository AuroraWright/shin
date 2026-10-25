use super::super::{
    audio::HigurashiAudio,
    host::HigurashiHost,
    input::{Buttons, Context, InputState, ReadState, Request},
};
use super::*;

fn context() -> Context {
    Context {
        permission: true,
        captured: false,
        disabled: false,
        system_locked: false,
        vm: Some(ReadState {
            flags: 0x8000_0000,
            fast_forward_blocked: 0,
            is_read: 1,
        }),
        paused: false,
        continuation_blocked: false,
        overlay_transition_busy: false,
        overlay_transition_value: 0.0,
        notification_busy: false,
        animations_enabled: true,
        overlay: None,
        hint_busy: false,
        scene_jump_available: false,
        // Deliberately wrong: host must use its own retained WAIT owner.
        wait_allow_interrupt: 2,
    }
}
fn remaining(host: &HigurashiHost) -> f32 {
    match &host.runner.pending.as_ref().unwrap().owner {
        Owner::Clock(wait) => wait.remaining,
        _ => panic!("Expected retained WAIT"),
    }
}

#[test]
#[ignore = "real message and WAIT owners on offscreen Metal and the default audio output"]
fn higurashi_host_input_drives_retained_message_before_wait() {
    use shin_core::layout::commands::{Command, Wait as MessageWait};
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut raw = scenario.raw().to_vec();
    let start = scenario.header.code_offset as usize;
    raw[start..start + 6].copy_from_slice(&[0x83, 1, 12, 0, 0, 0]);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut host = HigurashiHost::new(
        runner,
        HigurashiAudio::new(
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            assets,
            scenario,
        ),
    );
    let policy = super::tests::policy();
    assert_eq!(
        shin_tasks::block_on(
            host.runner
                .pump(policy, 8, 1000, || [1, 2, 3, 4], &mut host.audio)
        )
        .unwrap(),
        Status::Waiting
    );
    assert_eq!(remaining(&host), 12.0);
    let retained = (
        host.runner.position(),
        host.runner.next_position(),
        host.runner.random_state(),
        host.runner.completed_commands(),
    );
    let mut state = InputState {
        modes: 0,
        temporary_indicator: 9,
    };
    let press = |bit| Buttons {
        pressed: 1u32 << bit,
        ..Default::default()
    };
    let playback = &mut host.runner.frame.message.playback;
    playback.install(0, [0; 2], 0, 0, 7, false);
    playback.set_layout(
        &[Command::Wait(MessageWait {
            time: 0.0,
            line_index: 0,
            is_last_wait: true,
            is_auto_click: false,
        })],
        &[],
    );
    // Native click consumes confirm while a message block exists, even before
    // its clock has installed the final wait. It must not reach ADV408.
    assert!(playback.interested_in_input());
    let outcome = host.process_input(&mut state, context(), press(6));
    assert!(!outcome.overlay_input_enabled && outcome.requests.is_empty());
    assert_eq!(remaining(&host), 12.0);
    let playback = &mut host.runner.frame.message.playback;
    playback.wait = Some(super::super::message_playback::WaitKind::Last);
    let outcome = host.process_input(&mut state, context(), press(24));
    assert!(!outcome.overlay_input_enabled && outcome.requests.is_empty());
    assert_eq!(host.runner.frame.message.playback.current_block, 1);
    assert_eq!(
        host.runner.frame.message.playback.completed_messages,
        vec![7]
    );
    assert_eq!(remaining(&host), 12.0);
    // Next confirm sees the completed message and the real allow_interrupt=1.
    let outcome = host.process_input(&mut state, context(), press(6));
    assert!(!outcome.overlay_input_enabled && outcome.requests.is_empty());
    assert_eq!(remaining(&host), 0.0);
    assert_eq!(
        retained,
        (
            host.runner.position(),
            host.runner.next_position(),
            host.runner.random_state(),
            host.runner.completed_commands()
        )
    );
    assert_eq!(
        shin_tasks::block_on(
            host.runner
                .pump(policy, 8, 1000, || [1, 2, 3, 4], &mut host.audio)
        )
        .unwrap(),
        Status::Exited
    );
    assert_eq!(host.runner.completed_commands(), 2);
    // Non-message mode owns its real hint tween. Caller busy flags cannot
    // restart an active pulse, and no external HintPulse request remains.
    let mut hint_context = context();
    hint_context.vm.as_mut().unwrap().flags = 0;
    let outcome = host.process_input(&mut state, hint_context, press(24));
    assert!(outcome.requests.is_empty());
    assert!(host.ui.hint.is_waiting());
    host.ui.hint.update(Ticks::from_f32(16.0));
    assert_eq!(host.ui.hint.value(), 0.5);
    let outcome = host.process_input(&mut state, hint_context, press(24));
    assert!(outcome.requests.is_empty());
    assert_eq!(host.ui.hint.value(), 0.5);
    // Requests remain visible to the caller; input does not acknowledge a
    // scene/menu continuation or start a fabricated modal owner.
    let outcome = host.process_input(&mut state, context(), press(9));
    assert_eq!(outcome.requests, vec![Request::NextChapter]);
    assert!(!outcome.overlay_input_enabled);
    assert_eq!(host.runner.completed_commands(), 2);
    println!(
        "Higurashi host input:real message confirm consumes before interruptible WAIT, final message completion retains PC/PRNG/command ownership, next confirm zeros WAIT then VM reaches EXIT; chapter request remains external"
    );
}
