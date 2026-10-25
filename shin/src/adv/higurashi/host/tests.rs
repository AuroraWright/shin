use super::super::audio::{AudioFrameServices, advance_audio};
use super::*;
use crate::audio::VoiceFocus;
use std::{
    cell::RefCell,
    io::Write,
    process::{Command, Stdio},
};

struct AudioReference {
    events: RefCell<String>,
    voice: u32,
    after: u32,
    active: bool,
    effects: VoiceFocus,
    music: VoiceFocus,
}
impl AudioFrameServices for AudioReference {
    fn voice_active(&self) -> bool {
        self.events.borrow_mut().push_str("QW");
        self.voice & 2 != 0
    }
    fn set_focus_target(&mut self, active: bool) {
        self.active = active;
    }
    fn advance_voice(&mut self, _: Ticks) {
        self.events.get_mut().push('V');
        self.voice = self.after;
    }
    fn advance_effects(&mut self, delta: Ticks, enabled: bool) {
        self.events.get_mut().push('S');
        if enabled {
            self.effects.update(delta, true, self.active);
        }
    }
    fn advance_music(&mut self, delta: Ticks) {
        self.events.get_mut().push('B');
        self.music.update(delta, true, self.active);
    }
}

#[test]
#[ignore = "executes full updated ADV and platform fragment with original empty-vector SE/BGM focus clocks"]
fn higurashi_host_matches_native_order_pause_focus_and_elapsed_clamp() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for state in 0..128 {
        let bit = |i| state & (1 << i) != 0;
        let (animate, paused, vm, scene, mode, enabled, effects_paused) =
            (bit(0), bit(1), bit(2), bit(3), bit(4), bit(5), bit(6));
        for voice in [0, 1, 2, 3, 4, 16] {
            for after in [0, 3] {
                for elapsed in [0.0f32, 0.5, 1.0, 3.0, 6.0] {
                    for rate in [0.5f32, 1.0, 2.0] {
                        let row = expected.len();
                        let gain = if row % 2 == 0 { 1.0f32 } else { 0.5 };
                        let target = if row % 3 == 0 { 0.5f32 } else { 1.0 };
                        let commands = row % 4;
                        input.push_str(&format!("U {:x} {} {} {} {} {} {} {} {commands} {voice} {after} {:x} {:x} {:x}\n",
                    elapsed.to_bits(),u8::from(animate),u8::from(paused),u8::from(vm),u8::from(scene),u8::from(mode),u8::from(enabled),u8::from(effects_paused),gain.to_bits(),target.to_bits(),rate.to_bits()));
                        let d = frame_delta(Ticks::from_f32(elapsed)).unwrap();
                        let mut audio = AudioReference {
                            events: RefCell::new(String::new()),
                            voice,
                            after,
                            active: target == 0.5,
                            effects: VoiceFocus { gain },
                            music: VoiceFocus { gain },
                        };
                        if scene {
                            audio.events.get_mut().push('I');
                            if vm && !paused {
                                for _ in 0..commands {
                                    audio.events.get_mut().push_str("NCD");
                                }
                                audio.events.get_mut().push_str("NC");
                            }
                            if animate && !paused {
                                audio.events.get_mut().push_str("AP");
                            }
                            audio.events.get_mut().push('O');
                            advance_audio(
                                &mut audio,
                                d,
                                AudioFramePolicy {
                                    focus_mode_active: mode,
                                    voice_focus_enabled: enabled,
                                    effects_update_enabled: !effects_paused,
                                    music_rate: rate,
                                },
                            );
                        } else {
                            audio.advance_music(Ticks::from_f32(d.as_f32() * rate));
                        }
                        let wait = if scene && vm && !paused {
                            12.0 - d.as_f32()
                        } else {
                            12.0f32
                        };
                        let focus = if audio.active { 0.5f32 } else { 1.0 };
                        expected.push((
                            audio.events.into_inner(),
                            [
                                d.as_f32().to_bits(),
                                wait.to_bits(),
                                audio.effects.gain.to_bits(),
                                focus.to_bits(),
                                audio.music.gain.to_bits(),
                                focus.to_bits(),
                                1.0f32.to_bits(),
                                audio.voice,
                            ],
                        ));
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_HOST").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (row, (line, (events, words))) in output.lines().zip(&expected).enumerate() {
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("U"));
        assert_eq!(fields.next(), Some(events.as_str()), "host event row{row}");
        let actual: Vec<u32> = fields
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        assert_eq!(actual.as_slice(), words, "host words row{row}");
    }
    println!(
        "Higurashi host:23040 native frames match full ADV/platform order, three-tick clamp, WAIT/pause/animation gates, pre-poll voice focus, retained targets, SE pause and BGM rate/reset"
    );
}

#[test]
#[ignore = "real retained VM/voice/SE/BGM host frame on offscreen Metal and the default audio output"]
fn higurashi_host_drives_real_command_clock_and_audio_owners() {
    use super::super::{
        effect_commands::EffectCommands, message_playback::MessageVoice,
        voice_commands::VoiceCommands,
    };
    use shin_core::{
        format::scenario::higurashi::HigurashiScenario,
        vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut raw = authored.raw().to_vec();
    // Keep actual first BGMPLAY; add WAIT, actual first SEPLAY, WAIT, EXIT.
    let mut code = vec![0x83, 1, 12];
    code.extend_from_slice(&raw[0x3028b..0x30295]);
    code.extend([0x83, 1, 12, 0, 0, 0]);
    raw[0x25465..0x25465 + code.len()].copy_from_slice(&code);
    let mut scenario = HigurashiScenario::new(raw.into()).unwrap();
    scenario.header.code_offset = 0x2545e;
    let scenario = Arc::new(scenario);
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut audio = HigurashiAudio::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        assets,
        scenario,
    );
    audio.set_preferences(100, u32::MAX, true, 70);
    audio.set_effects_volume(60);
    audio.set_paused(true);
    audio.play_command("S01/11/VTF_oishi_0001", 1.0, 1);
    assert!(audio.is_playing());
    let mut host = HigurashiHost::new(runner, audio);
    host.ui.pulse_hint();
    host.ui.indicator.hidden = true;
    host.ui.menu.indicator.hidden = true;
    let p = Policy {
        animations_enabled: true,
        paused: false,
        fast_skip: false,
        suppress_transient_audio: false,
        read_flags: 0,
        text_speed: 50,
        outline_mode: 0,
    };
    let ap = AudioFramePolicy {
        focus_mode_active: true,
        voice_focus_enabled: true,
        effects_update_enabled: true,
        music_rate: 1.0,
    };
    let update = |h: &mut HigurashiHost, p, ap, d| {
        shin_tasks::block_on(h.update(Ticks::from_f32(d), p, ap, 8, 10000, || [1, 2, 3, 4]))
            .unwrap()
    };
    for frame in 0..4 {
        assert_eq!(update(&mut host, p, ap, 120.0), Status::Waiting);
        assert_eq!(host.runner.completed_commands(), 1);
        assert!(matches!(
            host.runner.pending_command(),
            Some(Cmd::WAIT { amount: 12, .. })
        ));
        let (active, gain) = host.audio.reference_focus();
        assert!(active);
        let mut reference = VoiceFocus::default();
        for _ in 0..=frame {
            reference.update(Ticks::from_f32(3.0), true, true);
        }
        assert_eq!(gain.to_bits(), reference.gain.to_bits());
    }
    let retained = (
        host.runner.position(),
        host.runner.next_position(),
        host.runner.random_state(),
    );
    let mut paused = p;
    paused.paused = true;
    let mut held = ap;
    held.effects_update_enabled = false;
    assert_eq!(update(&mut host, paused, held, 120.0), Status::Waiting);
    assert_eq!(host.ui.hint.value(), 15.0 / 32.0);
    assert_eq!(host.ui.indicator.opacity, 0.0);
    assert_eq!(host.ui.menu.indicator.opacity, 0.0);
    assert_eq!(host.ui.menu.elapsed, 0.0);
    assert_eq!(
        retained,
        (
            host.runner.position(),
            host.runner.next_position(),
            host.runner.random_state()
        )
    );
    // WAIT completes on its next poll, then the authored SE starts. Its task
    // remains loading while native SE's own update is gated.
    assert_eq!(update(&mut host, p, held, 120.0), Status::Waiting);
    assert_eq!(host.runner.completed_commands(), 3);
    assert_eq!(host.audio.effect_status(0), 3);
    let old_focus = host.audio.reference_focus();
    let mut dormant = held;
    dormant.focus_mode_active = false;
    dormant.voice_focus_enabled = false;
    assert_eq!(update(&mut host, paused, dormant, 120.0), Status::Waiting);
    assert_eq!(host.audio.reference_focus(), old_focus);
    dormant.focus_mode_active = true;
    assert_eq!(update(&mut host, paused, dormant, 120.0), Status::Waiting);
    assert_eq!(host.audio.reference_focus(), (false, old_focus.1));
    dormant.focus_mode_active = false;
    dormant.effects_update_enabled = true;
    assert_eq!(update(&mut host, paused, dormant, 120.0), Status::Waiting);
    let mut ref_focus = VoiceFocus { gain: old_focus.1 };
    ref_focus.update(Ticks::from_f32(3.0), true, false);
    assert_eq!(host.audio.reference_focus(), (false, ref_focus.gain));
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.audio.effect_status(0) & 1 != 0 {
        assert!(
            Instant::now() < deadline,
            "SE worker did not publish during ADV pause"
        );
        update(&mut host, paused, dormant, 0.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.audio.effect_status(0), 2);
    while host.audio.status_bits() & 1 != 0 {
        assert!(
            Instant::now() < deadline,
            "voice worker did not publish during ADV pause"
        );
        update(&mut host, paused, dormant, 0.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.audio.status_bits(), 2);
    assert_eq!(host.runner.completed_commands(), 3);
    assert_eq!(host.audio.position_micros(), 0);
    // ADV pause has left the second WAIT clock untouched. Three updates
    // advance its remaining9 ticks, and its next poll reaches EXIT.
    for _ in 0..3 {
        assert_eq!(update(&mut host, p, dormant, 120.0), Status::Waiting);
    }
    assert_eq!(update(&mut host, p, dormant, 0.0), Status::Exited);
    assert_eq!(host.runner.completed_commands(), 5);
    println!(
        "Higurashi host real owners:5 VM commands, actual BGM/SE/Oishi voice; command-before-WAIT clock, elapsed clamp, ADV pause, independent SE polling gate, focus retention/recovery and paused zero clock pass"
    );
}
