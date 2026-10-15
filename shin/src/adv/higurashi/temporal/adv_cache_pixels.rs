//! NEW independent original ADV98 cache/full-callback corpus and raw GPU replay.
//! Parent registers this test-only module. No production Scene/cache preparation
//! or material transform runs in the original reference path.
use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) mod gpu;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    New {
        dimension_mode: u32,
    },
    Tile {
        where_: u32,
        slot: u32,
        id: i32,
        params: [i32; 8],
    },
    Property {
        actor: u32,
        index: u32,
        bits: u32,
    },
    Update {
        enabled: bool,
        paused: bool,
        request: u32,
        delta: u32,
    },
    Callback {
        request: u32,
        delta: u32,
    },
    ScreenUpdate {
        delta: u32,
    },
    Prepare,
    Capture {
        kind: u32,
        index: u32,
    },
    Draw {
        extent: [u32; 2],
        pass: u32,
        stencil: u32,
    },
    ModalEnter,
    ModalExit,
    NavigationEndpointFinish,
    Dispose,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Update { actor: u32, delta: u32 },
    Prepare { actor: u32 },
    Draw { actor: u32, pass: u32, stencil: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Navigation {
    Constructor(u32),
    Publication,
    EndpointFinish,
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
    Constructor([u32; 6]),
    VirtualMethods([u32; 4]),
    Leaf([i32; 5]),
    Phase(Phase),
    PhaseEnd([u32; 2]),
    Window([u32; 3]),
    MessageBuffer([u32; 2]),
    Update([u32; 4]),
    Control([u32; 2]),
    Audio([u32; 2]),
    Ui([u32; 2]),
    Navigation(Navigation),
    Output([u32; 3]),
    Capture([u32; 6]),
}
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) index: usize,
    pub(crate) operation: Operation,
    pub(crate) events: Vec<Event>,
    /// 0W,1H,2CURRENT,3previous,4ADVcolor,5ADVdepth,6modal,7navigation,
    /// 8animations,9pause,10conceal,11alloc,12retire,13controls,
    /// 14..=31 six actors x(update,prepare,draw),32window,33OS,34MT.
    pub(crate) snapshot: [u32; 35],
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
pub(crate) struct Profile {
    pub(crate) first_row: usize,
    pub(crate) last_row: usize,
    pub(crate) dimension_mode: u32,
    pub(crate) constructor_extent: [u32; 2],
}
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Manifest {
    pub(crate) action_count: usize,
    pub(crate) observations: Vec<Observation>,
    pub(crate) comparisons: Vec<Comparison>,
    pub(crate) profiles: Vec<Profile>,
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
    let operation = match t.next().unwrap() {
        "N" => Operation::New {
            dimension_mode: word(&mut t),
        },
        "L" => {
            let where_ = word(&mut t);
            let slot = word(&mut t);
            let id = signed(&mut t);
            let mut params = [0; 8];
            for p in &mut params[..5] {
                *p = signed(&mut t);
            }
            Operation::Tile {
                where_,
                slot,
                id,
                params,
            }
        }
        "P" => Operation::Property {
            actor: word(&mut t),
            index: word(&mut t),
            bits: word(&mut t),
        },
        "U" => Operation::Update {
            enabled: word(&mut t) != 0,
            paused: word(&mut t) != 0,
            request: word(&mut t),
            delta: word(&mut t),
        },
        "B" => Operation::Callback {
            request: word(&mut t),
            delta: word(&mut t),
        },
        "A" => Operation::ScreenUpdate {
            delta: word(&mut t),
        },
        "R" => Operation::Prepare,
        "C" => Operation::Capture {
            kind: word(&mut t),
            index: word(&mut t),
        },
        "D" => Operation::Draw {
            extent: words(&mut t),
            pass: word(&mut t),
            stencil: word(&mut t),
        },
        "M" => Operation::ModalEnter,
        "X" => Operation::ModalExit,
        "V" => Operation::NavigationEndpointFinish,
        "F" => Operation::Dispose,
        value => panic!("Unreviewed original ADV-cache operation{value}"),
    };
    assert!(t.next().is_none());
    operation
}
pub(crate) fn corpus() -> Corpus {
    let directory = PathBuf::from(
        std::env::var("SHIN_HIGURASHI_LIVE_ADV_CACHE_CAPTURE").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../reverse-engineering/higurashi/live-adv-cache-capture"
            )
            .into()
        }),
    );
    let input = std::fs::read_to_string(directory.join("input.txt")).unwrap();
    let output = std::fs::read_to_string(directory.join("output.txt")).unwrap();
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).unwrap()).unwrap();
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
                    // Native owner/kind/pass prefix is signed, including internal-1.
                    "D" => Event::Draw(Box::new(std::array::from_fn(|i| {
                        if i < 3 {
                            signed(&mut t) as u32
                        } else {
                            word(&mut t)
                        }
                    }))),
                    "H" => Event::Flush(word(&mut t)),
                    "Q" => Event::Render(std::array::from_fn(|_| signed(&mut t))),
                    "NEW" => Event::Constructor(words(&mut t)),
                    "VT" => Event::VirtualMethods(words(&mut t)),
                    "LC" => Event::Leaf(std::array::from_fn(|_| signed(&mut t))),
                    "PH" => {
                        let actor = word(&mut t);
                        Event::Phase(match word(&mut t) {
                            0 => Phase::Update {
                                actor,
                                delta: word(&mut t),
                            },
                            1 => Phase::Prepare { actor },
                            2 => Phase::Draw {
                                actor,
                                pass: word(&mut t),
                                stencil: word(&mut t),
                            },
                            value => panic!("Unreviewed original ADV-cache phase{value}"),
                        })
                    }
                    "PE" => Event::PhaseEnd(words(&mut t)),
                    "MW" => Event::Window(words(&mut t)),
                    "MB" => Event::MessageBuffer(words(&mut t)),
                    "UPDATE" => Event::Update(words(&mut t)),
                    "CTL" => Event::Control(words(&mut t)),
                    "AU" => Event::Audio(words(&mut t)),
                    "UI" => Event::Ui(words(&mut t)),
                    "NAV" => Event::Navigation(match word(&mut t) {
                        0 => Navigation::Constructor(word(&mut t)),
                        1 => Navigation::Publication,
                        2 => Navigation::EndpointFinish,
                        value => panic!("Unreviewed checked ADV-cache navigation boundary{value}"),
                    }),
                    "OUT" => Event::Output(words(&mut t)),
                    "CP" => Event::Capture(words(&mut t)),
                    "AS" => {
                        assert!(snapshot.is_none());
                        snapshot = Some(words(&mut t));
                        continue;
                    }
                    value => panic!("Unreviewed original ADV-cache row{index} event{value}"),
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
    pub(crate) fn profile_for(&self, observation: &Observation) -> &Profile {
        self.manifest
            .profiles
            .iter()
            .find(|p| p.first_row <= observation.row && observation.row <= p.last_row)
            .unwrap()
    }
    pub(crate) fn resource_for(&self, observation: &Observation) -> u32 {
        self.rows[observation.row]
            .events
            .iter()
            .find_map(|event| match event {
                Event::Output(v) if observation.kind == "display" => Some(v[0]),
                Event::Capture(v) if observation.kind != "display" => {
                    assert_eq!(&v[4..6], &observation.extent);
                    Some(v[3])
                }
                _ => None,
            })
            .unwrap()
    }
}

#[test]
fn higurashi_live_adv_cache_original_callbacks_and_resources_are_bounded_and_ordered() {
    let c = corpus();
    assert_eq!(
        (
            c.rows.len(),
            c.manifest.observations.len(),
            c.manifest.comparisons.len()
        ),
        (128, 64, 34)
    );
    let mut resources = BTreeMap::<u32, ([u32; 3], u32)>::new();
    let mut prior = None::<[u32; 35]>;
    let mut callbacks = 0;
    let mut admitted = 0;
    let mut blocked = 0;
    let mut draws = 0;
    let mut copies = 0;
    let mut modal = 0;
    let mut navigation = 0;
    let mut outer = 0;
    let mut outer_skips = 0;
    for row in &c.rows {
        let s = row.snapshot;
        assert_eq!(s[34], 0);
        if let Operation::New { dimension_mode } = row.operation {
            resources.clear();
            assert_eq!(
                s[..2],
                if dimension_mode == 0 {
                    [1920, 1080]
                } else {
                    [1280, 720]
                }
            );
            assert_eq!(s[3..8], [0; 5]);
            assert_eq!(s[11..14], [2, 0, 0]);
            assert_eq!(&s[33..35], [24, 0]);
        }
        let mut zero_releases = vec![];
        let mut row_draws = vec![];
        let mut row_copies = vec![];
        let mut parent_phases = vec![];
        let mut update = None;
        for (event_index, event) in row.events.iter().enumerate() {
            match event {
                Event::Resource(v) => {
                    assert!(resources.insert(v[0], ([v[1], v[2], v[3]], 1)).is_none());
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
                        zero_releases.push(v[0]);
                    }
                }
                Event::Copy(v) => {
                    assert_eq!(resources[&v[0]].0, resources[&v[1]].0);
                    copies += 1;
                    row_copies.push(*v);
                }
                Event::Constructor(v) => {
                    assert_eq!(v[1], if s[0] == 1920 { 0 } else { 1 });
                    assert_eq!(v[2..], [s[0], s[1], 24, 0]);
                }
                Event::Leaf(v) => assert_eq!(&v[3..], [4, 0]),
                Event::Phase(Phase::Update { actor: 0, .. }) => parent_phases.push(0),
                Event::Phase(Phase::Prepare { actor: 0 }) => {
                    parent_phases.push(1);
                    let screen_end = row.events[event_index..]
                        .iter()
                        .position(|e| matches!(e, Event::PhaseEnd([1, 1])))
                        .unwrap();
                    let cached_target = row.events[event_index..]
                        .iter()
                        .position(|e| matches!(e, Event::Target(v) if v[0] == s[4]))
                        .unwrap();
                    assert!(screen_end < cached_target);
                }
                Event::Update(v) => {
                    assert!(update.is_none());
                    update = Some(*v);
                }
                Event::Draw(d) => {
                    draws += 1;
                    row_draws.push(&**d);
                }
                Event::Capture(v) => {
                    assert_eq!(resources[&v[2]].0, resources[&v[3]].0);
                    assert_eq!(resources[&v[3]].0, [0, v[4], v[5]]);
                    assert!(
                        !row.events
                            .iter()
                            .any(|e| matches!(e, Event::Phase(_) | Event::Draw(_)))
                    );
                }
                _ => {}
            }
        }
        if let Some([enabled, paused, request, _]) = update {
            callbacks += 1;
            let gate = enabled != 0 && paused == 0 && request == 0;
            assert_eq!(parent_phases, if gate { vec![0, 1] } else { vec![] });
            if gate {
                admitted += 1;
            } else {
                blocked += 1;
                assert_eq!(s[2..6], prior.unwrap()[2..6]);
            }
            if request == 1 {
                modal += 1;
                assert_eq!(row_draws.len(), 2);
                assert!(row_draws.iter().all(|d| d[3] == 23));
                assert_eq!(row_draws[0][13], prior.unwrap()[2]);
                assert_eq!(row_draws[1][13], row_draws[0][5]);
                assert_eq!(s[8..10], [0, 1]);
            } else if request == 2 {
                navigation += 1;
                assert!(row_draws.is_empty());
                assert_eq!(row_copies, [[prior.unwrap()[4], s[7]]]);
                assert_eq!(s[8], 0);
            }
        }
        if let Operation::Draw { pass, stencil, .. } = row.operation {
            if row_draws.is_empty() {
                outer_skips += 1;
            } else {
                assert_eq!(row_draws.len(), 1);
                outer += 1;
                let d = row_draws[0];
                assert_eq!(pass, 1);
                assert_eq!(&d[3..5], [6, 1]);
                assert_eq!(d[13], s[4]);
                assert_eq!(&d[16..21], [4, 0, 0, 2, 0xffff | (stencil << 16)]);
                assert_eq!(&d[41..45], [1f32.to_bits(); 4]);
                assert_eq!(
                    &d[25..41],
                    [
                        986500301, 0, 0, 0, 0, 3140881249, 0, 0, 0, 0, 3212836864, 0, 3212836864,
                        1065353216, 2147483648, 1065353216
                    ]
                );
            }
        }
        if matches!(row.operation, Operation::Dispose) {
            assert!(resources.values().all(|r| r.1 == 0));
            assert_eq!((s[11], s[12], s[33]), (47, 47, 60));
            let before = prior.unwrap();
            assert!(
                zero_releases
                    .iter()
                    .position(|&id| id == before[5])
                    .unwrap()
                    < zero_releases
                        .iter()
                        .position(|&id| id == before[4])
                        .unwrap()
            );
        }
        prior = Some(s);
    }
    assert_eq!(
        (
            callbacks,
            admitted,
            blocked,
            draws,
            copies,
            modal,
            navigation,
            outer,
            outer_skips
        ),
        (28, 16, 12, 150, 52, 2, 2, 10, 4)
    );
    println!(
        "live_adv_cache_original_sequence_proof=128actions/64observations/28callbacks/16admitted/12blocked/150packets/52copies;actualFixed1080+explicit720service;realOwners;boundedServices"
    );
}

#[test]
#[ignore = "Full original128-action cache/callback events + literal programs0/1/6/23; pure offscreen720p/1080p, no audio/window/user saves"]
fn higurashi_live_adv_cache_original_packet_pixels_preserve_capture_order() {
    let corpus = corpus();
    let (device, queue, _) = reference_gpu_assets();
    let mut frames = vec![];
    let mut records = vec![];
    for observation in &corpus.manifest.observations {
        assert_eq!(observation.index, frames.len());
        let pixels = reference_render_to_texture(
            &device,
            &queue,
            (observation.extent[0], observation.extent[1]),
            |context, target| gpu::replay(context, target, &corpus, observation),
        );
        let colors = pixels
            .chunks_exact(4)
            .map(|p| <[u8; 4]>::try_from(p).unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        if observation.require_visible_pattern {
            assert!(
                colors.len() >= 3,
                "{} colors{}",
                observation.name,
                colors.len()
            );
            assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        }
        records.push(serde_json::json!({"name":observation.name,"kind":observation.kind,"row":observation.row,"extent":observation.extent,"colors":colors.len()}));
        frames.push(pixels);
    }
    let mut controls = vec![];
    for comparison in &corpus.manifest.comparisons {
        let (a, b) = (&frames[comparison.baseline], &frames[comparison.effect]);
        assert_eq!(a.len(), b.len());
        let differences = a.iter().zip(b).filter(|(a, b)| a != b).count();
        let max = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        controls.push(serde_json::json!({"baseline":comparison.baseline,"effect":comparison.effect,"equal":comparison.equal,"differences":differences,"max":max,"reason":comparison.reason}));
        if comparison.equal {
            assert_eq!(differences, 0, "{}", comparison.reason);
        } else {
            assert!(differences > 0, "{}", comparison.reason);
        }
    }
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_ADV_CACHE_PIXEL_SUMMARY") {
        std::fs::write(path, serde_json::to_vec_pretty(&serde_json::json!({"frames":records,"controls":controls,"independent":"Actual original128-action events/150D113 packets and literal SPIR-V, no production cache/Scene/Clip/material helpers","limits":"Checked13140 requests, blank Messagebox artwork, audio/UI tails and navigation mode endpoints; production cache integration comparison remains separate"})).unwrap()).unwrap();
    }
    println!(
        "live_adv_cache_original_packet_pixels=64observations/34controls;literalShaders0/1/6/23;noAudio;productionIntegrationSeparate"
    );
}
