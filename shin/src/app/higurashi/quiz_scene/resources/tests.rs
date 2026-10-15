use super::*;
use crate::app::higurashi::quiz_scene::tests::append_string;
use crate::layer::user::higurashi::HigurashiLayerKind;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[test]
#[ignore = "actuala08e4 resource worker/bc3fc BUP metadata/bca14 filenames/selected question TXAs,checked file/atlas/mask/BUP constructors and disposal;no GPU/audio/window/saves"]
fn higurashi_quiz_resource_plan_matches_original_worker_paths_and_bup_metadata() {
    use std::{
        fmt::Write as _,
        io::Write as _,
        process::{Command, Stdio},
    };
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    for context in 0..9 {
        for pool in 0..8 {
            let seed = [1, 17, u32::MAX][(context as usize + pool as usize) % 3];
            let model = Model::new(
                crate::app::higurashi::tips_menu::tests::source(100, context as usize % 2),
                [pool, context, 9],
                seed,
            )
            .unwrap();
            let plan = Plan::new(&model, &scenario).unwrap();
            writeln!(input, "N {pool} {context} 9 {} {seed} 0\nW\nD", context % 2).unwrap();
            let mut row = "W".to_owned();
            append_string(&mut row, "/quiz.txa");
            append_string(&mut row, "/default.msk");
            for id in plan.bustups {
                let info = &scenario.bustup_info[id as usize];
                write!(row, " {id} {}", info.lipsync_character_id).unwrap();
                append_string(&mut row, &format!("/bustup/{}.bup", info.name.as_str()));
                append_string(&mut row, info.emotion.as_str());
            }
            for path in plan.questions {
                append_string(&mut row, &path);
            }
            expected.push(row);
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_MOTION").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_QUIZ_SCENE_CAPTURE") {
        let dir = std::path::Path::new(&dir).join("resource-worker");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    let rows = output.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), expected.len() * 3);
    for (i, expected) in expected.iter().enumerate() {
        assert!(rows[3 * i].starts_with("M 0 0 -1 "));
        assert_eq!(rows[3 * i + 1], expected, "native resource owner{i}");
        assert_eq!(rows[3 * i + 2], "D");
    }
    eprintln!(
        "Quiz native resources:72 original workers/216 observations;actual parent/mask/11 BUP metadata IDs-emotions-character flags/10 shuffled question TXA paths and allocation/refs/capture disposal pass;file/codec boundaries,no GPU/audio/window/saves"
    );
}

#[test]
#[ignore = "real updated quiz/default-mask/BUP expressions/question atlases through retained loader on offscreen Metal;no audio/window/saves"]
fn higurashi_quiz_resources_load_all_character_contexts_and_retire_producers() {
    let (_device, _queue, server) = crate::adv::higurashi::reference_gpu_assets();
    let server = Arc::new(server);
    let scenario = server.load_sync::<HigurashiScenario>("/main.snr").unwrap();
    let mut owners = 0;
    for context in 0..9 {
        let model = Model::new(
            crate::app::higurashi::tips_menu::tests::source(100, context as usize % 2),
            [context % 8, context, 9],
            17,
        )
        .unwrap();
        let plan = Plan::new(&model, &scenario).unwrap();
        let seeds = Arc::new(AtomicUsize::new(0));
        let counter = seeds.clone();
        let mut loader = Loader::new(server.clone(), scenario.clone(), plan.clone(), move || {
            counter.fetch_add(1, Ordering::SeqCst) as u32 + 1
        });
        let retirement = loader.job.as_ref().unwrap().retirement.clone();
        let end = Instant::now() + Duration::from_secs(20);
        while loader.poll().unwrap() != 2 {
            assert!(Instant::now() < end, "Quiz resources did not complete");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(loader.atlas.is_some());
        assert_eq!(seeds.load(Ordering::SeqCst), 44);
        assert!(!loader.published);
        loader.publish().unwrap();
        let prepared = loader.prepared.as_mut().unwrap();
        assert!(prepared.mask.is_some());
        for bustup in &prepared.bustups {
            assert_eq!(bustup.layer_type(), HigurashiLayerKind::Bustup);
            assert_eq!(bustup.properties.values()[0], -500.0);
        }
        if let Some(pair) = plan.bustups.windows(2).position(|v| v[0] == v[1]) {
            prepared.bustups[pair]
                .properties
                .tweener_mut(0)
                .unwrap()
                .fast_forward_to(123.0);
            assert_eq!(
                prepared.bustups[pair + 1].properties.values()[0],
                -500.0,
                "Shared BUP asset aliased mutable owner"
            );
        }
        loader.retire_first();
        assert!(loader.job.is_none());
        assert!(retirement.is_finished());
        assert!(loader.prepared.as_ref().unwrap().mask.is_none());
        assert!(loader.publish().is_err());
        owners += 1;
    }
    // Destroy a still-running producer and an atlas-published producer;the
    // retirement handle proves it cannot publish back into a later scene.
    for atlas_prefix in [false, true] {
        let model = Model::new(
            crate::app::higurashi::tips_menu::tests::source(100, 0),
            [0, 0, 9],
            1,
        )
        .unwrap();
        let (entered, reached) = mpsc::channel();
        let (resume, gate) = mpsc::channel();
        let mut first = true;
        let mut loader = Loader::new(
            server.clone(),
            scenario.clone(),
            Plan::new(&model, &scenario).unwrap(),
            move || {
                if atlas_prefix && first {
                    first = false;
                    entered.send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(20)).unwrap();
                }
                17
            },
        );
        let retirement = loader.job.as_ref().unwrap().retirement.clone();
        if atlas_prefix {
            reached.recv_timeout(Duration::from_secs(20)).unwrap();
            assert_eq!(loader.poll().unwrap(), 1);
            assert!(loader.atlas.is_some() && loader.prepared.is_none());
            assert!(!retirement.is_finished());
        }
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2));
            let _ = resume.send(());
        });
        drop(loader);
        release.join().unwrap();
        assert!(retirement.is_finished());
        owners += 1;
    }
    eprintln!(
        "Quiz resources:{owners} real loader owners;all9 character contexts/99 independent BUP expression owners/90 selected question atlases/parent atlas/default mask,44 fresh owner seeds per complete worker,initial-500 placement,publication/First-mask retirement and early producer disposal pass;offscreen Metal,no audio/window/saves"
    );
}
