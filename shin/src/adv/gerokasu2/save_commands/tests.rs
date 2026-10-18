use super::*;
use shin_core::format::{save::gerokasu2::ProgressEntry, scenario::gerokasu2::Gerokasu2Scenario};

fn vm() -> Gerokasu2Scripter {
    let mut data = b"SNR ".to_vec();
    for word in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        data.extend(word.to_le_bytes());
    }
    let mut tables = Vec::new();
    for i in 0..10 {
        data.extend((0x4c + tables.len() as u32).to_le_bytes());
        if i < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    data.extend(tables);
    let start = data.len() as u32;
    data[32..36].copy_from_slice(&start.to_le_bytes());
    data.extend([0, 0, 0]);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut vm = Gerokasu2Scripter::new(
        Arc::new(Gerokasu2Scenario::new(data.into()).unwrap()),
        0,
        42,
    );
    vm.run(shin_core::vm::command::CommandResult::None, 32)
        .unwrap();
    vm
}
struct Services {
    save: SaveSession,
    time: u64,
    clocks: u32,
    titles: Vec<String>,
    published: u32,
    fail: u8,
}
impl SaveServices for Services {
    fn save_session(&mut self) -> &mut SaveSession {
        &mut self.save
    }
    fn packed_time(&mut self) -> Result<u64> {
        self.clocks += 1;
        ensure!(self.fail != 1, "clock failure");
        Ok(self.time)
    }
    fn autosave_captured(&mut self, _: &AutosaveMemory) -> Result<()> {
        ensure!(self.fail != 2, "publish failure");
        self.published += 1;
        Ok(())
    }
    fn show_scene_title(&mut self, title: &str) -> Result<()> {
        ensure!(self.fail != 3, "title failure");
        self.titles.push(title.to_owned());
        Ok(())
    }
}
fn services(active: usize, boundary: u32, event: i32, n: usize) -> Services {
    let entries = std::array::from_fn(|i| ProgressEntry {
        scenario_id: i as i32 - 19,
        random_seed: 42 + i as u32 * 65536,
        save_position: 123 + i as u32,
        replay_target: 7 + i as u32,
        responses: (0..n + i).map(|n| (n * 137) as u8).collect(),
    });
    Services {
        save: SaveSession::new(
            Progress::new(active, entries).unwrap(),
            Default::default(),
            Default::default(),
            SaveTimeline {
                boundary,
                previous_message: 17,
                current_message: 23,
                previous_chapter: 32,
                current_chapter: 42,
                last_skip: 99,
                current_interaction: 0,
                resume_position: 0,
                normal_messages: u32::MAX,
                chapter_messages: 18,
                event,
            },
        ),
        time: 0x123456789abcdef,
        clocks: 0,
        titles: Vec::new(),
        published: 0,
        fail: 0,
    }
}
#[test]
fn ger_save_command_clock_publication_title_retries_preserve_applied_prefix_and_checkpoint() {
    let mut s = services(1, 99, -1, 33);
    let vm = vm();
    let layers = Gerokasu2Layers::default();
    s.save.metadata.set(0, "半角ｶﾅ😀").unwrap();
    s.save.metadata.set(1, "プロローグ").unwrap();
    let mut command = s.save.begin_autosave(&vm, &layers).unwrap();
    assert_eq!(s.save.timeline.boundary, 100);
    assert_eq!(s.save.checkpoints.len(), 1);
    let old = s.save.progress.entries[0].clone();
    s.fail = 1;
    assert!(command.poll_live(&mut s).is_err());
    assert!(s.save.autosave.valid);
    assert_eq!(s.save.autosave.packed_time, 0);
    assert_eq!(s.save.progress.active_entry().save_position, 100);
    s.fail = 2;
    assert!(command.poll_live(&mut s).is_err());
    let captured = s.save.autosave.clone();
    assert_eq!(s.clocks, 2);
    assert_eq!(captured.progress.entries[0], old);
    s.fail = 3;
    assert!(command.poll_live(&mut s).is_err());
    assert_eq!(s.clocks, 2);
    assert_eq!(s.published, 1);
    s.save.metadata.set(1, "次の章").unwrap();
    s.fail = 0;
    assert!(command.poll_live(&mut s).unwrap());
    assert!(command.poll_live(&mut s).unwrap());
    assert_eq!(s.titles, ["プロローグ"]);
    assert_eq!(s.published, 1);
    assert_eq!(s.save.autosave, captured);
    assert_eq!(s.save.metadata.saved()[0].as_ref(), "半角ｶﾅ😀");
    assert!(s.save.metadata.set(2, "bad").is_err());
    let checkpoint = &s.save.checkpoints[0];
    assert_eq!(checkpoint.timeline.boundary, 99);
    assert_eq!(checkpoint.vm.position(), vm.position());
    assert_eq!(checkpoint.vm.random_state(), 42);
    let mut copy = checkpoint.vm.scripter();
    let copied = copy
        .run(shin_core::vm::command::CommandResult::None, 32)
        .unwrap();
    assert!(matches!(
        copied,
        shin_core::vm::gerokasu2::Gerokasu2RuntimeCommand::Shared(
            shin_core::vm::command::RuntimeCommand::EXIT(_)
        )
    ));
}
fn words(s: &Services, done: bool, flags: [u8; 2], checkpoints: u32) -> Vec<u8> {
    let mut out = b"GSV1".to_vec();
    let t = s.save.timeline;
    for w in [
        done as u32,
        u32::from(flags[0]),
        u32::from(flags[1]),
        checkpoints,
        s.clocks,
        s.titles.len() as u32,
        s.save.progress.active_index() as u32,
        s.save.autosave.valid as u32,
        s.save.autosave.progress.active_index() as u32,
        t.boundary,
        t.previous_message,
        t.current_message,
        t.previous_chapter,
        t.current_chapter,
        t.last_skip,
        t.normal_messages,
        t.chapter_messages,
        t.event as u32,
    ] {
        out.extend(w.to_le_bytes());
    }
    out.extend(s.save.autosave.packed_time.to_le_bytes());
    for e in s
        .save
        .progress
        .entries
        .iter()
        .chain(s.save.autosave.progress.entries.iter())
    {
        for w in [
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.responses.len() as u32,
        ] {
            out.extend(w.to_le_bytes());
        }
        out.extend(&e.responses);
    }
    let [a, b] = s.save.metadata.current();
    let [c, d] = s.save.metadata.saved();
    for text in [
        a.as_ref(),
        c.as_ref(),
        b.as_ref(),
        d.as_ref(),
        s.titles.last().map(String::as_str).unwrap_or(""),
    ] {
        out.extend((text.len() as u32).to_le_bytes());
        out.extend(text.as_bytes());
    }
    out
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original Ger save constructors/replay/live/progress-copy bodies;CPU only"]
fn ger_native_save_metadata_autosave_message_boundaries_and_two_progress_entries() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/save");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-save-commands");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-save-commands.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut request = Vec::new();
    let mut expected = Vec::new();
    let vm = vm();
    let layers = Gerokasu2Layers::default();
    fn row(request: &mut Vec<u8>, r: [u32; 8]) {
        request.extend(r.into_iter().flat_map(u32::to_le_bytes));
    }
    for active in 0..2 {
        for boundary in [0, 1, 99, 100, 201, u32::MAX] {
            for event in [-1, 0, -2, i32::MAX] {
                for n in [0, 1, 33, 1024] {
                    let mut s = services(active, boundary, event, n);
                    row(
                        &mut request,
                        [
                            0,
                            active as u32,
                            0,
                            0,
                            n as u32,
                            n as u32 + 1,
                            s.time as u32,
                            (s.time >> 32) as u32,
                        ],
                    );
                    let t = s.save.timeline;
                    for w in [
                        t.boundary,
                        t.previous_message,
                        t.current_message,
                        t.previous_chapter,
                        t.current_chapter,
                        t.last_skip,
                        t.normal_messages,
                        t.chapter_messages,
                        t.event as u32,
                    ] {
                        request.extend(w.to_le_bytes());
                    }
                    for e in &s.save.progress.entries {
                        for w in [
                            e.scenario_id as u32,
                            e.random_seed,
                            e.save_position,
                            e.replay_target,
                        ] {
                            request.extend(w.to_le_bytes());
                        }
                    }
                    for e in &s.save.progress.entries {
                        request.extend(&e.responses);
                    }
                    expected.push(words(&s, true, [0; 2], 0));
                    for (level, text) in [
                        (0, "ﾊﾝｶｸ😀"),
                        (1, "プロローグ 最初の章"),
                        (1, ""),
                        (1, "題名あいうえおかきくけこさしすせそ😀😀😀"),
                    ] {
                        row(&mut request, [1, level, text.len() as u32, 0, 0, 0, 0, 0]);
                        request.extend(text.as_bytes());
                        s.save.metadata.set(level as i32, text).unwrap();
                        expected.push(words(&s, true, [0; 2], 0));
                    }
                    let mut command = s.save.begin_autosave(&vm, &layers).unwrap();
                    row(&mut request, [2, 0, 0, 0, 0, 0, 0, 0]);
                    expected.push(words(&s, true, command.flags(), 1));
                    for b in [0, 123, 124, u32::MAX] {
                        s.save.timeline.boundary = b;
                        row(&mut request, [7, b, event as u32, 0, 0, 0, 0, 0]);
                        expected.push(words(&s, true, command.flags(), 1));
                        let done = command.poll_replay(&s.save.progress, b).unwrap();
                        row(&mut request, [3, 0, 0, 0, 0, 0, 0, 0]);
                        expected.push(words(&s, done, command.flags(), 1));
                    }
                    command.poll_live(&mut s).unwrap();
                    row(&mut request, [4, 0, 0, 0, 0, 0, 0, 0]);
                    expected.push(words(&s, true, command.flags(), 1));
                    // The saved labels remain independent of subsequent live SAVEINFO.
                    let text = "次の題名";
                    row(&mut request, [1, 1, text.len() as u32, 0, 0, 0, 0, 0]);
                    request.extend(text.as_bytes());
                    s.save.metadata.set(1, text).unwrap();
                    expected.push(words(&s, true, [0; 2], 1));
                    s.save.message(&vm, &layers).unwrap();
                    row(&mut request, [5, 19, 0, 0, 0, 0, 0, 0]);
                    let checkpoints = 1 + u32::from(event < 0);
                    expected.push(words(&s, true, [0; 2], checkpoints));
                    SaveCapture::default().poll(&mut s).unwrap();
                    row(&mut request, [6, 0, 0, 0, 0, 0, 0, 0]);
                    expected.push(words(&s, true, [0; 2], checkpoints));
                }
            }
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut at = 0;
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(&output.stdout[at..at + e.len()], e, "save snapshot{i}");
        at += e.len();
    }
    assert_eq!(at, output.stdout.len());
    println!(
        "GEROKASU2_SAVE_COMMANDS_NATIVE snapshots={} compared_bytes={} original_metadata_ctor_replay_live_message_ctor_two_complete_routes=verified",
        expected.len(),
        at
    );
}
