use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
#[test]
fn ger_backlog_lifecycle_barrier_and_desktop_jump_recovery() {
    let mut c = Controller::new(4);
    assert_eq!(c.begin_frame(10.), vec![Event::MainInstalled]);
    assert!(!c.accepts_main());
    c.end_frame(10.);
    assert!(c.begin_frame(0.).is_empty());
    assert!(c.accepts_main());
    c.request_quick_load(2, false);
    assert_eq!(c.begin_frame(1.), vec![Event::ShowJumpQuestion { row: 2 }]);
    assert!(c.answer(false, 0).is_empty());
    assert_eq!(c.phase(), Phase::Question);
    assert_eq!(
        c.answer(true, 0),
        vec![Event::JumpAccepted { row: 2 }, Event::CancelHistory]
    );
    assert_eq!(c.begin_frame(10.), vec![Event::PublishJump { row: 2 }]);
    c.recover_failed_jump();
    assert_eq!(c.progress(), 1.);
    assert!(!c.history_cancelled());
    assert!(!c.accepts_main());
    c.begin_frame(0.);
    assert!(c.accepts_main());
}
#[derive(Clone, Copy)]
enum Op {
    New,
    Begin(f32),
    End(f32),
    Sync(u8),
    Close,
    Jump(usize, bool),
    Answer(bool, i32),
    Highlight(f32),
}
struct Fixture {
    c: Controller,
    installed: bool,
    permission: bool,
    questions: u32,
    question_live: bool,
    published: u32,
    r0: u32,
    r1: u32,
    script: String,
    expected: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            c: Controller::new(4),
            installed: false,
            permission: false,
            questions: 0,
            question_live: false,
            published: 0,
            r0: 0,
            r1: 0,
            script: String::new(),
            expected: vec![],
        }
    }
    fn op(&mut self, op: Op) {
        let (text, events) = match op {
            Op::New => {
                self.c = Controller::new(4);
                self.installed = false;
                self.permission = false;
                self.questions = 0;
                self.question_live = false;
                self.published = 0;
                self.r0 = 0;
                self.r1 = 0;
                ("N".to_owned(), vec![])
            }
            Op::Begin(dt) => {
                let e = self.c.begin_frame(dt);
                self.permission = self.c.accepts_main();
                (format!("B {dt}"), e)
            }
            Op::End(dt) => {
                self.c.end_frame(dt);
                (format!("E {dt}"), vec![])
            }
            Op::Sync(mask) => {
                let ids = (0..4).filter(|i| mask >> i & 1 != 0).collect::<Vec<_>>();
                self.c.sync_cached(&ids);
                (format!("S {mask}"), vec![])
            }
            Op::Close => {
                self.c.request_close();
                ("C".to_owned(), vec![])
            }
            Op::Jump(row, cached) => (
                format!("J {row} {}", u8::from(cached)),
                self.c.request_quick_load(row, cached),
            ),
            Op::Answer(final_stage, button) => {
                if final_stage {
                    self.question_live = false;
                }
                (
                    format!("Q {} {button}", u8::from(final_stage)),
                    self.c.answer(final_stage, button),
                )
            }
            Op::Highlight(value) => {
                self.c.set_close_flash(value);
                (format!("H {value}"), vec![])
            }
        };
        for event in events {
            match event {
                Event::MainInstalled => self.installed = true,
                Event::ShowJumpQuestion { .. } => {
                    self.questions += 1;
                    self.question_live = true;
                }
                Event::PublishJump { .. } => self.published += 1,
                Event::ParentReturn { reason: 0 } => self.r0 += 1,
                Event::ParentReturn { reason: 1 } => self.r1 += 1,
                _ => {}
            }
        }
        self.script += &text;
        self.script.push('\n');
        let c = &self.c;
        let flag = |v: bool| u8::from(v);
        let mut line = format!(
            "L {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            text.as_bytes()[0] as char,
            c.progress().to_bits(),
            flag(self.installed),
            flag(self.permission),
            flag(c.cancelled),
            flag(c.cancelled),
            flag(c.jump),
            flag(matches!(c.phase, Phase::Done | Phase::Retired)),
            flag(c.phase == Phase::Retired),
            self.questions,
            flag(self.question_live),
            self.published,
            self.published,
            self.r0,
            self.r1,
            c.close_flash().to_bits(),
            flag(c.close_busy())
        );
        for i in 0..4 {
            line += &format!(
                " {} {}",
                c.jump_flash(i).to_bits(),
                c.portrait_flash(i).to_bits()
            );
        }
        self.expected.push(line);
    }
    fn frame(&mut self, dt: f32) {
        self.op(Op::Begin(dt));
        self.op(Op::End(dt));
    }
    fn until(&mut self, dt: f32, predicate: impl Fn(&Controller) -> bool) {
        for _ in 0..1000 {
            if predicate(&self.c) {
                return;
            }
            self.frame(dt);
        }
        panic!("bounded lifecycle did not reach target");
    }
}
#[test]
#[ignore = "executes original browser Start/Close/Exit/QuickLoad/TaskGroup/tweens CPU"]
fn ger_native_backlog_lifecycle_tasks_and_callback_order_match() {
    let mut f = Fixture::new();
    let mut profiles = 0;
    for dt in [0.25f32, 1., 2.5, 10., 1000.] {
        for path in 0..6 {
            profiles += 1;
            f.op(Op::New);
            f.op(Op::Sync(15));
            f.until(dt, Controller::accepts_main);
            if path == 0 || path == 4 {
                if path == 4 {
                    f.op(Op::Highlight(0.7));
                    f.op(Op::End(0.5));
                }
                f.op(Op::Close);
                f.until(dt, |c| c.phase() == Phase::Retired);
            } else {
                let row = (path % 4) as usize;
                let cached = path != 2;
                f.op(Op::Jump(row, cached));
                if path == 3 {
                    f.op(Op::End(0.25));
                    f.op(Op::Sync(0));
                }
                f.until(dt, |c| c.phase() == Phase::Question);
                f.op(Op::Answer(false, 0));
                if path == 5 || path == 2 {
                    f.op(Op::Answer(true, 1));
                    f.until(dt, Controller::accepts_main);
                    f.op(Op::Close);
                    f.until(dt, |c| c.phase() == Phase::Retired);
                } else {
                    f.op(Op::Answer(true, 0));
                    f.until(dt, |c| c.phase() == Phase::Published);
                }
            }
        }
    }
    let root = std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("PWD")));
    let binary = std::env::var_os("SHIN_GEROKASU2_BACKLOG_LIFECYCLE_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("reverse-engineering/gerokasu2/backlog-lifecycle/native-lifecycle")
        });
    let mut child = Command::new(binary)
        .args([
            root.join("game-data/gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("build original lifecycle oracle first");
    let mut stdin = child.stdin.take().unwrap();
    let script = f.script.clone();
    let writer = std::thread::spawn(move || stdin.write_all(script.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let native = String::from_utf8(result.stdout).unwrap();
    assert_eq!(native.lines().count(), f.expected.len());
    for (i, (a, b)) in native.lines().zip(f.expected).enumerate() {
        assert_eq!(
            a,
            b,
            "observation{i} operation{}",
            f.script.lines().nth(i).unwrap()
        );
    }
    println!(
        "GER_BACKLOG_LIFECYCLE_MATCH profiles={profiles} observations={} wholeStart/Close/Exit/QuickLoad/tasks/tweens",
        native.lines().count()
    );
}
