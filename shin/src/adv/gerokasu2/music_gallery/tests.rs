use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn catalog(count: usize) -> Vec<CatalogItem> {
    (0..count)
        .map(|i| CatalogItem {
            bgm: i as u16,
            name: i as u16 + 1,
            once: match i % 3 {
                0 => 0,
                1 => 1,
                _ => 17,
            },
        })
        .collect()
}
fn owner(count: usize, pattern: u32) -> Controller {
    let mut bits = vec![0u32; count.div_ceil(32)];
    for i in 0..count {
        if pattern == 1 || (pattern == 2 && i % 3 == 1) {
            bits[i / 32] |= 1 << (i % 32);
        }
    }
    Controller::new(&catalog(count), &bits).unwrap()
}
#[test]
fn ger_music_locked_rows_full_counts_and_native_return_audio_contract() {
    let mut m = owner(3, 2);
    assert_eq!(m.rows.iter().map(|r| r.name).collect::<Vec<_>>(), [0, 2, 0]);
    m.assets_ready();
    m.update(60., false, false, Input::default(), false);
    assert_eq!(m.take_actions(), [Action::Stop(60)]);
    m.update(0., true, false, Input::default(), false);
    assert!(m.accepts_input());
    m.update(
        0.,
        true,
        false,
        Input {
            confirm: true,
            ..Default::default()
        },
        false,
    );
    assert_eq!(m.take_actions(), [Action::Cue("error")]);
    assert_eq!(m.playing, -1);
    m.selected = 1;
    m.play();
    assert_eq!(m.take_actions(), [Action::Play { bgm: 1, once: 1 }]);
    m.update(
        0.,
        true,
        false,
        Input {
            cancel: true,
            confirm: true,
            stop: true,
            ..Default::default()
        },
        true,
    );
    assert_eq!(m.take_actions(), [Action::Cue("cancel")]);
    m.update(7., true, false, Input::default(), true);
    assert!(m.take_actions().is_empty());
    m.update(0., true, false, Input::default(), true);
    assert_eq!(m.take_actions(), [Action::ReturnTitle { requested_id: 9 }]);
    assert_eq!(m.phase, Phase::Closed);
    let selected = m.selected;
    let playing = m.playing;
    m.recover_return();
    assert!(m.accepts_input());
    assert!(!m.closing);
    assert_eq!((m.selected, m.playing), (selected, playing));
    assert_eq!(m.flash(), 0.);
    let all = owner(3, 1);
    assert_eq!(all.actions, [Action::Achievement(14)]);
    assert!(Controller::new(&[], &[]).is_err());
}
fn root() -> PathBuf {
    std::env::var_os("SHIN_GEROKASU2_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap())
}
fn native(kind: &str, input: String) -> Vec<String> {
    let root = root();
    let binary = std::env::var_os(format!("SHIN_GEROKASU2_MUSIC_{}", kind.to_uppercase()))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(format!("gerokasu2/.local/music-gallery/native-{kind}")));
    let mut child = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "native {kind}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn snapshot(m: &mut Controller, result: i32) -> Vec<i64> {
    let mut cues = [0i64; 4];
    let (mut play, mut once, mut stop, mut ret) = (0, 0, 0, 0);
    for action in m.take_actions() {
        match action {
            Action::Cue(name) => {
                cues[["move", "decide", "cancel", "error"]
                    .iter()
                    .position(|v| *v == name)
                    .unwrap()] += 1
            }
            Action::Play { bgm, once: n } => {
                play = i64::from(bgm) + 1;
                once = i64::from(n);
            }
            Action::Stop(n) => stop = i64::from(n),
            Action::ReturnTitle { requested_id } => {
                assert_eq!(requested_id, 9);
                ret += 1;
            }
            Action::Achievement(14) => {}
            _ => panic!("unexpected action"),
        }
    }
    let mut values = vec![
        i64::from(result),
        i64::from(m.document_height),
        i64::from(m.selected),
        i64::from(m.playing),
        i64::from(m.closing),
        i64::from(m.region),
        i64::from(m.hover),
        i64::from(m.velocity.to_bits()),
    ];
    for tween in [&m.scroll, &m.close_flash] {
        values.extend([i64::from(tween.value().to_bits()), i64::from(tween.busy())]);
    }
    values.push(i64::from(m.disabled));
    m.disabled = 0;
    values.extend(cues);
    values.extend([
        play,
        once,
        stop,
        ret,
        i64::from(m.rows.iter().all(|r| r.unlocked)),
        m.rows.len() as i64,
    ]);
    for row in &m.rows {
        values.extend([
            i64::from(row.name),
            i64::from(row.unlocked),
            i64::from(row.flash.value().to_bits()),
            i64::from(row.flash.busy()),
        ]);
    }
    values
}
fn push(
    script: &mut String,
    expected: &mut Vec<Vec<i64>>,
    m: &mut Controller,
    command: String,
    result: i32,
) {
    script.push_str(&command);
    script.push('\n');
    expected.push(snapshot(m, result));
}
#[test]
#[ignore = "executes original supplied Ger BGMMode constructor/controller/pointer/tweens/audio/return; CPU only"]
fn ger_native_music_catalog_controller_pointer_clocks_and_return() {
    let (mut script, mut expected, mut profiles) = (String::new(), vec![], 0);
    for count in [1, 3, 6, 17, 24, 45, 64] {
        for pattern in 0..3 {
            let mut m = owner(count, pattern);
            profiles += 1;
            push(
                &mut script,
                &mut expected,
                &mut m,
                format!("N {count} {pattern}"),
                -1,
            );
            for permission in [false, true] {
                for captured in [false, true] {
                    for (pressed, repeat) in [
                        (0, 1 << 27),
                        (0, 1 << 25),
                        (0, 1 << 11),
                        (1 << 28, 0),
                        (1 << 7, 0),
                        (0, (1 << 26) | (1 << 27)),
                        (0, (1 << 24) | (1 << 25)),
                        (0, 1 << 10),
                    ] {
                        let input = Input {
                            confirm: pressed & (1 << 28) != 0,
                            stop: pressed & (1 << 7) != 0,
                            left: repeat & (1 << 26) != 0,
                            right: repeat & (1 << 27) != 0,
                            up: repeat & (1 << 24) != 0,
                            down: repeat & (1 << 25) != 0,
                            previous: repeat & (1 << 10) != 0,
                            next: repeat & (1 << 11) != 0,
                            ..Default::default()
                        };
                        let result = i32::from(m.poll_controls(permission, captured, input));
                        push(
                            &mut script,
                            &mut expected,
                            &mut m,
                            format!(
                                "U {} {} {pressed:x} {repeat:x}",
                                u8::from(permission),
                                u8::from(captured)
                            ),
                            result,
                        );
                        m.advance_clocks(4., true);
                        push(&mut script, &mut expected, &mut m, "F 4 2".into(), -1);
                    }
                }
            }
            for (x, y) in [
                (204., 182.),
                (915.99, 269.99),
                (916., 200.),
                (1004., 282.),
                (1716., 282.),
                (160., 176.),
                (200., 175.99),
                (1766., 46.),
                (1874., 46.),
                (1766., 154.),
                (10., 1000.),
            ] {
                let result = i32::from(m.probe(x, y));
                push(
                    &mut script,
                    &mut expected,
                    &mut m,
                    format!("V {x} {y}"),
                    result,
                );
                m.press();
                push(&mut script, &mut expected, &mut m, "P".into(), -1);
                m.drag(220., -460.);
                push(
                    &mut script,
                    &mut expected,
                    &mut m,
                    "G 300 220 300 -460".into(),
                    -1,
                );
                m.gesture(-7.);
                push(&mut script, &mut expected, &mut m, "I 0 -7".into(), -1);
                for dt in [0., 0.25, 1., 2.5, 100.] {
                    m.advance_clocks(dt, false);
                    push(&mut script, &mut expected, &mut m, format!("F {dt} 0"), -1);
                }
                m.cancel();
                push(&mut script, &mut expected, &mut m, "C".into(), -1);
                m.reset();
                push(&mut script, &mut expected, &mut m, "Z".into(), -1);
            }
            for (x, y) in [(204., 182.), (1004., 182.), (1766., 46.)] {
                let result = i32::from(m.probe(x, y));
                push(
                    &mut script,
                    &mut expected,
                    &mut m,
                    format!("V {x} {y}"),
                    result,
                );
                m.press();
                push(&mut script, &mut expected, &mut m, "P".into(), -1);
                m.release();
                push(&mut script, &mut expected, &mut m, "E".into(), -1);
                if x == 1766. {
                    let result = i32::from(m.poll_close());
                    push(&mut script, &mut expected, &mut m, "T".into(), result);
                }
                m.advance_clocks(8., true);
                push(&mut script, &mut expected, &mut m, "F 8 2".into(), -1);
            }
            let result = i32::from(m.poll_close());
            push(&mut script, &mut expected, &mut m, "T".into(), result);
            script.push_str("X\n");
        }
    }
    let actual = native("model", script);
    let rows: Vec<_> = actual.iter().filter(|s| s.starts_with("M ")).collect();
    assert_eq!(rows.len(), expected.len());
    for (i, (line, wanted)) in rows.iter().zip(&expected).enumerate() {
        let got: Vec<i64> = line
            .split_whitespace()
            .skip(2)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(&got, wanted, "Music sample{i}: {line}");
    }
    println!(
        "Ger Music native catalog/controller/pointer/clocks/return: {profiles} profiles,{} exact snapshots",
        expected.len()
    );
}
fn vertex_packet(q: &artwork::Packet) -> Vec<u32> {
    let mut out = vec![
        if q.flash > 0. { 6 } else { 2 },
        q.blend,
        0,
        7,
        0,
        0,
        0,
        65535,
    ];
    out.extend(artwork::PROJECTION.map(f32::to_bits));
    out.extend([0u32; 4]);
    out.extend(if q.flash > 0. {
        [
            1f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            q.flash.to_bits(),
        ]
    } else {
        [0u32; 4]
    });
    out.push(24);
    for (x, y, u, v) in [
        (q.rect[0], q.rect[1], q.uv[0], q.uv[1]),
        (q.rect[2], q.rect[1], q.uv[2], q.uv[1]),
        (q.rect[0], q.rect[3], q.uv[0], q.uv[3]),
        (q.rect[2], q.rect[3], q.uv[2], q.uv[3]),
    ] {
        out.extend([
            x.to_bits(),
            y.to_bits(),
            0,
            q.color,
            u.to_bits(),
            v.to_bits(),
        ]);
    }
    out
}
#[test]
#[ignore = "executes original supplied Ger BGMMode complete row/chrome/hint renderer; CPU only"]
fn ger_native_music_row_chrome_hint_packets() {
    let (mut script, mut expected) = (String::new(), vec![]);
    let mut bars = vec![];
    for count in [1, 3, 17, 45, 64] {
        for pattern in 0..3 {
            for (scroll, elapsed, flash) in [
                (0., 0., 0.),
                (0., 1., 0.7),
                (16., 0.25, 0.2),
                (101.5, 2.5, 0.),
                (800., 20., 0.),
            ] {
                let mut m = owner(count, pattern);
                m.selected = (count - 1) as i32;
                m.playing = if pattern == 1 { 0 } else { -1 };
                m.scroll.snap(scroll);
                m.scrollbar_position = scroll as i32;
                m.elapsed = elapsed;
                m.close_flash.snap(flash);
                for swapped in [false, true] {
                    script.push_str(&format!(
                        "D {count} {pattern} {} {} {:x} {:x} {:x} {}\n",
                        m.selected,
                        m.playing,
                        scroll.to_bits(),
                        flash.to_bits(),
                        elapsed.to_bits(),
                        u8::from(swapped)
                    ));
                    expected.push(artwork::packets(&m, swapped));
                    bars.push(artwork::scrollbar(&m));
                }
            }
        }
    }
    let actual = native("render", script);
    let names: std::collections::HashMap<_, _> = actual
        .iter()
        .filter(|s| s.starts_with("T "))
        .map(|s| {
            let v: Vec<_> = s.split_whitespace().collect();
            (v[1].parse::<u32>().unwrap(), v[2])
        })
        .collect();
    let (mut profile, mut packet, mut count) = (0, 0, 0);
    let (mut bar_index, mut bar_count) = (0, 0);
    for line in &actual {
        if line.starts_with("D ") {
            profile = line[2..].parse::<usize>().unwrap();
            packet = 0;
            bar_index = 0;
        } else if line.starts_with("P ") {
            let mut got: Vec<u32> = line
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse().unwrap())
                .collect();
            let q = &expected[profile][packet];
            assert_eq!(
                names[&got[2]], q.texture,
                "Music texture profile{profile} packet{packet}"
            );
            got[2] = 0;
            assert_eq!(
                got,
                vertex_packet(q),
                "Music render profile{profile} packet{packet} texture{}",
                q.texture
            );
            packet += 1;
            count += 1;
        } else if line.starts_with("S ") {
            let got: Vec<u32> = line
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(names[&got[0]], "scroll");
            let bar = &bars[profile][bar_index];
            let mut wanted = bar.matrix.map(f32::to_bits).to_vec();
            for v in bar.vertices {
                wanted.extend(v.position.map(f32::to_bits));
                wanted.push(u32::MAX);
                wanted.extend(v.uv.map(f32::to_bits));
            }
            assert_eq!(
                got[1..],
                wanted,
                "Music scrollbar profile{profile} strip{bar_index}"
            );
            bar_index += 1;
            bar_count += 1;
        } else if line.starts_with("E ") {
            assert_eq!(
                packet,
                expected[profile].len(),
                "Music packet count profile{profile}"
            );
            assert_eq!(
                bar_index,
                bars[profile].len(),
                "Music scrollbar count profile{profile}"
            );
        }
    }
    println!(
        "Ger Music native artwork: {} profiles,{count} exact matrix/uniform/vertex/color packets plus{bar_count} exact scrollbar matrix/8-vertex strips",
        expected.len()
    );
}

#[test]
#[ignore = "executes original First37130 resource gate/capture clock/audio stop/input install; CPU only"]
fn ger_native_music_first_resource_gate_capture_clock_and_stop() {
    let (mut script, mut expected, mut profiles) = (String::new(), vec![], 0);
    for dt in [0., 0.25, 1., 2.5, 60., 1000.] {
        for source in [false, true] {
            for mask in [false, true] {
                script.push_str(&format!("{dt} {} {}\n", u8::from(source), u8::from(mask)));
                let mut m = owner(1, 0);
                for frame in 0..260 {
                    if frame == 2 {
                        m.assets_ready();
                    }
                    m.update(dt, true, false, Input::default(), false);
                    let opened = m.phase == Phase::Active;
                    let active = m.phase != Phase::Loading;
                    let stop = if m.take_actions().contains(&Action::Stop(60)) {
                        60
                    } else {
                        0
                    };
                    expected.push(vec![
                        profiles,
                        frame,
                        if opened {
                            -1
                        } else if active {
                            1
                        } else {
                            0
                        },
                        i64::from(active),
                        i64::from(m.opening_elapsed.to_bits()),
                        i64::from(if active { 60f32 } else { 1f32 }.to_bits()),
                        i64::from(if active { 0.3f32 } else { 0f32 }.to_bits()),
                        0,
                        stop,
                        i64::from(opened),
                    ]);
                    if opened || (dt == 0. && frame == 5) {
                        break;
                    }
                }
                profiles += 1;
            }
        }
    }
    let mut actual = native("opening", script);
    assert_eq!(actual.remove(0), format!("W {ATLAS} {MASK}"));
    assert_eq!(actual.len(), expected.len());
    for (line, wanted) in actual.iter().zip(&expected) {
        let got: Vec<i64> = line
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(&got, wanted, "Music First{line}");
    }
    println!(
        "Ger Music native First: {profiles} profiles,{} exact resource/capture/audio/input snapshots",
        expected.len()
    );
}
