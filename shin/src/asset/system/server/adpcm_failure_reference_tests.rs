//! Malformed ADPCM cue recovery before the real audio callback consumes it.
use super::*;
use crate::audio::SystemSoundPlayer;
use shin_core::{format::sysse::SysSe, vm::command::types::Volume};

fn word(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

#[test]
#[ignore = "requires supplied ROM, Metal and the default audio output; temporary bank only"]
fn reference_system_sound_adpcm_error_recovery() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ADPCM_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("sysse.bin");
    let bytes = super::accessor_io_reference_tests::supplied_payload("/sysse.bin").2;
    let bank = shin_core::format::sysse::read_sys_se(&bytes).unwrap();
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
    for (cue, block, channel, filter) in [
        ("ahaha", 0, 0, Some(5u8)),
        ("ahaha", 80, 0, Some(15)),
        ("decide", 0, 0, Some(8)),
        ("decide", 80, 1, Some(5)),
        ("decide", 0, 0, None),
    ] {
        let entry = (0..word(&bytes, 8))
            .map(|i| 12 + i * 24)
            .find(|offset| {
                let name = &bytes[*offset..*offset + 16];
                name.starts_with(cue.as_bytes()) && name[cue.len()] == 0
            })
            .unwrap();
        let offset = word(&bytes, entry + 16);
        let channels = usize::from(u16::from_le_bytes(
            bytes[offset + 8..offset + 10].try_into().unwrap(),
        ));
        let mut bad = bytes.clone();
        let profile = if let Some(filter) = filter {
            let at = offset + 16 + block * channels * 16 + channel * 16;
            assert!(at < offset + word(&bytes, entry + 20));
            bad[at] = (bad[at] & 15) | filter << 4;
            format!("{channels}ch/block{block}/channel{channel}/filter{filter}")
        } else {
            bad[offset + 10..offset + 12].copy_from_slice(&0u16.to_le_bytes());
            "zero-sample-rate".to_string()
        };
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
        let mut player = SystemSoundPlayer::new(audio.clone());
        player.set_settings_volume(73);
        let track = player.track.id();
        player.play(&server, cue, Volume(0.6));
        // Only the baseline queues malformed data. Give its real callback a
        // bounded opportunity to run in this isolated test process.
        if player.bank.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        assert!(player.bank.is_none(), "Malformed {profile} bank was queued");
        assert!(player.sounds.is_empty() && player.retired.is_empty());
        assert_eq!(player.track.id(), track);
        assert_eq!(player.volume_percent, 73);
        let error = match server.load_sync::<SysSe>("/sysse.bin") {
            Ok(_) => panic!("Malformed bank unexpectedly loaded"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("/sysse.bin"));
        assert!(error.contains(if filter.is_some() {
            "ADPCM filter"
        } else {
            "sample rate"
        }));
        assert!(
            server
                .loaded_assets
                .read()
                .unwrap()
                .get::<AssetMap<SysSe>>()
                .is_none()
        );
        std::fs::write(&file, &bytes).unwrap();
        player.play(&server, cue, Volume(0.6));
        assert_eq!(player.sounds.len(), 1);
        assert_eq!(player.track.id(), track);
        assert_eq!(player.volume_percent, 73);
        let retained = player.bank.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            retained,
            &server.load_sync::<SysSe>("/sysse.bin").unwrap()
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while player.sounds[0].position_samples() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "Valid retry did not advance"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let played = player.sounds[0].position_samples();
        assert!(played <= bank.sounds[cue].sample_count());
        player.sounds[0].stop_immediate();
        cases.push(serde_json::json!({"profile":profile,"error":error,
            "malformed_voice_queued":false,"callback_returned":true,
            "same_player_server_retry":true,"played_samples":played}));
    }
    std::fs::write(
        output.join("adpcm-recovery.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "cases_passed":cases.len(),"cases":cases,"audio_device":"default output",
            "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
            "window_created":false,"archives_modified":false,"working_progress_written":false
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "{} malformed ADPCM profiles return before queuing and retry on the default audio output",
        cases.len()
    );
}
