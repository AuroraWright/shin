use super::*;
use shin_core::{format::font::read_font_metrics, layout::HigurashiMessageLayouter};
use std::{
    io::{BufReader, Write},
    process::{Command as Process, Stdio},
};

#[derive(Default)]
struct Voice {
    playing: bool,
    allowed: bool,
    position: u64,
    plays: u32,
    stops: u32,
    last: [u32; 8],
}
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        self.playing
    }
    fn position_micros(&self) -> u64 {
        self.position
    }
    fn play(
        &mut self,
        filename: &str,
        volume: f32,
        lipsync: bool,
        start: u32,
        duration: u32,
    ) -> bool {
        self.plays += 1;
        self.playing = self.allowed;
        let hash = filename
            .as_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            });
        let start = u64::from(start) * 1000;
        let duration = u64::from(duration) * 1000;
        self.last = [
            hash as u32,
            (hash >> 32) as u32,
            start as u32,
            (start >> 32) as u32,
            duration as u32,
            (duration >> 32) as u32,
            if lipsync { 3 } else { 2 },
            volume.to_bits(),
        ];
        self.allowed
    }
    fn stop(&mut self) {
        self.stops += 1;
        self.playing = false;
    }
}

fn row(
    op: char,
    flags: u32,
    style: i32,
    alignment: i32,
    argument: i32,
    animate: bool,
    skip: u8,
    keep: bool,
    text: &str,
) -> String {
    let hex = text
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{op} {flags} {style} {alignment} -1 2 {argument} 123 {} 50 {skip} {} {hex}",
        u8::from(animate),
        u8::from(keep)
    )
}

#[test]
#[ignore = "executes actual updated message install/update/click/skip/wait callbacks with supplied font"]
fn higurashi_native_message_playback_install_reveal_wait_and_input() {
    let font = read_font_metrics(&mut BufReader::new(
        std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
    ))
    .unwrap();
    let texts = [
        "@r本文",
        "名前@r@bかな.@<漢字@>、@k続く@|節@y同期@r次",
        "@r@[即時@]次@k待つ@e後",
        "@r@vfoo.声@x20.次@/後@y同期@|節",
        "@r@w200.待つ@t並列@w300.後",
        "@r一行@r二行@r三行@r四行@r五行@r六行@r七行@r八行",
        "@r",
        "@r……もやがかったような意識のすぐそばで、いびき交じりの寝息が聞こえてくる。",
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut sequences = 0;
    for text in texts {
        for style in 0..5 {
            for alignment in 0..4 {
                for flags in [0, 2] {
                    for animate in [false, true] {
                        for skip_speed in [0, 100] {
                            sequences += 1;
                            let mut owner = HigurashiMessagePlayback::default();
                            let mut voice = Voice {
                                allowed: true,
                                ..Default::default()
                            };
                            let mut settings = MessageSettings {
                                skip_speed,
                                continue_voice_on_click: alignment & 1 != 0,
                                ..Default::default()
                            };
                            let mut ops = vec![
                                row(
                                    'N',
                                    flags,
                                    style,
                                    alignment,
                                    -1,
                                    animate,
                                    skip_speed,
                                    settings.continue_voice_on_click,
                                    text,
                                ),
                                "W -1".into(),
                                "I".into(),
                                "C".into(),
                                "H 1".into(),
                                "U 3f800000 1 1 0 1 100 0".into(),
                                "C".into(),
                                "F".into(),
                                "H 0".into(),
                                "U 3f000000 0 0 0 1 100 0".into(),
                            ];
                            for frame in 0..32 {
                                let delta = [0.0f32, 0.5, 1.0, 2.0, 5.0, 10.0][frame % 6];
                                ops.push(format!(
                                    "U {:x} {} {} {} {} {} {}",
                                    delta.to_bits(),
                                    u8::from(frame % 3 == 0),
                                    u8::from(frame % 11 == 6),
                                    u8::from(frame % 8 < 3),
                                    u8::from(frame % 7 != 0),
                                    if frame % 9 == 0 { 0 } else { 100 },
                                    frame * 1000
                                ));
                                if frame % 7 == 0 {
                                    ops.push("C".into());
                                }
                                if frame % 6 == 0 {
                                    ops.push("S".into());
                                    ops.push("W 0".into());
                                    ops.push("V 0 -1".into());
                                    ops.push("V 0 20".into());
                                }
                            }
                            ops.extend([
                                "F".into(),
                                "W -1".into(),
                                "S".into(),
                                "F".into(),
                                row(
                                    'M',
                                    flags,
                                    4,
                                    alignment,
                                    27,
                                    true,
                                    skip_speed,
                                    settings.continue_voice_on_click,
                                    "@r次のページ",
                                ),
                                "U 40a00000 0 0 0 1 100 0".into(),
                                row(
                                    'M',
                                    flags,
                                    0,
                                    alignment,
                                    i32::MAX,
                                    true,
                                    skip_speed,
                                    settings.continue_voice_on_click,
                                    "@r最終ページ",
                                ),
                                "U 3f800000 0 0 0 1 100 0".into(),
                                "X 1".into(),
                                "W -1".into(),
                                "U 40a00000 0 0 0 1 100 0".into(),
                                "F".into(),
                                "X 0".into(),
                            ]);
                            for (step, op) in ops.into_iter().enumerate() {
                                let words: Vec<_> = op.split_whitespace().collect();
                                let integer = |i: usize| words[i].parse::<i32>().unwrap();
                                let bit = |i: usize| integer(i) != 0;
                                let result = match words[0] {
                                    "N" | "M" => {
                                        if words[0] == "N" {
                                            owner = Default::default();
                                            voice = Voice {
                                                allowed: true,
                                                ..Default::default()
                                            };
                                        }
                                        let mode = integer(2);
                                        let align = integer(3);
                                        owner.install(
                                            mode | (align << 4),
                                            [integer(4), integer(5)],
                                            integer(1) as u32,
                                            integer(6),
                                            integer(7) as u32,
                                            bit(8),
                                        );
                                        let bytes = words[12]
                                            .as_bytes()
                                            .chunks_exact(2)
                                            .map(|h| {
                                                u8::from_str_radix(
                                                    std::str::from_utf8(h).unwrap(),
                                                    16,
                                                )
                                                .unwrap()
                                            })
                                            .collect::<Vec<_>>();
                                        let text = String::from_utf8(bytes).unwrap();
                                        let (commands, lines, _) =
                                            HigurashiMessageLayouter::for_message(
                                                &font,
                                                mode | (align << 4),
                                                integer(9),
                                            )
                                            .parse(&text);
                                        owner.set_layout(&commands, &lines);
                                        settings.voice_volume = 100;
                                        settings.skip_speed = integer(10) as u8;
                                        settings.continue_voice_on_click = bit(11);
                                        0
                                    }
                                    "U" => {
                                        voice.playing = bit(4);
                                        voice.allowed = bit(5);
                                        voice.position = words[7].parse().unwrap();
                                        settings.voice_volume = integer(6) as u8;
                                        owner.advance(
                                            f32::from_bits(
                                                u32::from_str_radix(words[1], 16).unwrap(),
                                            ),
                                            bit(2),
                                            bit(3),
                                            settings,
                                            &mut voice,
                                        );
                                        0
                                    }
                                    "C" => u32::from(owner.click(settings, &mut voice)),
                                    "F" => {
                                        owner.fast_forward(&mut voice);
                                        0
                                    }
                                    "H" => {
                                        owner.hidden = bit(1);
                                        0
                                    }
                                    "I" => {
                                        owner.block_input();
                                        0
                                    }
                                    "S" => {
                                        owner.send_sync();
                                        0
                                    }
                                    "W" => u32::from(owner.waiting(integer(1))),
                                    "V" => u32::from(owner.voice_waiting(
                                        integer(1),
                                        integer(2),
                                        &voice,
                                    )),
                                    "X" => {
                                        owner.close(bit(1));
                                        0
                                    }
                                    _ => unreachable!(),
                                };
                                input.push_str(&op);
                                input.push('\n');
                                let mut snapshot = vec![result];
                                snapshot.extend(owner.snapshot());
                                snapshot.extend([
                                    voice.plays,
                                    voice.stops,
                                    owner.completed_messages.len() as u32,
                                ]);
                                snapshot.extend(voice.last);
                                expected.push((sequences, step, op, snapshot));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_MESSAGE_PLAYBACK").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("balanced ownership"));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), expected.len());
    let observations = expected.len();
    for (line, (sequence, step, op, wanted)) in stdout.lines().zip(expected) {
        let actual = line
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            wanted,
            "sequence{sequence} style{style_dummy} step{step}: {op}",
            style_dummy = "see install row"
        );
    }
    println!(
        "Higurashi message playback:{sequences} native sequences/{observations} observations match install, reveal, waits, syncs, voice feedback, clicks, modal/slide gates, fast-forward and close"
    );
}
