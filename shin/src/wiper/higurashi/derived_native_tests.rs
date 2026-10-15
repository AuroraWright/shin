//! NEW test-only original0..4 WIPE factory/clock/packet corpus. Parent registers
//! this module and owns compilation/GPU comparison. Raw prepared clock and ready
//! Mask GPU services are explicit, separate from complete command/worker proof.
use std::{collections::BTreeMap, path::PathBuf};
pub(crate) mod gpu;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    New {
        kind: i32,
        duration: i32,
        params: [i32; 8],
        mask_extent: [u32; 2],
    },
    Update(u32),
    Clock([u32; 2]),
    FastForward,
    Draw([u32; 2]),
    Dispose,
}
#[derive(Clone, Debug)]
pub(crate) enum Event {
    Resource([u32; 4]),
    InitialClear([u32; 2]),
    Keep([u32; 2]),
    Drop([u32; 2]),
    Target([u32; 4]),
    Viewport([u32; 2]),
    Restore([u32; 4]),
    Clear([u32; 5]),
    Depth([u32; 2]),
    Stencil([u32; 5]),
    SamplerFilter([u32; 2]),
    SamplerWrap([u32; 3]),
    Bind([u32; 3], Option<[u32; 5]>),
    Copy([u32; 2]),
    Draw(Box<[u32; 113]>),
    Constructor([u32; 6]),
    VirtualMethods([u32; 7]),
    LoweredArgs([u32; 10]),
    Clock([u32; 2]),
    Update([u32; 2]),
    MaskKeep(u32),
    MaskDrop(u32),
    Output([u32; 3]),
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) kind: u32,
    pub(crate) references: u32,
    pub(crate) clock: [u32; 2],
    /// Captured last ec9e4 return; T/S do not recompute this fixture field.
    pub(crate) last_update_return: u32,
    pub(crate) mask_references: u32,
    pub(crate) private: Vec<u32>,
    pub(crate) os_words: u32,
    pub(crate) mt_words: u32,
    pub(crate) allocations: u32,
    pub(crate) retirements: u32,
}
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) index: usize,
    pub(crate) operation: Operation,
    pub(crate) events: Vec<Event>,
    pub(crate) snapshot: Snapshot,
}
impl Row {
    pub(crate) fn draws(&self) -> impl Iterator<Item = &[u32; 113]> {
        self.events.iter().filter_map(|event| match event {
            Event::Draw(words) => Some(&**words),
            _ => None,
        })
    }
}
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Observation {
    pub(crate) index: usize,
    pub(crate) row: usize,
    pub(crate) profile: usize,
    pub(crate) name: String,
    pub(crate) extent: [u32; 2],
    pub(crate) phase: f32,
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Profile {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) kind: i32,
    pub(crate) duration: i32,
    pub(crate) params: [i32; 8],
    pub(crate) mask_extent: [u32; 2],
    pub(crate) first_row: usize,
    pub(crate) last_row: usize,
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Manifest {
    pub(crate) action_count: usize,
    pub(crate) profile_count: usize,
    pub(crate) observation_count: usize,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) observations: Vec<Observation>,
}
pub(crate) struct Corpus {
    pub(crate) rows: Vec<Row>,
    pub(crate) manifest: Manifest,
}
fn word(t: &mut std::str::SplitWhitespace<'_>) -> u32 {
    t.next().unwrap().parse().unwrap()
}
fn signed(t: &mut std::str::SplitWhitespace<'_>) -> i32 {
    t.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(t: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(t))
}
fn operation(line: &str) -> Operation {
    let mut t = line.split_whitespace();
    let op = match t.next().unwrap() {
        "N" => Operation::New {
            kind: signed(&mut t),
            duration: signed(&mut t),
            params: std::array::from_fn(|_| signed(&mut t)),
            mask_extent: words(&mut t),
        },
        "U" => Operation::Update(word(&mut t)),
        "T" => Operation::Clock(words(&mut t)),
        "S" => Operation::FastForward,
        "D" => Operation::Draw(words(&mut t)),
        "F" => Operation::Dispose,
        value => panic!("Unreviewed original derivedWipe operation{value}"),
    };
    assert!(t.next().is_none());
    op
}
pub(crate) fn corpus() -> Corpus {
    let path = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_DERIVED_WIPE_CAPTURE").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../reverse-engineering/higurashi/nondefault-wipe-render-capture"
            )
            .into()
        }),
    );
    let input = std::fs::read_to_string(path.join("input.txt")).unwrap();
    let output = std::fs::read_to_string(path.join("output.txt")).unwrap();
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(input.lines().count(), manifest.action_count);
    assert_eq!(output.lines().count(), manifest.action_count);
    let rows = input
        .lines()
        .zip(output.lines())
        .enumerate()
        .map(|(index, (request, response))| {
            let operation = operation(request);
            let mut t = response.split_whitespace();
            assert_eq!(t.next(), Some("G"));
            assert_eq!(word(&mut t), index as u32);
            assert_eq!(t.next(), request.split_whitespace().next());
            let mut events = vec![];
            let mut snapshot = None;
            while let Some(tag) = t.next() {
                let event = match tag {
                    "N" => Event::Resource(words(&mut t)),
                    "I" => Event::InitialClear(words(&mut t)),
                    "K" => Event::Keep(words(&mut t)),
                    "F" => Event::Drop(words(&mut t)),
                    "T" => Event::Target(words(&mut t)),
                    "V" => Event::Viewport(words(&mut t)),
                    "Z" => Event::Restore(words(&mut t)),
                    "C" => Event::Clear(words(&mut t)),
                    "Y" => Event::Depth(words(&mut t)),
                    "S" => Event::Stencil(words(&mut t)),
                    "M" => Event::SamplerFilter(words(&mut t)),
                    "W" => Event::SamplerWrap(words(&mut t)),
                    "B" => {
                        let v = words::<3>(&mut t);
                        Event::Bind(v, (v[2] != 0).then(|| words(&mut t)))
                    }
                    "X" => Event::Copy(words(&mut t)),
                    "D" => Event::Draw(Box::new(std::array::from_fn(|i| {
                        if i < 3 {
                            signed(&mut t) as u32
                        } else {
                            word(&mut t)
                        }
                    }))),
                    "NEW" => Event::Constructor(std::array::from_fn(|i| {
                        if i == 1 || i == 2 {
                            signed(&mut t) as u32
                        } else {
                            word(&mut t)
                        }
                    })),
                    "VT" => Event::VirtualMethods(words(&mut t)),
                    "ARGS" => Event::LoweredArgs(words(&mut t)),
                    "CLOCK" => Event::Clock(words(&mut t)),
                    "UP" => Event::Update(words(&mut t)),
                    "MK" => Event::MaskKeep(word(&mut t)),
                    "MF" => Event::MaskDrop(word(&mut t)),
                    "OUT" => Event::Output(words(&mut t)),
                    "WS" => {
                        assert!(snapshot.is_none());
                        let kind = word(&mut t);
                        let references = word(&mut t);
                        let clock = words(&mut t);
                        let last_update_return = word(&mut t);
                        let mask_references = word(&mut t);
                        let count = word(&mut t);
                        assert!(count <= 2);
                        let private = (0..count).map(|_| word(&mut t)).collect();
                        snapshot = Some(Snapshot {
                            kind,
                            references,
                            clock,
                            last_update_return,
                            mask_references,
                            private,
                            os_words: word(&mut t),
                            mt_words: word(&mut t),
                            allocations: word(&mut t),
                            retirements: word(&mut t),
                        });
                        continue;
                    }
                    value => panic!("Unreviewed derivedWipe row{index} event{value}"),
                };
                events.push(event);
            }
            Row {
                index,
                operation,
                events,
                snapshot: snapshot.unwrap(),
            }
        })
        .collect();
    Corpus { rows, manifest }
}

impl Corpus {
    /// Focused native GPU coverage: all effects, independent directions, mask
    /// strength/known/reserved flags, native-size dimensions and fractionalphase.
    /// Whole81-profile CPU and994-observation corpus remain available unchanged.
    pub(crate) fn gpu_observations(&self) -> Vec<&Observation> {
        const NAMES: &[&str] = &[
            "default-fallback-kind0",
            "default-fallback-kind5",
            "mask-strength0-flags0",
            "mask-strength1-flags1",
            "mask-strength333-flags0",
            "mask-strength-333-flags1",
            "mask-flags2",
            "mask-flags4",
            "mask-flags8",
            "mask-flags15",
            "mask-flags-1",
            "mask-native-size-full",
            "scroll-0-0",
            "scroll-1-0",
            "scroll-0-3",
            "scroll-2-6",
            "scroll-4-8",
            "scroll--1-9",
            "zoom-0-0",
            "zoom-1000-1000",
            "zoom-333-777",
            "zoom--333-1250",
            "zoom-2000-500",
            "turn-direction0",
            "turn-direction1",
        ];
        self.manifest
            .observations
            .iter()
            .filter(|o| {
                NAMES.contains(&self.manifest.profiles[o.profile].name.as_str())
                    && [0., 0.37, 0.5, 1.].contains(&o.phase)
            })
            .collect()
    }
}

#[test]
fn higurashi_nondefault_wipe_original_factories_clocks_and_packets_are_bounded() {
    let c = corpus();
    assert_eq!(
        (
            c.rows.len(),
            c.manifest.profiles.len(),
            c.manifest.observations.len()
        ),
        (2058, 81, 994)
    );
    assert_eq!(c.gpu_observations().len(), 200);
    let mut resources = BTreeMap::<u32, ([u32; 3], u32)>::new();
    let mut draws = 0;
    let mut updates = 0;
    let mut allocations = 0;
    let mut retirements = 0;
    let mut clock = [0; 2];
    let mut kind = 0;
    for row in &c.rows {
        let s = &row.snapshot;
        assert_eq!((s.os_words, s.mt_words), (0, 0));
        if let Operation::New {
            kind: raw,
            duration,
            params,
            ..
        } = row.operation
        {
            resources.clear();
            kind = if (1..=4).contains(&raw) {
                raw as u32
            } else {
                0
            };
            assert_eq!(s.kind, kind);
            assert_eq!(s.references, 1);
            clock = if duration == 0 {
                [1f32.to_bits(); 2]
            } else {
                [0, (duration as u32 as f32).to_bits()]
            };
            assert_eq!(s.clock, clock);
            match kind {
                0 => assert!(s.private.is_empty()),
                1 => {
                    let strength = if params[1] == 0 { 100 } else { params[1] };
                    assert_eq!(
                        s.private,
                        [(strength as f32 / 256.).abs().to_bits(), params[2] as u32]
                    );
                    assert_eq!(s.mask_references, 2);
                }
                2 => assert_eq!(s.private, [params[0] as u32, params[1] as u32]),
                3 => assert_eq!(
                    s.private,
                    [
                        (params[0] as f32 * f32::from_bits(0x3a83126f)).to_bits(),
                        (params[1] as f32 * f32::from_bits(0x3a83126f)).to_bits()
                    ]
                ),
                4 => assert_eq!(s.private, [params[0] as u32]),
                _ => unreachable!(),
            }
        }
        for event in &row.events {
            match event {
                Event::Resource(v) => {
                    assert!(resources.insert(v[0], ([v[1], v[2], v[3]], 1)).is_none());
                    allocations += 1;
                }
                Event::Keep(v) => {
                    let r = resources.get_mut(&v[0]).unwrap();
                    assert_eq!(r.1 + 1, v[1]);
                    r.1 = v[1];
                }
                Event::Drop(v) => {
                    let r = resources.get_mut(&v[0]).unwrap();
                    assert_eq!(r.1 - 1, v[1]);
                    r.1 = v[1];
                    if v[1] == 0 {
                        retirements += 1;
                    }
                }
                Event::SamplerFilter(v) => assert_eq!(*v, [5, 1]),
                Event::SamplerWrap(v) => assert_eq!(*v, [7; 3]),
                Event::VirtualMethods(v) => assert_eq!(&v[4..6], [0xec9d8, 0xec9e4]),
                Event::Draw(d) => {
                    draws += 1;
                    assert_eq!(
                        (d[9], d[10], d[11]),
                        (1, 4, if kind == 0 { 64 } else { 96 })
                    );
                    assert_eq!(&d[16..21], [7, 0, 0, 0, 0xffff]);
                    assert_eq!(
                        &d[21..25],
                        if d[3] == 2 {
                            &[64, 0, 0, 0]
                        } else {
                            &[64, 16, 0, 0]
                        }
                    );
                }
                _ => {}
            }
        }
        match row.operation {
            Operation::Clock(v) => clock = v,
            Operation::Update(delta) => {
                let mut current = f32::from_bits(clock[0]);
                let total = f32::from_bits(clock[1]);
                if current < total {
                    current += f32::from_bits(delta);
                    if current > total {
                        current = total;
                    }
                }
                clock = [current.to_bits(), total.to_bits()];
                assert_eq!(s.last_update_return, u32::from(current < total));
                updates += 1;
            }
            Operation::FastForward => clock = [clock[1]; 2],
            Operation::Draw(extent) => {
                let ds = row.draws().collect::<Vec<_>>();
                assert_eq!(ds.len(), if kind < 2 { 1 } else { 2 });
                for d in &ds {
                    assert_eq!(&d[7..9], extent);
                }
                if kind == 0 {
                    assert_eq!(&ds[0][3..5], [21, 1]);
                    assert_eq!(&ds[0][13..15], [1, 2]);
                } else if kind == 1 {
                    assert_eq!(&ds[0][3..5], [22, 1]);
                    assert_eq!(&ds[0][13..16], [1, 2, 3]);
                } else {
                    assert_eq!(&ds[0][3..5], [2, 1]);
                    assert_eq!(&ds[1][3..5], [2, 2]);
                }
            }
            Operation::Dispose => {
                assert!(resources.values().all(|r| r.1 == 0));
                assert_eq!(s.allocations, s.retirements);
                assert_eq!(s.mask_references, 1);
            }
            _ => {}
        }
        if !matches!(row.operation, Operation::Dispose) {
            assert_eq!(s.clock, clock);
        }
    }
    assert_eq!(
        (draws, updates, allocations, retirements),
        (1512, 243, 2231, 2231)
    );
    println!(
        "higurashi_derived_wipe_original=81profiles/2058actions/994observations/1512packets/243updates;0OS0MT;2231alloc-retire;preparedClock+MaskGPUboundaries"
    );
}
