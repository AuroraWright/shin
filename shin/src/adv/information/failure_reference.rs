//! Real ADV notification side-effect recovery with temporary typed artwork.
use super::*;
use crate::{
    adv::{Adv, assets::AdvAssets},
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
};
use shin_core::vm::Scripter;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal and exact Loopback Audio; temporary atlas only"]
fn reference_notification_artwork_failure_preserves_state_and_retries() {
    std::thread::Builder::new()
        .name("notification-recovery".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_NOTIFICATION_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("adv.txa");
    let bytes = supplied_payload("/adv.txa").2;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for profile in ["declared-size", "short-header", "unknown-notify"] {
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
        let mut adv = Adv::new(
            manager.clone(),
            server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, 0, 42),
        );
        let before_globals = adv.reference_global_save().encode();
        let before_entry = serde_json::to_vec(&adv.reference_entry()).unwrap();
        let before_info = adv.vm_state.information.clone();
        let track = adv.adv_state.audio.system_player.track.id();
        let settings = adv.adv_state.audio.system_player.volume_percent;
        adv.adv_state
            .notifications
            .enqueue(NotificationKind::Characters, 1);
        adv.adv_state
            .notifications
            .enqueue(NotificationKind::Tips, 2);
        let mut bad = bytes.clone();
        match profile {
            "declared-size" => bad[8..12].copy_from_slice(&(bytes.len() as u32 + 1).to_le_bytes()),
            "short-header" => bad.truncate(15),
            "unknown-notify" => {
                let at = bad.windows(7).position(|w| w == b"notify\0").unwrap();
                bad[at + 5] = b'x';
            }
            _ => unreachable!(),
        }
        std::fs::write(&file, &bad).unwrap();
        // Actual production notification callback: baseline's expect panics
        // here after the producer correctly returns a typed asset error.
        adv.adv_state
            .update_notifications(&server, Ticks::from_f32(15.0));
        assert!(adv.adv_state.notifications.textures.is_none());
        let motion = adv.adv_state.notifications.reference_motion();
        assert_eq!(motion.0, Some(0));
        assert_eq!(motion.1, vec![17]);
        assert!((motion.2 - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001);
        assert_eq!(motion.3, 0.5);
        assert!(!motion.4);
        assert_eq!(adv.adv_state.audio.system_player.sounds.len(), 1);
        assert_eq!(adv.adv_state.audio.system_player.track.id(), track);
        assert_eq!(adv.adv_state.audio.system_player.volume_percent, settings);
        assert_eq!(adv.reference_global_save().encode(), before_globals);
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(adv.vm_state.information, before_info);
        let error = match server.load_sync::<NotificationTextures>("/adv.txa") {
            Ok(_) => panic!("Malformed typed notification atlas loaded"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("/adv.txa"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while adv.adv_state.audio.system_player.sounds[0].position_samples() == 0 {
            assert!(
                Instant::now() < deadline,
                "Notification cue did not advance"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let first_cue_samples = adv.adv_state.audio.system_player.sounds[0].position_samples();
        std::fs::write(&file, &bytes).unwrap();
        adv.adv_state
            .update_notifications(&server, Ticks::from_f32(15.0));
        assert!(adv.adv_state.notifications.textures.is_none());
        assert_eq!(
            adv.adv_state.audio.system_player.sounds.len(),
            1,
            "Active toast must not repeat its cue or load each frame"
        );
        adv.adv_state
            .update_notifications(&server, Ticks::from_f32(210.0));
        assert!(adv.adv_state.notifications.opacity.is_idle());
        adv.adv_state.update_notifications(&server, Ticks::ZERO);
        let retained = adv
            .adv_state
            .notifications
            .textures
            .as_ref()
            .unwrap()
            .clone();
        assert!(Arc::ptr_eq(
            &retained,
            &server
                .load_sync::<NotificationTextures>("/adv.txa")
                .unwrap()
        ));
        assert_eq!(adv.adv_state.notifications.active, Some(17));
        assert_eq!(adv.adv_state.audio.system_player.sounds.len(), 2);
        let deadline = Instant::now() + Duration::from_secs(2);
        while adv.adv_state.audio.system_player.sounds[1].position_samples() == 0 {
            assert!(
                Instant::now() < deadline,
                "Retried notification cue did not advance"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let retry_cue_samples = adv.adv_state.audio.system_player.sounds[1].position_samples();
        // Valid cached artwork survives a later bad file replacement and is
        // reused for a subsequent notice without re-reading the broken file.
        std::fs::write(&file, &bad).unwrap();
        adv.adv_state
            .update_notifications(&server, Ticks::from_f32(240.0));
        adv.adv_state
            .notifications
            .enqueue(NotificationKind::Characters, 2);
        adv.adv_state.update_notifications(&server, Ticks::ZERO);
        assert!(Arc::ptr_eq(
            &retained,
            adv.adv_state.notifications.textures.as_ref().unwrap()
        ));
        assert_eq!(adv.adv_state.audio.system_player.sounds.len(), 3);
        assert_eq!(adv.reference_global_save().encode(), before_globals);
        assert_eq!(
            serde_json::to_vec(&adv.reference_entry()).unwrap(),
            before_entry
        );
        assert_eq!(adv.vm_state.information, before_info);
        for handle in &adv.adv_state.audio.system_player.sounds {
            handle.stop_immediate();
        }
        std::fs::remove_file(&file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"error":error,
            "story_entry_preserved":true,"global_progress_preserved":true,
            "information_state_preserved":true,"queue_timing_preserved":true,
            "same_owner_next_notification_retry":true,"first_cue_samples":first_cue_samples,
            "retry_cue_samples":retry_cue_samples,"cue_not_repeated_for_active_toast":true,
            "healthy_warm_atlas_retained":true}));
    }
    std::fs::write(
        output.join("notification-recovery.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "cases_passed":cases.len(),"cases":cases,"audio_device":"exact Loopback Audio",
            "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
            "window_created":false,"archives_modified":false,"working_progress_written":false
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "{} actual ADV notification artwork failures preserve story/progress/toast timing and retry with exact Loopback Audio cues",
        cases.len()
    );
}
