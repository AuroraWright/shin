//! Original whole105E0 frame and11190/PAGEBACK calls with checked root owners.
use super::PreviousRoot;
use std::{
    fmt::Write as _,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Clone)]
struct Root {
    id: u32,
    value: f32,
}
struct State {
    previous: PreviousRoot<Root>,
    current: Root,
    clones: u32,
    next: u32,
    completed: u32,
    pending: bool,
    prefix: u32,
}
impl State {
    fn new(prefix: u32) -> Self {
        Self {
            previous: PreviousRoot::default(),
            current: Root { id: 0, value: 0. },
            clones: 0,
            next: 0,
            completed: 0,
            pending: false,
            prefix,
        }
    }
    fn line(&self, op: char, events: &[u32]) -> String {
        let has = !self.previous.is_none();
        let mut s = format!(
            "B {op} {} {} {} {} {} {} {}",
            has as u8,
            self.current.value.to_bits(),
            if has {
                self.previous.selected(&self.current).value.to_bits()
            } else {
                0
            },
            self.clones,
            self.next,
            self.completed,
            events.len()
        );
        for value in events {
            write!(s, " {value}").unwrap();
        }
        s
    }
    fn capture(&mut self) -> Vec<u32> {
        let mut events = vec![];
        if self.previous.capture(&self.current, |r| Root {
            id: 1,
            value: r.value,
        }) {
            self.clones += 1;
            events.extend([50, self.clones]);
        }
        self.current.value += 1.;
        events.extend([51, 1]);
        events
    }
    fn draw(&self) -> Vec<u32> {
        let selected = self.previous.selected(&self.current);
        vec![62, 7, 60, selected.id, 61, selected.value.to_bits()]
    }
    fn frame(
        &mut self,
        paused: bool,
        animate: bool,
        reconstructing: bool,
        worker: bool,
        vm: bool,
        dt: f32,
    ) -> Vec<u32> {
        let mut events = vec![];
        if !paused && !reconstructing && !worker && vm {
            loop {
                events.extend([20, (!self.previous.is_none()) as u32, 1, self.next]);
                if !self.pending {
                    events.extend([2, 100 + self.next]);
                    self.next += 1;
                    self.pending = true;
                }
                let i = self.next - 1;
                events.extend([21, (!self.previous.is_none()) as u32, 3, 100 + i]);
                if i < self.prefix {
                    events.extend([4, 100 + i]);
                    self.completed += 1;
                    self.pending = false;
                } else {
                    self.previous.command_blocked(animate);
                    break;
                }
            }
        }
        if !paused {
            self.previous.update_pair(&mut self.current, |r| {
                events.extend([30 + r.id, animate as u32]);
                if animate {
                    r.value += dt;
                }
            });
            events.extend([40, self.previous.selected(&self.current).id]);
        }
        events
    }
}

#[test]
fn previous_root_retains_first_capture_and_drops_only_on_enabled_block() {
    use std::{cell::Cell, rc::Rc};
    #[derive(Clone)]
    struct Owner {
        value: u32,
        drops: Rc<Cell<u32>>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    let drops = Rc::new(Cell::new(0));
    let mut current = Owner {
        value: 3,
        drops: drops.clone(),
    };
    let mut previous = PreviousRoot::default();
    assert!(previous.capture(&current, Clone::clone));
    current.value = 7;
    assert!(!previous.capture(&current, Clone::clone));
    assert_eq!(previous.selected(&current).value, 3);
    assert!(!previous.command_blocked(false));
    assert_eq!(drops.get(), 0);
    assert!(previous.command_blocked(true));
    assert_eq!(drops.get(), 1);
    assert_eq!(previous.selected(&current).value, 7);
    assert!(!previous.command_blocked(true));
    assert!(previous.capture(&current, Clone::clone));
    previous.selected_mut(&mut current).value = 11;
    assert_eq!(previous.selected(&current).value, 11);
    assert_eq!(current.value, 7);
    drop(previous);
    assert_eq!(drops.get(), 2);
    drop(current);
    assert_eq!(drops.get(), 3);
}

#[test]
#[ignore = "requires original Ger executable/fonts and standalone native-back-root CPU probe"]
fn ger_native_previous_root_capture_blocked_command_update_prepare_and_draw() {
    let root = PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROOT").unwrap());
    let out = root.join("gerokasu2/.local/back-root");
    let binary = std::env::var_os("SHIN_GEROKASU2_BACK_ROOT_NATIVE")
        .map(PathBuf::from)
        .unwrap_or_else(|| out.join("native-back-root"));
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    let mut frames = 0;
    for prefix in [0, 2, 7] {
        for flags in 0..32 {
            let mut state = State::new(prefix);
            writeln!(input, "N {prefix}").unwrap();
            expected.push(state.line('N', &[]));
            let mut step = |op: char, state: &mut State| {
                input.push(op);
                input.push('\n');
                let e = if op == 'P' {
                    state.capture()
                } else {
                    state.draw()
                };
                expected.push(state.line(op, &e));
            };
            step('R', &mut state);
            step('P', &mut state);
            step('R', &mut state);
            step('P', &mut state);
            let first = [
                flags & 1 != 0,
                flags & 2 != 0,
                flags & 4 != 0,
                flags & 8 != 0,
                flags & 16 != 0,
            ];
            let sequence = [
                (first, 1.),
                ([true, true, false, false, true], 0.25),
                ([false, false, false, false, true], 0.25),
                ([false, true, false, false, true], 2.),
                ([false, true, false, false, true], 0.),
            ];
            for (i, (f, dt)) in sequence.into_iter().enumerate() {
                writeln!(
                    input,
                    "F {} {} {} {} {} {dt}",
                    f[0] as u8, f[1] as u8, f[2] as u8, f[3] as u8, f[4] as u8
                )
                .unwrap();
                let e = state.frame(f[0], f[1], f[2], f[3], f[4], dt);
                expected.push(state.line('F', &e));
                frames += 1;
                input.push_str("R\n");
                expected.push(state.line('R', &state.draw()));
                if i == 1 || i == 3 {
                    input.push_str("P\n");
                    let e = state.capture();
                    expected.push(state.line('P', &e));
                }
            }
            profiles += 1;
        }
    }
    std::fs::write(out.join("native-back-root.input"), &input).unwrap();
    let mut child = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join("native-back-root.output"), &result.stdout).unwrap();
    std::fs::write(out.join("native-back-root.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let actual = String::from_utf8(result.stdout).unwrap();
    let lines = actual.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), expected.len());
    for (i, (a, e)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(a, e, "observation{i}");
    }
    eprintln!(
        "GER_BACK_ROOT_RUST profiles={profiles} frames={frames} observations={} capture_pending_animation_gate_current_previous_update_selected_prepare_draw=exact",
        lines.len()
    );
}
