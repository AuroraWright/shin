//! Live in-place publication against the immutable36 original18db0/19578
//! observations. Basic descriptors stand at the explicitly bounded asset
//! preparation boundary; publication, keyed reset, retirees and Scene history
//! run through production. CPU history IDs are ownership symbols, not a GPU
//! texture/depth identity or pixel claim. Native full-control/fade/task results
//! remain original evidence; this test does not emulate that outer task.
use super::*;
use crate::{adv::higurashi::scene::HigurashiScene, layer::user::higurashi::Prepared};
use serde::Deserialize;
use shin_core::{format::save::higurashi::SaveData, primitives::color::FloatColor4};
use std::{collections::BTreeMap, sync::Weak};

const OBSERVATIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/retained-restoration-capture/parsed.json"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/retained-restoration-capture/output.txt"
));

// [key,remaining bits,diagnostic cookie,native owner ID,refs,canonical base hash].
// Active entry's raw flags word is NOT transported by the array-backed adapter.
type Row = (i32, u32, u32, u32, u32, u64);
#[derive(Debug, Deserialize)]
struct Snapshot([u32; 11], [Vec<Row>; 3]);
#[derive(Debug, Deserialize)]
struct Native {
    header: [i32; 11],
    snapshots: BTreeMap<String, Snapshot>,
    jobs: Vec<[u32; 2]>,
    publishes: Vec<[u32; 3]>,
    balance: [u64; 7],
    pending: Option<[i32; 4]>,
    complete: Option<[i32; 5]>,
}

fn next_seeds(cursor: &mut u32) -> [u32; 4] {
    std::array::from_fn(|_| {
        *cursor += 1;
        0xabc0_0000 + *cursor
    })
}
fn basic(kind: i32, seeds: [u32; 4]) -> Option<HigurashiUserLayer> {
    match kind {
        -1 => None,
        0 => Some(HigurashiUserLayer::null(seeds)),
        1 => Some(HigurashiUserLayer::tile(
            FloatColor4::from_4bpp_property(0xf123),
            glam::vec4(0., 0., 1280., 720.),
            seeds,
        )),
        _ => panic!("Unreviewed retained-restoration factory {kind}"),
    }
}
// Mirrors the checked original lf_mutate INPUT, not its evaluated output.
// Only tweener0 receives the initial2-tick update; oscillator phases are not
// updated until the enabled Screen callback after publication.
fn source(kind: i32, label: i32, cursor: &mut u32) -> HigurashiUserLayer {
    let mut owner = basic(kind, next_seeds(cursor)).unwrap();
    owner.set_owner_id(label);
    for (i, initial) in INITIAL_VALUES.iter().enumerate() {
        owner
            .properties
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to((*initial + (i % 5) as i32) as f32);
    }
    for (oscillator, phase) in owner
        .properties
        .oscillators
        .iter_mut()
        .zip([0.25, 0.5, 1., 2.])
    {
        oscillator.phase = phase;
    }
    owner
        .properties
        .control(0, 101., Ticks::from_f32(8.), 0)
        .unwrap();
    owner
        .properties
        .control(0, 37., Ticks::from_f32(11.), 0x80)
        .unwrap();
    owner
        .properties
        .tweener_mut(0)
        .unwrap()
        .update(Ticks::from_f32(2.));
    owner
}
fn stored(native: &Native) -> LayerBanks {
    let mut state = LayerBanks::default();
    for slot in 0..SLOTS {
        for bank in 0..2 {
            let record = &mut state.records[slot][bank];
            record.words[0] = -1;
            record.words[1] = (if bank == 0 { 500 } else { 900 }) + slot as i32;
            record.words[2] = -1;
            record.reset_properties();
        }
    }
    for bank in 0..2 {
        let record = &mut state.records[2][bank];
        record.words[0] = native.header[3];
        if native.header[3] == 1 {
            record.words[6..14].copy_from_slice(&[0xf123, 0, 0, 1280, 720, 0, 0, 0]);
        }
    }
    state.dirty[2] = [
        u8::from(native.header[4] != 0),
        u8::from(native.header[4] == 1),
    ];
    state.root[39] = native.header[5];
    state
}
fn ready(state: &LayerBanks, native: &Native) -> ReadyReconstruction {
    let mut observed_jobs = Vec::new();
    let owners = state
        .reconstruction_plan()
        .into_iter()
        .map(|plan| {
            let slot = usize::from(plan.slot);
            observed_jobs.push([u32::from(plan.slot), 0]);
            observed_jobs.push([
                u32::from(plan.slot),
                match plan.back {
                    BackReconstruction::Load => 1,
                    BackReconstruction::CopyFront => 2,
                    BackReconstruction::Empty => 3,
                },
            ]);
            let front = basic(state.records[slot][0].layer_type(), [0; 4]).map(Prepared::Basic);
            let back = if plan.back == BackReconstruction::Load {
                basic(state.records[slot][1].layer_type(), [0; 4]).map(Prepared::Basic)
            } else {
                None
            };
            (plan, front, back)
        })
        .collect();
    assert_eq!(observed_jobs, native.jobs, "original18db0 physical jobs");
    assert_eq!(
        native
            .publishes
            .iter()
            .map(|p| (p[0], p[1]))
            .collect::<Vec<_>>(),
        (0..SLOTS as u32)
            .flat_map(|slot| [(0, slot), (1, slot)])
            .collect::<Vec<_>>(),
        "original19578 publishes even empty/unallocated rows, slotwise FRONT thenBACK"
    );
    ReadyReconstruction {
        state: Box::new(state.clone()),
        owners,
    }
}
fn shape(layers: &HigurashiLayers, bank: usize) -> Vec<(i32, Option<u32>, i32)> {
    layers
        .reference_group_entries(bank)
        .into_iter()
        .map(|(key, remaining, owner)| (key, (key < 0).then_some(remaining), owner.owner_id()))
        .collect()
}
fn native_shape(snapshot: &Snapshot, bank: usize) -> Vec<(i32, Option<u32>, i32)> {
    snapshot.1[bank + 1]
        .iter()
        .map(|row| (row.0, (row.0 < 0).then_some(row.1), row.3 as i32))
        .collect()
}
fn retained(layers: &HigurashiLayers) -> Vec<(usize, usize, String)> {
    (0..2)
        .flat_map(|bank| {
            layers.retiring[bank].entries().iter().map(move |entry| {
                let owner = entry.owner.as_ref().unwrap();
                (
                    bank,
                    owner as *const HigurashiUserLayer as usize,
                    format!("{owner:?}"),
                )
            })
        })
        .collect()
}
fn check_history(scene: &HigurashiScene, snapshot: &Snapshot, profile: usize) {
    let actual = scene.history().reference_snapshot();
    assert_eq!(
        actual.0,
        [0, 0],
        "profile{profile}: explicitly unbound CPU graphics"
    );
    assert_eq!(
        (actual.1, actual.2.unwrap_or(0)),
        (u64::from(snapshot.0[0]), u64::from(snapshot.0[1])),
        "profile{profile}: ordered logical current1498/previous14a0 ownership"
    );
    assert_eq!(
        scene.layers().root.values()[39].to_bits(),
        snapshot.0[10],
        "profile{profile}: actual root39 sample"
    );
}
fn fixture(native: &Native) -> (HigurashiScene, u32) {
    let mut scene = HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4]));
    scene
        .layers_mut()
        .unwrap()
        .root
        .tweener_mut(39)
        .unwrap()
        .fast_forward_to(752.);
    for _ in 0..native.header[6] {
        scene.advance(Ticks::ZERO, true, false, None);
    }
    // Original real Screen constructor has consumed16 OS samples here.
    let mut cursor = 16;
    let layers = scene.layers_mut().unwrap();
    for bank in 0..2 {
        for slot in 0..3 {
            let owner = source(
                (slot % 2) as i32,
                1000 + 100 * bank as i32 + slot as i32,
                &mut cursor,
            );
            if slot < 2 {
                layers.retiring[bank].prepend_retiring(owner, (5 + 4 * slot) as f32);
            } else {
                layers.owners[slot][bank] = Some(owner);
            }
        }
    }
    if native.header[8] != 0 {
        let mut outgoing = HigurashiLayers::new(|| [1, 2, 3, 4]);
        if native.header[7] != 0 {
            outgoing.retiring[0].prepend_retiring(source(0, 900, &mut cursor), 4.);
            outgoing.owners[1][0] = Some(source(1, 901, &mut cursor));
        }
        let previous = outgoing.reference_rotate_planes().unwrap();
        scene.reference_set_transition(previous, 1000);
    } else {
        assert_eq!(
            native.header[7], 0,
            "corpus has no bare outgoing withoutwiper"
        );
    }
    (scene, cursor)
}

#[test]
fn higurashi_live_in_place_restoration_matches_36_original_retained_profiles() {
    let corpus: Vec<Native> = serde_json::from_str(OBSERVATIONS).unwrap();
    let outputs: Vec<_> = OUTPUT.lines().collect();
    assert_eq!(corpus.len(), 36);
    assert_eq!(outputs.len(), corpus.len());
    let mut row_checks = 0;
    let mut preserved = 0;
    for (index, native) in corpus.iter().enumerate() {
        assert_eq!(native.header[0], index as i32);
        let before_native = &native.snapshots["B"];
        let after_native = &native.snapshots["A"];
        let next_native = &native.snapshots["U"];
        let state = stored(native);
        let (mut scene, mut cursor) = fixture(native);
        check_history(&scene, before_native, index);
        for bank in 0..2 {
            assert_eq!(
                shape(scene.layers(), bank),
                native_shape(before_native, bank),
                "profile{index}/before/bank{bank}"
            );
        }
        let before = retained(scene.layers());
        let weak = scene.history().weak();
        let history_before = scene.history().reference_snapshot();
        let bases = [
            scene.layers().root.oscillators,
            scene.layers().screen[0].oscillators,
            scene.layers().screen[1].oscillators,
            scene.layers().page.oscillators,
        ];
        let old_front = scene.layers().owners[2][0]
            .as_ref()
            .unwrap()
            .properties
            .oscillators;
        let prepared = ready(&state, native);
        assert_eq!(
            retained(scene.layers()),
            before,
            "profile{index}: preparation does not mutate private queues/geometry"
        );
        let mut settings = SaveData::default();
        settings.extra_word = 0xdead_beef;
        settings.save_menu_position = 73;
        settings.save_vectors.extra_words_0 = vec![0x1357_9bdf, 0xfedc_ba98];
        let settings_before = serde_json::to_vec(&settings).unwrap();
        let os_before = cursor;
        let mt_before = crate::engine_random::seed_call_count();
        scene
            .publish_reconstruction_in_place(
                prepared,
                || next_seeds(&mut cursor),
                Some(&mut settings),
            )
            .unwrap();
        let delta: [u32; 2] = outputs[index]
            .split_once(" s ")
            .unwrap()
            .1
            .split_whitespace()
            .take(2)
            .map(|word| word.parse().unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_eq!(
            [
                cursor - os_before,
                (crate::engine_random::seed_call_count() - mt_before) as u32
            ],
            delta,
            "profile{index}: actual19578 constructor-only OS/MT consumption"
        );
        assert_eq!(
            u64::from(cursor),
            native.balance[0],
            "profile{index}: source pluspublication OS"
        );
        assert!(
            scene.transition().is_none(),
            "profile{index}: b905c clears outgoing/wiper"
        );
        assert!(!scene.command_pending());
        assert!(
            Weak::ptr_eq(&weak, &scene.history().weak()),
            "profile{index}: same Scene history state allocation"
        );
        assert_eq!(
            scene.history().reference_snapshot(),
            history_before,
            "profile{index}: no publication swap/allocation/retirement"
        );
        assert_eq!(
            retained(scene.layers()),
            before,
            "profile{index}: retained complete Null/Tile properties/queuedcontrols/addresses"
        );
        preserved += before.len();
        check_history(&scene, after_native, index);
        for bank in 0..2 {
            assert_eq!(
                shape(scene.layers(), bank),
                native_shape(after_native, bank),
                "profile{index}/published/bank{bank}"
            );
            // Only negative entries remain outside the immutable physical
            // keyed rows; stale keyed slots0/1 have been explicitly cleared.
            for slot in 0..SLOTS {
                let owner = scene.layers().owners[slot][bank].as_ref();
                let published = &native.publishes[2 * slot + bank];
                assert_eq!(
                    owner.is_some(),
                    published[2] != 0,
                    "profile{index}/slot{slot}/bank{bank}"
                );
                if let Some(owner) = owner {
                    assert_eq!(owner.owner_id(), state.records[slot][bank].words[1]);
                    let expected: [f32; 65] = std::array::from_fn(|i| {
                        state.records[slot][bank].words[PROPERTIES_WORD + i] as f32
                    });
                    assert_eq!(owner.properties.values(), expected);
                    assert!((0..65).all(|p| owner.properties.tweener(p).unwrap().is_idle()));
                    let actual = owner
                        .properties
                        .oscillators
                        .map(|o| (o.seed, o.phase.to_bits()));
                    if bank == 0 || native.header[4] == 2 {
                        assert_eq!(
                            actual,
                            old_front.map(|o| (o.seed, o.phase.to_bits())),
                            "profile{index}: inherited FRONT/COPY seeds andphases"
                        );
                    } else {
                        assert_eq!(
                            actual,
                            std::array::from_fn(|i| (0xabc0_0000 + os_before + i as u32 + 1, 0)),
                            "profile{index}: separate BACK constructor"
                        );
                    }
                }
                row_checks += 1;
            }
        }
        for (actual, old) in [
            scene.layers().root.oscillators,
            scene.layers().screen[0].oscillators,
            scene.layers().screen[1].oscillators,
            scene.layers().page.oscillators,
        ]
        .into_iter()
        .zip(bases)
        {
            assert_eq!(
                actual.map(|o| (o.seed, o.phase.to_bits())),
                old.map(|o| (o.seed, o.phase.to_bits()))
            );
        }
        assert_eq!(
            replay_snapshot(&scene.layers().state),
            replay_snapshot(&state)
        );
        assert_eq!(
            serde_json::to_vec(&settings).unwrap(),
            settings_before,
            "profile{index}: Basic restore does not mutate complete settings"
        );
        if native.header[9] != 0 {
            assert_eq!(native.pending, Some([1, 1, 0, 0]));
            assert_eq!(native.complete, Some([0, -1, 0, 1, 1]));
            // Actual original task's completion snapshot leaves all observable
            // Screen/groups/history untouched. Desktop outer task is separate.
            assert_eq!(native.snapshots["H"].0, after_native.0);
            assert_eq!(native.snapshots["H"].1, after_native.1);
        }
        scene.advance(Ticks::from_f32(1.), true, false, None);
        check_history(&scene, next_native, index);
        assert!(Weak::ptr_eq(&weak, &scene.history().weak()));
        assert_eq!(
            scene.history().reference_snapshot().3,
            native.balance[2],
            "profile{index}: history allocation ownership through nexttick"
        );
        for bank in 0..2 {
            assert_eq!(
                shape(scene.layers(), bank),
                native_shape(next_native, bank),
                "profile{index}/next enabled tick/bank{bank}"
            );
        }
        let advanced = retained(scene.layers());
        for (old, current) in before.iter().zip(&advanced) {
            assert_eq!(
                (old.0, old.1),
                (current.0, current.1),
                "profile{index}: enabled update retains same retiring owner allocation"
            );
            assert_ne!(
                old.2, current.2,
                "profile{index}: queued clocks advance onceafter publication"
            );
        }
        assert_eq!(serde_json::to_vec(&settings).unwrap(), settings_before);
    }
    assert_eq!(row_checks, 2304);
    assert_eq!(preserved, 144);
    println!(
        "Live in-place Higurashi restoration:36 frozen original profiles/2304 keyed memberships/144 retained complete CPU owners; FRONT/BACK jobs,constructor OS/MT,queued-clock/address retention,b905c outgoing retirement,same history allocation andnext enabled tick match. Asset readiness/GPU textures/full fade-task transport remain separate boundaries."
    );
}

#[test]
fn higurashi_pending_prepared_wipe_keeps_ordinary_bank_access_guarded() {
    let corpus: Vec<Native> = serde_json::from_str(OBSERVATIONS).unwrap();
    let native = &corpus[0];
    let state = stored(native);
    let (mut scene, _) = fixture(native);
    // Native reconstruction has its own ordered source VM retirement path.
    // Ordinary command dispatch and direct bank access must still be blocked.
    let wipe = HigurashiRuntimeCommand::WIPE {
        args: [0, 1, 60],
        params: UntypedNumberArray::default(),
    };
    assert_eq!(
        scene.dispatch(&wipe, false).unwrap(),
        Some(crate::adv::higurashi::scene::CommandStatus::Pending)
    );
    assert!(scene.command_pending());
    let before = (
        replay_snapshot(&scene.layers().state),
        retained(scene.layers()),
        scene.history().reference_snapshot(),
    );
    let mut seeds = 0;
    assert!(scene.layers_mut().is_err());
    assert_eq!(seeds, 0);
    assert_eq!(
        (
            replay_snapshot(&scene.layers().state),
            retained(scene.layers()),
            scene.history().reference_snapshot()
        ),
        before
    );
    assert!(scene.transition().is_some());

    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    let _prepared_wipe = layers.prepare_back();
    assert!(layers.pending_front_ids.is_some());
    let before = format!("{:?}/{:?}/{:?}", layers.state, layers.root, layers.page);
    assert!(
        layers
            .publish_reconstruction_in_place(
                ready(&state, native),
                || {
                    seeds += 1;
                    [0; 4]
                },
                None
            )
            .is_err()
    );
    assert_eq!(seeds, 0);
    assert_eq!(
        format!("{:?}/{:?}/{:?}", layers.state, layers.root, layers.page),
        before
    );
    assert!(layers.pending_front_ids.is_some());
}
