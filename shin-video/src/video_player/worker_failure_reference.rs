//! Controlled faults in actual decoded/player/loop owners, not authored routes.
use super::*;
use std::{
    io::{Cursor, Read},
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM, Metal, ffmpeg and exact Loopback Audio"]
fn reference_movie_player_worker_late_frame_and_loop_failures_retire() {
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let bytes = opening_bytes();
    let mut count = 0;
    for (fault, repeat) in [
        ("frame-size", false),
        ("plane-short", false),
        ("frame-size", true),
        ("plane-short", true),
        ("loop-panic", true),
        ("loop-size", true),
    ] {
        println!("Movie late worker: {fault}/repeat={repeat}");
        assert_eq!(audio.kira_manager().lock().main_track().num_sounds(), 0);
        let mp4 = Mp4::new(Cursor::new(bytes.clone())).unwrap();
        let worker_device = device.clone();
        let worker_audio = audio.clone();
        let player = shin_tasks::block_on(shin_tasks::compute::spawn(move || {
            VideoPlayerHandle::new_with_playback_state(
                &worker_device,
                &worker_audio,
                mp4,
                Volume(0.0),
                true,
                repeat,
            )
        }))
        .unwrap();
        player.start();
        let mut frame = FrameId::default();
        frame.advance();
        player.update(frame, Ticks::ZERO, &queue);
        let image = player.current_frame_timing();
        assert!(image.is_some());
        assert!(player.get_retained_frame().is_some());
        let retirement = {
            let mut inner = player.inner.write();
            if fault.starts_with("loop") {
                // A real loop restart begins after the old stream reaches EOF.
                // Retain its uploaded frame, but retire pending decode work.
                inner.pending_frame = None;
                inner.retire_video_decoder();
                let looping = inner.movie_loop.as_mut().unwrap();
                let original = looping.prepare.clone();
                looping.prepare = if fault == "loop-panic" {
                    Arc::new(|| panic!("controlled decoder restart panic"))
                } else {
                    Arc::new(move || {
                        let mut prepared = original()?;
                        prepared.first.1.size.width += 2;
                        Ok(prepared)
                    })
                };
                looping.rewind();
                Some(looping.task.as_mut().unwrap().get_mut().retirement_handle())
            } else {
                let (timing, pending) = inner
                    .pending_frame
                    .as_mut()
                    .expect("Actual movie has another frame");
                timing.start_time = 0;
                if fault == "frame-size" {
                    pending.size.width += 2;
                } else {
                    pending.y_plane.pop().unwrap();
                }
                None
            }
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !player.is_finished() {
            frame.advance();
            player.update(frame, Ticks::ZERO, &queue);
            assert!(
                Instant::now() < deadline,
                "Fault did not reach terminal player state"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(player.inner.read().stopped);
        assert!(player.get_frame().is_none());
        assert!(player.get_retained_frame().is_some());
        assert_eq!(
            player.current_frame_timing(),
            image,
            "Failure replaced last uploaded frame"
        );
        if let Some(retirement) = retirement {
            assert!(
                retirement.is_finished(),
                "Failed prepared decoder children did not retire"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
        let frozen = player.current_time();
        assert_eq!(
            audio.kira_manager().lock().main_track().num_sounds(),
            1,
            "Terminal failure retains paused voice until owner drop"
        );
        for _ in 0..4 {
            player.start();
            player.set_volume(Volume(0.5));
            frame.advance();
            player.update(frame, Ticks::from_seconds(60.0), &queue);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(player.current_time(), frozen);
        assert_eq!(player.current_frame_timing(), image);
        drop(player);
        let deadline = Instant::now() + Duration::from_secs(5);
        while audio.kira_manager().lock().main_track().num_sounds() != 0 {
            assert!(Instant::now() < deadline, "Failed player leaked audio");
            std::thread::sleep(Duration::from_millis(5));
        }
        count += 1;
    }
    assert_eq!(count, 6);
    println!(
        "Movie late-worker recovery:6 controlled actual-player faults; terminal state,last frame,paused voice and final audio/decoder retirement pass"
    );
}
fn opening_bytes() -> Vec<u8> {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        paths.push(p);
    }
    paths.push(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    for path in paths {
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        if let Ok(file) = rom.find_file("/movie/op1.mp4") {
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            return bytes;
        }
    }
    panic!("Supplied archives lack opening movie");
}
