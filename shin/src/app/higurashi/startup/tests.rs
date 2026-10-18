use super::*;
use crate::app::higurashi::load_game::{Paths, QuestionKind};
use std::time::{Duration, Instant};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "updated full loaded-save copy/clock/platform/notice/title factory and queue;checked Title constructor"]
fn higurashi_startup_transition_matches_native_loaded_globals_notice_and_title_queue() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for profile in 0..33 {
        for seconds in [0, 1, 93, u32::MAX] {
            for flag in [false, true] {
                protocol.push_str(&format!("N {profile} {seconds} {}\n", flag as u32));
                let mut saved = SaveData::default();
                saved.play_seconds = seconds;
                saved.settings.common.v16_showtoucheffect = flag;
                let mut transition = Transition::default();
                transition.loaded(&saved);
                let snapshot = |t: &Transition, events: &[u64]| {
                    let mut words = vec![
                        t.play_ticks.to_bits(),
                        u64::from(t.platform_flag),
                        1,
                        0,
                        0,
                        u64::from(t.pending.is_some()),
                        t.pending.map_or(0, |p| u64::from(p.mode)),
                        1,
                        events.len() as u64,
                    ];
                    words.extend(events);
                    words
                };
                expected.push(("N", snapshot(&transition, &[])));
                for (stage, button) in [(0, 0), (2, -1), (u32::MAX, -2), (1, 0), (0, 1), (1, -1)] {
                    protocol.push_str(&format!("Q {stage} {button}\n"));
                    transition
                        .callback(crate::adv::higurashi::question::Callback { stage, button });
                    expected.push(("Q", snapshot(&transition, &[])));
                }
                assert_eq!(
                    transition.pending.take(),
                    Some(TitleDemoRequest { mode: 0 })
                );
                protocol.push_str("R\nD\n");
                // Original dispatch is executed and checked independently:old scene
                // deletion precedes the boundary for the still-unimplemented Title ctor.
                expected.push(("R", snapshot(&transition, &[1, 2])));
                owners += 1;
            }
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_STARTUP_TRANSITION").unwrap())
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
    for (index, (line, (tag, words))) in text.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some(*tag));
        assert_eq!(
            fields
                .map(|v| v.parse::<u64>().unwrap())
                .collect::<Vec<_>>(),
            *words,
            "startup transition observation{index}"
        );
    }
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    eprintln!(
        "Higurashi startup transition:{owners} owners/{} observations/{} exact words;all101 slots/full physical vectors copied natively,seconds/platform flag/notice/final callback/title queue and native dispatch pass",
        expected.len(),
        expected.iter().map(|(_, words)| words.len()).sum::<usize>()
    );
}

#[test]
#[ignore = "actual mandatory startup notice,loaded settings and final-stage TitleDemo request;offscreen Metal plus exact Loopback Audio"]
fn higurashi_startup_auto_sleep_notice_gates_title_and_retains_global_cues() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    use crate::app::higurashi::load_game::Source;
    struct Saved(Vec<u8>);
    impl Storage for Saved {
        fn trial_token(&self) -> bool {
            false
        }
        fn read(&self, _: Source) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(Some(self.0.clone()))
        }
        fn trial_exists(&self) -> anyhow::Result<bool> {
            Ok(false)
        }
    }
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let pointer = PointerFrame {
        events: &[],
        number: 0,
        time_seconds: 0.0,
    };
    let mut frames = 0;
    for flag in [false, true] {
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut saved = SaveData::default();
        saved.play_seconds = 93;
        saved.settings.common.v16_showtoucheffect = flag;
        saved.settings.common.v3_sysvol = 37;
        let mut startup = Startup::new(
            server.clone(),
            manager.clone(),
            sounds.clone(),
            Arc::new(Saved(saved.encode().unwrap())),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let ready = loop {
            if let Some(ready) = startup
                .poll(Ticks::from_f32(1.0), Buttons::default(), &pointer)
                .unwrap()
            {
                break ready;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(ready.platform_flag, flag);
        assert_eq!(ready.play_ticks, 5580.0);
        assert_eq!(ready.data.encode().unwrap(), saved.encode().unwrap());
        assert_eq!(sounds.lock().volume_percent, 37);
        assert_eq!(startup.children.len(), 2);
        assert!(startup.take_title_demo_request().is_none());
        let render = |startup: &mut Startup, size| {
            render_to_texture(&device, &queue, size, |context, target| {
                startup.prepare(context);
                let mut pass = context.begin_pass(
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Higurashi sleep notice/title transition",
                );
                startup.render(&mut pass);
            })
        };
        // The new parent notice is excluded from the loaded frame's snapshot.
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render(&mut startup, size);
            assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            frames += 1;
        }
        loop {
            assert!(
                startup
                    .poll(Ticks::from_f32(1.0), Buttons::default(), &pointer)
                    .unwrap()
                    .is_none()
            );
            if startup.children.iter().any(|child| {
                child
                    .reference_question()
                    .is_some_and(|(kind, phase, _)| kind == QuestionKind::AutoSleep && phase == 2)
            }) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render(&mut startup, size);
            assert!(
                pixels
                    .chunks_exact(4)
                    .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            );
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            frames += 1;
        }
        assert!(
            startup
                .poll(
                    Ticks::from_f32(1.0),
                    Buttons {
                        pressed: 1 << 25,
                        ..Default::default()
                    },
                    &pointer
                )
                .unwrap()
                .is_none()
        );
        assert!(startup.take_title_demo_request().is_none());
        assert!(
            startup
                .children
                .iter()
                .any(|child| child.reference_question() == Some((QuestionKind::AutoSleep, 2, 0)))
        );
        assert!(
            startup
                .poll(
                    Ticks::from_f32(1.0),
                    Buttons {
                        pressed: 1 << 24,
                        ..Default::default()
                    },
                    &pointer
                )
                .unwrap()
                .is_none()
        );
        assert!(startup.take_title_demo_request().is_none());
        let mut closing = false;
        loop {
            assert!(
                startup
                    .poll(Ticks::from_f32(1.0), Buttons::default(), &pointer)
                    .unwrap()
                    .is_none()
            );
            if let Some(request) = startup.take_title_demo_request() {
                assert_eq!(request, TitleDemoRequest { mode: 0 });
                break;
            }
            if startup.children.iter().any(|child| {
                child
                    .reference_question()
                    .is_some_and(|(kind, phase, _)| kind == QuestionKind::AutoSleep && phase == 4)
            }) {
                closing = true;
            }
            assert!(Instant::now() < deadline);
        }
        assert!(closing);
        assert!(
            startup
                .children
                .iter()
                .any(|child| matches!(child,Child::Notice(q) if q.complete()))
        );
        assert!(!sounds.lock().sounds.is_empty());
        for size in [(1280, 720), (1920, 1080)] {
            let pixels = render(&mut startup, size);
            assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            frames += 1;
        }
        let handles: Vec<_> = startup
            .children
            .iter()
            .flat_map(|child| child.reference_retirements())
            .collect();
        assert!(!handles.is_empty());
        assert!(
            startup
                .poll(Ticks::ZERO, Buttons::default(), &pointer)
                .unwrap()
                .is_none()
        );
        assert!(startup.take_title_demo_request().is_none());
        drop(startup);
        assert!(handles.iter().all(|handle| handle.is_finished()));
        assert!(
            !sounds.lock().sounds.is_empty(),
            "global cue survives startup retirement"
        );
    }
    eprintln!(
        "Higurashi mandatory startup notice:2 saved platform flags,{frames} frames at720p/1080p;loaded settings/clock,delayed parent snapshot,B ignored,A confirmation,stage0 gate,one owned mode0 request,closing-frame retention/global cues and loader retirement pass"
    );
}

#[test]
#[ignore = "actual Higurashi startup/save child/questions/GPU publication/retirement;offscreen Metal and exact Loopback Audio"]
fn higurashi_startup_load_game_retains_real_questions_and_publishes_loaded_data() {
    use crate::adv::higurashi::{
        reference_gpu_assets as gpu_assets, reference_render_to_texture as render_to_texture,
    };
    let (device, queue, server) = gpu_assets();
    let server = Arc::new(server);
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let dir = std::env::temp_dir().join(format!(
        "higurashi-startup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let pointer = PointerFrame {
        events: &[],
        number: 0,
        time_seconds: 0.0,
    };
    let mut saved = SaveData::default();
    saved.play_seconds = 93;
    saved.extra_word = 0x81234567;
    saved.persist_data.set(24, 3);
    let bytes = saved.encode().unwrap();
    let mut frames = 0;
    let mut owners = 0;
    // Normal save, corrupt notice, trial inheritance and desktop IO retry.
    for kind in [
        None,
        Some(QuestionKind::Recovery(1)),
        Some(QuestionKind::Inherit),
        Some(QuestionKind::ReadFailure),
    ] {
        let current = dir.join(format!("current{owners}"));
        let trial = dir.join(format!("trial{owners}"));
        match kind {
            None => std::fs::write(&current, &bytes).unwrap(),
            Some(QuestionKind::Recovery(_)) => std::fs::write(&current, [1, 2, 3]).unwrap(),
            Some(QuestionKind::Inherit) => std::fs::write(&trial, &bytes).unwrap(),
            _ => std::fs::create_dir(&current).unwrap(),
        }
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut startup = Startup::new(
            server.clone(),
            manager.clone(),
            sounds.clone(),
            Arc::new(Paths {
                current: Some(current.clone()),
                trial: if kind == Some(QuestionKind::Inherit) {
                    Some(trial.clone())
                } else {
                    None
                },
            }),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut ready = None;
        loop {
            let published = startup
                .poll(Ticks::from_f32(1.0), Buttons::default(), &pointer)
                .unwrap();
            if published.is_some() {
                assert!(kind.is_none());
                ready = published;
                break;
            }
            if let Some(kind) = kind {
                if startup.children.iter().any(|child| {
                    child
                        .reference_question()
                        .is_some_and(|(k, p, _)| k == kind && p == 2)
                }) {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "startup/read/question publication"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(kind) = kind {
            assert!(ready.is_none());
            assert_eq!(startup.children.len(), 1);
            for size in [(1280, 720), (1920, 1080)] {
                let pixels = render_to_texture(&device, &queue, size, |context, target| {
                    startup.prepare(context);
                    let mut pass=context.begin_pass(shin_render::shaders::types::texture::TextureTarget {view:target,kind:shin_render::shaders::types::texture::TextureTargetKind::RenderTexture},Some(context.depth_stencil),"Higurashi startup storage question");
                    startup.render(&mut pass);
                });
                assert!(
                    pixels
                        .chunks_exact(4)
                        .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0),
                    "question did not draw"
                );
                assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
                frames += 1;
            }
            if kind == QuestionKind::Inherit {
                assert!(
                    startup
                        .poll(
                            Ticks::from_f32(1.0),
                            Buttons {
                                repeat: 1 << 7,
                                ..Default::default()
                            },
                            &pointer
                        )
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    startup
                        .children
                        .iter()
                        .next()
                        .unwrap()
                        .reference_question()
                        .unwrap()
                        .2,
                    0
                );
            }
            if kind == QuestionKind::ReadFailure {
                std::fs::remove_dir(&current).unwrap();
                std::fs::write(&current, &bytes).unwrap();
                assert!(
                    startup
                        .poll(
                            Ticks::from_f32(1.0),
                            Buttons {
                                pressed: 1 << 25,
                                ..Default::default()
                            },
                            &pointer
                        )
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    startup
                        .children
                        .iter()
                        .next()
                        .unwrap()
                        .reference_question()
                        .unwrap()
                        .1,
                    2
                );
            }
            assert!(
                startup
                    .poll(
                        Ticks::from_f32(1.0),
                        Buttons {
                            pressed: 1 << 24,
                            ..Default::default()
                        },
                        &pointer
                    )
                    .unwrap()
                    .is_none()
            );
            // Parent callback waits through feedback and closing. Native early
            // stage0 must not publish defaults or start reading the trial file.
            loop {
                let published = startup
                    .poll(Ticks::from_f32(1.0), Buttons::default(), &pointer)
                    .unwrap();
                if published.is_some() {
                    ready = published;
                    break;
                }
                assert!(Instant::now() < deadline, "question callback/storage retry");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !sounds.lock().sounds.is_empty(),
                "actual global confirmation cue"
            );
            assert_eq!(sounds.lock().volume_percent, 40);
            match kind {
                QuestionKind::Inherit => {
                    assert!(!current.exists());
                    assert_eq!(std::fs::read(&trial).unwrap(), bytes);
                }
                QuestionKind::Recovery(_) => {
                    assert_eq!(std::fs::read(&current).unwrap(), [1, 2, 3])
                }
                _ => assert_eq!(std::fs::read(&current).unwrap(), bytes),
            }
        }
        let ready = ready.unwrap();
        if matches!(kind, Some(QuestionKind::Recovery(_))) {
            assert_eq!(
                ready.data.encode().unwrap(),
                SaveData::default().encode().unwrap()
            );
            assert_eq!(ready.play_ticks, 0.0);
        } else {
            assert_eq!(ready.data.encode().unwrap(), bytes);
            assert_eq!(ready.play_ticks, 93.0 * 60.0);
        }
        let font: Arc<crate::asset::font::GpuFontLazy> = server.load_sync("/newrodin.fnt").unwrap();
        assert!(Arc::ptr_eq(&font, &ready.common.font));
        let handles: Vec<_> = startup
            .children
            .iter()
            .flat_map(|child| child.reference_retirements())
            .collect();
        assert!(!handles.is_empty());
        assert!(
            startup
                .poll(Ticks::ZERO, Buttons::default(), &pointer)
                .unwrap()
                .is_none()
        );
        drop(startup);
        assert!(handles.iter().all(|handle| handle.is_finished()));
        owners += 1;
    }
    std::fs::remove_dir_all(dir).unwrap();
    eprintln!(
        "Higurashi startup/load integration:{owners} owners,{frames}720p/1080p question frames;normal/corrupt/trial-Yes/desktop-IO-retry,exact data/clock/one-font publication,global cues,no storage writes,retained producer/question retirement pass"
    );
}
