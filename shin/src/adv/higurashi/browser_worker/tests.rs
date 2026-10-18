use super::*;
use shin_core::format::{font::read_font_metrics, text::write_sjis_string};
use std::{
    fmt::Write as _,
    io::{BufReader, Write as _},
    process::{Command, Stdio},
};

fn hex(text: &str) -> String {
    let mut bytes = Vec::new();
    write_sjis_string(text, &mut bytes).unwrap();
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn snapshot(out: &mut String, m: &Measurement) {
    write!(
        out,
        "S {} {} {} {} {} {} {}",
        m.selected,
        m.selected,
        m.viewport.first,
        m.viewport.end,
        m.viewport.minimum.to_bits(),
        m.viewport.extent.to_bits(),
        u8::from(m.active)
    )
    .unwrap();
    for row in &m.measured {
        let (top, height, voice) = row.map_or((0, 0, 0), |r| {
            (r.top.to_bits(), r.height.to_bits(), r.voice_count)
        });
        write!(out, " {top} {height} {voice}").unwrap();
    }
    out.push('\n');
}
#[test]
#[ignore = "executes original75b28 worker/range locks and actual font/parser/layout without GPU/audio/save effects"]
fn higurashi_browser_worker_matches_native_outward_measurement_and_cancel() {
    let fontpath = std::env::var("SHIN_HIGURASHI_FONT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    let font =
        read_font_metrics(&mut BufReader::new(std::fs::File::open(&fontpath).unwrap())).unwrap();
    let mut input = String::new();
    let mut expected = String::new();
    let mut owners = 0;
    let mut ready_while_active = false;
    for count in [1, 4, 20, 101, 128] {
        let mut anchors = vec![0, count / 2, count];
        anchors.sort();
        anchors.dedup();
        for anchor in anchors {
            for profile in 0..3 {
                for limit in [0, 1, 2, 6, 103] {
                    let rows: Arc<[Row]> = (0..count)
                        .map(|index| Row {
                            index: index as u32,
                            boundary: index as u32 + 1,
                            item: match (index + profile) % 8 {
                                0 => Item::Message {
                                    text: String::new(),
                                    style: 0,
                                    argument: Some(0),
                                },
                                1 => Item::Message {
                                    text: "短い@r本文".into(),
                                    style: 0,
                                    argument: Some(-1),
                                },
                                2 => Item::Message {
                                    text: "@s50.小さい".into(),
                                    style: 3,
                                    argument: Some(9),
                                },
                                3 => Item::Message {
                                    text: "@vvoice1.@r本文@vvoice2.".into(),
                                    style: 4,
                                    argument: Some(9),
                                },
                                4 => Item::Choice {
                                    title: "選択".into(),
                                    text: "返答\n本文".into(),
                                },
                                5 => Item::Checkpoint {
                                    title: "章".into(),
                                    text: if profile == 1 { "" } else { "説明" }.into(),
                                },
                                6 => Item::CharacterChoice { response: profile },
                                _ => Item::Message {
                                    text: "@vvoice.".repeat(256),
                                    style: 0,
                                    argument: Some(9),
                                },
                            },
                        })
                        .collect();
                    writeln!(input, "{count} {anchor} {limit}").unwrap();
                    for row in rows.iter() {
                        let (kind, style, argument, title, text) = match &row.item {
                            Item::Message {
                                text,
                                style,
                                argument,
                            } => (0, *style, argument.unwrap_or(-1), "", text.as_str()),
                            Item::Choice { title, text } => {
                                (1, 0, 0, title.as_str(), text.as_str())
                            }
                            Item::Checkpoint { title, text } => {
                                (2, 0, 0, title.as_str(), text.as_str())
                            }
                            Item::CharacterChoice { response } => (3, 0, *response, "", ""),
                        };
                        writeln!(
                            input,
                            "{kind} {style} {argument} {} {}",
                            hex(title),
                            hex(text)
                        )
                        .unwrap();
                    }
                    let mut m = Measurement::new(rows, anchor as usize).unwrap();
                    let mut commits = 1;
                    snapshot(&mut expected, &m);
                    if commits == limit {
                        m.active = false;
                    }
                    loop {
                        if !m.active {
                            snapshot(&mut expected, &m);
                            break;
                        }
                        m.step(&font).unwrap();
                        snapshot(&mut expected, &m);
                        commits += 1;
                        ready_while_active |=
                            m.viewport.end - m.viewport.first >= 100 && m.ends != 3;
                        if commits == limit {
                            m.active = false;
                        }
                        if m.ends == 3 {
                            m.active = false;
                            break;
                        }
                    }
                    writeln!(expected, "F {}", u8::from(m.active)).unwrap();
                    owners += 1;
                }
            }
        }
    }
    assert!(ready_while_active);
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_WORKER").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            &fontpath,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    let cap = std::path::PathBuf::from(
        std::env::var("SHIN_HIGURASHI_BROWSER_WORKER_CAPTURE")
            .unwrap_or_else(|_| "/tmp/higurashi-browser-worker-packets".into()),
    );
    std::fs::create_dir_all(&cap).unwrap();
    std::fs::write(cap.join("input.txt"), input).unwrap();
    std::fs::write(cap.join("native.txt"), &output.stdout).unwrap();
    std::fs::write(cap.join("native.log"), &output.stderr).unwrap();
    std::fs::write(cap.join("rust.txt"), &expected).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    let wanted: Vec<_> = expected.lines().collect();
    assert_eq!(lines.len(), wanted.len());
    for (index, (a, b)) in lines.iter().zip(&wanted).enumerate() {
        assert_eq!(a, b, "publication{index}");
    }
    println!(
        "Browser worker:{owners} native owners/{} range snapshots/{} exact words; height floor, outward order, raw end anchor, voice byte wrap,100-row readiness and cancellation pass",
        lines.len() - owners,
        actual.split_whitespace().count()
    );
}

fn request(count: usize) -> Request {
    // Small updated SNR, using the same authored opcode encoding as the
    // independent history replay matrix. No disk writes or ADV effects.
    let mut bytes = vec![0; 0x50];
    bytes[..4].copy_from_slice(b"SNR ");
    bytes[12..16].copy_from_slice(&65u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&133u32.to_le_bytes());
    for i in 0..11 {
        let at = bytes.len() as u32;
        bytes[36 + i * 4..40 + i * 4].copy_from_slice(&at.to_le_bytes());
        if !matches!(i, 7 | 8) {
            bytes.extend(4u32.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
    }
    let at = bytes.len() as u32;
    bytes[32..36].copy_from_slice(&at.to_le_bytes());
    for i in 0..count {
        bytes.push(0x86);
        bytes.extend(&(i as u32 + 1).to_le_bytes()[..3]);
        bytes.push(0);
        bytes.push(0x7f);
        let text = format!("Name@rBody{i}@vvoice.");
        bytes.extend(((text.len() + 1) as u16).to_le_bytes());
        bytes.extend(text.as_bytes());
        bytes.push(0);
    }
    bytes.extend([0, 0, 0]);
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let entries = std::array::from_fn(|i| super::super::autosave::ProgressEntry {
        scenario_id: 42,
        random_seed: 42,
        save_position: if i == 0 { 5 } else { 17 },
        replay_target: 19,
        response_data: vec![0; 16],
    });
    Request {
        scenario: Arc::new(HigurashiScenario::new(bytes.into()).unwrap()),
        source: ProgressMemory::new(
            Default::default(),
            super::super::autosave::SaveProgress::new(0, 0, entries).unwrap(),
            vec![0; 16],
        ),
        read_history: MessageReadHistory::new(),
        choices: ChoiceHistory::new(
            shin_core::format::save::index_set::IndexSet::new(),
            vec![0; 32],
        ),
        read_only: false,
        command_limit: 100000,
    }
}
#[test]
fn higurashi_browser_history_cancel_preserves_source_and_end_anchor() {
    let request = request(20);
    let original = request.source.clone();
    let mut calls = 0;
    let result = BrowserHistory::collect_cancellable(
        &request.scenario,
        &request.source,
        &request.read_history,
        &request.choices,
        false,
        1000,
        || {
            calls += 1;
            calls == 8
        },
    );
    assert!(result.err().unwrap().to_string().contains("cancelled"));
    assert_eq!(request.source.progress(), original.progress());
    assert_eq!(request.source.persistent().0, original.persistent().0);
    assert_eq!(request.source.bytes(), original.bytes());
    let expected = BrowserHistory::collect(
        &request.scenario,
        &request.source,
        &request.read_history,
        &request.choices,
        false,
        1000,
    )
    .unwrap();
    assert_eq!(expected.rows.len(), 20);
    assert!(expected.measurement_anchor as usize <= expected.rows.len());
}

#[derive(Default)]
struct Service {
    questions: Vec<super::super::browser_lifecycle::Question>,
    navigation: Vec<super::super::browser_lifecycle::NavigationViewRequest>,
}
#[test]
fn higurashi_browser_worker_range_publication_keeps_input_owners() {
    let placeholder = RowMetadata {
        kind: 0,
        voice_count: 0,
        top: 0.0,
        height: 1.0,
    };
    let measured = RowMetadata {
        height: 112.0,
        ..placeholder
    };
    let mut controls = BrowserControls::new(
        vec![placeholder, measured, placeholder],
        Viewport {
            first: 1,
            end: 2,
            minimum: 0.0,
            extent: 112.0,
        },
        1,
        0.25,
    )
    .unwrap();
    controls.set_cached_rows([1]);
    controls.input(
        1,
        1 << 15,
        0,
        &PointerFrame {
            number: 0,
            time_seconds: 0.0,
            events: &[],
        },
        &mut Service::default(),
    );
    let before = controls.flashes(1).unwrap();
    let flash = (before.jump.value().to_bits(), before.jump.is_waiting());
    assert!(flash.1);
    let rows = vec![
        RowMetadata {
            top: -112.0,
            ..measured
        },
        measured,
        RowMetadata {
            top: 112.0,
            ..measured
        },
    ];
    let grown = Viewport {
        first: 0,
        end: 3,
        minimum: -112.0,
        extent: 224.0,
    };
    controls.publish_measurement(rows.clone(), grown).unwrap();
    assert_eq!(controls.selected(), 1);
    assert_eq!(controls.scroll().to_bits(), 0.25f32.to_bits());
    let after = controls.flashes(1).unwrap();
    assert_eq!(
        flash,
        (after.jump.value().to_bits(), after.jump.is_waiting())
    );
    assert!(
        controls
            .publish_measurement(rows.clone(), Viewport { first: 1, ..grown })
            .is_err()
    );
    let mut invalid = rows;
    invalid[2].height = f32::NAN;
    assert!(controls.publish_measurement(invalid, grown).is_err());
    assert_eq!(controls.viewport().first, 0);
    assert_eq!(controls.viewport().end, 3);
    assert_eq!(controls.selected_metadata().height, 112.0);
}
impl super::super::browser_controls::Services for Service {
    fn event(&mut self, _event: super::super::browser_controls::Event) {}
    fn voice_is_playing(&self) -> bool {
        false
    }
}
impl Services for Service {
    fn returned(&mut self, _complete: bool) {}
    fn question(&mut self, q: super::super::browser_lifecycle::Question) {
        self.questions.push(q)
    }
    fn question_cue(&mut self, _cue: super::super::question::Cue) {}
    fn reconstruct(&mut self, _source: &ProgressMemory, _mode: u32) -> Result<()> {
        Ok(())
    }
    fn navigation_view(&mut self, q: super::super::browser_lifecycle::NavigationViewRequest) {
        self.navigation.push(q)
    }
}
#[test]
#[ignore = "real history/measurement/atlas loader tasks, retained browser and visible GPU cache; no audio/window/save effects"]
fn higurashi_browser_worker_retains_assets_cache_and_joins_cancelled_work() {
    use super::super::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
    use shin_core::primitives::color::UnormColor;
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let system: Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
    let input = request(120);
    let expected = BrowserHistory::collect(
        &input.scenario,
        &input.source,
        &input.read_history,
        &input.choices,
        false,
        100000,
    )
    .unwrap();
    let original = input.source.clone();
    let mut browser = Browser::start(assets.clone(), input);
    let handles = browser.worker.retirements.clone();
    let cancel = browser.worker.cancelled.clone();
    let mut service = Service::default();
    let tick = |browser: &mut Browser, service: &mut Service, pressed| {
        browser
            .poll(
                Ticks::from_f32(1.0),
                1,
                pressed,
                0,
                &PointerFrame {
                    events: &[],
                    number: 0,
                    time_seconds: 0.0,
                },
                false,
                service,
            )
            .unwrap()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tick(&mut browser, &mut service, 0);
        if browser.lifecycle().is_some_and(|b| b.slide() == 1.0)
            && browser.snapshot.as_ref().is_some_and(|s| !s.worker_active)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Browser worker readiness timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let snapshot = browser.snapshot.as_ref().unwrap();
    assert_eq!(&*snapshot.history.rows, &expected.rows);
    assert_eq!(snapshot.history.current_row, expected.current_row.unwrap());
    assert_eq!(
        snapshot.history.jump_source.progress(),
        expected.jump_source.progress()
    );
    assert_eq!(
        snapshot.history.jump_source.persistent().0,
        original.persistent().0
    );
    assert_eq!(
        snapshot.history.jump_source.bytes(),
        expected.jump_source.bytes()
    );
    assert_eq!(snapshot.viewport.first, 0);
    assert_eq!(snapshot.viewport.end, 120);
    assert!(browser.worker.atlas().is_some());
    browser.prepare(0).unwrap();
    assert!(!browser.cache.is_empty());
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_prepared_frame(
            &device,
            &queue,
            size,
            UnormColor::WHITE,
            |_| {},
            |pass| browser.render(pass, &system),
        );
        assert!(pixels.iter().any(|v| *v != 255));
        frames += 1;
    }
    tick(&mut browser, &mut service, 1 << 12);
    assert_eq!(service.navigation.len(), 1);
    let loaded = shin_tasks::block_on(service.navigation[0].clone().load(&assets)).unwrap();
    browser
        .lifecycle_mut()
        .unwrap()
        .publish_navigation_view(loaded)
        .unwrap();
    for _ in 0..6 {
        tick(&mut browser, &mut service, 0);
    }
    browser.prepare(0).unwrap();
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_prepared_frame(
            &device,
            &queue,
            size,
            UnormColor::WHITE,
            |_| {},
            |pass| browser.render(pass, &system),
        );
        assert!(pixels.iter().any(|v| *v != 255));
        frames += 1;
    }
    // Navigation jump owns its delayed feedback task; the parent publishes a
    // real retained question view and prepares its render target before drawing.
    // The earlier frames cover opening; input starts after phase0 finishes.
    for _ in 0..5 {
        tick(&mut browser, &mut service, 0);
    }
    let mut navigation_ready = false;
    browser.lifecycle_mut().unwrap().draw_navigation(|model| {
        navigation_ready |= model.phase() == 1;
    });
    assert!(navigation_ready);
    tick(&mut browser, &mut service, 1 << 15);
    for _ in 0..30 {
        if !service.questions.is_empty() {
            break;
        }
        tick(&mut browser, &mut service, 0);
    }
    assert_eq!(service.questions.len(), 1);
    let loaded = shin_tasks::block_on(service.questions[0].load(&assets)).unwrap();
    browser
        .lifecycle_mut()
        .unwrap()
        .publish_question(loaded)
        .unwrap();
    for _ in 0..8 {
        tick(&mut browser, &mut service, 0);
    }
    browser.prepare(0).unwrap();
    for size in [(1280, 720), (1920, 1080)] {
        let owner = std::cell::RefCell::new(&mut browser);
        let pixels = render_prepared_frame(
            &device,
            &queue,
            size,
            UnormColor::WHITE,
            |context| owner.borrow_mut().prepare_questions(context),
            |pass| owner.borrow_mut().render(pass, &system),
        );
        assert!(pixels.iter().any(|v| *v != 255));
        frames += 1;
    }
    drop(browser);
    assert!(cancel.load(Ordering::Acquire));
    assert!(handles.iter().all(|h| h.is_finished()));
    for count in [20, 120, 4096] {
        let browser = Browser::start(assets.clone(), request(count));
        let handles = browser.worker.retirements.clone();
        let cancel = browser.worker.cancelled.clone();
        drop(browser);
        assert!(cancel.load(Ordering::Acquire));
        assert!(handles.iter().all(|h| h.is_finished()));
    }
    println!(
        "Browser worker GPU:{frames} frames at720p/1080p; real history/atlas/font/measurement publication, visible content/shell/navigation/question composition;4 populated/pending owners join on retirement"
    );
}
