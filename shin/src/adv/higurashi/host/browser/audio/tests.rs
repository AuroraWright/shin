use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn text_words(out: &mut Vec<u32>, text: &str) {
    out.push(text.len() as u32);
    out.extend(text.bytes().map(u32::from));
}
fn words(
    sequence: &Sequence,
    result: bool,
    refs: u32,
    stops: u32,
    ticks: u32,
    played: Option<VoiceFile>,
) -> Vec<u32> {
    let mut out = vec![
        u32::from(result),
        u32::from(sequence.active),
        sequence.next as u32,
        sequence.delay as u32,
        sequence.files.len() as u32,
        refs,
        stops,
        u32::from(played.is_some()),
        ticks,
        played.as_ref().map_or(0, |file| file.volume.to_bits()),
    ];
    text_words(&mut out, played.as_ref().map_or("", |file| &file.path));
    for file in &sequence.files {
        out.push(file.volume.to_bits());
        text_words(&mut out, &file.path);
    }
    out
}
fn prepare(
    protocol: &mut String,
    expected: &mut Vec<Vec<u32>>,
    sequence: &mut Sequence,
    refs: &mut u32,
    text: &str,
) {
    let hex = if text.is_empty() {
        "-".into()
    } else {
        text.bytes().map(|b| format!("{b:02x}")).collect::<String>()
    };
    protocol.push_str(&format!("P {hex}\n"));
    let result = sequence.prepare(files(text));
    if result {
        *refs = 1;
    }
    expected.push(words(sequence, result, *refs, u32::from(result), 0, None));
}
fn tick(
    protocol: &mut String,
    expected: &mut Vec<Vec<u32>>,
    sequence: &mut Sequence,
    refs: u32,
    status: u32,
) {
    protocol.push_str(&format!("U {status}\n"));
    let playing = status & 2 != 0;
    let played = sequence.frame(playing);
    expected.push(words(sequence, playing, refs, 0, 1, played));
}
#[test]
#[ignore = "executes original updated replay parser/replacement/stop/sequence/status with checked audio SDK boundary"]
fn higurashi_browser_audio_matches_native_sequence_and_cue_names() {
    let texts = [
        "",
        "本文",
        "@r@vfirst.",
        "@vfirst|second.",
        "@v.",
        "@o0.@vzero.@o25.@vquarter.@o100.@vfull.",
        "@o101.@vclamped.",
        "@o-1.@vminus.",
        "@vfirst.@vsecond.@vthird.",
        "@r本文@o99.@vfolder/name.@o1.@vafter.",
        "@vlong-voice-name-with-more-than-23-bytes.",
        "@vfirst.@x10.@o75.@vsecond.",
    ];
    let mut protocol = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for text in texts {
        for status in [0, 1, 2, 3, 4, 16, 31] {
            protocol.push_str("N\n");
            let mut sequence = Sequence::default();
            let mut refs = 0;
            prepare(&mut protocol, &mut expected, &mut sequence, &mut refs, text);
            for frame in 0..24 {
                tick(
                    &mut protocol,
                    &mut expected,
                    &mut sequence,
                    refs,
                    if frame < 6 { status } else { 0 },
                );
            }
            // No tags preserve both the current resource and the partially consumed queue.
            prepare(
                &mut protocol,
                &mut expected,
                &mut sequence,
                &mut refs,
                "@r本文",
            );
            for _ in 0..12 {
                tick(&mut protocol, &mut expected, &mut sequence, refs, 0);
            }
            prepare(
                &mut protocol,
                &mut expected,
                &mut sequence,
                &mut refs,
                "@vnew.@o30.@vlast.",
            );
            for _ in 0..9 {
                tick(&mut protocol, &mut expected, &mut sequence, refs, 0);
            }
            prepare(
                &mut protocol,
                &mut expected,
                &mut sequence,
                &mut refs,
                "@vreplacement.",
            );
            for _ in 0..12 {
                tick(&mut protocol, &mut expected, &mut sequence, refs, 0);
            }
            protocol.push_str("S\n");
            sequence.stop();
            expected.push(words(&sequence, false, refs, 1, 0, None));
            for _ in 0..3 {
                tick(&mut protocol, &mut expected, &mut sequence, refs, 0);
            }
            protocol.push_str("D\n");
            owners += 1;
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_AUDIO").unwrap())
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
    let data = protocol.clone();
    let writer = std::thread::spawn(move || input.write_all(data.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (i, (line, words)) in lines.iter().zip(&expected).enumerate() {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("V"));
        let native: Vec<u32> = tokens.map(|v| v.parse().unwrap()).collect();
        assert_eq!(&native, words, "replay observation{i}");
    }
    // Follow the original two relocation indirections, as in shared Umineko cue proofs.
    let exe = std::fs::read("game-data/higurashi/uncompressed_executable_update").unwrap();
    let raw = &exe[256..];
    let relocation = |address: u64| {
        raw[0xee040..0xee040 + 295920]
            .chunks_exact(24)
            .find(|e| u64::from_le_bytes(e[..8].try_into().unwrap()) == address)
            .map(|e| u64::from_le_bytes(e[16..24].try_into().unwrap()))
            .unwrap()
    };
    for (address, name) in [
        (0x1cc1e0, "page"),
        (0x1cc1d8, "error"),
        (0x1cc258, "move"),
        (0x1cc260, "cancel"),
        (0x1cc1b8, "decide"),
    ] {
        let at = relocation(relocation(address)) as usize;
        let end = raw[at..].iter().position(|b| *b == 0).unwrap() + at;
        assert_eq!(std::str::from_utf8(&raw[at..end]).unwrap(), name);
    }
    let dir = std::env::var("SHIN_HIGURASHI_BROWSER_AUDIO_CAPTURE").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    for (name, data) in [
        ("input.txt", protocol.as_bytes()),
        ("native.txt", actual.as_bytes()),
        ("native.log", output.stderr.as_slice()),
    ] {
        std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
    }
    eprintln!(
        "Browser audio:{owners} native owners/{} observations/{} exact words;five relocated cue names;original ten-tick/parser/stop/replacement/status pass",
        expected.len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
}
