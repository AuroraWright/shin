use super::*;
use crate::adv::higurashi::{
    autosave::{ProgressEntry, SaveProgress},
    mode_stack::{Mode, ModePush, ModeStack},
};
use shin_core::format::save::higurashi::SaveDate;
use std::collections::VecDeque;
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/scenario-select-model-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/scenario-select-model-capture/output.txt"
));
struct Native {
    op: char,
    result: i32,
    header: Vec<u32>,
    rows: Vec<u32>,
    disabled: u32,
    cues: Vec<u32>,
    events: Vec<u32>,
    tasks: Vec<u32>,
    data: Vec<u32>,
}
fn native(line: &str) -> Native {
    let mut w = line.split_whitespace();
    assert_eq!(w.next(), Some("O"));
    let op = w.next().unwrap().chars().next().unwrap();
    let result = w.next().unwrap().parse().unwrap();
    let mut q = w
        .map(|x| x.parse::<i64>().unwrap() as u32)
        .collect::<VecDeque<_>>();
    let header = q.drain(..14).collect();
    let count = q.pop_front().unwrap() as usize;
    let rows = q.drain(..count * 6).collect();
    let disabled = q.pop_front().unwrap();
    let cues = q.drain(..4).collect();
    let events = q.drain(..9).collect();
    let count = q.pop_front().unwrap() as usize;
    let tasks = q.drain(..count * 10).collect();
    q.drain(..7);
    Native {
        op,
        result,
        header,
        rows,
        disabled,
        cues,
        events,
        tasks,
        data: q.into(),
    }
}
pub(super) fn full_data(active: usize, salt: u32) -> RuntimeGameData {
    let entries = std::array::from_fn(|i| ProgressEntry {
        scenario_id: (0x76540000 + i as u32 * 0x100 + salt * 31) as i32,
        random_seed: 0x76540001 + i as u32 * 0x100 + salt * 31,
        save_position: 0x76540002 + i as u32 * 0x100 + salt * 31,
        replay_target: 0x76540003 + i as u32 * 0x100 + salt * 31,
        response_data: (0..7 + i)
            .map(|j| (j as u32 * 7 + salt + i as u32) as u8)
            .collect(),
    });
    RuntimeGameData::from_progress(
        SaveProgress::new(active, 0x89abcdef, entries).unwrap(),
        SaveDate {
            year: ((salt + 2) | ((salt + 3) << 8)) as u16,
            month: (salt + 4) as u8,
            day: (salt + 5) as u8,
            hour: (salt + 6) as u8,
            minute: (salt + 7) as u8,
            second: (salt + 8) as u8,
        },
        (0..5).map(|i| (i * 31 + salt) as u16).collect(),
        (0..181).map(|i| (i * 7 + salt) as u8).collect(),
    )
}
pub(super) fn settings(profile: u32) -> SaveData {
    let mut s = SaveData::default();
    s.persist_data.0 = (0..192)
        .map(|i| match profile {
            0 => 0,
            1 => 0xffff,
            2 => {
                if i % 2 == 0 {
                    0x8000
                } else {
                    0
                }
            }
            3 => u16::from((i * 17 + 3) % 5 != 0),
            _ => unreachable!(),
        } as i16)
        .collect();
    s
}
fn data_words(data: &RuntimeGameData) -> Vec<u32> {
    let p = &data.progress;
    let mut w = vec![p.active_index() as u32, p.extra_word];
    for e in &p.entries {
        w.extend([
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.response_data.len() as u32,
        ]);
        w.extend(e.response_data.iter().copied().map(u32::from));
    }
    w.push(data.trailing_values.len() as u32);
    w.extend(data.trailing_values.iter().copied().map(u32::from));
    w.push(data.byte_variables.len() as u32);
    w.extend(data.byte_variables.iter().copied().map(u32::from));
    w
}
#[derive(Default)]
struct Platform {
    seed: u32,
    seeds: u32,
    pressed: u32,
    repeat: u32,
    captured: bool,
    disabled: u32,
    cues: [u32; 4],
    queue: Vec<SceneRequest>,
    plays: u32,
    stop30: u32,
    stop60: u32,
    bgm: i32,
    question_count: u32,
    question_default: u32,
    question_cancel: u32,
    question_live: bool,
    new_question: bool,
    pending: Option<(u32, i32)>,
    last_stage: u32,
    last_answer: i32,
}
impl Services for Platform {
    fn next_seed(&mut self) -> u32 {
        self.seeds += 1;
        self.seed
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let b = Buttons {
            pressed: self.pressed,
            repeat: self.repeat,
            ..Default::default()
        };
        let captured = self.captured;
        model.controls(permission, captured, b, &mut || self.next_seed());
        Ok(())
    }
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::DisableInput => self.disabled += 1,
            Action::Cue(name) => {
                self.cues[match name {
                    "move" => 0,
                    "decide" => 1,
                    "cancel" => 2,
                    "error" => 3,
                    _ => panic!("Native cue"),
                }] += 1
            }
            Action::PlayBgm { id, fade, loops } => {
                assert_eq!((fade, loops), (60, 0));
                self.plays += 1;
                self.bgm = id;
            }
            Action::StopBgm(30) => self.stop30 += 1,
            Action::StopBgm(60) => self.stop60 += 1,
            Action::StopBgm(n) => panic!("Native fade{n}"),
            Action::Request(r) => self.queue.push(r),
            Action::ExitQuestion => {
                assert!(!self.question_live);
                self.question_live = true;
                self.new_question = true;
                self.question_count += 1;
                self.question_default = 1;
                self.question_cancel = 1;
            }
        }
        Ok(())
    }
}
enum Child {
    Private(Task),
    Question { done: bool },
}
struct Poll<'a> {
    model: &'a mut Model,
    status: u32,
    platform: &'a mut Platform,
}
impl Mode<Poll<'_>> for Child {
    fn completion(&mut self) -> u32 {
        match self {
            Self::Private(t) => t.done.into(),
            Self::Question { done } => u32::from(*done),
        }
    }
    fn retire(&mut self, p: &mut Poll<'_>) {
        if matches!(self, Self::Question { .. }) {
            p.platform.question_live = false;
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        p: &mut Poll<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Private(t) => {
                for child in t.poll_native(p.model, p.status, delta, permission, p.platform)? {
                    pushes.push(Self::Private(child));
                }
            }
            Self::Question { done } => {
                if let Some((stage, answer)) = p.platform.pending.take() {
                    p.model.question_callback(stage, answer);
                    if stage != 0 {
                        *done = true;
                    }
                    for child in p.model.take_pending_tasks() {
                        pushes.push(Self::Private(child));
                    }
                }
            }
        }
        if p.platform.new_question {
            p.platform.new_question = false;
            pushes.push(Self::Question { done: false });
        }
        Ok(0)
    }
}
struct Fixture {
    model: Model,
    scope: ModeStack<Child>,
    platform: Platform,
    status: u32,
    initial_date: SaveDate,
}
impl Fixture {
    fn new(
        kind: u32,
        selected: i32,
        append: bool,
        profile: u32,
        seed: u32,
        salt: u32,
        active: usize,
        captured: bool,
    ) -> Self {
        let data = full_data(active, salt);
        let initial_date = data.date;
        let mut model = Model::new(
            kind,
            selected,
            append,
            &data,
            &settings(profile),
            Some(221),
            captured,
        )
        .unwrap();
        let mut scope = ModeStack::default();
        for t in model.take_pending_tasks() {
            scope.push(Child::Private(t));
        }
        Self {
            model,
            scope,
            platform: Platform {
                seed,
                bgm: -1,
                ..Default::default()
            },
            status: 0,
            initial_date,
        }
    }
    fn collect(&mut self) {
        for a in self.model.take_actions() {
            self.platform.event(a).unwrap();
        }
        for t in self.model.take_pending_tasks() {
            self.scope.push(Child::Private(t));
        }
    }
    fn frame(&mut self, delta: f32) {
        self.scope
            .poll(
                Ticks::from_f32(delta),
                1,
                &mut Poll {
                    model: &mut self.model,
                    status: self.status,
                    platform: &mut self.platform,
                },
            )
            .unwrap();
        self.model.advance_clocks(Ticks::from_f32(delta));
    }
    fn compare(&mut self, n: Native, result: i32, index: usize, question: &str) {
        let m = &self.model;
        assert_eq!(
            m.data.date, self.initial_date,
            "Native source datecopy assertedbyC; desktop datepreserved"
        );
        fn clock(c: &HigurashiTweener) -> [u32; 2] {
            [c.value().to_bits(), u32::from(!c.is_idle())]
        }
        let mut h = vec![
            m.append.into(),
            m.kind,
            m.bgm as u32,
            m.count as u32,
            m.mask,
            m.base_y().to_bits(),
            m.selected as u32,
            m.closing.into(),
            m.contact as u32,
            m.hover as u32,
        ];
        h.extend(clock(&m.close_flash));
        h.extend(clock(&m.blackout));
        assert_eq!(
            h, n.header,
            "ScenarioSelect observation{index}/{} header",
            n.op
        );
        assert_eq!(result, n.result, "ScenarioSelect result{index}");
        let rows = (0..m.count)
            .flat_map(|i| {
                [
                    clock(&m.opacity[i]),
                    clock(&m.highlight[i]),
                    clock(&m.hot[i]),
                ]
                .concat()
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, n.rows, "ScenarioSelect clockwords{index}");
        assert_eq!(
            self.platform.disabled, n.disabled,
            "Native deniedInput{index}"
        );
        assert_eq!(
            self.platform.cues.as_slice(),
            n.cues,
            "Native literalcues{index}"
        );
        self.platform.disabled = 0;
        self.platform.cues = [0; 4];
        let (kind, a0, a1) = match self.platform.queue.last() {
            None => (0, 0, 0),
            Some(SceneRequest::Adv { flags, .. }) => (1, *flags, 0),
            Some(SceneRequest::Title { selected, mode }) => (2, *selected as u32, *mode as u32),
            Some(SceneRequest::Tips {
                selected,
                alternate,
            }) => (3, *selected as u32, u32::from(*alternate)),
        };
        assert_eq!(
            vec![
                self.platform.seeds,
                self.platform.queue.len() as u32,
                kind,
                a0,
                a1,
                self.platform.plays,
                self.platform.stop30,
                self.platform.stop60,
                self.platform.bgm as u32
            ],
            n.events,
            "Native clock/scene/musicorder{index}"
        );
        let tasks = self
            .scope
            .iter()
            .flat_map(|child| match child {
                Child::Question { done } => vec![0x1c74a0, (*done).into(), 0, 0, 0, 0, 0, 0, 0, 0],
                Child::Private(t) => {
                    let (table, arg) = match t.kind {
                        Kind::First(phase) => (0x1c71a8, phase as u32),
                        Kind::Entrance => (0x1c7230, 0),
                        Kind::Input => (0x1c72a0, 0),
                        Kind::DecideWait(row) => (0x1c7330, row as u32),
                        Kind::AdvFade(id) => (0x1c73a0, id as u32),
                        Kind::ExitWait => (0x1c7430, 0),
                        Kind::ExitOpacity => (0x1c7510, 0),
                        Kind::ReturnFade => (0x1c7580, 0),
                    };
                    let mut w = vec![table, t.done.into(), arg];
                    if let Some(c) = &t.capture {
                        w.extend([
                            c.active.into(),
                            c.elapsed.to_bits(),
                            c.duration.to_bits(),
                            c.source_present.into(),
                            c.mask_present.into(),
                            c.softness.to_bits(),
                            c.flags,
                        ]);
                    } else {
                        w.extend([0; 7]);
                    }
                    w
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(tasks, n.tasks, "Native mixedsnapshotmembership{index}");
        let mut words = data_words(&m.data);
        if let Some(SceneRequest::Adv { data, flags }) = self.platform.queue.last() {
            assert_eq!(*flags, 0);
            assert_eq!(data.date, self.initial_date);
            words.extend(data_words(data));
        }
        assert_eq!(
            words, n.data,
            "Native complete bothrecords/vectorpayload{index}"
        );
        let mut w = question.split_whitespace();
        assert_eq!(w.next(), Some("Q"));
        let mut q = w
            .map(|x| x.parse::<i64>().unwrap() as u32)
            .collect::<VecDeque<_>>();
        let head = q.drain(..7).collect::<Vec<_>>();
        assert_eq!(
            head,
            vec![
                self.platform.question_live.into(),
                self.platform.question_count,
                self.platform.question_default,
                self.platform.question_cancel,
                self.platform.pending.is_some().into(),
                self.platform.last_stage,
                self.platform.last_answer as u32
            ],
            "Native Questionstate{index}"
        );
        let length = q.pop_front().unwrap() as usize;
        let prompt = q.drain(..length).map(|w| w as u8).collect::<Vec<_>>();
        let length = q.pop_front().unwrap() as usize;
        let labels = q.drain(..length).map(|w| w as u8).collect::<Vec<_>>();
        assert!(q.is_empty());
        if self.platform.question_count != 0 {
            assert_eq!(prompt, EXIT_PROMPT.as_bytes());
            assert_eq!(labels, EXIT_LABELS.join("|").as_bytes());
        } else {
            assert!(prompt.is_empty() && labels.is_empty());
        }
    }
}
#[test]
fn reference_updated_scenario_select_original457_owners3324_observations() {
    let mut fixture = None;
    let mut output = OUTPUT.lines();
    let mut owners = 0;
    let mut observations = 0;
    for input in INPUT.lines() {
        let mut w = input.split_whitespace();
        let op = w.next().unwrap().chars().next().unwrap();
        fn u(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            w.next().unwrap().parse().unwrap()
        }
        fn hex(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            u32::from_str_radix(w.next().unwrap(), 16).unwrap()
        }
        if op == 'X' {
            assert_eq!(output.next(), Some("X"));
            fixture = None;
            continue;
        }
        if op == 'N' || op == 'I' {
            let kind = u(&mut w);
            let selected = w.next().unwrap().parse().unwrap();
            let append = u(&mut w) != 0;
            let profile = u(&mut w);
            let seed = hex(&mut w);
            let salt = u(&mut w);
            let active = u(&mut w) as usize;
            let captured = hex(&mut w) != 0;
            fixture = Some(Fixture::new(
                kind, selected, append, profile, seed, salt, active, captured,
            ));
            owners += 1;
        }
        let f = fixture.as_mut().unwrap();
        let mut result = -1;
        match op {
            'N' | 'I' => {}
            'R' => {
                f.model.publish_resources();
                f.status = 2;
                result = 1;
            }
            'F' => {
                let delta = w.next().unwrap().parse().unwrap();
                f.platform.pressed = hex(&mut w);
                f.platform.repeat = hex(&mut w);
                f.platform.captured = u(&mut w) != 0;
                f.frame(delta);
                result = 0;
            }
            'U' => {
                let permission = u(&mut w) != 0;
                let captured = u(&mut w) != 0;
                let pressed = hex(&mut w);
                let repeat = hex(&mut w);
                result = f
                    .model
                    .controls(
                        permission,
                        captured,
                        Buttons {
                            pressed,
                            repeat,
                            ..Default::default()
                        },
                        &mut || f.platform.next_seed(),
                    )
                    .into();
            }
            'Q' => {
                let stage = u(&mut w);
                let answer = w.next().unwrap().parse().unwrap();
                f.platform.last_stage = stage;
                f.platform.last_answer = answer;
                f.platform.pending = Some((stage, answer));
            }
            'V' => {
                let x = w.next().unwrap().parse().unwrap();
                let y = w.next().unwrap().parse().unwrap();
                result = f.model.probe_contact(Vec2::new(x, y)).into();
            }
            'P' => f.model.press_contact(),
            'E' => f.model.release_contact(&mut || f.platform.next_seed()),
            'C' => f.model.cancel_contact(),
            'Z' => f.model.reset_contact(),
            _ => panic!("Native opcode"),
        }
        f.collect();
        observations += 1;
        f.compare(
            native(output.next().unwrap()),
            result,
            observations,
            output.next().unwrap(),
        );
    }
    assert_eq!((owners, observations), (457, 3324));
    assert!(output.next().is_none());
}
#[test]
#[ignore = "requires the original native ScenarioSelect model runner, updated executable, font and scenario; CPU only"]
fn reference_updated_scenario_select_frozen_original_reexecution() {
    let exe = std::env::var("SHIN_HIGURASHI_SCENARIO_SELECT_MODEL_CAPTURE").unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let font = std::env::var("SHIN_HIGURASHI_REFERENCE_FONT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    let snr = std::env::var("SHIN_HIGURASHI_REFERENCE_SNR")
        .unwrap_or_else(|_| "/tmp/higurashi-scenario-patch.rom.snr".into());
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg(font)
        .arg(snr)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(INPUT.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, OUTPUT.as_bytes());
}
