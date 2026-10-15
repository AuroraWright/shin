use super::super::effect_commands::EffectCommands;
use super::super::{
    checkpoints::AdvCheckpoint, input::ReadState, layers::LayerBanks,
    live_reconstruction::tests::Audio, read_state::Checkpoint,
};
use super::*;
use shin_core::format::scenario::instruction_elements::MessageId;
struct Services;
impl ReadServices for Services {
    fn checkpoint(&mut self, _: Checkpoint, _: ReadState) -> Result<()> {
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
fn pump(r: &mut HigurashiRunner, a: &mut Audio) -> Status {
    shin_tasks::block_on(r.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        &mut Services,
        32,
        10000,
        || [1; 4],
        a,
    ))
    .unwrap()
}

#[test]
#[ignore = "offscreen Metal final-memory resource/audio/message/VM publication; controlled audio has no OS device"]
fn higurashi_runner_publishes_replayed_memory_and_retires_old_wait_owner() {
    let (device, queue, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    let code = [0x83, 1, 0x80, 0x40, 0, 0, 0]; // WAIT64, interrupt1, EXIT0/0.
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.clone().into()).unwrap());
    let mut runner =
        shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || [1; 4])).unwrap();
    let mut audio = Audio::default();
    assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
    let before = (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
    );
    runner
        .frame
        .message
        .dispatch(
            &Cmd::MSGINIT {
                style: 1,
                updated_options: Some([3, 4]),
            },
            &device,
            &queue,
            false,
            0,
            50,
            0,
            &audio,
        )
        .unwrap();
    runner
        .frame
        .message
        .dispatch(
            &Cmd::MSGSET {
                msg_id: MessageId(123),
                auto_wait: 0,
                argument: -1,
                text: "@rOutgoing text".into(),
            },
            &device,
            &queue,
            false,
            0,
            50,
            0,
            &audio,
        )
        .unwrap();
    runner.frame.message.playback.current_time = 18.0;
    runner.frame.message.playback.flags = 3;
    runner.frame.message.playback.voice_disabled = true;
    let old_glyphs = runner.frame.message.glyphs().unwrap() as *const _;
    let mut read = CommandReadState::new();
    read.prepare(
        &Cmd::MSGINIT {
            style: 2,
            updated_options: Some([-1, 2]),
        },
        &mut Services,
    )
    .unwrap();
    read.prepare(
        &Cmd::BGMPLAY {
            args: [0, 12, 0, 500],
        },
        &mut Services,
    )
    .unwrap();
    read.prepare(
        &Cmd::SEPLAY {
            args: [2, 48, 12, 0, 700, -500, 1000],
        },
        &mut Services,
    )
    .unwrap();
    let mut layers = LayerBanks::default();
    layers.load(7, 2, 0, (424, 0, 0, 0, 0, 0, 0, 0)).unwrap();
    layers.control(7, 0, 321, 0).unwrap();
    let mut memory = AdvCheckpoint {
        vm: runner.vm.as_ref().unwrap().checkpoint_for_replay().unwrap(),
        read,
        layers: Box::new(layers),
        extra_words: vec![],
        response_cursor: 0,
    };
    // A different scenario cannot pair its bytecode VM with this resource table.
    raw[start + 3] = 0x41;
    let other = HigurashiScenario::new(raw.into()).unwrap();
    let mut wrong_vm = HigurashiScripter::new(&other, 0, 42).unwrap();
    wrong_vm.run(CommandResult::None, 100).unwrap();
    let old_vm = memory.vm.clone();
    memory.vm = wrong_vm.checkpoint_for_replay().unwrap();
    assert!(
        shin_tasks::block_on(runner.reconstruct_final_memory(&memory, || [9; 4], &mut audio))
            .is_err()
    );
    assert_eq!(audio.stops, 0);
    assert_eq!(
        (
            runner.position(),
            runner.next_position(),
            runner.random_state()
        ),
        before
    );
    memory.vm = old_vm;
    // Native19578 publishes visual owners before its audio/UI tail. A failed
    // desktop audio service retains that visual prefix and the old VM/WAIT;
    // command pumping stays faulted until publication is retried.
    audio.fail_slot = Some(5);
    assert!(
        shin_tasks::block_on(runner.reconstruct_final_memory(&memory, || [9; 4], &mut audio))
            .is_err()
    );
    assert_eq!(
        (
            runner.position(),
            runner.next_position(),
            runner.random_state()
        ),
        before
    );
    assert!(matches!(
        runner.pending.as_ref().unwrap().owner,
        Owner::Clock(_)
    ));
    let published = runner.frame.scene.layers().state.target(7).unwrap();
    let owner = runner.frame.scene.layers().layer(published).unwrap();
    assert_eq!(
        owner.layer_type(),
        shin_core::vm::command::types::LayerType::Picture
    );
    assert_eq!(owner.properties.values()[0], 321.0);
    assert!(runner.vm_fault.is_some());
    assert!(
        shin_tasks::block_on(runner.pump_native(
            super::tests::policy(),
            ReadPolicy {
                modes: 0,
                forced: false,
                read_only: true
            },
            &mut Services,
            32,
            10000,
            || [1; 4],
            &mut audio
        ))
        .is_err(),
        "partial publication must not resume the old command"
    );
    audio.fail_slot = None;
    shin_tasks::block_on(runner.reconstruct_final_memory(&memory, || [9; 4], &mut audio)).unwrap();
    assert_eq!(
        audio.stops, 4,
        "both ADV and message voice stop on each publication attempt"
    );
    assert!(runner.pending.is_none());
    assert!(runner.vm_fault.is_none());
    assert_eq!(runner.next_position(), memory.vm.position());
    assert_eq!(
        runner.frame.message.glyphs().unwrap() as *const _,
        old_glyphs
    );
    assert_eq!(runner.frame.message.playback.current_time, 18.0);
    assert_eq!(runner.frame.message.playback.flags, 0);
    assert!(!runner.frame.message.playback.voice_disabled);
    assert!(runner.frame.message.playback.input_blocked);
    let target = runner.frame.scene.layers().state.target(7).unwrap();
    let owner = runner.frame.scene.layers().layer(target).unwrap();
    assert_eq!(
        owner.layer_type(),
        shin_core::vm::command::types::LayerType::Picture
    );
    assert_eq!(owner.properties.values()[0], 321.0);
    assert_eq!(audio.effect_descriptor_id(2), 48);
    assert_eq!(pump(&mut runner, &mut audio), Status::Waiting);
    // The resumed WAIT is freshly constructed, not the old continuation.
    runner.advance(
        Ticks::from_f32(64.0),
        super::tests::policy(),
        &mut audio,
        None,
    );
    assert_eq!(pump(&mut runner, &mut audio), Status::Exited);
    runner
        .frame
        .message
        .dispatch(
            &Cmd::MSGSET {
                msg_id: MessageId(124),
                auto_wait: 0,
                argument: -1,
                text: "@rRestored text".into(),
            },
            &device,
            &queue,
            false,
            0,
            50,
            0,
            &audio,
        )
        .unwrap();
    assert_eq!(runner.frame.message.playback.style, 2);
    assert_eq!(runner.frame.message.playback.options, [-1, 2]);
    println!(
        "Higurashi final-memory runner:real PIC424 publication, retained glyphs/message clocks and restored init metadata, BGM/32 SE descriptors, scenario identity rejection, failure-prefix retry and fresh WAIT-to-EXIT VM handoff pass; higher replay/modes remain pending"
    );
}
