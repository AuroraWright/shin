use super::*;
use crate::adv::higurashi::{
    audio::{AudioFramePolicy, HigurashiAudio},
    autosave::{ProgressEntry, SaveProgress},
    frame::tests::gpu::render_to_texture,
    input::{self, Buttons, Context},
    message_glyphs::tests::gpu::gpu_assets,
    read_history::ReadHistoryServices,
    read_state::ReadPolicy,
    replay::tests::scenario,
    runner::{HigurashiRunner, Policy, Status},
    scene_textures::HigurashiSceneTextures,
};
use shin_core::{
    format::{save::index_set::IndexSet, scenario::higurashi::HigurashiScenario},
    primitives::color::UnormColor,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[test]
#[ignore = "real browser independent replay/global cues/music over the default audio output and offscreen Metal"]
fn higurashi_browser_audio_plays_while_story_paused_and_restores_music() {
    use crate::adv::higurashi::message_playback::MessageVoice;
    use crate::adv::higurashi::voice_commands::VoiceCommands;
    use crate::audio::SystemSoundPlayer;
    use shin_core::vm::command::types::{AudioWaitStatus, Pan};
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored: Arc<HigurashiScenario> = assets.load_sync("/main.snr").unwrap();
    let first = "S01/11/VTF_oishi_0001";
    let text = format!("@r@o75.@v{first}|{first}.A@o25.@v{first}.");
    let mut code = vec![0x86, 1, 0, 0, 1, 0x7f];
    code.extend(((text.len() + 1) as u16).to_le_bytes());
    code.extend(text.bytes());
    code.extend([0, 0, 0, 0]);
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario(&code),
        0,
        42,
        || [1; 4],
    ))
    .unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut host = HigurashiHost::new(
        runner,
        HigurashiAudio::new(manager.clone(), assets.clone(), authored),
    );
    let sounds = Arc::new(parking_lot::Mutex::new(SystemSoundPlayer::new(manager)));
    sounds.lock().set_settings_volume(37);
    host.configure_browser_audio(sounds.clone(), 37).unwrap();
    host.audio.set_modal_voice_paused(true);
    assert!(host.audio.play(first, 0.8, true, 10, 300));
    let audio_policy = AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while host.audio.status_bits() & 1 != 0 {
        host.audio.update_frame(Ticks::ZERO, audio_policy);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(host.audio.position_micros(), 10000);
    host.audio.set_preferences(100, 0, true, 100); // Replay must bypass the muted characters.
    let mut input = InputState {
        modes: 0,
        temporary_indicator: 1,
    };
    let history = MessageReadHistory::new();
    let choices = ChoiceHistory::new(IndexSet::new(), vec![]);
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut screens = HigurashiSceneTextures::new(context);
        let unavailable = source(0);
        assert!(
            shin_tasks::block_on(host.open_message_browser(
                &mut input,
                &unavailable,
                &history,
                &choices,
                false,
                10000,
                context,
                &screens
            ))
            .unwrap()
            .is_none()
        );
        assert_eq!(host.modal.pause_depth, 0);
        assert_eq!(sounds.lock().sounds.len(), 1); // Original unavailable "error".
        screens
            .prepare_scene(context, &host.runner.frame().scene)
            .unwrap();
        assert!(
            shin_tasks::block_on(host.open_message_browser(
                &mut input,
                &source(1),
                &history,
                &choices,
                false,
                10000,
                context,
                &screens
            ))
            .unwrap()
            .is_some()
        );
    });
    assert_eq!(sounds.lock().volume_percent, 37);
    assert_eq!(sounds.lock().sounds.len(), 2); // Original available "page".
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        poll(&mut host, 0);
        if host.modes.iter().any(|m|matches!(m,HostMode::Browser(b) if b.browser.lifecycle().is_some_and(|l|l.slide()==1.0))) { break; }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    host.set_browser_frame(BrowserFrame {
        duck_music: true,
        outline_mode: 0,
    })
    .unwrap();
    let id = host
        .modes
        .iter_mut()
        .find_map(|m| match m {
            HostMode::Browser(b) => {
                b.browser.reference_clear_glyph_cache();
                Some(b.id)
            }
            _ => None,
        })
        .unwrap();
    poll(&mut host, 1 << 24);
    assert!(host.set_browser_voice_playing(id, true).is_err());
    for _ in 0..8 {
        poll(&mut host, 0);
    }
    assert!(host.modes.iter().any(|m|matches!(m,HostMode::Browser(b) if b.audio.as_ref().unwrap().voice.get_wait_status().is_empty())));
    poll(&mut host, 0); // Tenth update starts the simultaneous voice files.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut advanced = false;
    loop {
        poll(&mut host, 0);
        let owner = host
            .modes
            .iter()
            .find_map(|m| match m {
                HostMode::Browser(b) => Some(b),
                _ => None,
            })
            .unwrap();
        let replay = &owner.audio.as_ref().unwrap().voice;
        if !replay.get_wait_status().contains(AudioWaitStatus::LOADING)
            && replay.position_millis() > 0
        {
            let metadata = replay.reference_voice_metadata();
            assert_eq!(metadata.len(), 2);
            assert!(
                metadata
                    .iter()
                    .all(|(speakers, pan)| speakers.is_empty() && *pan == Pan(0.0))
            );
            advanced = true;
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Independent replay stalled under story pause"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(advanced);
    assert_eq!(host.audio.position_micros(), 10000);
    assert_eq!(host.modal.pause_depth, 1);
    let effects = host.audio.reference_focus();
    let audio_policy = host.modal_audio_policy(audio_policy);
    assert!(!audio_policy.focus_mode_active && !audio_policy.effects_update_enabled);
    host.audio.update_frame(Ticks::from_f32(1.0), audio_policy);
    assert_eq!(host.audio.reference_music_focus(), 0.96);
    assert_eq!(host.audio.reference_focus(), effects);
    assert!(!host.take_browser_notifications().iter().any(|n| matches!(
        n.notice,
        Notice::Voice { .. }
            | Notice::VoiceTick
            | Notice::MusicMultiplier(_)
            | Notice::ControlCue(_)
    )));
    poll(&mut host, 1 << 25);
    for _ in 0..30 {
        poll(&mut host, 0);
    }
    assert!(!host.has_modes());
    let restore = host.modal_audio_policy(AudioFramePolicy {
        focus_mode_active: false,
        ..audio_policy
    });
    host.audio.update_frame(Ticks::from_f32(1.0), restore);
    assert_eq!(host.audio.reference_music_focus(), 1.0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while sounds
        .lock()
        .sounds
        .iter()
        .all(|s| s.position_samples() == 0)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "Global cue bus stalled during modal pause"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(sounds.lock().volume_percent, 37);
    eprintln!(
        "Browser audio integration:actual error/page/global close cues,ten-update simultaneous NXA replay without glyph cache while story paused,muting/pan/lipsync bypass,music-only duck/recovery and no duplicate audio notices pass"
    );
}

fn source(target: u32) -> ProgressMemory {
    let entries = std::array::from_fn(|_| ProgressEntry {
        scenario_id: 0,
        random_seed: 42,
        save_position: target,
        replay_target: target,
        response_data: vec![],
    });
    ProgressMemory::new(
        Default::default(),
        SaveProgress::new(0, 0, entries).unwrap(),
        vec![0; 16],
    )
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
fn pointer() -> crate::adv::input_area::PointerFrame<'static> {
    crate::adv::input_area::PointerFrame {
        events: &[],
        number: 0,
        time_seconds: 0.0,
    }
}
fn poll(host: &mut HigurashiHost, pressed: u32) {
    host.poll_modes_with_input(
        Ticks::from_f32(1.0),
        1,
        Buttons {
            pressed,
            ..Default::default()
        },
        &pointer(),
    )
    .unwrap();
}
fn seed(host: &mut HigurashiHost, pause: u32, conceal: u32) {
    host.modal.pause_depth = pause;
    host.modal.animations_enabled = true;
    host.conceal.depth = conceal;
    host.audio.set_modal_voice_paused(pause != 0);
    host.audio.set_modal_effects_paused(pause != 0);
    host.ui.indicator.hidden = conceal != 0;
    host.ui.menu.indicator.hidden = conceal != 0;
}
fn words(host: &mut HigurashiHost, input: &InputState, permission: u32) -> Vec<u32> {
    let mut values = vec![
        permission,
        input.modes,
        u32::from(input.temporary_indicator),
        host.modal.pause_depth,
        host.conceal.depth,
        u32::from(host.modal.animations_enabled),
        u32::from(host.audio.modal_effects_paused()),
        u32::from(host.audio.modal_effects_paused()),
        u32::from(host.ui.indicator.hidden),
        u32::from(host.ui.menu.indicator.hidden),
        host.modal.fade.value().to_bits(),
        u32::from(host.modal.fade.is_waiting()),
        host.modes.len() as u32,
    ];
    values.extend(host.modes.iter().map(|m| match m {
        HostMode::Browser(b) => b.completion(),
        _ => panic!("mode"),
    }));
    let mut draw = Vec::new();
    host.modes.draw(|m| match m {
        HostMode::Browser(b) => draw.push(b.completion()),
        _ => panic!("draw"),
    });
    values.push(draw.len() as u32);
    values.extend(draw);
    values
}
fn callback(host: &mut HigurashiHost, complete: bool) {
    let mut services = ModeServices {
        menu_children: None,
        runner: &mut host.runner,
        audio: &mut host.audio,
        ui: &mut host.ui,
        conceal: &mut host.conceal,
        overlay_visibility: &mut host.overlay_visibility,
        touch_overlay: &mut host.touch_overlay,
        buttons: Buttons::default(),
        pointer: None,
        mouse_release: None,
        modal: &mut host.modal,
        menu_frame: host.menu_frame,
        system_sounds: host.system_sounds.as_ref(),
        menu_requests: &mut host.menu_requests,
        next_menu_child: &mut host.next_menu_child,
        save_notifications: &mut host.save_notifications,
        menu_tips_return: &mut host.menu_tips_return,
        menu_return: &mut host.menu_return,
        browser_frame: host.browser_frame,
        browser_notifications: &mut host.browser_notifications,
        navigation_disabled: host.navigation_disabled,
        chapter_requests: &mut host.chapter_requests,
    };
    let owner = host
        .modes
        .iter_mut()
        .find_map(|m| match m {
            HostMode::Browser(b) => Some(b),
            _ => None,
        })
        .unwrap();
    let mut events = Vec::new();
    let mut callbacks = Callbacks {
        id: owner.id,
        assets: owner.assets.clone(),
        jobs: &mut owner.jobs,
        scope_active: &mut owner.scope_active,
        voice_playing: false,
        audio: None,
        rows: owner.browser.history_rows(),
        events: &mut events,
        services: &mut services,
    };
    browser_lifecycle::Services::returned(&mut callbacks, complete);
}
fn compare_native(protocol: &str, expected: &[Vec<u32>]) {
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_HOST").unwrap())
        .args([
            "game-data/higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = protocol.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut actual = Vec::new();
    for line in text.lines() {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("H"));
        let op = tokens.next().unwrap();
        let mut data: Vec<u32> = tokens.map(|t| t.parse().unwrap()).collect();
        let count = data[12] as usize;
        let draw_count = data[13 + count] as usize;
        let end = 14 + count + draw_count;
        let events = &data[end + 1..];
        assert_eq!(data[end] as usize, events.len());
        if op == "O" {
            assert!(events == [3] || events == [4, 2, 1]);
        } else {
            assert!(events.is_empty());
        }
        data.truncate(end);
        actual.push(data);
    }
    assert_eq!(actual.len(), expected.len());
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, b, "host native observation{i}");
    }
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_BROWSER_HOST_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("input.txt"), protocol).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("native.txt"), text).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("native.log"), output.stderr).unwrap();
    }
    eprintln!(
        "Browser host:{} exact native scope/permission/snapshot observations",
        expected.len()
    );
}
#[test]
#[ignore = "original14744/7db60/76448/1803c scope and real retained browser/capture; offscreen Metal plus the default audio output only"]
fn higurashi_browser_host_matches_native_preflight_and_early_return_scope() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored: Arc<HigurashiScenario> = assets.load_sync("/main.snr").unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let code = [
        0xa1, 0x86, 1, 0, 0, 1, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0, 0, 0,
    ];
    let scenario = scenario(&code);
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for target in [0, 1, 2] {
        for pause in [0, 1, 2] {
            for hide in [0, 1] {
                let runner = shin_tasks::block_on(HigurashiRunner::load(
                    assets.clone(),
                    scenario.clone(),
                    0,
                    42,
                    || [1; 4],
                ))
                .unwrap();
                let audio = HigurashiAudio::new(manager.clone(), assets.clone(), authored.clone());
                let mut host = HigurashiHost::new(runner, audio);
                seed(&mut host, pause, hide);
                let mut input = InputState {
                    modes: 0x12340003,
                    temporary_indicator: 1,
                };
                let source = source(target);
                let source_copy = source.clone();
                let history = MessageReadHistory::new();
                let choices = ChoiceHistory::new(IndexSet::new(), vec![]);
                let mut id = None;
                render_to_texture(&device, &queue, (1280, 720), |context, _| {
                    let mut screens = HigurashiSceneTextures::new(context);
                    if target != 0 {
                        assert!(
                            shin_tasks::block_on(host.open_message_browser(
                                &mut input, &source, &history, &choices, false, 10000, context,
                                &screens
                            ))
                            .is_err()
                        );
                        assert_eq!(
                            (host.modal.pause_depth, host.conceal.depth, input.modes),
                            (pause, hide, 0x12340003)
                        );
                        assert!(!host.has_modes() && host.browser_notifications.is_empty());
                        screens
                            .prepare_scene(context, &host.runner.frame().scene)
                            .unwrap();
                    }
                    id = host.open_message_browser_from_resources(
                        &mut input, &source, &history, &choices, false, 10000,
                        assets.load_sync("/systex.txa").unwrap(), context, &screens,
                    ).unwrap();
                });
                assert_eq!(id.is_some(), target != 0);
                assert_eq!(source.progress(), source_copy.progress());
                assert_eq!(source.persistent().0, source_copy.persistent().0);
                assert_eq!(source.bytes(), source_copy.bytes());
                let notices = host.take_browser_notifications();
                assert_eq!(notices.len(), 1);
                assert!(
                    matches!(notices[0].notice,Notice::Availability(available) if available==(target!=0))
                );
                writeln!(
                    protocol,
                    "N {target} {pause} {hide} {}\nO",
                    code.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
                .unwrap();
                expected.push(words(&mut host, &input, 0));
                host.advance_modal(Ticks::from_f32(1.0));
                protocol.push_str("U 1\n");
                expected.push(words(&mut host, &input, 0));
                let permission = host.poll_modes(Ticks::ZERO, 1).unwrap();
                protocol.push_str("P\n");
                expected.push(words(&mut host, &input, u32::from(permission)));
                if id.is_some() {
                    callback(&mut host, false);
                    protocol.push_str("C 0\n");
                    expected.push(words(&mut host, &input, 0));
                    callback(&mut host, true);
                    protocol.push_str("C 1\n");
                    expected.push(words(&mut host, &input, 0));
                }
                protocol.push_str("D\n");
                drop(host);
                owners += 1;
            }
        }
    }
    compare_native(&protocol, &expected);
    eprintln!(
        "Browser host:{owners} owners; unavailable/capture failure,nested scopes,early callback release,completion no-op,source isolation and destruction pass"
    );
}
fn input_context() -> Context {
    Context {
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
        overlay: Some(input::OverlayInput {
            menus: true,
            hide_window: true,
        }),
        hint_busy: false,
        scene_jump_available: false,
        wait_allow_interrupt: 0,
    }
}
#[test]
#[ignore = "real host input/browser/async children/closing and borrowed render snapshot on offscreen Metal plus the default audio output only"]
fn higurashi_browser_host_retains_input_children_and_releases_before_closing_finishes() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored: Arc<HigurashiScenario> = assets.load_sync("/main.snr").unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut code = Vec::new();
    for i in 0..12u32 {
        code.push(0x86);
        code.extend((i + 1).to_le_bytes()[..3].iter());
        code.extend([1, 0x7f, 4, 0, b'@', b'r', b'A', 0]);
    }
    code.extend([0, 0, 0]);
    let scenario = scenario(&code);
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario,
        0,
        42,
        || [1; 4],
    ))
    .unwrap();
    let audio = HigurashiAudio::new(manager.clone(), assets.clone(), authored);
    let mut host = HigurashiHost::new(runner, audio);
    let mut reads = ReadHistoryServices::new(MessageReadHistory::new(), |_, _| Ok(()));
    let read_policy = ReadPolicy {
        modes: 0,
        forced: false,
        read_only: false,
    };
    let status = shin_tasks::block_on(host.runner.pump_native(
        policy(),
        read_policy,
        &mut reads,
        8,
        10000,
        || [1; 4],
        &mut host.audio,
    ))
    .unwrap();
    assert_eq!(status, Status::Waiting);
    assert!(host.runner.native_read_state().unwrap().flags & 0x8000_0000 != 0);
    host.poll_modes(Ticks::ZERO, 1).unwrap();
    let mut input = InputState {
        modes: 0,
        temporary_indicator: 1,
    };
    let outcome = host.process_input(
        &mut input,
        input_context(),
        Buttons {
            pressed: 1 << 12,
            ..Default::default()
        },
    );
    assert!(outcome.requests.contains(&input::Request::MessageBrowser));
    let source = source(2);
    let choices = ChoiceHistory::new(IndexSet::new(), vec![]);
    let mut browser_id = None;
    render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut screens = HigurashiSceneTextures::new(context);
        screens
            .prepare_scene(context, &host.runner.frame().scene)
            .unwrap();
        browser_id = shin_tasks::block_on(host.open_message_browser(
            &mut input,
            &source,
            &reads.history,
            &choices,
            false,
            10000,
            context,
            &screens,
        ))
        .unwrap();
    });
    let id = browser_id.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        poll(&mut host, 0);
        if host.modes.iter().any(|m|matches!(m,HostMode::Browser(b) if b.browser.lifecycle().is_some_and(|l| l.slide()==1.0))) {break;}
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(host.modal.pause_depth, 1);
    let before = host.runner.completed_commands();
    let response = host.process_input(
        &mut input,
        input_context(),
        Buttons {
            pressed: 1 << 24,
            ..Default::default()
        },
    );
    assert!(!response.overlay_input_enabled && host.runner.completed_commands() == before);
    poll(&mut host, 1 << 12);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        poll(&mut host, 0);
        let ready = host.modes.iter().any(|m| match m {
            HostMode::Browser(b) => b
                .jobs
                .iter()
                .any(|j| matches!(j.id, ChildId::Navigation(_)) && j.task.is_none()),
            _ => false,
        });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Navigation view did not publish"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for _ in 0..20 {
        poll(&mut host, 0);
    }
    let mut frames = 0;
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            host.prepare_mode_views(context);
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "host browser modes",
            );
            pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
            host.render_modes(&mut pass);
        });
        assert!(pixels.iter().any(|b| *b != 255));
        frames += 1;
    }
    desktop_swipe::close_and_reopen(&mut host);
    // A navigation jump loads a retained question through the host's own job.
    poll(&mut host, 1 << 15);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        poll(&mut host, 0);
        let ready = host.modes.iter().any(|m| match m {
            HostMode::Browser(b) => b
                .jobs
                .iter()
                .any(|j| matches!(j.id, ChildId::Question(_)) && j.task.is_none()),
            _ => false,
        });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Question view did not publish"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for _ in 0..12 {
        poll(&mut host, 0);
    }
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = render_to_texture(&device, &queue, size, |context, target| {
            host.prepare_mode_views(context);
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "host browser question",
            );
            pass.clear(Some(UnormColor::WHITE), Some(0), Some(1.0));
            host.render_modes(&mut pass);
        });
        assert!(pixels.iter().any(|b| *b != 255));
        frames += 1;
    }
    // Cancel the question, then navigation; neither releases the browser scope.
    poll(&mut host, 1 << 25);
    for _ in 0..30 {
        poll(&mut host, 0);
    }
    assert_eq!(host.modal.pause_depth, 1);
    assert!(
        !host
            .take_browser_notifications()
            .iter()
            .any(|n| matches!(n.notice, Notice::Reconstruct { .. }))
    );
    poll(&mut host, 1 << 25);
    for _ in 0..12 {
        poll(&mut host, 0);
    }
    poll(&mut host, 1 << 25);
    let mut released = false;
    for _ in 0..30 {
        poll(&mut host, 0);
        if host.modal.pause_depth == 0 {
            assert!(host.has_modes());
            assert!(host.modes.iter().any(|m|matches!(m,HostMode::Browser(b) if b.browser.lifecycle().is_some_and(|l|l.slide()>0.0))));
            released = true;
            break;
        }
    }
    assert!(released && !host.audio.modal_effects_paused() && !host.ui.indicator.hidden);
    assert!(!host.poll_modes(Ticks::ZERO, 1).unwrap());
    for _ in 0..15 {
        poll(&mut host, 0);
    }
    assert!(!host.has_modes());
    assert!(host.poll_modes(Ticks::ZERO, 1).unwrap());
    assert!(host.set_browser_voice_playing(id, false).is_err());
    host.advance_modal(Ticks::from_f32(3.0));
    assert!(host.modal_snapshot().is_none());
    eprintln!(
        "Browser host integration:{frames}720p/1080p frames; input request,retained children,pause/input gates,early release,pruning and expired status pass"
    );
}

mod desktop_swipe;
