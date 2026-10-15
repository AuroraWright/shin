use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Default)]
struct Voice {
    status: i32,
    plays: Vec<(String, u32, i32)>,
    stops: u32,
}
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        self.status & 2 != 0
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        panic!("explicit commands must preserve their flags")
    }
    fn stop(&mut self) {
        self.stops += 1;
        self.status = 0;
    }
}
impl VoiceCommands for Voice {
    fn play_command(&mut self, name: &str, volume: f32, flags: i32) {
        self.plays.push((name.into(), volume.to_bits(), flags));
    }
    fn status_bits(&self) -> i32 {
        self.status
    }
}

#[test]
#[ignore = "executes updated VOICEPLAY/STOP/WAIT and native skip/read helper with bounded ASCII/loader contracts"]
fn higurashi_voice_commands_match_native_callbacks_and_status_masks() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut play_cases = 0;
    for profile in 0u32..64 {
        // This is the host input contract for actual137d0, not a claim that
        // ordinary input/read-state/preferences have been integrated.
        let suppressed = profile & 1 != 0
            && profile & 2 == 0
            && (profile & 4 != 0 || (profile & 8 != 0 && (profile & 16 == 0 || profile & 32 != 0)));
        for volume in [i32::MIN, -1000, -1, 0, 1, 500, 1000, i32::MAX] {
            for flags in [i32::MIN, -1, 0, 1, 2, 3, 0x7fffffff] {
                for name in ["", "S01/11/VTF_oishi_0001:long-filename"] {
                    for loader_ok in [0, 1] {
                        let hex = if name.is_empty() {
                            "-".into()
                        } else {
                            name.as_bytes()
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>()
                        };
                        input
                            .push_str(&format!("P {volume} {flags} {profile} {loader_ok} {hex}\n"));
                        let mut voice = Voice::default();
                        assert_eq!(
                            dispatch(
                                &Cmd::VOICEPLAY {
                                    name: name.into(),
                                    args: [volume, flags]
                                },
                                suppressed,
                                &mut voice
                            ),
                            Some(CommandStatus::Complete)
                        );
                        let (volume_bits, flags_bits) = match voice.plays.first() {
                            Some((actual_name, bits, flags)) => {
                                assert_eq!(actual_name, name);
                                (*bits, *flags as u32)
                            }
                            None => (0, 0),
                        };
                        expected.push(vec![
                            u32::from(suppressed),
                            1,
                            voice.plays.len() as u32,
                            volume_bits,
                            flags_bits,
                            1,
                        ]);
                        play_cases += 1;
                    }
                }
            }
        }
    }
    let states: &[&[u32]] = &[&[], &[0], &[1], &[2], &[0, 1], &[2, 1], &[1, 2], &[2, 0]];
    let masks = [i32::MIN, -1, 0, 1, 2, 3, 4, 8, 16, 31, i32::MAX];
    let mut wait_cases = 0;
    for entries in states {
        let status = if entries.contains(&0) {
            3
        } else if entries.contains(&1) {
            2
        } else {
            0
        };
        for mask in masks {
            input.push_str(&format!(
                "W {mask} {}{}\n",
                entries.len(),
                entries.iter().map(|s| format!(" {s}")).collect::<String>()
            ));
            let mut voice = Voice {
                status,
                ..Voice::default()
            };
            let actual = dispatch(&Cmd::VOICEWAIT { status: mask }, true, &mut voice).unwrap();
            assert_eq!(actual, wait(mask, &voice));
            expected.push(vec![
                status as u32,
                u32::from(actual == CommandStatus::Complete),
            ]);
            wait_cases += 1;
        }
    }
    input.push_str("S\n");
    let mut voice = Voice {
        status: 3,
        ..Voice::default()
    };
    assert_eq!(
        dispatch(&Cmd::VOICESTOP {}, true, &mut voice),
        Some(CommandStatus::Complete)
    );
    assert_eq!(wait(-1, &voice), CommandStatus::Complete);
    expected.push(vec![1, voice.stops]);
    assert_eq!(dispatch(&Cmd::AUTOSAVE {}, false, &mut voice), None);
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_VOICE_COMMANDS").unwrap())
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
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (index, (row, expected)) in text.lines().zip(expected).enumerate() {
        let actual = row
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "native command row{index}");
    }
    println!(
        "Higurashi explicit voice commands: {play_cases} native play/skip/read/flags/volume/loader-result cases, {wait_cases} status-mask cases and native stop match; actual137d0 covers64 host byte profiles; ASCII conversion/loader effects are checked boundaries"
    );
}
