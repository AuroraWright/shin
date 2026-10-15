use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

pub(in crate::app::higurashi) fn source(scenario: i32, active: usize) -> RuntimeGameData {
    let entry = |i: usize| ProgressEntry {
        scenario_id: if i == active {
            scenario
        } else {
            (0x87654000u32 + i as u32 * 0x100) as i32
        },
        random_seed: 0x87654001 + i as u32 * 0x100,
        save_position: 0x87654002 + i as u32 * 0x100,
        replay_target: 0x87654003 + i as u32 * 0x100,
        response_data: (0..7 + i).map(|j| (j * 7 + 23 + i) as u8).collect(),
    };
    RuntimeGameData::from_progress(
        SaveProgress::new(active, 0xdeadbeef, [entry(0), entry(1)]).unwrap(),
        SaveDate::now(),
        vec![0, 17, 244, 315, u16::MAX],
        (0..193).map(|i| (i * 7 + 13) as u8).collect(),
    )
}
pub(super) fn vectors(pattern: usize) -> SaveVectors {
    let mut vectors = SaveVectors::default();
    vectors.extra_words_0 = vec![0; 20];
    for i in 0..315 {
        let status = if pattern == 4 { i % 4 } else { pattern };
        for (at, enabled) in [(2 * i, status & 1 != 0), (2 * i + 1, status & 2 != 0)] {
            if enabled {
                vectors.extra_words_0[at / 32] |= 1 << (at % 32);
            }
        }
    }
    vectors
}
fn append_data(out: &mut String, data: &RuntimeGameData, unknown_dormant: bool) {
    write!(
        out,
        " {} {}",
        data.progress.active_index(),
        data.progress.extra_word
    )
    .unwrap();
    for (i, entry) in data.progress.entries.iter().enumerate() {
        for v in [
            entry.scenario_id as u32,
            entry.random_seed,
            entry.save_position,
            entry.replay_target,
        ] {
            if i == 1 && unknown_dormant {
                out.push_str(" *");
            } else {
                write!(out, " {v}").unwrap();
            }
        }
        write!(out, " {}", entry.response_data.len()).unwrap();
        for v in &entry.response_data {
            write!(out, " {v}").unwrap();
        }
    }
    write!(out, " {}", data.trailing_values.len()).unwrap();
    for v in &data.trailing_values {
        write!(out, " {v}").unwrap();
    }
    write!(out, " {}", data.byte_variables.len()).unwrap();
    for v in &data.byte_variables {
        write!(out, " {v}").unwrap();
    }
}
struct Profile {
    model: Model,
    input: String,
    expected: Vec<String>,
    cues: [u32; 4],
    seeds: u32,
    dates: u32,
    attached: u32,
    queued: Option<SceneRequest>,
}
impl Profile {
    fn new(
        snr: &HigurashiScenario,
        has: bool,
        scenario: i32,
        active: usize,
        alt: u32,
        selected: i32,
        pattern: usize,
    ) -> Self {
        let original = source(scenario, active);
        let model = Model::new(
            snr,
            has.then(|| original.clone()),
            selected,
            alt,
            &vectors(pattern),
        )
        .unwrap();
        let mut p = Self {
            model,
            input: format!(
                "N {} {} {active} {alt} {selected} {pattern}\n",
                u32::from(has),
                scenario as u32
            ),
            expected: vec![],
            cues: [0; 4],
            seeds: 0,
            dates: 0,
            attached: 1,
            queued: None,
        };
        p.snapshot(-1);
        if let Some(data) = &p.model.data {
            assert_eq!(data, &original);
            for i in 0..2 {
                assert_ne!(
                    data.progress.entries[i].response_data.as_ptr(),
                    original.progress.entries[i].response_data.as_ptr()
                );
            }
            assert_ne!(
                data.trailing_values.as_ptr(),
                original.trailing_values.as_ptr()
            );
            assert_ne!(
                data.byte_variables.as_ptr(),
                original.byte_variables.as_ptr()
            );
        }
        p
    }
    fn snapshot(&mut self, result: i32) {
        for action in self.model.take_actions() {
            match action {
                Action::Cue(cue) => {
                    self.cues[["move", "decide", "cancel", "error"]
                        .iter()
                        .position(|v| *v == cue)
                        .unwrap()] += 1
                }
                Action::FadeMusic(ticks) => assert_eq!(ticks, 30),
            }
        }
        let m = &self.model;
        let (kind, flags, parameter2) = match &self.queued {
            None => (0, 0, 0),
            Some(SceneRequest::Story { flags, .. }) => (1, *flags, 0),
            Some(SceneRequest::Title { selected, mode }) => (2, *selected as u32, *mode),
            Some(SceneRequest::Origin { selected }) => (3, *selected as u32, 0),
        };
        let mut s = format!(
            "T {result} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {kind} {flags} {parameter2} {} {}",
            u32::from(m.data.is_some()),
            u32::from(m.alternate),
            m.selected,
            m.content_height,
            m.scroll.value().to_bits(),
            m.feedback.value().to_bits(),
            u32::from(m.closing),
            m.hit,
            m.hovered,
            m.pressed,
            m.speed.to_bits(),
            m.background_phase.to_bits(),
            self.cues[0],
            self.cues[1],
            self.cues[2],
            self.cues[3],
            self.seeds,
            self.attached,
            m.rows.len()
        );
        for row in &m.rows {
            write!(
                s,
                " {} {} {} {} {} {:016x}",
                row.source_index,
                u32::from(row.current),
                row.status,
                row.flash.value().to_bits(),
                row.scenario_id,
                row.texture_path
                    .bytes()
                    .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b))
                        .wrapping_mul(0x100000001b3))
            )
            .unwrap();
        }
        write!(s, " {}", m.completion_text.len()).unwrap();
        for b in m.completion_text.bytes() {
            write!(s, " {b}").unwrap();
        }
        if let Some(data) = &m.data {
            append_data(&mut s, data, false);
        }
        if let Some(SceneRequest::Story { data, flags }) = &self.queued {
            append_data(&mut s, data, m.data.is_none() && *flags == 0);
        }
        self.expected.push(s);
    }
    fn controls(&mut self, permission: bool, captured: bool, pressed: u32, repeat: u32) {
        writeln!(
            self.input,
            "U {} {} {pressed:x} {repeat:x}",
            u32::from(permission),
            u32::from(captured)
        )
        .unwrap();
        let closing = self.model.closing;
        let result = self.model.poll_controls(
            permission,
            captured,
            Buttons {
                pressed,
                repeat,
                ..Buttons::default()
            },
        );
        if !closing && self.model.closing {
            self.attached += 1;
        }
        self.snapshot(i32::from(result));
    }
    fn advance(&mut self, delta: f32) {
        writeln!(self.input, "A {delta}").unwrap();
        self.model.advance(Ticks::from_f32(delta));
        self.snapshot(-1);
    }
    fn contact(&mut self, x: f32, y: f32) {
        writeln!(self.input, "V {x} {y}").unwrap();
        let result = self.model.contact(x, y);
        self.snapshot(i32::from(result));
    }
    fn press(&mut self) {
        self.input.push_str("P\n");
        self.model.press_contact();
        self.snapshot(-1);
    }
    fn confirm(&mut self) {
        self.input.push_str("C\n");
        let closing = self.model.closing;
        self.model.confirm_contact();
        if !closing && self.model.closing {
            self.attached += 1;
        }
        self.snapshot(-1);
    }
    fn drag(&mut self, from_y: f32, to_y: f32) {
        writeln!(self.input, "G 200 {from_y} 200 {to_y}").unwrap();
        self.model.drag(from_y, to_y);
        self.snapshot(-1);
    }
    fn inertia(&mut self, speed: f32) {
        writeln!(self.input, "I {speed}").unwrap();
        self.model.inertia(speed);
        self.snapshot(-1);
    }
    fn release(&mut self) {
        self.input.push_str("R\n");
        self.model.release_contact();
        self.snapshot(-1);
    }
    fn callback(&mut self) {
        assert!(self.model.pending.is_some());
        self.input.push_str("F\n");
        let source = self.model.data.clone();
        let request = self
            .model
            .poll_request(
                || {
                    self.dates += 1;
                    Ok(SaveDate::now())
                },
                || {
                    self.seeds += 1;
                    Ok(0x8badf00d)
                },
            )
            .unwrap();
        let result = if let Some(request) = request {
            if let SceneRequest::Story { data, flags } = &request {
                if let Some(source) = &source {
                    if *flags == 1 {
                        assert_eq!(data, source);
                    } else {
                        assert_eq!(data.date, source.date);
                        assert_eq!(data.progress.extra_word, source.progress.extra_word);
                        assert_eq!(data.progress.entries[0], source.progress.entries[0]);
                        assert_eq!(
                            data.progress.entries[1].response_data,
                            source.progress.entries[1].response_data
                        );
                        assert_eq!(data.trailing_values, source.trailing_values);
                        assert_eq!(data.byte_variables, source.byte_variables);
                    }
                    assert_ne!(
                        data.progress.entries[0].response_data.as_ptr(),
                        source.progress.entries[0].response_data.as_ptr()
                    );
                }
            }
            self.queued = Some(request);
            0
        } else {
            1
        };
        assert_eq!(self.model.data, source);
        self.snapshot(result);
    }
    fn finish(mut self, input: &mut String, expected: &mut Vec<String>) {
        self.input.push_str("D\n");
        self.expected.push("D".into());
        input.push_str(&self.input);
        expected.extend(self.expected);
    }
}

#[test]
#[ignore = "actual updated TIPS constructor/catalog/input/drag/clocks/start/retreat callbacks;CPU only,no hardware or saves"]
fn higurashi_tips_catalog_input_and_full_scene_payloads_match_native() {
    let snr = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SNR").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut input = String::new();
    let mut expected = vec![];
    let mut owners = 0;
    // Each original table index must map correctly in each filtered catalog.
    for selected in -1..=315 {
        for alt in [0, 1] {
            Profile::new(&snr, false, 100, 0, alt, selected, 4).finish(&mut input, &mut expected);
            owners += 1;
        }
    }
    for (has, scenario, active, alt) in [
        (false, 0, 0, 0),
        (false, 0, 0, 1),
        (true, 9999, 0, 1),
        (true, 10000, 1, 0),
    ] {
        for pattern in 0..=4 {
            for selected in [0, 187, 188, 244, 314, 315] {
                let mut p = Profile::new(&snr, has, scenario, active, alt, selected, pattern);
                for repeat in [
                    0x20, 0x80, 0x10, 0x40, 0xa0, 0x50, 0x400, 0x800, 0xc00, 0xff0,
                ] {
                    p.controls(true, false, 0, repeat);
                    p.controls(true, false, 0, 0x20); // pending scroll blocks input
                    p.advance(4.0);
                }
                p.controls(false, false, 1 << 24, 0x40);
                p.controls(true, true, 1 << 25, 0x20);
                for (x, y) in [
                    (149.999, 164.0),
                    (150.0, 162.0),
                    (150.0, 164.0),
                    (949.999, 250.0),
                    (950.0, 250.0),
                    (970.0, 255.0),
                    (1769.999, 979.999),
                    (1770.0, 979.999),
                    (1800.0, 29.999),
                    (1764.0, 30.0),
                    (1848.0, 114.0),
                ] {
                    p.contact(x, y);
                }
                p.contact(200.0, 200.0);
                p.press();
                p.advance(4.0);
                p.drag(200.0, 100.0);
                p.inertia(-10.0);
                p.advance(0.5);
                p.advance(3.0);
                p.drag(100.0, 500.0);
                p.inertia(0.05);
                p.advance(1.0);
                p.contact(1800.0, 60.0);
                p.press();
                p.advance(2.0);
                p.release();
                p.advance(2.0);
                p.controls(true, false, 1 << 24, 0);
                if p.model.pending.is_some() {
                    p.callback();
                    p.advance(2.0);
                    p.callback();
                    p.advance(6.0);
                    p.callback();
                    assert_eq!(p.seeds, 1);
                    assert_eq!(p.dates, u32::from(!has));
                } else {
                    p.advance(8.0);
                    p.controls(true, false, (1 << 24) | (1 << 25), 0);
                    p.callback();
                    p.advance(8.0);
                    p.callback();
                    assert_eq!(p.seeds, 0);
                    assert_eq!(p.dates, 0);
                }
                p.finish(&mut input, &mut expected);
                owners += 1;
            }
        }
    }
    // Back-button contact uses decide;controller B uses cancel. Their scene
    // payloads and flash gates must otherwise agree,including Origin return9.
    for (has, scenario, active, alt) in [
        (false, 0, 0, 0),
        (false, 0, 0, 1),
        (true, 100, 0, 1),
        (true, 11000, 1, 0),
    ] {
        let mut p = Profile::new(&snr, has, scenario, active, alt, -1, 3);
        p.contact(1800.0, 60.0);
        p.press();
        p.advance(2.0);
        p.confirm();
        p.callback();
        p.advance(8.0);
        p.callback();
        p.finish(&mut input, &mut expected);
        owners += 1;
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIPS_MODEL").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_SNR").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TIPS_MODEL_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let actual = text.lines().collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        let a = a.split_whitespace().collect::<Vec<_>>();
        let e = e.split_whitespace().collect::<Vec<_>>();
        assert_eq!(a.len(), e.len(), "TIPS native row{i} token count");
        for (j, (a, e)) in a.iter().zip(&e).enumerate() {
            if *e != "*" {
                assert_eq!(a, e, "TIPS native row{i} token{j}");
            }
        }
    }
    eprintln!(
        "TIPS model:{owners} original constructors/{} exact observations;315 original-index descriptors,normal/alternate filtering,unlock/completion/current bits,all initial selections,controller priorities/clamped scrolling/contact/drag/inertia/flash gates and full story/return factory payloads;fresh date/dormant scalar initialization explicitly supplied",
        actual.len()
    );
}
