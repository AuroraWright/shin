//! Real-time supplied-media completion, separate from cancellation and short fixtures.
use super::*;

fn emit(value: serde_json::Value) {
    eprintln!("MOVIE_ROSTER {value}");
}

fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    }))
}

#[test]
#[ignore = "all12 supplied MP4s at natural wall-clock speed through actual Movie Owner/Player/video EOF+PCM drain/private retirement; offscreen Metal/the default audio output; no user saves, no controller testing; about25minutes"]
fn higurashi_movie_roster_natural_eof_drains_and_restores_gallery() {
    let metadata: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/higurashi/movie-roster-eof-20261014/metadata.json"
    ))).unwrap();
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let common = common(&server);
    let audio = Arc::new(AudioManager::new_for_tests());
    let data = settings();
    let baseline = serde_json::to_vec(&data).unwrap();
    let mut events = Events::default();
    let mut capture = snapshot(&device, &queue);
    let mut owner = Owner::new_retained(
        environment(&server, &common, &audio), audio, common.scenario.clone(),
        &data, &mut capture, (1280, 720), &mut events,
    ).unwrap();
    assert!(owner.env.storage.is_none());
    let (mut frame, mut number) = (FrameId::default(), 0);
    ready_grid(&mut owner, &mut events, &mut frame, &mut number);
    let atlas = Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize;
    let resources = owner.reference_resources();
    assert_eq!(owner.model.rows.len(), 12);
    let count_before = events.accepted.len();
    for (row, info) in metadata["movies"].as_array().unwrap().iter().enumerate() {
        let movie_id = info["id"].as_u64().unwrap() as u16;
        let duration = info["seconds"].as_f64().unwrap();
        assert_eq!(owner.model.rows[row].movie_id, movie_id);
        assert_eq!(common.scenario.movie_info[usize::from(movie_id)].name.to_string(), info["name"].as_str().unwrap());
        // Ordinary arrows, one row at a time, including the four-column wrap.
        // No direct mutation of selection, model, task, transport or clock.
        while owner.model.selected() != row {
            let selected = owner.model.selected();
            let direction = if selected / 4 < row / 4 { 1 << 6 }
                else if selected % 4 > row % 4 { 1 << 7 } else { 1 << 5 };
            frame.advance(); number += 1;
            owner.poll(Ticks::ZERO, true, Buttons { repeat: direction, ..Buttons::default() },
                &pointer(number), frame, &mut events).unwrap();
        }
        let event_start = events.accepted.len();
        tick(&mut owner, &mut events, &mut frame, &mut number, 0., 1 << 24).unwrap();
        assert_eq!(owner.media.len(), 1);
        let id = viewer(&owner).id;
        assert_eq!(viewer(&owner).movie_id, movie_id);
        let retirement = owner.media[&id].player.as_ref().unwrap().reference_retirement();
        let start = Instant::now();
        let mut previous = start;
        let mut next = start;
        let mut first_playing = None;
        let mut first_eof = None;
        let mut last_time = 0;
        let mut samples = Vec::new();
        let mut next_progress = 30.;
        emit(serde_json::json!({"event":"start","row":row,"movie_id":movie_id,"name":info["name"],"duration_seconds":duration,"initial_status":owner.media[&id].player.as_ref().unwrap().status()}));
        loop {
            let now = Instant::now();
            if now < next { std::thread::sleep(next - now); }
            let now = Instant::now();
            // Fresh real-time intervals; never catch up with accelerated bursts.
            let delta = now.duration_since(previous).as_secs_f32() * 60.;
            previous = now;
            next = now + Duration::from_secs_f64(1. / 60.);
            tick(&mut owner, &mut events, &mut frame, &mut number, delta, 0).unwrap();
            assert!(owner.fault.is_none());
            let elapsed = start.elapsed().as_secs_f64();
            assert!(elapsed < duration + 60., "Movie{movie_id} failed natural EOF/retirement, elapsed={elapsed}, status={:?}", owner.reference_viewer_state());
            if let Some(player) = owner.media.get(&id).and_then(|m| m.player.as_ref()) {
                let playing = player.is_playing();
                let status = player.status();
                let time = player.reference_time_micros().unwrap_or(0);
                assert!(time >= last_time, "Movie{movie_id} clock rewound");
                last_time = time;
                if playing && first_playing.is_none() {
                    assert_eq!(status, 1, "Player must enter actual playback before EOF");
                    first_playing = Some(elapsed);
                }
                if status == 2 && first_eof.is_none() {
                    assert!(playing, "EOF must precede stop/release");
                    let started = first_playing.expect("Observed live playing");
                    let wall = elapsed - started;
                    let media = time as f64 / 1_000_000.;
                    assert!(wall >= duration - 2. && wall <= duration + 20., "Movie{movie_id} early/late wall EOF {wall}/{duration}");
                    assert!((media - duration).abs() < 2., "Movie{movie_id} early/late media EOF {media}/{duration}");
                    assert!(player.reference_frame_extent().is_some());
                    first_eof = Some(elapsed);
                    emit(serde_json::json!({"event":"natural_eof","row":row,"movie_id":movie_id,"elapsed_seconds":elapsed,"playing_seconds":wall,"media_micros":time,"playing":playing,"extent":player.reference_frame_extent()}));
                }
                // Actual complete Owner draw at first, middle and terminal frame.
                let should_sample = playing && player.reference_frame_extent().is_some()
                    && (samples.is_empty() || (samples.len() == 1 && elapsed > duration / 2.)
                        || (samples.len() == 2 && first_eof.is_some()));
                if should_sample {
                    let extent = if row % 2 == 0 { (1280,720) } else { (1920,1080) };
                    let rgba = image(&device, &queue, extent, &owner, &data);
                    samples.push(serde_json::json!({"elapsed_seconds":elapsed,"media_micros":time,"extent":extent,"rgba_fnv1a64":hash(&rgba),"nonblack_pixels":rgba.chunks_exact(4).filter(|p|p[..3]!=[0,0,0]).count()}));
                }
            }
            if elapsed >= next_progress {
                emit(serde_json::json!({"event":"progress","row":row,"movie_id":movie_id,"elapsed_seconds":elapsed,"media_micros":last_time,"eof_seen":first_eof.is_some()}));
                next_progress += 30.;
            }
            if owner.media.is_empty() {
                assert!(first_eof.is_some(), "Media retired without successful natural EOF");
                assert!(owner.reference_state().3 && !owner.model.closing);
                assert_eq!(owner.reference_scope_state().viewers, 0);
                assert!(retirement.is_finished(), "Actual media loader/codec descendants retired");
                assert_eq!(owner.model.selected(), row);
                assert_eq!(Arc::as_ptr(&owner.loader.atlas().unwrap()) as usize, atlas);
                let policy_events: Vec<_> = events.accepted[event_start..].iter()
                    .filter(|a| !matches!(a, Action::PlatformByte)).cloned().collect();
                assert_eq!(policy_events, [Action::Cue("decide")], "EOF must not invoke cancel or ReturnTitle");
                assert_eq!(serde_json::to_vec(&data).unwrap(), baseline);
                assert_eq!(samples.len(), 3, "First/middle/terminal retained conversion/draw sampled");
                emit(serde_json::json!({"event":"retired","row":row,"movie_id":movie_id,"elapsed_seconds":elapsed,"media_micros":last_time,"media_retirement_finished":retirement.is_finished(),"retained_grid_row":owner.model.selected(),"samples":samples}));
                break;
            }
        }
    }
    assert!(events.accepted[count_before..].iter().all(|a|matches!(a,Action::Cue("move"|"decide")|Action::PlatformByte)));
    drop(owner);
    assert!(resources.atlas.upgrade().is_none());
    assert!(resources.mask.is_none_or(|m|m.upgrade().is_none()));
    emit(serde_json::json!({"event":"pass","movies":12,"settings_unchanged":true,"storage":null,"user_saves_written":false,"scope":"desktop actual Owner/FFmpeg/Metal/audio natural EOF and retirement; no Nintendo decoder/framebuffer equivalence"}));
}
