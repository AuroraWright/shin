use super::super::read_state::Checkpoint;
use super::*;

struct Services {
    checkpoints: Vec<Checkpoint>,
    fail: bool,
}
impl ReadServices for Services {
    fn checkpoint(&mut self, kind: Checkpoint, _: super::super::input::ReadState) -> Result<()> {
        ensure!(!self.fail, "Full checkpoint temporarily unavailable");
        self.checkpoints.push(kind);
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
fn info(code: &mut Vec<u8>, level: u8, text: &str) {
    code.extend([0xa0, level, (text.len() + 1) as u8]);
    code.extend_from_slice(text.as_bytes());
    code.push(0);
}
fn pump(
    runner: &mut HigurashiRunner,
    services: &mut Services,
    voice: &mut super::tests::Voice,
) -> Result<Status> {
    shin_tasks::block_on(runner.pump_native(
        super::tests::policy(),
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        services,
        32,
        10000,
        || [1, 2, 3, 4],
        voice,
    ))
}
fn cursor(runner: &HigurashiRunner) -> (CodeAddress, CodeAddress, u32, u64) {
    (
        runner.position(),
        runner.next_position(),
        runner.random_state(),
        runner.completed_commands(),
    )
}

#[test]
#[ignore = "real retained native SAVEINFO/AUTOSAVE constructor/continuation ownership on offscreen Metal"]
fn higurashi_runner_retains_autosave_metadata_and_retries_checkpoint_before_chapter_mutation() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut code = Vec::new();
    info(&mut code, 0, "Title");
    info(&mut code, 1, "Chapter A");
    code.push(0xa1);
    info(&mut code, 1, "Chapter B");
    code.extend([0x83, 1, 0x83, 0xe8, 0xa1, 0xa2, 0]);
    info(&mut code, 0, "Event");
    code.extend([0xa1, 0xa3]);
    info(&mut code, 1, "");
    code.extend([0xa1, 0, 0, 0]);
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut runner = shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 0, 42, || {
        [1, 2, 3, 4]
    }))
    .unwrap();
    let mut services = Services {
        checkpoints: Vec::new(),
        fail: true,
    };
    let mut voice = super::tests::Voice::default();
    assert!(pump(&mut runner, &mut services, &mut voice).is_err());
    assert!(matches!(runner.pending_command(), Some(Cmd::AUTOSAVE {})));
    assert_eq!(runner.completed_commands(), 2); // Both SAVEINFO commands complete internally.
    let metadata = runner.native_save_metadata().unwrap();
    assert_eq!(
        [&*metadata.current()[0], &*metadata.current()[1]],
        ["Title", "Chapter A"]
    );
    assert_eq!([&*metadata.saved()[0], &*metadata.saved()[1]], ["", ""]);
    assert_eq!(runner.native_message_timeline().unwrap().boundary, 0);
    let before = cursor(&runner);
    assert!(pump(&mut runner, &mut services, &mut voice).is_err());
    assert_eq!(cursor(&runner), before);
    services.fail = false;
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(cursor(&runner), before);
    let first = runner.native_save_metadata().unwrap().clone();
    assert_eq!(
        [&*first.saved()[0], &*first.saved()[1]],
        ["Title", "Chapter A"]
    );
    let timeline = runner.native_message_timeline().unwrap();
    assert_eq!(
        (
            timeline.boundary,
            timeline.previous_chapter,
            timeline.current_chapter,
            timeline.replay_target
        ),
        (1, 0, 1, 1)
    );
    services.fail = true;
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(services.checkpoints, vec![Checkpoint::Autosave]);
    assert_eq!(runner.native_message_timeline().unwrap(), timeline);
    runner.complete_external(CommandResult::None).unwrap(); // Checked fixture storage service acknowledges it.
    services.fail = false;
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::Waiting
    );
    let metadata = runner.native_save_metadata().unwrap();
    assert_eq!(&*metadata.current()[1], "Chapter B");
    assert_eq!(&*metadata.saved()[1], "Chapter A");
    assert!(runner.interrupt_wait());
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert_eq!(
        &*runner.native_save_metadata().unwrap().saved()[1],
        "Chapter B"
    );
    assert_eq!(runner.native_message_timeline().unwrap().current_chapter, 2);
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert!(matches!(
        runner.pending_command(),
        Some(Cmd::EVBEGIN { argument: 0 })
    ));
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert!(matches!(runner.pending_command(), Some(Cmd::AUTOSAVE {})));
    assert_eq!(&*runner.native_save_metadata().unwrap().saved()[0], "Event");
    assert_eq!(runner.native_read_state().unwrap().flags, 0);
    assert_eq!(runner.native_message_timeline().unwrap().current_chapter, 4);
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    assert!(matches!(runner.pending_command(), Some(Cmd::EVEND {})));
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::External
    );
    let metadata = runner.native_save_metadata().unwrap();
    assert_eq!(
        [&*metadata.saved()[0], &*metadata.saved()[1]],
        ["Event", ""]
    );
    let t = runner.native_message_timeline().unwrap();
    assert_eq!(
        (t.boundary, t.previous_chapter, t.current_chapter),
        (6, 4, 6)
    );
    assert_eq!(
        (
            t.previous_message,
            t.current_message,
            t.resume_position,
            t.normal_messages,
            t.quick_return_messages
        ),
        (0, 0, 0, 0, 0)
    );
    runner.complete_external(CommandResult::None).unwrap();
    assert_eq!(
        pump(&mut runner, &mut services, &mut voice).unwrap(),
        Status::Exited
    );
    assert_eq!(runner.completed_commands(), 13);
    assert_eq!(
        [&*first.saved()[0], &*first.saved()[1]],
        ["Title", "Chapter A"]
    );
    assert_eq!(
        services.checkpoints,
        vec![
            Checkpoint::Autosave,
            Checkpoint::Autosave,
            Checkpoint::EventBegin,
            Checkpoint::Autosave,
            Checkpoint::EventEnd,
            Checkpoint::Autosave
        ]
    );
    println!(
        "Higurashi chapter runner:13 actual commands, four retained AUTOSAVE services and six pre-constructor checkpoints pass; SAVEINFO completes internally, current/saved labels diverge until chapter capture, errors/repeated polls preserve PC/PRNG/count and snapshots stay immutable"
    );
}
