//! Actual Title parent child failure/retry and Config save preparation ownership.
use super::super::motion::Phase;
use super::*;
use crate::{
    adv::{GlobalData, VmState, assets::AdvAssets, config::ConfigPhase, save_menu::SaveMenuPhase},
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
};
use shin_core::format::save::SaveData;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary assets/progress only"]
fn reference_title_child_failure_preserves_globals_and_retries() {
    std::thread::Builder::new()
        .name("title-child-recovery".into())
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
        .expect("Failed title child must recover locally without stopping SceneHost");
    *number += 1;
}
fn activate(scene: &mut TitleScene, globals: &mut GlobalData, number: &mut u32) {
    step(scene, globals, number, 7.0, Some(AppAction::Enter));
    step(scene, globals, number, 0.0, None);
}
fn ready(scene: &TitleScene) -> bool {
    match scene.children.last() {
        Some(TitleChild::Config { screen, .. }) => screen.model().is_some(),
        Some(TitleChild::Manual(screen)) => screen.model().is_some(),
        Some(TitleChild::Load { screen, .. }) => screen.model().phase() != SaveMenuPhase::Loading,
        _ => false,
    }
}
fn parent_ready(scene: &TitleScene) -> bool {
    scene.children.is_empty()
        && scene.motion.phase() == Phase::Active
        && scene.motion.frame().black_alpha == 0.0
}
fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_TITLE_CHILD_OUTPUT").unwrap());
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
        ("config-short", TitleItem::Config, "/config.txa", None),
        (
            "config-system-short",
            TitleItem::Config,
            "/systex.txa",
            None,
        ),
        (
            "config-window-name",
            TitleItem::Config,
            "/config.txa",
            Some(b"window\0".as_slice()),
        ),
        (
            "config-unused-scroll-name",
            TitleItem::Config,
            "/systex.txa",
            Some(b"scroll\0".as_slice()),
        ),
        ("manual-short", TitleItem::Manual, "/manual.txa", None),
        (
            "manual-caption-name",
            TitleItem::Manual,
            "/manual.txa",
            Some(b"caption\0".as_slice()),
        ),
        (
            "manual-no-pages",
            TitleItem::Manual,
            "/manual.txa",
            Some(b"001\0".as_slice()),
        ),
        ("load-short", TitleItem::Load, "/saveload.txa", None),
        ("load-system-short", TitleItem::Load, "/systex.txa", None),
        (
            "load-window-name",
            TitleItem::Load,
            "/saveload.txa",
            Some(b"window\0".as_slice()),
        ),
        (
            "load-caption-name",
            TitleItem::Load,
            "/saveload.txa",
            Some(b"caption_load\0".as_slice()),
        ),
        (
            "load-scroll-name",
            TitleItem::Load,
            "/systex.txa",
            Some(b"scroll\0".as_slice()),
        ),
        (
            "load-unused-caption-save",
            TitleItem::Load,
            "/saveload.txa",
            Some(b"caption_save\0".as_slice()),
        ),
        (
            "load-factory-missing-resources",
            TitleItem::Load,
            "/saveload.txa",
            None,
        ),
        (
            "config-save-notice-short",
            TitleItem::Config,
            "/systex.txa",
            None,
        ),
        (
            "load-confirmation-short",
            TitleItem::Load,
            "/systex.txa",
            None,
        ),
    ] {
        if std::env::var("SHIN_REFERENCE_TITLE_CHILD_PROFILE").is_ok_and(|only| only != profile) {
            continue;
        }
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
        save.persist_data.set(17, 3);
        save.save_menu_position = 91;
        save.manual_save_slots[91] = Some(VmState::for_scenario(0, 42).snapshot_game_data());
        let disk = output.join(format!("progress-{profile}.dat"));
        std::fs::write(&disk, save.encode()).unwrap();
        let mut globals = GlobalData::open(Some(disk.clone()), None).unwrap();
        let original_disk = std::fs::read(&disk).unwrap();
        let mut before = globals.save().encode();
        let setup = TitleSetup::from_save(globals.save(), item as i32);
        let mut scene = TitleScene::new(server.clone(), setup);
        let missing_resources = profile == "load-factory-missing-resources";
        if !missing_resources {
            scene.configure_load(assets.scenario.clone(), manager.clone(), (1920, 1080));
        }
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
        assert!(parent_ready(&scene));
        let selected = scene.motion.selected();
        let late = profile == "config-save-notice-short" || profile == "load-confirmation-short";
        let mut bad = bytes.clone();
        if let Some(name) = rename {
            let at = bad.windows(name.len()).position(|w| w == name).unwrap();
            bad[at + name.len() - 2] = b'x';
        } else {
            bad.truncate(15);
        }
        if !late && !missing_resources {
            std::fs::write(&file, &bad).unwrap();
        }
        activate(&mut scene, &mut globals, &mut number);
        if profile == "config-unused-scroll-name" || profile == "load-unused-caption-save" {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready(&scene) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}: unused artwork name must not reject this mode"
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
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            while !parent_ready(&scene) {
                assert!(Instant::now() < deadline);
                step(&mut scene, &mut globals, &mut number, 12.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(globals.save().encode(), before);
            assert_eq!(std::fs::read(&disk).unwrap(), original_disk);
            std::fs::remove_file(file).unwrap();
            cases.push(serde_json::json!({"profile":profile,"asset":path,"unused_name_accepted":true,
                "global_progress_preserved":true,"normal_close":true,"working_progress_written":false}));
            continue;
        }
        if late {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready(&scene) {
                assert!(Instant::now() < deadline);
                step(&mut scene, &mut globals, &mut number, 0.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
            step(&mut scene, &mut globals, &mut number, 12.0, None);
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            if item == TitleItem::Config {
                step(
                    &mut scene,
                    &mut globals,
                    &mut number,
                    0.0,
                    Some(AppAction::Left),
                );
                assert_ne!(
                    globals.save().encode(),
                    before,
                    "Config change must publish before failure"
                );
                before = globals.save().encode();
            }
            std::fs::write(&file, &bad).unwrap();
            step(
                &mut scene,
                &mut globals,
                &mut number,
                7.0,
                Some(if item == TitleItem::Config {
                    AppAction::Cancel
                } else {
                    AppAction::Enter
                }),
            );
            step(&mut scene, &mut globals, &mut number, 0.0, None);
        }
        if profile == "config-save-notice-short" {
            // Save preparation failed before any storage transaction: keep the
            // same editable screen and its dirty initial-settings baseline.
            assert_eq!(
                scene.reference_config().unwrap().model().unwrap().phase(),
                ConfigPhase::Active
            );
            assert_eq!(scene.children.len(), 1);
            assert!(!scene.has_save_task());
            assert_eq!(globals.save().encode(), before);
            assert_eq!(std::fs::read(&disk).unwrap(), original_disk);
            std::fs::write(&file, &bytes).unwrap();
            step(
                &mut scene,
                &mut globals,
                &mut number,
                7.0,
                Some(AppAction::Cancel),
            );
            step(&mut scene, &mut globals, &mut number, 0.0, None);
            assert!(
                scene.has_save_task(),
                "Retried dirty close must own the save task"
            );
        } else {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !parent_ready(&scene) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}: failed child retained parent"
                );
                step(&mut scene, &mut globals, &mut number, 12.0, None);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(scene.motion.selected(), selected);
            assert_eq!(globals.save().encode(), before);
            assert_eq!(std::fs::read(&disk).unwrap(), original_disk);
            assert!(!scene.has_save_task());
            assert!(scene.take_destination().is_none() && scene.take_load_game_data().is_none());
            std::fs::write(&file, &bytes).unwrap();
            if missing_resources {
                scene.configure_load(assets.scenario.clone(), manager.clone(), (1920, 1080));
            }
            activate(&mut scene, &mut globals, &mut number);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ready(&scene) {
                assert!(
                    Instant::now() < deadline,
                    "{profile}: repaired child did not load"
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
            step(&mut scene, &mut globals, &mut number, 0.0, None);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !parent_ready(&scene) {
            assert!(
                Instant::now() < deadline,
                "{profile}: healthy child/save did not close; title={:?}, children={}, config={:?}, save={:?}",
                scene.motion.phase(),
                scene.children.len(),
                scene
                    .reference_config()
                    .and_then(|screen| screen.model())
                    .map(|model| model.phase()),
                scene.children.iter().find_map(|child| match child {
                    TitleChild::Config {
                        save: Some(task), ..
                    } => Some(task.reference_state()),
                    _ => None,
                })
            );
            step(&mut scene, &mut globals, &mut number, 12.0, None);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(scene.motion.selected(), selected);
        assert_eq!(globals.save().encode(), before);
        assert!(scene.take_destination().is_none() && scene.take_load_game_data().is_none());
        assert!(!scene.has_save_task());
        if profile == "config-save-notice-short" {
            let persisted = SaveData::decode(&std::fs::read(&disk).unwrap()).unwrap();
            assert!(persisted.settings.serialized_eq(&globals.save().settings));
            assert_eq!(
                persisted.manual_save_slots[91]
                    .as_ref()
                    .unwrap()
                    .entry
                    .random_seed,
                42
            );
            assert_eq!(persisted.save_menu_position, 91);
        } else {
            assert_eq!(std::fs::read(&disk).unwrap(), original_disk);
        }
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"asset":path,"global_progress_preserved":true,
            "selected_title_row_retained":true,"same_parent_same_server_retry":true,"normal_close":true,
            "working_progress_written":false,"config_dirty_save_retry":profile=="config-save-notice-short"}));
    }
    // The opposite caption is also unused in Save mode. Use a fresh server,
    // so this control cannot accidentally reuse the preceding Load atlas.
    let bytes = supplied_payload("/saveload.txa").2;
    let file = directory.join("saveload.txa");
    let mut altered = bytes.clone();
    let at = altered
        .windows(13)
        .position(|w| w == b"caption_load\0")
        .unwrap();
    altered[at + 11] = b'x';
    std::fs::write(&file, &altered).unwrap();
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
    let mut save = crate::adv::save_menu_screen::SaveMenuScreen::new(
        server.clone(),
        manager.clone(),
        assets.scenario.clone(),
        SaveData::default(),
        Some(VmState::for_scenario(0, 42).snapshot_game_data()),
        (1920, 1080),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while save.model().phase() == SaveMenuPhase::Loading {
        assert!(Instant::now() < deadline);
        save.update(Ticks::ZERO).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(save.model().is_save());
    std::fs::remove_file(file).unwrap();
    std::fs::write(output.join("title-child-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases_passed":cases.len(),"cases":cases,
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"window_created":false,"working_progress_written":false,
        "audio_device":"exact Loopback Audio","storage":"isolated temporary progress files","save_mode_unused_load_caption_accepted":true})).unwrap()).unwrap();
    println!(
        "{} actual Title child failures preserve globals/parent and retry;dirty Config retry persists settings",
        cases.len()
    );
}
