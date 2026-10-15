use super::*;
use crate::adv::{
    higurashi::{
        audio::HigurashiAudio,
        frame::tests::gpu::render_to_texture,
        input::Buttons,
        message_glyphs::tests::gpu::gpu_assets,
        read_history::{MessageReadHistory, ReadHistoryServices},
        read_state::ReadPolicy,
        replay::tests::scenario,
        runner::{HigurashiRunner, Policy},
        scene_textures::HigurashiSceneTextures,
    },
    input_area::PointerFrame,
};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(in crate::adv::higurashi) fn hold_reload(host: &mut HigurashiHost) -> futures::channel::oneshot::Sender<()> {
    let mode = host.modes.iter_mut().find_map(|m| match m { HostMode::LayerReload(task)=>Some(task), _=>None }).unwrap();
    let worker = mode.worker.take().unwrap();
    let (sender, receiver) = futures::channel::oneshot::channel();
    mode.worker = Some(shin_tasks::async_io::spawn_loader(async move {
        receiver.await.context("Test reload gate cancelled")?;
        worker.await
    }));
    sender
}

#[test]
#[ignore = "executes original c4990/c4350/eadd0 title codec over all valid custom Shift-JIS code units"]
fn higurashi_system_menu_saveinfo_title_matches_native_codec_without_message_fixup() {
    use binrw::BinRead;
    use shin_core::format::{
        scenario::higurashi::{HigurashiVariant, bytecode::HigurashiCommand},
        text::read_sjis_string,
    };
    let mut cases = vec![Vec::new()];
    for first in 1..=255u8 {
        let bytes = vec![first];
        if read_sjis_string(&mut std::io::Cursor::new(&bytes), Some(bytes.len())).is_ok() {
            cases.push(bytes);
        }
    }
    for first in (0x81..=0x9f).chain(0xe0..=0xfc) {
        for second in (0x40..=0x7e).chain(0x80..=0xfc) {
            cases.push(vec![first, second]);
        }
    }
    for count in [10, 11, 12, 22, 23, 24, 127] {
        cases.push([0x82, 0xa0].repeat(count));
    }
    cases.extend([
        b"@c123.A@rB".to_vec(),
        vec![0xa0, 0xa1, 0xa2, 0xb1, 0xde, 0xdf],
        vec![b'A'; 254],
    ]);
    let mut protocol = String::new();
    let mut expected = Vec::new();
    for bytes in &cases {
        protocol += &if bytes.is_empty() {
            "-".to_owned()
        } else {
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        protocol.push('\n');
        let mut command = vec![0xa0, 1, (bytes.len() + 1) as u8];
        command.extend(bytes);
        command.push(0);
        let HigurashiCommand::SAVEINFO { info, .. } = HigurashiCommand::read_args(
            &mut std::io::Cursor::new(command),
            (HigurashiVariant::Updated,),
        )
        .unwrap() else {
            panic!("Expected SAVEINFO")
        };
        expected.push(info.0);
    }
    let mut native = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MENU_TITLE").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = native.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = native.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (i, (line, text)) in output.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("S"));
        assert_eq!(fields.next().unwrap().parse::<usize>().unwrap(), text.len());
        let bytes: Vec<u8> = fields.map(|f| f.parse().unwrap()).collect();
        assert_eq!(
            bytes,
            text.as_bytes(),
            "title codec case{i}: {:x?}",
            cases[i]
        );
    }
    println!(
        "Higurashi SystemMenu SAVEINFO title:{} exact native custom-SJIS cases including all valid code units, half-width kana and short/heap string boundaries",
        expected.len()
    );
}

struct Trace {
    model: Option<(i32, usize, u32, Vec<u32>, String)>,
    modes: Vec<u32>,
}
impl Trace {
    fn capture(host: &mut HigurashiHost) -> Self {
        let model = host.modes.iter().find_map(|mode| {
            let HostMode::Menu(owner) = mode else {
                return None;
            };
            let owner = owner.0.lock();
            let model = &owner.model;
            let mut words = Vec::new();
            for row in model.rows() {
                words.extend([
                    row.item,
                    row.slide.value().to_bits(),
                    u32::from(row.slide.is_waiting()),
                    row.flash.value().to_bits(),
                    u32::from(row.flash.is_waiting()),
                ]);
            }
            words.extend([
                model.detail_slide().to_bits(),
                model.detail_fade().to_bits(),
            ]);
            Some((
                model.phase(),
                model.selected(),
                model.completion_word(),
                words,
                model.play_time().to_owned(),
            ))
        });
        let modes = host.modes.iter_mut().map(completion).collect();
        Self { model, modes }
    }
    fn check(&self, native: &str, observation: usize) {
        let mut values = native.split_whitespace();
        let tag = values.next().unwrap();
        if self.model.is_none() {
            assert_eq!(tag, "Z", "observation{observation}");
            assert_eq!(values.next(), Some("1"));
            assert_eq!(
                values.next().unwrap().parse::<usize>().unwrap(),
                self.modes.len()
            );
            return;
        }
        assert_eq!(tag, "M");
        values.next(); // Protocol operation.
        assert_eq!(values.next(), Some("0")); // Menu consumes permission.
        let (phase, selected, completed, words, text) = self.model.as_ref().unwrap();
        assert_eq!(
            values.next().unwrap().parse::<i32>().unwrap(),
            *phase,
            "phase observation{observation}"
        );
        assert_eq!(values.next().unwrap().parse::<usize>().unwrap(), *selected);
        assert_eq!(values.next(), Some("72"));
        assert_eq!(values.next(), Some("-1")); // These traces have no pointer contact.
        for _ in 0..3 {
            values.next();
        } // Extended/initial setting/prompt, checked by host scope assertions.
        assert_eq!(values.next().unwrap().parse::<u32>().unwrap(), *completed);
        assert_eq!(
            values.next().unwrap().parse::<usize>().unwrap(),
            (words.len() - 2) / 5
        );
        let mut actual: Vec<u32> = (0..words.len() - 2)
            .map(|_| values.next().unwrap().parse().unwrap())
            .collect();
        for _ in 0..2 {
            actual.push(values.next().unwrap().parse().unwrap());
            values.next();
        }
        assert_eq!(&actual, words, "clock observation{observation}");
        let count = values.next().unwrap().parse::<usize>().unwrap();
        let bytes: Vec<u8> = (0..count)
            .map(|_| values.next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(bytes, text.as_bytes());
        let effects = values.next().unwrap().parse::<usize>().unwrap();
        for _ in 0..effects {
            values.next();
        } // Prior native row/input comparison covers cue events.
        assert_eq!(
            values.next().unwrap().parse::<usize>().unwrap(),
            self.modes.len()
        );
        let actual: Vec<u32> = (0..self.modes.len())
            .map(|_| values.next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(
            actual, self.modes,
            "mode ownership observation{observation}"
        );
        assert_eq!(values.next(), Some("1"));
        assert!(values.next().is_none());
    }
}

#[test]
#[ignore = "original combined AArch64 mode/menu/task dispatch, real cached capture/view and exact Loopback Audio"]
fn higurashi_host_system_menu_owns_modal_tasks_children_and_global_cues() {
    let (device, queue, assets) = gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
    sounds.lock().set_settings_volume(37);
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut rendered = 0;
    for size in [(1280, 720), (1920, 1080)] {
        // Each reached0..4 constructor boundary; the real reloader has its own fixture.
        for selection in 0..5 {
            let id = if selection == 5 { 11001 } else { 0 };
            let code = [
                0xa0, 0, 2, b'U', 0, 0xa0, 1, 2, b'T', 0, 0xa1, 0x86, 1, 0, 0, 1, 0x7f, 2, 0, b'A',
                0, 0, 0, 0,
            ];
            let runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                scenario(&code),
                0,
                42,
                || [1; 4],
            ))
            .unwrap();
            let audio = HigurashiAudio::new(manager.clone(), assets.clone(), authored.clone());
            let mut host = HigurashiHost::new(runner, audio);
            let mut reads = ReadHistoryServices::new(MessageReadHistory::new(), |_, _| Ok(()));
            let policy = Policy {
                animations_enabled: true,
                paused: false,
                fast_skip: false,
                suppress_transient_audio: false,
                read_flags: 0,
                text_speed: 100,
                outline_mode: 0,
            };
            let read_policy = ReadPolicy {
                modes: 0,
                forced: false,
                read_only: false,
            };
            shin_tasks::block_on(host.runner.pump_native(
                policy,
                read_policy,
                &mut reads,
                3,
                1000,
                || [1; 4],
                &mut host.audio,
            ))
            .unwrap();
            assert_eq!(
                host.runner.native_save_metadata().unwrap().saved()[1].as_ref(),
                "T"
            );
            host.set_menu_frame(0, 3600.0);
            render_to_texture(&device, &queue, size, |context, _| {
                let mut screens = HigurashiSceneTextures::new(context);
                assert!(
                    shin_tasks::block_on(host.open_system_menu(
                        id,
                        sounds.clone(),
                        context,
                        &screens
                    ))
                    .is_err()
                );
                assert_eq!(host.modal_pause_depth(), 0);
                assert!(!host.has_modes());
                assert!(host.system_sounds.is_none());
                screens
                    .prepare_scene(context, &host.runner.frame().scene)
                    .unwrap();
                shin_tasks::block_on(host.open_system_menu(id, sounds.clone(), context, &screens))
                    .unwrap();
            });
            assert_eq!(host.modal_pause_depth(), 1);
            assert_eq!(host.conceal.depth, 1);
            assert!(host.audio.modal_effects_paused());
            assert!(host.ui.indicator.hidden);
            assert_eq!(sounds.lock().volume_percent, 37);
            assert_eq!(host.modes.len(), 1);
            if selection == 0 && size == (1280, 720) {
                sounds.lock().play(&assets, "decide", Volume(1.0));
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while sounds.lock().sounds.last().unwrap().position_samples() == 0 {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Global UI cue stalled during ADV modal pause"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert_eq!(host.modal_pause_depth(), 1);
                assert!(host.audio.modal_effects_paused());
            }
            protocol += &format!("N {id} 0\n");
            expected.push(Trace::capture(&mut host));
            let before = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "menu before borrowed snapshot",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                host.render_modes(&mut pass);
            });
            assert!(before.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
            let mut frame_number = 0;
            let mut poll =
                |host: &mut HigurashiHost, delta: f32, pressed: u32, repeat: u32, setting: u8| {
                    frame_number += 1;
                    host.set_menu_frame(setting, 3600.0);
                    let pointer = PointerFrame {
                        number: frame_number,
                        time_seconds: 0.0,
                        events: &[],
                    };
                    let permission = host
                        .poll_modes_with_input(
                            Ticks::from_f32(delta),
                            1,
                            Buttons {
                                pressed,
                                repeat,
                                ..Default::default()
                            },
                            &pointer,
                        )
                        .unwrap();
                    assert_eq!(permission, !host.has_modes());
                    protocol += &format!(
                        "F {frame_number} 0 1 {pressed:x} {repeat:x} {setting} {:x} 0 {:x}\n",
                        3600f64.to_bits(),
                        delta.to_bits()
                    );
                    expected.push(Trace::capture(host));
                };
            for _ in 0..8 {
                poll(&mut host, 3.0, 0, 0, 0);
            }
            let opened = render_to_texture(&device, &queue, size, |context, target| {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "menu borrowed view",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                host.render_modes(&mut pass);
            });
            assert!(opened.chunks_exact(4).any(|pixel| pixel != [0, 0, 0, 255]));
            rendered += 2;
            if selection < 5 {
                for _ in 0..selection {
                    poll(&mut host, 0.0, 0, 1 << 6, 0);
                }
                poll(&mut host, 1.0, 1 << 24, 0, 0);
                assert_eq!(host.modes.len(), 2);
                assert!(host.menu_requests.is_empty()); // Appended task did not poll yet.
                for _ in 0..16 {
                    poll(&mut host, 3.0, 0, 0, 0);
                }
                let requests = host.take_menu_child_requests();
                assert_eq!(requests.len(), 1);
                assert_eq!(
                    requests[0].kind,
                    ChildKind::Item([0, 1, 2, 4, 3][selection])
                );
                assert_eq!(host.modes.len(), 2);
                assert_eq!(host.modal_pause_depth(), 1);
                // Notification consumption retains the child and its menu.
                poll(&mut host, 3.0, 1 << 15, 0, 0);
                assert_eq!(host.modal_pause_depth(), 1);
                drop(poll);
                host.return_from_menu_child(requests[0].id).unwrap();
                assert!(host.return_from_menu_child(requests[0].id).is_err());
                protocol += "R\n";
                expected.push(Trace::capture(&mut host));
                for n in 0..8 {
                    frame_number += 1;
                    host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
                    protocol += &format!(
                        "F {frame_number} 0 1 0 0 0 {:x} 0 40400000\n",
                        3600f64.to_bits()
                    );
                    expected.push(Trace::capture(&mut host));
                    if n == 0 {
                        assert_eq!(host.modes.len(), 1);
                    }
                }
                frame_number += 1;
                host.poll_modes_with_input(
                    Ticks::ZERO,
                    1,
                    Buttons {
                        pressed: 1 << 15,
                        ..Default::default()
                    },
                    &PointerFrame {
                        number: frame_number,
                        time_seconds: 0.0,
                        events: &[],
                    },
                )
                .unwrap();
                protocol += &format!(
                    "F {frame_number} 0 1 8000 0 0 {:x} 0 0\n",
                    3600f64.to_bits()
                );
                expected.push(Trace::capture(&mut host));
            }
            for _ in 0..10 {
                frame_number += 1;
                host.poll_modes(Ticks::from_f32(3.0), 1).unwrap();
                protocol += &format!(
                    "F {frame_number} 0 1 0 0 {} {:x} 0 40400000\n",
                    u8::from(selection == 5),
                    3600f64.to_bits()
                );
                expected.push(Trace::capture(&mut host));
            }
            assert!(!host.has_modes());
            assert_eq!(host.modal_pause_depth(), 0);
            assert_eq!(host.conceal.depth, 0);
            assert!(!host.audio.modal_effects_paused());
            assert!(!host.ui.indicator.hidden);
            // Fade/snapshot retirement belongs to the host clock after exit.
            assert!(host.modal_snapshot().is_some());
            host.advance_modal(Ticks::from_f32(10.0));
            assert!(host.modal_snapshot().is_none());
            protocol += "D\n";
        }
    }
    std::fs::write("/tmp/higurashi-menu-host-protocol.txt", &protocol).unwrap();
    let mut native = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MENU_HOST").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = native.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(protocol.as_bytes()).unwrap());
    let output = native.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "status:{}; stderr:{}; last output:{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
        ,String::from_utf8_lossy(&output.stdout).lines().rev().take(3).collect::<Vec<_>>().join("\n")
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (i, (line, trace)) in output.lines().zip(&expected).enumerate() {
        trace.check(line, i);
    }
    println!(
        "Higurashi host SystemMenu:10 native combined timelines/{} exact model+mode observations;{rendered} view frames at720p/1080p; atomic failed capture, counted scope, deferred task/child, retained return callbacks pass",
        expected.len()
    );
    assert!(
        sounds
            .lock()
            .bank
            .as_ref()
            .unwrap()
            .sounds
            .contains_key("decide")
    );
    // The application-owned player survives all twelve ADV owners.
    sounds.lock().play(&assets, "decide", Volume(1.0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while sounds.lock().sounds.last().unwrap().position_samples() == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "Global UI cue did not advance on Loopback Audio"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!(
        "Higurashi global SYSSE: supplied move/cancel/decide bank reused; cue source advances on exact Loopback Audio after host retirement"
    );
}
