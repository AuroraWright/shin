use super::*;
use std::{cell::RefCell, fmt::Write, io::Write as IoWrite, process::{Command, Stdio}, rc::Rc};

type Events = Rc<RefCell<Vec<[u32; 4]>>>;
struct ScriptMode {
    id: u32,
    done: u32,
    rule: u32,
    after: u32,
    spawn: u32,
    spawn_done: u32,
    spawn_rule: u32,
    events: Events,
}
impl ScriptMode {
    fn draw(&mut self) {
        self.events.borrow_mut().push([4, self.id, 0, 0]);
    }
}
impl Drop for ScriptMode {
    fn drop(&mut self) {
        self.events.borrow_mut().push([3, self.id, 0, 0]);
    }
}
impl Mode<()> for ScriptMode {
    fn completion(&mut self) -> u32 {
        self.events.borrow_mut().push([1, self.id, self.done, 0]);
        self.done
    }
    fn poll(&mut self, delta: Ticks, permission: bool, _: &mut (), pushes: &mut ModePush<'_, Self>) -> Result<u32> {
        self.events.borrow_mut().push([2, self.id, u32::from(permission), delta.as_f32().to_bits()]);
        if self.after != u32::MAX {
            self.done = self.after;
        }
        if self.spawn != 0 {
            let id = std::mem::take(&mut self.spawn);
            pushes.push(Self {
                id, done: self.spawn_done, rule: self.spawn_rule, after: u32::MAX,
                spawn: 0, spawn_done: 0, spawn_rule: 0, events: self.events.clone(),
            });
            self.events.borrow_mut().push([5, id, 0, 0]);
        }
        Ok(match self.rule {
            0 => u32::from(permission), 1 => 0, 2 => 1,
            3 => u32::from(!permission), 4 => 2, 5 => 3, _ => unreachable!(),
        })
    }
}

struct Comparison {
    stack: ModeStack<ScriptMode>,
    events: Events,
    input: String,
    expected: Vec<String>,
}
impl Comparison {
    fn new() -> Self {
        Self { stack: Default::default(), events: Default::default(), input: String::new(), expected: Vec::new() }
    }
    fn row(&mut self, op: char, permission: bool) {
        let mut line = format!("O {op} {} {}", u32::from(permission), self.stack.len());
        for mode in self.stack.iter() {
            write!(line, " {} {}", mode.id, mode.done).unwrap();
        }
        write!(line, " {}", self.stack.snapshot_len).unwrap();
        for mode in self.stack.active[..self.stack.snapshot_len].iter().flatten() {
            write!(line, " {}", mode.id).unwrap();
        }
        let mut events = self.events.borrow_mut();
        write!(line, " {}", events.len()).unwrap();
        for event in events.drain(..) {
            for word in event { write!(line, " {word}").unwrap(); }
        }
        line.push_str(" 1");
        self.expected.push(line);
    }
    fn begin(&mut self) {
        assert!(self.stack.is_empty());
        self.input.push_str("N\n");
        self.row('N', false);
    }
    fn add(&mut self, words: [u32; 7]) {
        let [id, done, rule, after, spawn, spawn_done, spawn_rule] = words;
        writeln!(self.input, "A {id} {done} {rule} {after} {spawn} {spawn_done} {spawn_rule}").unwrap();
        self.stack.push(ScriptMode { id, done, rule, after, spawn, spawn_done, spawn_rule, events: self.events.clone() });
        self.events.borrow_mut().push([5, id, 0, 0]);
        self.row('A', false);
    }
    fn poll(&mut self, initial: u32, delta: f32) {
        writeln!(self.input, "P {initial} {:x}", delta.to_bits()).unwrap();
        let permission = self.stack.poll(Ticks::from_f32(delta), initial, &mut ()).unwrap();
        self.stack.draw(ScriptMode::draw);
        self.row('P', permission);
    }
    fn complete(&mut self, id: u32, word: u32) {
        writeln!(self.input, "C {id} {word}").unwrap();
        self.stack.active.iter_mut().flatten().find(|mode| mode.id == id).unwrap().done = word;
        self.row('C', false);
    }
    fn draw(&mut self) {
        self.input.push_str("G\n");
        self.stack.draw(ScriptMode::draw);
        self.row('G', false);
    }
    fn destroy(&mut self) {
        self.input.push_str("D\n");
        self.stack = Default::default();
        self.row('D', false);
    }
}

#[test]
#[ignore = "executes original updated AArch64 mode-stack functions"]
fn higurashi_mode_stack_ownership_permissions_and_snapshots_match_native() {
    let helper = std::env::var("SHIN_HIGURASHI_NATIVE_MODE_STACK").unwrap();
    println!("Retained reconstruction owner:{} bytes; boxed transfer:{} bytes",
        std::mem::size_of::<crate::adv::higurashi::runner::reconstruction_mode::Transition>(),
        std::mem::size_of::<Option<Box<crate::adv::higurashi::runner::reconstruction_mode::Transition>>>());
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut comparison = Comparison::new();
    let mut timelines = 0;
    for len in 0..8 {
        for mask in 0..(1u32 << len) {
            for initial in [0, 1, 2, 3, u32::MAX - 1, u32::MAX] {
                for delta in [0.0, 0.25, 1.0, 3.0] {
                    comparison.begin();
                    for index in 0..len {
                        let id = index + 1;
                        // Both true and false completion words carry high bits.
                        let done = (mask >> index & 1) | if (index + mask) % 2 == 0 { 2 } else { u32::MAX - 1 };
                        let after = if (index + mask + initial.wrapping_rem(3)) % 3 == 0 { 3 } else { u32::MAX };
                        let spawn = if (index + mask) % 5 == 0 { id + 16 } else { 0 };
                        comparison.add([id, done, (index + mask) % 6, after, spawn, 2, (index + 2) % 6]);
                    }
                    comparison.poll(initial, delta);
                    // A push after polling also cannot change that frame's draw snapshot.
                    comparison.add([64, 0, 0, u32::MAX, 0, 0, 0]);
                    comparison.draw();
                    comparison.poll(initial, delta);
                    comparison.complete(64, 3);
                    comparison.poll(initial, delta);
                    comparison.destroy();
                    timelines += 1;
                }
            }
        }
    }
    // Exercise multiple native owning and borrowed vector reallocations, then
    // interleaved stable compaction and reverse tail/destructor retirement.
    for len in [8, 16, 31, 63] {
        for pattern in 0..4 {
            comparison.begin();
            for id in 1..=len {
                let done = u32::from(match pattern { 0 => false, 1 => true, 2 => id % 2 == 0, _ => id % 3 == 0 });
                comparison.add([id, done, id % 6, if id % 4 == 0 { 1 } else { u32::MAX }, id + 64, 0, (id + 1) % 6]);
            }
            comparison.poll(3, 0.25);
            comparison.poll(1, 3.0);
            comparison.poll(2, 0.0);
            comparison.destroy();
            timelines += 1;
        }
    }
    let mut child = Command::new(helper).arg(exe).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = comparison.input;
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "native fixture failed: {:?} {}", output.status, String::from_utf8_lossy(&output.stderr));
    let output = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), comparison.expected.len());
    for (index, (actual, expected)) in lines.iter().zip(&comparison.expected).enumerate() {
        assert_eq!(actual, expected, "native mode stack observation {index}");
    }
    assert_eq!(timelines, 6136);
    println!("Higurashi mode stack:{timelines} native timelines,{} exact ownership/permission/snapshot observations pass", lines.len());
}
