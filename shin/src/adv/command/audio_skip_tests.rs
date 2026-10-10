use super::*;
use shin_core::{
    format::scenario::instruction_elements::NumberSpec,
    time::Ticks,
    vm::{
        IntoRuntimeForm, VmCtx,
        command::{CompiletimeCommand as C, compiletime as c},
    },
};

fn number<T>(value: i32) -> NumberSpec<T> {
    NumberSpec::new(NumberSpec::constant(value).into_untyped())
}

fn audio_commands(no_repeat: i32) -> Vec<C> {
    vec![
        C::BGMPLAY(c::BGMPLAY {
            bgm_data_id: number(0),
            fade_in_time: number(120),
            no_repeat: number(no_repeat),
            volume: number(400),
        }),
        C::BGMSTOP(c::BGMSTOP {
            fade_out_time: number(120),
        }),
        C::BGMVOL(c::BGMVOL {
            volume: number(400),
            fade_in_time: number(120),
        }),
        C::SEPLAY(c::SEPLAY {
            se_slot: number(3),
            se_data_id: number(0),
            fade_in_time: number(120),
            no_repeat: number(no_repeat),
            volume: number(400),
            pan: number(-250),
            play_speed: number(1500),
        }),
        C::SESTOP(c::SESTOP {
            se_slot: number(3),
            fade_out_time: number(120),
        }),
        C::SESTOPALL(c::SESTOPALL {
            fade_out_time: number(120),
        }),
        C::SEVOL(c::SEVOL {
            se_slot: number(3),
            volume: number(400),
            fade_in_time: number(120),
        }),
        C::SEPAN(c::SEPAN {
            se_slot: number(3),
            pan: number(-250),
            fade_in_time: number(120),
        }),
    ]
}

fn fade(command: &RuntimeCommand) -> Ticks {
    match command {
        RuntimeCommand::BGMPLAY(v) => v.fade_in_time,
        RuntimeCommand::BGMSTOP(v) => v.fade_out_time,
        RuntimeCommand::BGMVOL(v) => v.fade_in_time,
        RuntimeCommand::SEPLAY(v) => v.fade_in_time,
        RuntimeCommand::SESTOP(v) => v.fade_out_time,
        RuntimeCommand::SESTOPALL(v) => v.fade_out_time,
        RuntimeCommand::SEVOL(v) => v.fade_in_time,
        RuntimeCommand::SEPAN(v) => v.fade_in_time,
        _ => unreachable!(),
    }
}

#[test]
fn native_skip_clears_slot_audio_fades_preserves_saved_destinations_and_suppresses_nonrepeat_se() {
    for no_repeat in [0, 1] {
        for command in audio_commands(no_repeat) {
            let mut runtime = command.into_runtime_form(&VmCtx::new(0, 42));
            assert_eq!(fade(&runtime), Ticks::from_f32(120.0));
            let suppress = matches!(&runtime, RuntimeCommand::SEPLAY(v) if v.no_repeat);
            let mut before = VmState::for_scenario(0, 42);
            apply_command_state_ref(&runtime, &mut before).unwrap();
            assert_eq!(apply_audio_skip_policy(&mut runtime), suppress);
            assert_eq!(fade(&runtime), Ticks::ZERO);
            let mut after = VmState::for_scenario(0, 42);
            apply_command_state_ref(&runtime, &mut after).unwrap();
            assert_eq!(format!("{:?}", before.audio), format!("{:?}", after.audio));
            if let RuntimeCommand::SEPLAY(v) = &runtime {
                assert_eq!(v.se_slot, 3);
                assert_eq!(v.volume.0, 0.4);
                assert_eq!(v.pan.0, -0.25);
                assert_eq!(v.play_speed, 1500);
            }
        }
    }
    let mut wait = C::WAIT(c::WAIT {
        allow_interrupt: shin_core::format::scenario::instruction_elements::U8Bool(false),
        wait_amount: number(120),
    })
    .into_runtime_form(&VmCtx::new(0, 42));
    assert!(!apply_audio_skip_policy(&mut wait));
    let RuntimeCommand::WAIT(wait) = wait else {
        unreachable!()
    };
    assert_eq!(wait.wait_amount, 120);
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_audio_skip_fade_and_transient_branches() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x6c08, 0x94002e0a),
        (0x6c10, 0xb9001e7f),
        (0x6cc0, 0xb9001a7f),
        (0x6d40, 0xb9001e7f),
        (0x6ed8, 0x94002d56),
        (0x6ee4, 0xb900227f),
        (0x6ee8, 0x34000188),
        (0x6ef8, 0x2a1f03e2),
        (0x6efc, 0x9403ad41),
        (0x6f00, 0xa9417bfd),
        (0x6fc4, 0xb9001e7f),
        (0x7044, 0xb9001a7f),
        (0x70c4, 0xb900227f),
        (0x7164, 0xb900227f),
        (0x7338, 0x94002c3e),
        (0x733c, 0x360000a0),
        (0x7340, 0x320003e0),
        (0x7444, 0x94002bfb),
        (0x7448, 0x37000e60),
        (0x7618, 0x320003e0),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native audio skip instruction {address:x}"
        );
    }
}

pub(crate) fn reference_live_audio_skip_dispatch(
    context: &mut crate::update::UpdateContext,
    scenario: &std::sync::Arc<shin_core::format::scenario::Scenario>,
    adv: &mut crate::adv::AdvState,
) {
    use shin_core::vm::command::types::AudioWaitStatus;
    let mut state = VmState::for_scenario(0, 42);
    // Invalid transient asset IDs make accidental lookup observable: the
    // native skipped paths finish before touching the scenario asset table.
    let commands = [
        C::SEPLAY(c::SEPLAY {
            se_slot: number(3),
            se_data_id: number(30000),
            fade_in_time: number(120),
            no_repeat: number(1),
            volume: number(400),
            pan: number(-250),
            play_speed: number(1500),
        }),
        C::SEONCE(c::SEONCE {
            arg1: number(30000),
            arg2: number(2),
            arg3: number(400),
            arg4: number(-250),
            arg5: number(1500),
        }),
        C::VOICEPLAY(c::VOICEPLAY {
            name: shin_core::format::scenario::types::U16String::new("missing-skipped-voice"),
            volume: number(400),
            flags: number(0),
        }),
    ];
    for command in commands {
        let start = apply_command_state_and_start_with_skip(
            command.into_runtime_form(&VmCtx::new(0, 42)),
            context,
            scenario,
            &mut state,
            adv,
            true,
        );
        assert!(
            matches!(start, crate::adv::CommandStartResult::Continue(_)),
            "Skipped transient must finish without load/wait"
        );
    }
    assert!(state.audio.se[3].is_none());
    assert!(adv.audio.se_player.get_wait_status(3).is_empty());
    assert!(adv.audio.se_player.reference_source(3).0.is_none());
    // Repeating SE retains its real load path. Its fade is cleared but its
    // source, pan, speed and retained VM description are installed normally.
    let repeat = audio_commands(0)
        .into_iter()
        .find(|c| matches!(c, C::SEPLAY(_)))
        .unwrap();
    let start = apply_command_state_and_start_with_skip(
        repeat.into_runtime_form(&VmCtx::new(0, 42)),
        context,
        scenario,
        &mut state,
        adv,
        true,
    );
    assert!(matches!(start, crate::adv::CommandStartResult::Continue(_)));
    assert!(state.audio.se[3].is_some());
    assert!(
        adv.audio
            .se_player
            .get_wait_status(3)
            .contains(AudioWaitStatus::PLAYING)
    );
    adv.audio
        .se_player
        .stop(3, shin_core::time::Tween::IMMEDIATE);
    let once = C::SEONCE(c::SEONCE {
        arg1: number(0),
        arg2: number(2),
        arg3: number(400),
        arg4: number(-250),
        arg5: number(1500),
    });
    let start = apply_command_state_and_start_with_skip(
        once.into_runtime_form(&VmCtx::new(0, 42)),
        context,
        scenario,
        &mut state,
        adv,
        false,
    );
    let crate::adv::CommandStartResult::Yield(mut pending) = start else {
        panic!("Normal SEONCE must wait on its real pending source")
    };
    assert!(
        adv.audio
            .se_player
            .get_once_wait_status(-1)
            .contains(AudioWaitStatus::PLAYING)
    );
    assert!(
        pending
            .update(context, scenario, &mut state, adv, true)
            .is_some()
    );
    assert!(
        adv.audio
            .se_player
            .get_once_wait_status(-1)
            .contains(AudioWaitStatus::PLAYING),
        "Skip completes the command without stopping its owned source"
    );
    adv.audio
        .se_player
        .stop_all(shin_core::time::Tween::IMMEDIATE);
    eprintln!(
        "Native audio skip dispatch: suppressed nonrepeat SE/SEONCE/VOICEPLAY bypass asset lookup; repeating SE still loads and retains state; late SEONCE skip retains playback"
    );
}
