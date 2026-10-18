use super::*;
#[test]
fn ger_selection_history_original_index_four_bit_result_and_disabled_indexes() {
    let mut h = SelectionHistory::default();
    let mut request = SelectionRequest {
        base: 7,
        index: 1,
        mask: 5,
        variants: vec!["a".into(), "b".into(), "c".into()],
    };
    h.record(&request, 2).unwrap();
    assert!(h.seen.contains(9));
    assert_eq!(h.last.len(), 32);
    assert_eq!(h.last[1], 3);
    assert!(h.record(&request, -1).is_err());
    assert!(h.record(&request, 12).is_err());
    request.base = -1;
    request.index = -1;
    h.record(&request, 0).unwrap();
    assert!(!h.seen.contains(0));
    assert_eq!(h.last[0], 0);
    let mut save = super::super::save_commands::SaveSession::fresh(0, 42);
    save.record_selection(2);
    save.record_selection(11);
    assert_eq!(save.response_bit_position, 8);
    assert_eq!(save.progress.active_entry().responses[0], 0x2b);
    assert_eq!(save.timeline.boundary, 2);
}
#[test]
fn ger_selection_shared_model_retains_filtered_index_and_requires_full_close() {
    let mut m = ChoiceModel::gerokasu2(4, 0x8005, None);
    assert_eq!(m.indices, [0, 2]);
    for _ in 0..8 {
        m.update(Ticks::from_f32(10.0));
    }
    m.move_selection(1);
    m.confirm();
    assert_eq!(m.result, None);
    for _ in 0..30 {
        m.update(Ticks::from_f32(10.0));
    }
    assert_eq!(m.result, Some(2));
}
#[test]
fn ger_selection_capture_retry_retains_owner_history_and_four_bit_result() {
    use super::super::save_commands::{SaveServices, SaveSession};
    struct Host {
        save: SaveSession,
        history: SelectionHistory,
        installs: usize,
        clocks: usize,
        publications: usize,
        fail_publish: bool,
    }
    impl SelectionServices for Host {
        fn begin_selection(&mut self, _: &SelectionRequest) -> Result<()> {
            self.installs += 1;
            Ok(())
        }
        fn selection_result(&mut self) -> Option<i32> {
            Some(2)
        }
        fn record_selection_history(&mut self, r: &SelectionRequest, result: i32) -> Result<()> {
            self.history.record(r, result)
        }
        fn finish_selection(&mut self) {}
    }
    impl SaveServices for Host {
        fn save_session(&mut self) -> &mut SaveSession {
            &mut self.save
        }
        fn packed_time(&mut self) -> Result<u64> {
            self.clocks += 1;
            Ok(123)
        }
        fn autosave_captured(
            &mut self,
            _: &shin_core::format::save::gerokasu2::AutosaveMemory,
        ) -> Result<()> {
            self.publications += 1;
            if std::mem::take(&mut self.fail_publish) {
                anyhow::bail!("temporary publication failure");
            }
            Ok(())
        }
        fn show_scene_title(&mut self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    let mut host = Host {
        save: SaveSession::fresh(0, 42),
        history: Default::default(),
        installs: 0,
        clocks: 0,
        publications: 0,
        fail_publish: true,
    };
    let mut command = SelectionCommand::new(SelectionRequest {
        base: 7,
        index: 1,
        mask: 5,
        variants: vec!["a".into(), "b".into(), "c".into()],
    });
    command.bind(&mut host).unwrap();
    assert!(command.capture_before(&mut host).is_err());
    command.bind(&mut host).unwrap();
    command.capture_before(&mut host).unwrap();
    assert_eq!((host.installs, host.clocks), (1, 1));
    let result = command.poll(&mut host).unwrap();
    command.record_history(&mut host, result).unwrap();
    host.fail_publish = true;
    assert!(command.capture_after(result, &mut host).is_err());
    command.record_history(&mut host, result).unwrap();
    command.capture_after(result, &mut host).unwrap();
    command.capture_after(result, &mut host).unwrap();
    assert_eq!((host.installs, host.clocks, host.publications), (1, 2, 4));
    assert_eq!(
        (host.save.timeline.boundary, host.save.response_bit_position),
        (1, 4)
    );
    let responses = &host.save.progress.active_entry().responses;
    assert_eq!(responses[0], 0x20);
    assert!(responses[1..].iter().all(|&byte| byte == 0));
    assert!(host.history.seen.contains(9));
    assert_eq!(host.history.last[1], 3);
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original Ger SelectLayer constructor/update/input/tweens;CPU only"]
fn ger_native_select_layer_filtered_shuffle_tweens_input_and_result_match_shared_model() {
    use crate::adv::choice::model::Phase;
    use std::{
        fs,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/selection");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-model");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-selection-model.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut snapshots = 0;
    let mut bytes = 0;
    let mut compare =
        |r: [u32; 4], sizes: &[u32], m: &ChoiceModel, hidden: bool, opacity: f32, sounds: u32| {
            input
                .write_all(
                    &r.into_iter()
                        .chain(sizes.iter().copied())
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
            let phase = match m.phase {
                Phase::Initial => 0,
                Phase::Opening => 1,
                Phase::Waiting => 2,
                Phase::Flash => 3,
                Phase::Closing => 4,
                Phase::Done => 5,
            };
            let mut expected = b"GSM1".to_vec();
            for w in [
                phase,
                m.selected.map_or(u32::MAX, |i| i as u32),
                m.result.map_or(u32::MAX, |i| i as u32),
                sounds,
                m.indices.len() as u32,
                hidden as u32,
                opacity.to_bits(),
                m.top().to_bits(),
            ] {
                expected.extend(w.to_le_bytes());
            }
            for ((&index, row), &height) in m.indices.iter().zip(&m.rows).zip(&m.heights) {
                expected.extend((index as u32).to_le_bytes());
                expected.extend(height.to_bits().to_le_bytes());
                for t in [&row.x, &row.alpha, &row.focus, &row.flash] {
                    for w in [
                        t.value().to_bits(),
                        t.target_value().to_bits(),
                        (!t.is_idle()) as u32,
                    ] {
                        expected.extend(w.to_le_bytes());
                    }
                }
            }
            let mut actual = vec![0; expected.len()];
            output.read_exact(&mut actual).unwrap_or_else(|error| {
                panic!(
                    "native select read at snapshot{snapshots},r{r:?},indices{:?}: {error}",
                    m.indices
                )
            });
            let mismatch = actual
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .enumerate()
                .find_map(|(i, (a, b))| {
                    (a != b).then(|| {
                        (
                            i,
                            u32::from_le_bytes(a.try_into().unwrap()),
                            u32::from_le_bytes(b.try_into().unwrap()),
                        )
                    })
                });
            assert_eq!(
                mismatch, None,
                "native select snapshot{snapshots},r{r:?},indices{:?},phase{:?}",
                m.indices, m.phase
            );
            snapshots += 1;
            bytes += actual.len();
        };
    for count in [0, 1, 3, 7, 12] {
        for mask in [0, 1, 5, 0xaaa, 0xfff, 0x1005, 0x9aaa, u32::MAX] {
            // Native c24c0 unconditionally dereferences row0 when bit15 is
            // set; an empty filtered menu violates that constructor contract.
            if mask & 0x8000 != 0 && mask & ((1u32 << count) - 1) == 0 {
                continue;
            }
            for seed in [0, 42, 0xdeadbeef, u32::MAX] {
                let sizes = (0..count).map(|i| 70 + (i % 3) * 32).collect::<Vec<u32>>();
                let mut m = ChoiceModel::gerokasu2(
                    count as usize,
                    mask as i32,
                    (mask & 0x1000 != 0).then_some(seed),
                );
                m.heights = m.indices.iter().map(|&i| sizes[i] as f32).collect();
                let mut sounds = 0;
                let mut hidden = false;
                let mut opacity = 1.0f32;
                compare([0, count, mask, seed], &sizes, &m, hidden, opacity, sounds);
                for step in 0..100 {
                    if step == 5 || step == 9 {
                        hidden = !hidden;
                        compare([7, hidden as u32, 0, 0], &[], &m, hidden, opacity, sounds);
                    }
                    let delta: f32 = [0.0, 0.25, 1.0, 6.0, 10.0][step % 5];
                    opacity = delta
                        .mul_add(if hidden { -0.1 } else { 0.1 }, opacity)
                        .clamp(0.0, 1.0);
                    if opacity >= 1.0 {
                        m.update(Ticks::from_f32(delta));
                    }
                    sounds += m.take_sounds().len() as u32;
                    compare([1, delta.to_bits(), 0, 0], &[], &m, hidden, opacity, sounds);
                    if !m.indices.is_empty()
                        && (step == 35 || step == 38 || step == 41 || step == 44)
                    {
                        let bits = match step {
                            35 => 1 | 2 | 32,
                            38 => 4 | 8 | 16,
                            41 => 8,
                            44 => 32,
                            _ => 0,
                        };
                        m.input(
                            bits & 1 != 0,
                            bits & 2 != 0,
                            bits & 4 != 0,
                            bits & 8 != 0,
                            bits & 16 != 0,
                            bits & 32 != 0,
                        );
                        sounds += m.take_sounds().len() as u32;
                        compare([2, bits, 0, 0], &[], &m, hidden, opacity, sounds);
                    }
                }
            }
        }
    }
    // Real contact callbacks additionally use the pre-shuffle row88 index.
    // Keep graph hit/capture ordering explicit; this tests the layer handlers,
    // while the app hardware test covers accepted host gestures/focus loss.
    for mask in [0xfff, 0x1005] {
        for seed in [42, u32::MAX] {
            let sizes = [70, 102, 134];
            let mut m =
                ChoiceModel::gerokasu2(3, mask as i32, (mask & 0x1000 != 0).then_some(seed));
            m.heights = m.indices.iter().map(|&i| sizes[i] as f32).collect();
            let mut sounds = 0;
            compare([0, 3, mask, seed], &sizes, &m, false, 1.0, sounds);
            for _ in 0..10 {
                m.update(Ticks::from_f32(10.0));
                sounds += m.take_sounds().len() as u32;
                compare([1, 10.0f32.to_bits(), 0, 0], &[], &m, false, 1.0, sounds);
            }
            let point = Vec2::new(960.0, m.top() + m.heights[0] * 0.5);
            compare(
                [3, point.x.to_bits(), point.y.to_bits(), 0],
                &[],
                &m,
                false,
                1.0,
                sounds,
            );
            m.pointer_state(point, true);
            sounds += m.take_sounds().len() as u32;
            compare([4, 0, 0, 0], &[], &m, false, 1.0, sounds);
            m.cancel_pointer();
            compare([6, 0, 0, 0], &[], &m, false, 1.0, sounds);
            compare(
                [3, point.x.to_bits(), point.y.to_bits(), 0],
                &[],
                &m,
                false,
                1.0,
                sounds,
            );
            m.pointer_state(point, true);
            compare([4, 0, 0, 0], &[], &m, false, 1.0, sounds);
            m.pointer_state(point, false);
            sounds += m.take_sounds().len() as u32;
            compare([5, 0, 0, 0], &[], &m, false, 1.0, sounds);
            for _ in 0..30 {
                m.update(Ticks::from_f32(6.0));
                sounds += m.take_sounds().len() as u32;
                compare([1, 6.0f32.to_bits(), 0, 0], &[], &m, false, 1.0, sounds);
            }
        }
    }
    drop(compare);
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_SELECTION_MODEL_NATIVE_MATCH snapshots={snapshots} bytes={bytes} original_ctor_filter_clock_shuffle_Ger_tweens_hidden_input_result=verified label_sizes_GPU_base_clock_cues=bounded"
    );
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original Ger SELECT factory/live poll/read/history/route;CPU only"]
fn ger_native_select_command_constructor_capture_history_route_feedback_and_completion() {
    use shin_core::{
        format::{rom::RomReader, scenario::gerokasu2::Gerokasu2Scenario},
        primitives::stateless_reader::StatelessFile,
        vm::gerokasu2::Gerokasu2Scripter,
    };
    use std::{
        fs,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/selection");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-command");
    let c = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-selection.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    let mut rom = RomReader::new(
        StatelessFile::new(fs::File::open(root.join("gerokasu2/data.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    let scenario = Arc::new(Gerokasu2Scenario::new(data.into()).unwrap());
    let vm = Gerokasu2Scripter::new(scenario, 0, 42);
    let layers = super::super::layers::Gerokasu2Layers::default();
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut snapshots = 0;
    let mut bytes = 0;
    fn snapshot(
        save: &super::super::save_commands::SaveSession,
        h: &SelectionHistory,
        head: [u32; 13],
    ) -> Vec<u8> {
        let mut out = b"GSC1".to_vec();
        for w in head.into_iter().chain([
            save.response_bit_position as u32,
            h.seen.size_in_words() as u32,
            h.last.len() as u32,
            save.progress.active_entry().responses.len() as u32,
        ]) {
            out.extend(w.to_le_bytes());
        }
        let mut words = vec![0u32; h.seen.size_in_words()];
        for id in h.seen.iter() {
            words[id / 32] |= 1 << (id & 31);
        }
        out.extend(words.into_iter().flat_map(u32::to_le_bytes));
        out.extend(&h.last);
        out.extend(&save.progress.active_entry().responses);
        out
    }
    let mut compare = |r: [u32; 4],
                       save: &super::super::save_commands::SaveSession,
                       h: &SelectionHistory,
                       head: [u32; 13]| {
        input
            .write_all(&r.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>())
            .unwrap();
        let expected = snapshot(save, h, head);
        let mut actual = vec![0; expected.len()];
        output.read_exact(&mut actual).unwrap();
        assert_eq!(
            actual, expected,
            "native command snapshot{snapshots},r{r:?}"
        );
        snapshots += 1;
        bytes += actual.len();
    };
    for base in [-1, 0, 7, 8191, 65534] {
        for index in [-1, 0, 31, 32, 65534] {
            for has_text in [false, true] {
                for result in [0, 2] {
                    let request = SelectionRequest {
                        base,
                        index,
                        mask: 5,
                        variants: vec!["日本語0".into(), "半角ｶﾅ1".into(), "絵文字😀2".into()],
                    };
                    let mut save = super::super::save_commands::SaveSession::fresh(0, 42);
                    save.interactive(&vm, &layers, has_text).unwrap();
                    let mut h = SelectionHistory::default();
                    let checks = (!has_text) as u32;
                    let mut head = [
                        0,
                        checks,
                        0,
                        0,
                        0,
                        0,
                        0,
                        save.timeline.boundary,
                        save.timeline.current_interaction,
                        save.timeline.last_skip,
                        0,
                        0,
                        0,
                    ];
                    compare(
                        [0, base as u32, index as u32, has_text as u32],
                        &save,
                        &h,
                        head,
                    );
                    if base >= 0 {
                        h.seen.add(base as usize + 2);
                        compare([3, base as u32 + 2, 0, 0], &save, &h, head);
                    }
                    head[2] = 1;
                    head[3] = 1;
                    head[10] = 5;
                    head[11] = if base >= 0 { 4 } else { 0 };
                    head[12] = 3;
                    compare([1, 0, 0, 0], &save, &h, head);
                    compare([1, 0, 0, 0], &save, &h, head);
                    compare([2, result as u32, 0, 0], &save, &h, head);
                    h.record(&request, result).unwrap();
                    save.record_selection(result);
                    head[0] = 1;
                    head[2] = 2;
                    head[4] = 1;
                    head[5] = result as u32;
                    head[6] = 1;
                    head[7] = save.timeline.boundary;
                    compare([1, 0, 0, 0], &save, &h, head);
                }
            }
        }
    }
    drop(compare);
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_SELECTION_COMMAND_NATIVE_MATCH snapshots={snapshots} bytes={bytes} original_ctor_poll_read_history_four_bit_route_feedback_capture_order=verified prepared_UI_checkpoint_register_autosave=bounded"
    );
}
