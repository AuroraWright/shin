use super::*;
use crate::app::higurashi::kakera_menu::tests::{persistent, source};
use shin_core::format::save::higurashi::GameData;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn data(row: &mut String, g: &RuntimeGameData) {
    write!(
        row,
        " {} {}",
        g.progress.active_index(),
        g.progress.extra_word
    )
    .unwrap();
    for e in &g.progress.entries {
        write!(
            row,
            " {:08x} {:08x} {:08x} {:08x} {} {:016x}",
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.response_data.len(),
            hash(&e.response_data)
        )
        .unwrap();
    }
    let tail = g
        .trailing_values
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    write!(
        row,
        " {} {:016x} {} {:016x}",
        g.trailing_values.len(),
        hash(&tail),
        g.byte_variables.len(),
        hash(&g.byte_variables)
    )
    .unwrap();
}
fn kind(k: Kind) -> (u32, i32) {
    match k {
        Kind::First(p) => (0, p),
        Kind::Input => (1, 0),
        Kind::PageOut(d) => (2, d),
        Kind::PageIn => (3, 0),
        Kind::DecideShake(c) => (4, c as i32),
        Kind::DecideFade(f) => (5, f as i32),
        Kind::ExitShake => (6, 0),
        Kind::Question => (7, 0),
        Kind::ExitFade => (8, 0),
    }
}
#[derive(Default)]
struct Driver {
    captured: bool,
    buttons: Buttons,
    bgm: u32,
    stop: u32,
    cues: [u32; 3],
    random: u32,
    queues: u32,
    questions: u32,
    question: bool,
    answer: Option<(u32, i32)>,
}
impl Services for Driver {
    fn input(&mut self, state: &mut Motion, permission: bool) {
        state.controls(permission, self.captured, self.buttons);
    }
    fn question(&mut self, state: &mut Motion, _: bool, _: Ticks) -> bool {
        if let Some((stage, button)) = self.answer.take() {
            state.exit_answer(stage, button);
            stage == 1
        } else {
            false
        }
    }
    fn next_seed(&mut self) -> u32 {
        self.random += 1;
        0x12345678 + self.random
    }
}
struct Profile {
    state: Motion,
    driver: Driver,
    protocol: String,
    expected: Vec<String>,
    active: usize,
    source: GameData,
    value: usize,
    scenes: usize,
}
impl Profile {
    fn new(scenario: i32, active: usize, selected: i32, value: usize, captured: bool) -> Self {
        let source = source(scenario, active, value, 64, 0);
        let mut p = Self {
            state: Motion::new(
                Model::new(source.clone(), selected, &persistent(value, 0, 2)),
                captured,
            ),
            driver: Driver::default(),
            protocol: format!(
                "N {scenario} {active} {selected} {value} {}\n",
                u8::from(captured)
            ),
            expected: vec![],
            active,
            source,
            value,
            scenes: 1,
        };
        p.snapshot();
        p
    }
    fn actions(&mut self) {
        for action in std::mem::take(&mut self.state.actions) {
            match action {
                Action::PlayBgm(_) => self.driver.bgm += 1,
                Action::StopBgm => self.driver.stop += 1,
                Action::Cue(name) => {
                    self.driver.cues[match name {
                        "move" => 0,
                        "decide" => 1,
                        "cancel" => 2,
                        _ => unreachable!(),
                    }] += 1
                }
                Action::ExitQuestion => {
                    assert!(!self.driver.question);
                    self.driver.question = true;
                    self.driver.questions += 1;
                }
            }
        }
    }
    fn snapshot(&mut self) {
        let m = &self.state;
        let mut row = format!(
            "S {} {} {} {} {} {} {}",
            m.first_phase().unwrap_or(-2),
            u8::from(m.closing),
            m.model.page,
            m.model.selected,
            m.counter,
            m.pointer_kind,
            m.pointer_cell
        );
        for value in m.weights {
            write!(row, " {:08x}", value.to_bits()).unwrap();
        }
        for clock in &m.clocks {
            write!(
                row,
                " {:08x} {}",
                clock.value().to_bits(),
                u8::from(!clock.is_idle())
            )
            .unwrap();
        }
        if m.first_phase().is_some() {
            write!(
                row,
                " {} {:08x} {:08x} {}",
                u8::from(m.overlay.active),
                m.overlay.elapsed.to_bits(),
                m.overlay.duration.to_bits(),
                u8::from(m.overlay.source_present)
            )
            .unwrap();
        } else {
            row.push_str(" 0 00000000 00000000 0");
        }
        let tasks = m.tasks().collect::<Vec<_>>();
        write!(row, " {}", tasks.len()).unwrap();
        for (_, k, done) in &tasks {
            let (k, arg) = kind(*k);
            write!(row, " {k} {} {arg}", u8::from(*done)).unwrap();
        }
        write!(row, " {}", m.borrowed.len()).unwrap();
        for id in &m.borrowed {
            let k = tasks.iter().find(|(i, _, _)| i == id).unwrap().1;
            write!(row, " {}", kind(k).0).unwrap();
        }
        let d = &self.driver;
        write!(
            row,
            " {} {} {} {} {} {} {} {} {}",
            d.bgm,
            d.stop,
            d.cues[0],
            d.cues[1],
            d.cues[2],
            d.random,
            d.queues,
            d.questions,
            u8::from(d.question)
        )
        .unwrap();
        data(&mut row, &m.model.data);
        write!(row, " {}", u8::from(m.request.is_some())).unwrap();
        if let Some(request) = &m.request {
            write!(row, " {}", request.flags).unwrap();
            data(&mut row, &request.data);
        }
        self.expected.push(row);
    }
    fn step(&mut self, status: u32, captured: bool, pressed: u32, repeated: u32, delta: f32) {
        self.protocol.push_str(&format!(
            "U {status} {} {pressed:x} {repeated:x} {delta}\n",
            u8::from(captured)
        ));
        if self.state.tasks().any(|(_, k, d)| k == Kind::Question && d) {
            self.driver.question = false;
        }
        self.driver.buttons = Buttons {
            pressed,
            repeat: repeated,
            ..Default::default()
        };
        self.driver.captured = captured;
        let queued = self.state.request.is_some();
        self.state
            .advance(status, Ticks::from_f32(delta), &mut self.driver);
        if !queued && self.state.request.is_some() {
            self.driver.queues += 1;
        }
        self.actions();
        self.snapshot();
    }
    fn ready(&mut self, delta: f32) {
        let bgm = self.driver.bgm;
        self.step(0, false, 0, 0, 0.0);
        self.step(1, false, 0, 0, delta);
        for _ in 0..100 {
            self.step(2, false, 0, 0, delta);
            if self.state.first_phase().is_none() {
                assert_eq!(self.driver.bgm, bgm + 1);
                return;
            }
        }
        panic!("First failed to retire");
    }
    fn idle(&mut self, frames: usize, delta: f32) {
        for _ in 0..frames {
            self.step(2, false, 0, 0, delta);
        }
    }
    fn point(&mut self, x: f32, y: f32) {
        self.protocol.push_str(&format!("V {x} {y}\n"));
        self.state.probe(glam::vec2(x, y));
        self.snapshot();
    }
    fn contact(&mut self, op: char) {
        self.protocol.push_str(&format!("{op}\n"));
        match op {
            'P' => self.state.press(Vec2::ZERO),
            'R' => self.state.release(Vec2::ZERO),
            'C' => self.state.cancel(),
            'Z' => self.state.reset(),
            _ => unreachable!(),
        };
        self.actions();
        self.snapshot();
    }
    fn gesture(&mut self, x: f32, y: f32) {
        self.protocol.push_str(&format!("G {x} {y}\n"));
        self.state.gesture(glam::vec2(x, y));
        self.actions();
        self.snapshot();
    }
    fn answer(&mut self, stage: u32, button: i32) {
        assert!(self.driver.question);
        self.protocol.push_str(&format!("Q {stage} {button}\n"));
        self.driver.answer = Some((stage, button));
        self.snapshot();
    }
    fn reenter(&mut self, selected: i32, captured: bool) {
        let request = self.state.request.take().unwrap();
        assert_eq!(request.flags, 0);
        let mut saved = shin_core::format::save::higurashi::SaveData::default();
        saved.persist_data = persistent(self.value, 0, 2);
        let bindings =
            crate::app::higurashi::gameplay::bindings::Bindings::new_runtime(&saved, &request.data)
                .unwrap();
        let copied = bindings.runtime_game_data().unwrap();
        assert_eq!(copied, request.data);
        let replaced = bindings.replacement(&saved, &bindings.progress).unwrap();
        assert_eq!(replaced.runtime_game_data().unwrap(), copied);
        self.state = Motion::new(Model::new(copied, selected, &saved.persist_data), captured);
        self.driver.question = false;
        self.driver.answer = None;
        self.scenes += 1;
        self.protocol
            .push_str(&format!("E {selected} {}\n", u8::from(captured)));
        self.snapshot();
    }
    fn extra(&mut self, value: u32) {
        self.state.model.data.progress.extra_word = value;
        self.protocol.push_str(&format!("X {value:x}\n"));
        self.snapshot();
    }
    fn finish(mut self, input: &mut String, expected: &mut Vec<String>) {
        assert_eq!(self.state.model.data.date, self.source.date);
        assert_eq!(
            self.state.model.data.byte_variables,
            self.source.byte_variables
        );
        assert_eq!(
            self.state.model.data.trailing_values,
            self.source.trailing_values
        );
        self.protocol.push_str("D\n");
        self.expected.push(format!(
            "D {}",
            3 + usize::from(
                !self.state.model.data.progress.entries[0]
                    .response_data
                    .is_empty()
            ) + usize::from(
                !self.state.model.data.progress.entries[1]
                    .response_data
                    .is_empty()
            )
        ));
        input.push_str(&self.protocol);
        expected.extend(self.expected);
    }
}

#[test]
#[ignore = "real Kakera First/parent/child stack/Start/page/decide/exit/ADV factory queue/retirement with prepared capture/resource/modal/font/seed/audio;headless,no window/saves"]
fn higurashi_kakera_startup_animated_actions_and_gameplay_requests_match_native() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    let mut record = |p: Profile| {
        owners += p.scenes;
        p.finish(&mut input, &mut expected);
    };
    for scenario in [0, 10000] {
        for active in [0, 1] {
            for captured in [false, true] {
                for delta in [3.0, 12.0, 60.0] {
                    // Pending First and completed/retiring First have distinct owner prefixes.
                    let mut pending = Profile::new(scenario, active, 0, 1, captured);
                    pending.step(0, false, 0, 0, delta);
                    record(pending);
                    let mut p = Profile::new(scenario, active, 0, 1, captured);
                    p.ready(delta);
                    for pressed in [1 << 10, 1 << 11, (1 << 10) | (1 << 11)] {
                        p.step(2, false, pressed, 0, delta);
                        p.idle(12, delta);
                    }
                    p.step(2, true, 1 << 24, 0, delta);
                    p.step(2, false, 0, 0xa0, 0.0);
                    record(p);
                    for value in [0, 1, 2] {
                        let mut p = Profile::new(scenario, active, 0, value, captured);
                        p.ready(delta);
                        p.step(2, false, 1 << 24, 0, delta);
                        p.idle(80, delta);
                        if value == 0 {
                            assert!(p.state.request.is_none());
                            p.step(2, false, (1 << 24) | (1 << 25), 0, delta);
                            p.idle(12, delta);
                        } else {
                            let req = p.state.request.as_ref().unwrap();
                            assert_eq!(
                                req.data.progress.active_entry().scenario_id,
                                if scenario == 0 { 2000 } else { 12000 }
                            );
                            assert_eq!(p.driver.random, 1);
                            assert_eq!(p.driver.queues, 1);
                            assert_eq!(
                                req.data.save_data().primary_entry.as_ref().unwrap(),
                                p.source.primary_entry.as_ref().unwrap_or(&p.source.entry)
                            );
                        }
                        record(p);
                    }
                    if scenario == 0 {
                        for button in [-1, 0, 1] {
                            let mut p = Profile::new(scenario, active, 0, 1, captured);
                            p.ready(delta);
                            p.step(2, false, (1 << 24) | (1 << 25), 0, delta);
                            p.idle(20, delta);
                            assert_eq!(p.driver.questions, 1);
                            p.answer(0, button);
                            p.step(2, false, 0, 0, delta);
                            p.answer(1, button);
                            p.idle(80, delta);
                            assert_eq!(p.state.request.is_some(), button == 0);
                            assert_eq!(p.driver.random, 0);
                            if button == 0 {
                                assert!(
                                    p.state
                                        .request
                                        .as_ref()
                                        .unwrap()
                                        .data
                                        .save_data()
                                        .primary_entry
                                        .is_none()
                                );
                            }
                            record(p);
                        }
                    } else {
                        let mut p = Profile::new(scenario, active, 0, 1, captured);
                        p.ready(delta);
                        p.step(2, false, 1 << 25, 0, delta);
                        p.idle(12, delta);
                        assert_eq!(p.driver.cues, [0; 3]);
                        assert_eq!(p.driver.questions, 0);
                        record(p);
                    }
                }
            }
        }
    }
    for scenario in [0, 10000] {
        for active in [0, 1] {
            let mut p = Profile::new(
                scenario,
                active,
                if scenario == 0 { 86 } else { 49 },
                1,
                true,
            );
            p.ready(3.0);
            for (x, y) in [
                (250.0, 284.0),
                (744.0, 284.0),
                (1240.0, 284.0),
                (744.0, 546.0),
                (1800.0, 60.0),
                (1800.0, 120.0),
            ] {
                p.point(x, y);
                p.contact('P');
                p.contact('C');
                p.contact('Z');
            }
            for (x, y) in [(20.0, 0.0), (200.0, 0.0), (-200.0, 0.0), (0.0, 200.0)] {
                p.gesture(x, y);
                p.idle(12, 3.0);
            }
            p.point(250.0, 284.0);
            p.contact('P');
            p.contact('R');
            p.idle(80, 3.0);
            record(p);
        }
    }
    // Return/re-entry compares the complete native factory,including its dormant record.
    for captured in [false, true] {
        for selected in [0, 84] {
            let mut p = Profile::new(0, 1, selected, 2, captured);
            p.extra(0xdeadbeef);
            p.ready(12.0);
            let secondary = p.state.model.data.progress.entries[1].clone();
            let primary = p.state.model.data.progress.entries[0].clone();
            p.step(2, false, 1 << 25, 0, 12.0);
            p.idle(10, 12.0);
            p.answer(1, 0);
            p.idle(20, 12.0);
            let returned = &p.state.request.as_ref().unwrap().data;
            assert_eq!(returned.progress.active_index(), 0);
            assert_eq!(returned.progress.entries[1], secondary);
            assert_eq!(returned.progress.extra_word, 0xdeadbeef);
            p.reenter(selected, captured);
            p.ready(12.0);
            p.step(2, false, 1 << 24, 0, 12.0);
            p.idle(20, 12.0);
            let fragment = &p.state.request.as_ref().unwrap().data;
            assert_eq!(fragment.progress.active_index(), 1);
            assert_eq!(fragment.progress.entries[0], primary);
            assert_eq!(
                fragment.progress.entries[1].response_data,
                secondary.response_data
            );
            p.reenter(selected, captured);
            p.ready(12.0);
            p.step(2, false, 1 << 25, 0, 12.0);
            p.idle(10, 12.0);
            p.answer(1, 0);
            p.idle(20, 12.0);
            assert_eq!(
                p.state.request.as_ref().unwrap().data.progress.entries[1].response_data,
                secondary.response_data
            );
            record(p);
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_KAKERA_MENU_MOTION").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_KAKERA_MENU_MOTION_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", &output.stdout),
            ("stderr.txt", &output.stderr),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let actual = text.lines().collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "Kakera motion native row {i}");
    }
    eprintln!(
        "Kakera motion:{owners} native owners,{} exact observations;First readiness/capture/BGM/retirement,borrowed task snapshot and reverse permission,per-frame rates/queued clocks,L/R pages,A/B priority/alternate cancellation,locked dismissal,contact/gestures,exit callback branches,both active/dormant records/full source vectors/untruncated extra word,exit/re-entry across fresh Gameplay/browser bindings and one-seed mode0 ADV factories pass",
        expected.len()
    );
}
