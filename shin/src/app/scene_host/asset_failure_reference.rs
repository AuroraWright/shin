//! Supplied malformed fonts and typed dialog artwork through actual callers.
use super::*;
use crate::{
    adv::storage_retry_dialog::StorageRetryDialog,
    asset::{
        font::GpuFontLazy,
        system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    },
};
use shin_core::{format::save::SaveData, time::Ticks};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM,Metal and the default audio output;temporary assets/progress only"]
fn reference_common_effects_mandatory_artwork_recovery() {
    std::thread::Builder::new()
        .name("common-effects-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(common_effects_recovery)
        .unwrap()
        .join()
        .unwrap();
}

fn common_effects_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_COMMON_EFFECTS_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let supplied = crate::asset::system::supplied_payload("/effects.txa").2;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for profile in [
        "initialize",
        "lazy",
        "replacement",
        "startup",
        "unused-focusline",
    ] {
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
        let old_borrow = (profile == "replacement")
            .then(|| shin_tasks::block_on(server.initialize_effects_atlas()).unwrap());
        let old_identity = old_borrow.as_ref().map(|a| a.reference_identity());
        let old_weak = old_borrow.as_ref().map(|a| a.reference_weak());
        let mut bad = supplied.clone();
        let name = if profile == "unused-focusline" {
            "focusline"
        } else {
            "breakup"
        };
        let needle = format!("{name}\0");
        let at = bad
            .windows(needle.len())
            .position(|w| w == needle.as_bytes())
            .unwrap();
        bad[at + needle.len() - 2] = b'x';
        let file = directory.join("effects.txa");
        std::fs::write(&file, &bad).unwrap();
        let mut saved = SaveData::default();
        saved.settings.v0_bgmvol = 37;
        saved.save_menu_position = 91;
        let disk = output.join(format!("progress-{profile}.dat"));
        let disk_bytes = saved.encode();
        std::fs::write(&disk, &disk_bytes).unwrap();
        let mut globals = GlobalData::default();
        let before = globals.save().encode();
        let mut audio = GlobalAudio::new(manager.clone());
        let error = if profile == "startup" {
            let mut boot = FirstBoot::new(server.clone(), Some(disk.clone()));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match boot.update(&mut globals, &mut audio, Ticks::ZERO) {
                    Err(error) => break Some(format!("{error:#}")),
                    Ok(update) => {
                        if let Some(common) = update.assets {
                            // Same mandatory lookup as App's frame preparation.
                            common.effects.read().required("breakup");
                            panic!("Incomplete common effects atlas was published");
                        }
                        assert!(!update.complete);
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "Missing breakup did not report a startup error"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        } else {
            let result = if profile == "lazy" {
                shin_tasks::block_on(server.effects_atlas())
            } else {
                shin_tasks::block_on(server.initialize_effects_atlas())
            };
            match result {
                Err(error) => Some(format!("{error:#}")),
                Ok(borrower) => {
                    borrower.read().required("breakup");
                    assert_eq!(
                        profile, "unused-focusline",
                        "Missing mandatory artwork was published"
                    );
                    assert!(borrower.read().get("focusline").is_none());
                    None
                }
            }
        };
        if profile != "unused-focusline" {
            assert!(error.as_ref().unwrap().contains("breakup"));
            assert!(error.as_ref().unwrap().contains("/effects.txa"));
            assert_eq!(
                server.reference_effects_atlas().is_loaded(),
                old_borrow.is_some()
            );
        }
        if let Some(borrower) = &old_borrow {
            assert_eq!(Some(borrower.reference_identity()), old_identity);
            assert!(old_weak.as_ref().unwrap().upgrade().is_some());
            borrower.read().required("breakup");
        }
        assert_eq!(globals.save().encode(), before);
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        std::fs::write(&file, &supplied).unwrap();
        if profile == "startup" {
            let mut boot = FirstBoot::new(server.clone(), Some(disk.clone()));
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut common = None;
            loop {
                let update = boot.update(&mut globals, &mut audio, Ticks::ZERO).unwrap();
                if update.assets.is_some() {
                    common = update.assets;
                }
                if update.complete {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Repaired common assets did not complete"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            common.unwrap().effects.read().required("breakup");
            assert_eq!(globals.save().settings.v0_bgmvol, 37);
            assert_eq!(globals.save().save_menu_position, 91);
        } else {
            let repaired = if profile == "lazy" {
                shin_tasks::block_on(server.effects_atlas()).unwrap()
            } else {
                shin_tasks::block_on(server.initialize_effects_atlas()).unwrap()
            };
            repaired.read().required("breakup");
            if let Some(borrower) = &old_borrow {
                assert_eq!(borrower.reference_identity(), repaired.reference_identity());
                assert_ne!(Some(borrower.reference_identity()), old_identity);
                assert!(old_weak.as_ref().unwrap().upgrade().is_none());
            }
            assert_eq!(globals.save().encode(), before);
        }
        assert_eq!(std::fs::read(&disk).unwrap(), disk_bytes);
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"error":error,
            "same_server_repair_retry":true,"working_progress_written":false,
            "old_atlas_preserved_on_failed_replacement":old_borrow.is_some(),
            "borrower_follows_successful_replacement":old_borrow.is_some(),
            "unused_focusline_accepted":profile=="unused-focusline"}));
    }
    std::fs::write(output.join("common-effects-recovery.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "cases":cases,"cases_passed":cases.len(),"window_created":false,"working_progress_written":false,
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"audio_device":"default output",
        "lookup_scope":"Mandatory App breakup lookup used after real effects publication;no full App instance"
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary assets/save only"]
fn reference_font_metrics_startup_and_notice_error_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_STARTUP_UI_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let font = crate::asset::system::supplied_payload("/system.fnt").2;
    let system = crate::asset::system::supplied_payload("/systex.txa").2;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let make_server = || {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        Arc::new(AssetServer::new(
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
        ))
    };
    let mut cases = Vec::new();
    for (caller, profile) in [
        ("notice", "zero-line-height"),
        ("notice", "short-font"),
        ("notice", "missing-dialog-frame"),
        ("startup", "zero-line-height"),
        ("startup", "short-font"),
    ] {
        let (path, valid) = if profile == "missing-dialog-frame" {
            ("systex.txa", &system)
        } else {
            ("system.fnt", &font)
        };
        let file = directory.join(path);
        let mut bad = valid.clone();
        match profile {
            "zero-line-height" => bad[12..16].fill(0),
            "short-font" => bad.truncate(15),
            "missing-dialog-frame" => {
                let at = bad
                    .windows(b"dialog_frame\0".len())
                    .position(|w| w == b"dialog_frame\0")
                    .unwrap();
                bad[at + 11] = b'x';
            }
            _ => unreachable!(),
        }
        std::fs::write(&file, &bad).unwrap();
        let server = make_server();
        let mut disk = SaveData::default();
        disk.settings.v0_bgmvol = 37;
        disk.save_menu_position = 91;
        let progress = output.join(format!("{caller}-{profile}.dat"));
        let saved_bytes = disk.encode();
        std::fs::write(&progress, &saved_bytes).unwrap();
        let mut globals = GlobalData::default();
        let before_globals = globals.save().encode();
        let mut audio = GlobalAudio::new(manager.clone());
        let error = if caller == "notice" {
            match StorageRetryDialog::load_scene_error(&server, true, (1920, 1080)) {
                Ok(_) => panic!("Malformed {profile} notice unexpectedly loaded"),
                Err(error) => format!("{error:#}"),
            }
        } else {
            let mut boot = FirstBoot::new(server.clone(), Some(progress.clone()));
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                match boot.update(&mut globals, &mut audio, Ticks::ZERO) {
                    Ok(update) => assert!(update.assets.is_none() && !update.complete),
                    Err(error) => break format!("{error:#}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "Malformed startup did not report failure"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        assert!(error.contains(path));
        if profile == "zero-line-height" {
            assert!(error.contains("line height"));
        }
        assert_eq!(globals.save().encode(), before_globals);
        assert_eq!(std::fs::read(&progress).unwrap(), saved_bytes);
        // Only the temporary override changes. Reuse the same AssetServer,
        // including completed earlier startup resources and failed load cache.
        std::fs::write(&file, valid).unwrap();
        let font_owner = if caller == "notice" {
            let _dialog =
                StorageRetryDialog::load_scene_error(&server, true, (1920, 1080)).unwrap();
            let a = server.load_sync::<GpuFontLazy>("/system.fnt").unwrap();
            let b = server.load_sync::<GpuFontLazy>("/system.fnt").unwrap();
            assert!(Arc::ptr_eq(&a, &b));
            a
        } else {
            // Failed mandatory startup cannot publish a partial CommonAssets.
            // A fresh startup owner models reopening after data is repaired.
            let mut boot = FirstBoot::new(server.clone(), Some(progress.clone()));
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut common = None;
            loop {
                let update = boot.update(&mut globals, &mut audio, Ticks::ZERO).unwrap();
                if update.assets.is_some() {
                    common = update.assets;
                }
                if update.complete {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Repaired startup did not complete"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(globals.save().settings.v0_bgmvol, 37);
            assert_eq!(globals.save().save_menu_position, 91);
            let common = common.unwrap();
            assert!(Arc::ptr_eq(
                &common.fonts.system_font,
                &server.load_sync::<GpuFontLazy>("/system.fnt").unwrap()
            ));
            common.fonts.system_font
        };
        assert_eq!(std::fs::read(&progress).unwrap(), saved_bytes);
        assert!(
            shin_core::layout::font::FontMetrics::get_ascent(font_owner.as_ref())
                + shin_core::layout::font::FontMetrics::get_descent(font_owner.as_ref())
                > 0
        );
        // Avoid leaving a repaired override to interfere with the next case.
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"caller":caller,"profile":profile,"error":error,
            "partial_startup_published":false,"working_save_written":false,
            "temporary_progress_preserved":true,"same_server_retry":true,
            "retry_owner":if caller == "startup" {"fresh FirstBoot after data repair"} else {"same notice load callback"}}));
    }
    std::fs::write(
        output.join("startup-ui-recovery.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "cases_passed":cases.len(),"cases":cases,"audio_device":"default output",
            "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
            "window_created":false,"archives_modified":false,"working_progress_written":false
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "{} startup/notice malformed-asset profiles report errors and retry after temporary data repair",
        cases.len()
    );
}
