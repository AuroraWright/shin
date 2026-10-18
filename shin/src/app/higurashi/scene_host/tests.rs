use super::*;
use crate::app::higurashi::load_game::{QuestionKind, Source};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "updated title factories/queue/dispatch/clamped animation and unclamped post-scene clock"]
fn higurashi_scene_host_dispatch_and_clock_match_native() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for seed in [0, 1, 93, u32::MAX - 1, u32::MAX] {
        for elapsed in [0.0_f32, 0.125, 1.0, 3.0, 59.5, 123.5] {
            for restore in [false, true] {
                protocol.push_str(&format!("N {seed}\n"));
                let mut clock = PlayClock::from_saved_seconds(seed);
                let mut seen = clock.ticks();
                let mut delta: f32 = 0.0;
                let mut active = (0u32, 0u32, 0u32);
                let mut queued = None;
                let snapshot = |clock: &PlayClock,
                                seen: f64,
                                delta: f32,
                                active: (u32, u32, u32),
                                pending: bool,
                                events: &[u64]| {
                    let mut words = vec![
                        pending as u64,
                        active.0 as u64,
                        active.1 as u64,
                        active.2 as u64,
                        delta.to_bits() as u64,
                        clock.ticks().to_bits(),
                        seen.to_bits(),
                        clock.saved_seconds() as u64,
                        events.len() as u64,
                    ];
                    words.extend_from_slice(events);
                    words
                };
                // Last writer wins before a frame;both actual factories run.
                for destination in [(1, 0, 0), (2, 17, 1), (1, 0, 1)] {
                    protocol.push_str(&format!(
                        "Q {} {} {}\n",
                        destination.0, destination.1, destination.2
                    ));
                    queued = Some((destination.0, destination.2, destination.1));
                    expected.push(snapshot(&clock, seen, delta, active, true, &[]));
                }
                for (index, (raw, reset, seconds)) in
                    [(elapsed, restore, 7), (0.5, false, 0), (60.0, false, 0)]
                        .into_iter()
                        .enumerate()
                {
                    protocol.push_str(&format!("U {raw} {} {seconds}\n", reset as u32));
                    let mut events = Vec::new();
                    if let Some(destination) = queued.take() {
                        active = destination;
                        events.extend([1, 2]);
                    }
                    events.push(3);
                    seen = clock.ticks();
                    delta = raw.min(3.0);
                    if reset {
                        clock = PlayClock::from_saved_seconds(seconds);
                    }
                    clock.advance(raw);
                    expected.push(snapshot(&clock, seen, delta, active, false, &events));
                    if index == 0 {
                        protocol.push_str("Q 2 -1 0\n");
                        queued = Some((2, 0, u32::MAX));
                        expected.push(snapshot(&clock, seen, delta, active, true, &[]));
                    }
                }
                protocol.push_str("D\n");
                owners += 1;
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SCENE_HOST").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (index, (line, words)) in text.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("H"));
        assert_eq!(
            fields
                .map(|v| v.parse::<u64>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "native host observation{index}"
        );
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Higurashi scene host:{owners} owners/{} observations/{} exact words;both owned title factories,coalescing,old deletion before creation,three-tick scene clamp,full elapsed clock after update/restore,overflow pass",
        expected.len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
}

struct Saved(Vec<u8>);
impl Storage for Saved {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        assert_eq!(source, Source::Current);
        Ok(Some(self.0.clone()))
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
}
fn update(
    host: &mut SceneHost,
    id: &mut FrameId,
    number: &mut u32,
    delta: f32,
    pressed: u32,
    queue: &wgpu::Queue,
) -> Option<TitleTransition> {
    id.advance();
    *number += 1;
    host.update(
        *id,
        Ticks::from_f32(delta),
        Buttons {
            pressed,
            ..Default::default()
        },
        &PointerFrame {
            events: &[],
            number: *number,
            time_seconds: *number as f64 / 60.0,
        },
        queue,
    )
    .unwrap()
}
fn startup(host: &SceneHost) -> &Startup {
    match host.active.as_ref().unwrap() {
        Scene::Startup(s) => s,
        _ => panic!("expected retained startup"),
    }
}
fn demo(host: &SceneHost) -> &TitleDemo {
    match host.active.as_ref().unwrap() {
        Scene::Demo(s) => s,
        _ => panic!("expected retained opening movie"),
    }
}

#[test]
#[ignore = "actual startup/save/mandatory question/deferred opening movie/main-title owner handoff;offscreen Metal and exact Loopback Audio"]
fn higurashi_scene_host_runs_boot_movie_and_owned_title_handoff() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut saved = SaveData::default();
    saved.play_seconds = 93;
    saved.settings.common.v3_sysvol = 37;
    saved.settings.common.v0_bgmvol = 23;
    saved.settings.common.v16_showtoucheffect = false;
    let bytes = saved.encode().unwrap();
    let mut host = SceneHost::new(
        server.clone(),
        manager.clone(),
        sounds.clone(),
        Arc::new(Saved(bytes.clone())),
    );
    let mut id = FrameId::default();
    let mut number = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while host.globals.is_none() {
        assert!(update(&mut host, &mut id, &mut number, 5.0, 0, &queue).is_none());
        assert!(
            Instant::now() < deadline,
            "startup did not publish loaded data"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let globals = host.globals.as_ref().unwrap();
    assert_eq!(
        globals.clock.ticks(),
        5585.0,
        "restore precedes full elapsed clock advance on the publication frame"
    );
    assert_eq!(
        globals.data.encode().unwrap(),
        bytes,
        "sub-second accumulation does not alter saved seconds"
    );
    assert!(!globals.platform_flag);
    assert_eq!(sounds.lock().volume_percent, 37);
    let common = globals.common.clone();
    assert!(Arc::ptr_eq(
        sounds.lock().bank.as_ref().unwrap(),
        &common.sysse
    ));
    let mut expected_ticks = 5585.0;
    while !startup(&host)
        .reference_questions()
        .iter()
        .any(|&(kind, phase, _)| kind == QuestionKind::AutoSleep && phase == 2)
    {
        assert!(update(&mut host, &mut id, &mut number, 1.0, 0, &queue).is_none());
        expected_ticks += 1.0;
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(host.pending.is_none());
    assert!(update(&mut host, &mut id, &mut number, 1.0, 1 << 25, &queue).is_none());
    expected_ticks += 1.0;
    assert!(
        host.pending.is_none(),
        "mandatory notice cannot be cancelled with B"
    );
    assert!(update(&mut host, &mut id, &mut number, 1.0, 1 << 24, &queue).is_none());
    expected_ticks += 1.0;
    assert!(host.pending.is_none(), "stage0 feedback must not dispatch");
    assert!(
        !sounds.lock().sounds.is_empty(),
        "global confirmation sound survives scene ownership"
    );
    while host.pending.is_none() {
        assert!(update(&mut host, &mut id, &mut number, 1.0, 0, &queue).is_none());
        expected_ticks += 1.0;
        assert!(Instant::now() < deadline);
    }
    assert_eq!(
        host.pending,
        Some(Destination::Demo(TitleDemoRequest { mode: 0 }))
    );
    let old_handles = startup(&host).reference_retirements();
    assert!(!old_handles.is_empty());
    assert!(
        host.globals
            .as_ref()
            .unwrap()
            .data
            .save_vectors
            .unlocked_movies
            .is_empty(),
        "queued movie must not initialize on the notice frame"
    );
    assert!(update(&mut host, &mut id, &mut number, 0.0, 0, &queue).is_none());
    assert!(matches!(host.active, Some(Scene::Demo(_))));
    assert!(host.pending.is_none());
    assert!(
        old_handles.iter().all(|h| h.is_finished()),
        "startup scopes retire before opening owner takes over"
    );
    while !demo(&host).reference_frame_ready() {
        assert!(update(&mut host, &mut id, &mut number, 0.0, 0, &queue).is_none());
        assert!(
            Instant::now() < deadline,
            "actual opening MP4 did not publish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let data = &host.globals.as_ref().unwrap().data;
    assert_eq!(data.save_vectors.unlocked_movies.len(), 256);
    assert_eq!(data.save_vectors.unlocked_movies[0], 1);
    assert_eq!(data.save_vectors.unlocked_bgms.len(), 256);
    assert_ne!(
        data.save_vectors.unlocked_bgms[87 / 32] & (1 << (87 % 32)),
        0
    );
    assert!(data.save_vectors.unlocked_cgs.is_empty());
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            host.prepare(context);
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Higurashi retained scene host",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::WHITE),
                Some(0),
                Some(1.0),
            );
            host.render(&mut pass).unwrap();
        });
        assert!(pixels.chunks_exact(4).any(|p| p != [255, 255, 255, 255]));
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        frames += 1;
    }
    assert!(update(&mut host, &mut id, &mut number, 1.0, 1 << 25, &queue).is_none());
    expected_ticks += 1.0;
    assert_eq!(
        demo(&host).reference_motion(),
        (1.0, false),
        "B does not skip opening movie"
    );
    assert!(update(&mut host, &mut id, &mut number, 123.5, 1 << 24, &queue).is_none());
    expected_ticks += 123.5;
    let (opacity, fading) = demo(&host).reference_motion();
    assert!(
        fading && opacity < 1.0 && opacity > 0.95,
        "scene advances only three ticks of its 90-tick fade"
    );
    assert_eq!(
        host.globals.as_ref().unwrap().clock.ticks(),
        expected_ticks,
        "full elapsed time advances after scene update"
    );
    for _ in 0..29 {
        assert!(update(&mut host, &mut id, &mut number, 3.0, 0, &queue).is_none());
        expected_ticks += 3.0;
    }
    assert_eq!(demo(&host).reference_motion(), (0.0, true));
    assert!(
        host.pending.is_none(),
        "fade completion is observed on the following poll"
    );
    assert!(update(&mut host, &mut id, &mut number, 0.125, 0, &queue).is_none());
    expected_ticks += 0.125;
    assert_eq!(
        host.pending,
        Some(Destination::Title(TitleRequest {
            selected: -1,
            mode: 0
        }))
    );
    assert!(
        matches!(host.active, Some(Scene::Demo(_))),
        "opening remains active through the queueing frame"
    );
    let movie_handles = demo(&host).reference_retirements();
    assert!(!movie_handles.is_empty());
    let transition = update(&mut host, &mut id, &mut number, 1.0, 0, &queue)
        .expect("next frame transfers owned title request");
    assert!(
        movie_handles.iter().all(|h| h.is_finished()),
        "decoder/preparation scopes retire before title factory handoff"
    );
    assert!(host.active.is_none() && host.globals.is_none() && host.pending.is_none());
    assert_eq!(
        transition.request,
        TitleRequest {
            selected: -1,
            mode: 0
        }
    );
    let globals = transition.globals;
    assert!(Arc::ptr_eq(&common, &globals.common));
    assert!(Arc::ptr_eq(&sounds, &globals.sounds));
    assert!(Arc::ptr_eq(&manager, &globals.manager));
    assert!(Arc::ptr_eq(&server, &globals.server));
    assert_eq!(
        globals.clock.ticks(),
        expected_ticks,
        "receiving Title must update and finish the handoff frame"
    );
    assert_eq!(globals.data.play_seconds, (expected_ticks / 60.0) as u32);
    saved.play_seconds = globals.data.play_seconds;
    saved.save_vectors.unlocked_movies = vec![0; 256];
    saved.save_vectors.unlocked_movies[0] = 1;
    saved.save_vectors.unlocked_bgms = vec![0; 256];
    saved.save_vectors.unlocked_bgms[87 / 32] = 1 << (87 % 32);
    assert_eq!(
        globals.data.encode().unwrap(),
        saved.encode().unwrap(),
        "all unrelated loaded save fields survive both scene retirements"
    );
    assert!(
        host.update(
            id,
            Ticks::ZERO,
            Buttons::default(),
            &PointerFrame {
                events: &[],
                number,
                time_seconds: 0.0
            },
            &queue
        )
        .is_err(),
        "transferred owner cannot advance twice"
    );
    eprintln!(
        "Higurashi scene host integration:1 loaded boot/mandatory notice/real opening/main-title handoff;{frames}720p/1080p frames;A/B,owned globals/audio bank/clock,post-load full elapsed time/three-tick scene clamp,deferred dispatch and startup/movie retirement pass"
    );
}
