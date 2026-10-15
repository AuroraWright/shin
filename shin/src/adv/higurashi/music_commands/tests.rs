use super::*;
use crate::audio::native_voice_clock;
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[derive(Default)]
struct Music {
    calls: Vec<Vec<u32>>,
    status: i32,
    clock: u64,
}
impl MusicCommands for Music {
    fn play_bgm(&mut self, id: i32, fade: i32, count: u32, volume: f32) -> Result<()> {
        self.calls
            .push(vec![1, 1, id as u32, fade as u32, count, volume.to_bits()]);
        Ok(())
    }
    fn stop_bgm(&mut self, fade: i32) {
        self.calls.push(vec![1, 1, fade as u32, 0, 1]);
    }
    fn volume_bgm(&mut self, volume: f32, fade: i32) -> Result<()> {
        self.calls
            .push(vec![1, 1, fade as u32, volume.to_bits(), 0]);
        Ok(())
    }
    fn bgm_status(&self) -> i32 {
        self.status
    }
    fn bgm_clock(&self) -> u64 {
        self.clock
    }
}
#[test]
#[ignore = "runs updated native BGM callbacks with bounded service contracts and native media sample clocks"]
fn higurashi_music_commands_match_native_callbacks_clocks_and_waits() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let volumes = [i32::MIN, -1000, -1, 0, 1, 500, 1000, i32::MAX];
    let fades = [i32::MIN, -1, 0, 1, 4, 60, i32::MAX];
    let counts = [i32::MIN, -1, 0, 1, 2, 3, i32::MAX];
    for id in [-1, 0, 253] {
        for fade in fades {
            for count in counts {
                for volume in volumes {
                    for skip in [false, true] {
                        for changed in [0, 1] {
                            input.push_str(&format!(
                                "P {id} {fade} {count} {volume} {} {changed}\n",
                                u8::from(skip)
                            ));
                            let mut music = Music::default();
                            assert_eq!(
                                dispatch(
                                    &Cmd::BGMPLAY {
                                        args: [id, fade, count, volume]
                                    },
                                    skip,
                                    &mut music
                                )
                                .unwrap(),
                                Some(MusicCommand::Complete)
                            );
                            let mut row = music.calls.remove(0);
                            row.push(changed);
                            expected.push(row);
                        }
                    }
                }
            }
        }
    }
    let play_cases = expected.len();
    for volume in volumes {
        for fade in fades {
            for skip in [false, true] {
                for (op, command) in [
                    ('S', Cmd::BGMSTOP { fade }),
                    (
                        'V',
                        Cmd::BGMVOL {
                            args: [volume, fade],
                        },
                    ),
                ] {
                    input.push_str(&format!("{op} {volume} {fade} {}\n", u8::from(skip)));
                    let mut music = Music::default();
                    assert_eq!(
                        dispatch(&command, skip, &mut music).unwrap(),
                        Some(MusicCommand::Complete)
                    );
                    expected.push(music.calls.remove(0));
                }
            }
        }
    }
    let setters = expected.len() - play_cases;
    let statuses = [0, 1, 2, 3, 4, 7, 31, i32::MIN];
    for mask in [i32::MIN, -1, 0, 1, 2, 3, 4, 8, 16, 31, i32::MAX] {
        for status in statuses {
            input.push_str(&format!("W {mask} {} 0\n", status as u32));
            let mut music = Music {
                status,
                ..Music::default()
            };
            let Some(MusicCommand::Wait(wait)) =
                dispatch(&Cmd::BGMWAIT { status: mask }, true, &mut music).unwrap()
            else {
                panic!()
            };
            expected.push(vec![u32::from(wait.complete(&music))]);
        }
    }
    let mut sync_cases = 0;
    for time in [i32::MIN, -61, -1, 0, 1, 60, 1647, i32::MAX] {
        let target = sync_target(time);
        for clock in [
            0,
            target.saturating_sub(1),
            target,
            target + 1,
            16_665,
            u64::MAX,
        ] {
            for status in statuses {
                input.push_str(&format!("Y {time} {} {clock}\n", status as u32));
                let mut music = Music {
                    status,
                    clock,
                    ..Music::default()
                };
                let Some(MusicCommand::Wait(wait)) =
                    dispatch(&Cmd::BGMSYNC { time }, true, &mut music).unwrap()
                else {
                    panic!()
                };
                expected.push(vec![u32::from(wait.complete(&music))]);
                sync_cases += 1;
            }
        }
    }
    assert_eq!(
        dispatch(&Cmd::AUTOSAVE {}, false, &mut Music::default()).unwrap(),
        None
    );
    let mut clock_cases = 0;
    let states: &[&[u32]] = &[&[], &[0], &[1], &[2], &[0, 1], &[1, 0], &[2, 1], &[2, 0]];
    for rate in [8000, 22050, 44100, 48000, 96000] {
        for samples in [
            0u64,
            1,
            47,
            48,
            735,
            799,
            800,
            14464,
            48000,
            u32::MAX as u64,
        ] {
            for entries in states {
                for busy in [0, 1] {
                    input.push_str(&format!(
                        "R {rate} {samples} {busy} {}{}\n",
                        entries.len(),
                        entries.iter().map(|s| format!(" {s}")).collect::<String>()
                    ));
                    let first = entries.iter().find(|&&s| s <= 1);
                    let status = first.map_or(0, |s| if *s == 0 { 3 } else { 2 })
                        | if first.is_some() && busy != 0 { 4 } else { 0 };
                    let clock = if first == Some(&1) {
                        native_voice_clock(samples, rate)
                    } else {
                        0
                    };
                    let locks = first.map_or(0, |s| if *s == 0 { 2 } else { 4 });
                    expected.push(vec![status, (clock >> 32) as u32, clock as u32, locks]);
                    clock_cases += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MUSIC_COMMANDS").unwrap())
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
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "music row{index}");
    }
    println!(
        "Higurashi music: {play_cases} native PLAY cases, {setters} STOP/VOL cases,88 WAIT masks,{sync_cases} SYNC deadlines and{clock_cases} actual native BGM status/sample-clock cases match; skip fades, raw loop counts and signed arithmetic pass; title/loader are checked contracts"
    );
}
