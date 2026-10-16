use super::*;
use crate::adv::higurashi::character_selection::Model;
use crate::asset::texture_archive::{TextureArchive, TextureArchiveBuilder};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
#[derive(Clone, Debug)]
struct Case {
    kind: i32,
    old: bool,
    append: bool,
    route: bool,
    pattern: u32,
    params: [i32; 8],
}
#[derive(Clone, Debug)]
enum Op {
    New(Case),
    Poll(usize),
    Advance(usize, f32),
    Fast(usize),
    Refresh(usize),
    Input(usize, bool, u32, u32),
    Copy(usize, usize),
    Render(usize),
    Question(usize, i32),
    Drop(usize),
}
impl Op {
    fn slot(&self) -> usize {
        match self {
            Self::New(_) => 0,
            Self::Poll(i)
            | Self::Advance(i, _)
            | Self::Fast(i)
            | Self::Refresh(i)
            | Self::Input(i, ..)
            | Self::Render(i)
            | Self::Question(i, _)
            | Self::Drop(i) => *i,
            Self::Copy(_, i) => *i,
        }
    }
}
fn corpus() -> Vec<Op> {
    let mut cases = vec![];
    for old in [false, true] {
        for mask in [1, 2, 4, 8, 5, 7, 13, 15] {
            for route in [false, true] {
                cases.push(Case {
                    kind: 11,
                    old,
                    append: false,
                    route,
                    pattern: 0,
                    params: [mask, 0, 0, 0, 0, 0, 0, 0],
                });
            }
        }
        for alternate in [0, 1] {
            for pattern in [0, 1, 2, 3, 4] {
                for ids in [vec![], vec![2, 3, 2], vec![0, 17, 33, 244, 314, 17]] {
                    let mut params = [0; 8];
                    params[0] = alternate;
                    params[1] = ids.len() as i32;
                    params[2..2 + ids.len()].copy_from_slice(&ids);
                    cases.push(Case {
                        kind: 12,
                        old,
                        append: false,
                        route: false,
                        pattern,
                        params,
                    });
                }
            }
        }
        for append in [false, true] {
            for id in [0, 6, 7, 26, 27, 31, 32, 42] {
                cases.push(Case {
                    kind: 13,
                    old,
                    append,
                    route: false,
                    pattern: 4,
                    params: [id, 0, 0, 0, 0, 0, 0, 0],
                });
            }
        }
        for count in [0, 1, 3, 6, 7] {
            let mut params = [0; 8];
            params[0] = count;
            params[1..].copy_from_slice(&[0, 2, 33, 2, 127, 159, 9]);
            cases.push(Case {
                kind: 14,
                old,
                append: false,
                route: false,
                pattern: 2,
                params,
            });
        }
    }
    let mut ops = vec![];
    for c in cases {
        ops.push(Op::New(c.clone()));
        ops.push(Op::Render(0));
        ops.push(Op::Poll(0));
        ops.push(Op::Advance(0, 7.25));
        ops.push(Op::Render(0));
        // Copy active clock queues,not just completed/default scalar values.
        ops.push(Op::Copy(0, 1));
        ops.push(Op::Advance(1, 0.));
        ops.push(Op::Advance(1, 0.25));
        ops.push(Op::Advance(1, -0.25));
        ops.push(Op::Fast(1));
        ops.push(Op::Poll(1));
        ops.push(Op::Render(1));
        ops.push(Op::Input(1, false, 1 << 24, 0));
        ops.push(Op::Input(1, true, 1 << 25, 0));
        if c.kind == 11 {
            ops.push(Op::Input(1, true, 0, 1 << 6));
        }
        ops.push(Op::Input(1, true, 1 << 24, 0));
        ops.push(Op::Poll(1));
        if c.kind == 12 {
            ops.push(Op::Question(1, 0));
        }
        ops.push(Op::Advance(1, 60.));
        ops.push(Op::Poll(1));
        ops.push(Op::Fast(1));
        ops.push(Op::Poll(1));
        ops.push(Op::Render(1));
        // The source retains its original active clocks while only copy ticks.
        ops.push(Op::Render(0));
        ops.push(Op::Refresh(0));
        ops.push(Op::Render(0));
        if c.kind == 12 {
            ops.push(Op::Question(0, 1));
        }
        ops.push(Op::Drop(0));
        ops.push(Op::Render(1));
        ops.push(Op::Drop(1));
    }
    ops
}
struct Bits([u32; 20]);
impl Bits {
    fn new(pattern: u32) -> Self {
        let mut b = Self([0; 20]);
        for id in 0..315 {
            let s = if pattern == 4 {
                id % 4
            } else {
                pattern as usize
            };
            if s & 1 != 0 {
                b.0[2 * id / 32] |= 1 << (2 * id % 32);
            }
            if s & 2 != 0 {
                b.0[(2 * id + 1) / 32] |= 1 << ((2 * id + 1) % 32);
            }
        }
        b
    }
}
impl AcquiredBits for Bits {
    fn acquired(&self, id: usize) -> bool {
        self.0[2 * id / 32] & (1 << (2 * id % 32)) != 0
    }
    fn acquire(&mut self, id: usize) -> Result<()> {
        self.0[2 * id / 32] |= 1 << (2 * id % 32);
        Ok(())
    }
}
enum Harness {
    Character(Model),
    Notification(Owner),
}
impl Harness {
    fn new(c: &Case, scenario: &HigurashiScenario, bits: &mut Bits) -> Self {
        if c.kind == 11 {
            return Self::Character(Model::new(c.params[0] as u32).unwrap());
        }
        let content = match c.kind {
            12 => Content::Tips {
                rows: c.params[2..2 + c.params[1] as usize]
                    .iter()
                    .map(|&id| TipDescriptor {
                        id: id as usize,
                        atlas_word: scenario.tips_info[id as usize].id,
                    })
                    .collect(),
                alternate: c.params[0] != 0,
            },
            13 => Content::Otsu {
                argument: c.params[0],
                append: c.append,
            },
            14 => Content::Kakera {
                ids: c.params[1..1 + c.params[0].max(0) as usize].to_vec(),
            },
            _ => unreachable!(),
        };
        let atlas = Arc::new(<<CharacterTextures as TextureArchive>::Builder as TextureArchiveBuilder>::new().build());
        Self::Notification(
            Prepared::Notification { atlas, content }
                .construct(bits, c.route)
                .unwrap(),
        )
    }
    fn words(&self) -> Vec<u32> {
        match self {
            Self::Notification(o) => o.reference_native_words(),
            Self::Character(m) => {
                let clock = |t: &HigurashiTweener| [t.value().to_bits(), u32::from(t.is_waiting())];
                let mut w = vec![
                    m.phase,
                    m.selected as u32,
                    m.top as u32,
                    m.rows.len() as u32,
                ];
                for t in &m.clocks {
                    w.extend(clock(t));
                }
                for r in &m.rows {
                    w.extend([r.id as u32, r.original as u32, r.x as u32]);
                    w.extend(clock(&r.alpha));
                    w.extend(clock(&r.flash));
                }
                w
            }
        }
    }
    fn copied(&self) -> Self {
        match self {
            Self::Notification(o) => Self::Notification(o.native_copy()),
            Self::Character(m) => {
                let mut m = m.native_copy();
                m.take_cues();
                m.result = None;
                Self::Character(m)
            }
        }
    }
    fn cues(&mut self) -> Vec<u32> {
        let cues = match self {
            Self::Notification(o) => o.take_cues(),
            Self::Character(m) => m.take_cues(),
        };
        cues.into_iter()
            .map(|n| {
                ["select", "move", "decide", "suzu"]
                    .iter()
                    .position(|&x| x == n)
                    .unwrap() as u32
            })
            .collect()
    }
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words(w: &mut std::str::SplitWhitespace<'_>, n: usize) -> Vec<u32> {
    (0..n).map(|_| word(w)).collect()
}
#[derive(Debug)]
struct StateRow {
    slot: usize,
    kind: i32,
    os: u32,
    mt: u32,
    base: Vec<u32>,
    private: Vec<u32>,
    bits: Vec<u32>,
    cues: Vec<u32>,
    callback: Vec<u32>,
}
#[derive(Debug)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    mask: [u32; 2],
    uniforms: Vec<u32>,
    vertices: Vec<u32>,
}
enum Native {
    State(StateRow),
    Render(usize, Vec<Draw>, BTreeMap<u32, String>),
    Drop(usize),
}
fn parse(line: &str) -> Native {
    let mut w = line.split_whitespace();
    match w.next().unwrap() {
        "X" => {
            let slot = word(&mut w) as usize;
            assert!(w.next().is_none());
            Native::Drop(slot)
        }
        "R" => {
            let slot = word(&mut w) as usize;
            let mut draws = vec![];
            loop {
                match w.next().unwrap() {
                    "P" => {
                        let program = word(&mut w);
                        let blend = word(&mut w);
                        let texture = word(&mut w);
                        let mask = [word(&mut w), word(&mut w)];
                        let uniforms = words(&mut w, 48);
                        let vertices = words(&mut w, if program == 1 { 16 } else { 24 });
                        draws.push(Draw {
                            program,
                            blend,
                            texture,
                            mask,
                            uniforms,
                            vertices,
                        });
                    }
                    "E" => {
                        assert_eq!(word(&mut w) as usize, draws.len());
                        break;
                    }
                    v => panic!("Packet event{v}"),
                }
            }
            assert_eq!(w.next(), Some("K"));
            let count = word(&mut w);
            let names = (0..count)
                .map(|_| (word(&mut w), w.next().unwrap().into()))
                .collect();
            assert!(w.next().is_none());
            Native::Render(slot, draws, names)
        }
        "O" => {
            let _op = w.next().unwrap();
            let slot = word(&mut w) as usize;
            let kind = word(&mut w) as i32;
            let os = word(&mut w);
            let mt = word(&mut w);
            assert_eq!(w.next(), Some("H"));
            let base = words(&mut w, 138);
            assert_eq!(w.next(), Some("D"));
            let mut private = words(
                &mut w,
                match kind {
                    11 => 4,
                    12 => 7,
                    13 => 9,
                    14 => 6,
                    _ => unreachable!(),
                },
            );
            let (count, stride) = match kind {
                11 => (private[3], 7),
                12 => (private[6], 5),
                13 => (0, 0),
                14 => (private[5], 3),
                _ => unreachable!(),
            };
            if kind == 11 {
                private.extend(words(&mut w, 16));
            }
            private.extend(words(&mut w, count as usize * stride));
            assert_eq!(w.next(), Some("G"));
            let bits = words(&mut w, 20);
            assert_eq!(w.next(), Some("S"));
            let count = word(&mut w) as usize;
            let cues = words(&mut w, count);
            assert_eq!(w.next(), Some("B"));
            let callback = words(&mut w, 2);
            assert!(w.next().is_none());
            Native::State(StateRow {
                slot,
                kind,
                os,
                mt,
                base,
                private,
                bits,
                cues,
                callback,
            })
        }
        v => panic!("Native temporary token{v}"),
    }
}
fn native(ops: &[Op]) -> Vec<Native> {
    let mut input = String::new();
    for op in ops {
        match op {
            Op::New(c) => {
                write!(
                    input,
                    "N {} {} {} {} {}",
                    c.kind,
                    if c.old { 0 } else { 99 },
                    u32::from(c.append),
                    u32::from(c.route),
                    c.pattern
                )
                .unwrap();
                for p in c.params {
                    write!(input, " {p}").unwrap();
                }
                input.push('\n');
            }
            Op::Poll(i) => writeln!(input, "P {i}").unwrap(),
            Op::Advance(i, d) => writeln!(input, "U {i} {d}").unwrap(),
            Op::Fast(i) => writeln!(input, "F {i}").unwrap(),
            Op::Refresh(i) => writeln!(input, "V {i}").unwrap(),
            Op::Input(i, p, a, r) => writeln!(input, "I {i} {} {a} {r}", u32::from(*p)).unwrap(),
            Op::Copy(i, j) => writeln!(input, "C {i} {j}").unwrap(),
            Op::Render(i) => writeln!(input, "R {i}").unwrap(),
            Op::Question(i, v) => writeln!(input, "Q {i} {v}").unwrap(),
            Op::Drop(i) => writeln!(input, "D {i}").unwrap(),
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_TEMPORARY").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_LIVE_TEMPORARY_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        let d = std::path::Path::new(&dir);
        std::fs::write(d.join("input.txt"), copy).unwrap();
        std::fs::write(d.join("output.txt"), &output.stdout).unwrap();
        std::fs::write(d.join("stderr.txt"), output.stderr).unwrap();
    }
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(parse)
        .collect()
}
fn base(p: &HigurashiProperties) -> Vec<u32> {
    let mut w = p.values().map(f32::to_bits).to_vec();
    for o in p.oscillators {
        w.extend([o.seed, o.phase.to_bits()]);
    }
    for i in 0..65 {
        w.push(u32::from(p.tweener(i).unwrap().is_waiting()));
    }
    w
}
#[test]
#[ignore = "actual live11..14 factories/oldbase/queued private copies/phase/input/two-pass packets/acquired bits; Character productionModel reused,prepared empty GPU atlas CPU boundary,no hardware/audio/windows/saves"]
fn higurashi_live_temporary_clocks_copy_globals_and_packets_match_original() {
    let ops = corpus();
    let original = native(&ops);
    let mut original = original.into_iter();
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut owners: [Option<Harness>; 2] = [None, None];
    let mut props: [Option<HigurashiProperties>; 2] = [None, None];
    let mut bits = Bits::new(0);
    let mut kind = 0;
    let mut captures = 0;
    let mut packets = 0;
    let mut events = 0;
    let mut newowners = 0;
    for (op_index, op) in ops.iter().enumerate() {
        let mut expected_callback = vec![0, 0];
        match op {
            Op::New(c) => {
                kind = c.kind;
                bits = Bits::new(c.pattern);
                let mut p = HigurashiProperties::from_seeds([
                    0xabc00001, 0xabc00002, 0xabc00003, 0xabc00004,
                ]);
                if c.old {
                    p.control(0, 100., Ticks::from_f32(10.), 0).unwrap();
                    p.advance(Ticks::from_f32(2.5));
                }
                props[0] = Some(p);
                owners[0] = Some(Harness::new(c, &scenario, &mut bits));
                newowners += 1;
            }
            Op::Copy(i, j) => {
                owners[*j] = Some(owners[*i].as_ref().unwrap().copied());
                props[*j] = Some(props[*i].as_ref().unwrap().clone());
            }
            Op::Drop(i) => {
                owners[*i] = None;
                props[*i] = None;
                let Native::Drop(slot) = original.next().unwrap() else {
                    panic!("Retirement event")
                };
                assert_eq!(slot, *i);
                continue;
            }
            Op::Render(i) => {
                let Native::Render(slot, draws, names) = original.next().unwrap() else {
                    panic!("Render event")
                };
                assert_eq!(slot, *i);
                packets += draws.len();
                captures += 1;
                if let Harness::Notification(owner) = owners[*i].as_ref().unwrap() {
                    let State::Notification(n) = &owner.state else {
                        unreachable!()
                    };
                    let n = n.lock();
                    let p = props[*i].as_ref().unwrap();
                    let matrix = multiply(
                        multiply(
                            p.composed_transform(&HigurashiTransform::default())
                                .compute_native_final_transform(),
                            Mat4::from_translation(vec3(-640., -720., 0.)),
                        ),
                        Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
                    );
                    let actual = n
                        .packets(PassKind::Opaque)
                        .into_iter()
                        .chain(n.packets(PassKind::Transparent))
                        .collect::<Vec<_>>();
                    assert_eq!(actual.len(), draws.len());
                    for (a, d) in actual.iter().zip(draws) {
                        assert_eq!(d.mask, [7, 1]);
                        assert_eq!(d.program, if a.texture.is_some() { 2 } else { 1 });
                        assert_eq!(d.blend, if a.opaque { 1 } else { 2 });
                        assert_eq!(
                            matrix.to_cols_array().map(f32::to_bits).as_slice(),
                            &d.uniforms[..16],
                            "op{op_index}:matrix"
                        );
                        if let Some(name) = &a.texture {
                            assert_eq!(names.get(&d.texture).unwrap(), name);
                            let vertices = a.sprites();
                            let words: Vec<u32> = bytemuck::cast_slice::<_, u8>(&vertices)
                                .chunks_exact(4)
                                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                                .collect();
                            assert_eq!(words, d.vertices);
                        } else {
                            let vertices = a.fills();
                            let words: Vec<u32> = bytemuck::cast_slice::<_, u8>(&vertices)
                                .chunks_exact(4)
                                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                                .collect();
                            assert_eq!(words, d.vertices);
                        }
                    }
                }
            }
            op => {
                let owner = owners[op.slot()].as_mut().unwrap();
                match (owner, op) {
                    (Harness::Character(m), Op::Poll(_)) => m.poll_phase(),
                    (Harness::Character(m), Op::Advance(_, d)) => {
                        m.advance_layer(Ticks::from_f32(*d))
                    }
                    (Harness::Character(m), Op::Fast(_)) => m.fast_forward(),
                    (Harness::Character(m), Op::Refresh(_)) => {
                        for r in &mut m.rows {
                            r.alpha.fast_forward_to(1.);
                        }
                        for (i, v) in [
                            (0, 0.),
                            (1, 1.),
                            (2, 0.),
                            (3, 1.),
                            (4, 1.),
                            (6, 0.),
                            (7, 1.),
                        ] {
                            m.clocks[i].fast_forward_to(v);
                        }
                    }
                    (Harness::Character(m), Op::Input(_, p, a, r)) => m.input(
                        *p,
                        Buttons {
                            held: 0,
                            pressed: *a,
                            repeat: *r,
                            ..Default::default()
                        },
                    ),
                    (Harness::Notification(o), Op::Poll(_)) => {
                        o.poll_phase();
                    }
                    (Harness::Notification(o), Op::Advance(_, d)) => o.advance(Ticks::from_f32(*d)),
                    (Harness::Notification(o), Op::Fast(_)) => o.fast_forward(),
                    (Harness::Notification(o), Op::Refresh(_)) => o.refresh(),
                    (Harness::Notification(o), Op::Input(_, p, a, r)) => o.input(
                        *p,
                        Buttons {
                            held: 0,
                            pressed: *a,
                            repeat: *r,
                            ..Default::default()
                        },
                    ),
                    (Harness::Notification(o), Op::Question(_, v)) => {
                        let value = o.resolve_question(*v);
                        if let Some(v) = value {
                            expected_callback = vec![1, v as u32];
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        let Native::State(n) = original.next().unwrap() else {
            panic!("State event")
        };
        assert_eq!((n.slot, n.kind), (op.slot(), kind));
        assert_eq!(n.mt, 0);
        assert_eq!(
            n.os,
            if let Op::New(c) = op {
                if c.old { 0 } else { 4 }
            } else {
                0
            }
        );
        let h = owners[n.slot].as_mut().unwrap();
        let context = format!("op{op_index} {op:?}");
        assert_eq!(
            h.words(),
            n.private,
            "{context}:native private clock/model words"
        );
        assert_eq!(
            base(props[n.slot].as_ref().unwrap()),
            n.base,
            "{context}:frozen inherited65 clocks"
        );
        assert_eq!(
            bits.0.as_slice(),
            n.bits,
            "{context}:acquired/read/padding words"
        );
        assert_eq!(h.cues(), n.cues, "{context}:cue order");
        assert_eq!(
            expected_callback, n.callback,
            "{context}:Question outer callback"
        );
        events += 1;
    }
    assert!(original.next().is_none());
    assert!(owners.iter().all(Option::is_none));
    eprintln!(
        "Live temporaries:{newowners} actual factory profiles,{events} private/base/global/cue snapshots,{captures} render captures,{packets} native packets;type11productionModel/12..14liveOwner,oldbase frozen,zero MT/copyentropy,duplicate acquisition,Question callback0/1 and retirement;CPU services,no audio/GPU/windows/saves"
    );
}

#[test]
fn live_temporary_retained_input_shares_widget_but_scene_copy_is_independent() {
    let atlas = Arc::new(<<CharacterTextures as TextureArchive>::Builder as TextureArchiveBuilder>::new().build());
    let prepared = Prepared::Notification {
        atlas,
        content: Content::Tips {
            rows: vec![
                TipDescriptor {
                    id: 2,
                    atlas_word: 3,
                },
                TipDescriptor {
                    id: 2,
                    atlas_word: 3,
                },
            ],
            alternate: false,
        },
    };
    let mut bits = Bits::new(2);
    let source = prepared.construct(&mut bits, false).unwrap();
    let State::Notification(n) = &source.state else {
        unreachable!()
    };
    assert_eq!(
        n.lock()
            .rows
            .iter()
            .map(|r| r.previously_acquired)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
    assert!(bits.acquired(2));
    assert_ne!(
        bits.0[0] & (1 << 5),
        0,
        "Existing read bit survives acquisition"
    );
    let before = source.reference_native_words();
    let independent = source.native_copy();
    let mut retained = source.retain();
    retained.poll_phase();
    retained.advance(Ticks::from_f32(7.25));
    assert_ne!(source.reference_native_words(), before);
    assert_eq!(independent.reference_native_words(), before);
    assert_eq!(retained.take_cues(), vec!["suzu"]);
    assert!(source.cues.is_empty());
    drop(source);
    assert_eq!(retained.phase(), 1);
    assert_eq!(independent.phase(), 0);
}


#[test]
#[ignore = "actual base/updated notification title banks; Metal asset upload, no audio or savedata files"]
fn patchless_notification_missing_title_banks_keep_acquisition_and_common_rows() {
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo};
    let (device, queue, _) = crate::adv::higurashi::reference_gpu_assets();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("higurashi");
    for patch in [false, true] {
        let mut io = LayeredAssetIo::new();
        if patch { io.try_with_rom(root.join("patch.rom")).unwrap(); }
        io.try_with_rom(root.join("data.rom")).unwrap();
        let server = AssetServer::new(io.into(), AssetLoadContext {
            game: crate::cli::Game::Higurashi,
            wgpu_device: device.clone(), wgpu_queue: queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(), picture_cache: Default::default(), font_cache: Default::default(),
        });
        for content in [
            Content::Tips { rows: vec![TipDescriptor { id: 0, atlas_word: 191 }, TipDescriptor { id: 1, atlas_word: 192 }], alternate: false },
            Content::Kakera { ids: vec![127, 128] },
        ] {
            let atlas: Arc<CharacterTextures> = server.load_sync(content.archive()).unwrap();
            let tips = matches!(content, Content::Tips { .. });
            let mut bits = Bits([0; 20]);
            let mut owner = Prepared::Notification { atlas, content }.construct(&mut bits, false).unwrap();
            if tips { assert!(bits.acquired(0) && bits.acquired(1)); }
            let acquired = bits.0;
            owner.refresh();
            let before = owner.reference_native_words();
            {
                let State::Notification(n) = &owner.state else { unreachable!() };
                let n = n.lock();
                assert_eq!(n.rows.len(), 2, "presentation must retain both acquisition rows");
                let packets = n.packets_for_artwork(PassKind::Transparent, true);
                let names: Vec<_> = packets.iter().filter_map(|p| p.texture.as_deref()).collect();
                assert_eq!(names.iter().filter(|&&name| name == "plate").count(), if patch { 2 } else { 1 });
                assert!(names.contains(&if tips { "title_005" } else { "title_003" }));
                assert_eq!(names.contains(&if tips { "title_006" } else { "title_004" }), patch);
                assert!(names.iter().all(|name| n.atlas.contains(name)));
            }
            assert_eq!(owner.reference_native_words(), before, "drawing must preserve all clocks/rows");
            assert_eq!(bits.0, acquired, "missing artwork must not discard unlocks");
        }
    }
}
