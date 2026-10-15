//! Actual TitleScene/chooser viewer recovery using temporary artwork only.
use super::*;
use crate::{
    adv::{GlobalData, assets::AdvAssets},
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
    title::{TitleSetup, motion::Phase, scene::TitleScene},
};
use shin_core::format::save::SaveData;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; no window or working save writes"]
fn reference_title_viewer_artwork_failure_recovers_and_retries() {
    std::thread::Builder::new()
        .name("title-viewer-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
fn step(
    scene: &mut TitleScene,
    globals: &mut GlobalData,
    number: &mut u32,
    delta: f32,
    action: Option<AppAction>,
) {
    let mut state = EnumMap::default();
    if let Some(a) = action {
        state[a] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
    }
    scene
        .update_with_globals(
            Ticks::from_f32(delta),
            &PointerFrame {
                number: *number,
                time_seconds: *number as f64 / 60.0,
                events: &[],
            },
            &state,
            globals,
            glam::vec2(1900.0, 1050.0),
            false,
            delta as f64 / 60.0,
            0.0,
        )
        .expect("Failed viewer must recover locally, without stopping SceneHost");
    *number += 1;
}
fn activate(scene: &mut TitleScene, globals: &mut GlobalData, number: &mut u32) {
    step(scene, globals, number, 7.0, Some(AppAction::Enter));
    step(scene, globals, number, 0.0, None);
}
fn loaded(viewer: &Viewer) -> bool {
    match viewer {
        Viewer::Characters { screen, .. } => screen.is_some(),
        Viewer::Tips { screen, .. } => screen.model().is_some(),
    }
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_TITLE_FAILURE_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for (profile, item, path, rename) in [
        ("chars-short", TitleItem::Characters, "/chars.txa", None),
        (
            "character-system-short",
            TitleItem::Characters,
            "/systex.txa",
            None,
        ),
        (
            "character-window-name",
            TitleItem::Characters,
            "/chars.txa",
            Some(b"window\0".as_slice()),
        ),
        (
            "character-scroll-name",
            TitleItem::Characters,
            "/systex.txa",
            Some(b"scroll\0".as_slice()),
        ),
        ("tips-short", TitleItem::Tips, "/tips.txa", None),
        (
            "tips-window-name",
            TitleItem::Tips,
            "/tips.txa",
            Some(b"window\0".as_slice()),
        ),
        (
            "tips-scroll-name",
            TitleItem::Tips,
            "/systex.txa",
            Some(b"scroll\0".as_slice()),
        ),
    ] {
        let bytes = supplied_payload(path).2;
        let file = directory.join(&path[1..]);
        std::fs::write(&file, &bytes).unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
        let mut save = SaveData::default();
        for episode in [0, 2, 7] {
            save.persist_data.set(17 + episode, 3);
        }
        save.save_menu_position = 91;
        let mut globals = GlobalData::open(None, Some(save.clone())).unwrap();
        let before = globals.save().encode();
        let setup = TitleSetup::from_save(&save, item as i32);
        let mut scene = TitleScene::new(server.clone(), setup);
        scene.configure_load(assets.scenario.clone(), manager.clone(), (1920, 1080));
        let mut number = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !scene.is_loaded() {
            assert!(Instant::now() < deadline);
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        step(&mut scene, &mut globals, &mut number, 60.0, None);
        for _ in 0..3 {
            step(&mut scene, &mut globals, &mut number, 0.0, None);
        }
        assert_eq!(scene.motion.phase(), Phase::Active);
        step(
            &mut scene,
            &mut globals,
            &mut number,
            100.0,
            Some(AppAction::Enter),
        );
        step(&mut scene, &mut globals, &mut number, 0.0, None);
        step(&mut scene, &mut globals, &mut number, 18.0, None);
        for _ in 0..2 {
            step(&mut scene, &mut globals, &mut number, 0.0, None);
        }
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Active
        );
        let selected = scene.reference_episodes().unwrap().model.selected_episode();
        let mut bad = bytes.clone();
        if let Some(name) = rename {
            let at = bad.windows(name.len()).position(|w| w == name).unwrap();
            bad[at + name.len() - 2] = b'x';
        } else {
            bad.truncate(15);
        }
        std::fs::write(&file, &bad).unwrap();
        activate(&mut scene, &mut globals, &mut number);
        assert_eq!(scene.reference_episodes().unwrap().viewers.len(), 1);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            if scene.reference_episodes().unwrap().viewers.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{profile}: failed viewer retained input ownership"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(scene.motion.phase(), Phase::Child);
        assert_eq!(scene.motion.frame().black_alpha, 0.0);
        assert_eq!(
            scene.reference_episodes().unwrap().model.selected_episode(),
            selected
        );
        assert_eq!(globals.save().encode(), before);
        assert!(scene.take_destination().is_none());
        std::fs::write(&file, &bytes).unwrap();
        // Allow the input task to become eligible after retiring the failed child.
        step(&mut scene, &mut globals, &mut number, 0.0, None);
        activate(&mut scene, &mut globals, &mut number);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !loaded(&scene.reference_episodes().unwrap().viewers[0]) {
            assert!(
                Instant::now() < deadline,
                "{profile}: repaired viewer did not load"
            );
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        step(&mut scene, &mut globals, &mut number, 12.0, None);
        step(&mut scene, &mut globals, &mut number, 0.0, None);
        step(
            &mut scene,
            &mut globals,
            &mut number,
            7.0,
            Some(AppAction::Cancel),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !scene.reference_episodes().unwrap().viewers.is_empty() {
            assert!(
                Instant::now() < deadline,
                "{profile}: healthy viewer did not close"
            );
            step(&mut scene, &mut globals, &mut number, 12.0, None);
        }
        step(&mut scene, &mut globals, &mut number, 0.0, None);
        assert_eq!(scene.motion.frame().black_alpha, 0.0);
        assert_eq!(globals.save().encode(), before);
        step(
            &mut scene,
            &mut globals,
            &mut number,
            7.0,
            Some(AppAction::Cancel),
        );
        for _ in 0..30 {
            if scene.motion.phase() == Phase::Active {
                break;
            }
            step(&mut scene, &mut globals, &mut number, 12.0, None);
        }
        assert_eq!(scene.motion.phase(), Phase::Active);
        assert!(scene.reference_episodes().is_none());
        assert_eq!(globals.save().encode(), before);
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"asset":path,"selected_episode":selected,
            "global_progress_preserved":true,"parent_dim_restored":true,"same_parent_same_server_retry":true,
            "healthy_viewer_and_chooser_close":true,"scene_host_error_avoided":true}));
    }
    std::fs::write(output.join("title-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases_passed":cases.len(),"cases":cases,
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"working_progress_written":false,"window_created":false})).unwrap()).unwrap();
    println!(
        "{} actual title viewer failures recover parent/global ownership and retry",
        cases.len()
    );
}
