use super::*;
use crate::adv::{
    higurashi::browser_lifecycle::{JUMP_BUTTONS, JUMP_TEXT},
    input_area::{PointerEvent, PointerKind},
};
use shin_core::format::font::read_font_metrics;
use std::{
    fmt::Write as _,
    io::{BufReader, Write as _},
    process::{Command, Stdio},
    sync::Arc,
};

#[derive(Default)]
struct Service {
    cues: Vec<Cue>,
    callbacks: Vec<Callback>,
}
impl Services for Service {
    fn cue(&mut self, cue: Cue) {
        self.cues.push(cue);
    }
    fn callback(&mut self, callback: Callback) {
        self.callbacks.push(callback);
    }
}
struct Compare {
    input: String,
    expected: Vec<String>,
    model: Option<Question>,
    service: Service,
    frame: u32,
    micros: u64,
    scenarios: usize,
}
fn hex(text: &str) -> String {
    if text.is_empty() {
        return "-".into();
    }
    text.bytes().map(|b| format!("{b:02x}")).collect()
}
fn floats(out: &mut String, data: impl IntoIterator<Item = f32>) {
    for v in data {
        write!(out, " {}", v.to_bits()).unwrap();
    }
}
fn tween(out: &mut String, t: &HigurashiTweener) {
    write!(out, " {} {}", t.value().to_bits(), u8::from(t.is_waiting())).unwrap();
}
impl Compare {
    fn snapshot(&mut self, op: char) {
        let s = &self.model.as_ref().unwrap().state;
        let mut out = format!(
            "Q {op} {} {} {} {}",
            s.phase,
            s.selected,
            u8::from(s.dragging),
            s.pointer
        );
        floats(
            &mut out,
            [
                s.cooldown,
                s.velocity.x,
                s.velocity.y,
                s.origin.x,
                s.origin.y,
                s.outer.x,
                s.outer.y,
                s.inner.x,
                s.inner.y,
            ],
        );
        write!(out, " {}", u8::from(s.vertical)).unwrap();
        tween(&mut out, &s.scale);
        tween(&mut out, &s.opacity);
        write!(out, " {}", s.buttons.len()).unwrap();
        for (i, b) in s.buttons.iter().enumerate() {
            write!(out, " {i}").unwrap();
            floats(&mut out, b.rect.to_array());
            floats(&mut out, b.background.to_array());
            tween(&mut out, &b.focus);
            tween(&mut out, &b.flash);
            floats(
                &mut out,
                [
                    b.rect.x + b.rect.z * 0.5,
                    b.rect.y + b.rect.w * 0.5,
                    0.5,
                    0.5,
                ],
            );
            floats(&mut out, b.fill.to_array());
            floats(&mut out, b.fill.to_array());
            floats(&mut out, b.outline.to_array());
            floats(&mut out, b.text_size.to_array());
        }
        write!(out, " {}", self.service.cues.len()).unwrap();
        for cue in self.service.cues.drain(..) {
            write!(
                out,
                " {}",
                match cue {
                    Cue::Move => 0,
                    Cue::Cancel => 1,
                    Cue::Confirm => 2,
                    Cue::NoSelection => 3,
                }
            )
            .unwrap();
        }
        write!(out, " {}", self.service.callbacks.len()).unwrap();
        for cb in &self.service.callbacks {
            write!(out, " {} {}", cb.stage, cb.button as u32).unwrap();
        }
        self.service.callbacks.clear();
        self.expected.push(out);
    }
    fn begin(
        &mut self,
        font: impl FontMetrics + Clone,
        prompt: &str,
        answers: &[&str],
        selected: i32,
        cancellable: bool,
    ) {
        self.seed(font, prompt, answers, selected, cancellable, false);
    }
    fn seed(
        &mut self,
        font: impl FontMetrics + Clone,
        prompt: &str,
        answers: &[&str],
        selected: i32,
        cancellable: bool,
        predicate: bool,
    ) {
        assert!(self.model.is_none());
        self.scenarios += 1;
        writeln!(
            self.input,
            "N {selected} {} {} {} {}",
            u8::from(cancellable),
            u8::from(predicate),
            hex(prompt),
            hex(&answers.join("|"))
        )
        .unwrap();
        self.model =
            Some(Question::from_text(font, prompt, answers, selected, cancellable).unwrap());
        self.snapshot('N');
    }
    fn tick(
        &mut self,
        permission: u32,
        pressed: u32,
        repeated: u32,
        delta: f32,
        events: &[PointerEvent],
    ) {
        self.tick_predicate(permission, pressed, repeated, delta, events, false);
    }
    fn tick_predicate(
        &mut self,
        permission: u32,
        pressed: u32,
        repeated: u32,
        delta: f32,
        events: &[PointerEvent],
        dismiss: bool,
    ) {
        self.frame += 1;
        self.micros += 16667;
        writeln!(
            self.input,
            "F {} {} {permission} {pressed:x} {repeated:x} {delta} {} {}",
            self.frame,
            self.micros,
            u8::from(dismiss),
            events.len()
        )
        .unwrap();
        for e in events {
            writeln!(
                self.input,
                "{} {} {} {}",
                e.kind as i32, e.id, e.position.x, e.position.y
            )
            .unwrap();
        }
        let result = self.model.as_mut().unwrap().poll(
            Ticks::from_f32(delta),
            permission,
            pressed,
            repeated,
            &PointerFrame {
                number: self.frame,
                time_seconds: self.micros as f64 / 1_000_000.0,
                events,
            },
            dismiss,
            &mut self.service,
        );
        assert_eq!(result, 0);
        self.snapshot('F');
    }
    fn idle(&mut self, count: usize) {
        for _ in 0..count {
            self.tick(1, 0, 0, 1.0, &[]);
        }
    }
    fn finish(&mut self) {
        self.model.take().unwrap();
        self.input.push_str("D\n");
    }
    fn button(&self, index: usize) -> glam::IVec2 {
        let s = &self.model.as_ref().unwrap().state;
        let r = s.buttons[index].rect;
        ((s.origin + vec2(30.0 + r.x + r.z * 0.5, 46.0 + r.y + r.w * 0.5)) / 1.5).as_ivec2()
    }
    fn contact(&mut self, kind: PointerKind, id: i32, position: glam::IVec2) {
        self.tick(1, 0, 0, 1.0, &[PointerEvent { kind, id, position }]);
    }
    fn verify(&self) {
        let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_QUESTION").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let data = self.input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()));
        let output = child.wait_with_output().unwrap();
        let written = writer.join().unwrap();
        let expected = self.expected.join("\n") + "\n";
        if let Ok(directory) = std::env::var("SHIN_HIGURASHI_QUESTION_PACKETS") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            for (name, data) in [
                ("input.txt", self.input.as_bytes()),
                ("rust.txt", expected.as_bytes()),
                ("native.txt", &output.stdout),
                ("native.log", &output.stderr),
            ] {
                std::fs::write(directory.join(name), data).unwrap();
            }
        }
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        written.unwrap();
        let native = String::from_utf8(output.stdout).unwrap();
        assert_eq!(native.lines().count(), self.expected.len());
        for (i, (got, expected)) in native.lines().zip(&self.expected).enumerate() {
            assert_eq!(got, expected, "Question observation {i}");
        }
        println!(
            "Higurashi question:{} scenarios/{} exact native observations; balanced native constructor/contact/tween/callback owners",
            self.scenarios,
            self.expected.len()
        );
    }
}
#[test]
#[ignore = "Requires the original updated executable and newrodin font"]
fn higurashi_question_matches_native_layout_input_animation_and_callbacks() {
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
        ))
        .unwrap(),
    );
    let mut c = Compare {
        input: String::new(),
        expected: Vec::new(),
        model: None,
        service: Service::default(),
        frame: 0,
        micros: 0,
        scenarios: 0,
    };
    let variants = [
        (JUMP_TEXT, JUMP_BUTTONS),
        ("短い確認", ["最初の長い選択肢です", "次"]),
        ("Question@rSecond line", ["A@rB", "C"]),
        ("@bよみ.@<漢字@>を含む確認です。", ["確認する\n\t", "戻る"]),
    ];
    for (prompt, labels) in variants {
        for selected in [-5, 0, 1, 2, i32::MAX] {
            for cancellable in [false, true] {
                c.begin(font.clone(), prompt, &labels, selected, cancellable);
                c.tick(1, 1 << 24, 0xff, 0.0, &[]);
                c.tick(1, 0, 0, 12.0, &[]);
                assert_eq!(c.model.as_ref().unwrap().phase(), 1);
                c.tick(1, 0, 0, 0.0, &[]);
                assert_eq!(c.model.as_ref().unwrap().phase(), 2);
                for permission in [0, 2, 256, 1, 3, u32::MAX] {
                    for repeated in [0x10, 0x20, 0x40, 0x80, 0xf0, 0x90, 0x60] {
                        c.tick(permission, 0, repeated, 0.25, &[]);
                        c.tick(permission, 0xa0, repeated, 0.5, &[]);
                        c.tick(permission, 0x50, repeated, 1.0, &[]);
                    }
                }
                c.tick(1, 1 << 25, 0xf0, 0.0, &[]);
                if !cancellable {
                    assert_eq!(c.model.as_ref().unwrap().phase(), 2);
                    c.tick(1, (1 << 24) | (1 << 25), 0xff, 1.0, &[]);
                }
                c.idle(40);
                assert!(c.model.as_ref().unwrap().complete());
                c.finish();
            }
        }
    }
    // Destruction during every opening/feedback/closing phase; fractional and
    // oversized deltas retain the executable's switch-arm/tail-clock order.
    for delta in [0.0, 0.125, 0.5, 1.0, 3.25, 24.0] {
        for count in [0, 1, 12, 13, 14, 18, 30, 42] {
            c.begin(font.clone(), JUMP_TEXT, &JUMP_BUTTONS, 1, true);
            for frame in 0..count {
                c.tick(1, if frame >= 13 { 1 << 24 } else { 0 }, 0, delta, &[]);
            }
            c.finish();
        }
    }
    for vertical in [false, true] {
        for mode in 0..9 {
            let labels = if vertical {
                ["長い最初の選択肢です", "もう一つの選択肢"]
            } else {
                JUMP_BUTTONS
            };
            c.begin(font.clone(), JUMP_TEXT, &labels, 1, true);
            c.idle(14);
            let point = c.button(0);
            let body = ((c.model.as_ref().unwrap().origin() + Vec2::splat(10.0)) / 1.5).as_ivec2();
            let p = if mode >= 5 { body } else { point };
            c.contact(PointerKind::Down, 7, p);
            match mode {
                0 => {
                    c.contact(PointerKind::Up, 7, p);
                }
                1 => {
                    c.idle(12);
                    c.contact(PointerKind::Up, 7, p);
                }
                2 => {
                    c.idle(12);
                    c.contact(PointerKind::Cancel, 7, p);
                }
                3 => {
                    c.tick(2, 1 << 24, 0, 0.5, &[]);
                    c.contact(PointerKind::Up, 7, p);
                }
                4 => {
                    c.contact(PointerKind::Move, 7, p + glam::ivec2(30, 0));
                    c.contact(PointerKind::Up, 7, p);
                }
                5 | 6 => {
                    for step in 1..12 {
                        c.contact(
                            PointerKind::Move,
                            7,
                            p + glam::ivec2(step * if mode == 5 { -55 } else { 55 }, step * 25),
                        );
                    }
                    c.contact(PointerKind::Up, 7, p);
                }
                7 => {
                    c.contact(PointerKind::Down, 8, point);
                    c.contact(PointerKind::Move, 7, p + glam::ivec2(55, -25));
                    c.contact(PointerKind::Up, 7, p);
                    c.contact(PointerKind::Up, 8, point);
                }
                8 => {
                    c.contact(PointerKind::Cancel, 7, p);
                }
                _ => unreachable!(),
            }
            for delta in [0.0, 0.5, 1.0, 3.25] {
                c.tick(1, 0, 0, delta, &[]);
            }
            c.idle(40);
            c.finish();
        }
    }
    // No labels and a single label use the same native constructor/poll body.
    for answers in [vec![], vec!["了解"], vec!["長い単独の確認ボタンです"]] {
        c.begin(font.clone(), "確認の説明", &answers, -1, true);
        c.idle(14);
        c.tick(1, 1 << 24, 0xf0, 1.0, &[]);
        c.idle(40);
        c.finish();
    }
    for answers in [vec![], vec!["了解"], JUMP_BUTTONS.to_vec()] {
        for at in [0, 1, 12, 13, 20] {
            c.seed(font.clone(), "確認の説明", &answers, 1, true, true);
            for frame in 0..60 {
                c.tick_predicate(
                    1,
                    if frame == at { 1 << 24 } else { 0 },
                    0,
                    1.0,
                    &[],
                    frame >= at,
                );
            }
            assert_eq!(c.model.as_ref().unwrap().selected(), -2);
            assert!(c.model.as_ref().unwrap().complete());
            c.finish();
        }
    }
    c.verify();
}

#[test]
fn question_rejects_invalid_text_extents() {
    for bad in [
        vec2(f32::NAN, 1.0),
        vec2(1.0, f32::INFINITY),
        vec2(-1.0, 1.0),
    ] {
        assert!(Question::new(bad, &[], 0, true).is_err());
        assert!(Question::new(Vec2::ZERO, &[bad], 0, true).is_err());
    }
}
