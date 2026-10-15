use super::super::{
    audio::{AudioFramePolicy, HigurashiAudio},
    host::HigurashiHost,
    input::{Buttons, Context, InputState, ReadState},
    message_playback::WaitKind,
    read_history::{MessageReadHistory, ReadHistoryServices},
    read_state::Checkpoint,
};
use super::*;
use std::{cell::RefCell, rc::Rc};

struct Services<C> {
    read: ReadHistoryServices<C>,
    fail_id: Option<u32>,
    writes: Vec<u32>,
}
impl<C: FnMut(Checkpoint, ReadState) -> Result<()>> ReadServices for Services<C> {
    fn checkpoint(&mut self, kind: Checkpoint, before: ReadState) -> Result<()> {
        self.read.checkpoint(kind, before)
    }
    fn message_is_read(&self, id: u32) -> bool {
        self.read.message_is_read(id)
    }
    fn message_completed(&mut self, id: u32) -> Result<()> {
        ensure!(
            self.fail_id != Some(id),
            "Read history temporarily unavailable"
        );
        self.read.message_completed(id)?;
        self.writes.push(id);
        Ok(())
    }
}
fn message(code: &mut Vec<u8>, id: u32) {
    code.push(0x86);
    code.extend_from_slice(&id.to_le_bytes());
    code.extend([0, 2, 0, b'A', 0, 0x83, 1, 0x83, 0xe8]); // MSGSET; WAIT1000, interruptible.
}
fn update(
    host: &mut HigurashiHost,
    services: &mut impl ReadServices,
    delta: f32,
) -> Result<Status> {
    shin_tasks::block_on(host.update_native(
        Ticks::from_f32(delta),
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        services,
        AudioFramePolicy {
            focus_mode_active: true,
            voice_focus_enabled: true,
            effects_update_enabled: true,
            music_rate: 1.0,
        },
        32,
        10000,
        || [1, 2, 3, 4],
    ))
}
fn context() -> Context {
    Context {
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
        wait_allow_interrupt: 0,
    }
}
fn cursor(host: &HigurashiHost) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        host.runner.position(),
        host.runner.next_position(),
        host.runner.random_state(),
        host.runner.completed_commands(),
    )
}

#[test]
#[ignore = "real message completion/history/revisit host on offscreen Metal and exact Loopback Audio"]
fn higurashi_host_publishes_completed_message_history_before_revisit_and_retries_failures() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    for id in 1..=4 {
        message(&mut code, id);
        message(&mut code, id);
    }
    code.extend([0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut audio = HigurashiAudio::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        assets,
        scenario,
    );
    audio.set_paused(true);
    let mut host = HigurashiHost::new(runner, audio);
    let snapshots = Rc::new(RefCell::new(Vec::new()));
    let recorded = snapshots.clone();
    let mut services = Services {
        read: ReadHistoryServices::new(MessageReadHistory::new(), move |kind, before| {
            assert_eq!(kind, Checkpoint::Message);
            assert_eq!(before.flags, u32::MAX);
            recorded.borrow_mut().push(before);
            Ok(())
        }),
        fail_id: None,
        writes: Vec::new(),
    };
    let mut input = InputState {
        modes: 0,
        temporary_indicator: 0,
    };
    let buttons = Buttons {
        pressed: 1 << 6,
        ..Default::default()
    };
    assert!(
        host.process_input_native(&mut input, context(), buttons, &mut services)
            .is_err()
    );
    for id in 0..4 {
        assert_eq!(
            update(&mut host, &mut services, 0.0).unwrap(),
            Status::Waiting
        );
        assert_eq!(host.runner.frame.message.playback.message_id, id);
        assert!(!services.read.history.contains(id));
        assert_eq!(host.runner.native_read_state().unwrap().is_read, 0);
        let timeline = host.runner.native_message_timeline().unwrap();
        assert_eq!(timeline.boundary, id * 2 + 1);
        assert_eq!(timeline.normal_messages, id * 2 + 1);
        assert_eq!(timeline.previous_message, id * 2);
        assert_eq!(host.runner.frame.message.playback.flags, 0);
        if id == 0 {
            for _ in 0..250 {
                if host.runner.frame.message.playback.wait == Some(WaitKind::Last) {
                    break;
                }
                assert_eq!(
                    update(&mut host, &mut services, 3.0).unwrap(),
                    Status::Waiting
                );
            }
            assert_eq!(
                host.runner.frame.message.playback.wait,
                Some(WaitKind::Last)
            );
            let before = cursor(&host);
            let outcome = host
                .process_input_native(&mut input, context(), buttons, &mut services)
                .unwrap();
            assert!(outcome.requests.is_empty());
            assert_eq!(cursor(&host), before);
        } else if id <= 2 {
            for _ in 0..250 {
                if services.read.history.contains(id) {
                    break;
                }
                host.runner.request_message_update(id == 1, id == 2);
                assert_eq!(
                    update(&mut host, &mut services, 3.0).unwrap(),
                    Status::Waiting
                );
            }
        } else {
            host.runner.fast_forward_visuals(&mut host.audio);
            services.fail_id = Some(id);
            assert!(
                host.runner
                    .flush_message_completions(&mut services)
                    .is_err()
            );
        }
        assert_eq!(host.runner.native_read_state().unwrap().is_read, 0);
        assert!(
            !host
                .runner
                .read_state
                .as_ref()
                .unwrap()
                .suppression(ReadPolicy {
                    modes: 2,
                    forced: false,
                    read_only: true
                })
        );
        if id != 3 {
            assert!(services.read.history.contains(id));
            assert!(
                host.runner
                    .frame
                    .message
                    .playback
                    .completed_messages
                    .is_empty()
            );
        }
        assert!(host.runner.interrupt_wait());
        if id == 3 {
            let before = cursor(&host);
            assert!(update(&mut host, &mut services, 0.0).is_err());
            assert_eq!(cursor(&host), before);
            assert_eq!(
                host.runner.frame.message.playback.completed_messages,
                vec![id]
            );
            assert!(!services.read.history.contains(id));
            services.fail_id = None;
        }
        // Pump flushes before MSGSET: first completion is visible to the revisit.
        assert_eq!(
            update(&mut host, &mut services, 0.0).unwrap(),
            Status::Waiting
        );
        assert!(services.read.history.contains(id));
        assert_eq!(host.runner.frame.message.playback.message_id, id);
        assert_eq!(host.runner.native_read_state().unwrap().is_read, 1);
        let timeline = host.runner.native_message_timeline().unwrap();
        assert_eq!(timeline.boundary, id * 2 + 2);
        assert_eq!(timeline.current_message, id * 2 + 2);
        assert_eq!(timeline.previous_message, id * 2 + 1);
        assert_eq!(host.runner.frame.message.playback.flags, 1);
        assert!(
            host.runner
                .read_state
                .as_ref()
                .unwrap()
                .suppression(ReadPolicy {
                    modes: 2,
                    forced: false,
                    read_only: true
                })
        );
        host.runner.fast_forward_visuals(&mut host.audio);
        host.runner
            .flush_message_completions(&mut services)
            .unwrap();
        assert!(
            host.runner
                .frame
                .message
                .playback
                .completed_messages
                .is_empty()
        );
        assert!(host.runner.interrupt_wait());
    }
    assert_eq!(
        update(&mut host, &mut services, 0.0).unwrap(),
        Status::Exited
    );
    assert_eq!(host.runner.completed_commands(), 17);
    assert_eq!(services.writes, vec![0, 0, 1, 1, 2, 2, 3, 3]);
    assert_eq!(services.read.history.bits().size_in_words(), 256);
    assert_eq!(snapshots.borrow().len(), 8);
    // A partial service failure acknowledges only its successful prefix. The
    // negative ID is discarded without calling storage; duplicates remain safe.
    host.runner
        .frame
        .message
        .playback
        .completed_messages
        .extend([4, 5, 6, u32::MAX]);
    services.fail_id = Some(5);
    let before = cursor(&host);
    assert!(
        host.runner
            .flush_message_completions(&mut services)
            .is_err()
    );
    assert_eq!(
        host.runner.frame.message.playback.completed_messages,
        vec![5, 6, u32::MAX]
    );
    assert_eq!(services.writes.last(), Some(&4));
    services.fail_id = None;
    let mut paused = super::tests::policy();
    paused.paused = true;
    assert_eq!(
        shin_tasks::block_on(host.runner.pump_native(
            paused,
            ReadPolicy {
                modes: 0,
                forced: false,
                read_only: true
            },
            &mut services,
            32,
            10000,
            || [1, 2, 3, 4],
            &mut host.audio
        ))
        .unwrap(),
        Status::Exited
    );
    assert_eq!(cursor(&host), before);
    assert!(
        host.runner
            .frame
            .message
            .playback
            .completed_messages
            .is_empty()
    );
    assert_eq!(&services.writes[8..], &[4, 5, 6]);
    assert!(!services.read.history.contains(u32::MAX));
    println!(
        "Higurashi read-history host:17 real commands, manual/autoplay/skip/fast-forward completions, immediate input/same-frame writes, all4 revisits read, cached current read unchanged, failed write prevents VM advance and retries acknowledged suffix; exact Loopback/offscreen Metal pass"
    );
}
