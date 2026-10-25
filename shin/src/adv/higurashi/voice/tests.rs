use super::*;
use crate::audio::{
    character_voice_enabled, native_voice_clock, voice_pattern_matches, voice_speakers,
};
use shin_core::format::{scenario::types::U8SmallList, text::U16String};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn scenario() -> Arc<HigurashiScenario> {
    Arc::new(
        HigurashiScenario::new(
            std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
                .unwrap()
                .into(),
        )
        .unwrap(),
    )
}
fn hex(text: &str) -> String {
    assert!(
        text.is_ascii(),
        "This voice-path matrix uses supplied ASCII identifiers"
    );
    if text.is_empty() {
        return "-".into();
    }
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

struct ClockFeedback {
    playing: bool,
    clock: u64,
}
impl MessageVoice for ClockFeedback {
    fn is_playing(&self) -> bool {
        self.playing
    }
    fn position_micros(&self) -> u64 {
        self.clock
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        unreachable!()
    }
    fn stop(&mut self) {
        unreachable!()
    }
}

#[test]
#[ignore = "executes updated native speaker lookup/muting and actual sample/voice clocks"]
fn higurashi_voice_matches_native_mappings_and_clocks() {
    let scenario = scenario();
    let supplied = mappings(&scenario);
    assert!(!supplied.is_empty());
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut mapping_cases = 0;
    let mut compare = |mappings: &VoiceMappingInfo, name: &str, mask: u32, character: i32| {
        use std::fmt::Write as _;
        write!(
            input,
            "M {mask} {character} {} {}",
            hex(name),
            mappings.len()
        )
        .unwrap();
        for mapping in mappings {
            write!(
                input,
                " {} {}",
                hex(mapping.name_pattern.as_str()),
                mapping.lipsync_character_ids.0.len()
            )
            .unwrap();
            for id in &mapping.lipsync_character_ids.0 {
                write!(input, " {id}").unwrap();
            }
        }
        input.push('\n');
        let index = mappings
            .iter()
            .position(|m| voice_pattern_matches(m.name_pattern.as_str(), name));
        let speakers = voice_speakers(mappings, name, VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC);
        let speaker = u8::try_from(character).is_ok_and(|id| speakers.contains(&id));
        expected.push(format!(
            "M {} {} {}",
            index.map_or(-1, |i| i as i32),
            u8::from(character_voice_enabled(mappings, name, mask)),
            u8::from(speaker)
        ));
        mapping_cases += 1;
    };
    for name in [
        "S01/11/VTF_oishi_0001",
        "s01/11/vtf_oishi_0002",
        "no-such-character",
    ] {
        for mask in [0, 1, 1 << 31, u32::MAX] {
            for character in [-1, 0, 2, 31, 255, 256] {
                compare(&supplied, name, mask, character);
            }
        }
    }
    for mapping in &supplied {
        let name = mapping
            .name_pattern
            .as_str()
            .replace('*', "abc")
            .replace('?', "q");
        for (mask, character) in [(0, -1), (u32::MAX, 0), (1 << 2, 2)] {
            compare(&supplied, &name, mask, character);
        }
    }
    for (pattern, name) in [
        ("a**?c", "AbC"),
        ("*a", "ab"),
        ("a?", "abc"),
        ("?", ""),
        ("*", ""),
        ("a*b*c", "AXBYC"),
    ] {
        let mapping = |pattern, ids| VoiceMappingInfoItem {
            name_pattern: U16String::new(pattern),
            lipsync_character_ids: U8SmallList::from_contents(ids),
        };
        let list = vec![mapping(pattern, vec![2, 34, 255]), mapping("*", vec![1])];
        for mask in [0, 1, 4, u32::MAX] {
            for character in [-1, 1, 2, 34, 255, 256] {
                compare(&list, name, mask, character);
            }
        }
    }
    let mut clock_cases = 0;
    for rate in [8000, 11025, 22050, 44100, 48000, 96000] {
        for samples in [0, 1, 47, 48, 441, 480, 48000, 1000001, u32::MAX as u64] {
            for origin in [0, 123000, u64::from(u32::MAX) * 1000] {
                for states in [
                    &[][..],
                    &[0],
                    &[1],
                    &[2],
                    &[2, 1],
                    &[2, 0],
                    &[1, 0],
                    &[0, 1],
                ] {
                    use std::fmt::Write as _;
                    write!(input, "R {rate} {samples} {origin} {}", states.len()).unwrap();
                    for state in states {
                        write!(input, " {state}").unwrap();
                    }
                    input.push('\n');
                    let status = if states.contains(&0) {
                        3
                    } else if states.contains(&1) {
                        2
                    } else {
                        0
                    };
                    let active = states.iter().copied().find(|&state| state != 2);
                    let clock = match active {
                        Some(0) => origin,
                        Some(1) => origin + native_voice_clock(samples, rate),
                        _ => 0,
                    };
                    let locks = match active {
                        Some(0) => 2,
                        Some(1) => 4,
                        _ => 0,
                    };
                    let mut message =
                        super::super::message_playback::HigurashiMessagePlayback::default();
                    message.total_voices = 1;
                    let voice = ClockFeedback {
                        playing: status & 2 != 0,
                        clock,
                    };
                    let waits = [0, 1, 20, -1]
                        .map(|target| u8::from(message.voice_waiting(0, target, &voice)));
                    expected.push(format!(
                        "R {status} {clock} {locks} {} {} {} {}",
                        waits[0], waits[1], waits[2], waits[3]
                    ));
                    clock_cases += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_VOICE").unwrap())
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), expected.len());
    for (index, (native, expected)) in stdout.lines().zip(expected).enumerate() {
        assert_eq!(native, expected, "voice row{index}");
    }
    println!(
        "Higurashi voice:{mapping_cases} native speaker mapping/mask cases and{clock_cases} actual sample/voice clock/status cases with four MSGSYNC targets match;{} supplied mapping rows; no audio hardware",
        supplied.len()
    );
}

fn loaded(voice: &mut HigurashiVoicePlayer) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while voice.wait_status().contains(AudioWaitStatus::LOADING) {
        assert!(Instant::now() < deadline, "voice load timed out");
        voice.update(Ticks::ZERO);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires supplied Higurashi assets, offscreen Metal and the default audio output"]
fn higurashi_voice_transport_plays_authored_nxa_and_releases_waits() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let scenario = scenario();
    let manager = Arc::new(AudioManager::new_for_tests());
    let mut voice = HigurashiVoicePlayer::new(manager, assets, &scenario);
    let first = "S01/11/VTF_oishi_0001";
    let speakers = voice_speakers(
        &voice.mappings,
        first,
        VoicePlayFlags::ENABLE_CHARACTER_LIPSYNC,
    );
    assert!(!speakers.is_empty(), "authored voice must exercise mapping");
    voice.set_preferences(100, u32::MAX, true);
    voice.set_paused(true);
    assert!(voice.play(first, 0.8, true, 10, 100));
    assert_eq!(
        voice.wait_status(),
        AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING
    );
    assert_eq!(voice.position_micros(), 10000);
    loaded(&mut voice);
    assert!(voice.is_playing());
    assert_eq!(voice.position_micros(), 10000);
    assert_eq!(
        voice.player.borrow().reference_voice_metadata()[0].0,
        speakers
    );
    assert!(
        voice
            .character_lipsync(i32::from(speakers[0]), -0.5)
            .is_some()
    );
    let mut message = super::super::message_playback::HigurashiMessagePlayback::default();
    message.total_voices = 1;
    message.voice_counter = 0;
    assert!(message.voice_waiting(0, 1, &voice));
    voice.set_paused(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut advanced = false;
    while voice.is_playing() {
        assert!(Instant::now() < deadline, "voice playback did not finish");
        advanced |= voice.position_micros() > 10000;
        voice.update(Ticks::from_f32(1.0));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(advanced);
    assert!(!message.voice_waiting(0, 1, &voice));
    assert_eq!(voice.position_micros(), 0);
    assert!(
        voice
            .character_lipsync(i32::from(speakers[0]), 0.0)
            .is_none()
    );
    // Loading cancellation and replacement cannot resurrect the old request.
    voice.set_paused(true);
    assert!(voice.play(first, 1.0, true, 0, 100));
    assert!(voice.play("S01/11/VTF_oishi_0002", 0.5, true, 20, 100));
    loaded(&mut voice);
    assert!(voice.is_playing());
    assert_eq!(voice.position_micros(), 20000);
    voice.stop();
    assert!(!voice.is_playing());
    assert_eq!(voice.position_micros(), 0);
    assert!(
        voice
            .character_lipsync(i32::from(speakers[0]), 0.0)
            .is_none()
    );
    voice.update(Ticks::from_f32(10.0));
    voice.set_preferences(100, 0, true);
    assert!(!voice.play(first, 1.0, true, 0, 0));
    assert!(voice.wait_status().is_empty());
    // Unknown/missing paths are allowed by native mapping, then fail loading
    // and release playback waits without opening another audio device.
    assert!(voice.play("missing-file", 1.0, false, 0, 0));
    loaded(&mut voice);
    assert!(voice.wait_status().is_empty());
    println!(
        "Higurashi voice transport:the default audio output; authored0001/0002 NXA async load/segment/pause/clock/lipsync, EOF/stop waits, canceled replacement, character muting and failed-load retirement pass"
    );
}
