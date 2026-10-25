//! Menu cue recovery using the real bank loader and the default audio output.
use super::*;
use crate::audio::SystemSoundPlayer;
use shin_core::{format::sysse::SysSe, vm::command::types::Volume};

fn word(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}
fn set_word(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 4].copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary bank only"]
fn reference_system_sound_bank_error_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_SYSTEM_SOUND_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("sysse.bin");
    let bytes = super::accessor_io_reference_tests::supplied_payload("/sysse.bin").2;
    let bank = shin_core::format::sysse::read_sys_se(&bytes).unwrap();
    let (cue, sound) = bank
        .sounds
        .iter()
        .find(|(_, s)| s.sample_count() > 0)
        .unwrap();
    let expected_samples = sound.sample_count();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut cases = Vec::new();
    for corruption in ["declared-size", "entry-span", "sound-size", "missing-file"] {
        let mut bad = bytes.clone();
        match corruption {
            "declared-size" => set_word(&mut bad, 4, bytes.len() + 1),
            "entry-span" => set_word(&mut bad, 28, bytes.len() + 1),
            "sound-size" => {
                let offset = word(&bad, 28);
                let size = word(&bad, 32);
                assert!(offset + size <= bad.len());
                assert_eq!(&bad[offset..offset + 4], b"ADP1");
                set_word(&mut bad, offset + 4, size + 1);
            }
            "missing-file" => {}
            _ => unreachable!(),
        }
        std::fs::write(&file, &bad).unwrap();
        let server = AssetServer::new(
            AssetIo::new_dir(&directory).unwrap(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        if corruption == "missing-file" {
            std::fs::remove_file(&file).unwrap();
        }
        let mut player = SystemSoundPlayer::new(audio.clone());
        player.set_settings_volume(73);
        let track = player.track.id();
        // This is the actual side effect called by title/ADV menu input. The
        // baseline parser/expect panic fails here before any assertions run.
        player.play(&server, cue, Volume(0.6));
        assert!(player.bank.is_none());
        assert!(player.sounds.is_empty() && player.retired.is_empty());
        assert_eq!(player.track.id(), track);
        assert_eq!(player.volume_percent, 73);
        let error = match server.load_sync::<SysSe>("/sysse.bin") {
            Ok(_) => panic!("Malformed bank unexpectedly loaded"),
            Err(error) => error,
        };
        let message = format!("{error:#}");
        assert!(message.contains("/sysse.bin"));
        if corruption != "missing-file" {
            assert!(message.contains("SysSE worker failed:"));
        }
        assert!(
            server
                .loaded_assets
                .read()
                .unwrap()
                .get::<AssetMap<SysSe>>()
                .is_none()
        );

        // Restore only the temporary asset. The same player/server must retry
        // the failed bank and retain its original settings and sub-track.
        std::fs::write(&file, &bytes).unwrap();
        player.play(&server, cue, Volume(0.6));
        assert_eq!(player.sounds.len(), 1);
        assert_eq!(player.track.id(), track);
        assert_eq!(player.volume_percent, 73);
        let retained = player.bank.as_ref().unwrap().clone();
        assert!(Arc::ptr_eq(
            &retained,
            &server.load_sync::<SysSe>("/sysse.bin").unwrap()
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while player.sounds[0].position_samples() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "Restored cue did not advance on the default audio output"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let played_samples = player.sounds[0].position_samples();
        assert!(played_samples <= expected_samples);
        // A missing name must not interrupt an existing cue or discard the
        // loaded bank; valid cues in the same bank still work afterward.
        player.play(&server, "__missing_reference_cue__", Volume(1.0));
        assert_eq!(player.sounds.len(), 1);
        assert!(Arc::ptr_eq(&retained, player.bank.as_ref().unwrap()));
        player.play(&server, cue, Volume(0.6));
        assert_eq!(player.sounds.len(), 2);
        assert_eq!(player.track.id(), track);
        for handle in &player.sounds {
            handle.stop_immediate();
        }
        cases.push(serde_json::json!({"profile":corruption,"error":message,"played_samples":played_samples,"callback_returned_without_panic":true,"same_player_server_retry":true,"missing_name_preserves_existing_cue_and_bank":true}));
    }
    let report = serde_json::json!({
        "cases_passed":cases.len(),"cases":cases,"cue":cue,"cue_samples":expected_samples,
        "bank_bytes":bytes.len(),"bank_sound_count":bank.sounds.len(),
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "audio_device":"default output","window_created":false,
        "archives_modified":false,"working_progress_written":false
    });
    std::fs::write(
        output.join("system-sound-recovery.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{} system-bank failures return from menu cue callbacks; the default audio output retries and missing-name recovery pass",
        cases.len()
    );
}
