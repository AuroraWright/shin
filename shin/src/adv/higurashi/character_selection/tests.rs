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
fn tween(s: &mut String, t: &HigurashiTweener) {
    write!(s, " {} {}", t.value().to_bits(), u8::from(t.is_waiting())).unwrap();
}
impl Comparison {
    fn snapshot(&mut self, op: char, result: bool) {
        let m = self.model.as_mut().unwrap();
        let mut s = format!(
            "H {op} {} {} {} {} {} {} {}",
            u8::from(result),
            m.phase,
            m.selected,
            m.top,
            m.rows.len(),
            u8::from(m.result.is_some()),
            m.result.unwrap_or(0)
        );
        for t in &m.clocks {
            tween(&mut s, t);
        }
        for row in &m.rows {
            write!(s, " T {} {} {}", row.id, row.original, row.x).unwrap();
            tween(&mut s, &row.alpha);
            tween(&mut s, &row.flash);
        }
        let cues = m.take_cues();
        write!(s, " C {}", cues.len()).unwrap();
        for cue in cues {
            write!(
                s,
                " {}",
                ["select", "move", "decide"]
                    .iter()
                    .position(|s| *s == cue)
                    .unwrap()
            )
            .unwrap();
        }
        self.expected.push(s);
    }
    fn begin(&mut self, mask: u32) {
        writeln!(self.input, "N {mask:x}").unwrap();
        self.model = Some(Model::new(mask).unwrap());
        self.snapshot('N', false);
    }
    fn phase(&mut self) {
        self.input.push_str("Q\n");
        self.model.as_mut().unwrap().poll_phase();
        self.snapshot('Q', false);
    }
    fn update(&mut self, delta: f32) {
        writeln!(self.input, "U {:x}", delta.to_bits()).unwrap();
        self.model
            .as_mut()
            .unwrap()
            .advance_layer(Ticks::from_f32(delta));
        self.snapshot('U', false);
    }
    fn frame(&mut self, delta: f32) {
        self.phase();
        self.update(delta);
    }
    fn input(&mut self, permission: bool, held: u32, pressed: u32, repeat: u32) {
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
#[ignore = "actual character-selection ctor/mask/phase/clocks/controller/pointer wrapper/result/retirement;prepared base and atlas GPU handles,no audio/GPU/saves"]
fn higurashi_character_selection_state_and_input_match_native() {
    let mut c = Comparison::default();
    let mut owners = 0;
    for mask in (1..16).chain([0xffff000f, 0x12340005]) {
        for pointer in [false, true] {
            for fast in [false, true] {
                c.begin(mask);
                owners += 1;
                c.pointer(0, Vec2::new(432.0, 600.0));
                c.input(true, 0, 1 << 24, 0);
                c.frame(0.1);
                if fast {
                    c.fast_forward();
                    c.frame(0.0);
                } else {
                    for _ in 0..35 {
                        c.frame(3.0);
                    }
                }
                assert_eq!(c.model.as_ref().unwrap().phase, 2);
                c.input(false, 0, 1 << 24, 0);
                c.input(true, 0, 1 << 25, 0);
                c.input(true, 0, 0, (1 << 5) | (1 << 7));
                c.input(true, 0, 0, 1 << 4);
                c.update(0.1);
                c.input(true, 0, 0, 1 << 6);
                c.update(3.0);
                c.input(true, 0, 0, 1 << 6);
                c.update(3.0);
                c.input(true, 0, 0, 1 << 4);
                c.update(3.0);
                if pointer {
                    let m = c.model.as_ref().unwrap();
                    let index = m.rows.len() - 1;
                    let top = m.top as f32 + index as f32 * 150.0 + 3.0;
                    for x in [431.99997, 432.0, 1043.9999, 1044.0] {
                        c.pointer(0, Vec2::new(x, top));
                    }
                    for y in [top - 0.0001, top, top + 143.9999, top + 144.0] {
                        c.pointer(0, Vec2::new(600.0, y));
                    }
                    c.pointer(1, Vec2::new(600.0, top + 72.0));
                    c.pointer(3, Vec2::ZERO);
                    c.pointer(2, Vec2::new(600.0, top + 72.0));
                } else {
                    // Character selection gives A priority over simultaneous Up.
                    c.input(true, 0, 1 << 24, 1 << 4);
                }
                assert_eq!(c.model.as_ref().unwrap().phase, 3);
                if fast {
                    c.fast_forward();
                    c.frame(0.0);
                } else {
                    for _ in 0..55 {
                        c.frame(3.0);
                    }
                }
                assert_eq!(c.model.as_ref().unwrap().phase, 4);
                assert!(c.model.as_ref().unwrap().result.is_some());
                c.frame(3.0);
                c.phase();
                c.end();
            }
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHARACTER_SELECTION").unwrap())
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
    for (i, (a, b)) in actual.iter().zip(c.expected).enumerate() {
        assert_eq!(*a, b, "character operation{i}");
    }
    eprintln!(
        "Character selection:{owners} owners,{} exact native observations;four-bit masks/original indexes/even placement,8 clocks/staggered open-close/flash/focus,A priority/Up-Down/contact bounds,fast-forward/original result once and atlas/layer/wrapper retirement pass",
        actual.len()
    );
}
