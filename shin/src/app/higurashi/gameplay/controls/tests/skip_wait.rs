//! Enclosing host regression: native skip and manual A have distinct WAIT rules.
use super::*;
use crate::adv::higurashi::{audio::HigurashiAudio, reference_gpu_assets};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};

#[derive(Clone, Copy, Debug)]
struct Case {
    name: &'static str,
    modes: u32,
    forced: bool,
    read_only: bool,
    read: bool,
    protected: bool,
    skip: bool,
}

#[test]
#[ignore = "actual Host/Controls post-poll WAIT0 versus manual A, read/auto/protected gates; offscreen Metal and exact Loopback Audio"]
fn higurashi_origin_autoskip_host_waits_preserve_native_gates_and_next_poll_order() {
    let (_, _, assets) = reference_gpu_assets();
    let assets = Arc::new(assets);
    let authored = assets.load_sync::<HigurashiScenario>("/main.snr").unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let cases = [
        Case {
            name: "skip",
            modes: 2,
            forced: false,
            read_only: false,
            read: false,
            protected: false,
            skip: true,
        },
        Case {
            name: "read-only-read",
            modes: 2,
            forced: false,
            read_only: true,
            read: true,
            protected: false,
            skip: true,
        },
        Case {
            name: "forced-unread",
            modes: 0,
            forced: true,
            read_only: true,
            read: false,
            protected: false,
            skip: true,
        },
        Case {
            name: "skip-before-auto",
            modes: 3,
            forced: false,
            read_only: false,
            read: false,
            protected: false,
            skip: true,
        },
        Case {
            name: "idle",
            modes: 0,
            forced: false,
            read_only: false,
            read: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "auto",
            modes: 1,
            forced: false,
            read_only: false,
            read: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "read-only-unread",
            modes: 2,
            forced: false,
            read_only: true,
            read: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "unread-auto-fallback",
            modes: 3,
            forced: false,
            read_only: true,
            read: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "protected-skip",
            modes: 2,
            forced: false,
            read_only: false,
            read: false,
            protected: true,
            skip: false,
        },
        Case {
            name: "protected-forced",
            modes: 0,
            forced: true,
            read_only: false,
            read: false,
            protected: true,
            skip: false,
        },
    ];
    let mut checked = 0;
    for case in cases {
        for duration in [42u8, 48] {
            for allow in [0, 1, 2, 255] {
                // A synthetic boundary isolates the enclosing bridge. The separate
                // Runtime fixture uses the unmodified quote and real CALL helper.
                // MSGSET auto_wait0 establishes the cached read bit through actual
                // ReadServices; EVBEGIN establishes the protected-event flag.
                let mut code = vec![0x86, 1, 0, 0, 0, 0, 2, 0, b'A', 0];
                if case.protected {
                    code.extend([0xa2, 0]);
                }
                code.extend([0x83, allow, duration, 0x83, 0, 60, 0, 0, 0]);
                let mut raw = authored.raw().to_vec();
                let start = authored.header.code_offset as usize;
                raw[start..start + code.len()].copy_from_slice(&code);
                let script = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
                let runner = shin_tasks::block_on(HigurashiRunner::load(
                    assets.clone(),
                    script.clone(),
                    0,
                    42,
                    || [1, 2, 3, 4],
                ))
                .unwrap();
                let mut host = HigurashiHost::new(
                    runner,
                    HigurashiAudio::new(manager.clone(), assets.clone(), script),
                );
                let entry = ProgressEntry {
                    scenario_id: 0,
                    random_seed: 42,
                    save_position: 0,
                    replay_target: 0,
                    response_data: Vec::new(),
                };
                let mut reads = Reads {
                    memory: CheckpointMemory::new(Vec::new()),
                    progress: ProgressMemory::new(
                        PersistData::new(),
                        SaveProgress::new(0, 0, [entry.clone(), entry]).unwrap(),
                        Vec::new(),
                    ),
                    history: MessageReadHistory::new(),
                };
                if case.read {
                    reads.history.record_completion(true, 0);
                }
                let policy = super::super::super::tests::policy();
                let ap = super::super::super::tests::audio();
                let idle = ReadPolicy {
                    modes: 0,
                    forced: false,
                    read_only: false,
                };
                let selected = ReadPolicy {
                    modes: case.modes,
                    forced: case.forced,
                    read_only: case.read_only,
                };
                let update = |host: &mut HigurashiHost, reads: &mut Reads, rp: ReadPolicy| {
                    shin_tasks::block_on(host.update_native_with_frame_services(
                        Ticks::from_f32(1.0),
                        policy,
                        rp,
                        reads,
                        ap,
                        32,
                        10000,
                        || [1, 2, 3, 4],
                        |_, _, _| Ok(false),
                        |runner, audio, ui, _| post_poll(runner, audio, ui, rp),
                    ))
                    .unwrap()
                };
                assert_eq!(update(&mut host, &mut reads, idle), Status::Waiting);
                assert!(matches!(host.runner.pending_command(),
                    Some(Cmd::WAIT { amount, allow_interrupt })
                        if *amount == i32::from(duration) && *allow_interrupt == allow));
                assert_eq!(
                    host.runner.reference_wait(),
                    Some((f32::from(duration) - 1.0, allow))
                );
                let state = host.runner.native_read_state().unwrap();
                assert_eq!(state.is_read != 0, case.read);
                assert_eq!(state.flags & 0x8000_0000 == 0, case.protected);
                let cursor = (
                    host.runner.position(),
                    host.runner.next_position(),
                    host.runner.completed_commands(),
                    host.runner.random_state(),
                );
                assert_eq!(update(&mut host, &mut reads, selected), Status::Waiting);
                assert_eq!(
                    cursor,
                    (
                        host.runner.position(),
                        host.runner.next_position(),
                        host.runner.completed_commands(),
                        host.runner.random_state()
                    ),
                    "post-poll must not retire {case:?}/{duration}/{allow}"
                );
                let expected = if case.skip {
                    0.0
                } else {
                    f32::from(duration) - 2.0
                };
                assert_eq!(
                    host.runner.reference_wait(),
                    Some((expected, allow)),
                    "native skip clears only the clock, regardless of manual flag: {case:?}/{duration}/{allow}"
                );
                if case.skip {
                    assert_eq!(update(&mut host, &mut reads, idle), Status::Waiting);
                    assert_eq!(host.runner.completed_commands(), cursor.2 + 1);
                    assert!(matches!(
                        host.runner.pending_command(),
                        Some(Cmd::WAIT {
                            amount: 60,
                            allow_interrupt: 0
                        })
                    ));
                    assert_eq!(host.runner.reference_wait(), Some((59.0, 0)));
                } else {
                    // Manual A/touch must keep the original allow==1 rule.
                    assert_eq!(host.runner.interrupt_wait(), allow == 1);
                    assert_eq!(
                        host.runner.reference_wait(),
                        Some((if allow == 1 { 0.0 } else { expected }, allow))
                    );
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 80);
    eprintln!(
        "Origin autoskip Host/Controls:80 owned WAIT cases (42/48ticks,manual flags0/1/2/255),skip/forced/read/auto/protected guards,allow preserved,post-poll retention/next-poll retirement and unchanged manual interrupt rule pass"
    );
}
