use super::*;
use crate::adv::{
    higurashi::input::{self, Buttons, ReadState},
    input_area::{PointerEvent, PointerKind},
};
use std::{
    cell::RefCell,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

struct Reference {
    events: RefCell<String>,
    wait: f32,
    bits: u32,
}
impl Reference {
    fn event(&self, c: char) {
        self.events.borrow_mut().push(c);
    }
    fn point(&self, c: char, p: Vec2) {
        write!(
            self.events.borrow_mut(),
            "{c}:{:08x}:{:08x};",
            p.x.to_bits(),
            p.y.to_bits()
        )
        .unwrap();
    }
    fn bit(&self, n: u32) -> bool {
        self.bits & (1 << n) != 0
    }
}
impl Services for Reference {
    fn request(&mut self, r: Request) {
        use Request::*;
        let c = match r {
            FlushInput => return,
            CueEnabled => 'E',
            CueDisabled => 'D',
            CueSceneJump => 'J',
            NextChapter => 'N',
            PreviousChapter => 'P',
            HintPulse => 'T',
            SceneJump => 'G',
            SystemMenu => 'S',
            MessageBrowser => 'B',
            PreviousMessage => 'R',
            HideWindow => 'H',
        };
        self.event(c);
    }
    fn click_message(&mut self) -> bool {
        self.event('C');
        self.bit(15)
    }
    fn message_interested(&self) -> bool {
        self.event('I');
        self.bit(16)
    }
    fn interrupt_wait(&mut self) {
        self.wait = 0.0;
    }
    fn scene_jump_available(&mut self, _: bool) -> bool {
        self.bit(14)
    }
}
impl TouchServices for Reference {
    fn overlay_probe(&mut self, p: Vec2) -> bool {
        if self.bit(10) {
            self.point('p', p);
            self.bit(17)
        } else {
            false
        }
    }
    fn overlay_press(&mut self, p: Vec2) {
        if self.bit(10) {
            self.point('d', p);
        }
    }
    fn overlay_release(&mut self, p: Vec2) {
        if self.bit(10) {
            self.point('u', p);
        }
    }
    fn overlay_cancel(&mut self) {
        if self.bit(10) {
            self.event('x');
        }
    }
}
struct Comparison {
    root: RootTouch,
    input: InputState,
    context: Context,
    reference: Reference,
    commands: String,
    expected: Vec<String>,
}
impl Default for Comparison {
    fn default() -> Self {
        Self {
            root: Default::default(),
            input: InputState {
                modes: 0,
                temporary_indicator: 0,
            },
            context: context(0, 0, 0, 0.0),
            reference: Reference {
                events: RefCell::default(),
                wait: 12.0,
                bits: 0,
            },
            commands: String::new(),
            expected: Vec::new(),
        }
    }
}
fn context(bits: u32, flags: u32, allow: u32, value: f32) -> Context {
    let bit = |n: u32| bits & (1u32 << n) != 0;
    Context {
        permission: bit(0),
        captured: false,
        disabled: bit(2),
        system_locked: bit(3),
        vm: bit(4).then_some(ReadState {
            flags,
            fast_forward_blocked: 0,
            is_read: 0,
        }),
        paused: bit(5),
        continuation_blocked: bit(6),
        overlay_transition_busy: bit(7),
        overlay_transition_value: value,
        notification_busy: bit(8),
        animations_enabled: bit(9),
        overlay: bit(10).then_some(OverlayInput {
            menus: bit(11),
            hide_window: bit(12),
        }),
        hint_busy: bit(13),
        scene_jump_available: bit(14),
        wait_allow_interrupt: allow,
    }
}
impl Comparison {
    fn row(&mut self, op: char, result: u32) {
        let events = std::mem::take(self.reference.events.get_mut());
        let t = self.root.state;
        self.expected.push(format!(
            "T {op} {result} {:08x} {} {} {} {} {:08x} {}",
            self.input.modes,
            self.input.temporary_indicator,
            t.selected,
            t.pressed,
            u8::from(t.overlay),
            self.reference.wait.to_bits(),
            if events.is_empty() { "-" } else { &events }
        ));
    }
    fn seed(
        &mut self,
        op: char,
        bits: u32,
        flags: u32,
        modes: u32,
        indicator: u8,
        touch: State,
        allow: u32,
        value: f32,
    ) {
        if op == 'N' {
            self.root = RootTouch::default();
        }
        self.root.state = touch;
        self.input = InputState {
            modes,
            temporary_indicator: indicator,
        };
        self.context = context(bits, flags, allow, value);
        self.reference.bits = bits;
        self.reference.wait = 12.0;
        writeln!(
            self.commands,
            "{op} {bits:x} {flags:x} {modes:x} {indicator} {:x} {} {} {} {allow} {:x}",
            touch.opacity.to_bits(),
            touch.selected,
            touch.pressed,
            u8::from(touch.overlay),
            value.to_bits()
        )
        .unwrap();
        self.row(op, 0);
    }
    fn callback(&mut self, kind: u32, position: Vec2) {
        writeln!(
            self.commands,
            "C {kind} {:x} {:x}",
            position.x.to_bits(),
            position.y.to_bits()
        )
        .unwrap();
        let mut contact = Contact {
            touch: &mut self.root.state,
            input: &mut self.input,
            context: self.context,
            services: &mut self.reference,
        };
        let result = match kind {
            0 => u32::from(contact.probe(position)),
            1 => {
                contact.reset();
                0
            }
            2 => {
                contact.press(position);
                0
            }
            3 => {
                contact.release(position);
                0
            }
            4 => {
                contact.long_press(position);
                0
            }
            5 => {
                contact.drag(Vec2::ZERO, position);
                0
            }
            6 => {
                contact.gesture(position);
                0
            }
            7 => {
                contact.cancel();
                0
            }
            _ => unreachable!(),
        };
        self.row('C', result);
    }
    fn frame(
        &mut self,
        number: u32,
        ticks: u64,
        permission: bool,
        pressed: u32,
        repeat: u32,
        events: &[PointerEvent],
    ) {
        writeln!(
            self.commands,
            "F {number} {ticks} {} {pressed:x} {repeat:x} {}",
            u8::from(permission),
            events.len()
        )
        .unwrap();
        for e in events {
            writeln!(
                self.commands,
                "{} {} {} {}",
                e.kind as i32, e.id, e.position.x, e.position.y
            )
            .unwrap();
        }
        let mut context = self.context;
        context.permission = permission;
        context.captured = self.root.process(
            &mut self.input,
            context,
            &PointerFrame {
                number,
                time_seconds: ticks as f64 * 1e-6,
                events,
            },
            &mut self.reference,
        );
        let result = input::process(
            &mut self.input,
            context,
            Buttons {
                pressed,
                repeat,
                ..Default::default()
            },
            &mut self.reference,
        );
        self.row('F', u32::from(result));
    }
    fn end(&mut self) {
        self.commands.push_str("D\n");
        self.row('D', 0);
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
#[ignore = "original updated root callbacks/hit edges/gesture gates and actual contact/13140 frame timelines"]
fn higurashi_root_touch_matches_updated_callbacks_and_contacts() {
    let mut c = Comparison::default();
    let base = (1 << 0) | (1 << 4) | (1 << 9) | (1 << 16);
    let gates = [
        base,
        base | (1 << 2),
        base | (1 << 3),
        base & !(1 << 4),
        base | (1 << 5),
        base | (1 << 6),
        base | (1 << 7),
        base | (1 << 8),
        base & !(1 << 9),
        base & !(1 << 16),
        base | (1 << 10),
        base | (1 << 10) | (1 << 11) | (1 << 12) | (1 << 17),
    ];
    let xs = [
        0.0,
        1332.0,
        1332.0001,
        1428.0,
        1523.9999,
        1524.0,
        1524.0001,
        1619.9999,
        1620.0,
        1620.0001,
        1716.0,
        1812.0,
        1908.0,
        1920.0,
        f32::NAN,
    ];
    let ys = [
        0.0,
        119.99999,
        120.0,
        239.99998,
        240.0,
        251.99998,
        252.0,
        371.99997,
        372.0,
        f32::NAN,
    ];
    for bits in gates {
        for opacity in [0.0, 0.99999994, 1.0, f32::NAN] {
            for indicator in [0, 1, 9] {
                for x in xs {
                    for y in ys {
                        c.seed(
                            'N',
                            bits,
                            u32::MAX,
                            0x81,
                            indicator,
                            State {
                                selected: 4,
                                pressed: 4,
                                overlay: true,
                                opacity,
                            },
                            1,
                            0.0,
                        );
                        c.callback(0, Vec2::new(x, y));
                        c.end();
                    }
                }
            }
        }
    }
    let probes = c.expected.len() / 3;
    for bits in gates {
        for flags in [0x1234, u32::MAX] {
            for selected in [-1, 0, 1, 2, 3, 4, 5] {
                for overlay in [false, true] {
                    for kind in 1..8 {
                        for allow in [0, 1, 2] {
                            c.seed(
                                'N',
                                bits,
                                flags,
                                0x83,
                                7,
                                State {
                                    selected,
                                    pressed: 3,
                                    overlay,
                                    opacity: 1.0,
                                },
                                allow,
                                0.0,
                            );
                            c.callback(kind, Vec2::new(31.0, 0.0));
                            c.end();
                        }
                    }
                }
            }
        }
    }
    for bits in gates {
        for flags in [0x1234, u32::MAX] {
            for indicator in [0, 1, 9] {
                for length in [19.0, 20.0, 20.00001, 100.0] {
                    for degrees in [
                        -180.0f32, -150.0, -120.0, -90.0, -60.0, -30.0, 0.0, 30.0, 60.0, 90.0,
                        120.0, 150.0, 180.0,
                    ] {
                        let radians = degrees.to_radians();
                        let velocity = Vec2::new(radians.cos(), radians.sin()) * length;
                        c.seed('N', bits, flags, 0x83, indicator, State::default(), 1, 0.0);
                        c.callback(6, velocity);
                        c.end();
                    }
                }
            }
        }
    }
    // Actual contact capture, cached frames, delayed press, cancellation, multi-touch,
    // movement before Up, directional gestures and ignored coordinates on release.
    for bits in gates {
        for flags in [0x1234, u32::MAX] {
            for direction in [(-100, 0), (100, 0), (0, -100), (0, 100)] {
                for flush in [false, true] {
                    c.seed('N', bits, flags, 0x80, 0, State::default(), 1, 0.0);
                    c.frame(
                        1,
                        10000,
                        true,
                        1 << 10,
                        0,
                        &[event(PointerKind::Down, 7, 400, 400)],
                    );
                    c.frame(
                        1,
                        50000,
                        true,
                        1 << 10,
                        0,
                        &[event(PointerKind::Cancel, 7, 0, 0)],
                    );
                    c.frame(2, 160000, true, 1 << 11, 0, &[]);
                    c.frame(
                        3,
                        200000,
                        true,
                        0,
                        0,
                        &[event(
                            PointerKind::Move,
                            7,
                            400 + direction.0,
                            400 + direction.1,
                        )],
                    );
                    c.frame(
                        4,
                        210000,
                        !flush,
                        1 << 24,
                        0,
                        &[event(PointerKind::Up, 7, -1000, -1000)],
                    );
                    c.frame(5, 250000, true, 0, 0, &[]);
                    c.end();
                }
            }
        }
    }
    for bits in gates {
        for point in [
            (100, 100),
            (952, 120),
            (1080, 120),
            (1208, 120),
            (1016, 200),
            (1144, 200),
        ] {
            c.seed(
                'N',
                bits,
                u32::MAX,
                0x80,
                1,
                State {
                    opacity: 1.0,
                    ..Default::default()
                },
                1,
                0.0,
            );
            c.frame(
                1,
                10000,
                true,
                1 << 10,
                0,
                &[event(PointerKind::Down, 7, point.0, point.1)],
            );
            c.frame(2, 160000, true, 1 << 11, 0, &[]);
            c.frame(
                3,
                200000,
                true,
                1 << 24,
                0,
                &[event(PointerKind::Up, 7, 0, 0)],
            );
            c.end();
            c.seed('N', bits, u32::MAX, 0x80, 0, State::default(), 1, 0.0);
            c.frame(
                1,
                10000,
                true,
                0,
                0,
                &[event(PointerKind::Down, 7, point.0, point.1)],
            );
            c.frame(
                2,
                20000,
                true,
                0,
                0,
                &[event(PointerKind::Down, 8, point.0 + 30, point.1)],
            );
            c.frame(
                3,
                30000,
                true,
                0,
                0,
                &[
                    event(PointerKind::Cancel, 7, 0, 0),
                    event(PointerKind::Cancel, 8, 0, 0),
                ],
            );
            c.end();
        }
    }
    let input = c.commands;
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_ROOT_TOUCH").unwrap())
        .arg("game-data/higurashi/uncompressed_executable_update")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
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
    for (i, (actual, expected)) in actual.iter().zip(&c.expected).enumerate() {
        assert_eq!(*actual, expected, "original operation {i}");
    }
    eprintln!(
        "Root touch:{probes} hint hit profiles;{} exact callback/contact/logical frame observations;native children and heap balanced",
        actual.len()
    );
}
