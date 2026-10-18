//! Source-only candidate acceptance. Native reexecution is mandatory when the
//! ignored test is explicitly selected. No test starts audio, a window or GPU.
use super::*;
use crate::adv::higurashi::mode_stack::{Mode, ModePush, ModeStack};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(super) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
pub(super) fn evidence() -> PathBuf {
    root().join("test-fixtures/higurashi")
}
pub(super) fn required(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("Set {name}; native checks cannot be skipped")),
    )
}
pub(super) fn scenario() -> HigurashiScenario {
    HigurashiScenario::new(
        std::fs::read(required("SHIN_HIGURASHI_MUSIC_SCENARIO"))
            .unwrap()
            .into(),
    )
    .unwrap()
}
pub(super) fn unlocks(profile: u32) -> Vec<u32> {
    (0..8)
        .map(|i| match profile {
            0 => 0,
            1 => u32::MAX,
            2 => 0x55555555,
            3 => {
                if i % 3 == 0 {
                    u32::MAX
                } else {
                    0
                }
            }
            _ => panic!("Unlock fixture"),
        })
        .collect()
}
/// Execute the original runner against the immutable corpus and require the
/// entire new output to equal the frozen capture BEFORE comparing Rust state.
pub(super) fn reexecute(kind: &str, variable: &str) -> String {
    let dir = evidence().join(format!("music-gallery-{kind}-capture"));
    let mut child = Command::new(required(variable))
        .arg(root().join("game-data/higurashi/uncompressed_executable_update"))
        .arg(required("SHIN_HIGURASHI_MUSIC_FONT"))
        .arg(required("SHIN_HIGURASHI_MUSIC_SCENARIO"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = std::fs::read(dir.join("input.txt")).unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&input).unwrap();
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "Original runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        std::fs::read(dir.join("output.txt")).unwrap(),
        "Original capture drift"
    );
    assert_eq!(
        output.stderr,
        std::fs::read(dir.join("stderr.txt")).unwrap(),
        "Original service/balance drift"
    );
    String::from_utf8(output.stdout).unwrap()
}
struct Native {
    op: char,
    result: i32,
    scalar: Vec<u32>,
    rows: Vec<Vec<u32>>,
    scope: Vec<Vec<u32>>,
    text: String,
}
impl Native {
    fn parse(line: &str) -> Self {
        let mut t = line.split_whitespace();
        assert_eq!(t.next(), Some("M"));
        let op = t.next().unwrap().chars().next().unwrap();
        let result = t.next().unwrap().parse().unwrap();
        let mut t = t.map(|x| x.parse::<u32>().unwrap());
        let scalar = t.by_ref().take(55).collect();
        let n = t.next().unwrap();
        let rows = (0..n).map(|_| t.by_ref().take(6).collect()).collect();
        let n = t.next().unwrap();
        let scope = (0..n).map(|_| t.by_ref().take(10).collect()).collect();
        let n = t.next().unwrap();
        // Native resource/event transport is verified by full-output equality;
        // this pure-model comparator checks its own observable service subset.
        for _ in 0..n * 3 {
            t.next().unwrap();
        }
        let n = t.next().unwrap();
        let text =
            String::from_utf8(t.by_ref().take(n as usize).map(|x| x as u8).collect()).unwrap();
        assert_eq!(t.next(), None);
        Self {
            op,
            result,
            scalar,
            rows,
            scope,
            text,
        }
    }
}
#[derive(Default)]
struct Events {
    achievements: u32,
    queue: u32,
    plays: u32,
    bgm: u32,
    once: u32,
    fade: u32,
    gain: u32,
    stop30: u32,
    stop60: u32,
    platform: u32,
    disabled: u32,
    cues: [u32; 4],
}
impl Events {
    fn fresh() -> Self {
        Self {
            bgm: u32::MAX,
            ..Self::default()
        }
    }
    fn event(&mut self, action: Action) {
        match action {
            Action::Achievement(34) => self.achievements += 1,
            Action::ReturnTitle {
                selected: 16,
                mode: -1,
            } => self.queue += 1,
            Action::PlayBgm {
                id,
                once,
                fade,
                gain,
            } => {
                self.plays += 1;
                self.bgm = u32::from(id);
                self.once = u32::from(once);
                self.fade = fade;
                self.gain = gain.to_bits();
            }
            Action::StopBgm(30) => self.stop30 += 1,
            Action::StopBgm(60) => self.stop60 += 1,
            Action::PlatformByte => self.platform += 1,
            Action::DisableInput => self.disabled += 1,
            Action::Cue(name) => {
                self.cues[match name {
                    "move" => 0,
                    "decide" => 1,
                    "cancel" => 2,
                    "error" => 3,
                    _ => panic!("Cue"),
                }] += 1
            }
            _ => panic!("Unexpected Music action: {action:?}"),
        }
    }
}
struct Poll<'a> {
    model: &'a mut Model,
    events: &'a mut Events,
    status: u32,
    mask: bool,
    captured: bool,
    buttons: Buttons,
}
impl Services for Poll<'_> {
    fn job_status(&mut self) -> Result<u32> {
        Ok(self.status)
    }
    fn mask_published(&self) -> bool {
        self.mask
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> bool {
        model.controller(permission, self.captured, self.buttons)
    }
    fn event(&mut self, a: Action) {
        self.events.event(a);
    }
}
impl Mode<Poll<'_>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.complete())
    }
    fn poll(
        &mut self,
        d: Ticks,
        permission: bool,
        s: &mut Poll<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        // Separate reborrow avoids passing the service's Model twice. Input
        // service contains only independent transport fields for this call.
        struct Transport<'a> {
            status: u32,
            mask: bool,
            captured: bool,
            buttons: Buttons,
            events: &'a mut Events,
        }
        impl Services for Transport<'_> {
            fn job_status(&mut self) -> Result<u32> {
                Ok(self.status)
            }
            fn mask_published(&self) -> bool {
                self.mask
            }
            fn input(&mut self, m: &mut Model, p: bool) -> bool {
                m.controller(p, self.captured, self.buttons)
            }
            fn event(&mut self, a: Action) {
                self.events.event(a);
            }
        }
        let mut transport = Transport {
            status: s.status,
            mask: s.mask,
            captured: s.captured,
            buttons: s.buttons,
            events: s.events,
        };
        for t in self.poll_native(s.model, d, permission, &mut transport)? {
            pushes.push(t);
        }
        Ok(0)
    }
}
struct Fixture {
    model: Model,
    scope: ModeStack<Task>,
    events: Events,
    status: u32,
    atlas: bool,
    bgm_status: u32,
}
impl Fixture {
    fn new(model: Model, capture: bool) -> Self {
        let mut scope = ModeStack::default();
        scope.push(Task::first(capture));
        let mut f = Self {
            model,
            scope,
            events: Events::fresh(),
            status: 0,
            atlas: false,
            bgm_status: 0,
        };
        f.absorb();
        f
    }
    fn absorb(&mut self) {
        for event in self.model.take_actions() {
            self.events.event(event);
        }
        for task in self.model.take_pending_tasks() {
            self.scope.push(task);
        }
    }
    fn frame(&mut self, delta: f32, buttons: Buttons, captured: bool) {
        let delta = Ticks::from_f32(delta);
        self.scope
            .poll(
                delta,
                1,
                &mut Poll {
                    model: &mut self.model,
                    events: &mut self.events,
                    status: self.status,
                    mask: self.atlas,
                    captured,
                    buttons,
                },
            )
            .unwrap();
        self.model
            .advance_clocks(delta, self.atlas, self.bgm_status);
        self.absorb();
    }
    fn check(&mut self, n: Native, profile: usize, index: usize) {
        let ctx = format!("Music owner{profile} observation{index} op{}", n.op);
        let m = &self.model;
        let core = vec![
            u32::from(m.append),
            u32::from(self.atlas),
            m.document_height as u32,
            m.selected as u32,
            m.playing as u32,
            u32::from(m.closing),
            m.elapsed.to_bits(),
            m.region as u32,
            m.hover as u32,
            m.contact as u32,
            m.velocity.to_bits(),
            m.scroll.value().to_bits(),
            u32::from(!m.scroll.is_idle()),
            m.close_flash.value().to_bits(),
            u32::from(!m.close_flash.is_idle()),
            m.scrollbar.position as u32,
            m.scrollbar.visible as u32,
            m.scrollbar.first as u32,
            m.scrollbar.end as u32,
        ];
        assert_eq!(
            core,
            n.scalar[..19],
            "{ctx} model and pre-advance scrollbar"
        );
        assert_eq!(m.completion_text, n.text, "{ctx} real catalog completion");
        let rows: Vec<_> = m
            .rows
            .iter()
            .map(|r| {
                vec![
                    r.catalog_index as u32,
                    u32::from(r.name),
                    u32::from(r.unlocked),
                    u32::from(r.append),
                    r.flash.value().to_bits(),
                    u32::from(!r.flash.is_idle()),
                ]
            })
            .collect();
        assert_eq!(rows, n.rows, "{ctx} all retained rows");
        let scope: Vec<_> = self
            .scope
            .iter()
            .map(|t| match t {
                Task::First { phase, overlay: o } => vec![
                    0x1ae2f8,
                    u32::from(t.complete()),
                    *phase as u32,
                    u32::from(o.active),
                    o.elapsed.to_bits(),
                    o.duration.to_bits(),
                    u32::from(o.source_present),
                    u32::from(o.mask_present),
                    o.softness.to_bits(),
                    o.flags,
                ],
                Task::Input { done } => vec![0x1ae3d0, u32::from(*done), 0, 0, 0, 0, 0, 0, 0, 0],
                Task::Close { done } => vec![0x1ae490, u32::from(*done), 0, 0, 0, 0, 0, 0, 0, 0],
            })
            .collect();
        assert_eq!(scope, n.scope, "{ctx} actual private scope");
        let e = &mut self.events;
        assert_eq!(n.scalar[34], e.achievements, "{ctx} achievement34");
        assert_eq!(n.scalar[35], 0, "{ctx} original no MT requests");
        assert_eq!(n.scalar[36], e.queue, "{ctx} actual Title request count");
        if e.queue != 0 {
            assert_eq!(&n.scalar[38..40], &[16, u32::MAX], "{ctx} Title16/mode-1");
        }
        assert_eq!(
            &n.scalar[40..47],
            &[e.plays, e.bgm, e.once, e.fade, e.gain, e.stop30, e.stop60],
            "{ctx} exact audio args"
        );
        assert_eq!(n.scalar[47], self.bgm_status, "{ctx} status fixture");
        assert_eq!(n.scalar[48], e.platform, "{ctx} platform calls");
        assert_eq!(n.scalar[49], 1, "{ctx} text constructed once");
        assert_eq!(n.scalar[50], e.disabled, "{ctx} contact disable");
        assert_eq!(&n.scalar[51..55], &e.cues, "{ctx} cues");
        e.disabled = 0;
        e.cues = [0; 4];
    }
}
#[test]
#[ignore = "331 actual original owners/2607 observations; requires SHIN_HIGURASHI_MUSIC_MODEL_NATIVE, MUSIC_FONT and MUSIC_SCENARIO; noGPU/audio"]
fn higurashi_music_model_matches_actual_native_331_owners() {
    let output = reexecute("model", "SHIN_HIGURASHI_MUSIC_MODEL_NATIVE");
    let mut observations = output
        .lines()
        .filter(|l| l.starts_with("M "))
        .map(Native::parse);
    let scenario = scenario();
    assert_eq!(scenario.music_box_info.len(), 158);
    let source =
        std::fs::read_to_string(evidence().join("music-gallery-model-capture/input.txt")).unwrap();
    let mut fixture: Option<Fixture> = None;
    let (mut owners, mut checked, mut rows) = (0, 0, 0);
    for (i, line) in source.lines().enumerate() {
        let mut words = line.split_whitespace();
        let op = words.next().unwrap();
        let args: Vec<_> = words.collect();
        let f = |i: usize| args[i].parse::<f32>().unwrap();
        let u = |i: usize| args[i].parse::<u32>().unwrap();
        let hex = |i: usize| u32::from_str_radix(args[i], 16).unwrap();
        let n = observations.next().unwrap();
        assert_eq!(n.op.to_string(), op);
        if op == "N" {
            assert!(fixture.is_none());
            fixture = Some(Fixture::new(
                Model::new(&scenario.music_box_info, &unlocks(u(0)), u(1) != 0).unwrap(),
                u(2) != 0,
            ));
            owners += 1;
        } else if op == "X" {
            // Actual native deletion/balance is required by reexecution; this
            // candidate has no worker/GPU owner and makes no lifetime claim.
            fixture.take().unwrap();
            assert!(n.rows.is_empty() && n.scope.is_empty());
            checked += 1;
            continue;
        } else {
            let h = fixture.as_mut().unwrap();
            match op {
                "R" => {
                    h.atlas = true;
                    if u(0) != 0 {
                        h.status = 2;
                    }
                }
                "J" => h.status = u(0),
                "B" => h.bgm_status = u(0),
                "F" => h.frame(
                    f(0),
                    Buttons {
                        pressed: hex(1),
                        repeat: hex(2),
                        ..Buttons::default()
                    },
                    u(3) != 0,
                ),
                "U" => {
                    let result = h.model.controller(
                        u(0) != 0,
                        u(1) != 0,
                        Buttons {
                            pressed: hex(2),
                            repeat: hex(3),
                            ..Buttons::default()
                        },
                    );
                    assert_eq!(i32::from(result), n.result);
                    h.absorb();
                }
                "S" => {
                    h.model.selected = u(0) as i32;
                    h.model.scroll.fast_forward_to(f(1));
                }
                "T" => assert_eq!(i32::from(h.model.probe(Vec2::new(f(0), f(1)))), n.result),
                "P" => h.model.press(Vec2::ZERO),
                "E" => h.model.release(Vec2::ZERO),
                "H" => h.model.long_press(Vec2::ZERO),
                "Z" => h.model.reset(),
                "Q" => h.model.cancel(),
                "D" => h.model.drag(Vec2::new(f(0), f(1)), Vec2::new(f(2), f(3))),
                "I" => h.model.gesture(Vec2::new(f(0), f(1))),
                _ => panic!("Unknown native protocol operation"),
            }
            h.absorb();
        }
        let h = fixture.as_mut().unwrap();
        rows += h.model.rows.len();
        h.check(n, owners - 1, i);
        checked += 1;
    }
    assert!(fixture.is_none());
    assert!(observations.next().is_none());
    assert_eq!((owners, checked, rows), (331, 2607, 359608));
    println!(
        "Music:331 actual owners/2607 observations/359608 exact retained-row snapshots; full native runner output byte-identical; CPU candidate only"
    );
}
