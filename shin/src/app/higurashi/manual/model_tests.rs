use super::*;
use crate::adv::higurashi::mode_stack::{Mode, ModePush, ModeStack};
use std::collections::VecDeque;
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/manual-model-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/manual-model-capture/output.txt"
));
struct Native {
    op: char,
    result: i32,
    header: Vec<u32>,
    cues: Vec<u32>,
    returns: Vec<u32>,
    tasks: Vec<u32>,
}
fn native(line: &str) -> Native {
    let mut s = line.split_whitespace();
    assert_eq!(s.next(), Some("M"));
    let op = s.next().unwrap().chars().next().unwrap();
    let result = s.next().unwrap().parse().unwrap();
    let mut w = s
        .map(|v| v.parse::<u32>().unwrap())
        .collect::<VecDeque<_>>();
    let header = w.drain(..11).collect();
    let cues = w.drain(..4).collect();
    let n = w.pop_front().unwrap() as usize;
    let returns = w.drain(..n).collect();
    let n = w.pop_front().unwrap() as usize;
    let tasks = w.drain(..n * 2).collect();
    assert_eq!(w.len(), 7, "Native checked resource service fields");
    Native {
        op,
        result,
        header,
        cues,
        returns,
        tasks,
    }
}
#[derive(Default)]
struct Callbacks {
    events: Vec<Action>,
}
impl Services for Callbacks {
    fn input(&mut self, model: &mut Model, permission: bool) {
        model.poll_controls(permission, false, Buttons::default());
    }
    fn event(&mut self, a: Action) -> Result<()> {
        self.events.push(a);
        Ok(())
    }
}
struct Poll<'a> {
    model: &'a mut Model,
    status: u32,
    services: &'a mut Callbacks,
}
impl Mode<Poll<'_>> for Task {
    fn completion(&mut self) -> u32 {
        self.done.into()
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        p: &mut Poll<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        for task in self.poll_native(p.model, p.status, delta, permission, p.services)? {
            pushes.push(task);
        }
        Ok(0)
    }
}
struct Fixture {
    model: Model,
    scope: ModeStack<Task>,
    services: Callbacks,
    count: usize,
    status: u32,
}
impl Fixture {
    fn new(count: usize) -> Self {
        let mut model = Model::new();
        let mut scope = ModeStack::default();
        for t in model.take_pending_tasks() {
            scope.push(t);
        }
        Self {
            model,
            scope,
            services: Callbacks::default(),
            count,
            status: 0,
        }
    }
    fn collect(&mut self) {
        for t in self.model.take_pending_tasks() {
            self.scope.push(t);
        }
        self.services.events.extend(self.model.take_actions());
    }
    fn frame(&mut self, permission: bool, delta: f32) {
        self.scope
            .poll(
                Ticks::from_f32(delta),
                permission.into(),
                &mut Poll {
                    model: &mut self.model,
                    status: self.status,
                    services: &mut self.services,
                },
            )
            .unwrap();
        self.model.advance_clocks(Ticks::from_f32(delta));
    }
    fn compare(&mut self, expected: Native, result: i32, index: usize) {
        let m = &self.model;
        let header = vec![
            m.complete.into(),
            m.progress.to_bits(),
            m.contact as u32,
            m.swipe.into(),
            m.closing.into(),
            m.count as u32,
            m.selected as u32,
            m.scroll.value().to_bits(),
            u32::from(!m.scroll.is_idle()),
            m.close_flash.value().to_bits(),
            u32::from(!m.close_flash.is_idle()),
        ];
        assert_eq!(
            header, expected.header,
            "Manual native observation{index} {}",
            expected.op
        );
        assert_eq!(result, expected.result, "Manual native result{index}");
        let mut cues = vec![0; 4];
        let mut returns = vec![];
        for a in std::mem::take(&mut self.services.events) {
            match a {
                Action::DisableInput => cues[0] += 1,
                Action::Cue("move") => cues[1] += 1,
                Action::Cue("decide") => cues[2] += 1,
                Action::Cue("cancel") => cues[3] += 1,
                Action::ReturnSource(stage) => returns.push(stage),
                _ => panic!("Manual native cue"),
            }
        }
        assert_eq!(cues, expected.cues, "Manual native cues{index}");
        assert_eq!(returns, expected.returns, "Manual native returns{index}");
        let tasks = self
            .scope
            .iter()
            .flat_map(|t| {
                [
                    match t.kind {
                        Kind::First => 0,
                        Kind::Open => 1,
                        Kind::Input => 2,
                        Kind::CloseWait => 3,
                        Kind::Close => 4,
                    },
                    t.done.into(),
                ]
            })
            .collect::<Vec<u32>>();
        assert_eq!(tasks, expected.tasks, "Manual native borrowed scope{index}");
    }
}
#[test]
fn reference_updated_manual_original78_owners826_observations() {
    let mut fixture = None;
    let mut output = OUTPUT.lines();
    let mut observations = 0;
    let mut owners = 0;
    for input in INPUT.lines() {
        let mut words = input.split_whitespace();
        let op = words.next().unwrap().chars().next().unwrap();
        if op == 'X' {
            assert_eq!(output.next(), Some("X"));
            fixture = None;
            continue;
        }
        if op == 'N' {
            fixture = Some(Fixture::new(words.next().unwrap().parse().unwrap()));
            owners += 1;
        }
        let f = fixture.as_mut().unwrap();
        let mut result = -1;
        fn number(words: &mut std::str::SplitWhitespace<'_>) -> f32 {
            words.next().unwrap().parse().unwrap()
        }
        match op {
            'N' => {}
            'R' => {
                f.model.publish_resource(f.count).unwrap();
                f.status = 2;
                result = 1;
            }
            'F' => {
                let permission = number(&mut words) != 0.;
                let delta = number(&mut words);
                f.frame(permission, delta);
                result = 0;
            }
            'U' => {
                let permission = words.next().unwrap() == "1";
                let captured = words.next().unwrap() == "1";
                let pressed = u32::from_str_radix(words.next().unwrap(), 16).unwrap();
                let repeat = u32::from_str_radix(words.next().unwrap(), 16).unwrap();
                result = f
                    .model
                    .poll_controls(
                        permission,
                        captured,
                        Buttons {
                            pressed,
                            repeat,
                            ..Default::default()
                        },
                    )
                    .into();
            }
            'V' => {
                let x = number(&mut words);
                let y = number(&mut words);
                result = f.model.probe_contact(Vec2::new(x, y)).into();
            }
            'P' => f.model.press_contact(),
            'E' => f.model.release_contact(),
            'C' => f.model.cancel_contact(),
            'Z' => f.model.reset_contact(),
            'G' => {
                let x = number(&mut words);
                let y = number(&mut words);
                let x2 = number(&mut words);
                let y2 = number(&mut words);
                f.model.drag_contact(Vec2::new(x, y), Vec2::new(x2, y2));
            }
            'I' => {
                let x = number(&mut words);
                let y = number(&mut words);
                f.model.gesture_contact(Vec2::new(x, y));
            }
            _ => panic!("Manual opcode"),
        }
        f.collect();
        observations += 1;
        f.compare(native(output.next().unwrap()), result, observations);
    }
    assert_eq!((owners, observations), (78, 826));
    assert!(output.next().is_none());
}
#[test]
fn reference_updated_manual_frozen_original_reexecution() {
    let Ok(exe) = std::env::var("SHIN_HIGURASHI_MANUAL_MODEL_CAPTURE") else {
        return;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let font = std::env::var("SHIN_HIGURASHI_REFERENCE_FONT")
        .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg(font)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(INPUT.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, OUTPUT.as_bytes());
}
