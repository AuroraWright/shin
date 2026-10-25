//! Actual supplied Movie preparation across all public player factories.
use super::*;
use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache};
use shin_core::{primitives::update::FrameId, time::Ticks};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal, ffmpeg and the default audio output; no working progress writes"]
fn reference_movie_player_worker_limits_and_device_errors_recover() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_MOVIE_WORKER_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let (limited, _) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_texture_dimension_2d: 512,
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let mut io = LayeredAssetIo::new();
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
    let path = "/movie/op1.mp4";
    let asset: Arc<Movie> = shin_tasks::block_on(server.load(path)).unwrap();
    let metadata = asset
        .mp4
        .video_track
        .get_mp4_track_info(|t| (t.width(), t.height()));
    assert!(metadata.0 > 512 || metadata.1 > 512);
    let mut cases = Vec::new();
    for api in ["paused", "paused-loop", "play", "play-volume", "play-loop"] {
        for fault in ["limit", "destroyed-device"] {
            println!("Movie worker: {api}/{fault}");
            assert_eq!(audio.kira_manager().lock().main_track().num_sounds(), 0);
            let failed_device = if fault == "limit" {
                limited.clone()
            } else {
                let (dead, _) =
                    shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
                dead.destroy();
                dead
            };
            let producer_asset = asset.clone();
            let producer_audio = audio.clone();
            let prepare = move || create(api, &producer_asset, &failed_device, &producer_audio);
            // The baseline uses a joined Rust thread so the old escaped panic
            // is observable without aborting Rayon's whole test process.
            let result = if std::env::var_os("SHIN_REFERENCE_MOVIE_WORKER_BASELINE").is_some() {
                std::thread::spawn(prepare)
                    .join()
                    .expect("Movie preparation escaped the worker as a panic")
            } else {
                shin_tasks::block_on(shin_tasks::compute::spawn(prepare))
            };
            let error = result
                .err()
                .expect("Bad device must reject player preparation");
            let error = format!("{error:#}");
            if fault == "limit" {
                assert!(error.contains("limit") && error.contains("512"), "{error}");
            } else {
                assert!(
                    error.contains("preparation") && error.contains("panic"),
                    "{error}"
                );
            }
            assert_eq!(
                audio.kira_manager().lock().main_track().num_sounds(),
                0,
                "Failed texture preparation started audio"
            );
            assert!(Arc::ptr_eq(
                &asset,
                &shin_tasks::block_on(server.load::<Movie, _>(path)).unwrap()
            ));
            // Reuse the parsed Movie and audio manager with a valid device.
            let producer_asset = asset.clone();
            let producer_audio = audio.clone();
            let good_device = device.clone();
            let player = shin_tasks::block_on(shin_tasks::compute::spawn(move || {
                create(api, &producer_asset, &good_device, &producer_audio)
            }))
            .unwrap();
            player.start();
            let mut frame = FrameId::default();
            frame.advance();
            player.update(frame, Ticks::ZERO, &queue);
            assert!(
                player.get_retained_frame().is_some(),
                "Repaired player did not upload real ROM frame"
            );
            player.terminate();
            drop(player);
            let deadline = Instant::now() + Duration::from_secs(5);
            while audio.kira_manager().lock().main_track().num_sounds() != 0 {
                assert!(Instant::now() < deadline, "Movie voice leaked");
                std::thread::sleep(Duration::from_millis(5));
            }
            cases.push(serde_json::json!({"api":api,"fault":fault,"error":error,"same_parsed_movie_retry":true,"frame_uploaded":true,"audio_retired":true}));
        }
    }
    std::fs::write(output.join("movie-worker-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"cases_passed":cases.len(),"path":path,"metadata_size":metadata,"device_limit":limited.limits().max_texture_dimension_2d,"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"audio_device":"default output","window_created":false,"working_progress_written":false,"scope":"Actual public Movie factories on compute workers; not full scene/ADV dispatch"})).unwrap()).unwrap();
}
fn create(
    api: &str,
    movie: &Movie,
    device: &wgpu::Device,
    audio: &AudioManager,
) -> Result<VideoPlayerHandle> {
    match api {
        "paused" => movie.prepare_with_volume(device, audio, Volume(0.0)),
        "paused-loop" => movie.prepare_looping_with_volume(device, audio, Volume(0.0)),
        "play" => movie.play(device, audio),
        "play-volume" => movie.play_with_volume(device, audio, Volume(0.0)),
        "play-loop" => movie.play_looping_with_volume(device, audio, Volume(0.0)),
        _ => unreachable!(),
    }
}
