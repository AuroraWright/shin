//! NEW independent complete native Screen sequence replay. Original resources,
//! copies and D113 words drive literal shaders; production Scene/Clip/material
//! transforms are never used by the reference. Parent registers this module.
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) mod gpu;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    New([u32; 2]),
    Tile {
        bank: u32,
        slot: u32,
        id: i32,
        params: [i32; 8],
    },
    Property {
        which: u32,
        index: u32,
        bits: u32,
    },
    Update {
        enabled: bool,
        paused: bool,
        delta: u32,
    },
    Prepare,
    Draw {
        extent: [u32; 2],
        pass: u32,
        stencil: u32,
    },
    Capture(u32),
    Wipe(u32),
    Restore([[i32; 65]; 3]),
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
    Flush(u32),
    Render([i32; 4]),
    Constructor([u32; 3]),
    Leaf([i32; 4]),
    Update([u32; 3]),
    Output([u32; 3]),
    Capture([u32; 5]),
    Wipe(u32),
    RestoredTargets([u32; 2]),
}
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) index: usize,
    pub(crate) operation: Operation,
    pub(crate) events: Vec<Event>,
    /// ctorW/H,current,previous,wiper,time,total,allocations,retirements,prop39.
    pub(crate) screen: [u32; 10],
    pub(crate) groups: [Vec<(i32, u32)>; 3],
}
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Observation {
    pub(crate) index: usize,
    pub(crate) row: usize,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) extent: [u32; 2],
    pub(crate) require_visible_pattern: bool,
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Comparison {
    pub(crate) baseline: usize,
    pub(crate) effect: usize,
    pub(crate) equal: bool,
    pub(crate) reason: String,
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Sequence {
    pub(crate) first_row: usize,
    pub(crate) last_row: usize,
    pub(crate) constructor_extent: [u32; 2],
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Manifest {
    pub(crate) action_count: usize,
    pub(crate) display_count: usize,
    pub(crate) capture_count: usize,
    pub(crate) observations: Vec<Observation>,
    pub(crate) comparisons: Vec<Comparison>,
    pub(crate) sequences: Vec<Sequence>,
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
fn parse_operation(line: &str) -> Operation {
    let mut t = line.split_whitespace();
    let op = match t.next().unwrap() {
        "N" => Operation::New(words(&mut t)),
        "L" => {
            let bank = word(&mut t);
            let slot = word(&mut t);
            let id = signed(&mut t);
            let mut params = [0; 8];
            for p in &mut params[..5] {
                *p = signed(&mut t)
            }
            Operation::Tile {
                bank,
                slot,
                id,
                params,
            }
        }
        "P" => Operation::Property {
            which: word(&mut t),
            index: word(&mut t),
            bits: word(&mut t),
        },
        "A" => Operation::Update {
            enabled: word(&mut t) != 0,
            paused: word(&mut t) != 0,
            delta: word(&mut t),
        },
        "R" => Operation::Prepare,
        "D" => Operation::Draw {
            extent: words(&mut t),
            pass: word(&mut t),
            stencil: word(&mut t),
        },
        "C" => Operation::Capture(word(&mut t)),
        "W" => Operation::Wipe(word(&mut t)),
        "I" => Operation::Restore(std::array::from_fn(|_| {
            std::array::from_fn(|_| signed(&mut t))
        })),
        "F" => Operation::Dispose,
        v => panic!("Unreviewed original operation{v}"),
    };
    assert!(t.next().is_none());
    op
}
pub(crate) fn corpus() -> Corpus {
    let dir = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_LIVE_AFTERIMAGE_CAPTURE").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../reverse-engineering/higurashi/live-afterimage-frames-capture"
            )
            .into()
        }),
    );
    let input = std::fs::read_to_string(dir.join("input.txt")).unwrap();
    let output = std::fs::read_to_string(dir.join("output.txt")).unwrap();
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(input.lines().count(), manifest.action_count);
    assert_eq!(output.lines().count(), manifest.action_count);
    let rows = input
        .lines()
        .zip(output.lines())
        .enumerate()
        .map(|(index, (request, response))| {
            let operation = parse_operation(request);
            let mut t = response.split_whitespace();
            assert_eq!(t.next(), Some("G"));
            assert_eq!(word(&mut t), index as u32);
            assert_eq!(t.next(), request.split_whitespace().next());
            let mut events = vec![];
            let mut screen = None;
            let mut groups: [Vec<(i32, u32)>; 3] = std::array::from_fn(|_| vec![]);
            while let Some(tag) = t.next() {
                match tag {
                    "N" => events.push(Event::Resource(words(&mut t))),
                    "I" => events.push(Event::InitialClear(words(&mut t))),
                    "K" => events.push(Event::Keep(words(&mut t))),
                    "F" => events.push(Event::Drop(words(&mut t))),
                    "T" => events.push(Event::Target(words(&mut t))),
                    "V" => events.push(Event::Viewport(words(&mut t))),
                    "Z" => events.push(Event::Restore(words(&mut t))),
                    "C" => events.push(Event::Clear(words(&mut t))),
                    "Y" => events.push(Event::Depth(words(&mut t))),
                    "S" => events.push(Event::Stencil(words(&mut t))),
                    "M" => events.push(Event::SamplerFilter(words(&mut t))),
                    "W" => events.push(Event::SamplerWrap(words(&mut t))),
                    "B" => {
                        let v = words::<3>(&mut t);
                        let d = (v[2] != 0).then(|| words(&mut t));
                        events.push(Event::Bind(v, d))
                    }
                    "X" => events.push(Event::Copy(words(&mut t))),
                    // Original transport prints owner/kind/pass as signed;
                    // internal framebuffer passes use the sentinel pass=-1.
                    "D" => events.push(Event::Draw(Box::new(std::array::from_fn(|i| {
                        if i < 3 {
                            signed(&mut t) as u32
                        } else {
                            word(&mut t)
                        }
                    })))),
                    "H" => events.push(Event::Flush(word(&mut t))),
                    "Q" => events.push(Event::Render(std::array::from_fn(|_| signed(&mut t)))),
                    "CT" => events.push(Event::Constructor(words(&mut t))),
                    "LC" => events.push(Event::Leaf(std::array::from_fn(|_| signed(&mut t)))),
                    "UP" => events.push(Event::Update(words(&mut t))),
                    "OUT" => events.push(Event::Output(words(&mut t))),
                    "CP" => events.push(Event::Capture(words(&mut t))),
                    "WI" => events.push(Event::Wipe(word(&mut t))),
                    "RT" => events.push(Event::RestoredTargets(words(&mut t))),
                    "SS" => {
                        assert!(screen.is_none());
                        screen = Some(words(&mut t))
                    }
                    "GG" => {
                        let bank = word(&mut t) as usize;
                        let n = word(&mut t);
                        assert!(bank < 3 && n <= 8);
                        groups[bank] = (0..n).map(|_| (signed(&mut t), word(&mut t))).collect()
                    }
                    v => panic!("Unreviewed native frame row{index} event{v}"),
                }
            }
            Row {
                index,
                operation,
                events,
                screen: screen.unwrap(),
                groups,
            }
        })
        .collect();
    Corpus { rows, manifest }
}
impl Corpus {
    pub(crate) fn sequence_for(&self, observation: &Observation) -> &Sequence {
        self.manifest
            .sequences
            .iter()
            .find(|s| s.first_row <= observation.row && observation.row <= s.last_row)
            .unwrap()
    }
    pub(crate) fn resource_for(&self, observation: &Observation) -> u32 {
        let row = &self.rows[observation.row];
        row.events
            .iter()
            .find_map(|e| match e {
                Event::Output(v) if observation.kind == "display" => Some(v[0]),
                Event::Capture(v) if observation.kind == "capture" => {
                    assert_eq!(&v[3..5], &observation.extent);
                    Some(v[2])
                }
                _ => None,
            })
            .unwrap()
    }
}
#[test]
fn higurashi_live_afterimage_original_frame_sequences_are_bounded_and_ordered() {
    let c = corpus();
    assert_eq!(
        (
            c.rows.len(),
            c.manifest.display_count,
            c.manifest.capture_count
        ),
        (178, 40, 14)
    );
    let mut resources = BTreeMap::<u32, ([u32; 3], u32)>::new();
    let mut draws = 0;
    let mut history = 0;
    let mut root_filters = 0;
    let mut outer = 0;
    let mut copies = 0;
    for row in &c.rows {
        if matches!(row.operation, Operation::New(_)) {
            resources.clear();
            assert_eq!(row.screen[3], 0);
            assert_eq!(row.screen[2], 1)
        }
        for e in &row.events {
            match e {
                Event::Resource(v) => {
                    assert!(resources.insert(v[0], ([v[1], v[2], v[3]], 1)).is_none())
                }
                Event::Keep(v) => {
                    let r = resources.get_mut(&v[0]).unwrap();
                    assert_eq!(r.1 + 1, v[1]);
                    r.1 = v[1]
                }
                Event::Drop(v) => {
                    let r = resources.get_mut(&v[0]).unwrap();
                    assert_eq!(r.1 - 1, v[1]);
                    r.1 = v[1]
                }
                Event::Copy(v) => {
                    assert_eq!(resources[&v[0]].0, resources[&v[1]].0);
                    copies += 1
                }
                Event::Constructor(v) => assert_eq!(&v[1..], [16, 0]),
                Event::Leaf(v) => assert_eq!(&v[2..], [4, 0]),
                Event::RestoredTargets(v) => assert_eq!(*v, [row.screen[2], row.screen[3]]),
                Event::Capture(v) => {
                    assert_eq!(v[1], row.screen[2]);
                    assert_eq!(resources[&v[2]].0, [0, row.screen[0], row.screen[1]])
                }
                Event::Draw(d) => {
                    draws += 1;
                    if d[3] == 6 && d[4] == 5 {
                        history += 1;
                        assert_eq!(d[13], row.screen[3]);
                        assert!(matches!(row.operation, Operation::Prepare))
                    }
                    if d[3] == 8 {
                        root_filters += 1
                    }
                    if let Operation::Draw { pass, stencil, .. } = row.operation {
                        outer += 1;
                        assert_eq!(pass, 1);
                        assert_eq!(&d[3..5], [6, 1]);
                        assert_eq!(d[13], row.screen[2]);
                        assert_eq!(&d[16..20], [4, 0, 0, 2]);
                        assert_eq!(d[20], 0xffff | (stencil << 16));
                        assert_eq!(&d[41..45], [1f32.to_bits(); 4]);
                        assert_eq!(d[25], (2f32 / 1280.).to_bits());
                        assert_eq!(d[30], (-2f32 / 720.).to_bits());
                        assert_eq!(d[37], (-1f32).to_bits());
                        assert_eq!(d[38], 1f32.to_bits())
                    }
                }
                _ => {}
            }
        }
        if matches!(row.operation, Operation::Dispose) {
            assert!(resources.values().all(|v| v.1 == 0));
            assert_eq!(row.screen[7], row.screen[8]);
            assert!(row.groups.iter().all(Vec::is_empty))
        }
    }
    assert_eq!(outer, 40);
    assert!(draws > 200 && history > 20 && root_filters >= 2 && copies >= 14);
    println!(
        "live_afterimage_original_sequence_proof=178actions/40outer/14current-captures;draws={draws};history={history};rootFilters={root_filters};copies={copies}"
    );
}
#[test]
#[ignore = "Original full native frame events/literal programs0/1/6..11/21; pure offscreen720p/1080p, no audio/window/user saves"]
fn higurashi_live_afterimage_native_sequence_pixels_have_no_prepare_feedback() {
    let c = corpus();
    let (device, queue, _) = reference_gpu_assets();
    let mut frames = vec![];
    let mut records = vec![];
    for o in &c.manifest.observations {
        assert_eq!(o.index, frames.len());
        let pixels = reference_render_to_texture(
            &device,
            &queue,
            (o.extent[0], o.extent[1]),
            |ctx, target| gpu::replay(ctx, target, &c, o),
        );
        let colors = pixels
            .chunks_exact(4)
            .map(|p| <[u8; 4]>::try_from(p).unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        if o.require_visible_pattern {
            assert!(colors.len() >= 3, "{} colors{}", o.name, colors.len());
            assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]))
        }
        records.push(serde_json::json!({"name":o.name,"kind":o.kind,"row":o.row,"extent":o.extent,"colors":colors.len()}));
        frames.push(pixels);
    }
    for control in &c.manifest.comparisons {
        let (a, b) = (&frames[control.baseline], &frames[control.effect]);
        assert_eq!(a.len(), b.len());
        let n = a.iter().zip(b).filter(|(a, b)| a != b).count();
        if control.equal {
            assert_eq!(n, 0, "{}", control.reason)
        } else {
            assert!(n > 0, "{}", control.reason)
        }
    }
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_AFTERIMAGE_PIXEL_SUMMARY") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"frames":records,"comparisons":c.manifest.comparisons.len(),"independent":"Actual original full native event/resource/D113 sequence and literal SPIR-V; no production Scene","limits":"Standalone sequence proof; production lifecycle comparison must use this same replay in parent integration"})).unwrap()).unwrap()
    }
    println!(
        "live_afterimage_native_sequence_pixels=54observations/40outer/14current-captures/34controls;literalShaders;noAudio"
    );
}
