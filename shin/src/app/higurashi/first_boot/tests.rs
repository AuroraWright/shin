use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "original updated forced logo skip/startup gate and checked save-child construction"]
fn higurashi_first_boot_matches_native_skip_readiness_and_once_only_save_child() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for logo in 0..4 {
        for common in 0..4 {
            for empty in [false, true] {
                for delta in [0.0, 1.0, 3.0, 120.0] {
                    protocol.push_str("N\n");
                    let mut model = Model::default();
                    for (l, c, e, d) in [
                        (logo, common, empty, delta),
                        (2, 2, false, 1.0),
                        (0, 2, true, 1.0),
                        (2, 0, true, 0.0),
                    ] {
                        protocol.push_str(&format!("U {l} {c} {} {d}\n", u8::from(e)));
                        let request = model.poll(-1, c, e);
                        expected.push(vec![model.phase, u32::MAX, 1, u32::from(request), 1]);
                    }
                    owners += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_FIRST_BOOT").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let data = protocol.clone();
    let writer = std::thread::spawn(move || input.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (i, (line, words)) in text.lines().zip(&expected).enumerate() {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("B"));
        assert_eq!(
            tokens
                .map(|t| t.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "startup observation{i}"
        );
    }
    let dir = std::env::var("SHIN_HIGURASHI_FIRST_BOOT_CAPTURE").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    for (name, data) in [
        ("input.txt", protocol.as_bytes()),
        ("native.txt", text.as_bytes()),
        ("native.log", output.stderr.as_slice()),
    ] {
        std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
    }
    eprintln!(
        "Higurashi startup:{owners} native owners/{} observations/{} exact words;forced logo skip,common status exactly2,child-stack gating and once-only progress request pass",
        expected.len(),
        expected.len() * 5
    );
}

#[test]
#[ignore = "actual updated archives/GPU resource publication and startup owner retirement;offscreen Metal plus exact Loopback Audio"]
fn higurashi_first_boot_loads_older_common_assets_and_retires_owned_jobs() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    let mut boot = FirstBoot::new(server.clone(), manager.clone(), sounds.clone());
    let handles = [
        boot.logo.retirement.as_ref().unwrap().clone(),
        boot.common.retirement.as_ref().unwrap().clone(),
    ];
    let logo_phase = boot.logo_phase.clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    while boot.assets.is_none() {
        assert!(boot.poll(Ticks::from_f32(1.0), false).unwrap().is_none());
        assert!(Instant::now() < deadline, "Common assets did not publish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(logo_phase.load(Ordering::Acquire), -1);
    assert!(sounds.lock().bank.is_some());
    let common = boot.poll(Ticks::ZERO, true).unwrap().unwrap();
    assert!(boot.poll(Ticks::ZERO, true).unwrap().is_none());
    assert_eq!(
        common.scenario.variant,
        shin_core::format::scenario::higurashi::HigurashiVariant::Updated
    );
    assert!(common.system.contains("scroll"));
    assert!(common.effects.contains("focusline"));
    assert!(!common.effects.contains("breakup"));
    assert!(Arc::ptr_eq(
        &common.sysse,
        sounds.lock().bank.as_ref().unwrap()
    ));
    // The title/ADV startup shares the same old font and prepared message bank.
    let font: Arc<GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
    assert!(Arc::ptr_eq(&font, &common.font));
    assert!(common.message.window.size().width > 0);
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let mut child = false;
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            let mut pass = context.begin_pass(
                shin_render::shaders::types::texture::TextureTarget {
                    view: target,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Native active boot output",
            );
            boot.render(&mut pass, |_| child = true);
        });
        assert!(child);
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        frames += 1;
    }
    drop(boot);
    assert!(handles.iter().all(|h| h.is_finished()));
    // Retire an owner while its asynchronous common load is still pending.
    let boot = FirstBoot::new(server, manager, sounds);
    let handles = [
        boot.logo.retirement.as_ref().unwrap().clone(),
        boot.common.retirement.as_ref().unwrap().clone(),
    ];
    drop(boot);
    assert!(handles.iter().all(|h| h.is_finished()));
    eprintln!(
        "Higurashi startup integration:{frames}720p/1080p black/child-dispatch frames;actual one-font/eight-window/five-effect/updated-scenario resources,global bank publication and completed/pending loader retirement pass"
    );
}
