use super::*;
use crate::adv::{
    higurashi::mode_stack::{Mode, ModePush, ModeStack},
    input_area::{PointerEvent, PointerKind},
};
use std::{
    cell::Cell,
    fmt::Write,
    io::Write as IoWrite,
    process::{Command, Stdio},
    rc::Rc,
};

enum Child {
    Confirm(Confirmation),
    Reload(Rc<Cell<bool>>),
}
#[derive(Default)]
struct Services {
    effects: Vec<u32>,
    pending: Vec<Child>,
    reload: Rc<Cell<bool>>,
}
impl MenuServices for Services {
    fn enter_modal(&mut self) {
        self.effects.push(0);
    }
    fn exit_modal(&mut self) {
        self.effects.push(1);
    }
    fn cue(&mut self, cue: Cue) {
        self.effects.push(match cue {
            Cue::Move => 2,
            Cue::Cancel => 3,
            Cue::Confirm => 4,
        });
    }
    fn push_confirmation(&mut self, c: Confirmation) {
        self.pending.push(Child::Confirm(c));
    }
    fn push_layer_reloader(&mut self) {
        self.effects.push(5);
        self.pending.push(Child::Reload(self.reload.clone()));
    }
    fn dispatch_item(&mut self, item: u32) {
        self.effects.push(16 + item);
    }
}
struct ChildServices<'a> {
    menu: &'a mut SystemMenu,
    services: &'a mut Services,
}
impl<'a> Mode<ChildServices<'a>> for Child {
    fn completion(&mut self) -> u32 {
        self.done()
    }
    fn poll(
        &mut self,
        _: Ticks,
        _: bool,
        context: &mut ChildServices<'a>,
        _: &mut ModePush<'_, Self>,
    ) -> anyhow::Result<u32> {
        Ok(match self {
            Child::Confirm(c) => c.poll(context.menu, context.services),
            Child::Reload(_) => 0,
        })
    }
}
impl Child {
    fn done(&self) -> u32 {
        match self {
            Child::Confirm(c) => c.completion_word(),
            Child::Reload(p) => u32::from(p.get()),
        }
    }
}
struct Comparison {
    menu: Option<SystemMenu>,
    services: Services,
    children: ModeStack<Child>,
    input: String,
    expected: Vec<String>,
    frame: u32,
    ticks: u64,
}
impl Default for Comparison {
    fn default() -> Self {
        Self {
            menu: None,
            services: Default::default(),
            children: Default::default(),
            input: String::new(),
            expected: Vec::new(),
            frame: 0,
            ticks: 0,
        }
    }
}
impl Comparison {
    fn row(&mut self, op: char, permission: u32) {
        let m = self.menu.as_ref().unwrap();
        let s = &m.state;
        let mut out = format!(
            "M {op} {permission} {} {} {} {} {} {} {} {} {}",
            s.phase,
            s.selected,
            s.vertical_offset,
            s.pointer_row.map_or(-1, |i| i as i32),
            u32::from(s.extended_scenario),
            s.initial_setting,
            u32::from(s.layer_reload),
            m.completion_word(),
            s.rows.len()
        );
        for row in &s.rows {
            write!(out, " {}", row.item).unwrap();
            for t in [&row.slide, &row.flash] {
                write!(
                    out,
                    " {} {}",
                    t.value().to_bits(),
                    u32::from(t.is_waiting())
                )
                .unwrap();
            }
        }
        for t in [&s.detail_slide, &s.detail_fade] {
            write!(
                out,
                " {} {}",
                t.value().to_bits(),
                u32::from(t.is_waiting())
            )
            .unwrap();
        }
        write!(out, " {}", s.play_time.len()).unwrap();
        for b in s.play_time.bytes() {
            write!(out, " {b}").unwrap();
        }
        write!(out, " {}", self.services.effects.len()).unwrap();
        for e in self.services.effects.drain(..) {
            write!(out, " {e}").unwrap();
        }
        for c in self.services.pending.drain(..) {
            self.children.push(c);
        }
        write!(out, " {}", self.children.len()).unwrap();
        for c in self.children.iter() {
            write!(out, " {}", c.done()).unwrap();
        }
        out.push_str(" 1");
        self.expected.push(out);
    }
    fn begin(&mut self, id: i32, setting: u8) {
        assert!(self.menu.is_none());
        self.services = Services::default();
        self.frame = 0;
        self.ticks = 0;
        writeln!(self.input, "N {id} {setting}").unwrap();
        self.menu = Some(SystemMenu::new(id, setting, &mut self.services));
        self.row('N', 0);
    }
    fn frame(
        &mut self,
        delta: f32,
        permission: u32,
        pressed: u32,
        repeated: u32,
        setting: u8,
        play: f64,
        events: &[PointerEvent],
    ) {
        self.frame += 1;
        self.ticks += 50000;
        writeln!(
            self.input,
            "F {} {} {permission} {pressed:x} {repeated:x} {setting} {:x} {} {:x}",
            self.frame,
            self.ticks,
            play.to_bits(),
            events.len(),
            delta.to_bits()
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
        let out = self.menu.as_mut().unwrap().poll(
            Ticks::from_f32(delta),
            permission,
            pressed,
            repeated,
            &PointerFrame {
                number: self.frame,
                time_seconds: self.ticks as f64 * 1e-6,
                events,
            },
            setting,
            play,
            &mut self.services,
        );
        self.row('F', out);
    }
    fn children(&mut self, permission: u32) {
        writeln!(self.input, "C {permission}").unwrap();
        let mut context = ChildServices {
            menu: self.menu.as_mut().unwrap(),
            services: &mut self.services,
        };
        let out = self
            .children
            .poll(Ticks::ZERO, permission, &mut context)
            .unwrap();
        self.row('C', u32::from(out));
    }
    fn resolve(&mut self) {
        self.input.push_str("S\n");
        self.menu.as_mut().unwrap().layer_reload_finished();
        self.services.reload.set(true);
        self.row('S', 0);
    }
    fn destroy(&mut self) {
        self.input.push_str("D\n");
        self.children.clear_with(&mut ChildServices {
            menu: self.menu.as_mut().unwrap(),
            services: &mut self.services,
        });
        self.menu = None;
    }
    fn opened(&mut self, id: i32, setting: u8) {
        self.begin(id, setting);
        self.frame(20.0, 1, u32::MAX, u32::MAX, setting, 0.0, &[]);
    }
}
fn event(kind: PointerKind, id: i32, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id,
        position: glam::ivec2(x, y),
    }
}

#[test]
#[ignore = "executes original updated AArch64 menu constructor/poll/tweens/contact/task stack"]
fn higurashi_system_menu_rows_clocks_contacts_and_confirmation_match_native() {
    let helper = std::env::var("SHIN_HIGURASHI_NATIVE_SYSTEM_MENU").unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut c = Comparison::default();
    let mut timelines = 0;
    let ids = [
        i32::MIN,
        -1,
        0,
        4,
        5,
        6,
        12,
        13,
        999,
        1000,
        1999,
        2000,
        2999,
        3000,
        9999,
        10000,
        10001,
        10999,
        11000,
        11999,
        12000,
        12999,
        13000,
        i32::MAX,
    ];
    for id in ids {
        for setting in [0, 255] {
            c.begin(id, setting);
            for delta in [0.0, 0.25, 0.75, 1.0, 2.0, 3.0, 5.0, 8.0, 0.0] {
                c.frame(delta, 1, 0, 0, setting, 3600.0, &[]);
            }
            c.frame(20.0, 1, 1 << 15, 0, setting, 60000.0 * 3600.0, &[]);
            c.frame(0.0, 1, u32::MAX, u32::MAX, setting, 0.0, &[]);
            c.destroy();
            timelines += 1;
        }
    }
    // Permission low bit, pressed/repeated separation and repeat precedence.
    for permission in [0, 1, 2, 3, u32::MAX - 1, u32::MAX] {
        for pressed in (0..32).map(|bit| 1 << bit).chain([0, u32::MAX, 0x03008000]) {
            for repeated in [0, 0xf0, 1 << 4, 1 << 6, 1 << 7, 1 << 5] {
                c.opened(0, 0);
                c.frame(1.0, permission, pressed, repeated, 0, 59.0 * 3600.0, &[]);
                c.children(permission);
                c.frame(10.0, permission, 0, 0, 0, 60.0 * 3600.0, &[]);
                c.children(permission);
                c.frame(20.0, permission, 0, 0, 0, 59999.0 * 3600.0, &[]);
                c.children(permission);
                c.children(permission);
                c.destroy();
                timelines += 1;
            }
        }
    }
    // Current-selection callback: moving after Confirm can dispatch another row
    // immediately, because the callback queries that row's flash each poll.
    for repeated in [0, 1 << 4, 1 << 5, 1 << 6, 1 << 7] {
        c.opened(0, 0);
        c.frame(1.0, 1, 1 << 24, 0, 0, 0.0, &[]);
        c.frame(0.0, 1, 0, repeated, 0, 0.0, &[]);
        c.children(1);
        for delta in [0.25, 0.75, 1.0, 2.0, 3.0, 1.0, 0.0] {
            c.frame(delta, 1, 0, 0, 0, 0.0, &[]);
            c.children(1);
        }
        c.destroy();
        timelines += 1;
    }
    // Dirty settings gating uses strict signed id>10000 and compares the byte
    // captured at construction. Completion is checked before endpoint update.
    for id in [9999, 10000, 10001, 11000, 12000] {
        for before in [0, 1, 255] {
            for after in [0, 1, 255] {
                c.opened(id, before);
                c.frame(20.0, 1, 1 << 25, 0, after, 0.0, &[]);
                c.frame(20.0, 1, 0, 0, after, 0.0, &[]);
                c.children(1);
                c.resolve();
                c.children(1);
                c.frame(0.0, 1, 0, 0, after, 0.0, &[]);
                c.destroy();
                timelines += 1;
            }
        }
    }
    // Physical1280x720 samples hit authored x16/y72 rectangles, including all
    // boundaries/gaps; outside release closes. Cancel/drag suppress activation.
    use PointerKind::*;
    for id in [5, 0, 10000, 11001, 12001] {
        for x in [10, 11, 16, 274, 275, 1000] {
            for y in [
                -1, 0, 47, 48, 127, 128, 133, 134, 170, 171, 426, 427, 474, 475, 554, 555,
            ] {
                c.opened(id, 0);
                c.frame(1.0, 1, 1 << 24, 0, 0, 0.0, &[event(Down, 3, x, y)]);
                c.frame(1.0, 1, 1 << 24, 0, 0, 0.0, &[event(Up, 3, 900, 600)]);
                c.children(1);
                c.frame(20.0, 1, 0, 0, 0, 0.0, &[]);
                c.children(1);
                c.frame(0.0, 1, 0, 0, 0, 0.0, &[]);
                c.children(1);
                c.destroy();
                timelines += 1;
            }
        }
    }
    for sequence in [
        vec![event(Cancel, 1, 0, 0)],
        vec![event(Move, 1, 31, 55), event(Up, 1, 31, 55)],
        vec![
            event(Down, 2, 21, 55),
            event(Up, 1, 21, 55),
            event(Up, 2, 21, 55),
        ],
    ] {
        for permission in [0, 1] {
            c.opened(0, 0);
            c.frame(1.0, 1, 0, 0, 0, 0.0, &[event(Down, 1, 20, 55)]);
            c.frame(1.0, permission, 1 << 24, 0, 0, 0.0, &sequence);
            c.frame(20.0, 1, 0, 0, 0, 0.0, &[]);
            c.children(1);
            c.destroy();
            timelines += 1;
        }
    }
    // Held press reaches the real0.15-second callback. Frame-cache reuse,
    // movement thresholds and permission flush act on that live flash owner.
    for permission in [0, 1] {
        for sequence in [
            vec![event(Up, 7, 900, 700)],
            vec![event(Cancel, 7, 0, 0)],
            vec![event(Move, 7, 30, 400), event(Up, 7, 30, 400)],
            vec![event(Move, 7, 31, 400), event(Up, 7, 31, 400)],
        ] {
            c.opened(0, 0);
            c.frame(1.0, 1, 0, 0, 0, 0.0, &[event(Down, 7, 20, 400)]);
            for _ in 0..5 {
                c.frame(1.0, 1, 1 << 24, 0xf0, 0, 0.0, &[]);
            }
            c.frame -= 1;
            c.ticks -= 50000;
            c.frame(0.25, 1, 1 << 24, 0xf0, 0, 0.0, &[event(Move, 7, 900, 700)]);
            c.frame(1.0, permission, 0, 0, 0, 0.0, &sequence);
            c.children(1);
            c.frame(10.0, 1, 0, 0, 0, 0.0, &[]);
            c.children(1);
            c.children(1);
            c.destroy();
            timelines += 1;
        }
    }
    // Original f64 conversion, formatting, saturation and clock refresh.
    c.opened(0, 0);
    for play in [
        f64::NAN,
        f64::NEG_INFINITY,
        -1.0,
        0.0,
        3599.999,
        3600.0,
        215999.999,
        216000.0,
        59999.0 * 3600.0,
        60000.0 * 3600.0,
        f64::INFINITY,
    ] {
        c.frame(0.0, 0, 0, 0, 0, play, &[]);
    }
    c.destroy();
    timelines += 1;
    let mut process = Command::new(helper)
        .arg(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = process.stdin.take().unwrap();
    let input = c.input;
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = process.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "native helper {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<_> = actual.lines().collect();
    assert_eq!(rows.len(), c.expected.len());
    for (index, (actual, expected)) in rows.iter().zip(&c.expected).enumerate() {
        assert_eq!(actual, expected, "native menu observation{index}");
    }
    println!(
        "{timelines} native menu timelines,{} exact row/clock/contact/callback observations pass",
        rows.len()
    );
}
