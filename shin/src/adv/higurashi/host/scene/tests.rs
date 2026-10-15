use super::*;
use crate::adv::higurashi::{
    audio::{AudioFramePolicy, MusicEvent},
    autosave::{ProgressEntry, SaveProgress},
    checkpoints::CheckpointServices,
    frame::tests::gpu::render_to_texture,
    input::{Buttons, Context, InputState, OverlayInput, Request},
    message_glyphs::tests::gpu::gpu_assets,
    music_commands::MusicCommands,
    read_history::ReadHistoryServices,
    read_state::ReadPolicy,
    runner::{Policy, Status},
    scene_textures::HigurashiSceneTextures,
};
use shin_core::{
    format::save::{PersistData, index_set::IndexSet},
    primitives::color::UnormColor,
    time::Ticks,
};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn source(active: usize, n: u32) -> ProgressMemory {
    let entries = std::array::from_fn(|i| ProgressEntry {
        scenario_id: if i == active { 0 } else { 10 },
        random_seed: 42 + i as u32,
        save_position: n + i as u32,
        replay_target: n + i as u32,
        response_data: (0..n + 3).map(|j| (j + i as u32) as u8).collect(),
    });
    ProgressMemory::new(
        PersistData(vec![0; 64]),
        SaveProgress::new(active, 77, entries).unwrap(),
        vec![3; 64],
    )
    .with_trailing_bytes(vec![1, 2, 3, 4, 5])
}
fn payload(source: &ProgressMemory) -> Vec<u32> {
    let p = source.progress();
    let mut out = vec![p.active_index() as u32, p.extra_word];
    for e in &p.entries {
        out.extend([
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
            e.response_data.len() as u32,
        ]);
        out.extend(e.response_data.iter().map(|v| u32::from(*v)));
    }
    for bytes in [source.trailing_bytes(), source.bytes()] {
        out.push(bytes.len() as u32);
        out.extend(bytes.iter().map(|v| u32::from(*v)));
    }
    out
}
fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
#[test]
#[ignore = "original queued constructor/deep copy/coalescing/platform old-scene retirement and checked fresh ADV constructor"]
fn higurashi_browser_jump_matches_native_queue_factory_and_owned_vectors() {
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for active in 0..2 {
        for n in [0, 1, 24, 128] {
            for mode in [0, 1, 7, u32::MAX] {
                let mut input = source(active, n);
                let p = input.progress();
                let a = &p.entries[0];
                let b = &p.entries[1];
                protocol.push_str(&format!(
                    "N {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                    p.active_index(),
                    p.extra_word,
                    a.scenario_id as u32,
                    a.random_seed,
                    a.save_position,
                    a.replay_target,
                    hex(&a.response_data),
                    b.scenario_id as u32,
                    b.random_seed,
                    b.save_position,
                    b.replay_target,
                    hex(&b.response_data),
                    hex(input.trailing_bytes()),
                    hex(input.bytes())
                ));
                let notification = |source: ProgressMemory, mode| Notification {
                    browser: BrowserId::reference_id(0),
                    notice: Notice::Reconstruct { source, mode },
                };
                let mut queued =
                    Jump::from_notification(notification(input.clone(), mode)).unwrap();
                protocol.push_str(&format!("Q {mode}\n"));
                let snapshot = |queued: &Jump, pending, events: &[u32]| {
                    let mut words = vec![pending, 1, queued.mode];
                    words.extend(payload(&queued.source));
                    words.push(events.len() as u32);
                    words.extend(events);
                    words
                };
                expected.push(snapshot(&queued, 1, &[]));
                // Mutate the producer after queuing: both response vectors and both tails.
                let mut progress = input.progress().clone();
                for e in &mut progress.entries {
                    e.save_position = e.save_position.wrapping_add(1);
                    if let Some(v) = e.response_data.first_mut() {
                        *v ^= 0x55;
                    }
                }
                let mut trailing = input.trailing_bytes().to_vec();
                trailing[0] ^= 0x55;
                let mut bytes = input.bytes().to_vec();
                bytes[0] ^= 0x55;
                input = ProgressMemory::new(input.persistent().clone(), progress, bytes)
                    .with_trailing_bytes(trailing);
                protocol.push_str("M\n");
                expected.push(snapshot(&queued, 1, &[]));
                let next = mode.wrapping_add(1);
                queued = Jump::from_notification(notification(input, next)).unwrap();
                protocol.push_str(&format!("Q {next}\n"));
                expected.push(snapshot(&queued, 1, &[]));
                protocol.push_str("R\n");
                expected.push(snapshot(&queued, 0, &[1, 2]));
                protocol.push_str("R\n");
                expected.push(snapshot(&queued, 0, &[]));
                protocol.push_str("D\n");
                owners += 1;
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_JUMP").unwrap())
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
        let mut t = line.split_whitespace();
        assert_eq!(t.next(), Some("J"));
        assert_eq!(
            t.map(|v| v.parse::<u32>().unwrap()).collect::<Vec<_>>(),
            *words,
            "queue observation{i}"
        );
    }
    let dir = std::env::var("SHIN_HIGURASHI_BROWSER_JUMP_CAPTURE").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    for (name, data) in [
        ("input.txt", protocol.as_bytes()),
        ("native.txt", text.as_bytes()),
        ("native.log", output.stderr.as_slice()),
    ] {
        std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
    }
    eprintln!(
        "Browser jump:{owners} native owners/{} observations/{} exact words;four complete vectors,private copies,last writer,old destruction before new factory and retirement pass",
        expected.len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
}
fn policy() -> Policy {
    Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 100,
        outline_mode: 0,
    }
}
fn read_policy() -> ReadPolicy {
    ReadPolicy {
        modes: 0,
        forced: false,
        read_only: false,
    }
}
fn audio_policy() -> AudioFramePolicy {
    AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    }
}
fn pointer() -> crate::adv::input_area::PointerFrame<'static> {
    crate::adv::input_area::PointerFrame {
        events: &[],
        number: 0,
        time_seconds: 0.0,
    }
}
fn poll(host: &mut HigurashiHost, pressed: u32, repeat: u32) {
    host.poll_modes_with_input(
        Ticks::from_f32(1.0),
        1,
        Buttons {
            pressed,
            repeat,
            ..Default::default()
        },
        &pointer(),
    )
    .unwrap();
}
fn marker(pass: &mut shin_render::render_pass::RenderPass) {
    use shin_render::{
        DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{buffer::VertexSource, vertices::PosColVertex},
    };
    let vertices = shin_render::quad_vertices::build_quad_vertices(|uv| PosColVertex {
        position: (glam::vec2(-0.5, 0.5) + uv * glam::vec2(0.25, -0.25)).extend(0.0),
        color: UnormColor::from_rgba(32, 96, 160, 255),
    });
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::Fill {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            transform: glam::Mat4::IDENTITY,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
fn draw(
    host: &mut HigurashiHost,
    context: &mut crate::render::PreRenderContext,
    target: shin_render::shaders::types::texture::TextureTarget,
) {
    let mut screens = HigurashiSceneTextures::new(context);
    screens
        .prepare_frame_with_overlay(context, host.runner.frame(), host.touch_overlay.as_deref())
        .unwrap();
    host.prepare_mode_views(context);
    let mut pass = context.begin_pass(
        target,
        Some(context.depth_stencil),
        "Composed platform browser frame",
    );
    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
    host.render_adv(&mut pass, &screens).unwrap();
    host.render_modes(&mut pass);
    marker(&mut pass);
}

#[test]
#[ignore = "real browser Yes/queue/source worker retirement/fresh reconstruction/masked fade/resume on offscreen Metal and exact Loopback Audio"]
fn higurashi_browser_jump_replaces_adv_and_resumes_after_native_fade() {
    use crate::adv::higurashi::{choice::ChoiceHistory, replay::tests::scenario};
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored: Arc<HigurashiScenario> = assets.load_sync("/main.snr").unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let mut code = authored.raw()[0x2545e..0x25465].to_vec();
        for (i, c) in [b'A', b'B', b'C'].into_iter().enumerate() {
            code.push(0x86);
            code.extend(((i + 1) as u32).to_le_bytes()[..3].iter());
            code.extend([1, 0x7f, 4, 0, b'@', b'r', c, 0]);
        }
        code.extend([0, 0, 0]);
        let script = scenario(&code);
        let runner = shin_tasks::block_on(HigurashiRunner::load(
            assets.clone(),
            script.clone(),
            0,
            42,
            || [1; 4],
        ))
        .unwrap();
        let mut host = HigurashiHost::new(
            runner,
            HigurashiAudio::new(manager.clone(), assets.clone(), authored.clone()),
        );
        host.set_menu_frame(1, 900.0);
        let sounds = Arc::new(parking_lot::Mutex::new(
            crate::audio::SystemSoundPlayer::new(manager.clone()),
        ));
        sounds.lock().set_settings_volume(53);
        host.configure_browser_audio(sounds.clone(), 37).unwrap();
        let entries = std::array::from_fn(|i| ProgressEntry {
            scenario_id: 0,
            random_seed: 42,
            save_position: if i == 0 { 2 } else { 99 },
            replay_target: if i == 0 { 2 } else { 99 },
            response_data: vec![0x23; 64],
        });
        let mut progress = ProgressMemory::new(
            PersistData(vec![0; 64]),
            SaveProgress::new(0, 77, entries).unwrap(),
            vec![0; 64],
        )
        .with_trailing_bytes(vec![4, 5, 6, 7]);
        let mut memory = CheckpointMemory::new(vec![0xdeadbeef]);
        let mut history = MessageReadHistory::new();
        let pump = |host: &mut HigurashiHost,
                    memory: &mut CheckpointMemory,
                    progress: &mut ProgressMemory,
                    history: &mut MessageReadHistory| {
            shin_tasks::block_on(host.runner.pump_native(
                policy(),
                read_policy(),
                &mut CheckpointServices {
                    memory,
                    progress,
                    history,
                },
                32,
                10000,
                || [1; 4],
                &mut host.audio,
            ))
            .unwrap()
        };
        assert_eq!(
            pump(&mut host, &mut memory, &mut progress, &mut history),
            Status::Waiting
        );
        for _ in 0..16 {
            host.runner
                .advance(Ticks::from_f32(3.0), policy(), &mut host.audio, None);
            if host.runner.frame().message.playback.interested_in_input() {
                host.runner.click_message(&mut host.audio);
            }
            assert_eq!(
                pump(&mut host, &mut memory, &mut progress, &mut history),
                Status::Waiting
            );
            if host.runner.native_message_timeline().unwrap().boundary == 2 {
                break;
            }
        }
        assert_eq!(host.runner.native_message_timeline().unwrap().boundary, 2);
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.audio.bgm_status() & 1 != 0 {
            host.audio
                .update_frame(Ticks::from_f32(1.0), audio_policy());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let original = progress.clone();
        let choices = ChoiceHistory::new(IndexSet::new(), vec![]);
        let mut input = InputState {
            modes: 0,
            temporary_indicator: 1,
        };
        let mut id = None;
        render_to_texture(&device, &queue, size, |context, _| {
            let mut screens = HigurashiSceneTextures::new(context);
            screens
                .prepare_frame_with_overlay(
                    context,
                    host.runner.frame(),
                    host.touch_overlay.as_deref(),
                )
                .unwrap();
            id = shin_tasks::block_on(host.open_message_browser(
                &mut input, &progress, &history, &choices, false, 10000, context, &screens,
            ))
            .unwrap();
        });
        let id = id.unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            poll(&mut host, 0, 0);
            if host.modes.iter().any(|m|matches!(m,HostMode::Browser(b) if b.browser.lifecycle().is_some_and(|l|l.slide()==1.0))) {break;}
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        poll(&mut host, 0, 1 << 4);
        for _ in 0..20 {
            poll(&mut host, 0, 0);
        }
        let selected = host
            .modes
            .iter()
            .find_map(|m| match m {
                HostMode::Browser(b) => Some(b.browser.lifecycle().unwrap().controls().selected()),
                _ => None,
            })
            .unwrap();
        assert_eq!(selected, 0);
        poll(&mut host, 1 << 15, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            poll(&mut host, 0, 0);
            let ready = host.modes.iter_mut().any(|m| match m {
                HostMode::Browser(b) => {
                    let life = b.browser.lifecycle_mut().unwrap();
                    let mut ids = Vec::new();
                    life.draw_questions(|id| ids.push(id));
                    !ids.is_empty() && ids.iter().all(|id| !life.wants_question_view(*id))
                }
                _ => false,
            });
            if ready {
                break;
            }
            if Instant::now() > deadline {
                panic!("Question preparation stalled");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..14 {
            poll(&mut host, 0, 0);
        }
        poll(&mut host, 0, 1 << 4); // The vertical question moves No -> Yes.
        poll(&mut host, 1 << 24, 0);
        let mut notification = None;
        for _ in 0..120 {
            poll(&mut host, 0, 0);
            for n in host.take_browser_notifications() {
                if matches!(n.notice, Notice::Reconstruct { .. }) {
                    notification = Some(n);
                }
            }
            if notification.is_some() {
                break;
            }
        }
        let notification = notification.expect("Real Yes did not queue a scene jump");
        let Notice::Reconstruct {
            source: ref jumped,
            mode,
        } = notification.notice
        else {
            unreachable!()
        };
        assert_eq!(mode, 0);
        assert_eq!(jumped.progress().active_entry().save_position, 1);
        assert_eq!(jumped.trailing_bytes(), original.trailing_bytes());
        assert_eq!(jumped.progress().entries[1], original.progress().entries[1]);
        assert_eq!(progress.progress(), original.progress());
        assert_eq!(progress.bytes(), original.bytes());
        assert_eq!(host.modal.pause_depth, 1);
        assert_eq!(host.conceal.depth, 1);
        assert!(!host.modal.animations_enabled);
        assert!(host.has_modes());
        let lifetime = host
            .modes
            .iter()
            .find_map(|m| match m {
                HostMode::Browser(b) => Some(b.browser.reference_worker_lifetime()),
                _ => None,
            })
            .unwrap();
        let old_music = host.audio.bgm_clock();
        let queued_copy = jumped.clone();
        let mut slot = SceneSlot::new(host);
        slot.queue_browser_jump(notification.clone()).unwrap();
        assert_eq!(slot.pending_browser(), Some(id));
        assert_eq!(slot.active().unwrap().modal_pause_depth(), 1);
        assert!(
            slot.queue_browser_jump(Notification {
                browser: id,
                notice: Notice::Availability(true)
            })
            .is_err()
        );
        assert_eq!(slot.pending_browser(), Some(id));
        let expected = render_to_texture(&device, &queue, size, |context, target| {
            draw(
                slot.active_mut().unwrap(),
                context,
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
            )
        });
        let actual = render_to_texture(&device, &queue, size, |context, target| {
            let mut composed =
                context.new_render_texture("Composed previous platform frame".into());
            draw(
                slot.active_mut().unwrap(),
                context,
                composed.as_texture_target(),
            );
            assert!(
                shin_tasks::block_on(slot.dispatch_browser_jump(
                    context,
                    &composed,
                    history.clone(),
                    || [2; 4]
                ))
                .unwrap()
            );
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Fresh ADV initial overlay",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            slot.active()
                .unwrap()
                .runner
                .reconstruction_overlay()
                .unwrap()
                .render(&mut pass);
        });
        assert_eq!(
            actual, expected,
            "Composed frame changed during replacement {size:?}"
        );
        assert!(actual.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        frames += 1;
        assert!(lifetime.0.load(Ordering::Acquire));
        assert!(lifetime.1.iter().all(|h| h.is_finished()));
        assert!(slot.pending_browser().is_none());
        let host = slot.active_mut().unwrap();
        assert_eq!(host.modal_pause_depth(), 0);
        assert_eq!(host.conceal.depth, 0);
        assert!(!host.modal.animations_enabled);
        assert!(!host.ui.indicator.hidden);
        assert!(host.runner.native_read_state().is_none());
        assert_eq!(host.runner.reconstruction_phase(), Some(Phase::Preparing));
        assert!(host.runner.cancel_reconstruction_mode().is_err());
        assert_eq!(host.runner.layer_setting(), 1);
        assert!(host.audio.bgm_clock() >= old_music);
        assert!(matches!(
            host.audio.pop_music_event(),
            Some(MusicEvent::Started(_))
        ));
        assert_eq!(sounds.lock().volume_percent, 53);
        let mut reads = ReadHistoryServices::new(history.clone(), |_, _| Ok(()));
        let context = Context {
            permission: true,
            captured: false,
            disabled: false,
            system_locked: false,
            vm: None,
            paused: false,
            continuation_blocked: false,
            overlay_transition_busy: false,
            overlay_transition_value: 0.0,
            notification_busy: false,
            animations_enabled: true,
            overlay: Some(OverlayInput {
                menus: true,
                hide_window: true,
            }),
            hint_busy: false,
            scene_jump_available: false,
            wait_allow_interrupt: 0,
        };
        let gate = host
            .process_input_native(
                &mut input,
                context,
                Buttons {
                    pressed: 1 << 24,
                    ..Default::default()
                },
                &mut reads,
            )
            .unwrap();
        assert_eq!(gate.requests, [Request::FlushInput]);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(!host.poll_modes(Ticks::ZERO, 1).unwrap());
            if host.runner.reconstruction_phase() == Some(Phase::Fading) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(host.modal.animations_enabled);
        assert!(host.runner.native_read_state().is_none());
        assert_eq!(
            host.runner.reconstruction_overlay().unwrap().clock.duration,
            30.0
        );
        for _ in 0..10 {
            assert_eq!(
                shin_tasks::block_on(host.update_native(
                    Ticks::from_f32(3.0),
                    policy(),
                    read_policy(),
                    &mut reads,
                    audio_policy(),
                    32,
                    10000,
                    || [3; 4]
                ))
                .unwrap(),
                Status::Waiting
            );
        }
        assert!(host.runner.native_read_state().is_none());
        assert!(!host.poll_modes(Ticks::ZERO, 1).unwrap());
        assert!(host.runner.native_read_state().is_some());
        let result = host.runner.take_reconstruction_result().unwrap();
        assert_eq!(result.progress.progress(), queued_copy.progress());
        assert_eq!(
            result.progress.trailing_bytes(),
            queued_copy.trailing_bytes()
        );
        assert!(result.memory.extra_words.is_empty());
        assert!(
            result
                .memory
                .entries()
                .iter()
                .all(|e| e.state.extra_words.is_empty())
        );
        assert!(host.poll_modes(Ticks::ZERO, 1).unwrap());
        assert!(!host.has_modes());
        let mut restored_memory = result.memory;
        let mut restored_progress = result.progress;
        assert_eq!(
            pump(
                host,
                &mut restored_memory,
                &mut restored_progress,
                &mut history
            ),
            Status::Waiting
        );
        assert_eq!(host.runner.native_message_timeline().unwrap().boundary, 1);
        assert_eq!(
            host.runner
                .native_playback_descriptors()
                .unwrap()
                .message
                .text
                .as_deref(),
            Some("@rA")
        );
        assert!(host.audio.bgm_clock() >= old_music);
        assert!(host.audio.pop_music_event().is_none());
        assert!(host.set_browser_voice_playing(id, false).is_err());
    }
    eprintln!(
        "Browser jump integration:{frames}720p/1080p exact composed captures;real Yes/private source,source workers retired,fresh no-VM/counters,global BGM/cues,30-tick masked fade,empty archive and resumed target message pass"
    );
}
