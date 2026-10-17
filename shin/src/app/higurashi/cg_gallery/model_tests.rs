use super::*;
use crate::adv::higurashi::mode_stack::{Mode, ModePush, ModeStack};
use shin_core::format::scenario::types::{U8String, U16SmallList};
use std::collections::VecDeque;
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/cg-gallery-model-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/cg-gallery-model-capture/output.txt"
));
const TABLES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../test-fixtures/higurashi/cg-gallery-tables.txt"
));
pub(super) fn catalog() -> Vec<PictureBoxInfoItem> {
    TABLES
        .lines()
        .filter(|s| s.starts_with("C "))
        .map(|s| {
            let mut w = s.split_whitespace();
            w.next();
            let _: usize = w.next().unwrap().parse().unwrap();
            let name = w.next().unwrap();
            let n: usize = w.next().unwrap().parse().unwrap();
            let pictures = w.map(|s| s.parse::<u16>().unwrap()).collect::<Vec<_>>();
            assert_eq!(pictures.len(), n);
            PictureBoxInfoItem {
                name: U8String::new(name),
                picture_ids: U16SmallList::from_contents(pictures),
            }
        })
        .collect()
}
pub(super) fn settings(profile: u32) -> SaveData {
    let mut s = SaveData::default();
    s.save_vectors.unlocked_cgs = (0..128)
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
            _ => unreachable!(),
        })
        .collect();
    s
}
struct Native {
    op: char,
    result: i32,
    header: Vec<u32>,
    clocks: Vec<u32>,
    entries: Vec<u32>,
    disabled: u32,
    cues: Vec<u32>,
    events: Vec<u32>,
    tasks: Vec<u32>,
    achievement: u32,
    text: Vec<u8>,
}
fn native(line: &str) -> Native {
    let mut w = line.split_whitespace();
    assert_eq!(w.next(), Some("G"));
    let op = w.next().unwrap().chars().next().unwrap();
    let result = w.next().unwrap().parse().unwrap();
    let mut q = w
        .map(|s| s.parse::<i64>().unwrap() as u32)
        .collect::<VecDeque<_>>();
    let header = q.drain(..10).collect();
    let clocks = q.drain(..64).collect();
    let n = q.pop_front().unwrap() as usize;
    let mut entries = vec![];
    for _ in 0..n {
        entries.extend(q.drain(..2));
        let count = q.pop_front().unwrap();
        entries.push(count);
        entries.extend(q.drain(..count as usize));
    }
    let disabled = q.pop_front().unwrap();
    let cues = q.drain(..4).collect();
    let events = q.drain(..11).collect();
    let n = q.pop_front().unwrap() as usize;
    let tasks = q.drain(..n * 10).collect();
    q.drain(..8);
    let achievement = q.pop_front().unwrap();
    let len = q.pop_front().unwrap() as usize;
    let text = q.drain(..len).map(|v| v as u8).collect();
    assert!(q.is_empty());
    Native {
        op,
        result,
        header,
        clocks,
        entries,
        disabled,
        cues,
        events,
        tasks,
        achievement,
        text,
    }
}
#[derive(Default)]
struct Platform {
    pressed: u32,
    repeat: u32,
    captured: bool,
    disabled: u32,
    cues: [u32; 4],
    queue: u32,
    plays: u32,
    stops: u32,
    bgm: i32,
    viewers: u32,
    index: u32,
    push_viewer: bool,
    achievements: u32,
}
impl Services for Platform {
    fn input(&mut self, m: &mut Model, p: bool) -> Result<()> {
        m.controls(
            p,
            self.captured,
            Buttons {
                pressed: self.pressed,
                repeat: self.repeat,
                ..Default::default()
            },
        );
        Ok(())
    }
    fn event(&mut self, a: Action) -> Result<()> {
        match a {
            Action::Cue(c) => {
                self.cues[match c {
                    "move" => 0,
                    "decide" => 1,
                    "cancel" => 2,
                    "error" => 3,
                    _ => panic!("CG original cue"),
                }] += 1
            }
            Action::DisableInput => self.disabled += 1,
            Action::Achievement(id) => {
                assert_eq!(id, 35);
                self.achievements += 1;
            }
            Action::PlayBgm { id, loops, fade } => {
                assert_eq!((id, loops, fade), (75, 0, 60));
                self.plays += 1;
                self.bgm = id;
            }
            Action::StopBgm(t) => {
                assert_eq!(t, 30);
                self.stops += 1;
            }
            Action::ReturnTitle { selected, mode } => {
                assert_eq!((selected, mode), (15, -1));
                self.queue += 1;
            }
            Action::OpenViewer(i) => {
                self.viewers += 1;
                self.index = i as u32;
                self.push_viewer = true;
            }
        }
        Ok(())
    }
}
enum Child {
    Private(Task),
    ViewerBoundary,
}
struct Poll<'a> {
    model: &'a mut Model,
    platform: &'a mut Platform,
    status: u32,
}
impl Mode<Poll<'_>> for Child {
    fn completion(&mut self) -> u32 {
        match self {
            Self::Private(t) => t.done.into(),
            Self::ViewerBoundary => 0,
        }
    }
    fn poll(
        &mut self,
        d: Ticks,
        p: bool,
        c: &mut Poll<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        if let Self::Private(t) = self {
            for child in t.poll_native(c.model, c.status, d, p, c.platform)? {
                pushes.push(Self::Private(child));
            }
        }
        if c.platform.push_viewer {
            c.platform.push_viewer = false;
            pushes.push(Self::ViewerBoundary);
        }
        Ok(0)
    }
}
struct Fixture {
    model: Model,
    platform: Platform,
    scope: ModeStack<Child>,
    status: u32,
}
impl Fixture {
    fn new(count: usize, profile: u32, capture: bool) -> Self {
        let all = catalog();
        let mut m = Model::new(&all[..count], &settings(profile), capture).unwrap();
        let mut scope = ModeStack::default();
        for child in m.take_pending_tasks() {
            scope.push(Child::Private(child));
        }
        let mut f = Self {
            model: m,
            platform: Platform {
                bgm: -1,
                ..Default::default()
            },
            scope,
            status: 0,
        };
        f.collect();
        f
    }
    fn collect(&mut self) {
        for a in self.model.take_actions() {
            self.platform.event(a).unwrap();
        }
        for t in self.model.take_pending_tasks() {
            self.scope.push(Child::Private(t));
        }
        if self.platform.push_viewer {
            self.platform.push_viewer = false;
            self.scope.push(Child::ViewerBoundary);
        }
    }
    fn frame(&mut self, delta: f32) {
        self.scope
            .poll(
                Ticks::from_f32(delta),
                1,
                &mut Poll {
                    model: &mut self.model,
                    platform: &mut self.platform,
                    status: self.status,
                },
            )
            .unwrap();
        self.model.advance_clocks(Ticks::from_f32(delta));
    }
    fn compare(&mut self, n: Native, result: i32, index: usize) {
        let m = &self.model;
        assert_eq!(result, n.result, "CG result{index}/{}", n.op);
        assert_eq!(
            vec![
                m.pages as u32,
                m.page as u32,
                m.x as u32,
                m.y as u32,
                m.closing.into(),
                m.frame,
                m.spin.to_bits(),
                m.contact as u32,
                m.hover as u32,
                m.gesture as u32
            ],
            n.header,
            "CG header{index}/{}",
            n.op
        );
        fn clock(c: &HigurashiTweener) -> [u32; 2] {
            [c.value().to_bits(), u32::from(!c.is_idle())]
        }
        let mut clocks = vec![];
        for i in 0..12 {
            clocks.extend(clock(&m.positions[i]));
            clocks.push(m.focus[i].to_bits());
            clocks.extend(clock(&m.hot[i]));
        }
        clocks.extend([0, 0]);
        clocks.extend(clock(&m.close_flash));
        assert_eq!(clocks, n.clocks, "CG complete grid clockwords{index}");
        let mut entries = vec![];
        for (i, e) in m.entries.iter().enumerate() {
            entries.extend([i as u32, e.locked().into(), e.unlocked.len() as u32]);
            entries.extend(e.unlocked.iter().map(|x| u32::from(*x)));
        }
        assert_eq!(
            entries, n.entries,
            "CG original unlock/bitvector records{index}"
        );
        let p = &mut self.platform;
        assert_eq!(p.disabled, n.disabled, "CG permission{index}");
        assert_eq!(p.cues.as_slice(), n.cues, "CG literal cues{index}");
        p.disabled = 0;
        p.cues = [0; 4];
        assert_eq!(
            vec![
                0,
                p.queue,
                if p.queue == 0 { 0 } else { 2 },
                if p.queue == 0 { 0 } else { 15 },
                if p.queue == 0 { 0 } else { u32::MAX },
                p.plays,
                p.stops,
                0,
                p.bgm as u32,
                p.viewers,
                p.index
            ],
            n.events,
            "CG return/BGM/checkedViewer events{index}"
        );
        let tasks = self
            .scope
            .iter()
            .flat_map(|c| match c {
                Child::ViewerBoundary => vec![u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                Child::Private(t) => {
                    let (table, arg) = match t.kind {
                        Kind::First(phase) => (0x1aee90, phase as u32),
                        Kind::Input => (0x1af078, 0),
                        Kind::Page { phase, .. } => (0x1aeec8, phase as u32),
                        Kind::ViewerWait { .. } => (0x1af1a8, 0),
                        Kind::ExitWait => (0x1af128, 0),
                    };
                    let mut v = vec![table, t.done.into(), arg];
                    if let Some(c) = &t.capture {
                        v.extend([
                            c.active.into(),
                            c.elapsed.to_bits(),
                            c.duration.to_bits(),
                            c.source_present.into(),
                            c.mask_present.into(),
                            c.softness.to_bits(),
                            c.flags,
                        ]);
                    } else if let Kind::Page {
                        destination,
                        direction,
                        ..
                    } = t.kind
                    {
                        v.extend([destination as u32, direction as u32, 0, 0, 0, 0, 0]);
                    } else {
                        v.extend([0; 7]);
                    }
                    v
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(tasks, n.tasks, "CG borrowed scope/task membership{index}");
        assert_eq!(p.achievements, n.achievement);
        assert_eq!(
            m.completion.as_bytes(),
            n.text,
            "CG completion literal{index}"
        );
    }
}
#[test]
fn reference_updated_cg_gallery_original368_owners3619_observations() {
    let mut f = None;
    let mut out = OUTPUT.lines();
    let mut owners = 0;
    let mut count = 0;
    for line in INPUT.lines() {
        let mut w = line.split_whitespace();
        let op = w.next().unwrap().chars().next().unwrap();
        fn u(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            w.next().unwrap().parse().unwrap()
        }
        fn h(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            u32::from_str_radix(w.next().unwrap(), 16).unwrap()
        }
        fn x(w: &mut std::str::SplitWhitespace<'_>) -> f32 {
            w.next().unwrap().parse().unwrap()
        }
        if op == 'X' {
            assert_eq!(out.next(), Some("X"));
            f = None;
            continue;
        }
        if op == 'N' {
            let n = u(&mut w) as usize;
            let p = u(&mut w);
            let c = h(&mut w) != 0;
            f = Some(Fixture::new(n, p, c));
            owners += 1;
        }
        let f = f.as_mut().unwrap();
        let mut result = -1;
        match op {
            'N' => {}
            'R' => {
                f.model.publish_resources();
                f.status = 2;
                result = 1;
            }
            'F' => {
                let d = x(&mut w);
                f.platform.pressed = h(&mut w);
                f.platform.repeat = h(&mut w);
                f.platform.captured = u(&mut w) != 0;
                f.frame(d);
                result = 0;
            }
            'U' => {
                let p = u(&mut w) != 0;
                let c = u(&mut w) != 0;
                let pressed = h(&mut w);
                let repeat = h(&mut w);
                result = f
                    .model
                    .controls(
                        p,
                        c,
                        Buttons {
                            pressed,
                            repeat,
                            ..Default::default()
                        },
                    )
                    .into();
            }
            'S' => {
                f.model.page = u(&mut w) as usize;
                f.model.x = u(&mut w) as usize;
                f.model.y = u(&mut w) as usize;
            }
            'T' => {
                result = f
                    .model
                    .probe_contact(Vec2::new(x(&mut w), x(&mut w)))
                    .into();
            }
            'G' => f.model.gesture_contact(Vec2::new(x(&mut w), x(&mut w))),
            'P' => f.model.press_contact(),
            'E' => f.model.release_contact(),
            'H' => {}
            'K' => f.model.drag_contact(),
            'Q' => f.model.cancel_contact(),
            'Z' => f.model.reset_contact(),
            _ => panic!("CG opcode"),
        }
        f.collect();
        count += 1;
        f.compare(native(out.next().unwrap()), result, count);
    }
    assert_eq!((owners, count), (368, 3619));
    assert!(out.next().is_none());
}
#[test]
#[ignore = "requires explicit frozen original runner and supplied assets; no GPU/audio/window/save"]
fn reference_updated_cg_gallery_frozen_original_reexecution() {
    let exe = std::env::var("SHIN_HIGURASHI_CG_GALLERY_MODEL_CAPTURE")
        .expect("explicit original runner is required");
    use std::io::Write;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut child = std::process::Command::new(exe)
        .args([
            root.join("higurashi/uncompressed_executable_update"),
            std::env::var_os("SHIN_HIGURASHI_FONT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "/tmp/higurashi-newrodin.fnt".into()),
            std::env::var_os("SHIN_HIGURASHI_SNR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "/tmp/higurashi-scenario-patch.rom.snr".into()),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(INPUT.as_bytes()).unwrap());
    let o = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, OUTPUT.as_bytes());
}
