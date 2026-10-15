use super::*;
use crate::adv::higurashi::mode_stack::{Mode, ModePush, ModeStack};
use std::collections::VecDeque;
const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/cg-gallery-viewer-model-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/cg-gallery-viewer-model-capture/output.txt"
));
struct Native {
    op: char,
    result: i32,
    present: bool,
    status: u32,
    header: Vec<u32>,
    jobs: Vec<u32>,
    picture: Vec<u32>,
    capture: Vec<u32>,
    tasks: Vec<u32>,
    counters: Vec<u32>,
    captures: Vec<u32>,
    files: Vec<u32>,
    lookups: Vec<u32>,
}
fn native(line: &str) -> Native {
    let mut w = line.split_whitespace();
    assert_eq!(w.next(), Some("V"));
    let op = w.next().unwrap().chars().next().unwrap();
    let result = w.next().unwrap().parse().unwrap();
    let mut q = w
        .map(|s| s.parse::<i64>().unwrap() as u32)
        .collect::<VecDeque<_>>();
    let present = q.pop_front().unwrap() != 0;
    let (mut status, mut header, mut jobs, mut picture, mut capture, mut tasks) =
        (0, vec![], vec![], vec![], vec![], vec![]);
    if present {
        status = q.pop_front().unwrap();
        header = q.drain(..7).collect();
        jobs = q.drain(..4).collect();
        picture = q.drain(..142).collect();
        capture = q.drain(..7).collect();
        let n = q.pop_front().unwrap() as usize;
        tasks = q.drain(..2 * n).collect();
    }
    let counters = q.drain(..19).collect();
    let n = q.pop_front().unwrap() as usize;
    let captures = q.drain(..5 * n).collect();
    let n = q.pop_front().unwrap() as usize;
    let files = q.drain(..n).collect();
    let n = q.pop_front().unwrap() as usize;
    let lookups = q.drain(..n).collect();
    assert!(q.is_empty());
    Native {
        op,
        result,
        present,
        status,
        header,
        jobs,
        picture,
        capture,
        tasks,
        counters,
        captures,
        files,
        lookups,
    }
}
pub(super) struct Platform {
    input: Input,
    captured: bool,
    metadata: Metadata,
    generation: u32,
    status: u32,
    prepared: Option<Picture>,
    os: u32,
    pictures: u32,
    loads: u32,
    requested: u16,
    lookups: Vec<u32>,
    files: Vec<u32>,
    captures: Vec<u32>,
    disabled: u32,
    cues: [u32; 4],
}
impl Services for Platform {
    fn prepare_picture(&mut self, id: u16) -> Result<()> {
        self.generation += 1;
        self.status = 0;
        self.prepared = None;
        self.loads += 1;
        self.requested = id;
        self.lookups.push(u32::from(id));
        Ok(())
    }
    fn worker_status(&mut self) -> u32 {
        self.status
    }
    fn take_prepared(&mut self) -> Result<Picture> {
        self.prepared.take().ok_or_else(|| {
            anyhow::anyhow!("Checked original Picture worker must supply one ready owner")
        })
    }
    fn capture_parent(&mut self, m: &Model) -> Result<()> {
        assert!(
            !m.show_info,
            "Original capture hides badges before parent render"
        );
        let id = self.captures.len() / 5 + 1;
        self.captures.extend([
            m.closing.into(),
            m.picture.as_ref().map_or(0, |p| p.identity as u32),
            self.prepared.as_ref().map_or(0, |p| p.identity as u32),
            id as u32,
            self.status,
        ]);
        Ok(())
    }
    fn publish_capture(&mut self, _: &Model) -> Result<()> {
        // Packet/model transport owns no bitmap; real publication is checked
        // by the concrete owner acceptance tests.
        Ok(())
    }
    fn input(&mut self, m: &mut Model, p: bool) -> Result<()> {
        m.controls(p, self.captured, self.input, self)?;
        Ok(())
    }
    fn cue(&mut self, c: Cue) -> Result<()> {
        self.cues[match c {
            Cue::Decide => 1,
            Cue::Cancel => 2,
        }] += 1;
        Ok(())
    }
    fn disable_input(&mut self) {
        self.disabled += 1;
    }
}
struct Poll<'a> {
    model: &'a mut Model,
    platform: &'a mut Platform,
}
struct Private(Task);
impl Mode<Poll<'_>> for Private {
    fn completion(&mut self) -> u32 {
        self.0.done.into()
    }
    fn poll(
        &mut self,
        d: Ticks,
        p: bool,
        c: &mut Poll<'_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        for t in self.0.poll_native(c.model, d, p, c.platform)? {
            pushes.push(Self(t));
        }
        Ok(0)
    }
}
struct Owner {
    model: Model,
    scope: ModeStack<Private>,
}
enum Parent {
    GalleryInput,
    Viewer(Owner),
}
impl Mode<Platform> for Parent {
    fn completion(&mut self) -> u32 {
        match self {
            Self::GalleryInput => 0,
            Self::Viewer(o) => o.scope.is_empty().into(),
        }
    }
    fn poll(
        &mut self,
        d: Ticks,
        p: bool,
        s: &mut Platform,
        _: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::GalleryInput => {
                if !p {
                    s.disable_input();
                }
            }
            Self::Viewer(o) => {
                o.scope.poll(
                    d,
                    u32::from(p),
                    &mut Poll {
                        model: &mut o.model,
                        platform: s,
                    },
                )?;
                o.model.advance_clocks(d);
            }
        }
        Ok(0)
    }
}
pub(super) struct Fixture {
    parent: ModeStack<Parent>,
    platform: Platform,
    group: u32,
}
impl Fixture {
    pub(super) fn new(group: u32, profile: u32, metadata: Metadata) -> Self {
        let all = super::super::model_tests::catalog();
        let grid =
            super::super::Model::new(&all, &super::super::model_tests::settings(profile), true)
                .unwrap();
        let entry = grid.entries[group as usize].clone();
        // Native N has already executed the independently frozen gallery A/hot-wait
        // before creating this Viewer. Its one completed First job is generation1.
        let mut platform = Platform {
            input: Input::default(),
            captured: false,
            metadata,
            generation: 1,
            status: 0,
            prepared: None,
            os: 0,
            pictures: 0,
            loads: 0,
            requested: 0,
            lookups: vec![],
            files: vec![],
            captures: vec![],
            disabled: 2,
            cues: [0, 1, 0, 0],
        };
        let mut model = Model::new(entry, &mut platform).unwrap();
        let mut private = ModeStack::default();
        for t in model.take_pending_tasks() {
            private.push(Private(t));
        }
        let mut parent = ModeStack::default();
        parent.push(Parent::GalleryInput);
        parent.push(Parent::Viewer(Owner {
            model,
            scope: private,
        }));
        Self {
            parent,
            platform,
            group,
        }
    }
    fn owner(&self) -> Option<&Owner> {
        self.parent.iter().find_map(|p| {
            if let Parent::Viewer(o) = p {
                Some(o)
            } else {
                None
            }
        })
    }
    fn owner_mut(&mut self) -> &mut Owner {
        self.parent
            .iter_mut()
            .find_map(|p| {
                if let Parent::Viewer(o) = p {
                    Some(o)
                } else {
                    None
                }
            })
            .expect("Native Viewer still retained")
    }
    pub(super) fn direct(
        &mut self,
        run: impl FnOnce(&mut Model, &mut Platform) -> Result<i32>,
    ) -> i32 {
        let Self {
            parent, platform, ..
        } = self;
        let o = parent
            .iter_mut()
            .find_map(|p| {
                if let Parent::Viewer(o) = p {
                    Some(o)
                } else {
                    None
                }
            })
            .unwrap();
        let r = run(&mut o.model, platform).unwrap();
        for t in o.model.take_pending_tasks() {
            o.scope.push(Private(t));
        }
        r
    }
    pub(super) fn worker(&mut self) {
        self.platform.files.push(u32::from(self.platform.requested));
        self.platform.pictures += 1;
        let seeds = std::array::from_fn(|_| {
            self.platform.os += 1;
            0xc7100000 + self.platform.os
        });
        self.platform.prepared = Some(Picture::from_native_metadata(
            self.platform.pictures as u64,
            self.platform.metadata,
            seeds,
        ));
        self.platform.status = 2;
    }
    pub(super) fn tick(&mut self, delta: f32) {
        self.parent
            .poll(Ticks::from_f32(delta), 1, &mut self.platform)
            .unwrap();
    }
    pub(super) fn model(&self) -> &Model {
        &self.owner().unwrap().model
    }
    pub(super) fn model_mut(&mut self) -> &mut Model {
        &mut self.owner_mut().model
    }
    fn compare(&mut self, n: Native, result: i32, index: usize) {
        assert_eq!(result, n.result, "Viewer result{index}/{}", n.op);
        let o = self.owner();
        assert_eq!(
            o.is_some(),
            n.present,
            "Viewer actual parent retention{index}"
        );
        if let Some(o) = o {
            let m = &o.model;
            assert_eq!(
                u32::from(o.scope.is_empty()),
                n.status,
                "Viewer private-empty completion{index}"
            );
            assert_eq!(
                vec![
                    u32::from(m.closing) | (u32::from(m.show_info) << 8) | (u32::from(m.fit) << 16),
                    self.group,
                    m.variant as u32,
                    m.velocity.x.to_bits(),
                    m.velocity.y.to_bits(),
                    m.offset.x.to_bits(),
                    m.offset.y.to_bits()
                ],
                n.header,
                "Viewer flags/variant/full pan state{index}"
            );
            assert_eq!(
                vec![
                    self.platform.generation,
                    self.platform.status,
                    self.platform
                        .prepared
                        .as_ref()
                        .map_or(0, |p| p.identity as u32),
                    m.picture.as_ref().map_or(0, |p| p.identity as u32)
                ],
                n.jobs,
                "Viewer actual ready-publication identity{index}"
            );
            let mut picture = vec![];
            if let Some(p) = &m.picture {
                picture.push(1);
                picture.push(
                    u32::from(p.metadata.origin[0] as u16)
                        | (u32::from(p.metadata.origin[1] as u16) << 16),
                );
                picture.push(
                    u32::from(p.metadata.extent[0]) | (u32::from(p.metadata.extent[1]) << 16),
                );
                picture.push(0);
                for c in &p.properties.oscillators {
                    picture.extend([c.seed, c.phase.to_bits()]);
                }
                for i in 0..65 {
                    let c = p.properties.tweener(i).unwrap();
                    picture.extend([c.value().to_bits(), u32::from(!c.is_idle())]);
                }
            } else {
                picture.resize(142, 0);
            }
            assert_eq!(
                picture, n.picture,
                "Viewer native fourteen-a8 header/4seeds/full65clock owners{index}"
            );
            let c = &m.capture;
            assert_eq!(
                vec![
                    c.active.into(),
                    c.elapsed.to_bits(),
                    c.duration.to_bits(),
                    c.source_present.into(),
                    c.mask_present.into(),
                    c.softness.to_bits(),
                    c.flags
                ],
                n.capture,
                "Viewer retained capture clock{index}"
            );
            let tasks = o
                .scope
                .iter()
                .flat_map(|p| {
                    [
                        match p.0.kind {
                            Kind::Input => 0x1af2a8,
                            Kind::Ready => 0x1af3d8,
                            Kind::CaptureWait => 0x1af448,
                            Kind::CloseWait => 0x1af4d8,
                        },
                        p.0.done.into(),
                    ]
                })
                .collect::<Vec<_>>();
            assert_eq!(tasks, n.tasks, "Viewer private borrowed scope{index}");
        }
        assert_eq!(
            self.platform.os, n.counters[0],
            "Viewer four worker OS samples{index}"
        );
        assert_eq!(
            self.platform.generation, n.counters[1],
            "Viewer scoped worker prepare count{index}"
        );
        assert_eq!(self.platform.pictures, n.counters[5]);
        assert_eq!(
            self.platform.pictures, n.counters[7],
            "Viewer original file request count{index}"
        );
        assert_eq!(self.platform.captures.len() / 5, n.counters[8] as usize);
        assert_eq!(
            self.platform.disabled, n.counters[13],
            "Viewer/parent denied permission{index}"
        );
        assert_eq!(
            n.counters[14], 0,
            "Viewer does not sample engine seed clock"
        );
        assert_eq!(
            &self.platform.cues,
            n.counters[15..19].as_ref(),
            "Viewer literal controller/pointer cues{index}"
        );
        assert_eq!(
            self.platform.captures, n.captures,
            "Viewer exact capture-before-picture-swap/close order{index}"
        );
        assert_eq!(
            self.platform.files, n.files,
            "Viewer actual picture filenames/variant IDs{index}"
        );
        assert_eq!(
            self.platform.lookups, n.lookups,
            "Viewer executed original metadata lookup IDs{index}"
        );
        self.platform.disabled = 0;
        self.platform.cues = [0; 4];
    }
}
#[test]
fn reference_updated_cg_viewer_original144_owners2363_observations() {
    let mut f = None;
    let mut out = OUTPUT.lines();
    let (mut owners, mut observations) = (0, 0);
    for line in INPUT.lines() {
        let mut w = line.split_whitespace();
        let op = w.next().unwrap().chars().next().unwrap();
        fn u(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
            w.next().unwrap().parse().unwrap()
        }
        fn i(w: &mut std::str::SplitWhitespace<'_>) -> i32 {
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
            let group = u(&mut w);
            let profile = u(&mut w);
            let width = u(&mut w);
            let height = u(&mut w);
            assert_eq!(width * 9 / 16, height);
            let metadata = Metadata {
                origin: [i(&mut w) as i16, i(&mut w) as i16],
                extent: [u(&mut w) as u16, u(&mut w) as u16],
            };
            f = Some(Fixture::new(group, profile, metadata));
            owners += 1;
        }
        let f = f.as_mut().unwrap();
        let mut result = -1;
        match op {
            'N' => {}
            'R' => {
                f.worker();
                result = 1;
            }
            'F' => {
                let delta = x(&mut w);
                let held = h(&mut w);
                let pressed = h(&mut w);
                let repeat = h(&mut w);
                let axes = [i(&mut w) as i8, i(&mut w) as i8];
                f.platform.input = Input {
                    buttons: Buttons {
                        held,
                        pressed,
                        repeat,
                        ..Default::default()
                    },
                    first_stick: axes,
                };
                f.platform.captured = u(&mut w) != 0;
                f.parent
                    .poll(Ticks::from_f32(delta), 1, &mut f.platform)
                    .unwrap();
            }
            'U' => {
                let permission = u(&mut w) != 0;
                let captured = u(&mut w) != 0;
                let held = h(&mut w);
                let pressed = h(&mut w);
                let axes = [i(&mut w) as i8, i(&mut w) as i8];
                result = f.direct(|m, p| {
                    Ok(m.controls(
                        permission,
                        captured,
                        Input {
                            buttons: Buttons {
                                held,
                                pressed,
                                ..Default::default()
                            },
                            first_stick: axes,
                        },
                        p,
                    )? as i32)
                });
            }
            'T' => {
                let position = Vec2::new(x(&mut w), x(&mut w));
                result = f.owner_mut().model.probe_contact(position).into();
            }
            'G' => {
                let v = Vec2::new(x(&mut w), x(&mut w));
                f.owner_mut().model.gesture_contact(v);
            }
            'K' => {
                let a = Vec2::new(x(&mut w), x(&mut w));
                let b = Vec2::new(x(&mut w), x(&mut w));
                f.owner_mut().model.drag_contact(a, b);
            }
            'E' => {
                f.direct(|m, p| {
                    m.release_contact(p)?;
                    Ok(-1)
                });
            }
            'P' | 'Q' | 'Z' | 'H' => {}
            _ => panic!("Viewer opcode"),
        }
        observations += 1;
        f.compare(native(out.next().unwrap()), result, observations);
    }
    assert_eq!((owners, observations), (144, 2363));
    assert!(out.next().is_none());
}
#[test]
#[ignore = "requires explicit frozen original runner and supplied assets; no GPU/audio/window/save"]
fn reference_updated_cg_viewer_frozen_original_reexecution() {
    let exe = std::env::var("SHIN_HIGURASHI_CG_GALLERY_VIEWER_MODEL_CAPTURE")
        .expect("explicit original runner is required");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    use std::io::Write;
    let mut child = std::process::Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg("/tmp/higurashi-newrodin.fnt")
        .arg("/tmp/higurashi-scenario-patch.rom.snr")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(INPUT.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, OUTPUT.as_bytes());
}
