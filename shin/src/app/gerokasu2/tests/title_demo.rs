use super::{system_menu::driver, *};

fn step(d: &mut system_menu::Driver<'_>, input: EnumMap<AppAction, ActionState>, ticks: f64) {
    let gpu = d.gpu;
    d.app.frame.advance();
    d.app.poll_title_destination();
    if d.app.title_demo.is_some() {
        d.app.update_title_demo(
            &gpu.queue,
            &input,
            viewport(),
            Duration::from_secs_f64(ticks / 60.),
        );
    }
    // The window loop submits every frame, flushing the movie's uploads.
    gpu.queue.submit([]);
    d.app.advance_global_audio(Ticks::from_f32(ticks as f32));
    assert!(d.app.error.is_none(), "{:?}", d.app.error);
}

#[test]
#[ignore = "actual boot TitleDemo movie, Enter exit fade and Title publication on Metal/the default audio output"]
fn ger_app_boot_title_demo_plays_opening_and_enter_fades_to_title() {
    let gpu = Offscreen::new();
    let (_temporary, _path, mut d) = driver(&gpu, false);
    d.app.host = None;
    let store = d.app.save_store.as_mut().unwrap();
    store.data.save_vectors.unlocked_movies = vec![2];
    assert!(crate::app::gerokasu2::title_demo::enabled(&store.data));
    let (demo, changed) = crate::app::gerokasu2::title_demo::TitleDemo::new(
        d.app.assets.clone(),
        d.app.audio.clone(),
        &d.app.scenario,
        &mut store.data,
    );
    // Movie0 is op2 with linked BGM 12 and no picture.
    assert!(changed, "First records movie0 and its links");
    let vectors = &store.data.save_vectors;
    assert_eq!(vectors.unlocked_movies[0] & 3, 3);
    assert_eq!(vectors.unlocked_bgms[0] & (1 << 12), 1 << 12);
    d.app.title_demo = Some(demo);

    let started = Instant::now();
    while !d.app.title_demo.as_ref().unwrap().reference_state().2 {
        step(&mut d, EnumMap::default(), 1.);
        std::thread::sleep(Duration::from_millis(2));
        assert!(started.elapsed() < Duration::from_secs(30), "movie frame");
    }
    let (playing, fade, _) = d.app.title_demo.as_ref().unwrap().reference_state();
    assert!(playing && fade == 1.0);
    d.capture();
    // Optional real-time check for optimized builds: the audio-tied decoder
    // must keep pace with presentation at 60 fps.
    if let Some(seconds) = std::env::var("SHIN_GEROKASU2_TITLE_DEMO_PLAY_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        let mut slowest = Duration::ZERO;
        for _ in 0..seconds * 60 {
            let started = Instant::now();
            step(&mut d, EnumMap::default(), 1.);
            let spent = started.elapsed();
            slowest = slowest.max(spent);
            std::thread::sleep(Duration::from_micros(16_667).saturating_sub(spent));
        }
        println!("GEROKASU2_TITLE_DEMO_REALTIME seconds={seconds} slowest_step={slowest:?}");
        assert!(slowest < Duration::from_millis(50), "decoder fell behind");
        assert!(d.app.title_demo.as_ref().unwrap().reference_state().0);
        d.capture();
    }

    step(&mut d, held(AppAction::Enter), 1.);
    let (playing, fade, _) = d.app.title_demo.as_ref().unwrap().reference_state();
    assert!(!playing && (fade - 89. / 90.).abs() < 1e-4, "{fade}");
    step(&mut d, EnumMap::default(), 88.);
    let (_, fade, _) = d.app.title_demo.as_ref().unwrap().reference_state();
    assert!((fade - 1. / 90.).abs() < 1e-4, "{fade}");
    assert!(d.app.loading_title.is_none());
    d.capture();
    step(&mut d, EnumMap::default(), 1.);
    step(&mut d, EnumMap::default(), 1.);
    assert!(d.app.loading_title.is_some(), "idle Exit fade publishes Title");
    let started = Instant::now();
    while d.app.title.is_none() {
        step(&mut d, EnumMap::default(), 1.);
        assert!(started.elapsed() < Duration::from_secs(30), "Title publication");
    }
    assert!(d.app.title_demo.is_none());
    println!("GEROKASU2_TITLE_DEMO title_publication={:?}", started.elapsed());
}

/// Count changed pixels in a 320x180 capture rectangle and their most
/// frequent color (glyph interiors; edges blend with the backdrop).
fn changed(
    before: &[u8],
    after: &[u8],
    x: std::ops::Range<usize>,
    y: std::ops::Range<usize>,
) -> (usize, [u8; 4]) {
    let mut colors = std::collections::BTreeMap::new();
    for row in y {
        for column in x.clone() {
            let at = (row * 320 + column) * 4;
            let pixel: [u8; 4] = after[at..at + 4].try_into().unwrap();
            if before[at..at + 4] != pixel {
                *colors.entry(pixel).or_insert(0usize) += 1;
            }
        }
    }
    let count = colors.values().sum();
    let dominant = colors
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or([0; 4], |(color, _)| color);
    (count, dominant)
}

#[test]
#[ignore = "actual scene-title and BGM-title notifications composed over ADV on Metal/the default audio output"]
fn ger_app_scene_and_bgm_title_notifications_compose_over_adv() {
    let gpu = Offscreen::new();
    let (_temporary, _path, mut d) = driver(&gpu, false);
    d.settle(20);
    let before = d.capture();
    let host = d.app.host.as_mut().unwrap();
    host.show_scene_title("検証シーン");
    host.show_bgm_title(12);
    // 34 frames reach the scene title's held full alpha; the BGM banner
    // reaches its resting height at 2 px/tick well before that.
    for _ in 0..40 {
        d.app
            .host
            .as_mut()
            .unwrap()
            .update_notifications(Ticks::from_f32(1.));
        d.step(EnumMap::default());
    }
    let shown = d.capture();
    let (scene, scene_color) = changed(&before, &shown, 0..160, 168..180);
    let (bgm, bgm_color) = changed(&before, &shown, 0..160, 0..10);
    println!("GEROKASU2_NOTIFICATIONS scene={scene} {scene_color:?} bgm={bgm} {bgm_color:?}");
    assert!(scene > 20 && scene_color == [255; 4], "white scene title");
    // 7B3B0 fill (0.75,1,1): 0.75*255 truncates to 191.
    assert!(bgm > 20 && bgm_color == [191, 255, 255, 255], "cyan BGM title");

    // Settings rows 14/15 gate drawing only.
    let settings = &mut d.app.save_store.as_mut().unwrap().data.settings;
    settings.v14_showroutenavi = false;
    settings.v15 = false;
    d.step(EnumMap::default());
    let hidden = d.capture();
    assert_eq!(changed(&before, &hidden, 0..160, 168..180).0, 0);
    assert_eq!(changed(&before, &hidden, 0..160, 0..10).0, 0);
}
