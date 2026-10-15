//! Standalone Sprite packets and real shared media ownership. Native execution
//! is independently required by the gallery model/parent-render acceptance;
//! this focused test consumes their immutable packets, never generates goldens.
use super::*;
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    asset::system::{AssetLoadContext, LayeredAssetIo},
};
#[path = "../chart/view_tests/gpu.rs"]
mod gpu;
use gpu::{Draw, Renderer};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    path::PathBuf,
    sync::{
        Condvar,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const MODEL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/movie-gallery-model-native-corpus/output.txt"
));
const RENDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/movie-gallery-render-capture/output.txt"
));
fn words(value: &serde_json::Value) -> Vec<u32> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_u64().unwrap().try_into().unwrap())
        .collect()
}
fn sprites() -> Vec<(f32, Vec<u32>)> {
    let mut opacity = 0.;
    let mut out = Vec::new();
    for line in RENDER.lines() {
        if let Some(header) = line.strip_prefix("H ") {
            let header: Vec<u32> = header
                .split_whitespace()
                .map(|word| word.parse().unwrap())
                .collect();
            opacity = f32::from_bits(header[13]);
        } else if let Some(packet) = line.strip_prefix("P ") {
            let packet: Vec<u32> = packet
                .split_whitespace()
                .map(|word| word.parse().unwrap())
                .collect();
            if packet[0] == 2 && packet[2] == 6 {
                out.push((opacity, packet));
            }
        }
    }
    assert_eq!(out.len(), 30);
    out
}
fn packet(opacity: f32) -> Packet {
    Packet::new(
        Vec2::new(0.125, 0.25),
        Vec2::new(0.75, 0.5),
        Vec4::new(1., 1., 1., opacity),
    )
    .unwrap()
}

#[test]
fn higurashi_standalone_movie_matches_63_frozen_original_sprite_packets() {
    let mut opacity = 0.;
    let mut model_count = 0;
    for line in MODEL.lines() {
        if line.starts_with("M ") {
            let value: serde_json::Value = serde_json::from_str(&line[2..]).unwrap();
            if !value["viewer"].is_null() {
                opacity = f32::from_bits(value["viewer"]["opacity"][0].as_u64().unwrap() as u32);
            }
        } else if line.starts_with("D ") {
            let value: serde_json::Value = serde_json::from_str(&line[2..]).unwrap();
            if value["program"] != 2 {
                continue;
            }
            let actual = packet(opacity);
            assert_eq!(
                actual.matrix.to_cols_array().map(f32::to_bits).to_vec(),
                words(&value["matrix"])
            );
            assert_eq!(
                actual.vertices.into_iter().flatten().collect::<Vec<_>>(),
                words(&value["vertices"])
            );
            assert_eq!(actual.blend, value["blend"].as_u64().unwrap() as u32);
            model_count += 1;
        }
    }
    assert_eq!(model_count, 33);
    let mut zero = 0;
    for (opacity, native) in sprites() {
        let actual = packet(opacity);
        assert_eq!(native.len(), 85);
        assert_eq!(&native[6..11], &[1, 4, 0, 24, 64]);
        assert_eq!(actual.blend, native[1]);
        assert_eq!(
            actual.matrix.to_cols_array().map(f32::to_bits).to_vec(),
            native[13..29]
        );
        assert_eq!(
            actual.vertices.into_iter().flatten().collect::<Vec<_>>(),
            native[61..]
        );
        zero += usize::from(opacity == 0.);
    }
    assert!(zero > 0, "Playing alpha0 is an original Sprite submission");
    eprintln!(
        "Standalone Movie:63 immutable original Sprite2 packets (33 model+30 parent), exact matrix/negativezero/UV/packedcolor/order/blend; no codec or native reexecution claim"
    );
}

struct Fixtures(PathBuf);
impl Fixtures {
    fn new(audio: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "shin-higurashi-standalone-movie-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("movie")).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../reverse-engineering/higurashi/live-movie-owner-fixtures")
            .join(if audio {
                "short-aac.mp4"
            } else {
                "short-silent.mp4"
            });
        std::fs::copy(source, path.join("movie/mv01.mp4")).unwrap();
        Self(path)
    }
    fn assets(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<AssetServer> {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&self.0).unwrap();
        Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Higurashi,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ))
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "{label}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn clear(context: &mut crate::render::PreRenderContext, view: &wgpu::TextureView) {
    let mut pass = context.begin_pass(
        TextureTarget {
            view,
            kind: TextureTargetKind::RenderTexture,
        },
        Some(context.depth_stencil),
        "Standalone Movie background",
    );
    pass.clear(
        Some(UnormColor::from_rgba(23, 41, 67, 255)),
        Some(0),
        Some(1.),
    );
}
fn render_player(player: &Player, size: (u32, u32)) -> Vec<u8> {
    let context = player.assets.context();
    reference_render_to_texture(
        &context.wgpu_device,
        &context.wgpu_queue,
        size,
        |context, view| {
            clear(context, view);
            let mut pass = context.begin_pass_preserving_depth_stencil(
                TextureTarget {
                    view,
                    kind: TextureTargetKind::RenderTexture,
                },
                context.depth_stencil,
                "Actual standalone Movie draw",
            );
            assert!(player.draw(&mut pass).unwrap());
        },
    )
}
fn exact_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}");
    let differences = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let max_delta = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert_eq!(
        differences, 0,
        "{label}: changed channels={differences}, maximum delta={max_delta}"
    );
}

#[test]
#[ignore = "real H264/AAC retained RGBA, literal Sprite2 at720p/1080p and pause/EOF; offscreen Metal and exact Loopback Audio, no windows/saves"]
fn higurashi_standalone_movie_real_transport_and_literal_sprite_pixels() {
    let (device, queue, _) = reference_gpu_assets();
    let manager = Arc::new(AudioManager::new_for_tests());
    let originals = sprites();
    let mut renderer = Renderer::new(&device);
    let mut comparisons = 0;
    for audio in [false, true] {
        let fixtures = Fixtures::new(audio);
        let assets = fixtures.assets(&device, &queue);
        assert!(Player::open(assets.clone(), manager.clone(), "missing").is_err());
        let mut player = Player::open(assets, manager.clone(), "MV01").unwrap();
        let retirement = player.loading.as_ref().unwrap().retirement.clone();
        assert!(!player.is_playing());
        until(
            "Player publishes without update or observing worker result",
            || player.status() != 0,
        );
        assert_eq!(player.status(), 1);
        assert!(player.loading.as_ref().unwrap().task.is_some());
        assert!(player.cache.is_none());
        let mut frame = FrameId::default();
        until(
            "Paused player presents/converts retained first frame",
            || {
                frame.advance();
                player.update(frame).unwrap();
                player.cache.is_some()
            },
        );
        assert!(!player.is_playing());
        assert_eq!(player.cache.as_ref().unwrap().extent, (64, 48));
        for opacity in [1., 0.5, 0.] {
            player
                .set_gain(opacity * (255. * f32::from_bits(0x3c23d70a)))
                .unwrap();
            player.set_color(Vec4::new(1., 1., 1., opacity)).unwrap();
            let native = &originals.iter().find(|(a, _)| *a == opacity).unwrap().1;
            // The original corpus's SDK descriptor deliberately has a crop.
            // Real decoded NV12 is exactly the visible extent, so its explicit
            // desktop descriptor is0..1. Bridge ONLY that checked endpoint;
            // retain original matrix/color/positions/blend and literal shader.
            let mut vertices = native[61..].to_vec();
            for (v, uv) in
                vertices
                    .chunks_exact_mut(6)
                    .zip([[0., 0.], [1., 0.], [0., 1.], [1., 1.]])
            {
                v[4] = f32::to_bits(uv[0]);
                v[5] = f32::to_bits(uv[1]);
            }
            let draw = Draw {
                program: 2,
                blend: native[1],
                primitive: 1,
                depth: [7, 0],
                stencil: [7, 0, 0, 0, 255],
                uniforms: vec![native[13..29].to_vec()],
                vertices,
                count: 4,
                start: 0,
            };
            for size in [(1280, 720), (1920, 1080)] {
                let actual = render_player(&player, size);
                let source = player.cache.as_ref().unwrap().texture.as_texture_source();
                let original =
                    reference_render_to_texture(&device, &queue, size, |context, view| {
                        clear(context, view);
                        renderer.draw(context, view, &draw, Some(source), None);
                    });
                exact_pixels(
                    &actual,
                    &original,
                    &format!("Literal standalone Sprite audio{audio}/alpha{opacity}/{size:?}"),
                );
                comparisons += 1;
            }
        }
        player.set_color(Vec4::ONE).unwrap();
        let before = render_player(&player, (1280, 720));
        std::thread::sleep(Duration::from_millis(30));
        frame.advance();
        player.update(frame).unwrap();
        exact_pixels(
            &render_player(&player, (1280, 720)),
            &before,
            "Paused frame retained",
        );
        player.start();
        until("Real video EOF and PCM drain", || {
            frame.advance();
            player.update(frame).unwrap();
            player.status() == 2
        });
        player.pause();
        assert!(!player.is_playing());
        let final_frame = render_player(&player, (1280, 720));
        frame.advance();
        player.update(frame).unwrap();
        exact_pixels(
            &render_player(&player, (1280, 720)),
            &final_frame,
            "Final RGBA retained after pause/EOF",
        );
        player.start();
        player.shutdown();
        assert!(retirement.is_finished(), "Shutdown joins codec/AAC work");
        assert!(player.is_playing(), "Full shutdown preserves desired a88");
        assert!(
            player.published.lock().player.is_some(),
            "External facade retained until release"
        );
        assert!(player.cache.is_some(), "RGBA retained through shutdown");
        exact_pixels(
            &render_player(&player, (1280, 720)),
            &final_frame,
            "Shutdown retains external RGBA until release",
        );
        player.retire();
        assert!(player.published.lock().player.is_none());
        assert!(player.cache.is_none());
        assert!(retirement.is_finished());
        assert_eq!(player.status(), 2);
    }
    assert_eq!(comparisons, 12);
    eprintln!(
        "Standalone Movie:2 real silent/AAC opens; readiness before main poll, paused conversion,12 literal Sprite2 full720p/1080p comparisons with explicit visible-descriptor seam, retained pause/final frame, actual PCM-drained EOF, shutdown joins work while facade/RGBA/desired remain until external release; exact Loopback Audio"
    );
}

#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    changed: Condvar,
}
impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct TailDrop(Arc<std::sync::atomic::AtomicBool>);
impl Drop for TailDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct ReleaseGate(Arc<Gate>);
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
#[ignore = "real silent/AAC prepare cancellation and pre-completion publication; offscreen Metal and exact Loopback Audio, no windows/saves"]
fn higurashi_standalone_movie_publication_cancellation_and_suspension() {
    let (device, queue, _) = reference_gpu_assets();
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut owners = 0;
    for audio in [false, true] {
        for cancel_before_publication in [true, false] {
            let fixtures = Fixtures::new(audio);
            let assets = fixtures.assets(&device, &queue);
            let movie: Arc<Movie> = assets.load_sync("/movie/mv01.mp4").unwrap();
            let gate = Arc::new(Gate::default());
            let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let prepared_ok = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_prepared_ok = prepared_ok.clone();
            let tail_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
            // Construct outside the async tail: cancellation before that tail
            // is ever polled must still publish the actual future's drop.
            let tail_guard = TailDrop(tail_dropped.clone());
            let clock = Arc::new(AtomicU64::new(1_000_000));
            let (worker_gate, worker_entered, worker_clock) =
                (gate.clone(), entered.clone(), clock.clone());
            let (worker_device, worker_audio) = (device.clone(), manager.clone());
            let mut player = Player::preparing(
                assets,
                manager.clone(),
                "controlled real short media".into(),
                move || {
                    worker_entered.store(true, Ordering::Release);
                    worker_gate.wait();
                    let result = movie.prepare_higurashi_with_volume(
                        &worker_device,
                        &worker_audio,
                        Volume(1.),
                        Some(Arc::new(move || worker_clock.load(Ordering::Acquire))),
                    );
                    worker_prepared_ok.store(result.is_ok(), Ordering::Release);
                    result
                },
                async move {
                    let _guard = tail_guard;
                    std::future::pending::<()>().await;
                },
            );
            // Declared after Player so assertion unwinding releases a blocked
            // compute worker before Player's mandatory synchronous retirement.
            let _release_gate = ReleaseGate(gate.clone());
            let retirement = player.loading.as_ref().unwrap().retirement.clone();
            let publication = player.published.clone();
            until("Real compute preparation entered", || {
                entered.load(Ordering::Acquire)
            });
            assert_eq!(player.status(), 0);
            player.set_gain(0.37).unwrap();
            if !cancel_before_publication {
                player.start(); // A pending native desired flag is retained.
                gate.release();
                until(
                    "Published player independent of pending outer future",
                    || player.status() != 0,
                );
                assert_eq!(player.status(), 1);
                assert!(player.is_playing());
                assert!(
                    player
                        .published
                        .lock()
                        .player
                        .as_ref()
                        .unwrap()
                        .is_playing()
                );
                assert_eq!(player.published.lock().gain.to_bits(), 0.37f32.to_bits());
                assert!(!tail_dropped.load(Ordering::Acquire));
                assert!(player.loading.as_ref().unwrap().task.is_some());
                if !audio {
                    clock.fetch_add(25_000, Ordering::AcqRel);
                    manager.set_suspended(true);
                    player.sync_suspension();
                    let frozen = player
                        .published
                        .lock()
                        .player
                        .as_ref()
                        .unwrap()
                        .current_time_micros();
                    clock.fetch_add(8_000_000, Ordering::AcqRel);
                    assert_eq!(
                        player
                            .published
                            .lock()
                            .player
                            .as_ref()
                            .unwrap()
                            .current_time_micros(),
                        frozen,
                        "Silent clock freezes with no scene update"
                    );
                    manager.set_suspended(false);
                    player.sync_suspension();
                    assert_eq!(
                        player
                            .published
                            .lock()
                            .player
                            .as_ref()
                            .unwrap()
                            .current_time_micros(),
                        frozen,
                        "Suspended interval excluded on resume"
                    );
                    assert!(
                        player.is_playing(),
                        "Suspension preserves native desired flag"
                    );
                    clock.fetch_add(17_000, Ordering::AcqRel);
                    assert_eq!(
                        player
                            .published
                            .lock()
                            .player
                            .as_ref()
                            .unwrap()
                            .current_time_micros(),
                        frozen + 17_000
                    );
                }
                player.shutdown();
                assert!(retirement.is_finished());
                assert!(player.is_playing(), "Shutdown retains native desired flag");
                assert!(
                    publication.lock().player.is_some(),
                    "Shutdown retains external facade"
                );
                player.retire();
            } else {
                let (done, completed) = std::sync::mpsc::channel();
                let retiring = std::thread::spawn(move || {
                    player.retire();
                    done.send(()).unwrap();
                });
                until(
                    "Top future canceled while compute gate is still held",
                    || tail_dropped.load(Ordering::Acquire),
                );
                assert!(
                    publication.lock().player.is_none(),
                    "No late outer publication"
                );
                assert!(
                    !retirement.is_finished(),
                    "Running compute remains registered"
                );
                assert!(
                    completed.try_recv().is_err(),
                    "Retirement joins running preparation"
                );
                gate.release();
                completed
                    .recv_timeout(Duration::from_secs(10))
                    .expect("Cancellation drains actual preparation/codec/AAC");
                retiring.join().unwrap();
            }
            assert!(
                prepared_ok.load(Ordering::Acquire),
                "Real H264/AAC preparation succeeded"
            );
            assert!(tail_dropped.load(Ordering::Acquire));
            assert!(publication.lock().player.is_none());
            assert!(retirement.is_finished());
            owners += 1;
        }
    }
    assert_eq!(owners, 4);
    eprintln!(
        "Standalone Movie:4 real silent/AAC cancellation gates before prepare/publication and after publication before executor completion; early desired flag/gain retained, topfuture joined before published codec/PCM drop, all descendant scopes drained; silent focus-loss clock frozen without any update; exact Loopback Audio"
    );
}
