use super::*;
use crate::adv::higurashi::reference_gpu_assets;
use crate::app::higurashi::save_menu::preview::{Layer, Scene, globals};
use std::sync::atomic::{AtomicU32, Ordering};

#[test]
#[ignore = "actual 20df0/real Null factory and clone for absent PIC/BUP metadata, exact entropy and retained slot targets; offscreen asset server only, no audio/windows/user saves"]
fn higurashi_saved_preview_missing_catalog_factories_match_native_null_clones() {
    let (_, _, server) = reference_gpu_assets();
    let mut authored = Arc::try_unwrap(scenario(&[0, 0, 0])).unwrap();
    authored.picture_info.clear();
    authored.bustup_info.clear();
    let cases = [(2, -1), (2, 0), (3, -1), (3, 100)];
    let input = cases
        .iter()
        .map(|(kind, id)| format!("F {kind} 1 {id} 0 0 0 0 0 0 0\n"))
        .collect::<String>();
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_FACTORIES").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_FACTORIES_CAPTURE") {
        let path = std::path::Path::new(&path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("input.txt"), input).unwrap();
        std::fs::write(path.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(path.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), cases.len());
    for ((kind, id), row) in cases.into_iter().zip(output.lines()) {
        let words = row.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words.len(), 15);
        assert_eq!(
            &words[..10],
            &[
                "F",
                "1",
                "1",
                "4",
                "0",
                "0",
                "N4shin15NullLayerLoaderE",
                "N4shin9NullLayerE",
                "0",
                "0"
            ]
        );
        assert_eq!(words[14], "0");
        let count = Arc::new(AtomicU32::new(0xabc00001));
        let seeds: SeedSource = {
            let count = count.clone();
            Arc::new(move || count.fetch_add(1, Ordering::SeqCst))
        };
        let mut saved = Hydrated {
            records: [[-1; 79]; SLOTS],
            root: [0; 65],
            screen: [0; 65],
            labels: [Arc::from("A"), Arc::from("B")],
            scenario_id: 0,
        };
        // Retain a hole before the factory. A missing asset is a positive Null
        // owner, so neither its physical slot nor its restored targets vanish.
        let record = &mut saved.records[3];
        record.fill(0);
        record[0] = kind;
        record[1] = -123;
        record[6] = id;
        let values: [i32; 65] = std::array::from_fn(|i| i as i32 * 7 - 99);
        record[14..].copy_from_slice(&values);
        let scene = futures::executor::block_on(Scene::load(
            &server,
            &authored,
            &saved,
            [[0; 4]; 2],
            &seeds,
            0,
        ))
        .unwrap();
        assert_eq!(scene.layers.len(), 1);
        let (slot, owner_id, Layer::Basic(owner)) = &scene.layers[0] else {
            panic!("missing catalog entry must have a real basic Null clone")
        };
        assert_eq!((*slot, *owner_id), (3, -123));
        assert_eq!(owner.layer_type(), LayerType::Null);
        assert_eq!(owner.properties.values(), values.map(|v| v as f32));
        let native_seeds: [u32; 4] = std::array::from_fn(|i| words[10 + i].parse().unwrap());
        assert_eq!(owner.properties.oscillators.map(|o| o.seed), native_seeds);
        assert_eq!(
            count.load(Ordering::SeqCst) - 0xabc00001,
            words[3].parse::<u32>().unwrap()
        );
    }
    eprintln!(
        "Saved preview missing metadata:4 actual PIC/BUP factories/Null clones match;negative and absent IDs retain slot3/owner/65 targets and consume4 OS samples each;existing valid asset comparisons remain separate"
    );
}

#[test]
#[ignore = "actual integrated saved factory preparation/clones using real assets; native-proven nullable clone,OS/MT order,physical holes and notification/global behavior; offscreen asset server,no audio/windows/user saves"]
fn higurashi_saved_preview_extended_factories_preserve_clone_holes_entropy_and_globals() {
    let (_, _, server) = reference_gpu_assets();
    let authored = scenario(&[0, 0, 0]);
    let mut saved = Hydrated {
        records: [[-1; 79]; SLOTS],
        root: crate::layer::higurashi_properties::INITIAL_VALUES,
        screen: crate::layer::higurashi_properties::INITIAL_VALUES,
        labels: [Arc::from("A"), Arc::from("B")],
        scenario_id: 0,
    };
    for (slot, kind, params) in [
        (0, 0, [0; 8]),
        (1, 4, [6, 0, 0, 0, 0, 0, 0, 0]),
        (3, 7, [0; 8]),
        (4, 8, [640, 360, 0, 0, 0, 0, 0, 0]),
        (5, 9, [0, 100, 1500, 0, 0, 0, 0, 0]),
        (6, 13, [0; 8]),
        (7, 14, [0; 8]),
        (8, 12, [0, 3, 0, 17, 244, 0, 0, 0]),
        (9, 11, [7, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        let r = &mut saved.records[slot];
        r.fill(0);
        r[0] = kind;
        r[1] = 100 + slot as i32;
        r[6..14].copy_from_slice(&params);
        r[14..].copy_from_slice(&crate::layer::higurashi_properties::INITIAL_VALUES);
    }
    let original_records = saved.records;
    let mut settings = shin_core::format::save::higurashi::SaveData::default();
    let globals = globals::State::shared(globals::Snapshot::new(&settings));
    let count = Arc::new(AtomicU32::new(0xabc00001));
    let samples = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let seeds: SeedSource = {
        let count = count.clone();
        let samples = samples.clone();
        Arc::new(move || {
            samples.lock().push(crate::engine_random::seed_call_count());
            count.fetch_add(1, Ordering::SeqCst)
        })
    };
    crate::engine_random::with_reference_seed(5489, || {
        let before = crate::engine_random::seed_call_count();
        let scene = shin_tasks::block_on(Scene::load_with_globals(
            &server,
            &authored,
            &saved,
            [[1, 2, 3, 4], [1, 2, 11, 12]],
            &seeds,
            globals.clone(),
            &std::sync::atomic::AtomicBool::new(false),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            scene.layers.iter().map(|(s, _, _)| *s).collect::<Vec<_>>(),
            [0, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(count.load(Ordering::SeqCst), 0xabc00021);
        assert_eq!(crate::engine_random::seed_call_count(), before + 2);
        let expected = [0, 0, 0, 1, 2, 2, 2, 2]
            .into_iter()
            .flat_map(|n| [before + n; 4])
            .collect::<Vec<_>>();
        assert_eq!(
            *samples.lock(),
            expected,
            "four OS samples precede each derived constructor MT draw;NULL4 consumes neither"
        );
        for (index, (slot, id, owner)) in scene.layers.iter().enumerate() {
            assert_eq!(*id, 100 + *slot as i32);
            let properties = group::Drawable::properties(owner);
            assert_eq!(
                properties.values(),
                crate::layer::higurashi_properties::INITIAL_VALUES.map(|n| n as f32)
            );
            assert_eq!(
                properties.oscillators.map(|o| o.seed),
                std::array::from_fn(|i| 0xabc00001 + 4 * index as u32 + i as u32)
            );
            match *slot {
                0 | 3 => {
                    assert!(matches!(owner,Layer::Basic(v) if v.layer_type()==LayerType::Null))
                }
                4 | 5 => assert!(matches!(owner, Layer::Animation { .. })),
                6 | 7 => assert!(matches!(owner, Layer::Notification { .. })),
                8 | 9 => assert!(matches!(owner, Layer::Temporary { .. })),
                _ => unreachable!(),
            }
        }
    });
    globals.lock().synchronize(&mut settings).unwrap();
    for id in [0usize, 17, 244] {
        let bit = id * 2;
        assert_ne!(
            settings.save_vectors.extra_words_0[bit / 32] & (1 << (bit % 32)),
            0
        );
    }
    assert_eq!(settings.save_vectors.extra_words_0.len(), 256);
    assert_eq!(saved.records, original_records);
    eprintln!(
        "Saved extended factory integration:9 prepared records/8 positive clones retain slots0,3..9;NULL Animation slot1 and absent2 stay holes;32 OS samples/2 separate MT samples follow native ctor order;Movie-to-Null,Focus/Rain,13/14 notifications,Character/TIPS and before-publication acquired bits pass;no audio/windows/user saves"
    );
}
