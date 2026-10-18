use super::transitions::{Frames, assets};
use super::*;
use crate::adv::gerokasu2::{
    layer_factory::GerDrawable, movie::MovieServices, play_scene::Gerokasu2PlayScene,
    runner::FramePolicy,
};
use shin_core::primitives::update::FrameId;
use shin_render::shaders::types::{RenderClone, RenderCloneCtx};
fn num(d: &mut Vec<u8>, v: i32) {
    let mut c = Cursor::new(Vec::new());
    NumberSpec::<i32>::constant(v).write_le(&mut c).unwrap();
    d.extend(c.into_inner());
}
fn context() -> MessageCommandContext {
    MessageCommandContext {
        was_read: false,
        event_mode: 0,
        skip: false,
        message_speed: 50,
    }
}
fn policy() -> FramePolicy {
    FramePolicy {
        paused: false,
        scene_changing: false,
        animations_allowed: true,
        autoplay: false,
        fast_skip: false,
    }
}
struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    rows: usize,
}
impl Native {
    fn new() -> Self {
        use std::process::{Command, Stdio};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary = root.join("reverse-engineering/gerokasu2/movie/native-movie");
        let c = Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join("gerokasu2/native-movie.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        let mut child = Command::new(binary)
            .arg(root.join("gerokasu2/uncompressed_executable"))
            .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            rows: 0,
        }
    }
    fn compare(&mut self, row: [u32; 8], index: usize, state: [u32; 7]) {
        use std::io::Write;
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &row.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let mut bytes = [0u8; 120];
        self.output.read_exact(&mut bytes).unwrap();
        let at = 36 + index * 28;
        let actual: Vec<u32> = bytes[at..at + 28]
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(actual, state, "native movie row{}", self.rows);
        self.rows += 1;
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}
#[test]
#[ignore = "requires original op2.mp4,FFmpeg,Metal and exact Loopback Audio"]
fn ger_movie_scene_original_h264_aac_wait_render_copy_skip_unload_and_exit() {
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut d = bytes();
    d.truncate(original.header.code_offset as usize);
    d.push(0xc1);
    for v in [10, 6, 0] {
        num(&mut d, v);
    }
    d.push(3);
    num(&mut d, 1);
    num(&mut d, 1000);
    d.push(0xc7);
    num(&mut d, 10);
    num(&mut d, 3);
    d.push(0xc2);
    num(&mut d, 10);
    num(&mut d, 0);
    d.extend([0, 0]);
    num(&mut d, 0);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    let mut scene = shin_tasks::block_on(Gerokasu2PlayScene::load(
        assets(&gpu),
        Arc::new(Gerokasu2Scenario::new(d.into()).unwrap()),
        Arc::new(AudioManager::new_for_tests()),
        0,
        42,
        InitialMessage {
            style: 0,
            asynchronous: false,
            face: -1,
        },
        Default::default(),
        context(),
    ))
    .unwrap();
    let mut frame = FrameId::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut frames = Frames::new(&gpu);
    let mut started = None;
    let mut target = None;
    let mut visible = false;
    let mut cloned = false;
    let mut native = Native::new();
    let mut skipped = false;
    let mut playback = String::new();
    loop {
        frame.advance();
        let status = scene
            .step(frame, Ticks::from_f32(1.0), policy(), context())
            .unwrap();
        assert_ne!(
            status,
            RunnerStatus::External,
            "{:?}",
            scene.runner().pending_command()
        );
        if matches!(
            scene.runner().pending_command(),
            Some(Gerokasu2RuntimeCommand::MovieWait { .. })
        ) {
            let t = scene
                .runner()
                .layers()
                .target(LayerId::try_new(10).unwrap())
                .unwrap();
            let plane = scene.runner().layers().current_plane();
            target = Some((plane, t.layerbank));
            assert_eq!(
                scene
                    .backend_mut()
                    .movie_status(plane, t.layerbank)
                    .unwrap(),
                3
            );
            let guard = scene.backend().layer(plane, t.layerbank).unwrap();
            let GerDrawable::Shared(crate::layer::user::UserLayer::Movie(movie)) = &guard.layer
            else {
                panic!("Actual MovieLayer");
            };
            playback = movie.reference_playback_status();
            assert!(playback.contains("backend=Some"), "{playback}");
            if !cloned {
                native.compare(
                    [0, 0, 0, 1, 0, 1, 0, 0],
                    0,
                    movie.reference_gerokasu2_state(),
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                let mut clone = movie.render_clone(&mut RenderCloneCtx {
                    device: &gpu.device,
                    encoder: &mut encoder,
                });
                gpu.queue.submit([encoder.finish()]);
                native.compare(
                    [3, 1, 0, 0, 0, 0, 0, 0],
                    1,
                    clone.reference_gerokasu2_state(),
                );
                clone.fast_forward();
                native.compare(
                    [2, 1, 0, 0, 0, 0, 0, 0],
                    1,
                    clone.reference_gerokasu2_state(),
                );
                assert_eq!(clone.gerokasu2_wait_status(), 0);
                assert_eq!(movie.gerokasu2_wait_status(), 3);
                cloned = true;
            }
            drop(guard);
            let start = *started.get_or_insert_with(Instant::now);
            if start.elapsed() >= Duration::from_secs(5) && visible {
                scene
                    .backend_mut()
                    .layer_mut(plane, t.layerbank)
                    .unwrap()
                    .fast_forward();
                let guard = scene.backend().layer(plane, t.layerbank).unwrap();
                let GerDrawable::Shared(crate::layer::user::UserLayer::Movie(movie)) = &guard.layer
                else {
                    unreachable!()
                };
                native.compare(
                    [2, 0, 0, 0, 0, 0, 0, 0],
                    0,
                    movie.reference_gerokasu2_state(),
                );
                skipped = true;
            }
        }
        if started.is_some() && frames.frames < 40 {
            let pixels = frames.draw_scene(&gpu, scene.backend_mut());
            visible |= pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]);
        }
        if status == RunnerStatus::Exited {
            break;
        }
        assert!(Instant::now() < deadline, "Movie playback {playback}");
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(
        visible && cloned && skipped,
        "visible={visible} cloned={cloned} skipped={skipped} {playback}"
    );
    assert_eq!(scene.runner().completed_commands(), 4);
    let (plane, bank) = target.unwrap();
    assert!(scene.backend().layer(plane, bank).is_none());
    println!(
        "GEROKASU2_MOVIE_SCENE commands=4 original_op2_h264_aac=1 native_lease_snapshots=4 wait_mask=3 independent_clone_skip=1 explicit_primary_skip=1 visible=1 external_acknowledgements=0 frames={} channels={} audio=Loopback Audio playback={playback}",
        frames.frames, frames.channels
    );
}
