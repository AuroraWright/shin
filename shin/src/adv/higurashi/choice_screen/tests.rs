use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
#[derive(Default)]
struct Comparison {
    model: Option<Model>,
    input: String,
    expected: Vec<String>,
}
fn tween(row: &mut String, t: &HigurashiTweener) {
    write!(row, " {} {}", t.value().to_bits(), u8::from(t.is_waiting())).unwrap();
}
impl Comparison {
    fn snapshot(&mut self, op: char, result: bool) {
        let m = self.model.as_mut().unwrap();
        let mut row = format!(
            "C {op} {} {} {} {} {} {} {} {} {}",
            u8::from(result),
            m.phase,
            m.selected,
            m.opacity.to_bits(),
            u8::from(m.hidden),
            m.rows.len(),
            u8::from(
                self.input
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("N "))
                    .unwrap()
                    .split_whitespace()
                    .nth(2)
                    .map(|s| u32::from_str_radix(s, 16).unwrap() & 0x1000 != 0)
                    .unwrap()
            ),
            u8::from(m.result.is_some()),
            m.result.unwrap_or(0)
        );
        tween(&mut row, &m.badge_slide);
        tween(&mut row, &m.badge_alpha);
        for r in &m.rows {
            write!(
                row,
                " R {} {} {} {} {}",
                r.id,
                r.spec.original,
                r.spec.badges,
                r.y.to_bits(),
                r.spec.height.to_bits()
            )
            .unwrap();
            tween(&mut row, &r.x);
            tween(&mut row, &r.alpha);
            tween(&mut row, &r.focus);
            write!(row, " {}", r.spec.label.len()).unwrap();
            for b in r.spec.label.bytes() {
                write!(row, " {b}").unwrap();
            }
        }
        let cues = m.take_cues();
        write!(row, " S {}", cues.len()).unwrap();
        for cue in cues {
            write!(
                row,
                " {}",
                ["move", "error", "select", "seldecide"]
                    .iter()
                    .position(|s| *s == cue)
                    .unwrap()
            )
            .unwrap();
        }
        self.expected.push(row);
    }
    fn begin(&mut self, count: usize, flags: u32, seed: u32) {
        writeln!(self.input, "N {count} {flags:x} 2 {seed}").unwrap();
        let mut specs = Vec::new();
        for i in 0..count {
            let height = 48.0 + i as f32 * 7.25;
            let text = if i % 2 == 0 {
                format!("*05.Option{i}")
            } else {
                format!("Option{i}")
            };
            writeln!(
                self.input,
                "{:x} {}",
                height.to_bits(),
                text.bytes().map(|b| format!("{b:02x}")).collect::<String>()
            )
            .unwrap();
            if flags & (1 << i) != 0 {
                let (badges, label) = split_badges(&text);
                specs.push(Spec {
                    original: i,
                    label: label.into(),
                    badges,
                    seen: i == 1,
                    height: height + 16.0,
                });
            }
        }
        self.model = Some(Model::new(specs, flags, (flags & 0x1000 != 0).then_some(seed)).unwrap());
        self.snapshot('N', false);
    }
    fn update(&mut self, delta: f32) {
        writeln!(self.input, "U {:x}", delta.to_bits()).unwrap();
        self.model.as_mut().unwrap().advance(Ticks::from_f32(delta));
        self.snapshot('U', false);
    }
    fn control(&mut self, permission: bool, held: u32, pressed: u32, repeat: u32) {
        writeln!(
            self.input,
            "I {} {held:x} {pressed:x} {repeat:x}",
            u8::from(permission)
        )
        .unwrap();
        self.model.as_mut().unwrap().input(
            permission,
            Buttons {
                held,
                pressed,
                repeat,
                ..Default::default()
            },
        );
        self.snapshot('I', false);
    }
    fn pointer(&mut self, kind: u32, p: Vec2) {
        writeln!(
            self.input,
            "P {kind} {:x} {:x}",
            p.x.to_bits(),
            p.y.to_bits()
        )
        .unwrap();
        let m = self.model.as_mut().unwrap();
        let result = match kind {
            0 => m.probe(p),
            1 => {
                m.press(p);
                false
            }
            2 => {
                m.release(p);
                false
            }
            3 => false,
            _ => unreachable!(),
        };
        self.snapshot('P', result);
    }
    fn hide(&mut self, hidden: bool) {
        writeln!(self.input, "H {}", u8::from(hidden)).unwrap();
        self.model.as_mut().unwrap().hidden = hidden;
        self.snapshot('H', false);
    }
    fn fast_forward(&mut self) {
        self.input.push_str("F\n");
        self.model.as_mut().unwrap().fast_forward();
        self.snapshot('F', false);
    }
    fn end(&mut self) {
        self.input.push_str("D\n");
        self.model = None;
    }
}
#[test]
#[ignore = "original choice UI constructor/filter/prefix/shuffle/layout/controller/pointer/tweens with prepared text metrics"]
fn higurashi_choice_screen_matches_native_state_and_input() {
    let mut c = Comparison::default();
    for count in [1, 3, 12] {
        let mask = (1 << count) - 1;
        for flags in [
            mask,
            mask | 0x8000,
            mask | 0x1000,
            mask | 0x9000,
            mask & 0xaaa,
        ] {
            if flags == 0 {
                continue;
            }
            for seed in [0, 1, 42, u32::MAX] {
                for pointer in [false, true] {
                    c.begin(count, flags, seed);
                    c.pointer(0, Vec2::new(960.0, 500.0));
                    for _ in 0..30 {
                        c.update(3.0);
                    }
                    assert_eq!(c.model.as_ref().unwrap().phase, 2);
                    c.control(false, 0, 1 << 24, 0);
                    c.control(true, 0, 0, 1 << 4);
                    c.update(0.1);
                    c.control(true, 0, 0, 1 << 6);
                    c.control(true, 0x50, 0, 1 << 7);
                    c.control(true, 0, 0, 1 << 5);
                    c.update(1.0);
                    c.hide(true);
                    for _ in 0..4 {
                        c.update(3.0);
                    }
                    c.control(true, 0, 1 << 24, 0);
                    c.hide(false);
                    for _ in 0..4 {
                        c.update(3.0);
                    }
                    if pointer {
                        let row = &c.model.as_ref().unwrap().rows[0];
                        let y = row.y + row.spec.height * 0.5;
                        for x in [459.99997, 460.0, 1459.9999, 1460.0] {
                            c.pointer(0, Vec2::new(x, y));
                        }
                        c.pointer(1, Vec2::new(960.0, y));
                        c.pointer(3, Vec2::ZERO);
                        c.pointer(2, Vec2::new(960.0, y));
                    } else {
                        c.control(true, 0, 1 << 24, 0);
                    }
                    // Shuffled native row IDs can queue two alpha transitions
                    // on the selected array row; allow that preserved tail.
                    for _ in 0..70 {
                        c.update(3.0);
                    }
                    assert!(
                        c.model.as_ref().unwrap().result.is_some(),
                        "count={count} flags={flags:x} seed={seed} pointer={pointer}"
                    );
                    c.end();
                }
            }
        }
    }
    // Fast-forward checks the original phase1/3 row clocks and completion frame.
    c.begin(3, 7, 42);
    c.update(3.0);
    c.fast_forward();
    c.update(0.0);
    c.control(true, 0, 0, 1 << 6);
    c.control(true, 0, 1 << 24, 0);
    c.fast_forward();
    c.update(0.0);
    c.end();
    // A normal empty visibility set reaches phase2 and plays error on A;
    // safe unchanged endpoint navigation does not invent a result.
    c.begin(0, 0, 42);
    c.update(3.0);
    c.control(false, 0, 1 << 24, 0);
    c.control(true, 0, 1 << 24, 0);
    c.control(true, 0, 1 << 24, 1 << 4);
    c.control(true, 0, 1 << 24, 1 << 5);
    c.pointer(0, Vec2::new(960.0, 500.0));
    c.fast_forward();
    c.end();
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHOICE_SCREEN").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = c.input;
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let actual = actual.lines().collect::<Vec<_>>();
    assert_eq!(actual.len(), c.expected.len());
    for (i, (a, b)) in actual.iter().zip(&c.expected).enumerate() {
        assert_eq!(*a, b, "choice operation{i}");
    }
    eprintln!(
        "Choice screen:{} exact native state/input observations;12-row filtering,original indexes/shuffle/prefixes,focus/hidden clocks,controller/touch,fast-forward/completion and retirement pass",
        actual.len()
    );
}
