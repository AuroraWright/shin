use super::super::{
    autosave::{AutosaveMemory, Execution},
    character_selection::command::tests::Services,
    checkpoints::{CheckpointMemory, CheckpointServices},
    choice::ChoiceHistory,
    live_reconstruction::tests::Audio,
    read_history::MessageReadHistory,
    replay::{
        Status as ReplayStatus,
        tests::{scenario, session},
    },
};
use super::*;
use shin_core::format::{
    save::index_set::IndexSet,
    scenario::instruction_elements::{MessageId, Register},
};

fn pump(
    r: &mut HigurashiRunner,
    memory: &mut CheckpointMemory,
    progress: &mut super::super::progress::ProgressMemory,
    history: &mut MessageReadHistory,
) -> Status {
    shin_tasks::block_on(r.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut CheckpointServices {
            memory,
            progress,
            history,
        },
        32,
        10000,
        || [1, 2, 3, 4],
        &mut super::tests::Voice::default(),
    ))
    .unwrap()
}
const CODE: &[u8] = &[0xd1, 18, 0, 31, 0, 7, 0x83, 1, 0x80, 0x40, 0, 0, 0];
#[test]
#[ignore = "real offscreen Metal runner/character constructor/checkpoint/current response stream and retained replay publication;controlled audio,no window/saves"]
fn higurashi_runner_characters_capture_cleared_text_and_preserve_replay_live_ownership() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let snr = scenario(CODE);
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        snr.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut source = session(snr, 400, 400, 42);
    let mut read = CommandReadState::new();
    read.prepare(
        &Cmd::MSGSET {
            msg_id: MessageId(1),
            auto_wait: 1,
            argument: 0,
            text: "old text".into(),
        },
        &mut Reads,
    )
    .unwrap();
    read.timeline_mut().boundary = 199;
    runner.read_state = Some(read);
    let layers = &mut runner.frame.scene.layers_mut().unwrap().state;
    for id in [0, 2, 4] {
        layers.allocate(id).unwrap();
    }
    layers.start_back(1).unwrap();
    let before = super::super::layers::replay_snapshot(layers);
    let mut history = MessageReadHistory::new();
    assert_eq!(
        pump(
            &mut runner,
            &mut source.memory,
            &mut source.progress,
            &mut history
        ),
        Status::External
    );
    let owner = runner.character_command().unwrap();
    let target = owner.target().unwrap();
    assert_eq!((target.logical, target.slot, target.bank), (1, 3, 0));
    assert_eq!(owner.request().index, 17);
    assert_eq!(owner.request().mask, 7);
    assert_eq!(source.memory.entries().len(), 1);
    let checkpoint = &source.memory.entries()[0];
    assert_eq!(checkpoint.boundary, 199);
    assert!(checkpoint.state.read.descriptors().message.text.is_none());
    assert_eq!(checkpoint.state.read.timeline().boundary, 199);
    assert_eq!(
        super::super::layers::replay_snapshot(&checkpoint.state.layers),
        before
    );
    assert!(checkpoint.state.layers.target(1).is_none());
    assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 200);
    let retained = (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
        runner.completed_commands(),
    );
    let mut memory = AutosaveMemory {
        valid: false,
        packed_time: 0,
        progress: source.progress.progress().clone(),
    };
    let mut seen = IndexSet::new();
    seen.add(7);
    let mut choices = ChoiceHistory::new(seen, vec![]);
    let mut services = Services::default();
    for _ in 0..2 {
        assert_eq!(
            runner
                .poll_character_command(
                    &mut source.progress,
                    &mut memory,
                    &mut choices,
                    Execution::Live,
                    &mut services
                )
                .unwrap(),
            Some(false)
        );
        assert!(!memory.valid);
        assert_eq!(services.clocks, 0);
        assert_eq!(
            (
                runner.position(),
                runner.next_position(),
                runner.random_state(),
                runner.completed_commands()
            ),
            retained
        );
    }
    services.ready = true;
    assert_eq!(
        runner
            .poll_character_command(
                &mut source.progress,
                &mut memory,
                &mut choices,
                Execution::Live,
                &mut services
            )
            .unwrap(),
        Some(false)
    );
    assert!(memory.valid);
    assert_eq!(memory.progress.active_entry().save_position, 200);
    services.selected = Some(3);
    assert_eq!(
        runner
            .poll_character_command(
                &mut source.progress,
                &mut memory,
                &mut choices,
                Execution::Live,
                &mut services
            )
            .unwrap(),
        Some(true)
    );
    assert_eq!(memory.progress.active_entry().save_position, 201);
    assert_eq!(source.progress.responses().1, 4);
    assert_eq!(choices.last()[17], 4);
    assert_eq!(choices.seen().iter().collect::<Vec<_>>(), [7]);
    assert_eq!(
        runner
            .vm
            .as_ref()
            .unwrap()
            .read_register(Register::from_regular_register(31)),
        3
    );
    assert!(runner.frame.scene.layers().state.target(1).is_none());
    assert_eq!(
        runner.frame.scene.layers_mut().unwrap().state.allocate(1),
        Some(3)
    );
    assert_eq!(services.requests, 1);
    assert_eq!(services.clocks, 2);
    assert_eq!(
        pump(
            &mut runner,
            &mut source.memory,
            &mut source.progress,
            &mut history
        ),
        Status::Waiting
    );
    for saved in [1, 2] {
        let snr = scenario(CODE);
        let mut replay = session(snr.clone(), saved, saved, 42);
        assert_eq!(
            replay
                .pump(&MessageReadHistory::new(), 32, 10000, || false)
                .unwrap(),
            ReplayStatus::Ready
        );
        assert!(matches!(
            replay.pending_command(),
            Some(Cmd::CHARSELECT { .. })
        ));
        let stop = if saved == 1 { 0 } else { 1 };
        assert_eq!(
            replay
                .pending
                .as_ref()
                .unwrap()
                .character
                .as_ref()
                .unwrap()
                .replay_stop(),
            stop
        );
        let cursor = replay.progress.responses().1;
        assert_eq!(cursor, if saved == 1 { 0 } else { 4 });
        let reservation = replay
            .pending
            .as_ref()
            .unwrap()
            .character
            .as_ref()
            .unwrap()
            .target()
            .unwrap();
        let next = replay.vm.as_ref().unwrap().next_position();
        let prng = replay.vm.as_ref().unwrap().random_state();
        let mut runner =
            shin_tasks::block_on(HigurashiRunner::load(assets.clone(), snr, 0, 42, || [1; 4]))
                .unwrap();
        shin_tasks::block_on(runner.reconstruct_replay(
            &mut replay,
            || [2; 4],
            &mut Audio::default(),
        ))
        .unwrap();
        assert_eq!(
            runner.character_command().unwrap().target(),
            Some(reservation)
        );
        assert_eq!(runner.character_command().unwrap().replay_stop(), stop);
        let mut history = MessageReadHistory::new();
        assert_eq!(
            pump(
                &mut runner,
                &mut replay.memory,
                &mut replay.progress,
                &mut history
            ),
            Status::External
        );
        assert_eq!(runner.next_position(), next);
        assert_eq!(runner.random_state(), prng);
        assert_eq!(replay.progress.responses().1, cursor);
        assert_eq!(
            runner.read_state.as_ref().unwrap().timeline().boundary,
            saved
        );
        let mut memory = AutosaveMemory {
            valid: false,
            packed_time: 0,
            progress: replay.progress.progress().clone(),
        };
        let mut choices = ChoiceHistory::new(IndexSet::new(), vec![]);
        let mut services = Services::default();
        services.ready = saved == 1;
        services.selected = Some(3);
        assert_eq!(
            runner
                .poll_character_command(
                    &mut replay.progress,
                    &mut memory,
                    &mut choices,
                    Execution::Live,
                    &mut services
                )
                .unwrap(),
            Some(true)
        );
        assert_eq!(services.requests, u32::from(saved == 1));
        assert_eq!(services.clocks, if saved == 1 { 2 } else { 0 });
        assert_eq!(replay.progress.responses().1, 4);
        assert_eq!(runner.read_state.as_ref().unwrap().timeline().boundary, 2);
        assert_eq!(
            runner
                .vm
                .as_ref()
                .unwrap()
                .read_register(Register::from_regular_register(31)),
            if saved == 1 { 3 } else { 2 }
        );
        assert!(
            runner
                .frame
                .scene
                .layers()
                .state
                .target(reservation.logical)
                .is_none()
        );
        assert_eq!(
            choices.last().get(17).copied(),
            if saved == 1 { Some(4) } else { None }
        );
        assert_eq!(
            pump(
                &mut runner,
                &mut replay.memory,
                &mut replay.progress,
                &mut history
            ),
            Status::Waiting
        );
    }
    eprintln!(
        "Character runner:live cleared-text checkpoint before reservation,one retained load/two11780 copies/four-bit original result and slot reuse;zero/positive replay stops retain mappings/VM feedback and hand off once without constructor/response/UI duplication pass"
    );
}
struct Reads;
impl ReadServices for Reads {
    fn checkpoint(
        &mut self,
        _: super::super::read_state::Checkpoint,
        _: super::super::input::ReadState,
    ) -> Result<()> {
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
