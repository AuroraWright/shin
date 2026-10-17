use super::*;
use crate::adv::higurashi::mode_stack::{Mode as OwnedMode, ModePush, ModeStack};
use shin_core::{
    format::{rom::RomReader, scenario::higurashi::HigurashiScenario},
    primitives::stateless_reader::StatelessFile,
};
use std::{collections::VecDeque, io::Read, path::PathBuf};

struct Native {
    op: char,
    result: i32,
    header: Vec<u32>,
    available: Vec<u32>,
    nodes: Vec<Vec<u32>>,
    groups: Vec<Vec<u32>>,
    flashes: Vec<u32>,
    scroll: Vec<u32>,
    disabled: u32,
    cues: Vec<u32>,
    returns: Vec<u32>,
    question: Vec<u32>,
    scene: Vec<u32>,
    texts: Vec<String>,
}
fn native(line: &str) -> Native {
    let mut s = line.split_whitespace();
    assert_eq!(s.next(), Some("M"));
    let op = s.next().unwrap().chars().next().unwrap();
    let result = s.next().unwrap().parse().unwrap();
    let mut w = s
        .map(|s| s.parse::<u32>().unwrap())
        .collect::<VecDeque<_>>();
    let mut take = |n: usize| w.drain(..n).collect::<Vec<_>>();
    let header = take(14);
    let n = take(1)[0] as usize;
    let available = take(n);
    let n = take(1)[0];
    let nodes = (0..n).map(|_| take(7)).collect();
    let groups = (0..5).map(|_| take(13)).collect();
    let flashes = take(6);
    let scroll = take(4);
    let disabled = take(1)[0];
    let cues = take(4);
    let n = take(1)[0] as usize;
    let returns = take(n);
    let question = take(3);
    let scene = take(7);
    let texts = (0..4)
        .map(|_| {
            let n = take(1)[0] as usize;
            String::from_utf8(take(n).into_iter().map(|w| w as u8).collect()).unwrap()
        })
        .collect();
    drop(take);
    assert!(w.is_empty());
    Native {
        op,
        result,
        header,
        available,
        nodes,
        groups,
        flashes,
        scroll,
        disabled,
        cues,
        returns,
        question,
        scene,
        texts,
    }
}
enum Mode {
    Private(Task),
    Question { index: i32, done: bool },
}
#[derive(Default)]
struct Events {
    input: Input,
    captured: bool,
    events: Vec<Action>,
    seeds: u32,
}
impl Services for Events {
    fn input(&mut self, m: &mut Model, permission: bool) {
        m.poll_controls(permission, self.captured, self.input);
    }
    fn event(&mut self, e: Action) -> Result<()> {
        self.events.push(e);
        Ok(())
    }
    fn next_seed(&mut self) -> u32 {
        let v = 0x12345678 + self.seeds;
        self.seeds += 1;
        v
    }
}
struct Poll<'a> {
    model: &'a mut Model,
    status: u32,
    ready: &'a mut bool,
    events: &'a mut Events,
}
impl OwnedMode<Poll<'_>> for Mode {
    fn completion(&mut self) -> u32 {
        u32::from(match self {
            Self::Private(t) => t.done,
            Self::Question { done, .. } => *done,
        })
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        p: &mut Poll<'_>,
        push: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        if let Self::Private(t) = self {
            let at = p.events.events.len();
            if t.kind == Kind::First && p.status == 2 {
                *p.ready = true;
            }
            for t in t.poll_native(p.model, p.status, delta, permission, p.events)? {
                push.push(Self::Private(t));
            }
            for event in &p.events.events[at..] {
                if let Action::Question { index, .. } = event {
                    push.push(Self::Question {
                        index: *index,
                        done: false,
                    });
                }
            }
        }
        Ok(0)
    }
}
struct Fixture {
    m: Model,
    scope: ModeStack<Mode>,
    status: u32,
    ready: bool,
    scroll: Scrollbar,
    events: Events,
    observed: Vec<Action>,
    question: Vec<u32>,
    prompt: String,
    labels: String,
}
impl Fixture {
    fn new(m: Model, status: u32) -> Self {
        let mut scope = ModeStack::default();
        scope.push(Mode::Private(Task {
            kind: Kind::First,
            done: false,
        }));
        Self {
            m,
            scope,
            status,
            ready: false,
            scroll: Scrollbar::default(),
            events: Events::default(),
            observed: vec![],
            question: vec![0; 3],
            prompt: String::new(),
            labels: String::new(),
        }
    }
    fn absorb(&mut self) {
        for a in self.events.events.drain(..) {
            if let Action::Question { prompt, .. } = &a {
                self.question = vec![1, 1, 1];
                self.prompt = prompt.clone();
                self.labels = "はい|いいえ".into();
            }
            self.observed.push(a);
        }
        self.question[0] = u32::from(
            self.scope
                .iter()
                .any(|m| matches!(m, Mode::Question { .. })),
        );
    }
    fn immediate(&mut self) {
        for t in self.m.take_pending_tasks() {
            self.scope.push(Mode::Private(t));
        }
        for a in self.m.take_actions() {
            if let Action::Question { index, .. } = &a {
                self.scope.push(Mode::Question {
                    index: *index,
                    done: false,
                });
            }
            self.events.events.push(a);
        }
        self.absorb();
    }
    fn frame(&mut self, delta: f32, permission: u32) {
        self.scope
            .poll(
                Ticks::from_f32(delta),
                permission,
                &mut Poll {
                    model: &mut self.m,
                    status: self.status,
                    ready: &mut self.ready,
                    events: &mut self.events,
                },
            )
            .unwrap();
        self.m
            .advance_clocks(Ticks::from_f32(delta), self.ready, &mut self.scroll);
        self.absorb();
    }
    fn response(&mut self, stage: u32, response: i32) {
        let (index, done) = self
            .scope
            .iter_mut()
            .find_map(|m| {
                if let Mode::Question { index, done } = m {
                    Some((*index, done))
                } else {
                    None
                }
            })
            .expect("Original retained Question");
        self.m.question_response(index, stage, response);
        if stage == 1 {
            *done = true;
        }
        self.immediate();
    }
    fn check(&self, n: Native, profile: usize, row: usize) {
        let m = &self.m;
        let ctx = format!("profile{profile} row{row} op{}", n.op);
        assert_eq!(
            n.header,
            vec![
                u32::from(m.complete),
                m.initial_group as u32,
                m.progress.to_bits(),
                m.choice as u32,
                m.current_group as u32,
                m.available_index as u32,
                u32::from(m.title),
                m.frame,
                m.tooltip as u32,
                u32::from(m.closing),
                m.speed.to_bits(),
                m.target as u32,
                m.pointed as u32,
                m.pressed as u32
            ],
            "{ctx} header"
        );
        assert_eq!(
            n.available,
            m.available.iter().map(|v| *v as u32).collect::<Vec<_>>(),
            "{ctx} available"
        );
        assert_eq!(
            n.nodes,
            m.nodes
                .iter()
                .map(|n| vec![
                    n.group as u32,
                    n.index as u32,
                    n.x as u32,
                    n.y as u32,
                    n.source_id,
                    u32::from(n.current),
                    n.record as u32
                ])
                .collect::<Vec<_>>(),
            "{ctx} nodes"
        );
        for (i, (g, original)) in m.groups.iter().zip(n.groups).enumerate() {
            let mut words = vec![
                g.min_x as u32,
                g.max_x as u32,
                g.min_y as u32,
                g.max_y as u32,
                g.origin_x as u32,
                g.origin_y as u32,
                g.selected as u32,
            ];
            for t in [&g.x, &g.y, &g.scroll] {
                words.extend([t.value().to_bits(), u32::from(t.is_waiting())]);
            }
            assert_eq!(words, original, "{ctx} group{i}");
        }
        assert_eq!(
            n.flashes,
            m.flashes
                .iter()
                .flat_map(|t| [t.value().to_bits(), u32::from(t.is_waiting())])
                .collect::<Vec<_>>(),
            "{ctx} flashes"
        );
        assert_eq!(
            n.scroll,
            vec![
                self.scroll.position as u32,
                self.scroll.visible as u32,
                self.scroll.first as u32,
                self.scroll.end as u32
            ],
            "{ctx} pre-advance scrollbar"
        );
        let mut cues = vec![0; 4];
        let mut disabled = 0;
        let mut returns = vec![];
        let mut scene = vec![self.events.seeds, 0, 0, 0, 0, 0, 0];
        for a in &self.observed {
            match a {
                Action::Cue(s) => {
                    cues[match *s {
                        "move" => 0,
                        "decide" => 1,
                        "cancel" => 2,
                        "error" => 3,
                        _ => panic!("Unknown original cue"),
                    }] += 1
                }
                Action::DisableInput => disabled += 1,
                Action::ReturnSource(v) => returns.push(*v),
                Action::StartAdv { data, flags } => {
                    assert_eq!(*flags, 0);
                    scene[1] += 1;
                    scene[2] = 1;
                    scene[3] = data.progress.active_index() as u32;
                    scene[4] = data.progress.extra_word;
                    scene[5] = data.progress.active_entry().scenario_id as u32;
                    scene[6] = data.progress.active_entry().random_seed;
                    assert_eq!(data.progress.entries[1].response_data, Vec::<u8>::new());
                    assert!(data.trailing_values.is_empty() && data.byte_variables.is_empty());
                }
                Action::Question { .. } => {}
            }
        }
        assert_eq!(
            (n.disabled, n.cues, n.returns, n.scene),
            (disabled, cues, returns, scene),
            "{ctx} native effects"
        );
        assert_eq!(n.question, self.question, "{ctx} Question");
        assert_eq!(
            n.texts,
            vec![
                m.completion_text.clone(),
                m.tooltip_text.clone(),
                self.prompt.clone(),
                self.labels.clone()
            ],
            "{ctx} text"
        );
    }
}
/// The updated scenario's Chart catalog, or `None` (skipping the test) when
/// the Higurashi game files are not available.
fn catalog() -> Option<Arc<Catalog>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let dir = std::env::var_os("SHIN_HIGURASHI_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("higurashi"));
    if !dir.join("patch.rom").is_file() {
        eprintln!("skipping: Higurashi patch.rom is not available");
        return None;
    }
    let mut r = RomReader::new(
        StatelessFile::new(std::fs::File::open(dir.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let mut b = vec![];
    r.open_file(r.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut b)
        .unwrap();
    Some(Arc::new(
        Catalog::from_scenario(&HigurashiScenario::new(b.into()).unwrap()).unwrap(),
    ))
}
#[test]
fn higurashi_chart_model_matches_actual_authored_native_owners() {
    compare_chart_capture(false);
}

#[test]
fn higurashi_chart_wheel_matches_original_scroll_steps_and_following_state() {
    compare_chart_capture(true);
}

fn compare_chart_capture(wheel: bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../test-fixtures/higurashi/chart-catalog-model-capture");
    let input = std::fs::read_to_string(root.join("input.txt")).unwrap();
    let output = std::fs::read_to_string(root.join("output.txt")).unwrap();
    let mut original = output.lines();
    let Some(catalog) = catalog() else {
        return;
    };
    let mut f = None;
    let mut profiles = 0;
    let mut observations = 0;
    let mut status = 2;
    for (row, line) in input.lines().enumerate() {
        let mut w = line.split_whitespace();
        let op = w.next().unwrap();
        if op == "X" {
            assert_eq!(original.next(), Some("X"));
            f.take();
            continue;
        }
        let mut result = None;
        if op == "N" {
            let group = w.next().unwrap().parse().unwrap();
            let mask = w.next().unwrap().parse().unwrap();
            let n = w.next().unwrap().parse::<usize>().unwrap();
            let bank = (0..15)
                .map(|_| u32::from_str_radix(w.next().unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            let route = (0..n)
                .map(|_| w.next().unwrap().parse().unwrap())
                .collect::<Vec<_>>();
            f = Some(Fixture::new(
                Model::new(catalog.clone(), group, mask, &bank, &route).unwrap(),
                status,
            ));
            profiles += 1;
        } else {
            let f = f.as_mut().unwrap();
            match op {
                "A" => {
                    status = w.next().unwrap().parse().unwrap();
                    f.status = status;
                }
                "F" => {
                    let permission = w.next().unwrap().parse().unwrap();
                    let delta = w.next().unwrap().parse().unwrap();
                    f.events.captured = w.next().unwrap() == "1";
                    let buttons = (0..4)
                        .map(|_| u32::from_str_radix(w.next().unwrap(), 16).unwrap())
                        .collect::<Vec<_>>();
                    f.events.input = Input {
                        buttons: Buttons {
                            held: buttons[0],
                            pressed: buttons[1],
                            repeat: buttons[2],
                            rapid_repeat: buttons[3],
                        },
                        axes: std::array::from_fn(|_| w.next().unwrap().parse().unwrap()),
                        ..Default::default()
                    };
                    if wheel {
                        let axis = i32::from(f.events.input.axes[3]);
                        f.events.input.wheel_scroll = if !(-15..=15).contains(&axis) {
                            ((axis / 16) * 4) as f32 / 32.
                        } else {
                            0.
                        };
                        f.events.input.axes[3] = 0;
                    }
                    f.frame(delta, permission);
                }
                "V" | "P" | "R" => {
                    let p = Vec2::new(
                        w.next().unwrap().parse().unwrap(),
                        w.next().unwrap().parse().unwrap(),
                    );
                    match op {
                        "V" => result = Some(i32::from(f.m.contact(p))),
                        "P" => f.m.press_contact(p),
                        _ => f.m.release_contact(p),
                    }
                    f.immediate();
                }
                "Z" => f.m.reset_contact(),
                "C" => {
                    f.m.cancel_contact();
                    f.immediate();
                }
                "G" => {
                    let a = Vec2::new(
                        w.next().unwrap().parse().unwrap(),
                        w.next().unwrap().parse().unwrap(),
                    );
                    let b = Vec2::new(
                        w.next().unwrap().parse().unwrap(),
                        w.next().unwrap().parse().unwrap(),
                    );
                    f.m.drag_contact(a, b);
                    f.immediate();
                }
                "I" => {
                    let _: f32 = w.next().unwrap().parse().unwrap();
                    f.m.inertia(w.next().unwrap().parse().unwrap());
                }
                "S" => {
                    f.m.scroll_to(w.next().unwrap().parse().unwrap());
                    f.immediate();
                }
                "Q" => f.response(
                    w.next().unwrap().parse().unwrap(),
                    w.next().unwrap().parse().unwrap(),
                ),
                _ => panic!("Unknown original operation"),
            }
        }
        assert!(w.next().is_none());
        let n = native(original.next().unwrap());
        if let Some(result) = result {
            assert_eq!(n.result, result);
        }
        f.as_ref().unwrap().check(n, profiles - 1, row);
        observations += 1;
    }
    assert!(f.is_none() && original.next().is_none());
    assert_eq!((profiles, observations), (49, 757));
    println!(
        "Chart49 authored native owners/757 exact snapshots,full nodes/clocks/contacts/Question/stages/fresh complete ADV payloads"
    );
}

#[test]
fn higurashi_chart_fractional_wheel_preserves_cursor_bounds_and_input_priority() {
    fn fresh(catalog: Arc<Catalog>) -> Model {
        let mut m = Model::new(catalog, 0, 31, &[u32::MAX; 128], &[]).unwrap();
        assert!(m.group().max_y - m.group().min_y > 20);
        let g = m.group_mut();
        g.scroll.fast_forward_to(100.);
        g.x.fast_forward_to((g.min_x * 36) as f32);
        g.y.fast_forward_to((g.min_y * 56) as f32 + 400.);
        m.take_actions();
        m
    }
    let Some(c) = catalog() else {
        return;
    };
    let mut m = fresh(c.clone());
    let cursor = m.group().y.value();
    // A half-pixel trackpad movement survives the byte-axis boundary.
    m.speed = 7.;
    m.poll_controls(
        true,
        false,
        Input {
            wheel_scroll: 1. / 64.,
            ..Default::default()
        },
    );
    assert_eq!(m.group().scroll.value(), 100.5);
    assert_eq!(m.group().y.value(), cursor + 0.5);
    assert_eq!(m.speed, 0.);
    assert!(m.take_actions().is_empty());
    // Wheel displacement sums with genuine stick motion before XY panning.
    m.poll_controls(
        true,
        false,
        Input {
            axes: [0, 10, 0, 127],
            wheel_scroll: -0.125,
            ..Default::default()
        },
    );
    assert_eq!(m.group().scroll.value(), 124.5);
    assert_eq!(m.group().y.value(), cursor + 25.5);
    // Large finite packets reach the bounds without being clamped to a stick.
    m.poll_controls(
        true,
        false,
        Input {
            wheel_scroll: f32::MAX,
            ..Default::default()
        },
    );
    let end = (((m.group().max_y - m.group().min_y) * 56) as f32 - 819.).max(0.);
    assert_eq!(m.group().scroll.value(), end);
    let at_end = m.group().y.value();
    m.speed = 3.;
    m.poll_controls(
        true,
        false,
        Input {
            wheel_scroll: 1.,
            ..Default::default()
        },
    );
    assert_eq!(
        (m.group().scroll.value(), m.group().y.value(), m.speed),
        (end, at_end, 0.)
    );
    m.poll_controls(
        true,
        false,
        Input {
            wheel_scroll: -f32::MAX,
            ..Default::default()
        },
    );
    assert_eq!(m.group().scroll.value(), 0.);
    assert_eq!(m.group().y.value(), at_end - end);
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        m.poll_controls(
            true,
            false,
            Input {
                wheel_scroll: invalid,
                ..Default::default()
            },
        );
        assert_eq!(m.group().scroll.value(), 0.);
    }
    // No buffered motion after capture, denied permission or busy tweens.
    for gate in 0..5 {
        let mut m = fresh(c.clone());
        match gate {
            2 => control(&mut m.group_mut().scroll, 200., 4., 0),
            3 => control(&mut m.group_mut().x, 200., 4., 0),
            4 => control(&mut m.group_mut().y, 800., 4., 0),
            _ => {}
        }
        let before = (m.group().scroll.value(), m.group().y.value());
        m.poll_controls(
            gate != 0,
            gate == 1,
            Input {
                wheel_scroll: 4.,
                ..Default::default()
            },
        );
        assert_eq!(
            (m.group().scroll.value(), m.group().y.value()),
            before,
            "gate{gate}"
        );
    }
    // The native first matching shoulder/confirm/cancel keeps precedence.
    for bit in [10, 11, 24, 25] {
        let mut m = fresh(c.clone());
        m.poll_controls(
            true,
            false,
            Input {
                buttons: Buttons {
                    pressed: 1 << bit,
                    ..Default::default()
                },
                wheel_scroll: 4.,
                ..Default::default()
            },
        );
        assert_eq!(m.groups[0].scroll.value(), 100., "button{bit}");
    }
}
