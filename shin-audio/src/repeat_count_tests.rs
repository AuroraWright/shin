use super::*;
use shin_core::format::audio::AudioBuffer;

struct Pcm {
    position: u32,
    length: u32,
    end: Option<u32>,
    packet: u32,
}
impl AudioFrameSource for Pcm {
    fn max_frame_size(&self) -> usize {
        self.packet as usize
    }
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn pre_skip(&self) -> u32 {
        0
    }
    fn pre_roll(&self) -> u32 {
        0
    }
    fn loop_end(&self) -> Option<u32> {
        self.end
    }
    fn current_sample_position(&self) -> u32 {
        self.position
    }
    fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
        self.position = position;
        Ok(0)
    }
    fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
        let end = (self.position + self.packet).min(self.length);
        for i in self.position..end {
            let value = (i + 1) as f32 / 32768.0;
            buffer.push((value, -value));
        }
        let read = self.position < end;
        self.position = end;
        read
    }
}

fn provider(
    start: u32,
    end: Option<u32>,
    length: u32,
    packet: u32,
    count: u32,
) -> SampleProvider<Pcm> {
    SampleProvider::with_loop_count(
        Pcm {
            position: 0,
            length,
            end,
            packet,
        },
        Some(start),
        count,
    )
}

fn read(provider: &mut SampleProvider<Pcm>) -> i32 {
    provider.push_frame_to_resampler();
    let frame = provider.resampler.latest_frame();
    assert_eq!(frame.right, -frame.left);
    (frame.left * 32768.0) as i32
}

#[test]
fn finite_repeats_include_the_final_tail_and_fallback_eof() {
    for count in 1..=4 {
        for end in [Some(19), None] {
            let mut p = provider(7, end, 29, 8, count);
            let boundary = end.unwrap_or(29);
            let mut expected: Vec<i32> = (1..=boundary as i32).collect();
            for _ in 1..count {
                expected.extend(8..=boundary as i32);
            }
            expected.extend(boundary as i32 + 1..=29);
            for value in expected {
                assert_eq!(read(&mut p), value);
                assert!(!p.reached_eof);
            }
            assert_eq!(read(&mut p), 0);
            assert!(p.reached_eof);
        }
    }
}

#[test]
fn finite_repeat_settings_survive_pause_and_volume_retarget_then_clear_waits() {
    use crate::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use kira::{
            AudioManager, AudioManagerSettings};
    use shin_core::{
        time::Tween,
        vm::command::types::{AudioWaitStatus, Pan, Volume},
    };
    let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
        backend_settings: MockBackendSettings { sample_rate: 48000 },
        internal_buffer_size: 1,
        ..Default::default()
    })
    .unwrap();
    let mut handle = manager
        .play(AudioData {
            source: Pcm {
                position: 0,
                length: 73,
                end: Some(37),
                packet: 24,
            },
            settings: crate::AudioSettings {
                paused: true,
                playback_rate: 1.0,
                fade_in: Tween::IMMEDIATE,
                loop_start: Some(13),
                loop_count: 3,
                volume: Volume::default(),
                pan: Pan::default(),
            },
        })
        .unwrap();
    fn advance(manager: &mut AudioManager<MockBackend>, frames: usize) {
        for _ in 0..frames {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
    }
    advance(&mut manager, 100);
    assert_eq!(handle.position_samples(), 0);
    assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
    handle.set_paused(false);
    advance(&mut manager, 41); // Into the second pass of the loop section.
    handle.set_paused(true);
    manager.backend_mut().on_start_processing();
    let position = handle.position_samples();
    advance(&mut manager, 100);
    assert_eq!(handle.position_samples(), position);
    handle.set_volume(Volume(0.5), Tween::IMMEDIATE).unwrap();
    handle.set_paused(false);
    // Total playback is73 +2*(37-13) =121 samples, then the resampler window drains.
    advance(&mut manager, 110);
    assert!(handle.get_wait_status().is_empty());
    assert_eq!(handle.position_samples(), 73);
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[ignore = "requires original/1.03 supplied executable and AArch64 native execution"]
fn reference_native_audio_repeat_worker() {
    use std::{
        io::Write,
        path::Path,
        process::{Command, Stdio},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let binary = std::env::temp_dir().join(format!("shin-native-repeat-{}", std::process::id()));
    let status = Command::new("clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-audio-repeat-count.c"))
        .arg("-o")
        .arg(&binary)
        .status()
        .unwrap();
    assert!(status.success());
    let mut cases = Vec::new();
    let mut input = String::new();
    for count in [0, 1, 2, 3, 4, 17, 65535, u32::MAX] {
        for (start, end, length) in [
            (0, 1, 1),
            (0, 1, 7),
            (1, 3, 7),
            (7, 19, 29),
            (0, 29, 29),
            (13, 37, 73),
        ] {
            for packet in [1, 2, 8, 16, 24] {
                cases.push((count, start, end, length, packet));
                input.push_str(&format!("{count} {start} {end} {length} {packet} 128\n"));
            }
        }
    }
    for (name, shift) in [
        ("uncompressed_executable", 0),
        ("uncompressed_executable_update", 0x730),
    ] {
        let exe = root.join("game-data/umineko").join(name);
        let mut child = Command::new(&binary)
            .arg(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                stdin.write_all(input.as_bytes()).unwrap();
                drop(stdin);
            });
            child.wait_with_output().unwrap()
        });
        assert!(result.status.success(), "{name} native worker failed");
        let output = String::from_utf8(result.stdout).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), cases.len());
        let mut compared = 0;
        for ((count, start, end, length, packet), line) in cases.iter().copied().zip(lines) {
            let words: Vec<i32> = line
                .split_whitespace()
                .map(|word| word.parse().unwrap())
                .collect();
            assert_eq!(words[1], 0, "{name}: native error flag");
            assert_eq!(words[3] as usize, words.len() - 4);
            let mut p = provider(start, Some(end), length, packet, count);
            for (index, expected) in words[4..].iter().enumerate() {
                assert_eq!(
                    read(&mut p),
                    *expected,
                    "{name} count{count} loop{start}..{end} length{length} packet{packet} sample{index}"
                );
                assert!(!p.reached_eof);
                compared += 1;
            }
            if words[0] != 0 {
                assert_eq!(read(&mut p), 0);
                assert!(p.reached_eof, "{name} final tail did not end playback");
            }
        }
        let original = std::fs::read(&exe).unwrap();
        let mutant = binary.with_extension("mutant");
        for offset in [
            0xf459c, 0xf45a0, 0xf45a4, 0xf45a8, 0xf45dc, 0xf45e0, 0xf45e4, 0xf4604,
        ] {
            let mut bytes = original.clone();
            bytes[256 + offset + shift] ^= 1;
            std::fs::write(&mutant, &bytes).unwrap();
            let result = Command::new(&binary).arg(&mutant).output().unwrap();
            assert_eq!(result.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&result.stderr)
                    .contains("Unreviewed exact repeat-count image")
            );
        }
        std::fs::remove_file(mutant).unwrap();
        eprintln!(
            "{name}: {} unchanged native worker cases, {compared} exact stereo sample pairs and final EOF comparisons; 8 image mutants rejected",
            cases.len()
        );
    }
    std::fs::remove_file(binary).unwrap();
}
