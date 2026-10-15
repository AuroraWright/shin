//! Original acquisition668 matrix + real short-media live owner integration.
//! The CPU metadata check deliberately bounds media at a Null transport; the
//! separate ignored hardware check executes actual descriptors/codec owners.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo},
    layer::user::higurashi::{Prepared, movie::Services},
};
use shin_core::format::save::higurashi::SaveData;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

mod clip;
mod retirement;

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-movie-acquisition-capture/profiles.json"
));
const ORIGINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-movie-acquisition-capture/output.txt"
));

#[derive(Clone)]
struct Case {
    index: usize,
    op: String,
    movie: i32,
    salt: u32,
    setting: u8,
    bank: Option<u8>,
    banks: Option<u8>,
    flags: Option<u32>,
    old: Option<u32>,
}
struct Native {
    before: [u64; 3],
    after: [u64; 3],
    seeds: u32,
    calls: usize,
    targets: Vec<[u32; 5]>,
}
fn original(case: &Case, line: &str) -> Native {
    let words: Vec<_> = line.split_whitespace().collect();
    assert_eq!(words[0], case.op);
    assert_eq!(words[1].parse::<usize>().unwrap(), case.index);
    let mut out = Native {
        before: std::array::from_fn(|i| words[5 + i].parse().unwrap()),
        after: [0; 3],
        seeds: 0,
        calls: 0,
        targets: Vec::new(),
    };
    let mut i = 8;
    while i < words.len() {
        let tag = words[i];
        i += 1;
        let count = match tag {
            "s" => 1 + 5 * words[i].parse::<usize>().unwrap(),
            "N" => 4,
            "K" | "F" | "I" | "J" => 2,
            "j" => 1,
            "d" | "a" | "t" => 7,
            "c" | "o" | "k" | "r" => 6,
            "p" | "A" => 5,
            "g" => 14,
            "m" => 9,
            "e" => 11,
            _ => panic!("unrecognized original acquisition event{tag}"),
        };
        let data = &words[i..i + count];
        i += count;
        match tag {
            "s" => {
                out.targets = data[1..]
                    .chunks_exact(5)
                    .map(|entry| std::array::from_fn(|j| entry[j].parse().unwrap()))
                    .collect()
            }
            "g" => {
                assert_eq!(data[0].parse::<u32>().unwrap(), 0x93cc);
                assert_eq!(data[2], "7");
                assert_eq!(data[3].parse::<i32>().unwrap(), case.movie);
                assert_eq!(
                    std::array::from_fn::<_, 3, _>(|j| data[8 + j].parse::<u64>().unwrap()),
                    out.before
                );
                out.calls += 1;
            }
            "m" => assert_eq!(
                data.iter()
                    .map(|w| w.parse::<i32>().unwrap())
                    .collect::<Vec<_>>(),
                vec![1, case.movie, 5096, 0, 0, 0, 0, 0, 0]
            ),
            "e" => {
                assert_eq!(data[7], "0");
                out.seeds = data[6].parse().unwrap();
                assert_eq!(data[2].parse::<usize>().unwrap(), out.calls);
                out.after = std::array::from_fn(|j| data[8 + j].parse().unwrap());
            }
            _ => {}
        }
    }
    out
}
fn cases() -> Vec<(Case, Native)> {
    let values: serde_json::Value = serde_json::from_str(CORPUS).unwrap();
    let profiles: Vec<Case> = values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| Case {
            index: value["index"].as_u64().unwrap().try_into().unwrap(),
            op: value["op"].as_str().unwrap().to_owned(),
            movie: value["movie"].as_i64().unwrap().try_into().unwrap(),
            salt: value["salt"].as_u64().unwrap().try_into().unwrap(),
            setting: value["setting"].as_u64().unwrap().try_into().unwrap(),
            bank: value["bank"].as_u64().map(|word| word.try_into().unwrap()),
            banks: value["banks"].as_u64().map(|word| word.try_into().unwrap()),
            flags: value["flags"].as_u64().map(|word| word.try_into().unwrap()),
            old: value["old"].as_u64().map(|word| word.try_into().unwrap()),
        })
        .collect();
    assert_eq!(profiles.len(), 668);
    profiles
        .into_iter()
        .zip(ORIGINAL.lines())
        .map(|(case, line)| {
            let native = original(&case, line);
            (case, native)
        })
        .collect()
}
fn scenario() -> HigurashiScenario {
    let path = std::env::var("SHIN_HIGURASHI_SCENARIO")
        .expect("updated Scenario path for native Movie metadata");
    HigurashiScenario::new(std::fs::read(path).unwrap().into()).unwrap()
}
fn settings(salt: u32) -> SaveData {
    let mut settings = SaveData::default();
    let words = |bank: u32| {
        (0..256u32)
            .map(|i| {
                if salt == 0 {
                    0
                } else {
                    0xa5317b49 ^ bank.wrapping_mul(0x87654321) ^ i.wrapping_mul(0x10203041) ^ salt
                }
            })
            .collect()
    };
    settings.save_vectors.unlocked_cgs = words(0);
    settings.save_vectors.unlocked_bgms = words(1);
    settings.save_vectors.unlocked_movies = words(2);
    settings.settings.common.v0_bgmvol = 40;
    settings.settings.common.v1_sfxvol = 60;
    settings.settings.common.v2_voicevol = 80;
    settings
}
fn hashes(data: &SaveData) -> [u64; 3] {
    [
        &data.save_vectors.unlocked_cgs,
        &data.save_vectors.unlocked_bgms,
        &data.save_vectors.unlocked_movies,
    ]
    .map(|bank| {
        bank.iter()
            .flat_map(|word| word.to_le_bytes())
            .fold(0xcbf29ce484222325, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    })
}
fn seeds(counter: &mut u32) -> [u32; 4] {
    std::array::from_fn(|_| {
        *counter += 1;
        0xabc00000 + *counter
    })
}
fn setup(case: &Case, counter: &mut u32) -> HigurashiLayers {
    let mut layers = HigurashiLayers::new(|| seeds(counter));
    let count = if case.op == "M" { 3 } else { 1 };
    for slot in 0..count {
        let logical = if case.op == "M" { slot + 2 } else { 0 };
        assert_eq!(layers.state.allocate(logical as u16), Some(slot as u8));
        let bank = case
            .bank
            .unwrap_or_else(|| (case.banks.unwrap() >> slot) & 1);
        layers.state.back[logical] = bank;
        let prior = if case.op == "M" {
            case.old.unwrap() >> slot & 1
        } else {
            case.old.unwrap()
        };
        layers.state.records[slot][bank as usize].words[0] = if prior != 0 { 0 } else { -1 };
        if prior != 0 {
            let mut old = HigurashiUserLayer::null(seeds(counter));
            old.set_owner_id(91);
            for (index, value) in INITIAL_VALUES.iter().copied().enumerate() {
                old.properties
                    .tweener_mut(index)
                    .unwrap()
                    .fast_forward_to((value + (index % 5) as i32) as f32);
            }
            layers.owners[slot][bank as usize] = Some(old);
        }
    }
    if case.op == "M" {
        layers.state.select(2, 4);
    }
    layers
}
fn plan(layers: &HigurashiLayers, case: &Case) -> LayerLoadPlan {
    let plan = layers
        .plan_load(
            if case.op == "M" { -4 } else { 0 },
            7,
            case.flags.unwrap() as i32,
            (case.movie, 5096, 0, 0, 0, 0, 0, 0),
        )
        .unwrap();
    assert!(
        plan.immediate(),
        "actual Movie/Null descriptor71e74 is immediately ready"
    );
    plan
}
fn selected(plan: &LayerLoadPlan) -> Vec<[u32; 5]> {
    plan.targets
        .iter()
        .map(|(target, keep)| {
            [
                u32::from(target.slot),
                u32::from(target.logical),
                u32::from(target.bank),
                u32::from(*keep),
                plan.next.record(*target).words[1] as u32,
            ]
        })
        .collect()
}

#[test]
#[ignore = "updated Scenario metadata; pure CPU original668 bitmap oracle, bounded Null media transport; no audio/GPU/window/save"]
fn higurashi_movie_acquisition_matches_668_original_full_bitmap_profiles() {
    let scenario = scenario();
    let mut callbacks = 0;
    let mut published = 0;
    let mut hash_checks = 0;
    for (case, native) in cases() {
        let label = format!("original Movie acquisition{}", case.index);
        let mut data = settings(case.salt);
        assert_eq!(
            hashes(&data),
            native.before,
            "{label}:full initial bitmap payload"
        );
        hash_checks += 3;
        if case.op == "F" {
            // F does no acquisition. Its actual descriptor/asset boundary is
            // tested below; this CPU loop retains all three complete banks.
            assert_eq!(
                hashes(&data),
                native.after,
                "{label}:descriptor leaves bitmap banks untouched"
            );
            hash_checks += 3;
            continue;
        }
        let mt = crate::engine_random::seed_call_count();
        let mut counter = 0;
        let mut layers = setup(&case, &mut counter);
        let plan = plan(&layers, &case);
        if case.op == "M" {
            assert_eq!(
                selected(&plan),
                native.targets,
                "{label}:actual logical/physical/bank/keep/native-ID targets"
            );
        }
        let loaded = LoadedLayers {
            next: plan.next,
            owners: plan
                .targets
                .into_iter()
                .map(|(target,keep)|
            // Bound ONLY derived media: native publication/base/metadata path
            // below is the real production function; real Movie tested next.
            (target,keep,Prepared::Movie(None)))
                .collect(),
        };
        layers
            .publish_load_with_settings(loaded, || seeds(&mut counter), Some(&mut data))
            .unwrap();
        assert_eq!(
            counter, native.seeds,
            "{label}:inherited/fresh base OS-call count"
        );
        assert_eq!(
            crate::engine_random::seed_call_count(),
            mt,
            "{label}:Movie base/publication never consumes globalMT"
        );
        assert_eq!(
            hashes(&data),
            native.before,
            "{label}:publication precedes metadata dispatch"
        );
        hash_checks += 3;
        assert_eq!(
            layers.pending_metadata.len(),
            native.calls,
            "{label}:one requested callback iff any front"
        );
        layers
            .commit_pending_metadata(&scenario, Some(&mut data))
            .unwrap();
        assert_eq!(
            hashes(&data),
            native.after,
            "{label}:complete CG/BGM/Movie native hashes"
        );
        hash_checks += 3;
        assert!(layers.pending_metadata.is_empty());
        callbacks += native.calls;
        published += 1;
    }
    assert_eq!((published, callbacks, hash_checks), (608, 412, 5832));
    eprintln!(
        "Higurashi Movie metadata:668 unchanged original profiles/608 production plan-publication boundaries/412 callbacks/5832 full-bank hashes; derived media bounded Null, real codec owners separately tested"
    );
}

struct Fixtures {
    path: PathBuf,
}
impl Fixtures {
    fn new(audio: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "shin-higurashi-live-movie-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("movie")).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../reverse-engineering/higurashi/live-movie-owner-fixtures");
        let first = root.join(if audio {
            "short-aac.mp4"
        } else {
            "short-silent.mp4"
        });
        std::fs::copy(first, path.join("movie/mv01.mp4")).unwrap();
        for variant in ["pc", "cs"] {
            std::fs::copy(
                root.join("short-silent.mp4"),
                path.join(format!("movie/mv13-{variant}.mp4")),
            )
            .unwrap();
        }
        Self { path }
    }
    fn assets(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<AssetServer> {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&self.path).unwrap();
        Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Higurashi,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ))
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn tick_until(
    layers: &mut HigurashiLayers,
    predicate: impl Fn(&HigurashiLayers) -> bool,
    label: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(layers) {
        assert!(
            Instant::now() < deadline,
            "{label}:bounded short-codec transport did not reach expected state"
        );
        layers.advance(Ticks::ZERO, None);
        assert!(
            layers.movie_fault().is_none(),
            "{label}:{}",
            layers.movie_fault().unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "actual short H264/AAC owners, headless GPU and exact Loopback Audio; unique temporary asset storage, no window/save"]
fn higurashi_live_movie_owner_publication_reconstruction_and_scoped_retirement() {
    let (device, queue, _) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = scenario();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let fixtures = Fixtures::new(true);
    let assets = fixtures.assets(&device, &queue);
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let now = clock.clone();
    let services = Services::new(
        assets.clone(),
        manager.clone(),
        [40, 60, 80],
        Some(Arc::new(move || now.load(Ordering::Acquire))),
    );
    let corpus = cases();
    let mut descriptors = 0;
    for (case, native) in corpus.iter().filter(|(case, _)| case.op == "F") {
        let data = settings(case.salt);
        let requests = assets.reference_requested_paths().len();
        let prepared = shin_tasks::block_on(Prepared::load(
            &assets,
            &scenario,
            crate::layer::user::higurashi::HigurashiLayerKind::Movie,
            (case.movie, 5096, 0, 0, 0, 0, 0, 0),
            case.setting,
        ))
        .unwrap();
        let valid = (0..13).contains(&case.movie);
        assert_eq!(prepared.requires_media(), valid);
        assert_eq!(prepared.requires_settings(), valid);
        assert_eq!(
            assets.reference_requested_paths().len(),
            requests,
            "descriptor{} opened media before base construction",
            case.index
        );
        assert_eq!(hashes(&data), native.before);
        assert_eq!(hashes(&data), native.after);
        descriptors += 1;
    }
    assert_eq!(descriptors, 60);
    let mut initialized = 0;
    let mut bitmap_checks = 0;
    // Every native bankmask and flags combination, rotating old-owner masks
    // and valid/missing metadata. Expected all-bank hashes come from C only.
    for banks in 0..8u8 {
        for flags in 0..4u32 {
            let old = [0, 5, 7][(usize::from(banks) + flags as usize) % 3];
            let movie = [0, 12, 13][(usize::from(banks) + flags as usize) % 3];
            let (case, native) = corpus
                .iter()
                .find(|(case, _)| {
                    case.op == "M"
                        && case.banks == Some(banks)
                        && case.flags == Some(flags)
                        && case.old == Some(old)
                        && case.movie == movie
                })
                .unwrap();
            let label = format!("real Movie native selected profile{}", case.index);
            let mt = crate::engine_random::seed_call_count();
            let mut counter = 0;
            let mut layers = setup(case, &mut counter);
            layers.set_movie_services(services.clone());
            let mut data = settings(case.salt);
            let plan = plan(&layers, case);
            assert_eq!(
                selected(&plan),
                native.targets,
                "{label}:selected native targets"
            );
            let requests = assets.reference_requested_paths().len();
            let loaded =
                shin_tasks::block_on(plan.load_with_setting(&assets, &scenario, case.setting))
                    .unwrap();
            assert_eq!(
                assets.reference_requested_paths().len(),
                requests,
                "{label}:ready descriptor must not open Movie file"
            );
            layers
                .publish_load_with_settings(loaded, || seeds(&mut counter), Some(&mut data))
                .unwrap();
            assert_eq!(
                counter, native.seeds,
                "{label}:actual base seeds precede file/Player"
            );
            assert_eq!(
                crate::engine_random::seed_call_count(),
                mt,
                "{label}:Movie has no globalMT use"
            );
            assert_eq!(
                hashes(&data),
                native.before,
                "{label}:no pre-publication gallery mutation"
            );
            bitmap_checks += 3;
            assert_eq!(layers.pending_metadata.len(), native.calls);
            layers
                .commit_pending_metadata(&scenario, Some(&mut data))
                .unwrap();
            assert_eq!(
                hashes(&data),
                native.after,
                "{label}:native full bitmap hashes"
            );
            bitmap_checks += 3;
            let mut retirements = Vec::new();
            let mut identities = Vec::new();
            for slot in 0..3 {
                let bank = (banks >> slot) & 1;
                let owner = layers.owners[slot][bank as usize].as_ref().unwrap();
                let keep = flags & 1 != 0 && old >> slot & 1 != 0;
                assert_eq!(
                    owner.owner_id(),
                    if keep { 91 } else { slot as i32 + 2 },
                    "{label}:native owner ID independent of kept VM ID"
                );
                if movie < 13 {
                    assert_eq!(owner.layer_type().native_id(), 7);
                    let live = owner.reference_movie().unwrap();
                    let state = live.reference_state();
                    assert_eq!(
                        (
                            state.0, state.1, state.2, state.3, state.4, state.5, state.6
                        ),
                        (0, true, 0, None, None, false, true),
                        "{label}:publication exposes pending status before main-thread polling"
                    );
                    assert!(
                        owner.native_copy().is_none(),
                        "{label}:original Movie virtual38 is NULL"
                    );
                    identities.push(state.7);
                    retirements.push(live.reference_retirement().unwrap());
                    initialized += 1;
                } else {
                    assert_eq!(
                        owner.layer_type().native_id(),
                        0,
                        "{label}:missingInfo native Null fallback"
                    );
                }
            }
            assert_eq!(
                identities
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                identities.len(),
                "{label}:media owners never alias"
            );
            let before_copy = counter;
            let previous = layers.commit_back();
            assert_eq!(
                counter, before_copy,
                "{label}:WIPE/NULLcopy never samples OS seeds"
            );
            assert_eq!(
                layers.pending_metadata.len(),
                banks.count_ones() as usize,
                "{label}:WIPE marks changed records only"
            );
            for slot in 0..3 {
                let back = banks >> slot & 1 != 0;
                if movie < 13 {
                    assert_eq!(
                        layers.owners[slot][0].is_some(),
                        back,
                        "{label}:unchanged Movie copy is NULL; changed back retained"
                    );
                    if back {
                        assert_eq!(
                            layers.owners[slot][0]
                                .as_ref()
                                .unwrap()
                                .reference_movie()
                                .unwrap()
                                .reference_state()
                                .7,
                            identities[slot],
                            "{label}:actual back Movie retained at WIPE"
                        );
                    } else {
                        assert_eq!(
                            previous
                                .layer(slot as u8)
                                .unwrap()
                                .reference_movie()
                                .unwrap()
                                .reference_state()
                                .7,
                            identities[slot],
                            "{label}:outgoing Movie moved, never deep-cloned"
                        );
                    }
                } else {
                    assert!(
                        layers.owners[slot][0].is_some(),
                        "{label}:Null fallback copies normally"
                    );
                }
            }
            layers
                .commit_pending_metadata(&scenario, Some(&mut data))
                .unwrap();
            // An actual single front-load in the unchanged oracle is the
            // independent bit result after any valid changed-front WIPE.
            let expected = if banks != 0 {
                corpus
                    .iter()
                    .find(|(other, _)| {
                        other.op == "L"
                            && other.movie == movie
                            && other.salt == case.salt
                            && other.bank == Some(0)
                    })
                    .unwrap()
                    .1
                    .after
            } else {
                native.after
            };
            assert_eq!(
                hashes(&data),
                expected,
                "{label}:complete WIPE gallery payload"
            );
            bitmap_checks += 3;
            drop(previous);
            drop(layers);
            assert!(
                retirements
                    .iter()
                    .all(shin_tasks::LoaderRetirement::is_finished),
                "{label}:all unpublished codec/AAC descendants drained off worker"
            );
        }
    }
    // Nine exact original18db0/19578 profiles; imported original Player was
    // bounded there, while this test constructs the real paused codec owner.
    let reconstructed: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/live-movie-owner-fixtures/native-reconstruction.json"
    )))
    .unwrap();
    for native in reconstructed["profiles"].as_array().unwrap() {
        let old = native["old"].as_i64().unwrap() as i32;
        let back = native["back"].as_u64().unwrap() as u8;
        let mut counter = 0;
        let mut outgoing = HigurashiLayers::new(|| seeds(&mut counter));
        outgoing.set_movie_services(services.clone());
        let mut data = settings(0x173b5a29);
        let original_hash = hashes(&data);
        if old >= 0 {
            let mut owner = if old == 0 {
                HigurashiUserLayer::null(seeds(&mut counter))
            } else {
                let prepared = shin_tasks::block_on(Prepared::load(
                    &assets,
                    &scenario,
                    crate::layer::user::higurashi::HigurashiLayerKind::Movie,
                    (0, 5096, 0, 0, 0, 0, 0, 0),
                    0,
                ))
                .unwrap();
                prepared
                    .construct_with_services(
                        None,
                        Some(seeds(&mut counter)),
                        Some(&mut data),
                        Some(&services),
                    )
                    .unwrap()
                    .unwrap()
            };
            owner.set_owner_id(91);
            for (i, &value) in INITIAL_VALUES.iter().enumerate() {
                owner
                    .properties
                    .tweener_mut(i)
                    .unwrap()
                    .fast_forward_to((value + (i % 5) as i32) as f32);
            }
            for (i, oscillator) in owner.properties.oscillators.iter_mut().enumerate() {
                oscillator.phase = f32::from_bits(0x3e800000 + 0x00800000 * i as u32);
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
            outgoing.owners[0][0] = Some(owner);
        }
        let mut state = LayerBanks::default();
        for bank in 0..2 {
            state.records[0][bank].words[0] = 7;
            state.records[0][bank].words[1] = if bank == 0 { 500 } else { 900 };
            state.records[0][bank].words[2] = -1;
            state.records[0][bank].words[6..14].copy_from_slice(&[0, 5096, 0, 0, 0, 0, 0, 0]);
            state.records[0][bank].reset_properties();
        }
        state.dirty[0] = [u8::from(back != 0), u8::from(back == 1)];
        let before = counter;
        let result = shin_tasks::block_on(outgoing.reconstruct_with_settings(
            &state,
            &assets,
            &scenario,
            || seeds(&mut counter),
            0,
            Some(&mut data),
        ))
        .unwrap();
        assert_eq!(
            counter - before,
            native["delta"][0].as_u64().unwrap() as u32
        );
        assert_eq!(counter, native["total"][0].as_u64().unwrap() as u32);
        assert_eq!(
            hashes(&data),
            original_hash,
            "native reconstruction does not acquire gallery metadata"
        );
        let mut retirements = Vec::new();
        let mut identities = Vec::new();
        for (bank, tag) in [(0, "P"), (1, "B")] {
            let expected = &native[tag];
            let owner = result.owners[0][bank].as_ref();
            assert_eq!(
                owner.is_some(),
                !expected.is_null(),
                "original Movie back NULL-copy shape"
            );
            let Some(owner) = owner else {
                continue;
            };
            assert_eq!(owner.owner_id(), expected["id"].as_u64().unwrap() as i32);
            assert_eq!(
                owner.properties.values().map(f32::to_bits).to_vec(),
                json_words(&expected["values"])
            );
            let busy: Vec<_> = (0..65)
                .map(|i| u32::from(!owner.properties.tweener(i).unwrap().is_idle()))
                .collect();
            assert_eq!(busy, json_words(&expected["busy"]));
            let oscillators: Vec<_> = owner
                .properties
                .oscillators
                .iter()
                .flat_map(|o| [o.seed, o.phase.to_bits()])
                .collect();
            assert_eq!(oscillators, json_words(&expected["oscillators"]));
            let live = owner.reference_movie().unwrap();
            assert_eq!(live.status(), 0);
            identities.push(live.reference_state().7);
            retirements.push(live.reference_retirement().unwrap());
            initialized += 1;
        }
        assert_eq!(
            identities
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            identities.len()
        );
        assert!(
            result.owners[1..].iter().flatten().all(Option::is_none),
            "all remaining31 physical slots remain NULL"
        );
        drop(result);
        drop(outgoing);
        assert!(
            retirements
                .iter()
                .all(shin_tasks::LoaderRetirement::is_finished)
        );
    }
    eprintln!(
        "Live Higurashi Movie:32 actual selected publication/WIPE profiles/{bitmap_checks} original full-bank hashes plus9 exact original reconstruction profiles/{initialized} real short-media initializers; pending0,base keep/IDs/65targets/NULLcopy/retained banks/scoped retirement; category1 mixer-source routing remains named transport boundary, not a loop flag"
    );
}

fn json_words(value: &serde_json::Value) -> Vec<u32> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect()
}

#[test]
#[ignore = "two real8-frame H264/AAC live lifecycles, original program20 cache, headless720p/1080p and exact Loopback Audio; no window/save"]
fn higurashi_live_movie_owner_ready_eof_retained_cache_and_captured_wait() {
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let (device, queue, _) = crate::adv::higurashi::reference_gpu_assets();
    let scenario = scenario();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut rendered = 0;
    for audio in [false, true] {
        let fixtures = Fixtures::new(audio);
        let assets = fixtures.assets(&device, &queue);
        let clock = Arc::new(AtomicU64::new(1_000_000));
        let now = clock.clone();
        let services = Services::new(
            assets.clone(),
            manager.clone(),
            [40, 60, 80],
            Some(Arc::new(move || now.load(Ordering::Acquire))),
        );
        let (case, native) = cases()
            .into_iter()
            .find(|(case, _)| {
                case.op == "L"
                    && case.movie == 0
                    && case.salt == 0
                    && case.setting == 0
                    && case.bank == Some(0)
                    && case.flags == Some(0)
                    && case.old == Some(0)
            })
            .unwrap();
        let mut counter = 0;
        let mut layers = setup(&case, &mut counter);
        layers.set_movie_services(services);
        let mut data = settings(0);
        let loaded = shin_tasks::block_on(plan(&layers, &case).load(&assets, &scenario)).unwrap();
        layers
            .publish_load_with_settings(loaded, || seeds(&mut counter), Some(&mut data))
            .unwrap();
        layers
            .commit_pending_metadata(&scenario, Some(&mut data))
            .unwrap();
        assert_eq!(hashes(&data), native.after);
        let target = layers.state.target(0).unwrap();
        let live = layers
            .layer(target)
            .unwrap()
            .reference_movie()
            .unwrap()
            .clone();
        let retirement = live.reference_retirement().unwrap();
        assert_eq!(live.status(), 0);
        layers.state.select(0, 0);
        let prepared = super::movie_wait::MovieWait::capture(&layers, 0, 1).unwrap();
        let eof = super::movie_wait::MovieWait::capture(&layers, 0, 2).unwrap();
        let minus_two_ready = super::movie_wait::MovieWait::capture(&layers, -2, 1).unwrap();
        let minus_two_eof = super::movie_wait::MovieWait::capture(&layers, -2, 2).unwrap();
        assert!(!prepared.complete(&layers));
        assert!(!eof.complete(&layers));
        assert!(!minus_two_ready.complete(&layers));
        assert!(!minus_two_eof.complete(&layers));
        // Leave an arbitrarily large wall interval before consuming readiness.
        // Native342 control states prove this interval is not media elapsed.
        clock.store(50_000_000, Ordering::Release);
        assert_eq!(live.reference_state().2, 0);
        tick_until(
            &mut layers,
            |_| live.reference_state().4 == Some((64, 48)),
            "Movie first retained converted frame",
        );
        let first = live.reference_state();
        assert_eq!(first.0, 1);
        assert!(first.1);
        assert!(first.3.is_some());
        assert_eq!((first.4, first.5, first.6), (Some((64, 48)), false, false));
        if !audio {
            assert_eq!(
                first.2, 0,
                "preparation latency excluded by independent native clock"
            );
        }
        assert!(prepared.complete(&layers));
        assert!(!eof.complete(&layers));
        assert!(minus_two_ready.complete(&layers));
        assert!(!minus_two_eof.complete(&layers));
        // Original waits own captured physical bank0 even after later logical
        // selection/bank changes; a newly captured missing-bank wait completes.
        layers.state.select(2, 4);
        layers.state.back[0] = 1;
        assert!(!eof.complete(&layers));
        assert!(!minus_two_eof.complete(&layers));
        assert!(
            super::movie_wait::MovieWait::capture(&layers, -2, 2)
                .unwrap()
                .complete(&layers)
        );
        assert!(
            super::movie_wait::MovieWait::capture(&layers, 0, 2)
                .unwrap()
                .complete(&layers)
        );
        layers.state.back[0] = 0;
        manager.set_suspended(true);
        layers.advance(Ticks::ZERO, None);
        let suspended = live.reference_state();
        clock.store(60_000_000, Ordering::Release);
        layers.advance(Ticks::ZERO, None);
        assert_eq!(
            live.reference_state().3,
            suspended.3,
            "external gate suppresses frame publication"
        );
        assert!(
            live.reference_state().1,
            "external gate retains native desired playback flag"
        );
        manager.set_suspended(false);
        layers.advance(Ticks::ZERO, None);
        let before_skip = live.reference_state();
        layers.fast_forward_screen();
        assert_eq!(
            live.reference_state().3,
            before_skip.3,
            "native fast-forward is base-only; no media seek/still copy"
        );
        if !audio {
            clock.store(61_000_000, Ordering::Release);
        } else {
            clock.store(u64::MAX / 2, Ordering::Release);
        } // AAC uses actual PCM position, not wall deadline.
        tick_until(
            &mut layers,
            |_| live.status() == 2,
            "short Movie actual videoEOF plus PCM drain",
        );
        assert!(eof.complete(&layers));
        assert!(minus_two_eof.complete(&layers));
        let final_state = live.reference_state();
        assert_eq!(final_state.4, Some((64, 48)));
        assert!(!final_state.5);
        assert!(final_state.3.is_some());
        for extent in [(1280, 720), (1920, 1080)] {
            let render = |layers: &HigurashiLayers| {
                crate::adv::higurashi::reference_render_to_texture(
                    &device,
                    &queue,
                    extent,
                    |context, target| {
                        let mut pass = context.begin_pass(
                            TextureTarget {
                                view: target,
                                kind: TextureTargetKind::Screen,
                            },
                            Some(context.depth_stencil),
                            "live Movie retained final frame",
                        );
                        pass.clear(
                            Some(shin_core::primitives::color::UnormColor::BLACK),
                            Some(0),
                            Some(1.),
                        );
                        layers
                            .render_front_group(&mut pass, &Default::default())
                            .unwrap();
                    },
                )
            };
            let before = render(&layers);
            clock.fetch_add(1_000_000, Ordering::AcqRel);
            layers.advance(Ticks::from_f32(1000.), None);
            let after = render(&layers);
            let differing = before.iter().zip(&after).filter(|(a, b)| a != b).count();
            assert_eq!(
                differing, 0,
                "retained final Movie RGBA changes after EOF:{extent:?}"
            );
            assert!(
                before
                    .chunks_exact(4)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    >= 3,
                "real tiny decoded pattern projects visibly"
            );
            rendered += 2;
        }
        assert_eq!(
            live.reference_state().3,
            final_state.3,
            "EOF retains the exact final decoded timing/cache"
        );
        assert!(layers.layer(target).unwrap().native_copy().is_none());
        let native_identity = live.reference_state().7;
        if !audio {
            shin_tasks::block_on(layers.dispatch(
                &shin_core::vm::higurashi::command::HigurashiRuntimeCommand::LAYERBACK {
                    argument: 0,
                },
                &assets,
                &scenario,
                || seeds(&mut counter),
            ))
            .unwrap();
            assert!(
                layers.owners[0][1].is_none(),
                "actual LAYERBACK invokes native Movie NULLcopy"
            );
        }
        let previous = layers.commit_back();
        assert!(
            layers.owners[0][0].is_none(),
            "changed NULL back or unchanged Movie virtual38 yields incoming hole"
        );
        assert_eq!(
            previous
                .layer(0)
                .unwrap()
                .reference_movie()
                .unwrap()
                .reference_state()
                .7,
            native_identity,
            "outgoing original Movie retained without deep clone"
        );
        assert_eq!(
            layers.pending_metadata.len(),
            usize::from(!audio),
            "WIPE acquisition only for changed VM record, including NULL child"
        );
        drop(live);
        drop(previous);
        drop(layers);
        assert!(
            retirement.is_finished(),
            "final scene retirement drains scoped codec/AAC producer work"
        );
    }
    assert_eq!(rendered, 8);
    eprintln!(
        "Live Higurashi Movie:two real silent/AAC8-frame lifecycles/eight720p1080p retained RGBA captures; pending0→ready1→EOF+PCMdrain2,cache converted during update before render,captured wait bank,base-only skip,external suspension,NULLcopy/outgoing retention,scoped retirement; exact Loopback only,no original SDK codec/audio-source allocation claim"
    );
}
