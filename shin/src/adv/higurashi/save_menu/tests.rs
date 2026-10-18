use super::*;
use crate::{adv::higurashi::input::Buttons, layer::higurashi_properties::HigurashiTweener};
use shin_core::time::Ticks;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Default)]
struct Service {
    disabled: u32,
    cues: [u32; 4],
    returns: Vec<u32>,
    question: Option<QuestionKind>,
    question_active: bool,
    write_active: bool,
    captured: bool,
    first_retires: u32,
    load_requests: u32,
    queued: u32,
}
impl Services for Service {
    fn input(&mut self, model: &mut Model, permission: bool) {
        model.poll_controls(permission, self.captured, Buttons::default());
    }
    fn event(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::DisableInput => self.disabled += 1,
            Action::Cue(name) => {
                self.cues[match name {
                    "move" => 0,
                    "decide" => 1,
                    "cancel" => 2,
                    "error" => 3,
                    _ => panic!("Unexpected menu cue"),
                }] += 1
            }
            Action::ReturnSource(stage) => self.returns.push(stage),
            Action::OpenQuestion(kind) => {
                self.question = Some(kind);
                self.question_active = true;
            }
            Action::SaveSelected(_) | Action::DeleteSelected(_) => self.write_active = true,
            Action::LoadSelected(_) => self.load_requests += 1,
            Action::QueueAdv { flags } => {
                assert_eq!(flags, 0);
                self.queued += 1;
            }
        }
        Ok(())
    }
    fn retire_first(&mut self) {
        self.first_retires += 1;
    }
}
struct Compare {
    input: String,
    expected: Vec<(String, Option<QuestionKind>)>,
    motion: Option<Motion>,
    service: Service,
    owners: usize,
}
fn tween(out: &mut String, t: &HigurashiTweener) {
    write!(out, " {} {}", t.value().to_bits(), u8::from(t.is_waiting())).unwrap();
}
impl Compare {
    fn new() -> Self {
        Self {
            input: String::new(),
            expected: vec![],
            motion: None,
            service: Service::default(),
            owners: 0,
        }
    }
    fn collect(&mut self) {
        let actions = self.motion.as_mut().unwrap().model.take_actions();
        for action in actions {
            self.service.event(action).unwrap();
        }
    }
    fn snapshot(&mut self, op: char, result: i32) {
        self.collect();
        let m = &self.motion.as_ref().unwrap().model;
        let mut out = format!(
            "M {op} {result} {} {} {} {} {} {} {} {} {} {}",
            m.mode as u32,
            m.selected,
            m.request,
            m.hit,
            m.pointed_row,
            u8::from(m.closing),
            u8::from(m.complete),
            u8::from(m.loaded),
            m.progress.to_bits(),
            m.speed.to_bits()
        );
        tween(&mut out, &m.scroll);
        tween(&mut out, &m.close_flash);
        tween(&mut out, &m.row_flash[m.selected as usize]);
        write!(out, " {}", self.service.disabled).unwrap();
        for cue in self.service.cues {
            write!(out, " {cue}").unwrap();
        }
        write!(out, " {}", self.service.returns.len()).unwrap();
        for stage in self.service.returns.drain(..) {
            write!(out, " {stage}").unwrap();
        }
        self.expected.push((out, self.service.question));
        self.service.disabled = 0;
        self.service.cues = [0; 4];
    }
    fn owner(&mut self, mode: Mode, remembered: u8, append: bool, salt: u32) {
        writeln!(
            self.input,
            "N {} {remembered} {} {salt}",
            mode as u32,
            u8::from(append)
        )
        .unwrap();
        let slots = std::array::from_fn(|index| {
            let i = index as u32 + 1;
            SlotMetadata {
                valid: (i + salt) % 3 != 0,
                scenario_id: (100 + i * 13 + ((i + salt) & 1) * 10000 + salt) as i32,
            }
        });
        self.motion = Some(Motion::new(
            Model::new(mode, remembered, append, slots).unwrap(),
        ));
        self.service = Service::default();
        self.owners += 1;
        self.snapshot('N', -1);
    }
    fn frame(&mut self, permission: bool, delta: f32) {
        writeln!(self.input, "F {} {delta}", u8::from(permission)).unwrap();
        let allowed = permission && !self.service.question_active && !self.service.write_active;
        self.motion
            .as_mut()
            .unwrap()
            .advance(2, true, allowed, Ticks::from_f32(delta), &mut self.service)
            .unwrap();
        self.snapshot('F', 0);
    }
    fn open(&mut self) {
        self.frame(true, 1.0);
        self.frame(true, 10.0);
        self.frame(true, 1.0);
    }
    fn input(&mut self, permission: bool, captured: bool, pressed: u32, repeat: u32) {
        writeln!(
            self.input,
            "U {} {} {pressed:x} {repeat:x}",
            u8::from(permission),
            u8::from(captured)
        )
        .unwrap();
        self.service.captured = captured;
        let result = self.motion.as_mut().unwrap().model.poll_controls(
            permission,
            captured,
            Buttons {
                pressed,
                repeat,
                ..Buttons::default()
            },
        );
        self.snapshot('U', i32::from(result));
    }
    fn contact(&mut self, x: f32, y: f32) {
        writeln!(self.input, "V {x} {y}").unwrap();
        let result = self.motion.as_mut().unwrap().model.contact(x, y);
        self.snapshot('V', i32::from(result));
    }
    fn touch(&mut self, op: char) {
        writeln!(self.input, "{op}").unwrap();
        let m = &mut self.motion.as_mut().unwrap().model;
        match op {
            'P' => m.press_contact(),
            'C' => m.release_contact(),
            'H' => m.long_press_contact(),
            'R' => m.cancel_contact(),
            'Z' => m.reset_contact(),
            _ => unreachable!(),
        };
        self.snapshot(op, -1);
    }
    fn drag(&mut self, old: f32, new: f32) {
        writeln!(self.input, "G 200 {old} 200 {new}").unwrap();
        self.motion.as_mut().unwrap().model.drag_contact(old, new);
        self.snapshot('G', -1);
    }
    fn inertia(&mut self, speed: f32) {
        writeln!(self.input, "I {speed}").unwrap();
        self.motion.as_mut().unwrap().model.inertia(speed);
        self.snapshot('I', -1);
    }
    fn answer(&mut self, stage: u32, answer: i32) {
        writeln!(self.input, "Q {stage} {answer}").unwrap();
        self.motion
            .as_mut()
            .unwrap()
            .model
            .question_callback(stage, answer);
        if stage != 0 {
            self.service.question_active = false;
        }
        self.snapshot('Q', -1);
    }
    fn confirm_gate(&mut self) {
        self.frame(true, 1.0);
        self.frame(true, 5.0);
        self.frame(true, 1.0);
        self.frame(true, 0.0);
    }
    fn retire(&mut self) {
        writeln!(self.input, "X").unwrap();
        self.motion.as_mut().unwrap().retire(&mut self.service);
        assert_eq!(self.service.first_retires, 1);
        self.motion = None;
    }
    fn run(self) {
        let exe = std::env::var("HIGURASHI_SAVE_MENU_MODEL_NATIVE")
            .unwrap_or_else(|_| "/tmp/higurashi-native-save-menu-model".into());
        let font = std::env::var("HIGURASHI_NEWRODIN_FNT")
            .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
        let mut child = Command::new(exe)
            .args(["game-data/higurashi/uncompressed_executable_update", &font])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = self.input.clone();
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        if let Ok(path) = std::env::var("SHIN_HIGURASHI_SAVE_MENU_MODEL_CAPTURE") {
            let p = std::path::Path::new(&path);
            std::fs::create_dir_all(p).unwrap();
            std::fs::write(p.join("native-input.txt"), &self.input).unwrap();
            std::fs::write(p.join("native-output.txt"), &out.stdout).unwrap();
            std::fs::write(p.join("native-stderr.txt"), &out.stderr).unwrap();
        }
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        let native: Vec<_> = stdout
            .lines()
            .filter(|line| line.starts_with("M "))
            .collect();
        assert_eq!(native.len(), self.expected.len());
        for (index, (line, (prefix, question))) in native.iter().zip(&self.expected).enumerate() {
            let words: Vec<_> = line.split_whitespace().collect();
            let returns = words[24].parse::<usize>().unwrap();
            let end = 25 + returns;
            assert_eq!(
                words[..end].join(" "),
                *prefix,
                "Native menu observation {index}: {line}"
            );
            if let Some(kind) = question {
                assert_eq!(words[end + 1].parse::<i32>().unwrap(), kind.selected());
                assert_eq!(
                    words[end + 2].parse::<u8>().unwrap(),
                    u8::from(kind.cancellable())
                );
                let mut at = end + 9;
                let n = words[at].parse::<usize>().unwrap();
                at += 1;
                let prompt: Vec<u8> = words[at..at + n]
                    .iter()
                    .map(|w| w.parse().unwrap())
                    .collect();
                at += n;
                let n = words[at].parse::<usize>().unwrap();
                at += 1;
                let labels: Vec<u8> = words[at..at + n]
                    .iter()
                    .map(|w| w.parse().unwrap())
                    .collect();
                assert_eq!(
                    prompt,
                    kind.prompt().as_bytes(),
                    "Question prompt observation {index}"
                );
                assert_eq!(
                    labels,
                    kind.labels().as_bytes(),
                    "Question labels observation {index}"
                );
            }
        }
        eprintln!(
            "SaveMenu:{} native owners/{} exact constructor/input/contact/clock/task/callback observations; native full transaction data is compared by runtime_saves/manual tests",
            self.owners,
            self.expected.len()
        );
    }
}
#[test]
#[ignore = "original updated SaveMenu constructor/input/contact/poll/First/task/dialog/callback comparison with checked external transports"]
fn higurashi_save_menu_controls_clocks_and_dialog_stages_match_native() {
    let mut c = Compare::new();
    for mode in [Mode::Save, Mode::Load] {
        for salt in [0, 1, 2, 7] {
            for selected in [0, 1, 17, 99] {
                c.owner(mode, selected, true, salt);
                c.open();
                for repeat in [
                    1 << 4,
                    1 << 6,
                    1 << 7,
                    1 << 5,
                    1 << 10,
                    1 << 11,
                    (1 << 4) | (1 << 11),
                ] {
                    c.input(true, false, 0, repeat);
                    for _ in 0..4 {
                        c.frame(true, 0.25);
                    }
                }
                c.input(false, false, 1 << 24, 1 << 6);
                c.input(true, true, 1 << 24, 1 << 6);
                c.input(true, false, (1 << 15) | (1 << 25), 0);
                if c.service.question_active {
                    c.answer(0, 1);
                    c.answer(1, 1);
                    c.frame(true, 0.0);
                }
                for (x, y) in [
                    (150.0, 162.0),
                    (150.01, 162.0),
                    (1770.0, 200.0),
                    (200.0, 161.9),
                    (1900.0, 1000.0),
                ] {
                    c.contact(x, y);
                    c.touch('Z');
                }
                c.contact(200.0, 500.0);
                c.touch('P');
                c.drag(500.0, 480.0);
                c.inertia(1.25);
                c.frame(true, 0.25);
                c.frame(true, 3.5);
                c.touch('R');
                c.touch('Z');
                c.contact(1800.0, 40.0);
                c.touch('P');
                c.touch('R');
                c.confirm_gate();
                c.touch('Z');
                c.input(true, false, 1 << 25, 0);
                c.confirm_gate();
                c.frame(true, 10.0);
                c.frame(true, 0.0);
                assert!(c.motion.as_ref().unwrap().model.complete);
                assert_eq!(c.service.queued, 0);
                c.retire();
            }
        }
    }
    for mode in [Mode::Save, Mode::Load] {
        for append in [false, true] {
            for selected in [0, 1, 99] {
                for salt in [0, 1] {
                    c.owner(mode, selected, append, salt);
                    c.open();
                    c.input(true, false, 1 << 24, 0);
                    c.confirm_gate();
                    if c.service.question_active {
                        c.answer(0, 1);
                        c.answer(1, 1);
                        c.frame(true, 0.0);
                    }
                    c.retire();
                }
            }
        }
    }
    // Original final Load Yes closes and queues ADV after the menu slide reaches0.
    for append in [false, true] {
        c.owner(Mode::Load, 0, append, 1);
        c.open();
        c.input(true, false, 1 << 24, 0);
        c.confirm_gate();
        assert_eq!(c.service.question, Some(QuestionKind::Load));
        c.answer(0, 0);
        assert!(!c.motion.as_ref().unwrap().model.loaded);
        c.answer(1, 0);
        assert!(c.motion.as_ref().unwrap().model.loaded);
        c.frame(true, 10.0);
        assert_eq!(c.service.queued, 1);
        c.retire();
    }
    // Long press tests the old selection, then targets the contact row even
    // when that target is empty. The inverse combination remains disabled.
    for salt in [1, 2] {
        c.owner(Mode::Save, 0, true, salt);
        c.open();
        c.contact(200.0, 368.0);
        c.touch('H');
        if salt == 1 {
            assert_eq!(c.motion.as_ref().unwrap().model.selected, 1);
            assert_eq!(c.service.question, Some(QuestionKind::Delete));
            c.answer(0, 1);
            c.answer(1, 1);
            c.frame(true, 0.0);
        } else {
            assert_eq!(c.motion.as_ref().unwrap().model.selected, 0);
            assert!(!c.service.question_active);
        }
        c.retire();
    }
    c.run();
}

#[test]
fn bottom_gutter_probe_retains_native_row100_but_cannot_access_manual_rows() {
    let mut m = Model::new(
        Mode::Save,
        99,
        true,
        [SlotMetadata {
            valid: true,
            scenario_id: 1,
        }; 100],
    )
    .unwrap();
    assert!(m.contact(200.0, 979.0));
    assert_eq!(m.pointed_row, 100);
    m.press_contact();
    m.release_contact();
    m.long_press_contact();
    assert_eq!(m.selected, 99);
    assert!(m.take_actions().is_empty());
    assert!(Model::new(Mode::Save, 100, true, [SlotMetadata::default(); 100]).is_err());
}
